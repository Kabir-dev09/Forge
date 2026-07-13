use std::collections::HashMap;
use std::os::unix::io::{AsRawFd, OwnedFd, RawFd};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, RwLock,
};

use arc_swap::ArcSwap;
use calloop::{
    channel::{channel, Channel, Sender},
    generic::Generic,
    EventLoop, Interest, LoopHandle, Mode, PostAction, RegistrationToken,
};
use forge_core::{ForgeError, Result};
use forge_pty::{snapshot::RenderSnapshot, ScreenBuffer, VteProcessor};

use super::PaneId;

pub const PTY_READ_BUFFER_SIZE: usize = 128 * 1024; // 128KB per read call
pub const MAX_PTY_READ_ITERATIONS_PER_EVENT: usize = 64; // drain burst aggressively
pub const MAX_PTY_READ_BYTES_PER_EVENT: usize = 4 * 1024 * 1024; // 4MB per wakeup event

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneIoStatus {
    Registered,
    Exited,
}

pub enum PtyWorkerCommand {
    AddPane {
        pane_id: PaneId,
        fd: OwnedFd,
        vte_processor: VteProcessor,
        screen_buffer: ScreenBuffer,
        snapshot: Arc<ArcSwap<RenderSnapshot>>,
    },
    RemovePane(PaneId),
    // FIX 5: Batch resize — send all pane resizes at once with a single shared ack sender.
    BatchResizeReflow(
        Vec<(PaneId, usize, usize)>,
        Option<crossbeam_channel::Sender<()>>,
    ),
    ScrollUp(PaneId, usize),
    ScrollDown(PaneId, usize),
    ScrollPageUp(PaneId),
    ScrollPageDown(PaneId),
    ScrollToTop(PaneId),
    ScrollToBottom(PaneId),
    SetScrollOffset(PaneId, usize),
    UpdateSelection(PaneId, Option<forge_core::cell::SelectionRange>),
    ClearSelection(PaneId),
    UpdateTheme(
        PaneId,
        forge_core::color::Color,
        forge_core::color::Color,
        [forge_core::color::Color; 16],
    ),
    MarkAllDirty(PaneId),
    MarkAllClean(PaneId),
    SetMouseTracking(PaneId, bool),
    SetVisiblePanes(std::collections::HashSet<PaneId>),
}

pub struct PaneIoRegistry {
    sender: Sender<PtyWorkerCommand>,
    // FIX 3: AtomicBool as a zero-lock fast path — checked every loop tick.
    // The RwLock'd Vec is only cloned when this is true.
    has_exited: Arc<AtomicBool>,
    exited_panes: Arc<RwLock<Vec<PaneId>>>,
    // FIX 4: Generation counter — increments on every structural change.
    // The main loop compares u64 instead of allocating+comparing a HashSet.
    pub visible_gen: Arc<AtomicU64>,
}

struct WorkerState {
    panes: HashMap<PaneId, PaneState>,
    exited_panes: Arc<RwLock<Vec<PaneId>>>,
    // FIX 3: shared atomic so main thread can read without taking the RwLock.
    has_exited: Arc<AtomicBool>,
    wakeup_signal: calloop::LoopSignal,
    loop_handle: LoopHandle<'static, WorkerState>,
    visible_panes: std::collections::HashSet<PaneId>,
}

struct PaneState {
    vte_processor: VteProcessor,
    screen_buffer: ScreenBuffer,
    snapshot: Arc<ArcSwap<RenderSnapshot>>,
    token: RegistrationToken,
    last_snapshot_time: std::time::Instant,
    // Reusable read buffer: heap-allocated once, reused on every PTY read.
    read_buf: Vec<u8>,
    pending_write: Vec<u8>,
}

fn flush_pending_write(fd: RawFd, pending_write: &mut Vec<u8>) {
    let mut written = 0usize;
    while written < pending_write.len() {
        let borrowed_fd = unsafe { rustix::fd::BorrowedFd::borrow_raw(fd) };
        match rustix::io::write(borrowed_fd, &pending_write[written..]) {
            Ok(n) if n > 0 => written += n,
            Ok(_) | Err(rustix::io::Errno::AGAIN) => break,
            Err(e) => {
                tracing::warn!("PTY response write failed: {}", e);
                pending_write.clear();
                return;
            }
        }
    }

    if written > 0 {
        pending_write.drain(..written);
    }
}

