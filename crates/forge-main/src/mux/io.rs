use std::collections::HashMap;
use std::os::unix::io::{AsRawFd, OwnedFd, RawFd};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, RwLock,
};

use arc_swap::ArcSwap;
use calloop::{
    channel::{channel, Channel, Sender},
    EventLoop, Interest, LoopHandle, PostAction, RegistrationToken,
};
use forge_core::{ForgeError, Result};
use forge_pty::vte_parser::CommandLifecycleEvent;
use forge_pty::{snapshot::RenderSnapshot, ScreenBuffer, VteProcessor};

use super::PaneId;

pub const PTY_READ_BUFFER_SIZE: usize = 128 * 1024; // 128KB per read call
pub const MAX_PTY_READ_ITERATIONS_PER_EVENT: usize = 64; // drain burst aggressively
pub const MAX_PTY_READ_BYTES_PER_EVENT: usize = 4 * 1024 * 1024; // 4MB per wakeup event
const MAX_PENDING_PANE_WRITE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneIoStatus {
    Registered,
    Exited,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCompletionEvent {
    pub pane_id: PaneId,
    pub duration_ms: u64,
    pub exit_code: i32,
    pub program_name: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SelectionCopyResult {
    pub serial: u32,
    pub text: Option<String>,
}

pub enum PtyWorkerCommand {
    AddPane {
        pane_id: PaneId,
        fd: OwnedFd,
        vte_processor: Box<VteProcessor>,
        screen_buffer: Box<ScreenBuffer>,
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
    CopySelection {
        pane_id: PaneId,
        serial: u32,
        clear_after_copy: bool,
    },
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
    SetCommandCompletionTracking(bool),
    Write(PaneId, Vec<u8>),
}

pub struct DynamicPtySource {
    inner: calloop::generic::Generic<OwnedFd, std::io::Error>,
    interest: std::rc::Rc<std::cell::Cell<Interest>>,
}

impl calloop::EventSource for DynamicPtySource {
    type Event = calloop::Readiness;
    type Metadata = std::os::unix::io::RawFd;
    type Ret = std::io::Result<PostAction>;
    type Error = std::io::Error;

    fn process_events<C>(
        &mut self,
        readiness: calloop::Readiness,
        token: calloop::Token,
        mut callback: C,
    ) -> std::result::Result<PostAction, Self::Error>
    where
        C: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        self.inner.process_events(readiness, token, |ready, file| {
            use std::os::unix::io::AsRawFd;
            let mut fd = file.as_raw_fd();
            callback(ready, &mut fd)
        })
    }

    fn register(
        &mut self,
        poll: &mut calloop::Poll,
        token_factory: &mut calloop::TokenFactory,
    ) -> calloop::Result<()> {
        self.inner.interest = self.interest.get();
        self.inner.register(poll, token_factory)
    }

    fn reregister(
        &mut self,
        poll: &mut calloop::Poll,
        token_factory: &mut calloop::TokenFactory,
    ) -> calloop::Result<()> {
        self.inner.interest = self.interest.get();
        self.inner.reregister(poll, token_factory)
    }

    fn unregister(&mut self, poll: &mut calloop::Poll) -> calloop::Result<()> {
        self.inner.unregister(poll)
    }
}

#[cfg(test)]
// Tests stay beside command-normalization helpers while the runtime implementation follows.
#[allow(clippy::items_after_test_module)]
mod tests {
    use std::collections::HashMap;

    use super::{
        normalize_command_program_name, queue_pending_pane_write, selected_text,
        MAX_PENDING_PANE_WRITE_BYTES,
    };
    use crate::mux::PaneId;

    #[test]
    fn command_program_name_extracts_basename() {
        assert_eq!(
            normalize_command_program_name("cargo run --release").as_deref(),
            Some("cargo")
        );
        assert_eq!(
            normalize_command_program_name("./target/release/forge-main").as_deref(),
            Some("forge-main")
        );
        assert_eq!(
            normalize_command_program_name("/usr/bin/python script.py").as_deref(),
            Some("python")
        );
        assert_eq!(
            normalize_command_program_name("/home/user/bin/custom-tool --verbose").as_deref(),
            Some("custom-tool")
        );
    }

    #[test]
    fn command_program_name_skips_env_prefixes_and_common_wrappers() {
        assert_eq!(
            normalize_command_program_name("VAR=value cargo run").as_deref(),
            Some("cargo")
        );
        assert_eq!(
            normalize_command_program_name("env VAR=value cargo run").as_deref(),
            Some("cargo")
        );
        assert_eq!(
            normalize_command_program_name("sudo cargo run").as_deref(),
            Some("cargo")
        );
    }

    #[test]
    fn selection_copy_uses_worker_owned_selection_state() {
        let mut screen = forge_pty::ScreenBuffer::new(
            8,
            1,
            100,
            forge_core::color::Color::WHITE,
            forge_core::color::Color::BLACK,
        );
        let mut parser = forge_pty::VteProcessor::new();
        parser.process(b"selected", &mut screen);
        screen.set_selection(Some(forge_core::cell::SelectionRange {
            start_row: 0,
            start_col: 1,
            end_row: 0,
            end_col: 4,
        }));

        assert_eq!(selected_text(&screen).as_deref(), Some("elec"));

        screen.clear_selection();
        assert_eq!(selected_text(&screen), None);
    }

    #[test]
    fn pending_pane_input_is_bounded_and_keeps_earliest_bytes() {
        let pane_id = PaneId::new(7);
        let mut pending = HashMap::new();
        queue_pending_pane_write(&mut pending, pane_id, b"first");
        queue_pending_pane_write(
            &mut pending,
            pane_id,
            &vec![b'x'; MAX_PENDING_PANE_WRITE_BYTES],
        );

        let buffered = pending.get(&pane_id).unwrap();
        assert_eq!(buffered.len(), MAX_PENDING_PANE_WRITE_BYTES);
        assert_eq!(&buffered[..5], b"first");
    }
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
    has_command_events: Arc<AtomicBool>,
    command_events: Arc<RwLock<Vec<CommandCompletionEvent>>>,
    command_tracking_enabled: Arc<AtomicBool>,
    selection_copy_receiver: std::sync::mpsc::Receiver<SelectionCopyResult>,
}

struct WorkerState {
    panes: HashMap<PaneId, PaneState>,
    pending_pane_writes: HashMap<PaneId, Vec<u8>>,
    exited_panes: Arc<RwLock<Vec<PaneId>>>,
    // FIX 3: shared atomic so main thread can read without taking the RwLock.
    has_exited: Arc<AtomicBool>,
    wakeup_signal: calloop::LoopSignal,
    loop_handle: LoopHandle<'static, WorkerState>,
    visible_panes: std::collections::HashSet<PaneId>,
    command_events: Arc<RwLock<Vec<CommandCompletionEvent>>>,
    has_command_events: Arc<AtomicBool>,
    command_tracking_enabled: Arc<AtomicBool>,
    selection_copy_sender: std::sync::mpsc::Sender<SelectionCopyResult>,
}

struct PaneState {
    vte_processor: VteProcessor,
    screen_buffer: ScreenBuffer,
    snapshot: Arc<ArcSwap<RenderSnapshot>>,
    token: RegistrationToken,
    last_snapshot_time: std::time::Instant,
    read_buf: Vec<u8>,
    pending_write: Vec<u8>,
    interest: std::rc::Rc<std::cell::Cell<Interest>>,
    fd: RawFd,
    command_started_at: Option<std::time::Instant>,
    command_program_name: Option<String>,
}

fn tokenize_command_prefix(command: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut escaped = false;

    for (idx, ch) in command.char_indices() {
        if escaped {
            escaped = false;
            if start.is_none() {
                start = Some(idx);
            }
            continue;
        }
        if ch == '\\' {
            escaped = true;
            if start.is_none() {
                start = Some(idx);
            }
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            if start.is_none() {
                start = Some(idx);
            }
            continue;
        }
        if ch.is_whitespace() {
            if let Some(s) = start.take() {
                tokens.push(&command[s..idx]);
                if tokens.len() >= 16 {
                    return tokens;
                }
            }
        } else if start.is_none() {
            start = Some(idx);
        }
    }

    if let Some(s) = start {
        tokens.push(&command[s..]);
    }
    tokens
}

fn strip_token_quotes(token: &str) -> &str {
    token
        .strip_prefix(['\'', '"'])
        .and_then(|value| value.strip_suffix(['\'', '"']))
        .unwrap_or(token)
}

fn is_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    matches!(chars.next(), Some('_') | Some('a'..='z') | Some('A'..='Z'))
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn basename_token(token: &str) -> Option<String> {
    let token = strip_token_quotes(token).trim();
    if token.is_empty() {
        return None;
    }
    let name = token.rsplit('/').next().unwrap_or(token);
    (!name.is_empty()).then(|| name.to_string())
}

pub(crate) fn normalize_command_program_name(command: &str) -> Option<String> {
    let tokens = tokenize_command_prefix(command);
    let mut index = 0;

    while index < tokens.len() && is_env_assignment(strip_token_quotes(tokens[index])) {
        index += 1;
    }

    if tokens.get(index).map(|token| strip_token_quotes(token)) == Some("env") {
        index += 1;
        while index < tokens.len() {
            let token = strip_token_quotes(tokens[index]);
            if is_env_assignment(token) {
                index += 1;
            } else if token == "-u" || token == "--unset" {
                index = (index + 2).min(tokens.len());
            } else if token.starts_with('-') {
                index += 1;
            } else {
                break;
            }
        }
    }

    match tokens.get(index).map(|token| strip_token_quotes(token)) {
        Some("sudo") | Some("doas") | Some("command") | Some("builtin") => {
            index += 1;
            while index < tokens.len() {
                let token = strip_token_quotes(tokens[index]);
                if token.starts_with('-') {
                    index += 1;
                } else {
                    break;
                }
            }
        }
        _ => {}
    }

    tokens.get(index).and_then(|token| basename_token(token))
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

fn queue_pending_pane_write(
    pending_pane_writes: &mut HashMap<PaneId, Vec<u8>>,
    pane_id: PaneId,
    data: &[u8],
) {
    let pending = pending_pane_writes.entry(pane_id).or_default();
    let available = MAX_PENDING_PANE_WRITE_BYTES.saturating_sub(pending.len());
    pending.extend_from_slice(&data[..data.len().min(available)]);
}

fn selected_text(screen_buffer: &ScreenBuffer) -> Option<String> {
    screen_buffer.selection.and_then(|selection| {
        let text = screen_buffer.get_text_in_range(selection);
        (!text.is_empty()).then_some(text)
    })
}

impl PaneIoRegistry {
    pub fn new(wakeup_signal: calloop::LoopSignal, command_tracking_enabled: bool) -> Result<Self> {
        let (sender, receiver) = channel::<PtyWorkerCommand>();
        let has_exited = Arc::new(AtomicBool::new(false));
        let exited_panes = Arc::new(RwLock::new(Vec::new()));
        let visible_gen = Arc::new(AtomicU64::new(0));
        let has_command_events = Arc::new(AtomicBool::new(false));
        let command_events = Arc::new(RwLock::new(Vec::new()));
        let command_tracking_enabled = Arc::new(AtomicBool::new(command_tracking_enabled));
        let (selection_copy_sender, selection_copy_receiver) = std::sync::mpsc::channel();

        let worker_exited_panes = exited_panes.clone();
        let worker_has_exited = has_exited.clone();
        let worker_command_events = command_events.clone();
        let worker_has_command_events = has_command_events.clone();
        let worker_command_tracking_enabled = command_tracking_enabled.clone();

        std::thread::Builder::new()
            .name("forge-pty-worker".to_string())
            .spawn(move || {
                if let Err(e) = Self::run_worker(
                    receiver,
                    worker_exited_panes,
                    worker_has_exited,
                    worker_command_events,
                    worker_has_command_events,
                    worker_command_tracking_enabled,
                    selection_copy_sender,
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
            has_command_events,
            command_events,
            command_tracking_enabled,
            selection_copy_receiver,
        })
    }

    // Worker bootstrap passes each shared channel/state handle exactly once.
    #[allow(clippy::too_many_arguments)]
    fn run_worker(
        receiver: Channel<PtyWorkerCommand>,
        exited_panes: Arc<RwLock<Vec<PaneId>>>,
        has_exited: Arc<AtomicBool>,
        command_events: Arc<RwLock<Vec<CommandCompletionEvent>>>,
        has_command_events: Arc<AtomicBool>,
        command_tracking_enabled: Arc<AtomicBool>,
        selection_copy_sender: std::sync::mpsc::Sender<SelectionCopyResult>,
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
                            if let Err(e) = state.add_pane(
                                pane_id,
                                fd,
                                *vte_processor,
                                *screen_buffer,
                                snapshot,
                            ) {
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
                        PtyWorkerCommand::SetScrollOffset(pane_id, offset) => state
                            .with_pane(pane_id, |p| p.screen_buffer.view_scroll_to_offset(offset)),
                        PtyWorkerCommand::UpdateSelection(pane_id, sel) => {
                            state.with_pane(pane_id, |p| p.screen_buffer.set_selection(sel))
                        }
                        PtyWorkerCommand::ClearSelection(pane_id) => {
                            state.with_pane(pane_id, |p| p.screen_buffer.clear_selection())
                        }
                        PtyWorkerCommand::CopySelection {
                            pane_id,
                            serial,
                            clear_after_copy,
                        } => {
                            let text = state
                                .panes
                                .get(&pane_id)
                                .and_then(|pane| selected_text(&pane.screen_buffer));
                            if clear_after_copy {
                                state.with_pane(pane_id, |pane| {
                                    pane.screen_buffer.clear_selection()
                                });
                            }
                            let _ = state
                                .selection_copy_sender
                                .send(SelectionCopyResult { serial, text });
                            state.wakeup_signal.wakeup();
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
                        PtyWorkerCommand::Write(pane_id, data) => {
                            if let Some(pane) = state.panes.get_mut(&pane_id) {
                                pane.pending_write.extend_from_slice(&data);
                                flush_pending_write(pane.fd, &mut pane.pending_write);
                                if !pane.pending_write.is_empty() {
                                    let current = pane.interest.get();
                                    if !current.writable {
                                        pane.interest.set(calloop::Interest::BOTH);
                                        let _ = state.loop_handle.update(&pane.token);
                                    }
                                }
                            } else {
                                queue_pending_pane_write(
                                    &mut state.pending_pane_writes,
                                    pane_id,
                                    &data,
                                );
                            }
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
                        PtyWorkerCommand::SetCommandCompletionTracking(enabled) => {
                            state
                                .command_tracking_enabled
                                .store(enabled, Ordering::Release);
                            if !enabled {
                                for pane in state.panes.values_mut() {
                                    pane.command_started_at = None;
                                    pane.command_program_name = None;
                                }
                            }
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
            pending_pane_writes: HashMap::new(),
            exited_panes,
            has_exited: has_exited_worker,
            wakeup_signal,
            loop_handle: loop_handle.clone(),
            visible_panes: std::collections::HashSet::new(),
            command_events,
            has_command_events,
            command_tracking_enabled,
            selection_copy_sender,
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
                vte_processor: Box::new(vte_processor),
                screen_buffer: Box::new(screen_buffer),
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
        let mut exited = self.exited_panes.write().unwrap();
        let panes = std::mem::take(&mut *exited);
        self.has_exited.store(false, Ordering::Release);
        panes
    }

    pub fn has_command_events(&self) -> bool {
        self.has_command_events.load(Ordering::Acquire)
    }

    pub fn command_events(&self) -> Vec<CommandCompletionEvent> {
        let mut events = self.command_events.write().unwrap();
        let out = std::mem::take(&mut *events);
        self.has_command_events.store(false, Ordering::Release);
        out
    }

    pub fn set_command_completion_tracking(&self, enabled: bool) {
        self.command_tracking_enabled
            .store(enabled, Ordering::Release);
        self.sender
            .send(PtyWorkerCommand::SetCommandCompletionTracking(enabled))
            .ok();
    }

    pub fn request_selection_copy(
        &self,
        pane_id: PaneId,
        serial: u32,
        clear_after_copy: bool,
    ) -> bool {
        self.sender
            .send(PtyWorkerCommand::CopySelection {
                pane_id,
                serial,
                clear_after_copy,
            })
            .is_ok()
    }

    pub fn try_recv_selection_copy(&self) -> Option<SelectionCopyResult> {
        self.selection_copy_receiver.try_recv().ok()
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
        let raw_fd = std::os::fd::AsRawFd::as_raw_fd(&fd);
        let mut pending_write = self
            .pending_pane_writes
            .remove(&pane_id)
            .unwrap_or_default();
        flush_pending_write(raw_fd, &mut pending_write);
        let initial_interest = if pending_write.is_empty() {
            calloop::Interest::READ
        } else {
            calloop::Interest::BOTH
        };
        let interest = std::rc::Rc::new(std::cell::Cell::new(initial_interest));
        let source = DynamicPtySource {
            inner: calloop::generic::Generic::new(fd, initial_interest, calloop::Mode::Level),
            interest: interest.clone(),
        };
        let token = self
            .loop_handle
            .clone()
            .insert_source(source, move |readiness, src_fd, state| {
                state.process_pane_io(pane_id, src_fd.as_raw_fd(), readiness)
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
                pending_write,
                interest,
                fd: raw_fd,
                command_started_at: None,
                command_program_name: None,
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
        self.pending_pane_writes.remove(&pane_id);
        if let Some(pane) = self.panes.remove(&pane_id) {
            self.loop_handle.remove(pane.token);
        }
    }

    fn process_pane_io(
        &mut self,
        pane_id: PaneId,
        fd: RawFd,
        readiness: calloop::Readiness,
    ) -> std::io::Result<PostAction> {
        let mut bytes_read = 0usize;
        let mut iterations = 0usize;
        let mut processed_output = false;

        let mut exited = false;
        let mut interest_changed = false;

        if let Some(pane) = self.panes.get_mut(&pane_id) {
            if readiness.writable {
                flush_pending_write(fd, &mut pane.pending_write);
                if pane.pending_write.is_empty() {
                    let current = pane.interest.get();
                    if current.writable {
                        pane.interest.set(calloop::Interest::READ);
                        interest_changed = true;
                    }
                }
            }

            if readiness.readable {
                while iterations < MAX_PTY_READ_ITERATIONS_PER_EVENT
                    && bytes_read < MAX_PTY_READ_BYTES_PER_EVENT
                {
                    iterations += 1;

                    let borrowed_fd = unsafe { rustix::fd::BorrowedFd::borrow_raw(fd) };
                    let read_res = rustix::io::read(borrowed_fd, &mut pane.read_buf);

                    let len = match read_res {
                        Ok(0) => {
                            exited = true;
                            break;
                        }
                        Ok(n) => n,
                        Err(rustix::io::Errno::AGAIN) => {
                            break;
                        }
                        Err(rustix::io::Errno::IO) => {
                            exited = true;
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

                    if self.command_tracking_enabled.load(Ordering::Acquire) {
                        let now = std::time::Instant::now();
                        for event in pane.vte_processor.take_command_events() {
                            match event {
                                CommandLifecycleEvent::Started { command } => {
                                    pane.command_started_at = Some(now);
                                    pane.command_program_name =
                                        command.as_deref().and_then(normalize_command_program_name);
                                }
                                CommandLifecycleEvent::Finished { exit_code } => {
                                    if let Some(started_at) = pane.command_started_at.take() {
                                        let duration_ms =
                                            now.saturating_duration_since(started_at).as_millis();
                                        let event = CommandCompletionEvent {
                                            pane_id,
                                            duration_ms: duration_ms.min(u128::from(u64::MAX))
                                                as u64,
                                            exit_code,
                                            program_name: pane.command_program_name.take(),
                                        };
                                        if let Ok(mut events) = self.command_events.write() {
                                            events.push(event);
                                            self.has_command_events.store(true, Ordering::Release);
                                            self.wakeup_signal.wakeup();
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        let _ = pane.vte_processor.take_command_events();
                        pane.command_started_at = None;
                        pane.command_program_name = None;
                    }

                    if !responses.is_empty() {
                        pane.pending_write.extend_from_slice(&responses);
                        flush_pending_write(fd, &mut pane.pending_write);
                    }
                }
                if processed_output {
                    pane.screen_buffer.view_scroll_to_bottom();
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
                self.has_exited.store(true, Ordering::Release);
            }
            return Ok(PostAction::Remove);
        }

        if processed_output {
            if let Some(pane) = self.panes.get_mut(&pane_id) {
                if self.visible_panes.contains(&pane_id) {
                    let snapshot = pane.screen_buffer.generate_snapshot();
                    pane.snapshot.store(Arc::new(snapshot));
                    pane.last_snapshot_time = std::time::Instant::now();
                    self.wakeup_signal.wakeup();
                }
            }
        }

        if interest_changed {
            if let Some(pane) = self.panes.get(&pane_id) {
                let _ = self.loop_handle.update(&pane.token);
            }
        }

        Ok(PostAction::Continue)
    }
}