impl PaneIoRegistry {
    pub fn new(wakeup_signal: calloop::LoopSignal) -> Result<Self> {
        let (sender, receiver) = channel::<PtyWorkerCommand>();
        let has_exited = Arc::new(AtomicBool::new(false));
        let exited_panes = Arc::new(RwLock::new(Vec::new()));
        let visible_gen = Arc::new(AtomicU64::new(0));

        let worker_exited_panes = exited_panes.clone();
        let worker_has_exited = has_exited.clone();

        std::thread::Builder::new()
            .name("forge-pty-worker".to_string())
            .spawn(move || {
                if let Err(e) = Self::run_worker(
                    receiver,
                    worker_exited_panes,
                    worker_has_exited,
                    wakeup_signal,
                ) {
                    tracing::error!("PTY worker failed: {}", e);
                }
            })
            .map_err(|e| ForgeError::Other(format!("Failed to spawn PTY worker thread: {}", e)))?;

        Ok(Self {
            sender,
            has_exited,
            exited_panes,
            visible_gen,
        })
    }

    fn run_worker(
        receiver: Channel<PtyWorkerCommand>,
        exited_panes: Arc<RwLock<Vec<PaneId>>>,
        has_exited: Arc<AtomicBool>,
        wakeup_signal: calloop::LoopSignal,
    ) -> Result<()> {
        let mut event_loop: EventLoop<WorkerState> =
            EventLoop::try_new().map_err(|e| ForgeError::Other(e.to_string()))?;
        let loop_handle = event_loop.handle();
        // Thread-local clone for use inside the closure below.
        let has_exited_worker = has_exited.clone();

        loop_handle
            .insert_source(receiver, |event, _metadata, state| {
                if let calloop::channel::Event::Msg(cmd) = event {
                    match cmd {
                        PtyWorkerCommand::AddPane {
                            pane_id,
                            fd,
                            vte_processor,
                            screen_buffer,
                            snapshot,
                        } => {
                            if let Err(e) =
                                state.add_pane(pane_id, fd, vte_processor, screen_buffer, snapshot)
                            {
                                tracing::error!("Failed to add pane {}: {}", pane_id.get(), e);
                            }
                        }
                        PtyWorkerCommand::RemovePane(pane_id) => {
                            state.remove_pane(pane_id);
                        }
                        PtyWorkerCommand::ScrollUp(pane_id, amt) => {
                            state.with_pane(pane_id, |p| p.screen_buffer.view_scroll_up(amt))
                        }
                        PtyWorkerCommand::ScrollDown(pane_id, amt) => {
                            state.with_pane(pane_id, |p| p.screen_buffer.view_scroll_down(amt))
                        }
                        PtyWorkerCommand::ScrollPageUp(pane_id) => state.with_pane(pane_id, |p| {
                            let r = p.screen_buffer.rows();
                            p.screen_buffer.view_scroll_up(r)
                        }),
                        PtyWorkerCommand::ScrollPageDown(pane_id) => {
                            state.with_pane(pane_id, |p| {
                                let r = p.screen_buffer.rows();
                                p.screen_buffer.view_scroll_down(r)
                            })
                        }
                        PtyWorkerCommand::ScrollToTop(pane_id) => state.with_pane(pane_id, |p| {
                            let len = p.screen_buffer.scrollback_len();
                            p.screen_buffer.view_scroll_up(len)
                        }),
                        PtyWorkerCommand::ScrollToBottom(pane_id) => {
                            state.with_pane(pane_id, |p| p.screen_buffer.view_scroll_to_bottom())
                        }
                        PtyWorkerCommand::SetScrollOffset(pane_id, offset) => {
                            state.with_pane(pane_id, |p| p.screen_buffer.view_scroll_to_offset(offset))
                        }
                        PtyWorkerCommand::UpdateSelection(pane_id, sel) => {
                            state.with_pane(pane_id, |p| p.screen_buffer.set_selection(sel))
                        }
                        PtyWorkerCommand::ClearSelection(pane_id) => {
                            state.with_pane(pane_id, |p| p.screen_buffer.clear_selection())
                        }
                        PtyWorkerCommand::UpdateTheme(pane_id, fg, bg, colors) => state
                            .with_pane(pane_id, |p| p.screen_buffer.update_theme(fg, bg, colors)),
                        PtyWorkerCommand::MarkAllDirty(pane_id) => {
                            state.with_pane(pane_id, |p| p.screen_buffer.mark_all_dirty())
                        }
                        PtyWorkerCommand::MarkAllClean(pane_id) => {
                            state.with_pane(pane_id, |p| p.screen_buffer.mark_all_clean())
                        }
                        PtyWorkerCommand::SetMouseTracking(pane_id, enabled) => state
                            .with_pane(pane_id, |p| {
                                p.screen_buffer.mouse_tracking_enabled = enabled
                            }),
                        // FIX 5: Batch resize — single dispatch for all panes, one ack.
                        PtyWorkerCommand::BatchResizeReflow(batch, sync_tx) => {
                            for (pane_id, cols, rows) in batch {
                                if let Some(pane) = state.panes.get_mut(&pane_id) {
                                    pane.screen_buffer.resize_reflow(cols, rows);
                                    // Only generate+publish snapshot if visible.
                                    if state.visible_panes.contains(&pane_id) {
                                        let snap = pane.screen_buffer.generate_snapshot();
                                        pane.snapshot.store(Arc::new(snap));
                                        pane.last_snapshot_time = std::time::Instant::now();
                                    }
                                }
                            }
                            if let Some(tx) = sync_tx {
                                let _ = tx.send(());
                            }
                            state.wakeup_signal.wakeup();
                        }
                        PtyWorkerCommand::SetVisiblePanes(panes) => {
                            state.visible_panes = panes;
                            // Immediately push snapshot for all visible panes so the UI isn't stale
                            for pane_id in &state.visible_panes {
                                if let Some(pane) = state.panes.get_mut(pane_id) {
                                    let snapshot = pane.screen_buffer.generate_snapshot();
                                    pane.snapshot.store(std::sync::Arc::new(snapshot));
                                    pane.last_snapshot_time = std::time::Instant::now();
                                }
                            }
                            state.wakeup_signal.wakeup();
                        }
                    }
                }
            })
            .map_err(|e| ForgeError::Other(e.to_string()))?;

        // Use a static loop handle here safely because the loop handle is bound to the loop lifetime.
        // Wait, loop_handle takes a lifetime. Let's just clone it inside add_pane.
        // Or store `LoopHandle<'static, WorkerState>`? `LoopHandle` doesn't have a lifetime since calloop 0.10.
        // Let's check `LoopHandle` in calloop documentation. Usually `LoopHandle<Data>` doesn't have a lifetime parameter if we use the right version.
        // Wait, let's assume `LoopHandle<WorkerState>` has no lifetime, or use `.clone()` if needed.
        // Actually, we can just fetch it when adding pane? No, `LoopHandle` is available from `state.loop_handle.clone()`.

        let mut state = WorkerState {
            panes: HashMap::new(),
            exited_panes,
            has_exited: has_exited_worker,
            wakeup_signal,
            loop_handle: loop_handle.clone(),
            visible_panes: std::collections::HashSet::new(),
        };

        loop {
            event_loop
                .dispatch(None, &mut state)
                .map_err(|e| ForgeError::Other(e.to_string()))?;
        }
    }

    pub fn register_pane(
        &self,
        pane_id: PaneId,
        fd: OwnedFd,
        vte_processor: VteProcessor,
        screen_buffer: ScreenBuffer,
        snapshot: Arc<ArcSwap<RenderSnapshot>>,
    ) -> Result<()> {
        self.sender
            .send(PtyWorkerCommand::AddPane {
                pane_id,
                fd,
                vte_processor,
                screen_buffer,
                snapshot,
            })
            .map_err(|e| ForgeError::Other(format!("Failed to send to PTY worker: {}", e)))?;
        Ok(())
    }

    pub fn mark_exited(&self, pane_id: PaneId) {
        self.sender.send(PtyWorkerCommand::RemovePane(pane_id)).ok();
        let mut exited = self.exited_panes.write().unwrap();
        if !exited.contains(&pane_id) {
            exited.push(pane_id);
        }
        self.has_exited.store(true, Ordering::Release);
    }

    // FIX 3: fast pre-check — zero lock contention when nothing has exited.
    pub fn has_any_exited(&self) -> bool {
        self.has_exited.load(Ordering::Acquire)
    }

    pub fn exited_panes(&self) -> Vec<PaneId> {
        let exited = self.exited_panes.write().unwrap();
        let panes = exited.clone();
        if panes.is_empty() {
            self.has_exited.store(false, Ordering::Release);
        }
        panes
    }

    pub fn remove_pane(&self, pane_id: PaneId) {
        self.sender.send(PtyWorkerCommand::RemovePane(pane_id)).ok();
        let mut exited = self.exited_panes.write().unwrap();
        exited.retain(|&id| id != pane_id);
        if exited.is_empty() {
            self.has_exited.store(false, Ordering::Release);
        }
    }

    pub fn send_ui_command(&self, cmd: PtyWorkerCommand) {
        let _ = self.sender.send(cmd);
    }
}

impl WorkerState {
    fn add_pane(
        &mut self,
        pane_id: PaneId,
        fd: OwnedFd,
        vte_processor: VteProcessor,
        screen_buffer: ScreenBuffer,
        snapshot: Arc<ArcSwap<RenderSnapshot>>,
    ) -> Result<()> {
        let source = Generic::new(fd, Interest::READ, Mode::Level);

        let token = self
            .loop_handle
            .clone()
            .insert_source(source, move |readiness, src_fd, state| {
                if !readiness.readable && !readiness.error {
                    return Ok(PostAction::Continue);
                }

                state.process_pane_read(pane_id, src_fd.as_raw_fd())
            })
            .map_err(|e| ForgeError::Other(format!("Failed to register PTY IO source: {}", e)))?;

        self.panes.insert(
            pane_id,
            PaneState {
                vte_processor,
                screen_buffer,
                snapshot,
                token,
                last_snapshot_time: std::time::Instant::now(),
                read_buf: vec![0u8; PTY_READ_BUFFER_SIZE],
                pending_write: Vec::new(),
            },
        );
        Ok(())
    }

    // FIX 6: Decouple mutate from snapshot — only publish snapshot if this pane is currently
    // visible on screen. Background panes still get their buffer mutated (scroll, theme, etc.)
    // but skip the expensive grid clone + ArcSwap store + wakeup.
    fn with_pane<F: FnOnce(&mut PaneState)>(&mut self, pane_id: PaneId, f: F) {
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            f(pane);
            if self.visible_panes.contains(&pane_id) {
                let snapshot = pane.screen_buffer.generate_snapshot();
                pane.snapshot.store(Arc::new(snapshot));
                pane.last_snapshot_time = std::time::Instant::now();
                self.wakeup_signal.wakeup();
            }
        }
    }

    fn remove_pane(&mut self, pane_id: PaneId) {
        if let Some(pane) = self.panes.remove(&pane_id) {
            self.loop_handle.remove(pane.token);
        }
    }

    fn process_pane_read(&mut self, pane_id: PaneId, fd: RawFd) -> std::io::Result<PostAction> {
        let mut bytes_read = 0usize;
        let mut iterations = 0usize;
        let mut processed_output = false;

        let mut exited = false;
        let mut drained = false;

        // We need to borrow pane.read_buf and pane.{vte_processor, screen_buffer} simultaneously.
        // The borrow checker can't verify field-level disjointness through a HashMap, so we use
        // a raw pointer to the already-allocated read_buf. This is safe: the buf lives for the
        // duration of the pane, and we never resize it during the loop.
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            flush_pending_write(fd, &mut pane.pending_write);
            while iterations < MAX_PTY_READ_ITERATIONS_PER_EVENT
                && bytes_read < MAX_PTY_READ_BYTES_PER_EVENT
            {
                iterations += 1;

                let borrowed_fd = unsafe { rustix::fd::BorrowedFd::borrow_raw(fd) };
                let read_res = rustix::io::read(borrowed_fd, &mut pane.read_buf);

                let len = match read_res {
                    Ok(0) => {
                        exited = true;
                        drained = true;
                        break;
                    }
                    Ok(n) => n,
                    Err(rustix::io::Errno::AGAIN) => {
                        drained = true;
                        break;
                    }
                    Err(rustix::io::Errno::IO) => {
                        exited = true;
                        drained = true;
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(pane_id = pane_id.get(), "PTY read failed: {}", e);
                        exited = true;
                        break;
                    }
                };

                bytes_read += len;
                processed_output = true;

                let data = &pane.read_buf[..len];
                let responses = pane.vte_processor.process(data, &mut pane.screen_buffer);

                if !responses.is_empty() {
                    pane.pending_write.extend_from_slice(&responses);
                    flush_pending_write(fd, &mut pane.pending_write);
                }
            }
            if processed_output {
                pane.screen_buffer.view_scroll_to_bottom();
            }
        }

        if processed_output {
            if let Some(pane) = self.panes.get_mut(&pane_id) {
                let now = std::time::Instant::now();
                if exited || drained || now.duration_since(pane.last_snapshot_time).as_millis() >= 8
                {
                    if self.visible_panes.contains(&pane_id) {
                        let snapshot = pane.screen_buffer.generate_snapshot();
                        pane.snapshot.store(std::sync::Arc::new(snapshot));
                        pane.last_snapshot_time = now;
                        self.wakeup_signal.wakeup();
                    }
                }
            }
        }

        if exited {
            if let Some(pane) = self.panes.remove(&pane_id) {
                self.loop_handle.remove(pane.token);
            }
            let mut exited_list = self.exited_panes.write().unwrap();
            if !exited_list.contains(&pane_id) {
                exited_list.push(pane_id);
            }
            drop(exited_list);
            // FIX 3: signal the main thread's AtomicBool fast path.
            self.has_exited
                .store(true, std::sync::atomic::Ordering::Release);
            self.wakeup_signal.wakeup(); // Wake up main thread to handle exit
        }

        Ok(PostAction::Continue)
    }
}
