use crate::wayland::connection::WaylandState;
use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;
use forge_core::{ForgeError, Result};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};
use wayland_client::EventQueue;

// ─── Pane Animation ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaneAnimationKind {
    /// Pane is opening (rect expands from start → target)
    Open,
    /// Pane is closing (rect contracts from start → target, then removal fires)
    Close,
    /// An existing pane is transitioning to a new layout position
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneAnimation {
    pub kind: PaneAnimationKind,
    pub dx: f32,
    pub dy: f32,
    pub dw: f32,
    pub dh: f32,
    pub started_at: Instant,
    pub duration: Duration,
}

pub struct ClosingPane {
    pub pane_id: crate::mux::PaneId,
    pub rect: crate::mux::PaneRect,
    pub snapshot: std::sync::Arc<forge_pty::snapshot::RenderSnapshot>,
    pub anim: PaneAnimation,
    pub is_floating: bool,
}

const MAX_DEFERRED_STARTUP_WORK: usize = 64;

#[derive(Debug, Clone)]
enum DeferredStartupWork {
    Split(crate::wayland::connection::PendingSplit),
    Action(forge_core::bindings::Action),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandCompletionIndicator {
    success: bool,
    program_name: std::sync::Arc<str>,
    exit_text: Option<std::sync::Arc<str>>,
    generation: u64,
    tab_id: crate::mux::TabId,
    shown_at: std::time::Instant,
    opening_frame_presented: bool,
    dismissed_at: Option<std::time::Instant>,
    expires_at: Option<std::time::Instant>,
}

const COMMAND_INDICATOR_CIRCLE_HOLD: std::time::Duration = std::time::Duration::from_millis(500);
const COMMAND_INDICATOR_EXPAND: std::time::Duration = std::time::Duration::from_millis(150);
const COMMAND_INDICATOR_CONTRACT: std::time::Duration = std::time::Duration::from_millis(140);

#[derive(Clone, Copy, Debug, PartialEq)]
struct CommandIndicatorVisual {
    expansion: f32,
    animating: bool,
}

fn command_indicator_smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn command_indicator_phase_progress(
    now: std::time::Instant,
    start: std::time::Instant,
    duration: std::time::Duration,
) -> f32 {
    if duration.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(start).as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
}

fn command_indicator_visual(
    indicator: &CommandCompletionIndicator,
    config: &forge_core::config_registry::CommandCompletionIndicatorConfig,
    now: std::time::Instant,
) -> CommandIndicatorVisual {
    let expand_at = indicator.shown_at + COMMAND_INDICATOR_CIRCLE_HOLD;
    let expanded_at = expand_at + COMMAND_INDICATOR_EXPAND;
    if now < expand_at {
        return CommandIndicatorVisual {
            expansion: 0.0,
            animating: false,
        };
    }
    if now < expanded_at {
        let progress = command_indicator_smoothstep(command_indicator_phase_progress(
            now,
            expand_at,
            COMMAND_INDICATOR_EXPAND,
        ));
        return CommandIndicatorVisual {
            expansion: progress,
            animating: true,
        };
    }

    let dismiss_at = indicator.dismissed_at.or_else(|| {
        matches!(
            config.dismissal,
            forge_core::config_registry::CommandCompletionIndicatorDismissal::Timeout
        )
        .then(|| expanded_at + std::time::Duration::from_millis(config.display_duration_ms))
    });
    let Some(dismiss_at) = dismiss_at else {
        return CommandIndicatorVisual {
            expansion: 1.0,
            animating: false,
        };
    };
    if now < dismiss_at {
        return CommandIndicatorVisual {
            expansion: 1.0,
            animating: false,
        };
    }

    let finished_at = dismiss_at + COMMAND_INDICATOR_CONTRACT;
    if now < finished_at {
        let progress = command_indicator_smoothstep(command_indicator_phase_progress(
            now,
            dismiss_at,
            COMMAND_INDICATOR_CONTRACT,
        ));
        return CommandIndicatorVisual {
            expansion: 1.0 - progress,
            animating: true,
        };
    }

    CommandIndicatorVisual {
        expansion: 0.0,
        animating: false,
    }
}

impl PaneAnimation {
    pub fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        let elapsed = now.saturating_duration_since(self.started_at);
        let t = (elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0);
        pane_ease_out_cubic(t)
    }

    pub fn is_complete(&self, now: Instant) -> bool {
        self.duration.is_zero() || now.saturating_duration_since(self.started_at) >= self.duration
    }
}

#[inline(always)]
fn pane_ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

fn compute_statusbar_hover_color(bg_color_hex: &str) -> [f32; 4] {
    let bg = crate::statusbar::parse_hex_color(bg_color_hex)
        .unwrap_or(forge_core::color::Color::TRANSPARENT);

    let r = bg.r as f32 / 255.0;
    let g = bg.g as f32 / 255.0;
    let b = bg.b as f32 / 255.0;

    let r = r * 0.6 + 1.0 * 0.4;
    let g = g * 0.6 + 1.0 * 0.4;
    let b = b * 0.6 + 1.0 * 0.4;

    let lin = forge_core::color::Color {
        r: (r * 255.0) as u8,
        g: (g * 255.0) as u8,
        b: (b * 255.0) as u8,
        a: 255,
    }
    .to_srgb_linear();

    [lin.r, lin.g, lin.b, 0.35]
}

pub struct AppData {
    pub wayland_state: WaylandState,
    pub loop_signal: calloop::LoopSignal,
    pub tab_manager: crate::mux::TabManager,
    pub pane_runtime: crate::mux::PaneRuntime,
    pub pane_io: crate::mux::PaneIoRegistry,
    command_completion_indicators:
        std::collections::HashMap<crate::mux::PaneId, CommandCompletionIndicator>,
    command_completion_generation: u64,
    selection_copies_in_flight: u8,
    pastes_waiting_for_copy: u8,

    pub key_receiver: std::sync::mpsc::Receiver<Vec<u8>>,
    pub pointer_receiver: std::sync::mpsc::Receiver<crate::wayland::connection::PointerEvent>,
    pub paste_receiver: std::sync::mpsc::Receiver<Vec<u8>>,
    pub config: forge_core::config_registry::ForgeConfig,
    pub renderer: Option<forge_renderer::Renderer>,
    pub queue_handle: wayland_client::QueueHandle<WaylandState>,
    pub drag_start: Option<(usize, usize)>,
    pub active_mouse_button: Option<u32>,
    pub last_mouse_col: usize,
    pub last_mouse_row: usize,
    pub pointer_x: f64,
    pub pointer_y: f64,
    pub scroll_accum: f64,
    pub last_window_size: forge_core::geometry::Size,
    pub startup_geometry_ready: bool,
    pub pending_startup_window_size: Option<forge_core::geometry::Size>,
    deferred_startup_work: Vec<DeferredStartupWork>,
    pub font_atlas_receiver: Option<std::sync::mpsc::Receiver<forge_renderer::font::FontData>>,
    pub cursor_visible_phase: bool,
    pub last_cursor_blink: std::time::Instant,
    pub config_rx: Option<crossbeam_channel::Receiver<forge_config::ConfigUpdate>>,
    pub last_scrollbar_state: Option<(f64, f64)>,
    pub last_mouse_activity: std::time::Instant,
    pub mouse_started_moving: std::time::Instant,
    pub is_hovering_edge: bool,
    pub is_hovering_statusbar: bool,
    pub current_thumb_width: f32,
    pub current_track_opacity: f32,
    pub current_thumb_opacity: f32,
    pub is_dragging_scrollbar: bool,
    pub scrollbar_drag_offset_y: f64,
    pub startup_start: std::time::Instant,
    pub last_snapshot_ids: std::collections::HashMap<crate::mux::PaneId, u64>,
    pub first_vulkan_text_frame_logged: bool,
    pub cached_bg_color: forge_core::color::ColorF32,
    pub cached_cursor_color: forge_core::color::ColorF32,
    pub cached_selection_bg_color: forge_core::color::ColorF32,
    pub cached_statusbar_hover_color: [f32; 4],
    pub cached_grid_metrics: Option<GridMetrics>,
    pub last_visible_gen: u64,
    /// When true, skip the frame_ready gate and render immediately this tick.
    pub force_immediate_render: bool,
    pub hovered_split: Option<usize>,
    pub dragging_split: Option<crate::mux::layout::SplitBorder>,
    pub hovered_scrolling_resize: Option<crate::mux::ScrollingResizeHandle>,
    pub dragging_scrolling_resize: Option<crate::mux::ScrollingResizeDrag>,
    pub statusbar: crate::statusbar::StatusBarState,
    pub sidebar: crate::sidebar::SidebarState,
    pub context_menu: Option<crate::context_menu::ContextMenuState>,
    pub active_modal: Option<crate::confirm_modal::ConfirmCloseModal>,
    pub modal_generation: u64,
    /// Per-pane animations (open, close, move). Only populated while an animation is running.
    pub pane_animations: std::collections::HashMap<crate::mux::PaneId, PaneAnimation>,
    pub last_layout_rects:
        std::collections::HashMap<crate::mux::PaneId, (crate::mux::PaneRect, crate::mux::PaneRect)>,
    pub closing_panes: Vec<ClosingPane>,
}

impl AsMut<WaylandState> for AppData {
    fn as_mut(&mut self) -> &mut WaylandState {
        &mut self.wayland_state
    }
}

fn active_cursor_blink_enabled(app_data: &AppData) -> bool {
    let mux = app_data.tab_manager.active_mux();
    mux.panes
        .get(&mux.active_pane)
        .map(|pane| {
            let snapshot = pane.snapshot.load();
            snapshot.cursor.is_some()
                && snapshot
                    .cursor_blink_override
                    .unwrap_or(app_data.config.cursor.blink)
        })
        .unwrap_or(app_data.config.cursor.blink)
}

impl AppData {
    pub fn active_pane_id(&self) -> crate::mux::PaneId {
        let active_mux = self.tab_manager.active_mux();
        if active_mux.floating_panes.contains(&active_mux.active_pane) {
            return active_mux.active_pane;
        }
        match &self.pane_runtime {
            crate::mux::PaneRuntime::Tiling => active_mux.active_pane_id(),
            crate::mux::PaneRuntime::Scrolling(manager) => manager
                .active_pane_id()
                .unwrap_or_else(|| active_mux.active_pane_id()),
        }
    }

    pub fn visible_pane_ids(&self) -> Vec<crate::mux::PaneId> {
        let active_mux = self.tab_manager.active_mux();
        let mut ids = match &self.pane_runtime {
            crate::mux::PaneRuntime::Tiling => active_mux.visible_pane_ids(),
            crate::mux::PaneRuntime::Scrolling(manager) => manager.visible_pane_ids(),
        };
        for fp in &active_mux.floating_panes {
            if !ids.contains(fp) {
                ids.push(*fp);
            }
        }
        ids
    }

    pub fn pane_at_point(&self, x: f32, y: f32) -> Option<crate::mux::PaneId> {
        let active_mux = self.tab_manager.active_mux();
        for &pane_id in active_mux.floating_panes.iter().rev() {
            if let Some(pane) = active_mux.panes.get(&pane_id) {
                if pane.rect.contains_point(x, y) {
                    return Some(pane_id);
                }
            }
        }

        match &self.pane_runtime {
            crate::mux::PaneRuntime::Tiling => active_mux.pane_at_point(x, y),
            crate::mux::PaneRuntime::Scrolling(manager) => {
                let metrics = self.cached_grid_metrics?;
                let padding = self.effective_pane_padding();
                let content_x = x as f64 - metrics.pad_x - padding.left as f64;
                let content_y = y as f64 - metrics.pad_y - padding.top as f64;
                let col = (content_x / metrics.effective_cell_w).floor();
                let row = (content_y / metrics.effective_cell_h).floor();
                if col < 0.0 || row < 0.0 {
                    return None;
                }
                manager
                    .active_tab()
                    .and_then(|tab| tab.panes.pane_at_cell(col as usize, row as usize))
            }
        }
    }

    pub fn point_target(&self, x: f32, y: f32) -> Option<crate::mux::PanePointTarget> {
        let active_mux = self.tab_manager.active_mux();
        for &pane_id in active_mux.floating_panes.iter().rev() {
            if let Some(pane) = active_mux.panes.get(&pane_id) {
                if pane.rect.contains_point(x, y) {
                    let (local_x, local_y) = pane.rect.local_point(x, y);
                    return Some(crate::mux::PanePointTarget {
                        pane_id,
                        rect: pane.rect,
                        local_x,
                        local_y,
                    });
                }
            }
        }

        match &self.pane_runtime {
            crate::mux::PaneRuntime::Tiling => active_mux.point_target(x, y),
            crate::mux::PaneRuntime::Scrolling(manager) => {
                let metrics = self.cached_grid_metrics?;
                let padding = self.effective_pane_padding();
                let content_x = x as f64 - metrics.pad_x - padding.left as f64;
                let content_y = y as f64 - metrics.pad_y - padding.top as f64;
                let col = (content_x / metrics.effective_cell_w).floor();
                let row = (content_y / metrics.effective_cell_h).floor();
                if col < 0.0 || row < 0.0 {
                    return None;
                }
                let frac_x = content_x - col * metrics.effective_cell_w;
                let frac_y = content_y - row * metrics.effective_cell_h;
                let target = manager
                    .active_tab()
                    .and_then(|tab| tab.panes.point_target(col as usize, row as usize))?;
                let rect = crate::mux::PaneRect {
                    x: metrics.pad_x as f32
                        + padding.left as f32
                        + target.viewport_col as f32 * metrics.effective_cell_w as f32,
                    y: metrics.pad_y as f32
                        + padding.top as f32
                        + target.viewport_row as f32 * metrics.effective_cell_h as f32,
                    width: metrics.effective_cell_w as f32,
                    height: metrics.effective_cell_h as f32,
                };
                Some(crate::mux::PanePointTarget {
                    pane_id: target.pane_id,
                    rect,
                    local_x: target.local_col as f32 * metrics.effective_cell_w as f32
                        + frac_x as f32,
                    local_y: target.local_row as f32 * metrics.effective_cell_h as f32
                        + frac_y as f32,
                })
            }
        }
    }

    pub fn effective_pane_padding(&self) -> forge_core::config_registry::PaddingConfig {
        if self.tab_manager.active_mux().panes.len() <= 1
            || self.tab_manager.active_mux().is_zoomed()
        {
            forge_core::config_registry::PaddingConfig {
                top: 0,
                bottom: 0,
                left: 0,
                right: 0,
            }
        } else {
            self.config.window.pane_padding
        }
    }

    pub fn update_statusbar(&mut self, sb_cols: usize) {
        if !self.config.statusbar.enabled {
            return;
        }

        let tabs: Vec<crate::statusbar::StatusbarTab> = self
            .tab_manager
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| crate::statusbar::StatusbarTab {
                index: i,
                title: format!("Tab {}", i + 1),
                is_zoomed: tab.mux.is_zoomed(),
            })
            .collect();
        let active_tab = self.tab_manager.active_tab_index;

        let active_pane = self.tab_manager.active_mux().active_pane;
        if let Some(pane) = self.tab_manager.active_mux().panes.get(&active_pane) {
            let snap = pane.snapshot.load();
            if let Some(dir) = &snap.current_dir {
                self.statusbar.set_var("dir", dir);
            }
            if let Some(title) = &snap.current_title {
                self.statusbar.set_var("title", title);
            }
        }

        self.statusbar
            .rebuild(&self.config.statusbar, sb_cols, &tabs, active_tab);
    }
}

fn debug_screen_artifacts(rows: &[&[forge_core::cell::Cell]]) {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    static LOGGED_FRAMES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    if !*ENABLED.get_or_init(|| std::env::var_os("FORGE_DEBUG_SCREEN_ARTIFACTS").is_some()) {
        return;
    }

    let frame_index = LOGGED_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if frame_index >= 40 {
        return;
    }

    for (row_idx, row) in rows.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            if cell.c != ';' {
                continue;
            }

            let start = col_idx.saturating_sub(12);
            let end = (col_idx + 13).min(row.len());
            let context: String = row[start..end]
                .iter()
                .map(|cell| match cell.c {
                    '\0' => '·',
                    c if c.is_control() => '�',
                    c => c,
                })
                .collect();
            let codes: Vec<String> = row[start..end]
                .iter()
                .map(|cell| format!("U+{:04X}", cell.c as u32))
                .collect();

            tracing::warn!(
                frame = frame_index,
                row = row_idx,
                col = col_idx,
                context_start = start,
                context = %context,
                codes = ?codes,
                fg = ?cell.fg,
                bg = ?cell.bg,
                flags = cell.flags,
                "Screen buffer contains literal semicolon cell"
            );
        }
    }
}

// This pure predicate keeps independent event-loop state explicit at its call sites.
#[allow(clippy::too_many_arguments)]
fn scrollbar_overlay_wants_redraw(
    use_alt_buffer: bool,
    scrollback_lines: usize,
    current_thumb_opacity: f32,
    current_track_opacity: f32,
    is_hovering_edge: bool,
    is_dragging_scrollbar: bool,
    last_mouse_activity: std::time::Instant,
    mouse_started_moving: std::time::Instant,
    now: std::time::Instant,
) -> bool {
    if use_alt_buffer || scrollback_lines == 0 {
        return false;
    }

    if is_hovering_edge || is_dragging_scrollbar {
        return true;
    }

    let idle_secs = now.duration_since(last_mouse_activity).as_secs_f32();
    let active_secs = now.duration_since(mouse_started_moving).as_secs_f32();
    let pointer_reveal_active = idle_secs < 0.5;
    let pointer_reveal_pending = pointer_reveal_active && active_secs < 0.25;
    let pointer_reveal_visible = pointer_reveal_active && active_secs >= 0.25;

    pointer_reveal_pending
        || pointer_reveal_visible
        || current_thumb_opacity > 0.01
        || current_track_opacity > 0.01
}

fn reveal_scrollbar_from_scroll(app_data: &mut AppData, now: std::time::Instant) {
    app_data.last_mouse_activity = now;
    app_data.mouse_started_moving = now
        .checked_sub(std::time::Duration::from_millis(250))
        .unwrap_or(now);
    app_data.wayland_state.force_redraw = true;
    app_data.loop_signal.wakeup();
}

fn copy_selection_from_pane(app_data: &mut AppData, pane_id: crate::mux::PaneId) {
    let serial = app_data.wayland_state.pointer_button_serial;
    copy_selection_from_pane_with_serial(app_data, pane_id, serial, false);
}

fn copy_selection_from_pane_with_serial(
    app_data: &mut AppData,
    pane_id: crate::mux::PaneId,
    serial: u32,
    clear_after_copy: bool,
) {
    if !app_data
        .tab_manager
        .active_mux()
        .panes
        .contains_key(&pane_id)
    {
        return;
    }
    if app_data
        .pane_io
        .request_selection_copy(pane_id, serial, clear_after_copy)
    {
        app_data.selection_copies_in_flight = app_data.selection_copies_in_flight.saturating_add(1);
    }
}

fn request_clipboard_paste(app_data: &mut AppData) {
    if app_data.selection_copies_in_flight > 0
        || app_data.wayland_state.pending_copy_serial.is_some()
    {
        app_data.pastes_waiting_for_copy = app_data.pastes_waiting_for_copy.saturating_add(1);
        return;
    }

    if let Some(clipboard) = &mut app_data.wayland_state.clipboard {
        app_data.wayland_state.needs_flush |= clipboard.request_paste();
    }
}

fn finish_selection_copy_requests(app_data: &mut AppData) {
    while let Some(result) = app_data.pane_io.try_recv_selection_copy() {
        app_data.selection_copies_in_flight = app_data.selection_copies_in_flight.saturating_sub(1);
        if let Some(text) = result.text {
            if let Some(clipboard) = &mut app_data.wayland_state.clipboard {
                app_data.wayland_state.needs_flush |=
                    clipboard.set_clipboard(text, result.serial, &app_data.queue_handle);
            }
        }
    }

    if app_data.selection_copies_in_flight == 0 && app_data.pastes_waiting_for_copy > 0 {
        let pending = std::mem::take(&mut app_data.pastes_waiting_for_copy);
        for _ in 0..pending {
            request_clipboard_paste(app_data);
        }
    }
}

fn current_layout_params(
    app_data: &AppData,
) -> Option<(GridMetrics, crate::mux::layout::LayoutParams)> {
    let metrics = app_data.cached_grid_metrics?;
    let content_rect = crate::mux::PaneRect::new(
        metrics.pad_x as f32,
        metrics.pad_y as f32,
        (metrics.cols as f64 * metrics.effective_cell_w) as f32,
        (metrics.rows as f64 * metrics.effective_cell_h) as f32,
    );
    Some((
        metrics,
        crate::mux::LayoutParams::new(
            content_rect,
            metrics.effective_cell_w as f32,
            metrics.effective_cell_h as f32,
            app_data.config.window.gap as f32,
            app_data.effective_pane_padding(),
        ),
    ))
}

fn apply_pane_layout_changes(
    app_data: &mut AppData,
    changes: Vec<crate::mux::state::PaneLayoutChange>,
    metrics: GridMetrics,
) {
    if changes.is_empty() {
        return;
    }

    let batch: Vec<_> = changes
        .iter()
        .map(|change| {
            (
                change.pane_id,
                change.new_grid_size.cols,
                change.new_grid_size.rows,
            )
        })
        .collect();

    for change in &changes {
        if let Some(pane_pty) = app_data
            .tab_manager
            .active_mux_mut()
            .panes
            .get_mut(&change.pane_id)
            .and_then(|pane| pane.pty.as_mut())
        {
            let px_w = (change.new_grid_size.cols as f64 * metrics.effective_cell_w) as u16;
            let px_h = (change.new_grid_size.rows as f64 * metrics.effective_cell_h) as u16;
            let _ = pane_pty.resize(
                change.new_grid_size.cols as u16,
                change.new_grid_size.rows as u16,
                px_w,
                px_h,
            );
        }
    }

    app_data
        .pane_io
        .send_ui_command(crate::mux::io::PtyWorkerCommand::BatchResizeReflow(
            batch, None,
        ));
}

fn apply_scrolling_grid_changes(
    app_data: &mut AppData,
    changes: Vec<(crate::mux::PaneId, crate::mux::GridSize)>,
    metrics: GridMetrics,
) {
    let tab_id = app_data.tab_manager.active_tab().id;
    apply_scrolling_grid_changes_to_tab(app_data, tab_id, changes, metrics);
}

fn apply_scrolling_grid_changes_to_tab(
    app_data: &mut AppData,
    tab_id: crate::mux::TabId,
    changes: Vec<(crate::mux::PaneId, crate::mux::GridSize)>,
    metrics: GridMetrics,
) {
    if changes.is_empty() {
        return;
    }

    let Some(tab_index) = app_data
        .tab_manager
        .tabs
        .iter()
        .position(|tab| tab.id == tab_id)
    else {
        return;
    };

    for (pane_id, grid_size) in &changes {
        if let Some(pane) = app_data.tab_manager.tabs[tab_index]
            .mux
            .panes
            .get_mut(pane_id)
        {
            if pane.grid_size != *grid_size {
                pane.grid_size = *grid_size;
            }
        }
    }

    let batch: Vec<_> = changes
        .iter()
        .map(|(pane_id, grid_size)| (*pane_id, grid_size.cols, grid_size.rows))
        .collect();

    for (pane_id, grid_size) in &changes {
        if let Some(pty) = app_data.tab_manager.tabs[tab_index]
            .mux
            .panes
            .get_mut(pane_id)
            .and_then(|pane| pane.pty.as_mut())
        {
            let px_w = (grid_size.cols as f64 * metrics.effective_cell_w) as u16;
            let px_h = (grid_size.rows as f64 * metrics.effective_cell_h) as u16;
            let _ = pty.resize(grid_size.cols as u16, grid_size.rows as u16, px_w, px_h);
        }
    }

    app_data
        .pane_io
        .send_ui_command(crate::mux::io::PtyWorkerCommand::BatchResizeReflow(
            batch, None,
        ));
}

fn toggle_pane_zoom(
    app_data: &mut AppData,
    target_pane: Option<crate::mux::PaneId>,
) -> std::result::Result<(), crate::mux::state::RelayoutError> {
    if let Some(pane_id) = target_pane {
        if !app_data
            .tab_manager
            .active_mux()
            .panes
            .contains_key(&pane_id)
        {
            return Ok(());
        }
        app_data.tab_manager.active_mux_mut().active_pane = pane_id;
    }

    if let Some(scrolling) = app_data.pane_runtime.scrolling_mut() {
        if let Some(changes) = scrolling.toggle_zoom_active() {
            if let Some(metrics) = app_data.cached_grid_metrics {
                apply_scrolling_grid_changes(app_data, changes, metrics);
            }
            app_data.hovered_split = None;
            app_data.dragging_split = None;
            app_data.hovered_scrolling_resize = None;
            app_data.dragging_scrolling_resize = None;
            app_data.active_mouse_button = None;
            app_data
                .pane_io
                .visible_gen
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            app_data.force_immediate_render = true;
            app_data.wayland_state.force_redraw = true;
        }
        return Ok(());
    }

    let Some((metrics, layout_params)) = current_layout_params(app_data) else {
        return Ok(());
    };

    let changes = app_data
        .tab_manager
        .active_mux_mut()
        .toggle_zoom(layout_params)?;
    app_data.hovered_split = None;
    app_data.dragging_split = None;
    app_data.hovered_scrolling_resize = None;
    app_data.dragging_scrolling_resize = None;
    app_data.active_mouse_button = None;
    apply_pane_layout_changes(app_data, changes, metrics);
    app_data
        .pane_io
        .visible_gen
        .fetch_add(1, std::sync::atomic::Ordering::Release);
    app_data.force_immediate_render = true;
    app_data.wayland_state.force_redraw = true;

    Ok(())
}

fn toggle_pane_floating(
    app_data: &mut AppData,
    target_pane: Option<crate::mux::PaneId>,
) -> std::result::Result<(), crate::mux::state::RelayoutError> {
    let pane_id = target_pane.unwrap_or_else(|| app_data.tab_manager.active_mux().active_pane);

    if !app_data
        .tab_manager
        .active_mux()
        .panes
        .contains_key(&pane_id)
    {
        return Ok(());
    }

    let is_floating = app_data
        .tab_manager
        .active_mux()
        .floating_panes
        .contains(&pane_id);
    if !is_floating {
        // We only implement docking a floating pane for now.
        return Ok(());
    }

    // Determine an appropriate axis. We default to Vertical for now, or we could look at the window dimensions.
    let axis = crate::mux::state::SplitAxis::Vertical;

    if let Some(scrolling) = app_data.pane_runtime.scrolling_mut() {
        app_data
            .tab_manager
            .active_mux_mut()
            .floating_panes
            .retain(|&id| id != pane_id);
        if let Some(changes) = scrolling.split_active_with_existing(axis, pane_id) {
            if let Some(metrics) = app_data.cached_grid_metrics {
                apply_scrolling_grid_changes(app_data, changes, metrics);
            }
        }
    } else {
        // Tiling mode
        if let Some((metrics, layout_params)) = current_layout_params(app_data) {
            if app_data
                .tab_manager
                .active_mux_mut()
                .dock_floating_pane(pane_id, axis)
                .is_ok()
            {
                let changes = app_data
                    .tab_manager
                    .active_mux_mut()
                    .relayout(layout_params)?;
                apply_pane_layout_changes(app_data, changes, metrics);
            }
        }
    }

    app_data.hovered_split = None;
    app_data.dragging_split = None;
    app_data.hovered_scrolling_resize = None;
    app_data.dragging_scrolling_resize = None;
    app_data.active_mouse_button = None;
    app_data
        .pane_io
        .visible_gen
        .fetch_add(1, std::sync::atomic::Ordering::Release);
    app_data.force_immediate_render = true;
    app_data.wayland_state.force_redraw = true;

    Ok(())
}

fn pane_foreground_program_name(app_data: &AppData, pane_id: crate::mux::PaneId) -> Option<String> {
    let pane = app_data.tab_manager.active_mux().panes.get(&pane_id)?;
    let pty = pane.pty.as_ref()?;
    let pgrp = unsafe { libc::tcgetpgrp(pty.master_fd.as_raw_fd()) };
    if pgrp <= 0 || pgrp == pty.child_pid.as_raw() {
        return None;
    }

    std::fs::read_to_string(format!("/proc/{}/comm", pgrp))
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn sanitize_program_name(value: &str) -> Option<String> {
    let first = value.split_whitespace().next()?.trim();
    let basename = std::path::Path::new(first)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(first)
        .trim();
    (!basename.is_empty()).then(|| basename.to_string())
}

fn pane_close_confirmation(
    app_data: &AppData,
    pane_id: crate::mux::PaneId,
) -> Option<Option<String>> {
    let pane = app_data.tab_manager.active_mux().panes.get(&pane_id)?;
    let snap = pane.snapshot.load();
    let foreground_program = pane_foreground_program_name(app_data, pane_id);
    let program = foreground_program.clone().or_else(|| {
        snap.is_command_running
            .then(|| {
                snap.current_command
                    .as_deref()
                    .and_then(sanitize_program_name)
            })
            .flatten()
    });
    let running = snap.use_alt_buffer || snap.is_command_running || foreground_program.is_some();
    running.then_some(program)
}

fn request_close_active_pane(app_data: &mut AppData) {
    let pane_id = app_data.active_pane_id();
    if let Some(program_name) = pane_close_confirmation(app_data, pane_id) {
        app_data.active_modal = Some(crate::confirm_modal::ConfirmCloseModal::for_pane(
            pane_id,
            program_name,
        ));
        app_data.modal_generation = app_data.modal_generation.wrapping_add(1);
        app_data.context_menu = None;
        app_data.wayland_state.force_redraw = true;
    } else {
        close_pane(app_data, pane_id);
    }
}

pub fn close_pane(app_data: &mut AppData, pane_id: crate::mux::PaneId) {
    let play_anim = !matches!(
        app_data.config.render.pane_animation,
        forge_core::config_registry::PaneAnimationMode::None
    );

    if play_anim {
        let is_floating = app_data
            .tab_manager
            .active_mux()
            .floating_panes
            .contains(&pane_id);

        let is_last_pane = if is_floating {
            false
        } else if app_data.pane_runtime.is_tiling() {
            app_data.tab_manager.active_mux().panes.len() <= 1
        } else {
            app_data
                .pane_runtime
                .scrolling()
                .map(|m| {
                    m.active_tab()
                        .map(|t| t.panes.pane_count() <= 1)
                        .unwrap_or(true)
                })
                .unwrap_or(true)
        };

        if !is_last_pane {
            let duration =
                Duration::from_millis(app_data.config.render.pane_animation_duration_ms as u64);
            let rect = app_data
                .last_layout_rects
                .get(&pane_id)
                .map(|(_, s)| *s)
                .unwrap_or(crate::mux::PaneRect::new(0.0, 0.0, 0.0, 0.0));

            // Get the last snapshot to render while closing
            if let Some(pane) = app_data.tab_manager.active_mux().panes.get(&pane_id) {
                let snapshot = pane.snapshot.load_full();

                let anim = PaneAnimation {
                    kind: PaneAnimationKind::Close,
                    dx: 0.0,
                    dy: 0.0,
                    dw: 0.0,
                    dh: 0.0, // handled in render_closing_panes
                    started_at: Instant::now(),
                    duration,
                };
                let is_floating = app_data
                    .tab_manager
                    .active_mux()
                    .floating_panes
                    .contains(&pane_id);
                app_data.closing_panes.push(ClosingPane {
                    pane_id,
                    rect,
                    snapshot,
                    anim,
                    is_floating,
                });
                app_data.wayland_state.force_redraw = true;
            }
        }
    }
    execute_close_pane(app_data, pane_id);
}

fn execute_close_pane(app_data: &mut AppData, pane_id: crate::mux::PaneId) {
    app_data.command_completion_indicators.remove(&pane_id);
    app_data.pane_io.remove_pane(pane_id);
    app_data.context_menu = None;
    app_data.hovered_split = None;
    app_data.dragging_split = None;
    app_data.hovered_scrolling_resize = None;
    app_data.dragging_scrolling_resize = None;
    app_data.active_mouse_button = None;

    let is_floating = app_data
        .tab_manager
        .active_mux()
        .floating_panes
        .contains(&pane_id);

    let remove_result = if is_floating {
        app_data
            .tab_manager
            .active_mux_mut()
            .remove_floating_pane(pane_id)
    } else if app_data.pane_runtime.is_tiling() {
        app_data.tab_manager.active_mux_mut().remove_pane(pane_id)
    } else {
        app_data
            .tab_manager
            .active_mux_mut()
            .remove_detached_pane(pane_id)
    };

    match remove_result {
        crate::mux::state::RemovePaneResult::RemovedLastPane => {
            if app_data.tab_manager.tabs.len() == 1 {
                app_data.wayland_state.running = false;
            } else {
                let closed_tab_id = app_data.tab_manager.active_tab().id;
                let exited = app_data.tab_manager.close_active_tab();
                if exited {
                    app_data.wayland_state.running = false;
                } else {
                    if let Some(scrolling) = app_data.pane_runtime.scrolling_mut() {
                        scrolling.remove_tab(closed_tab_id);
                        scrolling.sync_active_tab_index(app_data.tab_manager.active_tab_index);
                    }
                    app_data
                        .pane_io
                        .visible_gen
                        .fetch_add(1, std::sync::atomic::Ordering::Release);
                    app_data.force_immediate_render = true;
                    app_data.wayland_state.force_redraw = true;
                }
            }
        }
        crate::mux::state::RemovePaneResult::Removed { .. } => {
            if is_floating {
                app_data
                    .pane_io
                    .visible_gen
                    .fetch_add(1, std::sync::atomic::Ordering::Release);
                app_data.force_immediate_render = true;
                app_data.wayland_state.force_redraw = true;
                return;
            }
            if let Some(scrolling) = app_data.pane_runtime.scrolling_mut() {
                let removal = scrolling.remove_pane_with_changes(pane_id);
                let grid_changes = removal.grid_changes;
                if let Some(active_pane) = scrolling.active_pane_id() {
                    app_data.tab_manager.active_mux_mut().active_pane = active_pane;
                }
                if let Some(metrics) = app_data.cached_grid_metrics {
                    apply_scrolling_grid_changes(app_data, grid_changes, metrics);
                }
                app_data
                    .pane_io
                    .visible_gen
                    .fetch_add(1, std::sync::atomic::Ordering::Release);
                app_data.force_immediate_render = true;
                app_data.wayland_state.force_redraw = true;
                return;
            }
            let Some((metrics, layout_params)) = current_layout_params(app_data) else {
                app_data.wayland_state.force_redraw = true;
                return;
            };
            match app_data
                .tab_manager
                .active_mux_mut()
                .relayout(layout_params)
            {
                Ok(changes) => apply_pane_layout_changes(app_data, changes, metrics),
                Err(err) => tracing::warn!(?err, "Failed to relayout after closing pane"),
            }
            app_data
                .pane_io
                .visible_gen
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            app_data.force_immediate_render = true;
            app_data.wayland_state.force_redraw = true;
        }
        crate::mux::state::RemovePaneResult::MissingPane => {}
    }
}

fn request_close_active_tab(app_data: &mut AppData) {
    let tab_id = app_data.tab_manager.active_tab().id;
    let pane_ids: Vec<_> = app_data
        .tab_manager
        .active_mux()
        .panes
        .keys()
        .copied()
        .collect();
    let confirmations: Vec<_> = pane_ids
        .iter()
        .filter_map(|pane_id| pane_close_confirmation(app_data, *pane_id))
        .collect();

    if confirmations.is_empty() {
        close_tab(app_data, tab_id);
    } else {
        let program_name = confirmations.into_iter().flatten().next();
        app_data.active_modal = Some(crate::confirm_modal::ConfirmCloseModal::for_tab(
            tab_id,
            program_name,
        ));
        app_data.modal_generation = app_data.modal_generation.wrapping_add(1);
        app_data.context_menu = None;
        app_data.wayland_state.force_redraw = true;
    }
}

fn close_tab(app_data: &mut AppData, tab_id: crate::mux::TabId) {
    let Some(tab_idx) = app_data
        .tab_manager
        .tabs
        .iter()
        .position(|tab| tab.id == tab_id)
    else {
        return;
    };

    let pane_ids: Vec<_> = app_data.tab_manager.tabs[tab_idx]
        .mux
        .panes
        .keys()
        .copied()
        .collect();
    for pane_id in pane_ids {
        app_data.command_completion_indicators.remove(&pane_id);
        app_data.pane_io.remove_pane(pane_id);
    }

    if app_data.tab_manager.tabs.len() == 1 {
        app_data.wayland_state.running = false;
        return;
    }

    app_data.tab_manager.tabs.remove(tab_idx);
    if tab_idx < app_data.tab_manager.active_tab_index {
        app_data.tab_manager.active_tab_index -= 1;
    } else if app_data.tab_manager.active_tab_index >= app_data.tab_manager.tabs.len() {
        app_data.tab_manager.active_tab_index = app_data.tab_manager.tabs.len() - 1;
    }
    if let Some(scrolling) = app_data.pane_runtime.scrolling_mut() {
        scrolling.remove_tab(tab_id);
        scrolling.sync_active_tab_index(app_data.tab_manager.active_tab_index);
    }

    app_data
        .pane_io
        .visible_gen
        .fetch_add(1, std::sync::atomic::Ordering::Release);
    app_data.force_immediate_render = true;
    app_data.wayland_state.force_redraw = true;
}

fn handle_modal_action(app_data: &mut AppData, action: crate::confirm_modal::ModalAction) {
    match action {
        crate::confirm_modal::ModalAction::Ignored => {}
        crate::confirm_modal::ModalAction::Redraw => {
            app_data.modal_generation = app_data.modal_generation.wrapping_add(1);
            app_data.wayland_state.force_redraw = true;
        }
        crate::confirm_modal::ModalAction::Cancel => {
            app_data.active_modal = None;
            app_data.modal_generation = app_data.modal_generation.wrapping_add(1);
            app_data.wayland_state.force_redraw = true;
        }
        crate::confirm_modal::ModalAction::Confirm(target) => {
            app_data.active_modal = None;
            app_data.modal_generation = app_data.modal_generation.wrapping_add(1);
            match target {
                crate::confirm_modal::ConfirmCloseTarget::Pane(pane_id) => {
                    close_pane(app_data, pane_id);
                }
                crate::confirm_modal::ConfirmCloseTarget::Tab(tab_id) => {
                    close_tab(app_data, tab_id);
                }
            }
        }
    }
}

fn pointer_motion_has_effect(
    use_alt_buffer: bool,
    scrollback_lines: usize,
    mouse_tracking_enabled: bool,
    active_mouse_button: Option<u32>,
    drag_start: Option<(usize, usize)>,
    is_dragging_scrollbar: bool,
) -> bool {
    if drag_start.is_some() || is_dragging_scrollbar {
        return true;
    }

    if mouse_tracking_enabled && active_mouse_button.is_some() {
        return true;
    }

    !use_alt_buffer && scrollback_lines > 0
}

fn frame_wants_redraw(
    has_dirty_rows: bool,
    force_redraw: bool,
    scrollbar_wants_redraw: bool,
    scroll_animation_wants_redraw: bool,
    pane_animation_wants_redraw: bool,
    command_indicator_animation_wants_redraw: bool,
    cursor_trail_wants_redraw: bool,
) -> bool {
    has_dirty_rows
        || force_redraw
        || scrollbar_wants_redraw
        || scroll_animation_wants_redraw
        || pane_animation_wants_redraw
        || command_indicator_animation_wants_redraw
        || cursor_trail_wants_redraw
}

fn redraw_can_run_immediately(
    force_immediate_render: bool,
    force_redraw: bool,
    frame_ready: bool,
) -> bool {
    force_immediate_render || (force_redraw && frame_ready)
}

fn frame_callback_request_needed(frame_callback_pending: bool) -> bool {
    !frame_callback_pending
}

fn frame_should_mark_clean(needs_recreate: bool) -> bool {
    !needs_recreate
}

fn renderer_scroll_event(
    event: forge_pty::ScrollEvent,
) -> forge_renderer::grid_tessellator::ScrollEvent {
    let direction = match event.direction {
        forge_pty::ScrollDirection::Up => forge_renderer::grid_tessellator::ScrollDirection::Up,
        forge_pty::ScrollDirection::Down => forge_renderer::grid_tessellator::ScrollDirection::Down,
    };

    forge_renderer::grid_tessellator::ScrollEvent {
        direction,
        top: event.top,
        bottom: event.bottom,
        lines: event.lines,
        full_viewport: event.full_viewport,
    }
}

fn translate_visible_cursor(
    cursor: Option<(usize, usize)>,
    visible_col_start: usize,
    visible_row_start: usize,
    visible_cols: usize,
    visible_rows: usize,
) -> Option<(usize, usize)> {
    let (col, row) = cursor?;
    if col < visible_col_start
        || row < visible_row_start
        || col >= visible_col_start.saturating_add(visible_cols)
        || row >= visible_row_start.saturating_add(visible_rows)
    {
        return None;
    }

    Some((col - visible_col_start, row - visible_row_start))
}

fn translate_visible_selection(
    selection: Option<forge_core::cell::SelectionRange>,
    visible_col_start: usize,
    visible_row_start: usize,
    visible_cols: usize,
    visible_rows: usize,
) -> Option<forge_core::cell::SelectionRange> {
    let selection = selection?;
    let (mut start_row, mut start_col, mut end_row, mut end_col) = if selection.start_row
        < selection.end_row
        || (selection.start_row == selection.end_row && selection.start_col <= selection.end_col)
    {
        (
            selection.start_row,
            selection.start_col,
            selection.end_row,
            selection.end_col,
        )
    } else {
        (
            selection.end_row,
            selection.end_col,
            selection.start_row,
            selection.start_col,
        )
    };

    let visible_row_end = visible_row_start
        .saturating_add(visible_rows)
        .saturating_sub(1);
    let visible_col_end = visible_col_start
        .saturating_add(visible_cols)
        .saturating_sub(1);
    if end_row < visible_row_start
        || start_row > visible_row_end
        || visible_rows == 0
        || visible_cols == 0
    {
        return None;
    }

    start_row = start_row.max(visible_row_start);
    end_row = end_row.min(visible_row_end);

    if start_row == end_row {
        if end_col < visible_col_start || start_col > visible_col_end {
            return None;
        }
        start_col = start_col.max(visible_col_start);
        end_col = end_col.min(visible_col_end);
    } else {
        if start_row == selection.start_row {
            start_col = start_col.max(visible_col_start);
        } else {
            start_col = visible_col_start;
        }
        if end_row == selection.end_row {
            end_col = end_col.min(visible_col_end);
        } else {
            end_col = visible_col_end;
        }
    }

    Some(forge_core::cell::SelectionRange {
        start_row: start_row - visible_row_start,
        start_col: start_col - visible_col_start,
        end_row: end_row - visible_row_start,
        end_col: end_col - visible_col_start,
    })
}

fn pointer_layout_metrics(app_data: &AppData) -> (f64, f64, f64, f64) {
    let metrics = if let Some(m) = app_data.cached_grid_metrics {
        m
    } else {
        let cell_w = app_data
            .renderer
            .as_ref()
            .map(|r| r.cell_width as f64)
            .unwrap_or(10.0);
        let cell_h = app_data
            .renderer
            .as_ref()
            .map(|r| r.cell_height as f64)
            .unwrap_or(20.0);
        let win_w = app_data
            .wayland_state
            .window
            .as_ref()
            .map(|w| w.size.width as f64)
            .unwrap_or(800.0);
        let win_h = app_data
            .wayland_state
            .window
            .as_ref()
            .map(|w| w.size.height as f64)
            .unwrap_or(600.0);
        compute_grid_metrics(
            win_w,
            win_h,
            &app_data.config.window.padding,
            app_data.config.window.center_grid,
            &app_data.config.statusbar,
            app_data.sidebar.width_cols(),
            cell_w,
            cell_h,
        )
    };
    (
        metrics.effective_cell_w,
        metrics.effective_cell_h,
        metrics.pad_x,
        metrics.pad_y,
    )
}

fn recompute_grid_metrics_for_window(app_data: &AppData) -> Option<GridMetrics> {
    let renderer = app_data.renderer.as_ref()?;
    let cell_w = renderer.cell_width as f64;
    let cell_h = renderer.cell_height as f64;
    if cell_w <= 0.0 || cell_h <= 0.0 {
        return None;
    }
    let (width, height) = render_surface_size(app_data)?;

    Some(compute_grid_metrics(
        width,
        height,
        &app_data.config.window.padding,
        app_data.config.window.center_grid,
        &app_data.config.statusbar,
        app_data.sidebar.width_cols(),
        cell_w,
        cell_h,
    ))
}

fn render_surface_size(app_data: &AppData) -> Option<(f64, f64)> {
    if let Some(renderer) = app_data.renderer.as_ref() {
        let extent = renderer.swapchain.extent;
        if extent.width > 0 && extent.height > 0 {
            return Some((extent.width as f64, extent.height as f64));
        }
    }

    app_data
        .wayland_state
        .window
        .as_ref()
        .map(|window| (window.size.width as f64, window.size.height as f64))
}

fn apply_metrics_to_active_mux(app_data: &mut AppData, metrics: GridMetrics) {
    if let crate::mux::PaneRuntime::Scrolling(manager) = &mut app_data.pane_runtime {
        let changes = manager.set_active_viewport_size(metrics.cols, metrics.rows);
        if !changes.is_empty() {
            apply_scrolling_grid_changes(app_data, changes, metrics);
            app_data
                .pane_io
                .visible_gen
                .fetch_add(1, std::sync::atomic::Ordering::Release);
        }
        return;
    }

    let content_rect = crate::mux::PaneRect::new(
        metrics.pad_x as f32,
        metrics.pad_y as f32,
        (metrics.cols as f64 * metrics.effective_cell_w) as f32,
        (metrics.rows as f64 * metrics.effective_cell_h) as f32,
    );
    let params = crate::mux::LayoutParams::new(
        content_rect,
        metrics.effective_cell_w as f32,
        metrics.effective_cell_h as f32,
        app_data.config.window.gap as f32,
        app_data.effective_pane_padding(),
    );

    let changes = match app_data.tab_manager.active_mux_mut().relayout(params) {
        Ok(changes) => changes,
        Err(err) => {
            tracing::warn!(?err, "Mux relayout failed while applying grid metrics");
            return;
        }
    };
    if changes.is_empty() {
        return;
    }

    let batch: Vec<_> = changes
        .iter()
        .map(|change| {
            (
                change.pane_id,
                change.new_grid_size.cols,
                change.new_grid_size.rows,
            )
        })
        .collect();
    app_data
        .pane_io
        .send_ui_command(crate::mux::io::PtyWorkerCommand::BatchResizeReflow(
            batch, None,
        ));

    for change in &changes {
        if let Some(pty) = app_data
            .tab_manager
            .active_mux_mut()
            .panes
            .get_mut(&change.pane_id)
            .and_then(|pane| pane.pty.as_mut())
        {
            let px_w = (change.new_grid_size.cols as f64 * metrics.effective_cell_w) as u16;
            let px_h = (change.new_grid_size.rows as f64 * metrics.effective_cell_h) as u16;
            let _ = pty.resize(
                change.new_grid_size.cols as u16,
                change.new_grid_size.rows as u16,
                px_w,
                px_h,
            );
        }
    }
}

fn apply_authoritative_window_geometry(app_data: &mut AppData) -> bool {
    let Some(metrics) = recompute_grid_metrics_for_window(app_data) else {
        app_data.cached_grid_metrics = None;
        return false;
    };

    app_data.cached_grid_metrics = Some(metrics);
    apply_metrics_to_active_mux(app_data, metrics);
    app_data.statusbar.generation = app_data.statusbar.generation.wrapping_add(1);
    app_data
        .pane_io
        .visible_gen
        .fetch_add(1, std::sync::atomic::Ordering::Release);
    app_data.force_immediate_render = true;
    app_data.wayland_state.force_redraw = true;
    true
}

fn defer_startup_work(app_data: &mut AppData, work: DeferredStartupWork) {
    if app_data.deferred_startup_work.len() >= MAX_DEFERRED_STARTUP_WORK {
        tracing::warn!(
            max = MAX_DEFERRED_STARTUP_WORK,
            "Dropping startup action because the font-dependent work queue is full"
        );
        return;
    }

    app_data.deferred_startup_work.push(work);
}

fn replay_deferred_startup_work(app_data: &mut AppData) {
    if app_data.deferred_startup_work.is_empty() {
        return;
    }

    let deferred = std::mem::take(&mut app_data.deferred_startup_work);
    for work in deferred {
        match work {
            DeferredStartupWork::Split(split) => app_data.wayland_state.pending_splits.push(split),
            DeferredStartupWork::Action(action) => {
                app_data.wayland_state.pending_tab_actions.push(action)
            }
        }
    }
}

fn finish_startup_geometry(app_data: &mut AppData) {
    if app_data.startup_geometry_ready {
        replay_deferred_startup_work(app_data);
        return;
    }

    app_data.startup_geometry_ready = true;
    let _latest_deferred_size = app_data.pending_startup_window_size.take();
    apply_authoritative_window_geometry(app_data);
    replay_deferred_startup_work(app_data);
}

fn sync_scrolling_active_tab(app_data: &mut AppData) {
    if let Some(manager) = app_data.pane_runtime.scrolling_mut() {
        if manager.sync_active_tab_index(app_data.tab_manager.active_tab_index) {
            app_data
                .pane_io
                .visible_gen
                .fetch_add(1, std::sync::atomic::Ordering::Release);
        }
    }
}

fn focus_pane_direction(app_data: &mut AppData, dir: crate::mux::state::Direction) {
    let mut new_active_pane = None;
    let mut scrolling_focus = false;

    if let Some(manager) = app_data.pane_runtime.scrolling_mut() {
        scrolling_focus = true;
        if manager.focus_pane_direction(dir) {
            new_active_pane = manager.active_pane_id();
        }
    } else if app_data
        .tab_manager
        .active_mux_mut()
        .focus_pane_direction(dir)
    {
        new_active_pane = Some(app_data.tab_manager.active_mux().active_pane_id());
    }

    let Some(pane_id) = new_active_pane else {
        return;
    };

    if !app_data.pane_runtime.is_tiling()
        && app_data
            .tab_manager
            .active_mux()
            .get_pane(pane_id)
            .is_some()
    {
        app_data.tab_manager.active_mux_mut().active_pane = pane_id;
    }

    if scrolling_focus {
        app_data
            .pane_io
            .visible_gen
            .fetch_add(1, std::sync::atomic::Ordering::Release);
        app_data.force_immediate_render = true;
    }
    app_data.wayland_state.force_redraw = true;
}

fn move_scrolling_pane_direction(app_data: &mut AppData, dir: crate::mux::state::Direction) {
    if app_data.active_mouse_button.is_some() || app_data.dragging_scrolling_resize.is_some() {
        return;
    }
    let moved = app_data
        .pane_runtime
        .scrolling_mut()
        .map(|manager| manager.move_active_pane_direction(dir))
        .unwrap_or(false);
    if !moved {
        return;
    }

    app_data.hovered_scrolling_resize = None;
    app_data
        .pane_io
        .visible_gen
        .fetch_add(1, std::sync::atomic::Ordering::Release);
    app_data.force_immediate_render = true;
    app_data.wayland_state.force_redraw = true;
}

fn prepare_scrolling_transfer_animation_history(
    app_data: &mut AppData,
    tab_move: &crate::mux::runtime::ScrollingPaneTabMove,
    metrics: GridMetrics,
) {
    if matches!(
        app_data.config.render.pane_animation,
        forge_core::config_registry::PaneAnimationMode::None
    ) {
        return;
    }

    app_data.last_layout_rects.remove(&tab_move.pane_id);
    app_data.pane_animations.remove(&tab_move.pane_id);

    let destination_tab = app_data
        .tab_manager
        .tabs
        .iter()
        .find(|tab| tab.id == tab_move.destination_tab_id);
    for (pane_id, rect) in &tab_move.destination_previous_rects {
        let logical_rect = crate::mux::PaneRect::new(
            rect.col as f32 * metrics.effective_cell_w as f32,
            rect.row as f32 * metrics.effective_cell_h as f32,
            rect.cols as f32 * metrics.effective_cell_w as f32,
            rect.rows as f32 * metrics.effective_cell_h as f32,
        );
        let screen_rect = destination_tab
            .and_then(|tab| tab.mux.panes.get(pane_id))
            .map(|pane| pane.rect)
            .unwrap_or(logical_rect);
        app_data
            .last_layout_rects
            .insert(*pane_id, (logical_rect, screen_rect));
        app_data.pane_animations.remove(pane_id);
    }
}

fn move_scrolling_pane_to_tab(app_data: &mut AppData, destination_index: usize) {
    if app_data.active_mouse_button.is_some() || app_data.dragging_scrolling_resize.is_some() {
        return;
    }
    let Some(destination_tab_id) = app_data
        .tab_manager
        .tabs
        .get(destination_index)
        .map(|tab| tab.id)
    else {
        return;
    };
    let source_tab_id = app_data.tab_manager.active_tab().id;
    if source_tab_id == destination_tab_id {
        return;
    }
    let pane_id = app_data.active_pane_id();
    let source_has_only_scrolling_pane = app_data
        .pane_runtime
        .scrolling()
        .and_then(|manager| manager.active_tab())
        .map(|tab| tab.panes.pane_count() == 1)
        .unwrap_or(false);
    if app_data
        .tab_manager
        .active_mux()
        .floating_panes
        .contains(&pane_id)
        || (source_has_only_scrolling_pane
            && !app_data.tab_manager.active_mux().floating_panes.is_empty())
        || !app_data
            .tab_manager
            .active_mux()
            .panes
            .contains_key(&pane_id)
    {
        return;
    }

    let Some(tab_move) = app_data
        .pane_runtime
        .scrolling_mut()
        .and_then(|manager| manager.move_active_pane_to_tab(destination_tab_id))
    else {
        return;
    };
    if !app_data
        .tab_manager
        .move_detached_pane_to_tab(tab_move.pane_id, destination_tab_id)
    {
        tracing::error!(
            pane_id = tab_move.pane_id.get(),
            "Scrolling pane metadata moved but mux ownership transfer failed"
        );
        return;
    }

    if let Some(metrics) = app_data.cached_grid_metrics {
        prepare_scrolling_transfer_animation_history(app_data, &tab_move, metrics);
        apply_scrolling_grid_changes_to_tab(
            app_data,
            tab_move.source_tab_id,
            tab_move.source_grid_changes,
            metrics,
        );
        apply_scrolling_grid_changes_to_tab(
            app_data,
            tab_move.destination_tab_id,
            tab_move.destination_grid_changes,
            metrics,
        );
    }
    sync_scrolling_active_tab(app_data);
    app_data.context_menu = None;
    app_data.hovered_scrolling_resize = None;
    app_data.dragging_scrolling_resize = None;
    app_data
        .pane_io
        .visible_gen
        .fetch_add(1, std::sync::atomic::Ordering::Release);
    app_data.force_immediate_render = true;
    app_data.wayland_state.force_redraw = true;
}

fn command_completion_tracking_enabled(
    config: &forge_core::config_registry::CommandCompletionIndicatorConfig,
) -> bool {
    !matches!(
        config.mode,
        forge_core::config_registry::CommandCompletionIndicatorMode::Disabled
    )
}

fn command_completion_tracking_enabled_for_config(
    config: &forge_core::config_registry::ForgeConfig,
) -> bool {
    config.shell.integration_enabled
        && command_completion_tracking_enabled(&config.command_completion_indicator)
}

fn pane_zoomed_for_indicator(app_data: &AppData, pane_id: crate::mux::PaneId) -> bool {
    app_data.tab_manager.tabs.iter().any(|tab| {
        tab.mux.is_zoomed() && tab.mux.visible_pane_ids().first().copied() == Some(pane_id)
    })
}

fn pane_tab_location(
    app_data: &AppData,
    pane_id: crate::mux::PaneId,
) -> Option<(usize, crate::mux::TabId)> {
    app_data
        .tab_manager
        .tabs
        .iter()
        .enumerate()
        .find_map(|(index, tab)| {
            tab.mux
                .panes
                .contains_key(&pane_id)
                .then_some((index, tab.id))
        })
}

fn should_show_command_completion_indicator(
    app_data: &AppData,
    event: &crate::mux::io::CommandCompletionEvent,
) -> bool {
    let config = &app_data.config.command_completion_indicator;
    let Some((tab_index, _)) = pane_tab_location(app_data, event.pane_id) else {
        return command_completion_indicator_should_show(
            config,
            event.duration_ms,
            false,
            false,
            false,
            false,
            app_data.wayland_state.is_activated,
        );
    };
    let tab_is_inactive = tab_index != app_data.tab_manager.active_tab_index;
    let pane_is_unfocused = tab_is_inactive || event.pane_id != app_data.active_pane_id();
    command_completion_indicator_should_show(
        config,
        event.duration_ms,
        true,
        pane_zoomed_for_indicator(app_data, event.pane_id),
        tab_is_inactive,
        pane_is_unfocused,
        app_data.wayland_state.is_activated,
    )
}

fn command_completion_indicator_should_show(
    config: &forge_core::config_registry::CommandCompletionIndicatorConfig,
    duration_ms: u64,
    pane_exists: bool,
    pane_zoomed: bool,
    tab_is_inactive: bool,
    pane_is_unfocused: bool,
    window_is_activated: bool,
) -> bool {
    if !command_completion_tracking_enabled(config) {
        return false;
    }
    if duration_ms < config.minimum_duration_ms || !pane_exists {
        return false;
    }
    if matches!(
        config.mode,
        forge_core::config_registry::CommandCompletionIndicatorMode::DisabledOnZoom
    ) && pane_zoomed
    {
        return false;
    }

    tab_is_inactive || pane_is_unfocused || !window_is_activated
}

fn command_completion_indicator_expiry(
    config: &forge_core::config_registry::CommandCompletionIndicatorConfig,
    now: std::time::Instant,
) -> Option<std::time::Instant> {
    match config.dismissal {
        forge_core::config_registry::CommandCompletionIndicatorDismissal::Timeout => Some(
            now + COMMAND_INDICATOR_CIRCLE_HOLD
                + COMMAND_INDICATOR_EXPAND
                + std::time::Duration::from_millis(config.display_duration_ms)
                + COMMAND_INDICATOR_CONTRACT,
        ),
        forge_core::config_registry::CommandCompletionIndicatorDismissal::OnInteraction => None,
    }
}

fn process_command_completion_events(app_data: &mut AppData) {
    if !app_data.pane_io.has_command_events() {
        return;
    }

    let mut changed = false;
    let now = std::time::Instant::now();
    for event in app_data.pane_io.command_events() {
        let event_tab_id = pane_tab_location(app_data, event.pane_id).map(|(_, tab_id)| tab_id);
        if should_show_command_completion_indicator(app_data, &event) {
            app_data.command_completion_generation =
                app_data.command_completion_generation.wrapping_add(1);
            app_data.command_completion_indicators.insert(
                event.pane_id,
                CommandCompletionIndicator {
                    success: event.exit_code == 0,
                    program_name: std::sync::Arc::from(
                        event.program_name.as_deref().unwrap_or("command"),
                    ),
                    exit_text: (event.exit_code != 0)
                        .then(|| std::sync::Arc::from(format!("[{}]", event.exit_code))),
                    generation: app_data.command_completion_generation,
                    tab_id: event_tab_id.unwrap_or_else(|| app_data.tab_manager.active_tab().id),
                    shown_at: now,
                    opening_frame_presented: false,
                    dismissed_at: None,
                    expires_at: command_completion_indicator_expiry(
                        &app_data.config.command_completion_indicator,
                        now,
                    ),
                },
            );
            changed = true;
        } else if app_data
            .command_completion_indicators
            .remove(&event.pane_id)
            .is_some()
        {
            changed = true;
        }
    }

    if changed {
        app_data.force_immediate_render = true;
        app_data.wayland_state.force_redraw = true;
    }
}

fn expire_command_completion_indicators(app_data: &mut AppData, now: std::time::Instant) {
    if app_data.command_completion_indicators.is_empty() {
        return;
    }

    let before = app_data.command_completion_indicators.len();
    app_data
        .command_completion_indicators
        .retain(|_, indicator| {
            indicator
                .expires_at
                .is_none_or(|expires_at| expires_at > now)
        });
    if app_data.command_completion_indicators.len() != before {
        app_data.force_immediate_render = true;
        app_data.wayland_state.force_redraw = true;
    }
}

fn next_command_completion_indicator_timeout(
    app_data: &AppData,
    now: std::time::Instant,
) -> Option<std::time::Duration> {
    let config = &app_data.config.command_completion_indicator;
    app_data
        .command_completion_indicators
        .values()
        .filter_map(|indicator| {
            let expand_at = indicator.shown_at + COMMAND_INDICATOR_CIRCLE_HOLD;
            if now < expand_at {
                return Some(expand_at);
            }
            let expanded_at = expand_at + COMMAND_INDICATOR_EXPAND;
            let dismiss_at = indicator.dismissed_at.or_else(|| {
                matches!(
                    config.dismissal,
                    forge_core::config_registry::CommandCompletionIndicatorDismissal::Timeout
                )
                .then(|| expanded_at + std::time::Duration::from_millis(config.display_duration_ms))
            });
            match (dismiss_at, indicator.expires_at) {
                (Some(dismiss_at), _) if now < dismiss_at => Some(dismiss_at),
                (_, expires_at) => expires_at,
            }
        })
        .map(|deadline| deadline.saturating_duration_since(now))
        .min()
}

fn command_completion_indicator_animation_active(
    app_data: &AppData,
    now: std::time::Instant,
) -> bool {
    latest_command_completion_indicator(app_data).is_some_and(|indicator| {
        command_indicator_needs_frame(
            indicator,
            &app_data.config.command_completion_indicator,
            now,
        )
    })
}

fn command_indicator_needs_frame(
    indicator: &CommandCompletionIndicator,
    config: &forge_core::config_registry::CommandCompletionIndicatorConfig,
    now: std::time::Instant,
) -> bool {
    command_indicator_visual(indicator, config, now).animating
        || (!indicator.opening_frame_presented
            && now >= indicator.shown_at + COMMAND_INDICATOR_CIRCLE_HOLD + COMMAND_INDICATOR_EXPAND)
}

fn latest_command_completion_indicator(app_data: &AppData) -> Option<&CommandCompletionIndicator> {
    if app_data.active_modal.is_some()
        || !command_completion_tracking_enabled(&app_data.config.command_completion_indicator)
    {
        return None;
    }

    let suppressed_zoom_pane = matches!(
        app_data.config.command_completion_indicator.mode,
        forge_core::config_registry::CommandCompletionIndicatorMode::DisabledOnZoom
    )
    .then(|| {
        if app_data.pane_runtime.is_tiling() {
            app_data
                .tab_manager
                .active_mux()
                .is_zoomed()
                .then_some(app_data.active_pane_id())
        } else {
            app_data.pane_runtime.scrolling().and_then(|manager| {
                manager
                    .is_zoomed()
                    .then(|| manager.active_pane_id())
                    .flatten()
            })
        }
    })
    .flatten();

    app_data
        .command_completion_indicators
        .iter()
        .filter(|(pane_id, _)| suppressed_zoom_pane != Some(**pane_id))
        .filter(|(_, indicator)| {
            app_data
                .tab_manager
                .tabs
                .iter()
                .any(|tab| tab.id == indicator.tab_id)
        })
        .max_by_key(|(_, indicator)| indicator.generation)
        .map(|(_, indicator)| indicator)
}

fn clear_command_completion_indicators_for_user_interaction(app_data: &mut AppData) {
    if !matches!(
        app_data.config.command_completion_indicator.dismissal,
        forge_core::config_registry::CommandCompletionIndicatorDismissal::OnInteraction
    ) {
        return;
    }
    if app_data.command_completion_indicators.is_empty() {
        return;
    }

    let now = std::time::Instant::now();
    let mut changed = false;
    for indicator in app_data.command_completion_indicators.values_mut() {
        if indicator.dismissed_at.is_none() {
            let expanded_at =
                indicator.shown_at + COMMAND_INDICATOR_CIRCLE_HOLD + COMMAND_INDICATOR_EXPAND;
            let dismiss_at = now.max(expanded_at);
            indicator.dismissed_at = Some(dismiss_at);
            indicator.expires_at = Some(dismiss_at + COMMAND_INDICATOR_CONTRACT);
            changed = true;
        }
    }
    if changed {
        app_data.force_immediate_render = true;
        app_data.wayland_state.force_redraw = true;
    }
}

fn command_completion_indicator_rect(
    metrics: GridMetrics,
    content_cols: usize,
    horizontal_padding: f32,
    vertical_padding: f32,
) -> Option<forge_renderer::renderer::PaneRenderRect> {
    if metrics.cols == 0 || metrics.rows == 0 || content_cols == 0 {
        return None;
    }

    let cell_w = metrics.effective_cell_w as f32;
    let cell_h = metrics.effective_cell_h as f32;
    let viewport_x = metrics.pad_x as f32;
    let viewport_y = metrics.pad_y as f32;
    let viewport_width = metrics.cols as f32 * cell_w;
    let viewport_height = metrics.rows as f32 * cell_h;
    let width = (content_cols as f32 * cell_w + horizontal_padding * 2.0).min(viewport_width);
    let height = (cell_h + vertical_padding * 2.0).min(viewport_height);
    let bottom_inset = (cell_h * 0.5).min((viewport_height - height).max(0.0));
    Some(forge_renderer::renderer::PaneRenderRect {
        x: viewport_x + ((viewport_width - width) * 0.5).max(0.0),
        y: viewport_y + (viewport_height - height - bottom_inset).max(0.0),
        width,
        height,
    })
}

fn command_completion_indicator_layout(
    metrics: GridMetrics,
    indicator: &CommandCompletionIndicator,
    theme: &forge_core::config_registry::ThemeConfig,
    config: &forge_core::config_registry::CommandCompletionIndicatorConfig,
    now: std::time::Instant,
) -> Option<forge_renderer::renderer::CommandCompletionIndicatorRenderData> {
    if metrics.cols == 0 || metrics.rows == 0 {
        return None;
    }

    let cell_w = metrics.effective_cell_w as f32;
    let cell_h = metrics.effective_cell_h as f32;
    let viewport_width = metrics.cols as f32 * cell_w;
    let horizontal_padding = (cell_w * 0.75).min(((viewport_width - cell_w) * 0.5).max(0.0));
    let vertical_padding = cell_h * 0.25;
    let available_content_cols =
        ((viewport_width - horizontal_padding * 2.0).max(cell_w) / cell_w).floor() as usize;

    let mut exit_text = indicator.exit_text.clone();
    let label_cols = available_content_cols.saturating_sub(2);
    if let Some(text) = exit_text.as_ref() {
        let exit_cols = text.chars().count();
        if exit_cols > label_cols {
            exit_text = Some(std::sync::Arc::from(
                text.chars().take(label_cols).collect::<String>(),
            ));
        }
    }
    let exit_cols = exit_text
        .as_ref()
        .map(|text| text.chars().count())
        .unwrap_or(0);
    let max_command_cols = label_cols.saturating_sub(exit_cols);
    let command = if indicator.program_name.chars().count() <= max_command_cols {
        indicator.program_name.clone()
    } else {
        std::sync::Arc::from(
            indicator
                .program_name
                .chars()
                .take(max_command_cols)
                .collect::<String>(),
        )
    };
    let command_cols = command.chars().count();
    let total_cols = (2 + command_cols + exit_cols).min(available_content_cols.max(1));
    let full_rect = command_completion_indicator_rect(
        metrics,
        total_cols,
        horizontal_padding,
        vertical_padding,
    )?;
    let visual = command_indicator_visual(indicator, config, now);
    let circle_width = full_rect.height.min(full_rect.width);
    let initial_x = full_rect.x + (full_rect.width - circle_width) * 0.5;
    let rect = forge_renderer::renderer::PaneRenderRect {
        x: initial_x + (full_rect.x - initial_x) * visual.expansion,
        y: full_rect.y,
        width: circle_width + (full_rect.width - circle_width) * visual.expansion,
        height: full_rect.height,
    };
    let final_dot_center = full_rect.x + horizontal_padding + cell_w * 0.5;
    let dot_center_x = initial_x
        + circle_width * 0.5
        + (final_dot_center - (initial_x + circle_width * 0.5)) * visual.expansion;
    let (success_fg, failure_fg) = command_completion_indicator_colors(theme);
    let background_color =
        (!config.transparent).then(|| color_to_render_array(theme.parsed_popup_background));

    Some(
        forge_renderer::renderer::CommandCompletionIndicatorRenderData {
            rect,
            cell_width: cell_w,
            cell_height: cell_h,
            content_x: full_rect.x + horizontal_padding,
            content_y: full_rect.y + vertical_padding,
            dot_center_x,
            corner_radius: rect.height * 0.5,
            background_color,
            dot_color: color_to_render_array(if indicator.success {
                success_fg
            } else {
                failure_fg
            }),
            text_color: color_to_render_array(theme.parsed_foreground),
            failure_color: color_to_render_array(failure_fg),
            command,
            exit_text,
        },
    )
}

fn command_completion_indicator_colors(
    theme: &forge_core::config_registry::ThemeConfig,
) -> (forge_core::color::Color, forge_core::color::Color) {
    (theme.parsed_ansi_colors[2], theme.parsed_ansi_colors[1])
}

fn color_to_render_array(color: forge_core::color::Color) -> [f32; 4] {
    let color = color.to_srgb_linear();
    [color.r, color.g, color.b, color.a]
}

fn runtime_reload_config(
    current: &forge_core::config_registry::ForgeConfig,
    mut requested: forge_core::config_registry::ForgeConfig,
) -> forge_core::config_registry::ForgeConfig {
    if requested.font.family != current.font.family
        || requested.font.bold_family != current.font.bold_family
        || requested.font.italic_family != current.font.italic_family
        || requested.font.bold_italic_family != current.font.bold_italic_family
        || requested.font.size != current.font.size
        || requested.font.nerd_fonts != current.font.nerd_fonts
    {
        tracing::warn!(
            "Config option font face/size requires restart; keeping current font settings."
        );
        requested.font.family = current.font.family.clone();
        requested.font.bold_family = current.font.bold_family.clone();
        requested.font.italic_family = current.font.italic_family.clone();
        requested.font.bold_italic_family = current.font.bold_italic_family.clone();
        requested.font.size = current.font.size;
        requested.font.nerd_fonts = current.font.nerd_fonts;
    }

    if requested.shell != current.shell {
        tracing::warn!("Config option shell requires restart; keeping current shell settings.");
        requested.shell = current.shell.clone();
    }

    if requested.panes != current.panes {
        tracing::warn!(
            "Config option panes.mode requires restart; keeping current pane manager mode."
        );
        requested.panes = current.panes.clone();
    }

    if requested.window.width != current.window.width
        || requested.window.height != current.window.height
        || requested.window.title != current.window.title
        || requested.window.decorations != current.window.decorations
        || requested.window.center_on_launch != current.window.center_on_launch
    {
        tracing::warn!(
            "Config option window size/title/decorations/center_on_launch requires restart; keeping current values."
        );
        requested.window.width = current.window.width;
        requested.window.height = current.window.height;
        requested.window.title = current.window.title.clone();
        requested.window.decorations = current.window.decorations;
        requested.window.center_on_launch = current.window.center_on_launch;
    }

    requested
}

#[allow(clippy::too_many_arguments)]
pub fn run_event_loop(
    mut event_loop: EventLoop<AppData>,
    wayland_state: WaylandState,
    event_queue: EventQueue<WaylandState>,
    mux: crate::mux::MuxState,
    initial_vte_processor: forge_pty::VteProcessor,
    initial_screen_buffer: forge_pty::ScreenBuffer,
    key_receiver: std::sync::mpsc::Receiver<Vec<u8>>,
    pointer_receiver: std::sync::mpsc::Receiver<crate::wayland::connection::PointerEvent>,
    paste_receiver: std::sync::mpsc::Receiver<Vec<u8>>,
    config: forge_core::config_registry::ForgeConfig,
    renderer: Option<forge_renderer::Renderer>,
    font_atlas_receiver: Option<std::sync::mpsc::Receiver<forge_renderer::font::FontData>>,
    config_rx: Option<crossbeam_channel::Receiver<forge_config::ConfigUpdate>>,
    startup_start: std::time::Instant,
) -> Result<()> {
    let loop_handle = event_loop.handle();
    let loop_signal = event_loop.get_signal();

    let queue_handle = event_queue.handle();

    // We can't flush yet, but `wayland_state` is updated. We'll set needs_flush below.
    let initial_window_size =
        wayland_state
            .window
            .as_ref()
            .map(|w| w.size)
            .unwrap_or(forge_core::geometry::Size {
                width: 0,
                height: 0,
            });
    let mut wayland_state = wayland_state;
    wayland_state.keybindings = config.keybindings.clone();
    wayland_state.hide_mouse_when_typing = config.behavior.hide_mouse_when_typing;
    // No frame callback has been requested yet. The first rendered frame starts
    // the callback chain; keeping this truthful prevents both duplicate callbacks
    // during immediate redraws and a false pending state at startup.
    wayland_state.frame_callback_pending = false;
    wayland_state.needs_flush = false;
    let _ = wayland_state.conn.flush();

    // Give the clipboard manager a clone of the loop_signal
    if let Some(clip) = wayland_state.clipboard.as_mut() {
        clip.loop_signal = Some(loop_signal.clone());
    }

    let source = WaylandSource::new(wayland_state.conn.clone(), event_queue);
    loop_handle
        .insert_source(source, |(), queue, app_data| {
            queue.dispatch_pending(&mut app_data.wayland_state)
        })
        .map_err(|e| ForgeError::Wayland(e.to_string()))?;

    let tab_manager = crate::mux::TabManager::new(mux);
    let fallback_pane_size = tab_manager.active_mux().active_pane().grid_size;
    let initial_sidebar = crate::sidebar::SidebarState::default();
    let initial_runtime_metrics = renderer.as_ref().map(|renderer| {
        let extent = renderer.swapchain.extent;
        compute_grid_metrics(
            extent.width as f64,
            extent.height as f64,
            &config.window.padding,
            config.window.center_grid,
            &config.statusbar,
            initial_sidebar.width_cols(),
            renderer.cell_width as f64,
            renderer.cell_height as f64,
        )
    });
    let pane_runtime = crate::mux::PaneRuntime::from_config(
        config.panes.mode,
        config.panes.scroll_animation_duration_ms,
        &tab_manager,
        initial_runtime_metrics
            .map(|metrics| metrics.cols)
            .unwrap_or(fallback_pane_size.cols),
        initial_runtime_metrics
            .map(|metrics| metrics.rows)
            .unwrap_or(fallback_pane_size.rows),
        fallback_pane_size,
    );
    let startup_geometry_ready = renderer
        .as_ref()
        .map(|renderer| renderer.has_real_font_metrics())
        .unwrap_or(true)
        || font_atlas_receiver.is_none();
    let mut app_data = AppData {
        wayland_state,
        loop_signal: loop_signal.clone(),
        tab_manager,
        pane_runtime,
        pane_io: crate::mux::PaneIoRegistry::new(
            loop_signal,
            command_completion_tracking_enabled_for_config(&config),
        )?,
        command_completion_indicators: std::collections::HashMap::new(),
        command_completion_generation: 0,
        selection_copies_in_flight: 0,
        pastes_waiting_for_copy: 0,

        key_receiver,
        pointer_receiver,
        paste_receiver,
        config: config.clone(),
        renderer,
        queue_handle,
        drag_start: None,
        active_mouse_button: None,
        last_mouse_col: 0,
        last_mouse_row: 0,
        pointer_x: 0.0,
        pointer_y: 0.0,
        scroll_accum: 0.0,
        last_window_size: initial_window_size,
        startup_geometry_ready,
        pending_startup_window_size: None,
        deferred_startup_work: Vec::new(),
        font_atlas_receiver,
        cursor_visible_phase: true,
        last_cursor_blink: std::time::Instant::now(),
        config_rx,
        last_scrollbar_state: None,
        last_mouse_activity: std::time::Instant::now(),
        mouse_started_moving: std::time::Instant::now(),
        is_hovering_edge: false,
        current_thumb_width: 5.0,
        current_track_opacity: 0.0,
        current_thumb_opacity: 0.0,
        is_dragging_scrollbar: false,
        scrollbar_drag_offset_y: 0.0,
        startup_start,
        first_vulkan_text_frame_logged: false,
        cached_bg_color: config.theme.parsed_background.to_srgb_linear(),
        cached_cursor_color: config.theme.parsed_cursor_color.to_srgb_linear(),
        cached_selection_bg_color: config.theme.parsed_selection_bg.to_srgb_linear(),
        cached_statusbar_hover_color: compute_statusbar_hover_color(&config.statusbar.bg_color),
        cached_grid_metrics: None,
        last_snapshot_ids: std::collections::HashMap::new(),
        last_visible_gen: u64::MAX,
        force_immediate_render: false,
        hovered_split: None,
        dragging_split: None,
        hovered_scrolling_resize: None,
        dragging_scrolling_resize: None,
        is_hovering_statusbar: false,
        statusbar: crate::statusbar::StatusBarState::default(),
        sidebar: initial_sidebar,
        context_menu: None,
        active_modal: None,
        modal_generation: 1,
        pane_animations: std::collections::HashMap::new(),
        last_layout_rects: std::collections::HashMap::new(),
        closing_panes: Vec::new(),
    };

    let active_pane = app_data.tab_manager.active_mux().active_pane;
    let fd_clone = app_data
        .tab_manager
        .active_mux()
        .panes
        .get(&active_pane)
        .unwrap()
        .pty
        .as_ref()
        .unwrap()
        .master_fd
        .try_clone()
        .unwrap();
    let initial_snapshot = app_data
        .tab_manager
        .active_mux()
        .panes
        .get(&active_pane)
        .unwrap()
        .snapshot
        .clone();

    app_data
        .pane_io
        .register_pane(
            active_pane,
            fd_clone,
            initial_vte_processor,
            initial_screen_buffer,
            initial_snapshot,
        )
        .unwrap();

    while app_data.wayland_state.running {
        process_command_completion_events(&mut app_data);
        expire_command_completion_indicators(&mut app_data, std::time::Instant::now());

        // FIX 4: Use a u64 generation counter instead of allocating a HashSet every loop tick.
        // visible_gen increments whenever a structural change (tab switch, pane split/close) occurs.
        let current_gen = app_data
            .pane_io
            .visible_gen
            .load(std::sync::atomic::Ordering::Acquire);
        if current_gen != app_data.last_visible_gen {
            let current_visible: std::collections::HashSet<_> =
                app_data.visible_pane_ids().into_iter().collect();
            app_data
                .pane_io
                .send_ui_command(crate::mux::io::PtyWorkerCommand::SetVisiblePanes(
                    current_visible,
                ));
            app_data.last_visible_gen = current_gen;
        }

        // FIX 3: Use AtomicBool fast path — no RwLock read unless something actually exited.
        let exited = if app_data.pane_io.has_any_exited() {
            app_data.pane_io.exited_panes()
        } else {
            vec![]
        };
        if !exited.is_empty() {
            for exited_pane in exited {
                let mut closed_tab_idx = None;
                let mut needs_relayout_in_tab = None;

                let scrolling_mode = !app_data.pane_runtime.is_tiling();
                for (i, tab) in app_data.tab_manager.tabs.iter_mut().enumerate() {
                    let remove_result = if scrolling_mode {
                        tab.mux.remove_detached_pane(exited_pane)
                    } else {
                        tab.mux.remove_pane(exited_pane)
                    };
                    match remove_result {
                        crate::mux::state::RemovePaneResult::RemovedLastPane => {
                            closed_tab_idx = Some(i);
                            break;
                        }
                        crate::mux::state::RemovePaneResult::Removed { .. } => {
                            needs_relayout_in_tab = Some(i);
                            break;
                        }
                        crate::mux::state::RemovePaneResult::MissingPane => {}
                    }
                }

                if let Some(idx) = closed_tab_idx {
                    if app_data.tab_manager.tabs.len() == 1 {
                        app_data.wayland_state.running = false;
                        break;
                    } else {
                        let closed_tab_id = app_data.tab_manager.tabs[idx].id;
                        app_data.tab_manager.tabs.remove(idx);
                        if idx < app_data.tab_manager.active_tab_index {
                            app_data.tab_manager.active_tab_index -= 1;
                        } else if app_data.tab_manager.active_tab_index
                            >= app_data.tab_manager.tabs.len()
                        {
                            app_data.tab_manager.active_tab_index =
                                app_data.tab_manager.tabs.len() - 1;
                        }
                        if let Some(scrolling) = app_data.pane_runtime.scrolling_mut() {
                            scrolling.remove_tab(closed_tab_id);
                            scrolling.sync_active_tab_index(app_data.tab_manager.active_tab_index);
                        }
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                } else if let Some(idx) = needs_relayout_in_tab {
                    if let Some(scrolling) = app_data.pane_runtime.scrolling_mut() {
                        let removal = scrolling.remove_pane_any_with_changes(exited_pane);
                        let grid_changes = removal.grid_changes;
                        if let Some(active_pane) = scrolling.active_pane_id() {
                            app_data.tab_manager.tabs[idx].mux.active_pane = active_pane;
                        }
                        if let Some(metrics) = app_data.cached_grid_metrics {
                            apply_scrolling_grid_changes(&mut app_data, grid_changes, metrics);
                        }
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                        continue;
                    }
                    if let Some(metrics) = app_data.cached_grid_metrics {
                        let new_content_rect = crate::mux::PaneRect::new(
                            metrics.pad_x as f32,
                            metrics.pad_y as f32,
                            (metrics.cols as f64 * metrics.effective_cell_w) as f32,
                            (metrics.rows as f64 * metrics.effective_cell_h) as f32,
                        );
                        let layout_params = crate::mux::LayoutParams::new(
                            new_content_rect,
                            metrics.effective_cell_w as f32,
                            metrics.effective_cell_h as f32,
                            app_data.config.window.gap as f32,
                            app_data.effective_pane_padding(),
                        );
                        if let Ok(changes) =
                            app_data.tab_manager.tabs[idx].mux.relayout(layout_params)
                        {
                            for change in changes {
                                // FIX #2: Reflow the screen buffer directly on the main thread.
                                // This avoids an IPC round-trip (send → worker wakeup → reflow → ack → recv)
                                // and instead executes the reflow synchronously here, atomically publishing
                                // the reflowed snapshot into the ArcSwap before the next GPU frame.
                                if let Some(pane) = app_data.tab_manager.tabs[idx]
                                    .mux
                                    .panes
                                    .get_mut(&change.pane_id)
                                {
                                    // The ScreenBuffer lives inside the PTY IO worker thread — we cannot
                                    // access it directly. Instead, build a *display-only* snapshot with
                                    // the new dimensions by reflowing the existing snapshot grid.
                                    // Then send the resize to the worker asynchronously (no sync ack needed).
                                    // The worker will reflow its own copy in the background; by then we have
                                    // already rendered the correct-sized grid from our eagerly reflowed snapshot.
                                    let px_w = (change.new_grid_size.cols as f64
                                        * metrics.effective_cell_w)
                                        as u16;
                                    let px_h = (change.new_grid_size.rows as f64
                                        * metrics.effective_cell_h)
                                        as u16;
                                    if let Some(pty) = pane.pty.as_mut() {
                                        let _ = pty.resize(
                                            change.new_grid_size.cols as u16,
                                            change.new_grid_size.rows as u16,
                                            px_w,
                                            px_h,
                                        );
                                    }
                                }
                                // Send async reflow to worker (no blocking ack — worker catches up in background).
                                app_data.pane_io.send_ui_command(
                                    crate::mux::io::PtyWorkerCommand::BatchResizeReflow(
                                        vec![(
                                            change.pane_id,
                                            change.new_grid_size.cols,
                                            change.new_grid_size.rows,
                                        )],
                                        None, // async — no sync handshake needed
                                    ),
                                );
                            }
                        }
                        // FIX #1: bypass the frame_ready gate — render this frame immediately
                        // without waiting for the compositor's next vsync callback.
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                }
            }
        }

        if !app_data.wayland_state.running {
            break;
        }
        // tracing::trace!("Event loop top");

        let mut timeout = None;
        if let Some(repeating) = &app_data.wayland_state.repeating_key {
            let now = std::time::Instant::now();
            if now >= repeating.next_repeat_time {
                timeout = Some(std::time::Duration::from_millis(0));
            } else {
                timeout = Some(repeating.next_repeat_time - now);
            }
        }

        let cursor_blink = active_cursor_blink_enabled(&app_data);
        if cursor_blink {
            let blink_rate = app_data.config.cursor.blink_rate_ms as u128;
            let elapsed = app_data.last_cursor_blink.elapsed().as_millis();
            let blink_timeout = if elapsed < blink_rate {
                std::time::Duration::from_millis((blink_rate - elapsed) as u64)
            } else {
                std::time::Duration::from_millis(0)
            };
            if let Some(t) = timeout {
                timeout = Some(t.min(blink_timeout));
            } else {
                timeout = Some(blink_timeout);
            }
        }
        if let Some(indicator_timeout) =
            next_command_completion_indicator_timeout(&app_data, std::time::Instant::now())
        {
            timeout = Some(timeout.map_or(indicator_timeout, |t| t.min(indicator_timeout)));
        }
        if let Some(trail_timeout) = app_data.renderer.as_ref().and_then(|renderer| {
            renderer.cursor_trail_next_wakeup(std::time::Instant::now())
        }) {
            timeout = Some(timeout.map_or(trail_timeout, |t| t.min(trail_timeout)));
        }
        if redraw_can_run_immediately(
            app_data.force_immediate_render,
            app_data.wayland_state.force_redraw,
            app_data.wayland_state.frame_ready,
        ) {
            timeout = Some(std::time::Duration::ZERO);
        }

        event_loop
            .dispatch(timeout, &mut app_data)
            .map_err(|e| ForgeError::Other(e.to_string()))?;
        finish_selection_copy_requests(&mut app_data);
        expire_command_completion_indicators(&mut app_data, std::time::Instant::now());
        let use_alt_buffer = app_data
            .tab_manager
            .active_mux()
            .panes
            .get(&app_data.tab_manager.active_mux().active_pane)
            .unwrap()
            .snapshot
            .load()
            .use_alt_buffer;
        if app_data.wayland_state.is_alt_buffer != use_alt_buffer {
            app_data.wayland_state.is_alt_buffer = use_alt_buffer;
            if use_alt_buffer {
                let had_scrollbar_state = app_data.is_hovering_edge
                    || app_data.is_dragging_scrollbar
                    || app_data.current_thumb_opacity > 0.01
                    || app_data.current_track_opacity > 0.01;
                app_data.is_hovering_edge = false;
                app_data.is_dragging_scrollbar = false;
                app_data.current_thumb_opacity = 0.0;
                app_data.current_track_opacity = 0.0;
                if had_scrollbar_state {
                    app_data.wayland_state.force_redraw = true;
                }
            }
            // The cursor shape will naturally update on the next pointer motion or enter event.
        }
        if cursor_blink {
            let blink_rate = app_data.config.cursor.blink_rate_ms as u128;
            if app_data.last_cursor_blink.elapsed().as_millis() >= blink_rate {
                app_data.cursor_visible_phase = !app_data.cursor_visible_phase;
                app_data.last_cursor_blink = std::time::Instant::now();
                app_data.wayland_state.force_redraw = true;
            }
        } else {
            if !app_data.cursor_visible_phase {
                app_data.cursor_visible_phase = true;
                app_data.wayland_state.force_redraw = true;
            }
        }

        if !app_data.wayland_state.running {
            app_data.loop_signal.stop();
            break;
        }

        if app_data.active_modal.is_some() {
            app_data.wayland_state.repeating_key = None;
            app_data.wayland_state.pending_splits.clear();
            app_data.wayland_state.pending_tab_actions.clear();
            app_data.wayland_state.pending_copy_serial = None;

            while let Ok(input) = app_data.key_receiver.try_recv() {
                clear_command_completion_indicators_for_user_interaction(&mut app_data);
                let action = {
                    let modal = app_data.active_modal.as_mut().unwrap();
                    modal.handle_key_bytes(&input)
                };
                handle_modal_action(&mut app_data, action);
                if app_data.active_modal.is_none() || !app_data.wayland_state.running {
                    break;
                }
            }

            while app_data.paste_receiver.try_recv().is_ok() {}

            if !app_data.wayland_state.running {
                app_data.loop_signal.stop();
                break;
            }
        }

        // Process repeating key
        let mut repeated_input_pane = None;
        if let Some(repeating) = &mut app_data.wayland_state.repeating_key {
            let now = std::time::Instant::now();
            if now >= repeating.next_repeat_time {
                let active_pane = app_data.tab_manager.active_mux().active_pane;
                app_data
                    .pane_io
                    .send_ui_command(crate::mux::io::PtyWorkerCommand::Write(
                        active_pane,
                        repeating.bytes.clone(),
                    ));
                repeated_input_pane = Some(active_pane);
                if let Some((rate, _)) = app_data.wayland_state.repeat_info {
                    if rate > 0 {
                        repeating.next_repeat_time =
                            now + std::time::Duration::from_millis(1000 / rate as u64);
                    }
                }

                // Typing trap
                app_data.cursor_visible_phase = true;
                app_data.last_cursor_blink = std::time::Instant::now();
                app_data.wayland_state.force_redraw = true;
            }
        }
        if repeated_input_pane.is_some() {
            clear_command_completion_indicators_for_user_interaction(&mut app_data);
        }

        while let Ok(input) = app_data.key_receiver.try_recv() {
            let active_pane = app_data.tab_manager.active_mux().active_pane;
            clear_command_completion_indicators_for_user_interaction(&mut app_data);
            app_data
                .pane_io
                .send_ui_command(crate::mux::io::PtyWorkerCommand::Write(
                    active_pane,
                    input.to_vec(),
                ));

            // Typing trap
            app_data.cursor_visible_phase = true;
            app_data.last_cursor_blink = std::time::Instant::now();
            app_data.wayland_state.force_redraw = true;
        }

        while let Ok(bytes) = app_data.paste_receiver.try_recv() {
            let active_pane = app_data.tab_manager.active_mux().active_pane;
            clear_command_completion_indicators_for_user_interaction(&mut app_data);
            let bracketed_paste = app_data
                .tab_manager
                .active_mux()
                .panes
                .get(&active_pane)
                .unwrap()
                .snapshot
                .load()
                .bracketed_paste;
            if bracketed_paste {
                let mut wrapped = Vec::with_capacity(bytes.len() + 12);
                wrapped.extend_from_slice(b"\x1b[200~");
                wrapped.extend_from_slice(&bytes);
                wrapped.extend_from_slice(b"\x1b[201~");
                app_data
                    .pane_io
                    .send_ui_command(crate::mux::io::PtyWorkerCommand::Write(
                        active_pane,
                        wrapped,
                    ));
            } else {
                app_data
                    .pane_io
                    .send_ui_command(crate::mux::io::PtyWorkerCommand::Write(active_pane, bytes));
            }
            // Typing trap
            app_data.cursor_visible_phase = true;
            app_data.last_cursor_blink = std::time::Instant::now();
            app_data.wayland_state.force_redraw = true;
        }

        // Process pane splits
        if !app_data.startup_geometry_ready && !app_data.wayland_state.pending_splits.is_empty() {
            let splits: Vec<_> = app_data.wayland_state.pending_splits.drain(..).collect();
            for split in splits {
                defer_startup_work(&mut app_data, DeferredStartupWork::Split(split));
            }
        }

        if !app_data.wayland_state.pending_splits.is_empty() {
            let splits: Vec<_> = app_data.wayland_state.pending_splits.drain(..).collect();
            for split in splits {
                if app_data.tab_manager.active_mux().is_zoomed()
                    || app_data
                        .pane_runtime
                        .scrolling()
                        .map(|manager| manager.is_zoomed())
                        .unwrap_or(false)
                {
                    tracing::debug!("Ignoring pane split while a pane is zoomed");
                    continue;
                }

                let axis = match split {
                    crate::wayland::connection::PendingSplit::Vertical => {
                        crate::mux::state::SplitAxis::Vertical
                    }
                    crate::wayland::connection::PendingSplit::Horizontal => {
                        crate::mux::state::SplitAxis::Horizontal
                    }
                };

                let next_pane_id = app_data.tab_manager.alloc_pane_id();
                let grid_size = if app_data.pane_runtime.is_tiling() {
                    crate::mux::GridSize::new(80, 24) // Dummy size, corrected in relayout
                } else {
                    let Some(grid_size) = app_data
                        .pane_runtime
                        .scrolling()
                        .and_then(|manager| manager.planned_split_grid_size(axis))
                    else {
                        tracing::debug!(
                            "Ignoring scrolling pane split because active pane is too small"
                        );
                        continue;
                    };
                    grid_size
                };

                let metrics = pointer_layout_metrics(&app_data);
                let mut winsize = forge_pty::pty::size_to_winsize(
                    app_data.wayland_state.window.as_ref().unwrap().size,
                    1,
                    1,
                );
                winsize.ws_col = grid_size.cols as u16;
                winsize.ws_row = grid_size.rows as u16;
                winsize.ws_xpixel = (grid_size.cols as f64 * metrics.0) as u16;
                winsize.ws_ypixel = (grid_size.rows as f64 * metrics.1) as u16;

                match forge_pty::Pty::spawn(&app_data.config.shell, winsize) {
                    Ok(pty) => {
                        let mut screen_buffer = forge_pty::ScreenBuffer::new(
                            grid_size.cols,
                            grid_size.rows,
                            app_data.config.scrollback.lines.unwrap_or(100_000),
                            app_data.config.theme.parsed_foreground,
                            app_data.config.theme.parsed_background,
                        );
                        screen_buffer.palette = app_data.config.theme.parsed_ansi_colors;
                        let vte_processor = forge_pty::VteProcessor::new();
                        let snapshot = std::sync::Arc::new(arc_swap::ArcSwap::from_pointee(
                            screen_buffer.generate_snapshot(),
                        ));

                        let pane =
                            crate::mux::Pane::new(next_pane_id, pty, snapshot.clone(), grid_size);
                        let fd_clone = pane.pty.as_ref().unwrap().master_fd.try_clone().unwrap();

                        let new_active_pane = if app_data.pane_runtime.is_tiling() {
                            match app_data
                                .tab_manager
                                .active_mux_mut()
                                .commit_split_active(axis, pane)
                            {
                                Ok(pane_id) => pane_id,
                                Err(err) => {
                                    tracing::warn!(?err, "Failed to split active pane");
                                    continue;
                                }
                            }
                        } else {
                            app_data
                                .tab_manager
                                .active_mux_mut()
                                .insert_detached_pane(pane)
                        };

                        app_data
                            .pane_io
                            .register_pane(
                                new_active_pane,
                                fd_clone,
                                vte_processor,
                                screen_buffer,
                                snapshot,
                            )
                            .unwrap();
                        // Bumping the generator makes sure the newly registered pane gets its first snapshot drawn.

                        if let Some(scrolling) = app_data.pane_runtime.scrolling_mut() {
                            if let Some(changes) =
                                scrolling.split_active_with_existing(axis, new_active_pane)
                            {
                                if let Some(grid_metrics) = app_data.cached_grid_metrics {
                                    apply_scrolling_grid_changes(
                                        &mut app_data,
                                        changes,
                                        grid_metrics,
                                    );
                                }
                            }
                        } else {
                            let grid_metrics =
                                app_data.cached_grid_metrics.expect("metrics missing");
                            let startup_content_rect = crate::mux::PaneRect::new(
                                grid_metrics.pad_x as f32,
                                grid_metrics.pad_y as f32,
                                (grid_metrics.cols as f64 * grid_metrics.effective_cell_w) as f32,
                                (grid_metrics.rows as f64 * grid_metrics.effective_cell_h) as f32,
                            );
                            let layout_params = crate::mux::LayoutParams::new(
                                startup_content_rect,
                                grid_metrics.effective_cell_w as f32,
                                grid_metrics.effective_cell_h as f32,
                                app_data.config.window.gap as f32,
                                app_data.effective_pane_padding(),
                            );

                            if let Ok(changes) = app_data
                                .tab_manager
                                .active_mux_mut()
                                .relayout(layout_params)
                            {
                                let batch: Vec<_> = changes
                                    .iter()
                                    .map(|c| {
                                        (c.pane_id, c.new_grid_size.cols, c.new_grid_size.rows)
                                    })
                                    .collect();
                                app_data.pane_io.send_ui_command(
                                    crate::mux::io::PtyWorkerCommand::BatchResizeReflow(
                                        batch, None,
                                    ),
                                );
                                for change in &changes {
                                    if let Some(pane_pty) = app_data
                                        .tab_manager
                                        .active_mux_mut()
                                        .panes
                                        .get_mut(&change.pane_id)
                                        .and_then(|p| p.pty.as_mut())
                                    {
                                        let px_w = (change.new_grid_size.cols as f64
                                            * grid_metrics.effective_cell_w)
                                            as u16;
                                        let px_h = (change.new_grid_size.rows as f64
                                            * grid_metrics.effective_cell_h)
                                            as u16;
                                        let _ = pane_pty.resize(
                                            change.new_grid_size.cols as u16,
                                            change.new_grid_size.rows as u16,
                                            px_w,
                                            px_h,
                                        );
                                    }
                                }
                            }
                        }

                        // Bump gen so the worker learns about the new pane on next tick.
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);

                        // Auto-FLIP handles Open animations in the render loop
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    Err(e) => tracing::error!("Failed to spawn PTY for split pane: {}", e),
                }
            }
        }

        // Process tab actions
        if !app_data.startup_geometry_ready
            && !app_data.wayland_state.pending_tab_actions.is_empty()
        {
            let actions: Vec<_> = app_data
                .wayland_state
                .pending_tab_actions
                .drain(..)
                .collect();
            for action in actions {
                defer_startup_work(&mut app_data, DeferredStartupWork::Action(action));
            }
        }

        if !app_data.wayland_state.pending_tab_actions.is_empty() {
            let actions: Vec<_> = app_data
                .wayland_state
                .pending_tab_actions
                .drain(..)
                .collect();
            for action in actions {
                match action {
                    forge_core::bindings::Action::Copy => {
                        // Process after queued pointer events so drag-selection updates are
                        // enqueued before the worker extracts the selected text.
                    }
                    forge_core::bindings::Action::Paste => {
                        request_clipboard_paste(&mut app_data);
                    }
                    forge_core::bindings::Action::NewTab => {
                        let grid_metrics =
                            app_data.cached_grid_metrics.expect("grid metrics missing");
                        let cols = grid_metrics.cols;
                        let rows = grid_metrics.rows;

                        let mut winsize = forge_pty::pty::size_to_winsize(
                            app_data.wayland_state.window.as_ref().unwrap().size,
                            1,
                            1,
                        );
                        let metrics = pointer_layout_metrics(&app_data);
                        winsize.ws_col = cols as u16;
                        winsize.ws_row = rows as u16;
                        winsize.ws_xpixel = (cols as f64 * metrics.0) as u16;
                        winsize.ws_ypixel = (rows as f64 * metrics.1) as u16;

                        match forge_pty::Pty::spawn(&app_data.config.shell, winsize) {
                            Ok(pty) => {
                                let mut screen_buffer = forge_pty::ScreenBuffer::new(
                                    cols,
                                    rows,
                                    app_data.config.scrollback.lines.unwrap_or(100_000),
                                    app_data.config.theme.parsed_foreground,
                                    app_data.config.theme.parsed_background,
                                );
                                screen_buffer.palette = app_data.config.theme.parsed_ansi_colors;
                                let vte_processor = forge_pty::VteProcessor::new();
                                let snapshot =
                                    std::sync::Arc::new(arc_swap::ArcSwap::from_pointee(
                                        screen_buffer.generate_snapshot(),
                                    ));

                                let new_pane_id = app_data.tab_manager.alloc_pane_id();
                                let mut new_mux = crate::mux::MuxState::with_single_pane_id(
                                    new_pane_id,
                                    pty,
                                    snapshot.clone(),
                                    crate::mux::GridSize::new(cols, rows),
                                );

                                let startup_content_rect = crate::mux::PaneRect::new(
                                    metrics.2 as f32,
                                    metrics.3 as f32,
                                    (cols as f64 * metrics.0) as f32,
                                    (rows as f64 * metrics.1) as f32,
                                );
                                if let Err(err) = new_mux.relayout(crate::mux::LayoutParams::new(
                                    startup_content_rect,
                                    metrics.0 as f32,
                                    metrics.1 as f32,
                                    app_data.config.window.gap as f32,
                                    app_data.effective_pane_padding(),
                                )) {
                                    tracing::warn!(?err, "New tab mux relayout failed");
                                }

                                let active_pane = new_mux.active_pane;
                                let fd_clone = new_mux
                                    .panes
                                    .get(&active_pane)
                                    .unwrap()
                                    .pty
                                    .as_ref()
                                    .unwrap()
                                    .master_fd
                                    .try_clone()
                                    .unwrap();

                                app_data.tab_manager.create_tab(new_mux);
                                if let Some(scrolling) = app_data.pane_runtime.scrolling_mut() {
                                    scrolling.add_tab_from_tiling(
                                        app_data.tab_manager.active_tab(),
                                        cols,
                                        rows,
                                        app_data.config.panes.scroll_animation_duration_ms,
                                    );
                                }
                                app_data
                                    .pane_io
                                    .register_pane(
                                        active_pane,
                                        fd_clone,
                                        vte_processor,
                                        screen_buffer,
                                        snapshot,
                                    )
                                    .unwrap();
                                // Bump gen so the worker learns about the new pane on next tick.
                                app_data
                                    .pane_io
                                    .visible_gen
                                    .fetch_add(1, std::sync::atomic::Ordering::Release);
                                app_data.force_immediate_render = true;
                                app_data.wayland_state.force_redraw = true;
                            }
                            Err(e) => tracing::error!("Failed to spawn PTY for new tab: {}", e),
                        }
                    }
                    forge_core::bindings::Action::CloseTab => {
                        request_close_active_tab(&mut app_data);
                    }
                    forge_core::bindings::Action::NextTab => {
                        app_data.tab_manager.switch_next();
                        sync_scrolling_active_tab(&mut app_data);
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::PreviousTab => {
                        app_data.tab_manager.switch_previous();
                        sync_scrolling_active_tab(&mut app_data);
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::MoveTabLeft => {
                        app_data.tab_manager.move_active_left();
                        sync_scrolling_active_tab(&mut app_data);
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::MoveTabRight => {
                        app_data.tab_manager.move_active_right();
                        sync_scrolling_active_tab(&mut app_data);
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::FocusPaneLeft => {
                        focus_pane_direction(&mut app_data, crate::mux::state::Direction::Left);
                    }
                    forge_core::bindings::Action::FocusPaneRight => {
                        focus_pane_direction(&mut app_data, crate::mux::state::Direction::Right);
                    }
                    forge_core::bindings::Action::FocusPaneUp => {
                        focus_pane_direction(&mut app_data, crate::mux::state::Direction::Up);
                    }
                    forge_core::bindings::Action::FocusPaneDown => {
                        focus_pane_direction(&mut app_data, crate::mux::state::Direction::Down);
                    }
                    forge_core::bindings::Action::MovePaneLeft => {
                        move_scrolling_pane_direction(
                            &mut app_data,
                            crate::mux::state::Direction::Left,
                        );
                    }
                    forge_core::bindings::Action::MovePaneRight => {
                        move_scrolling_pane_direction(
                            &mut app_data,
                            crate::mux::state::Direction::Right,
                        );
                    }
                    forge_core::bindings::Action::MovePaneUp => {
                        move_scrolling_pane_direction(
                            &mut app_data,
                            crate::mux::state::Direction::Up,
                        );
                    }
                    forge_core::bindings::Action::MovePaneDown => {
                        move_scrolling_pane_direction(
                            &mut app_data,
                            crate::mux::state::Direction::Down,
                        );
                    }
                    forge_core::bindings::Action::MovePaneToTab1 => {
                        move_scrolling_pane_to_tab(&mut app_data, 0);
                    }
                    forge_core::bindings::Action::MovePaneToTab2 => {
                        move_scrolling_pane_to_tab(&mut app_data, 1);
                    }
                    forge_core::bindings::Action::MovePaneToTab3 => {
                        move_scrolling_pane_to_tab(&mut app_data, 2);
                    }
                    forge_core::bindings::Action::MovePaneToTab4 => {
                        move_scrolling_pane_to_tab(&mut app_data, 3);
                    }
                    forge_core::bindings::Action::MovePaneToTab5 => {
                        move_scrolling_pane_to_tab(&mut app_data, 4);
                    }
                    forge_core::bindings::Action::MovePaneToTab6 => {
                        move_scrolling_pane_to_tab(&mut app_data, 5);
                    }
                    forge_core::bindings::Action::MovePaneToTab7 => {
                        move_scrolling_pane_to_tab(&mut app_data, 6);
                    }
                    forge_core::bindings::Action::MovePaneToTab8 => {
                        move_scrolling_pane_to_tab(&mut app_data, 7);
                    }
                    forge_core::bindings::Action::MovePaneToTab9 => {
                        move_scrolling_pane_to_tab(&mut app_data, 8);
                    }
                    forge_core::bindings::Action::TogglePaneZoom => {
                        if let Err(err) = toggle_pane_zoom(&mut app_data, None) {
                            tracing::warn!(?err, "Failed to toggle pane zoom");
                        }
                    }
                    forge_core::bindings::Action::TogglePaneFloating => {
                        if let Err(err) = toggle_pane_floating(&mut app_data, None) {
                            tracing::warn!(?err, "Failed to toggle pane floating");
                        }
                    }
                    forge_core::bindings::Action::ToggleSidebar => {
                        app_data.sidebar.toggle();
                        if let Some(metrics) = recompute_grid_metrics_for_window(&app_data) {
                            app_data.cached_grid_metrics = Some(metrics);
                            apply_metrics_to_active_mux(&mut app_data, metrics);
                        } else {
                            app_data.cached_grid_metrics = None;
                        }
                        app_data.statusbar.generation =
                            app_data.statusbar.generation.wrapping_add(1);
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::ClosePane => {
                        request_close_active_pane(&mut app_data);
                    }
                    forge_core::bindings::Action::SpawnFloatingPane => {
                        if app_data.tab_manager.active_mux().floating_panes.len() >= 2 {
                            continue;
                        }

                        let Some(win) = app_data.wayland_state.window.as_ref() else {
                            continue;
                        };
                        let win_w = win.size.width as f64;
                        let win_h = win.size.height as f64;
                        let target_w = win_w * 0.8;
                        let target_h = win_h * 0.8;
                        let metrics = pointer_layout_metrics(&app_data);
                        let cols = (target_w / metrics.0).max(1.0) as usize;
                        let rows = (target_h / metrics.1).max(1.0) as usize;
                        let grid_size = crate::mux::GridSize::new(cols, rows);

                        let pane_w = cols as f32 * metrics.0 as f32;
                        let pane_h = rows as f32 * metrics.1 as f32;
                        let mut status_bar_y = 0.0;
                        let mut status_bar_h = 0.0;
                        if app_data.config.statusbar.enabled {
                            status_bar_h = metrics.1 as f32;
                            if matches!(
                                app_data.config.statusbar.position,
                                forge_core::config_registry::StatusbarPosition::Top
                            ) {
                                status_bar_y = status_bar_h;
                            }
                        }

                        let usable_h = (win_h as f32 - status_bar_h).max(0.0);
                        let base_x = ((win_w as f32 - pane_w) / 2.0).max(0.0);
                        let base_y = status_bar_y + ((usable_h - pane_h) / 2.0).max(0.0);

                        let mut pane_x = base_x;
                        let mut pane_y = base_y;

                        let mux = app_data.tab_manager.active_mux();
                        if let Some(&last_fp_id) = mux.floating_panes.last() {
                            if let Some(last_fp) = mux.panes.get(&last_fp_id) {
                                let offset_x = (metrics.0 as f32 * 4.0).max(32.0);
                                let offset_y = (metrics.1 as f32 * 2.0).max(32.0);

                                let mut next_x = last_fp.rect.x + offset_x;
                                let mut next_y = last_fp.rect.y - offset_y;

                                if next_x + pane_w > win_w as f32 - 10.0
                                    || next_y < status_bar_y + 10.0
                                {
                                    next_x = base_x;
                                    next_y = base_y;
                                }

                                pane_x = next_x;
                                pane_y = next_y;
                            }
                        }

                        let mut winsize = forge_pty::pty::size_to_winsize(win.size, 1, 1);
                        winsize.ws_col = cols as u16;
                        winsize.ws_row = rows as u16;
                        winsize.ws_xpixel = (cols as f64 * metrics.0) as u16;
                        winsize.ws_ypixel = (rows as f64 * metrics.1) as u16;

                        match forge_pty::Pty::spawn(&app_data.config.shell, winsize) {
                            Ok(pty) => {
                                let mut screen_buffer = forge_pty::ScreenBuffer::new(
                                    cols,
                                    rows,
                                    app_data.config.scrollback.lines.unwrap_or(100_000),
                                    app_data.config.theme.parsed_foreground,
                                    app_data.config.theme.parsed_background,
                                );
                                screen_buffer.palette = app_data.config.theme.parsed_ansi_colors;
                                let vte_processor = forge_pty::VteProcessor::new();
                                let snapshot =
                                    std::sync::Arc::new(arc_swap::ArcSwap::from_pointee(
                                        screen_buffer.generate_snapshot(),
                                    ));

                                let pane_id = app_data.tab_manager.alloc_pane_id();
                                let mut pane = crate::mux::Pane::new(
                                    pane_id,
                                    pty,
                                    snapshot.clone(),
                                    grid_size,
                                );
                                pane.rect =
                                    crate::mux::PaneRect::new(pane_x, pane_y, pane_w, pane_h);
                                pane.dirty_layout = false;

                                let fd_clone =
                                    pane.pty.as_ref().unwrap().master_fd.try_clone().unwrap();

                                let new_active_pane = app_data
                                    .tab_manager
                                    .active_mux_mut()
                                    .add_floating_pane(pane);

                                // Add opening animation
                                app_data.pane_animations.insert(
                                    new_active_pane,
                                    PaneAnimation {
                                        kind: PaneAnimationKind::Open,
                                        dx: 0.0,
                                        dy: 0.0,
                                        dw: 0.0,
                                        dh: 0.0,
                                        started_at: Instant::now(),
                                        duration: Duration::from_millis(
                                            app_data.config.render.pane_animation_duration_ms
                                                as u64,
                                        ),
                                    },
                                );

                                app_data
                                    .pane_io
                                    .register_pane(
                                        new_active_pane,
                                        fd_clone,
                                        vte_processor,
                                        screen_buffer,
                                        snapshot,
                                    )
                                    .unwrap();

                                app_data
                                    .pane_io
                                    .visible_gen
                                    .fetch_add(1, std::sync::atomic::Ordering::Release);
                                app_data.force_immediate_render = true;
                                app_data.wayland_state.force_redraw = true;
                            }
                            Err(e) => tracing::error!("Failed to spawn floating pane: {}", e),
                        }
                    }
                    forge_core::bindings::Action::SwitchTab1 => {
                        app_data.tab_manager.switch_to_index(0);
                        sync_scrolling_active_tab(&mut app_data);
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::SwitchTab2 => {
                        app_data.tab_manager.switch_to_index(1);
                        sync_scrolling_active_tab(&mut app_data);
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::SwitchTab3 => {
                        app_data.tab_manager.switch_to_index(2);
                        sync_scrolling_active_tab(&mut app_data);
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::SwitchTab4 => {
                        app_data.tab_manager.switch_to_index(3);
                        sync_scrolling_active_tab(&mut app_data);
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::SwitchTab5 => {
                        app_data.tab_manager.switch_to_index(4);
                        sync_scrolling_active_tab(&mut app_data);
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::SwitchTab6 => {
                        app_data.tab_manager.switch_to_index(5);
                        sync_scrolling_active_tab(&mut app_data);
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::SwitchTab7 => {
                        app_data.tab_manager.switch_to_index(6);
                        sync_scrolling_active_tab(&mut app_data);
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::SwitchTab8 => {
                        app_data.tab_manager.switch_to_index(7);
                        sync_scrolling_active_tab(&mut app_data);
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    forge_core::bindings::Action::SwitchTab9 => {
                        app_data.tab_manager.switch_to_index(8);
                        sync_scrolling_active_tab(&mut app_data);
                        app_data
                            .pane_io
                            .visible_gen
                            .fetch_add(1, std::sync::atomic::Ordering::Release);
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                    _ => {}
                }
            }
        }

        if !app_data.wayland_state.running {
            app_data.loop_signal.stop();
            break;
        }

        let font_load_result = app_data
            .font_atlas_receiver
            .as_ref()
            .map(|rx| rx.try_recv());
        match font_load_result {
            Some(Ok(font_data)) => {
                tracing::info!("Received full FontData from background thread!");
                if let Some(renderer) = app_data.renderer.as_mut() {
                    if let Err(e) = renderer.update_font_data(
                        font_data.regular,
                        font_data.bold,
                        font_data.italic,
                        font_data.bold_italic,
                        font_data.fallbacks,
                        font_data.px_size,
                        font_data.atlas,
                    ) {
                        tracing::error!("Failed to update font atlas: {}", e);
                    }
                }

                app_data
                    .pane_io
                    .send_ui_command(crate::mux::io::PtyWorkerCommand::MarkAllDirty(
                        app_data.tab_manager.active_mux().active_pane,
                    ));
                finish_startup_geometry(&mut app_data);
                app_data.font_atlas_receiver = None; // Drop receiver once loaded
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                tracing::warn!(
                    "Background font loading ended before producing font data; using current renderer metrics"
                );
                finish_startup_geometry(&mut app_data);
                app_data.font_atlas_receiver = None;
            }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) | None => {}
        }

        if let Some(rx) = app_data.config_rx.as_ref() {
            // Drain the channel and only keep the LAST update
            let mut latest_update = None;
            while let Ok(update) = rx.try_recv() {
                latest_update = Some(update);
            }

            if let Some(update) = latest_update {
                let new_config = runtime_reload_config(&app_data.config, update.config);
                let changes =
                    forge_config::types::ConfigChangeSet::between(&app_data.config, &new_config);
                if !changes.any() {
                    tracing::info!("Manual config reload produced no runtime changes.");
                    continue;
                }

                tracing::info!(changes = ?changes, "Applying manual config reload.");
                app_data.config = new_config;
                app_data.cached_bg_color = app_data.config.theme.parsed_background.to_srgb_linear();
                app_data.cached_cursor_color =
                    app_data.config.theme.parsed_cursor_color.to_srgb_linear();
                app_data.cached_selection_bg_color =
                    app_data.config.theme.parsed_selection_bg.to_srgb_linear();
                app_data.cached_statusbar_hover_color =
                    compute_statusbar_hover_color(&app_data.config.statusbar.bg_color);
                if changes.keybindings {
                    app_data.wayland_state.keybindings = app_data.config.keybindings.clone();
                }
                if changes.behavior {
                    app_data.wayland_state.hide_mouse_when_typing =
                        app_data.config.behavior.hide_mouse_when_typing;
                }
                if changes.blur {
                    let compositor = app_data.wayland_state.globals.compositor.clone();
                    let kde_blur_manager = app_data.wayland_state.globals.kde_blur_manager.clone();
                    if let Some(window) = app_data.wayland_state.window.as_mut() {
                        let blur_status = window.blur.apply(
                            &window.surface,
                            &compositor,
                            kde_blur_manager.as_ref(),
                            &app_data.queue_handle,
                            window.size,
                            &app_data.config.blur,
                        );
                        tracing::debug!(?blur_status, "Wayland blur config update applied");
                        app_data.wayland_state.needs_flush = true;
                    }
                }
                if changes.font {
                    if let Some(renderer) = app_data.renderer.as_mut() {
                        renderer.set_ligature_config(app_data.config.font.ligatures.clone());
                    }
                }
                if changes.cursor {
                    if let Some(renderer) = app_data.renderer.as_mut() {
                        renderer.set_cursor_trail_config(&app_data.config.cursor.trail);
                    }
                }
                if changes.command_completion_indicator || changes.shell {
                    let enabled = command_completion_tracking_enabled_for_config(&app_data.config);
                    app_data.pane_io.set_command_completion_tracking(enabled);
                    if !enabled {
                        app_data.command_completion_indicators.clear();
                    } else if !app_data.command_completion_indicators.is_empty() {
                        let now = std::time::Instant::now();
                        let expiry = command_completion_indicator_expiry(
                            &app_data.config.command_completion_indicator,
                            now,
                        );
                        for indicator in app_data.command_completion_indicators.values_mut() {
                            indicator.shown_at = now;
                            indicator.opening_frame_presented = false;
                            indicator.dismissed_at = None;
                            indicator.expires_at = expiry;
                        }
                    }
                    app_data.wayland_state.force_redraw = true;
                }
                if changes.window || changes.statusbar {
                    if app_data.startup_geometry_ready {
                        apply_authoritative_window_geometry(&mut app_data);
                    } else {
                        app_data.cached_grid_metrics = None;
                        app_data.pending_startup_window_size = app_data
                            .wayland_state
                            .window
                            .as_ref()
                            .map(|window| window.size);
                    }
                    app_data.statusbar.generation = app_data.statusbar.generation.wrapping_add(1);
                }

                app_data
                    .pane_io
                    .send_ui_command(crate::mux::io::PtyWorkerCommand::MarkAllDirty(
                        app_data.tab_manager.active_mux().active_pane,
                    ));
                app_data.wayland_state.force_redraw = true;
            }
        }

        if let Some(win_size) = app_data
            .wayland_state
            .window
            .as_ref()
            .map(|window| window.size)
        {
            if win_size != app_data.last_window_size {
                let resized_renderer = if let Some(renderer) = app_data.renderer.as_mut() {
                    let _ = renderer.recreate_swapchain(win_size.width, win_size.height);
                    app_data.pane_io.send_ui_command(
                        crate::mux::io::PtyWorkerCommand::MarkAllDirty(
                            app_data.tab_manager.active_mux().active_pane,
                        ),
                    ); // Force re-render on new swapchain images

                    true
                } else {
                    false
                };

                if resized_renderer {
                    if app_data.startup_geometry_ready {
                        apply_authoritative_window_geometry(&mut app_data);
                    } else {
                        app_data.pending_startup_window_size = Some(win_size);
                        app_data.cached_grid_metrics = None;
                        app_data.force_immediate_render = true;
                        app_data.wayland_state.force_redraw = true;
                    }
                }
                let compositor = app_data.wayland_state.globals.compositor.clone();
                let kde_blur_manager = app_data.wayland_state.globals.kde_blur_manager.clone();
                if let Some(window) = app_data.wayland_state.window.as_mut() {
                    let blur_status = window.blur.apply(
                        &window.surface,
                        &compositor,
                        kde_blur_manager.as_ref(),
                        &app_data.queue_handle,
                        window.size,
                        &app_data.config.blur,
                    );
                    tracing::trace!(?blur_status, "Wayland blur resize state checked");
                    app_data.wayland_state.needs_flush = true;
                }
                app_data.last_window_size = win_size;
            }
        }

        let (mouse_tracking_enabled, mouse_sgr_mode, use_alt, scrollback_lines) = {
            let sb = app_data
                .tab_manager
                .active_mux()
                .panes
                .get(&app_data.tab_manager.active_mux().active_pane)
                .unwrap()
                .snapshot
                .load();
            (
                sb.mouse_tracking_enabled,
                sb.mouse_sgr_mode,
                sb.use_alt_buffer,
                sb.history_lines as usize,
            )
        };

        use crate::wayland::connection::PointerEvent;
        let mut pending_pointer_event = app_data.pointer_receiver.try_recv().ok();
        while let Some(evt) = pending_pointer_event.take() {
            let evt = match evt {
                PointerEvent::Motion { mut x, mut y }
                    if !pointer_motion_has_effect(
                        use_alt,
                        scrollback_lines,
                        mouse_tracking_enabled,
                        app_data.active_mouse_button,
                        app_data.drag_start,
                        app_data.is_dragging_scrollbar,
                    ) =>
                {
                    loop {
                        match app_data.pointer_receiver.try_recv() {
                            Ok(PointerEvent::Motion {
                                x: next_x,
                                y: next_y,
                            }) => {
                                x = next_x;
                                y = next_y;
                            }
                            Ok(next_evt) => {
                                pending_pointer_event = Some(next_evt);
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    PointerEvent::Motion { x, y }
                }
                evt => evt,
            };

            if app_data.active_modal.is_some() {
                continue;
            }

            match evt {
                PointerEvent::Enter { x, y } | PointerEvent::Motion { x, y } => {
                    let mut needs_redraw = false;
                    let now = std::time::Instant::now();
                    let scrollbar_available = !use_alt && scrollback_lines > 0;

                    if scrollbar_available {
                        if now
                            .duration_since(app_data.last_mouse_activity)
                            .as_secs_f32()
                            > 0.5
                        {
                            app_data.mouse_started_moving = now;
                            needs_redraw = true;
                        }
                        app_data.last_mouse_activity = now;
                    }

                    let mut new_hovering = false;
                    if scrollbar_available {
                        if let Some(window) = app_data.wayland_state.window.as_ref() {
                            new_hovering = x > window.size.width as f64 - 24.0;
                        }
                    }

                    let mut new_hovered_split = None;
                    let mut new_hovered_scrolling_resize = None;
                    if let Some(manager) = app_data.pane_runtime.scrolling() {
                        if !manager.is_zoomed()
                            && !app_data.is_hovering_edge
                            && app_data.dragging_scrolling_resize.is_none()
                        {
                            if let Some(metrics) = app_data.cached_grid_metrics {
                                let col = ((x - metrics.pad_x) / metrics.effective_cell_w).floor();
                                let row = ((y - metrics.pad_y) / metrics.effective_cell_h).floor();
                                if col >= 0.0 && row >= 0.0 {
                                    new_hovered_scrolling_resize =
                                        manager.active_tab().and_then(|tab| {
                                            tab.panes
                                                .resize_handle_at_cell(col as usize, row as usize)
                                        });
                                }
                            }
                        }
                    } else if !app_data.tab_manager.active_mux().is_zoomed()
                        && !app_data.is_hovering_edge
                        && app_data.dragging_split.is_none()
                    {
                        let ptr_x = x as f32;
                        let ptr_y = y as f32;
                        for (i, border) in app_data
                            .tab_manager
                            .active_mux()
                            .last_borders
                            .iter()
                            .enumerate()
                        {
                            let rect = &border.rect;
                            let tol = 8.0;
                            if ptr_x >= rect.x - tol
                                && ptr_x <= rect.x + rect.width + tol
                                && ptr_y >= rect.y - tol
                                && ptr_y <= rect.y + rect.height + tol
                            {
                                new_hovered_split = Some(i);
                                break;
                            }
                        }
                    }

                    let mut hovering_statusbar = false;
                    let mut new_hovered_action = None;
                    let mut new_hovered_region = None;
                    if app_data.config.statusbar.enabled {
                        if let Some(metrics) = app_data.cached_grid_metrics {
                            if y >= metrics.sb_y && y < metrics.sb_y + metrics.effective_cell_h {
                                hovering_statusbar = true;
                                let col = ((x - metrics.pad_x) / metrics.effective_cell_w).max(0.0)
                                    as usize;
                                for region in &app_data.statusbar.click_regions {
                                    if col >= region.start_col && col < region.end_col {
                                        new_hovered_action = Some(region.action.clone());
                                        new_hovered_region =
                                            Some((region.start_col, region.end_col));
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    if app_data.tab_manager.active_mux().is_zoomed() {
                        app_data.dragging_split = None;
                    }

                    if new_hovered_split != app_data.hovered_split
                        || new_hovered_scrolling_resize != app_data.hovered_scrolling_resize
                        || new_hovering != app_data.is_hovering_edge
                        || hovering_statusbar != app_data.is_hovering_statusbar
                        || new_hovered_action != app_data.statusbar.hovered_action
                    {
                        app_data.is_hovering_edge = new_hovering;
                        app_data.hovered_split = new_hovered_split;
                        app_data.hovered_scrolling_resize = new_hovered_scrolling_resize;
                        app_data.is_hovering_statusbar = hovering_statusbar;

                        if let Some(action) = &new_hovered_action {
                            app_data.statusbar.hovered_is_square = action == "NewTab";
                        }

                        app_data.statusbar.hovered_action = new_hovered_action;
                        app_data.statusbar.hovered_region = new_hovered_region;
                        needs_redraw = true;
                    }

                    let mut hovering_context_menu = false;
                    let (cell_w, cell_h, _, _) = pointer_layout_metrics(&app_data);
                    if let Some(cm) = &mut app_data.context_menu {
                        if let Some(window) = app_data.wayland_state.window.as_ref() {
                            let win_w = window.size.width as f64;
                            let win_h = window.size.height as f64;
                            if cm.contains(x, y, win_w, win_h, cell_w, cell_h) {
                                hovering_context_menu = true;
                            }
                            if cm.update_hover(x, y, win_w, win_h, cell_w, cell_h) {
                                needs_redraw = true;
                            }
                        }
                    }

                    if needs_redraw {
                        if let Some(pointer) = &app_data.wayland_state.pointer {
                            if let Some(shape_manager) =
                                &app_data.wayland_state.globals.cursor_shape_manager
                            {
                                let device =
                                    shape_manager.get_pointer(pointer, &app_data.queue_handle, ());
                                let shape = if new_hovering
                                    || hovering_statusbar
                                    || hovering_context_menu
                                {
                                    wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape::Default
                                } else if let Some(handle) = app_data
                                    .dragging_scrolling_resize
                                    .as_ref()
                                    .map(|drag| drag.handle)
                                    .or(app_data.hovered_scrolling_resize)
                                {
                                    match handle.axis {
                                        crate::mux::state::SplitAxis::Vertical => wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape::ColResize,
                                        crate::mux::state::SplitAxis::Horizontal => wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape::RowResize,
                                    }
                                } else if let Some(border) =
                                    app_data.dragging_split.as_ref().or_else(|| {
                                        app_data.hovered_split.and_then(|i| {
                                            app_data.tab_manager.active_mux().last_borders.get(i)
                                        })
                                    })
                                {
                                    match border.axis {
                                        crate::mux::state::SplitAxis::Vertical => wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape::ColResize,
                                        crate::mux::state::SplitAxis::Horizontal => wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape::RowResize,
                                    }
                                } else {
                                    if use_alt {
                                        wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape::Default
                                    } else {
                                        wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape::Text
                                    }
                                };
                                device.set_shape(app_data.wayland_state.pointer_serial, shape);
                                device.destroy();
                            }
                        }
                    }

                    if let Some(border) = app_data.dragging_split.clone() {
                        let parent = &border.parent_rect;
                        let new_ratio = if border.axis == crate::mux::state::SplitAxis::Vertical {
                            let rel_x = x as f32 - parent.x;
                            rel_x / parent.width
                        } else {
                            let rel_y = y as f32 - parent.y;
                            rel_y / parent.height
                        }
                        .clamp(0.05, 0.95);
                        if app_data
                            .tab_manager
                            .active_mux_mut()
                            .root
                            .set_split_ratio(&border.path, new_ratio)
                        {
                            let (cell_w, cell_h, pad_x, pad_y) = pointer_layout_metrics(&app_data);
                            let metrics = app_data.cached_grid_metrics.unwrap();
                            let layout_params = crate::mux::LayoutParams::new(
                                crate::mux::PaneRect::new(
                                    pad_x as f32,
                                    pad_y as f32,
                                    (metrics.cols as f64 * cell_w) as f32,
                                    (metrics.rows as f64 * cell_h) as f32,
                                ),
                                cell_w as f32,
                                cell_h as f32,
                                app_data.config.window.gap as f32,
                                app_data.effective_pane_padding(),
                            );
                            let _ = app_data
                                .tab_manager
                                .active_mux_mut()
                                .relayout(layout_params);
                            // VISUAL DRAG ONLY: Do NOT send BatchResizeReflow here.
                            app_data.force_immediate_render = true;
                            app_data.wayland_state.force_redraw = true;
                            app_data.loop_signal.wakeup();
                        }
                        app_data.pointer_x = x;
                        app_data.pointer_y = y;
                        if pending_pointer_event.is_none() {
                            pending_pointer_event = app_data.pointer_receiver.try_recv().ok();
                        }
                        continue;
                    }

                    if let Some(drag) = app_data.dragging_scrolling_resize.clone() {
                        if let Some(metrics) = app_data.cached_grid_metrics {
                            let col = ((x - metrics.pad_x) / metrics.effective_cell_w)
                                .floor()
                                .max(0.0) as usize;
                            let row = ((y - metrics.pad_y) / metrics.effective_cell_h)
                                .floor()
                                .max(0.0) as usize;
                            let changes = app_data
                                .pane_runtime
                                .scrolling_mut()
                                .and_then(|manager| {
                                    manager.active_tab_mut().and_then(|tab| {
                                        tab.panes.resize_drag_to_cell(drag, col, row)
                                    })
                                })
                                .unwrap_or_default();
                            if !changes.is_empty() {
                                apply_scrolling_grid_changes(&mut app_data, changes, metrics);
                                app_data
                                    .pane_io
                                    .visible_gen
                                    .fetch_add(1, std::sync::atomic::Ordering::Release);
                                app_data.force_immediate_render = true;
                                app_data.wayland_state.force_redraw = true;
                                app_data.loop_signal.wakeup();
                            }
                        }
                        app_data.pointer_x = x;
                        app_data.pointer_y = y;
                        if pending_pointer_event.is_none() {
                            pending_pointer_event = app_data.pointer_receiver.try_recv().ok();
                        }
                        continue;
                    }

                    if scrollbar_available {
                        let active_secs = now
                            .duration_since(app_data.mouse_started_moving)
                            .as_secs_f32();
                        if active_secs >= 0.25 && app_data.current_thumb_opacity < 0.99 {
                            needs_redraw = true;
                        }
                    }

                    if app_data.is_dragging_scrollbar {
                        needs_redraw = true;
                    }

                    if needs_redraw {
                        app_data.wayland_state.force_redraw = true;
                        app_data.loop_signal.wakeup();
                    }
                    app_data.pointer_x = x;
                    app_data.pointer_y = y;

                    let needs_terminal_drag =
                        mouse_tracking_enabled && app_data.active_mouse_button.is_some();
                    let needs_selection_drag =
                        !mouse_tracking_enabled && app_data.drag_start.is_some();
                    let needs_scrollbar_drag =
                        !mouse_tracking_enabled && app_data.is_dragging_scrollbar;

                    if !needs_terminal_drag && !needs_selection_drag && !needs_scrollbar_drag {
                        if pending_pointer_event.is_none() {
                            pending_pointer_event = app_data.pointer_receiver.try_recv().ok();
                        }
                        continue;
                    }

                    let (cell_w, cell_h, _pad_x, _pad_y) = pointer_layout_metrics(&app_data);
                    let active_pane = app_data.tab_manager.active_mux().active_pane;
                    let active_rect = app_data
                        .tab_manager
                        .active_mux()
                        .panes
                        .get(&active_pane)
                        .unwrap()
                        .rect;

                    let col_1 = ((x
                        - (active_rect.x as f64 + app_data.effective_pane_padding().left as f64))
                        / cell_w)
                        .max(0.0) as usize
                        + 1;
                    let row_1 = ((y
                        - (active_rect.y as f64 + app_data.effective_pane_padding().top as f64))
                        / cell_h)
                        .max(0.0) as usize
                        + 1;

                    if mouse_tracking_enabled {
                        if let Some(btn) = app_data.active_mouse_button {
                            if col_1 != app_data.last_mouse_col || row_1 != app_data.last_mouse_row
                            {
                                let btn_code = match btn {
                                    272 => 0,
                                    274 => 1,
                                    273 => 2,
                                    _ => 0,
                                } + 32; // Drag flag
                                if mouse_sgr_mode {
                                    let seq = format!("\x1b[<{};{};{}M", btn_code, col_1, row_1);
                                    let active_pane = app_data.tab_manager.active_mux().active_pane;
                                    app_data.pane_io.send_ui_command(
                                        crate::mux::io::PtyWorkerCommand::Write(
                                            active_pane,
                                            seq.into_bytes(),
                                        ),
                                    );
                                }
                                app_data.last_mouse_col = col_1;
                                app_data.last_mouse_row = row_1;
                            }
                        }
                    } else {
                        if let Some((start_col, start_row)) = app_data.drag_start {
                            let col = ((x
                                - (active_rect.x as f64
                                    + app_data.effective_pane_padding().left as f64))
                                / cell_w)
                                .max(0.0) as usize;
                            let row = ((y
                                - (active_rect.y as f64
                                    + app_data.effective_pane_padding().top as f64))
                                / cell_h)
                                .max(0.0) as usize;
                            let active_pane = app_data.tab_manager.active_mux().active_pane;
                            if let Some(pane) =
                                app_data.tab_manager.active_mux().panes.get(&active_pane)
                            {
                                let sb = pane.snapshot.load();
                                if col != start_col || row != start_row || sb.selection.is_some() {
                                    if sb.selection.is_none() {
                                        app_data.pane_io.send_ui_command(
                                            crate::mux::io::PtyWorkerCommand::UpdateSelection(
                                                active_pane,
                                                Some(forge_core::cell::SelectionRange {
                                                    start_row,
                                                    start_col,
                                                    end_row: row,
                                                    end_col: col,
                                                }),
                                            ),
                                        );
                                    } else if let Some(sel) = sb.selection {
                                        if sel.end_row != row || sel.end_col != col {
                                            app_data.pane_io.send_ui_command(
                                                crate::mux::io::PtyWorkerCommand::UpdateSelection(
                                                    active_pane,
                                                    Some(forge_core::cell::SelectionRange {
                                                        start_row: sel.start_row,
                                                        start_col: sel.start_col,
                                                        end_row: row,
                                                        end_col: col,
                                                    }),
                                                ),
                                            );
                                        }
                                    }
                                }
                            }
                        } else if app_data.is_dragging_scrollbar {
                            if let Some(window) = app_data.wayland_state.window.as_ref() {
                                let win_h = window.size.height as f64;
                                let track_top = 4.0;
                                let track_bottom = win_h - 4.0;
                                let usable_track_height = track_bottom - track_top;

                                if let Some((_, thumb_height)) = app_data.last_scrollbar_state {
                                    let available_travel_space = usable_track_height - thumb_height;
                                    if available_travel_space > 0.0 {
                                        let new_thumb_y = y - app_data.scrollbar_drag_offset_y;
                                        let mut scroll_ratio = 1.0
                                            - (new_thumb_y - track_top) / available_travel_space;
                                        scroll_ratio = scroll_ratio.clamp(0.0, 1.0);

                                        let active_pane =
                                            app_data.tab_manager.active_mux().active_pane;
                                        if let Some(pane) = app_data
                                            .tab_manager
                                            .active_mux()
                                            .panes
                                            .get(&active_pane)
                                        {
                                            let sb = pane.snapshot.load();
                                            let history_lines = sb.history_lines;
                                            let new_offset =
                                                (scroll_ratio * history_lines).round() as usize;

                                            app_data.pane_io.send_ui_command(
                                                crate::mux::io::PtyWorkerCommand::SetScrollOffset(
                                                    active_pane,
                                                    new_offset,
                                                ),
                                            );
                                            app_data.loop_signal.wakeup();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                PointerEvent::Leave => {
                    if !use_alt && scrollback_lines > 0 {
                        app_data.is_hovering_edge = false;
                        app_data.is_dragging_scrollbar = false;
                        app_data.current_thumb_opacity = 0.0;
                        app_data.current_track_opacity = 0.0;
                        app_data.wayland_state.force_redraw = true;
                        app_data.loop_signal.wakeup();
                    }
                }
                PointerEvent::Press { button, .. } => {
                    clear_command_completion_indicators_for_user_interaction(&mut app_data);
                    if button == 272 {
                        if let Some(handle) = app_data.hovered_scrolling_resize {
                            if let Some(metrics) = app_data.cached_grid_metrics {
                                let col = ((app_data.pointer_x - metrics.pad_x)
                                    / metrics.effective_cell_w)
                                    .floor();
                                let row = ((app_data.pointer_y - metrics.pad_y)
                                    / metrics.effective_cell_h)
                                    .floor();
                                if col >= 0.0 && row >= 0.0 {
                                    app_data.dragging_scrolling_resize =
                                        app_data.pane_runtime.scrolling().and_then(|manager| {
                                            manager.active_tab().and_then(|tab| {
                                                tab.panes.start_resize_drag(
                                                    handle,
                                                    col as usize,
                                                    row as usize,
                                                )
                                            })
                                        });
                                    if app_data.dragging_scrolling_resize.is_some() {
                                        app_data.active_mouse_button = Some(button);
                                        continue;
                                    }
                                }
                            }
                        }

                        if app_data.pane_runtime.is_tiling()
                            && !app_data.tab_manager.active_mux().is_zoomed()
                        {
                            if let Some(hovered_idx) = app_data.hovered_split {
                                if let Some(border) = app_data
                                    .tab_manager
                                    .active_mux()
                                    .last_borders
                                    .get(hovered_idx)
                                    .cloned()
                                {
                                    app_data.dragging_split = Some(border);
                                    app_data.active_mouse_button = Some(button);
                                    continue;
                                }
                            }
                        }
                    }

                    let ptr_x = app_data.pointer_x;
                    let ptr_y = app_data.pointer_y;

                    if let Some(cm) = app_data.context_menu.take() {
                        let win_w =
                            app_data.wayland_state.window.as_ref().unwrap().size.width as f64;
                        let win_h =
                            app_data.wayland_state.window.as_ref().unwrap().size.height as f64;
                        if button == 272 {
                            let (cell_w, cell_h, _, _) = pointer_layout_metrics(&app_data);
                            if let Some(action) =
                                cm.action_at(ptr_x, ptr_y, win_w, win_h, cell_w, cell_h)
                            {
                                match action {
                                    crate::context_menu::ContextMenuAction::Copy => {
                                        let target_pane =
                                            cm.target_pane_id().unwrap_or_else(|| {
                                                app_data.tab_manager.active_mux().active_pane
                                            });
                                        copy_selection_from_pane(&mut app_data, target_pane);
                                    }
                                    crate::context_menu::ContextMenuAction::Paste => {
                                        request_clipboard_paste(&mut app_data);
                                    }
                                    crate::context_menu::ContextMenuAction::Split => {
                                        let mut menu = cm;
                                        menu.open_split_submenu();
                                        app_data.context_menu = Some(menu);
                                    }
                                    crate::context_menu::ContextMenuAction::SplitHorizontal => {
                                        let target_pane =
                                            cm.target_pane_id().unwrap_or_else(|| {
                                                app_data.tab_manager.active_mux().active_pane
                                            });
                                        if app_data
                                            .tab_manager
                                            .active_mux()
                                            .panes
                                            .contains_key(&target_pane)
                                        {
                                            app_data.tab_manager.active_mux_mut().active_pane =
                                                target_pane;
                                            app_data
                                                .wayland_state
                                                .pending_splits
                                                .push(crate::wayland::connection::PendingSplit::Horizontal);
                                        }
                                    }
                                    crate::context_menu::ContextMenuAction::SplitVertical => {
                                        let target_pane =
                                            cm.target_pane_id().unwrap_or_else(|| {
                                                app_data.tab_manager.active_mux().active_pane
                                            });
                                        if app_data
                                            .tab_manager
                                            .active_mux()
                                            .panes
                                            .contains_key(&target_pane)
                                        {
                                            app_data.tab_manager.active_mux_mut().active_pane =
                                                target_pane;
                                            app_data.wayland_state.pending_splits.push(
                                                crate::wayland::connection::PendingSplit::Vertical,
                                            );
                                        }
                                    }
                                    crate::context_menu::ContextMenuAction::ZoomPane => {
                                        let target_pane =
                                            cm.target_pane_id().unwrap_or_else(|| {
                                                app_data.tab_manager.active_mux().active_pane
                                            });
                                        if let Err(err) =
                                            toggle_pane_zoom(&mut app_data, Some(target_pane))
                                        {
                                            tracing::warn!(
                                                ?err,
                                                "Failed to toggle pane zoom from context menu"
                                            );
                                        }
                                    }
                                    crate::context_menu::ContextMenuAction::TogglePaneFloating => {
                                        let target_pane =
                                            cm.target_pane_id().unwrap_or_else(|| {
                                                app_data.tab_manager.active_mux().active_pane
                                            });
                                        if let Err(err) =
                                            toggle_pane_floating(&mut app_data, Some(target_pane))
                                        {
                                            tracing::warn!(
                                                ?err,
                                                "Failed to toggle pane floating from context menu"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        app_data.wayland_state.force_redraw = true;
                        continue;
                    }

                    if app_data.config.statusbar.enabled {
                        if let Some(metrics) = app_data.cached_grid_metrics {
                            if ptr_y >= metrics.sb_y
                                && ptr_y < metrics.sb_y + metrics.effective_cell_h
                            {
                                let col = ((ptr_x - metrics.pad_x) / metrics.effective_cell_w)
                                    .max(0.0) as usize;
                                let mut action_to_execute = None;
                                for region in &app_data.statusbar.click_regions {
                                    if col >= region.start_col && col < region.end_col {
                                        action_to_execute = Some(region.action.clone());
                                        break;
                                    }
                                }

                                if let Some(action) = action_to_execute {
                                    if let Some(tab) = action.strip_prefix("SwitchTab") {
                                        if let Ok(idx) = tab.parse::<usize>() {
                                            if idx > 0 {
                                                let target = idx - 1;
                                                if target < app_data.tab_manager.tabs.len()
                                                    && app_data.tab_manager.active_tab_index
                                                        != target
                                                {
                                                    app_data.tab_manager.active_tab_index = target;
                                                    sync_scrolling_active_tab(&mut app_data);
                                                    app_data.pane_io.visible_gen.fetch_add(
                                                        1,
                                                        std::sync::atomic::Ordering::Release,
                                                    );
                                                    app_data.force_immediate_render = true;
                                                    app_data.wayland_state.force_redraw = true;
                                                }
                                            }
                                        }
                                    } else if action == "NewTab" {
                                        app_data
                                            .wayland_state
                                            .pending_tab_actions
                                            .push(forge_core::bindings::Action::NewTab);
                                    }
                                }
                                continue;
                            }
                        }
                    }

                    let clicked_pane = app_data.pane_at_point(ptr_x as f32, ptr_y as f32);
                    if let Some(pane_id) = clicked_pane {
                        let current = app_data.active_pane_id();
                        if pane_id != current {
                            match &mut app_data.pane_runtime {
                                crate::mux::PaneRuntime::Tiling => {
                                    app_data.tab_manager.active_mux_mut().active_pane = pane_id;
                                }
                                crate::mux::PaneRuntime::Scrolling(manager) => {
                                    if manager.focus_pane(pane_id) {
                                        app_data.tab_manager.active_mux_mut().active_pane = pane_id;
                                    }
                                }
                            }
                            app_data.force_immediate_render = true;
                            app_data.wayland_state.force_redraw = true;
                        }
                    }

                    let (cell_w, cell_h, _pad_x, _pad_y) = pointer_layout_metrics(&app_data);
                    let active_pane = app_data.tab_manager.active_mux().active_pane;
                    let active_rect = if let Some(pane) =
                        app_data.tab_manager.active_mux().panes.get(&active_pane)
                    {
                        pane.rect
                    } else {
                        continue;
                    };

                    let col_1 = ((app_data.pointer_x
                        - (active_rect.x as f64 + app_data.effective_pane_padding().left as f64))
                        / cell_w)
                        .max(0.0) as usize
                        + 1;
                    let row_1 = ((app_data.pointer_y
                        - (active_rect.y as f64 + app_data.effective_pane_padding().top as f64))
                        / cell_h)
                        .max(0.0) as usize
                        + 1;
                    app_data.active_mouse_button = Some(button);
                    app_data.last_mouse_col = col_1;
                    app_data.last_mouse_row = row_1;

                    if mouse_tracking_enabled {
                        let btn_code = match button {
                            272 => 0, // Left
                            274 => 1, // Middle
                            273 => 2, // Right
                            _ => 0,
                        };
                        if mouse_sgr_mode {
                            let seq = format!("\x1b[<{};{};{}M", btn_code, col_1, row_1);
                            let active_pane = app_data.tab_manager.active_mux().active_pane;
                            app_data.pane_io.send_ui_command(
                                crate::mux::io::PtyWorkerCommand::Write(
                                    active_pane,
                                    seq.into_bytes(),
                                ),
                            );
                        }
                    } else {
                        if button == 272 {
                            // Left click
                            if app_data.is_hovering_edge {
                                if let Some((thumb_y, thumb_height)) = app_data.last_scrollbar_state
                                {
                                    if app_data.pointer_y >= thumb_y
                                        && app_data.pointer_y <= thumb_y + thumb_height
                                    {
                                        app_data.is_dragging_scrollbar = true;
                                        app_data.scrollbar_drag_offset_y =
                                            app_data.pointer_y - thumb_y;
                                        continue;
                                    }
                                }
                            }

                            let col = ((app_data.pointer_x
                                - (active_rect.x as f64
                                    + app_data.effective_pane_padding().left as f64))
                                / cell_w)
                                .max(0.0) as usize;
                            let row = ((app_data.pointer_y
                                - (active_rect.y as f64
                                    + app_data.effective_pane_padding().top as f64))
                                / cell_h)
                                .max(0.0) as usize;
                            app_data.drag_start = Some((col, row));
                            let active_pane = app_data.tab_manager.active_mux().active_pane;
                            app_data.pane_io.send_ui_command(
                                crate::mux::io::PtyWorkerCommand::ClearSelection(active_pane),
                            ); // clear previous selection on click
                        } else if button == 273 {
                            // Right click
                            let active_pane = app_data.tab_manager.active_mux().active_pane;
                            let is_floating = app_data
                                .tab_manager
                                .active_mux()
                                .floating_panes
                                .contains(&active_pane);
                            let can_zoom = app_data.tab_manager.active_mux().panes.len() > 1;
                            app_data.context_menu =
                                Some(crate::context_menu::ContextMenuState::open_for_pane(
                                    ptr_x,
                                    ptr_y,
                                    Some(active_pane),
                                    can_zoom,
                                    is_floating,
                                ));
                            app_data.wayland_state.force_redraw = true;
                        } else if button == 274 {
                            // Middle click
                            request_clipboard_paste(&mut app_data);
                        }
                    }
                }
                PointerEvent::Release { button } => {
                    if button == 272 {
                        if let Some(drag) = app_data.dragging_scrolling_resize.take() {
                            app_data.active_mouse_button = None;
                            app_data.hovered_scrolling_resize = None;
                            if let Some(metrics) = app_data.cached_grid_metrics {
                                let changes: Vec<_> = drag
                                    .affected_pane_ids()
                                    .filter_map(|pane_id| {
                                        app_data
                                            .tab_manager
                                            .active_mux()
                                            .panes
                                            .get(&pane_id)
                                            .map(|pane| (pane_id, pane.grid_size))
                                    })
                                    .collect();
                                apply_scrolling_grid_changes(&mut app_data, changes, metrics);
                            }
                            app_data
                                .pane_io
                                .visible_gen
                                .fetch_add(1, std::sync::atomic::Ordering::Release);
                            app_data.force_immediate_render = true;
                            app_data.wayland_state.force_redraw = true;
                            continue;
                        }

                        if app_data.dragging_split.is_some() {
                            app_data.dragging_split = None;
                            app_data.active_mouse_button = None;

                            let mut batch = Vec::new();
                            let metrics = app_data.cached_grid_metrics.unwrap();
                            for pane in app_data.tab_manager.active_mux_mut().panes.values_mut() {
                                let px_w =
                                    (pane.grid_size.cols as f64 * metrics.effective_cell_w) as u16;
                                let px_h =
                                    (pane.grid_size.rows as f64 * metrics.effective_cell_h) as u16;
                                if let Some(pty) = pane.pty.as_mut() {
                                    let _ = pty.resize(
                                        pane.grid_size.cols as u16,
                                        pane.grid_size.rows as u16,
                                        px_w,
                                        px_h,
                                    );
                                }
                                batch.push((pane.id, pane.grid_size.cols, pane.grid_size.rows));
                            }
                            app_data.pane_io.send_ui_command(
                                crate::mux::io::PtyWorkerCommand::BatchResizeReflow(batch, None),
                            );

                            app_data.wayland_state.force_redraw = true;
                            continue;
                        }
                    }

                    app_data.active_mouse_button = None;
                    let (cell_w, cell_h, _pad_x, _pad_y) = pointer_layout_metrics(&app_data);
                    let active_pane = app_data.tab_manager.active_mux().active_pane;
                    let active_rect = if let Some(pane) =
                        app_data.tab_manager.active_mux().panes.get(&active_pane)
                    {
                        pane.rect
                    } else {
                        continue;
                    };

                    let col_1 = ((app_data.pointer_x
                        - (active_rect.x as f64 + app_data.effective_pane_padding().left as f64))
                        / cell_w)
                        .max(0.0) as usize
                        + 1;
                    let row_1 = ((app_data.pointer_y
                        - (active_rect.y as f64 + app_data.effective_pane_padding().top as f64))
                        / cell_h)
                        .max(0.0) as usize
                        + 1;

                    if mouse_tracking_enabled {
                        let btn_code = match button {
                            272 => 0, // Left
                            274 => 1, // Middle
                            273 => 2, // Right
                            _ => 0,
                        };
                        if mouse_sgr_mode {
                            let seq = format!("\x1b[<{};{};{}m", btn_code, col_1, row_1);
                            let active_pane = app_data.tab_manager.active_mux().active_pane;
                            app_data.pane_io.send_ui_command(
                                crate::mux::io::PtyWorkerCommand::Write(
                                    active_pane,
                                    seq.into_bytes(),
                                ),
                            );
                        }
                    } else {
                        if button == 272 {
                            // Left click
                            app_data.drag_start = None;
                            app_data.is_dragging_scrollbar = false;
                            if app_data.config.behavior.copy_on_select {
                                let active_pane = app_data.tab_manager.active_mux().active_pane;
                                let serial = app_data.wayland_state.pointer_button_serial;
                                copy_selection_from_pane_with_serial(
                                    &mut app_data,
                                    active_pane,
                                    serial,
                                    true,
                                );
                            }
                        }
                    }
                }
                PointerEvent::Axis { mut amount } => {
                    if app_data.is_hovering_edge {
                        amount *= 5.0;
                    }
                    if mouse_tracking_enabled {
                        let (cell_w, cell_h, _pad_x, _pad_y) = pointer_layout_metrics(&app_data);
                        let active_pane = app_data.tab_manager.active_mux().active_pane;
                        let active_rect = app_data
                            .tab_manager
                            .active_mux()
                            .panes
                            .get(&active_pane)
                            .unwrap()
                            .rect;

                        let col_1 = ((app_data.pointer_x
                            - (active_rect.x as f64
                                + app_data.effective_pane_padding().left as f64))
                            / cell_w)
                            .max(0.0) as usize
                            + 1;
                        let row_1 = ((app_data.pointer_y
                            - (active_rect.y as f64
                                + app_data.effective_pane_padding().top as f64))
                            / cell_h)
                            .max(0.0) as usize
                            + 1;

                        let btn_code = if amount > 0.0 { 65 } else { 64 };
                        if mouse_sgr_mode {
                            let seq = format!("\x1b[<{};{};{}M", btn_code, col_1, row_1);
                            let active_pane = app_data.tab_manager.active_mux().active_pane;
                            app_data.pane_io.send_ui_command(
                                crate::mux::io::PtyWorkerCommand::Write(
                                    active_pane,
                                    seq.into_bytes(),
                                ),
                            );
                        }
                    } else {
                        app_data.scroll_accum += amount;
                        let threshold = 10.0;
                        if app_data.scroll_accum >= threshold {
                            let lines = (app_data.scroll_accum / threshold) as usize;
                            app_data.scroll_accum -= lines as f64 * threshold;
                            let use_alt_buffer = app_data
                                .tab_manager
                                .active_mux()
                                .panes
                                .get(&app_data.tab_manager.active_mux().active_pane)
                                .unwrap()
                                .snapshot
                                .load()
                                .use_alt_buffer;
                            if use_alt_buffer {
                                let active_pane = app_data.tab_manager.active_mux().active_pane;
                                for _ in 0..lines {
                                    app_data.pane_io.send_ui_command(
                                        crate::mux::io::PtyWorkerCommand::Write(
                                            active_pane,
                                            b"\x1b[B".to_vec(),
                                        ),
                                    );
                                }
                            } else {
                                let offset_changed = {
                                    let active_pane = app_data.tab_manager.active_mux().active_pane;
                                    app_data.pane_io.send_ui_command(
                                        crate::mux::io::PtyWorkerCommand::ScrollDown(
                                            active_pane,
                                            lines,
                                        ),
                                    );
                                    true
                                };
                                if offset_changed {
                                    reveal_scrollbar_from_scroll(
                                        &mut app_data,
                                        std::time::Instant::now(),
                                    );
                                }
                            }
                        } else if app_data.scroll_accum <= -threshold {
                            let lines = (-app_data.scroll_accum / threshold) as usize;
                            app_data.scroll_accum += lines as f64 * threshold;
                            let use_alt_buffer = app_data
                                .tab_manager
                                .active_mux()
                                .panes
                                .get(&app_data.tab_manager.active_mux().active_pane)
                                .unwrap()
                                .snapshot
                                .load()
                                .use_alt_buffer;
                            if use_alt_buffer {
                                let active_pane = app_data.tab_manager.active_mux().active_pane;
                                for _ in 0..lines {
                                    app_data.pane_io.send_ui_command(
                                        crate::mux::io::PtyWorkerCommand::Write(
                                            active_pane,
                                            b"\x1b[A".to_vec(),
                                        ),
                                    );
                                }
                            } else {
                                let offset_changed = {
                                    let active_pane = app_data.tab_manager.active_mux().active_pane;
                                    app_data.pane_io.send_ui_command(
                                        crate::mux::io::PtyWorkerCommand::ScrollUp(
                                            active_pane,
                                            lines,
                                        ),
                                    );
                                    true
                                };
                                if offset_changed {
                                    reveal_scrollbar_from_scroll(
                                        &mut app_data,
                                        std::time::Instant::now(),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            if pending_pointer_event.is_none() {
                pending_pointer_event = app_data.pointer_receiver.try_recv().ok();
            }
        }

        if let Some(serial) = app_data.wayland_state.pending_copy_serial.take() {
            let active_pane = app_data.tab_manager.active_mux().active_pane;
            copy_selection_from_pane_with_serial(&mut app_data, active_pane, serial, false);
        }

        let mut pane_dirty_rows = std::collections::HashMap::new();
        let mut has_dirty_rows = false;
        {
            let active_pane_id = app_data.tab_manager.active_mux().active_pane;
            if let Some(pane) = app_data.tab_manager.active_mux().panes.get(&active_pane_id) {
                let sb_arc = pane.snapshot.load_full();
                let sid = sb_arc.snapshot_id;
                if app_data.last_snapshot_ids.get(&active_pane_id) != Some(&sid) {
                    has_dirty_rows = true;
                }
                pane_dirty_rows.insert(active_pane_id, sb_arc.dirty_generations.clone());
            }
        }

        let (use_alt_buffer, scrollback_lines) = {
            let active_pane = app_data.tab_manager.active_mux().active_pane;
            if let Some(pane) = app_data.tab_manager.active_mux().panes.get(&active_pane) {
                let sb = pane.snapshot.load();
                (sb.use_alt_buffer, sb.history_lines as usize)
            } else {
                (false, 0)
            }
        };
        let now = std::time::Instant::now();
        let scrollbar_wants_redraw = scrollbar_overlay_wants_redraw(
            use_alt_buffer,
            scrollback_lines,
            app_data.current_thumb_opacity,
            app_data.current_track_opacity,
            app_data.is_hovering_edge,
            app_data.is_dragging_scrollbar,
            app_data.last_mouse_activity,
            app_data.mouse_started_moving,
            now,
        );
        let scroll_animation_wants_redraw = match &app_data.pane_runtime {
            crate::mux::PaneRuntime::Scrolling(manager) => {
                manager.active_scroll_animation_active(now)
            }
            crate::mux::PaneRuntime::Tiling => false,
        };
        let mut finished_animations = Vec::new();
        let mut pane_animation_wants_redraw = false;
        for (pane_id, anim) in &app_data.pane_animations {
            if anim.is_complete(now) {
                finished_animations.push(*pane_id);
            } else {
                pane_animation_wants_redraw = true;
            }
        }
        for pane_id in finished_animations {
            app_data.pane_animations.remove(&pane_id);
            app_data.wayland_state.force_redraw = true;
        }

        app_data.closing_panes.retain(|c| {
            if c.anim.is_complete(now) {
                app_data.wayland_state.force_redraw = true;
                false
            } else {
                pane_animation_wants_redraw = true;
                true
            }
        });
        let command_indicator_animation_wants_redraw =
            command_completion_indicator_animation_active(&app_data, now);
        let cursor_trail_wants_redraw = app_data
            .renderer
            .as_ref()
            .is_some_and(|renderer| renderer.cursor_trail_wants_redraw(now));

        let wants_redraw = frame_wants_redraw(
            has_dirty_rows,
            app_data.wayland_state.force_redraw,
            scrollbar_wants_redraw,
            scroll_animation_wants_redraw,
            pane_animation_wants_redraw,
            command_indicator_animation_wants_redraw,
            cursor_trail_wants_redraw,
        );
        // FIX #1: When force_immediate_render is set (structural event like pane close),
        // bypass the frame_ready gate entirely so we don't wait up to one full vsync.
        // We still reset it immediately so only this one frame benefits from the bypass.
        let frame_gate_open = app_data.wayland_state.frame_ready || app_data.force_immediate_render;
        if app_data.force_immediate_render {
            app_data.force_immediate_render = false;
        }
        if frame_gate_open && wants_redraw {
            {
                let active_pane_id = app_data.tab_manager.active_mux().active_pane;
                let sb_arc = app_data
                    .tab_manager
                    .active_mux()
                    .panes
                    .get(&active_pane_id)
                    .unwrap()
                    .snapshot
                    .load_full();
                let sid = sb_arc.snapshot_id;
                app_data.last_snapshot_ids.insert(active_pane_id, sid);
            }
            app_data.wayland_state.frame_ready = false;

            if let Some(window) = app_data.wayland_state.window.as_ref() {
                if frame_callback_request_needed(app_data.wayland_state.frame_callback_pending) {
                    crate::wayland::frame_callback::request_frame_callback(
                        &window.surface,
                        &app_data.queue_handle,
                    );
                    app_data.wayland_state.frame_callback_pending = true;
                    app_data.wayland_state.needs_flush = true;
                }
            }

            if app_data.cached_grid_metrics.is_none() {
                let (win_w, win_h) = render_surface_size(&app_data).unwrap_or((800.0, 600.0));
                let cell_w = app_data
                    .renderer
                    .as_ref()
                    .map(|r| r.cell_width as f64)
                    .unwrap_or(10.0);
                let cell_h = app_data
                    .renderer
                    .as_ref()
                    .map(|r| r.cell_height as f64)
                    .unwrap_or(20.0);
                let metrics = compute_grid_metrics(
                    win_w,
                    win_h,
                    &app_data.config.window.padding,
                    app_data.config.window.center_grid,
                    &app_data.config.statusbar,
                    app_data.sidebar.width_cols(),
                    cell_w,
                    cell_h,
                );
                app_data.cached_grid_metrics = Some(metrics);
                apply_metrics_to_active_mux(&mut app_data, metrics);
            }

            app_data.update_statusbar(app_data.cached_grid_metrics.map(|m| m.sb_cols).unwrap_or(0));
            let effective_pane_padding = app_data.effective_pane_padding();
            let runtime_active_pane_id = app_data.active_pane_id();
            let metrics = app_data.cached_grid_metrics.unwrap();
            let (command_completion_indicator, opening_frame_generation) =
                if let Some(indicator) = latest_command_completion_indicator(&app_data) {
                    let render_data = command_completion_indicator_layout(
                        metrics,
                        indicator,
                        &app_data.config.theme,
                        &app_data.config.command_completion_indicator,
                        now,
                    );
                    let opening_frame_generation = (render_data.is_some()
                        && !indicator.opening_frame_presented
                        && now
                            >= indicator.shown_at
                                + COMMAND_INDICATOR_CIRCLE_HOLD
                                + COMMAND_INDICATOR_EXPAND)
                        .then_some(indicator.generation);
                    (render_data, opening_frame_generation)
                } else {
                    (None, None)
                };
            if let Some(renderer) = app_data.renderer.as_mut() {
                let scroll_event = app_data
                    .tab_manager
                    .active_mux()
                    .panes
                    .get(&app_data.tab_manager.active_mux().active_pane)
                    .unwrap()
                    .snapshot
                    .load()
                    .scroll_event
                    .map(renderer_scroll_event);
                let active_pane = app_data.tab_manager.active_mux().active_pane;
                let sb = app_data
                    .tab_manager
                    .active_mux()
                    .panes
                    .get(&app_data.tab_manager.active_mux().active_pane)
                    .unwrap()
                    .snapshot
                    .load();
                let bg_color = app_data.cached_bg_color;
                let final_alpha = bg_color.a * app_data.config.window.opacity;
                let default_bg = [bg_color.r, bg_color.g, bg_color.b, bg_color.a];
                let clear_color = [
                    bg_color.r * final_alpha,
                    bg_color.g * final_alpha,
                    bg_color.b * final_alpha,
                    final_alpha,
                ];

                let cursor_color = app_data.cached_cursor_color;
                let cursor_color_arr = [
                    cursor_color.r,
                    cursor_color.g,
                    cursor_color.b,
                    cursor_color.a,
                ];

                let grid_refs: Vec<&[forge_core::cell::Cell]> =
                    sb.grid.iter().map(|row| row.as_slice()).collect();
                debug_screen_artifacts(&grid_refs);

                let (win_w, win_h) = if let Some(window) = app_data.wayland_state.window.as_ref() {
                    (window.size.width as f64, window.size.height as f64)
                } else {
                    (800.0, 600.0)
                };

                let mut scrollbar_state = None;
                let mut needs_scrollbar_redraw = false;
                // DA06k/l - Minimal Scrollbar Math
                if !sb.use_alt_buffer {
                    let visible_screen_lines = sb.grid.len() as f64;
                    let history_lines = sb.history_lines;
                    let total_lines = visible_screen_lines + history_lines;

                    if total_lines > visible_screen_lines {
                        let thumb_height_percentage = visible_screen_lines / total_lines;
                        let minimum_thumb_height_px = 20.0_f64;
                        let thumb_height_pixels =
                            minimum_thumb_height_px.max(win_h * thumb_height_percentage);

                        let viewport_offset = sb.viewport_offset;
                        let scroll_ratio = if history_lines > 0.0 {
                            viewport_offset / history_lines
                        } else {
                            0.0
                        };
                        let track_top = 4.0;
                        let track_bottom = win_h - 4.0;
                        let usable_track_height = track_bottom - track_top;

                        let available_travel_space = usable_track_height - thumb_height_pixels;
                        let thumb_y = track_top + available_travel_space
                            - (scroll_ratio * available_travel_space);
                        let idle_secs = now
                            .duration_since(app_data.last_mouse_activity)
                            .as_secs_f32();
                        let active_secs = now
                            .duration_since(app_data.mouse_started_moving)
                            .as_secs_f32();
                        let mut target_thumb_opacity = if active_secs >= 0.25 && idle_secs < 0.5 {
                            1.0
                        } else {
                            0.0
                        };
                        let mut target_track_opacity = 0.0;
                        let mut target_thumb_width = 5.0;

                        if app_data.is_hovering_edge || app_data.is_dragging_scrollbar {
                            target_track_opacity = 1.0;
                            target_thumb_width = 9.0;
                            target_thumb_opacity = 1.0;
                        }

                        app_data.current_thumb_width +=
                            (target_thumb_width - app_data.current_thumb_width) * 0.2;
                        app_data.current_track_opacity +=
                            (target_track_opacity - app_data.current_track_opacity) * 0.2;
                        app_data.current_thumb_opacity +=
                            (target_thumb_opacity - app_data.current_thumb_opacity) * 0.2;

                        let is_animating = (target_thumb_width - app_data.current_thumb_width)
                            .abs()
                            > 0.01
                            || (target_track_opacity - app_data.current_track_opacity).abs() > 0.01
                            || (target_thumb_opacity - app_data.current_thumb_opacity).abs() > 0.01
                            || (idle_secs < 0.5 && app_data.current_thumb_opacity > 0.01);

                        if is_animating {
                            needs_scrollbar_redraw = true;
                        }

                        let thumb_x = win_w as f32 - 4.0 - app_data.current_thumb_width;
                        let thumb_opacity = app_data.current_thumb_opacity;
                        let track_opacity = app_data.current_track_opacity;
                        let thumb_width = app_data.current_thumb_width;

                        if thumb_opacity > 0.01 || track_opacity > 0.01 {
                            scrollbar_state = Some((
                                thumb_y as f32,
                                thumb_height_pixels as f32,
                                thumb_width,
                                thumb_x,
                                thumb_opacity,
                                track_opacity,
                            ));
                        }
                        app_data.last_scrollbar_state = Some((thumb_y, thumb_height_pixels));
                    }
                }

                let modal_open = app_data.active_modal.is_some();
                let modal_blank_screen = modal_open
                    && app_data.config.confirm_close.background_mode
                        == forge_core::config_registry::ConfirmCloseBackgroundMode::BlankScreen;
                if modal_blank_screen {
                    scrollbar_state = None;
                }

                let mut statusbar_hover = None;
                let mut needs_statusbar_hover_redraw = false;

                if app_data.config.statusbar.enabled && !modal_blank_screen {
                    let mut is_hovering = false;
                    if app_data.is_hovering_statusbar {
                        if let Some(action) = app_data.statusbar.hovered_action.as_deref() {
                            if action == "NewTab" {
                                is_hovering = true;
                            } else if let Some(tab) = action.strip_prefix("SwitchTab") {
                                if let Ok(tab_num) = tab.parse::<usize>() {
                                    if tab_num > 0
                                        && tab_num - 1 != app_data.tab_manager.active_tab_index
                                    {
                                        is_hovering = true;
                                    }
                                }
                            }
                        }
                    }
                    let target_opacity = if is_hovering { 1.0 } else { 0.0 };

                    let diff = target_opacity - app_data.statusbar.hover_opacity;
                    if diff.abs() > 0.01 {
                        app_data.statusbar.hover_opacity += diff * 0.5;
                        needs_statusbar_hover_redraw = true;
                    } else {
                        app_data.statusbar.hover_opacity = target_opacity;
                    }

                    if app_data.statusbar.hover_opacity > 0.01 {
                        if let Some((start_col, end_col)) = app_data.statusbar.hovered_region {
                            let orig_x = metrics.sb_x as f32
                                + (start_col as f64 * metrics.effective_cell_w) as f32;
                            let orig_w =
                                ((end_col - start_col) as f64 * metrics.effective_cell_w) as f32;
                            let size = metrics.effective_cell_h as f32;

                            let (x, width) = if app_data.statusbar.hovered_is_square {
                                (orig_x + (orig_w - size) / 2.0, size)
                            } else {
                                (orig_x, orig_w)
                            };

                            statusbar_hover =
                                Some(forge_renderer::grid_tessellator::StatusbarHoverRenderData {
                                    x,
                                    y: metrics.sb_y as f32,
                                    width,
                                    height: size,
                                    opacity: app_data.statusbar.hover_opacity,
                                    color: app_data.cached_statusbar_hover_color,
                                });
                        }
                    }
                }

                let selection_bg_color = app_data.cached_selection_bg_color;
                let selection_bg_arr = [
                    selection_bg_color.r,
                    selection_bg_color.g,
                    selection_bg_color.b,
                    selection_bg_color.a,
                ];

                let mut all_grid_refs: Vec<Vec<&[forge_core::cell::Cell]>> = Vec::new();
                let mut all_dirty_rows = Vec::new();
                let mut closing_grid_refs: Vec<Vec<&[forge_core::cell::Cell]>> = Vec::new();
                let mut closing_dirty_rows: Vec<Vec<u64>> = Vec::new();
                let mut pane_render_inputs = Vec::new();

                #[derive(Clone, Copy)]
                struct PaneRenderSpan {
                    pane_id: crate::mux::PaneId,
                    rect: crate::mux::PaneRect,
                    logical_rect: crate::mux::PaneRect,
                    visible_col_start: usize,
                    visible_row_start: usize,
                    visible_cols: usize,
                    visible_rows: usize,
                    is_partial: bool,
                    overflow: crate::mux::ScrollingOverflowIndicators,
                    is_floating: bool,
                }

                let pane_spans: Vec<PaneRenderSpan> = if modal_blank_screen {
                    Vec::new()
                } else {
                    let mut spans: Vec<PaneRenderSpan> = match &mut app_data.pane_runtime {
                        crate::mux::PaneRuntime::Tiling => {
                            let visible_pane_ids =
                                app_data.tab_manager.active_mux().visible_pane_ids();
                            app_data
                                .tab_manager
                                .active_mux()
                                .panes
                                .iter()
                                .filter(|(pane_id, _)| {
                                    visible_pane_ids.contains(pane_id)
                                        && !app_data
                                            .tab_manager
                                            .active_mux()
                                            .floating_panes
                                            .contains(pane_id)
                                })
                                .map(|(pane_id, p)| PaneRenderSpan {
                                    pane_id: *pane_id,
                                    rect: p.rect,
                                    logical_rect: p.rect,
                                    visible_col_start: 0,
                                    visible_row_start: 0,
                                    visible_cols: p.grid_size.cols,
                                    visible_rows: p.grid_size.rows,
                                    is_partial: false,
                                    overflow: crate::mux::ScrollingOverflowIndicators::NONE,
                                    is_floating: false,
                                })
                                .collect()
                        }
                        crate::mux::PaneRuntime::Scrolling(manager) => manager
                            .active_render_visible_panes(now)
                            .iter()
                            .map(|visible| {
                                let viewport_col = (visible.viewport_col
                                    + visible.visible_col_start as f32)
                                    .max(0.0);
                                let viewport_row = (visible.viewport_row
                                    + visible.visible_row_start as f32)
                                    .max(0.0);
                                PaneRenderSpan {
                                    pane_id: visible.pane_id,
                                    rect: crate::mux::PaneRect::new(
                                        metrics.pad_x as f32
                                            + viewport_col * metrics.effective_cell_w as f32,
                                        metrics.pad_y as f32
                                            + viewport_row * metrics.effective_cell_h as f32,
                                        visible.visible_cols as f32
                                            * metrics.effective_cell_w as f32,
                                        visible.visible_rows as f32
                                            * metrics.effective_cell_h as f32,
                                    ),
                                    logical_rect: crate::mux::PaneRect::new(
                                        visible.virtual_rect.col as f32
                                            * metrics.effective_cell_w as f32,
                                        visible.virtual_rect.row as f32
                                            * metrics.effective_cell_h as f32,
                                        visible.virtual_rect.cols as f32
                                            * metrics.effective_cell_w as f32,
                                        visible.virtual_rect.rows as f32
                                            * metrics.effective_cell_h as f32,
                                    ),
                                    visible_col_start: visible.visible_col_start,
                                    visible_row_start: visible.visible_row_start,
                                    visible_cols: visible.visible_cols,
                                    visible_rows: visible.visible_rows,
                                    is_partial: visible.visible_col_start > 0
                                        || visible.visible_row_start > 0
                                        || visible.visible_cols < visible.grid_size.cols
                                        || visible.visible_rows < visible.grid_size.rows,
                                    overflow: visible.overflow,
                                    is_floating: false,
                                }
                            })
                            .collect(),
                    };
                    for &fp_id in &app_data.tab_manager.active_mux().floating_panes {
                        if let Some(p) = app_data.tab_manager.active_mux().panes.get(&fp_id) {
                            spans.push(PaneRenderSpan {
                                pane_id: fp_id,
                                rect: p.rect,
                                logical_rect: p.rect,
                                visible_col_start: 0,
                                visible_row_start: 0,
                                visible_cols: p.grid_size.cols,
                                visible_rows: p.grid_size.rows,
                                is_partial: false,
                                overflow: crate::mux::ScrollingOverflowIndicators::NONE,
                                is_floating: true,
                            });
                        }
                    }
                    spans
                };

                let panes_data: Vec<_> = if modal_blank_screen {
                    Vec::new()
                } else {
                    pane_spans
                        .iter()
                        .filter_map(|span| {
                            let pane =
                                app_data.tab_manager.active_mux().panes.get(&span.pane_id)?;
                            Some((*span, pane.snapshot.load()))
                        })
                        .collect()
                };

                if !modal_blank_screen {
                    let active_pane_id = runtime_active_pane_id;

                    let play_anim = !matches!(
                        app_data.config.render.pane_animation,
                        forge_core::config_registry::PaneAnimationMode::None
                    );

                    let is_tab_switch_or_new = !pane_spans.is_empty()
                        && !pane_spans
                            .iter()
                            .any(|span| app_data.last_layout_rects.contains_key(&span.pane_id));

                    if play_anim && !is_tab_switch_or_new {
                        let duration = Duration::from_millis(
                            app_data.config.render.pane_animation_duration_ms as u64,
                        );
                        for span in &pane_spans {
                            if let Some((old_logical_rect, _)) =
                                app_data.last_layout_rects.get(&span.pane_id)
                            {
                                if old_logical_rect != &span.logical_rect {
                                    let mut current_start = *old_logical_rect;
                                    if let Some(anim) = app_data.pane_animations.get(&span.pane_id)
                                    {
                                        let p = anim.progress(now);
                                        let inv = 1.0 - p;
                                        current_start = crate::mux::PaneRect {
                                            x: old_logical_rect.x + anim.dx * inv,
                                            y: old_logical_rect.y + anim.dy * inv,
                                            width: (old_logical_rect.width + anim.dw * inv)
                                                .max(0.0),
                                            height: (old_logical_rect.height + anim.dh * inv)
                                                .max(0.0),
                                        };
                                    }
                                    let dx = current_start.x - span.logical_rect.x;
                                    let dy = current_start.y - span.logical_rect.y;
                                    let dw = current_start.width - span.logical_rect.width;
                                    let dh = current_start.height - span.logical_rect.height;

                                    tracing::trace!(
                                        "Creating Move animation for pane {:?}: dx={}, dy={}",
                                        span.pane_id,
                                        dx,
                                        dy
                                    );

                                    app_data.pane_animations.insert(
                                        span.pane_id,
                                        PaneAnimation {
                                            kind: PaneAnimationKind::Move,
                                            dx,
                                            dy,
                                            dw,
                                            dh,
                                            started_at: Instant::now(),
                                            duration,
                                        },
                                    );
                                } else {
                                    tracing::trace!("old_logical_rect == span.logical_rect for pane {:?}, no animation created", span.pane_id);
                                }
                            } else {
                                // New pane open animation! Expand from its center.
                                tracing::info!(
                                    "Creating Open animation for pane {:?}",
                                    span.pane_id
                                );
                                let start_rect = crate::mux::PaneRect {
                                    x: span.logical_rect.x + span.logical_rect.width / 2.0,
                                    y: span.logical_rect.y + span.logical_rect.height / 2.0,
                                    width: 0.0,
                                    height: 0.0,
                                };
                                let dx = start_rect.x - span.logical_rect.x;
                                let dy = start_rect.y - span.logical_rect.y;
                                let dw = start_rect.width - span.logical_rect.width;
                                let dh = start_rect.height - span.logical_rect.height;
                                app_data.pane_animations.insert(
                                    span.pane_id,
                                    PaneAnimation {
                                        kind: PaneAnimationKind::Open,
                                        dx,
                                        dy,
                                        dw,
                                        dh,
                                        started_at: Instant::now(),
                                        duration,
                                    },
                                );
                            }
                        }
                    }

                    let active_panes = &app_data.tab_manager.active_mux().panes;
                    app_data
                        .last_layout_rects
                        .retain(|id, _| active_panes.contains_key(id));
                    for span in &pane_spans {
                        app_data
                            .last_layout_rects
                            .insert(span.pane_id, (span.logical_rect, span.rect));
                    }

                    for (span, snap) in &panes_data {
                        let row_start = span.visible_row_start.min(snap.grid.len());
                        let row_end = row_start
                            .saturating_add(span.visible_rows)
                            .min(snap.grid.len());
                        let col_start = span.visible_col_start;
                        let col_count = span.visible_cols;

                        all_grid_refs.push(
                            snap.grid[row_start..row_end]
                                .iter()
                                .map(|row| {
                                    let start = col_start.min(row.len());
                                    let end = start.saturating_add(col_count).min(row.len());
                                    &row[start..end]
                                })
                                .collect(),
                        );

                        if let Some(gens) = pane_dirty_rows.remove(&span.pane_id) {
                            all_dirty_rows.push(
                                gens.get(row_start..row_end)
                                    .map(|rows| rows.to_vec())
                                    .unwrap_or_else(|| vec![1; row_end.saturating_sub(row_start)]),
                            );
                        } else {
                            all_dirty_rows.push(vec![1; row_end.saturating_sub(row_start)]);
                        }
                    }

                    for (i, (span, snap)) in panes_data.iter().enumerate() {
                        let is_active = span.pane_id == active_pane_id;
                        let cursor_style = snap
                            .cursor_style_override
                            .unwrap_or(app_data.config.cursor.style);
                        let cursor_blink = snap
                            .cursor_blink_override
                            .unwrap_or(app_data.config.cursor.blink);
                        let cursor_visible_phase = if cursor_blink {
                            app_data.cursor_visible_phase
                        } else {
                            true
                        };
                        let cursor = translate_visible_cursor(
                            snap.cursor,
                            span.visible_col_start,
                            span.visible_row_start,
                            span.visible_cols,
                            span.visible_rows,
                        );
                        let selection = translate_visible_selection(
                            snap.selection,
                            span.visible_col_start,
                            span.visible_row_start,
                            span.visible_cols,
                            span.visible_rows,
                        );
                        let (render_rect, opacity) = if let Some(anim) =
                            app_data.pane_animations.get(&span.pane_id)
                        {
                            let p = anim.progress(now);
                            let inv = 1.0 - p;
                            match anim.kind {
                                PaneAnimationKind::Move => (
                                    forge_renderer::renderer::PaneRenderRect {
                                        x: span.rect.x + anim.dx * inv,
                                        y: span.rect.y + anim.dy * inv,
                                        width: (span.rect.width + anim.dw * inv).max(0.0),
                                        height: (span.rect.height + anim.dh * inv).max(0.0),
                                    },
                                    1.0,
                                ),
                                PaneAnimationKind::Open => {
                                    if app_data.config.render.pane_animation
                                        == forge_core::config_registry::PaneAnimationMode::Fade
                                    {
                                        (
                                            forge_renderer::renderer::PaneRenderRect {
                                                x: span.rect.x,
                                                y: span.rect.y,
                                                width: span.rect.width,
                                                height: span.rect.height,
                                            },
                                            p,
                                        )
                                    } else {
                                        (
                                            forge_renderer::renderer::PaneRenderRect {
                                                x: span.rect.x + anim.dx * inv,
                                                y: span.rect.y + anim.dy * inv,
                                                width: (span.rect.width + anim.dw * inv).max(0.0),
                                                height: (span.rect.height + anim.dh * inv).max(0.0),
                                            },
                                            1.0,
                                        )
                                    }
                                }
                                PaneAnimationKind::Close => (
                                    forge_renderer::renderer::PaneRenderRect {
                                        x: span.rect.x,
                                        y: span.rect.y,
                                        width: span.rect.width,
                                        height: span.rect.height,
                                    },
                                    1.0,
                                ),
                            }
                        } else {
                            (
                                forge_renderer::renderer::PaneRenderRect {
                                    x: span.rect.x,
                                    y: span.rect.y,
                                    width: span.rect.width,
                                    height: span.rect.height,
                                },
                                1.0,
                            )
                        };
                        pane_render_inputs.push(forge_renderer::renderer::PaneRenderInput {
                            pane_id: forge_renderer::renderer::PaneRenderId(span.pane_id.get()),
                            rect: render_rect,
                            opacity,
                            layer: if span.is_floating {
                                forge_renderer::renderer::PaneRenderLayer::Floating
                            } else {
                                forge_renderer::renderer::PaneRenderLayer::Normal
                            },
                            apply_pane_padding: true,
                            grid: &all_grid_refs[i],
                            dirty_generations: &all_dirty_rows[i],
                            cursor: if is_active { cursor } else { None },
                            cursor_style,
                            cursor_visible_phase,
                            selection,
                            default_bg,
                            cursor_color: cursor_color_arr,
                            selection_bg: selection_bg_arr,
                            viewport_offset: snap.viewport_offset,
                            scroll_event: if is_active && !span.is_partial {
                                scroll_event
                            } else {
                                None
                            },
                            scroll_id: span.pane_id.get(),
                            is_active,
                            overflow_indicators: forge_renderer::renderer::PaneOverflowIndicators {
                                above: span.overflow.above,
                                below: span.overflow.below,
                                left: span.overflow.left,
                                right: span.overflow.right,
                            },
                        });
                    }

                    for c in &app_data.closing_panes {
                        let grid_refs: Vec<&[forge_core::cell::Cell]> =
                            c.snapshot.grid.iter().map(|row| row.as_slice()).collect();
                        closing_grid_refs.push(grid_refs);
                        closing_dirty_rows.push(vec![1; c.snapshot.grid.len()]);
                    }

                    for (i, c) in app_data.closing_panes.iter().enumerate() {
                        let p = c.anim.progress(now);
                        let inv = 1.0 - p;
                        let rect = c.rect;

                        let (render_rect, opacity) = if app_data.config.render.pane_animation
                            == forge_core::config_registry::PaneAnimationMode::Fade
                        {
                            (
                                forge_renderer::renderer::PaneRenderRect {
                                    x: rect.x,
                                    y: rect.y,
                                    width: rect.width,
                                    height: rect.height,
                                },
                                inv,
                            )
                        } else {
                            let new_w = rect.width * inv;
                            let new_h = rect.height * inv;
                            let ox = (rect.width - new_w) / 2.0;
                            let oy = (rect.height - new_h) / 2.0;
                            (
                                forge_renderer::renderer::PaneRenderRect {
                                    x: rect.x + ox,
                                    y: rect.y + oy,
                                    width: new_w.max(0.0),
                                    height: new_h.max(0.0),
                                },
                                1.0,
                            )
                        };

                        pane_render_inputs.push(forge_renderer::renderer::PaneRenderInput {
                            pane_id: forge_renderer::renderer::PaneRenderId(c.pane_id.get()),
                            rect: render_rect,
                            opacity,
                            layer: if c.is_floating {
                                forge_renderer::renderer::PaneRenderLayer::Floating
                            } else {
                                forge_renderer::renderer::PaneRenderLayer::Normal
                            },
                            apply_pane_padding: true,
                            grid: &closing_grid_refs[i],
                            dirty_generations: &closing_dirty_rows[i],
                            cursor: None,
                            cursor_style: forge_core::config_registry::CursorStyle::Block,
                            cursor_visible_phase: false,
                            selection: None,
                            default_bg,
                            cursor_color: cursor_color_arr,
                            selection_bg: selection_bg_arr,
                            viewport_offset: c.snapshot.viewport_offset,
                            scroll_event: None,
                            scroll_id: c.pane_id.get(),
                            is_active: false,
                            overflow_indicators:
                                forge_renderer::renderer::PaneOverflowIndicators::default(),
                        });
                    }
                }

                struct OverflowIndicatorSpec {
                    rect: forge_renderer::renderer::PaneRenderRect,
                    grid: Vec<Vec<forge_core::cell::Cell>>,
                }

                let mut overflow_indicator_specs = Vec::new();
                if matches!(app_data.pane_runtime, crate::mux::PaneRuntime::Scrolling(_)) {
                    let cell_w = metrics.effective_cell_w as f32;
                    let cell_h = metrics.effective_cell_h as f32;
                    for span in &pane_spans {
                        if !span.overflow.any() || span.rect.width <= 0.0 || span.rect.height <= 0.0
                        {
                            continue;
                        }
                        let fg = if span.pane_id == runtime_active_pane_id {
                            app_data.config.theme.parsed_pane_outline_active
                        } else {
                            app_data.config.theme.parsed_pane_outline_inactive
                        };
                        let make_cell = |c| forge_core::cell::Cell {
                            c,
                            fg,
                            bg: forge_core::color::Color::TRANSPARENT,
                            flags: 0,
                        };
                        let blank = make_cell(' ');
                        let center_x = span.rect.x + (span.rect.width - cell_w) * 0.5;
                        let center_y = span.rect.y + (span.rect.height - cell_h) * 0.5;
                        if span.overflow.above {
                            overflow_indicator_specs.push(OverflowIndicatorSpec {
                                rect: forge_renderer::renderer::PaneRenderRect {
                                    x: (center_x - cell_w).round(),
                                    y: (span.rect.y - cell_h * 0.5).round(),
                                    width: cell_w * 3.0,
                                    height: cell_h,
                                },
                                grid: vec![vec![blank, make_cell(''), blank]],
                            });
                        }
                        if span.overflow.below {
                            overflow_indicator_specs.push(OverflowIndicatorSpec {
                                rect: forge_renderer::renderer::PaneRenderRect {
                                    x: (center_x - cell_w).round(),
                                    y: (span.rect.y + span.rect.height - cell_h * 0.5).round(),
                                    width: cell_w * 3.0,
                                    height: cell_h,
                                },
                                grid: vec![vec![blank, make_cell(''), blank]],
                            });
                        }
                        if span.overflow.left {
                            overflow_indicator_specs.push(OverflowIndicatorSpec {
                                rect: forge_renderer::renderer::PaneRenderRect {
                                    x: (span.rect.x - cell_w * 0.5).round(),
                                    y: center_y.round(),
                                    width: cell_w,
                                    height: cell_h,
                                },
                                grid: vec![vec![make_cell('')]],
                            });
                        }
                        if span.overflow.right {
                            overflow_indicator_specs.push(OverflowIndicatorSpec {
                                rect: forge_renderer::renderer::PaneRenderRect {
                                    x: (span.rect.x + span.rect.width - cell_w * 0.5).round(),
                                    y: center_y.round(),
                                    width: cell_w,
                                    height: cell_h,
                                },
                                grid: vec![vec![make_cell('')]],
                            });
                        }
                    }
                }

                let overflow_indicator_grids: Vec<Vec<Vec<forge_core::cell::Cell>>> =
                    overflow_indicator_specs
                        .iter()
                        .map(|spec| spec.grid.clone())
                        .collect();
                let overflow_indicator_dirty_generation = app_data
                    .pane_io
                    .visible_gen
                    .load(std::sync::atomic::Ordering::Acquire)
                    ^ runtime_active_pane_id.get();
                let overflow_indicator_dirty: Vec<Vec<u64>> = overflow_indicator_grids
                    .iter()
                    .map(|grid| vec![overflow_indicator_dirty_generation; grid.len()])
                    .collect();
                let overflow_indicator_refs: Vec<Vec<&[forge_core::cell::Cell]>> =
                    overflow_indicator_grids
                        .iter()
                        .map(|grid| grid.iter().map(|row| row.as_slice()).collect())
                        .collect();

                for (idx, spec) in overflow_indicator_specs.iter().enumerate() {
                    pane_render_inputs.push(forge_renderer::renderer::PaneRenderInput {
                        pane_id: forge_renderer::renderer::PaneRenderId(u64::MAX - 32 - idx as u64),
                        rect: spec.rect,
                        opacity: 1.0,
                        layer: forge_renderer::renderer::PaneRenderLayer::Normal,
                        apply_pane_padding: true,
                        grid: &overflow_indicator_refs[idx],
                        dirty_generations: &overflow_indicator_dirty[idx],
                        cursor: None,
                        cursor_style: forge_core::config_registry::CursorStyle::Block,
                        cursor_visible_phase: false,
                        selection: None,
                        default_bg: [0.0, 0.0, 0.0, 0.0],
                        cursor_color: cursor_color_arr,
                        selection_bg: selection_bg_arr,
                        viewport_offset: 0.0,
                        scroll_event: None,
                        scroll_id: u64::MAX - 32 - idx as u64,
                        is_active: false,
                        overflow_indicators:
                            forge_renderer::renderer::PaneOverflowIndicators::default(),
                    });
                }

                let sb_grid = vec![app_data.statusbar.cells.as_slice()];
                let sb_dirty = vec![app_data.statusbar.generation];

                if metrics.sb_enabled && !modal_blank_screen {
                    pane_render_inputs.push(forge_renderer::renderer::PaneRenderInput {
                        pane_id: forge_renderer::renderer::PaneRenderId(u64::MAX),
                        rect: forge_renderer::renderer::PaneRenderRect {
                            x: metrics.sb_x as f32,
                            y: metrics.sb_y as f32,
                            width: (metrics.sb_cols as f64 * metrics.effective_cell_w) as f32,
                            height: metrics.effective_cell_h as f32,
                        },
                        opacity: 1.0,
                        layer: forge_renderer::renderer::PaneRenderLayer::Normal,
                        apply_pane_padding: true,
                        grid: &sb_grid,
                        dirty_generations: &sb_dirty,
                        cursor: None,
                        cursor_style: forge_core::config_registry::CursorStyle::Block,
                        cursor_visible_phase: false,
                        selection: None,
                        default_bg,
                        cursor_color: cursor_color_arr,
                        selection_bg: selection_bg_arr,
                        viewport_offset: 0.0,
                        scroll_event: None,
                        scroll_id: u64::MAX,
                        is_active: false,
                        overflow_indicators:
                            forge_renderer::renderer::PaneOverflowIndicators::default(),
                    });
                }

                let sidebar_grid = if app_data.sidebar.visible && !modal_blank_screen {
                    let sidebar_rows = (win_h / metrics.effective_cell_h).ceil().max(1.0) as usize;
                    Some(
                        app_data
                            .sidebar
                            .render_grid(sidebar_rows, &app_data.config.theme),
                    )
                } else {
                    None
                };
                let sidebar_dirty = sidebar_grid
                    .as_ref()
                    .map(|grid| vec![app_data.sidebar.generation; grid.len()]);
                let sidebar_refs = sidebar_grid.as_ref().map(|grid| {
                    grid.iter()
                        .map(|row| row.as_slice())
                        .collect::<Vec<&[forge_core::cell::Cell]>>()
                });

                if let (Some(grid), Some(dirty), Some(refs)) = (
                    sidebar_grid.as_ref(),
                    sidebar_dirty.as_ref(),
                    sidebar_refs.as_ref(),
                ) {
                    let sidebar_cols = grid.first().map(|row| row.len()).unwrap_or(1);
                    let sidebar_rows = grid.len().max(1);
                    pane_render_inputs.push(forge_renderer::renderer::PaneRenderInput {
                        pane_id: forge_renderer::renderer::PaneRenderId(u64::MAX - 3),
                        rect: forge_renderer::renderer::PaneRenderRect {
                            x: -(effective_pane_padding.left as f32),
                            y: -(effective_pane_padding.top as f32),
                            width: (sidebar_cols as f64 * metrics.effective_cell_w) as f32
                                + effective_pane_padding.left as f32,
                            height: (sidebar_rows as f64 * metrics.effective_cell_h) as f32
                                + effective_pane_padding.top as f32,
                        },
                        opacity: 1.0,
                        layer: forge_renderer::renderer::PaneRenderLayer::Normal,
                        apply_pane_padding: true,
                        grid: refs,
                        dirty_generations: dirty,
                        cursor: None,
                        cursor_style: forge_core::config_registry::CursorStyle::Block,
                        cursor_visible_phase: false,
                        selection: None,
                        default_bg,
                        cursor_color: cursor_color_arr,
                        selection_bg: selection_bg_arr,
                        viewport_offset: 0.0,
                        scroll_event: None,
                        scroll_id: u64::MAX - 3,
                        is_active: false,
                        overflow_indicators:
                            forge_renderer::renderer::PaneOverflowIndicators::default(),
                    });
                }

                let modal_base_grid = app_data.active_modal.as_ref().map(|modal| {
                    modal.render_base_grid(
                        metrics.cols.max(1),
                        metrics.rows.max(1),
                        &app_data.config.theme,
                        &app_data.config.confirm_close,
                    )
                });
                let modal_text_grid = app_data.active_modal.as_ref().map(|modal| {
                    modal.render_text_grid(
                        metrics.cols.max(1),
                        metrics.rows.max(1),
                        &app_data.config.theme,
                        &app_data.config.confirm_close,
                    )
                });
                let modal_base_dirty = modal_base_grid
                    .as_ref()
                    .map(|grid| vec![app_data.modal_generation; grid.len()]);
                let modal_text_dirty = modal_text_grid
                    .as_ref()
                    .map(|grid| vec![app_data.modal_generation; grid.len()]);
                let modal_base_refs = modal_base_grid.as_ref().map(|grid| {
                    grid.iter()
                        .map(|row| row.as_slice())
                        .collect::<Vec<&[forge_core::cell::Cell]>>()
                });
                let modal_text_refs = modal_text_grid.as_ref().map(|grid| {
                    grid.iter()
                        .map(|row| row.as_slice())
                        .collect::<Vec<&[forge_core::cell::Cell]>>()
                });

                if let (Some(grid), Some(dirty), Some(refs)) = (
                    modal_base_grid.as_ref(),
                    modal_base_dirty.as_ref(),
                    modal_base_refs.as_ref(),
                ) {
                    let modal_cols = grid.first().map(|row| row.len()).unwrap_or(1);
                    let modal_rows = grid.len().max(1);
                    let modal_w = modal_cols as f64 * metrics.effective_cell_w;
                    let modal_h = modal_rows as f64 * metrics.effective_cell_h;

                    pane_render_inputs.push(forge_renderer::renderer::PaneRenderInput {
                        pane_id: forge_renderer::renderer::PaneRenderId(u64::MAX - 1),
                        rect: forge_renderer::renderer::PaneRenderRect {
                            x: -(effective_pane_padding.left as f32),
                            y: -(effective_pane_padding.top as f32),
                            width: modal_w as f32
                                + effective_pane_padding.left as f32
                                + effective_pane_padding.right as f32,
                            height: modal_h as f32
                                + effective_pane_padding.top as f32
                                + effective_pane_padding.bottom as f32,
                        },
                        opacity: 1.0,
                        layer: forge_renderer::renderer::PaneRenderLayer::Modal,
                        apply_pane_padding: true,
                        grid: refs,
                        dirty_generations: dirty,
                        cursor: None,
                        cursor_style: forge_core::config_registry::CursorStyle::Block,
                        cursor_visible_phase: false,
                        selection: None,
                        default_bg: [0.0, 0.0, 0.0, 0.0],
                        cursor_color: cursor_color_arr,
                        selection_bg: selection_bg_arr,
                        viewport_offset: 0.0,
                        scroll_event: None,
                        scroll_id: u64::MAX - 1,
                        is_active: false,
                        overflow_indicators:
                            forge_renderer::renderer::PaneOverflowIndicators::default(),
                    });
                }

                if let (Some(grid), Some(dirty), Some(refs)) = (
                    modal_text_grid.as_ref(),
                    modal_text_dirty.as_ref(),
                    modal_text_refs.as_ref(),
                ) {
                    let modal_cols = grid.first().map(|row| row.len()).unwrap_or(1);
                    let modal_rows = grid.len().max(1);
                    let modal_w = modal_cols as f64 * metrics.effective_cell_w;
                    let modal_h = modal_rows as f64 * metrics.effective_cell_h;

                    pane_render_inputs.push(forge_renderer::renderer::PaneRenderInput {
                        pane_id: forge_renderer::renderer::PaneRenderId(u64::MAX - 2),
                        rect: forge_renderer::renderer::PaneRenderRect {
                            x: -(effective_pane_padding.left as f32),
                            y: -(effective_pane_padding.top as f32),
                            width: modal_w as f32
                                + effective_pane_padding.left as f32
                                + effective_pane_padding.right as f32,
                            height: modal_h as f32
                                + effective_pane_padding.top as f32
                                + effective_pane_padding.bottom as f32,
                        },
                        opacity: 1.0,
                        layer: forge_renderer::renderer::PaneRenderLayer::Modal,
                        apply_pane_padding: true,
                        grid: refs,
                        dirty_generations: dirty,
                        cursor: None,
                        cursor_style: forge_core::config_registry::CursorStyle::Block,
                        cursor_visible_phase: false,
                        selection: None,
                        default_bg: [0.0, 0.0, 0.0, 0.0],
                        cursor_color: cursor_color_arr,
                        selection_bg: selection_bg_arr,
                        viewport_offset: 0.0,
                        scroll_event: None,
                        scroll_id: u64::MAX - 2,
                        is_active: false,
                        overflow_indicators:
                            forge_renderer::renderer::PaneOverflowIndicators::default(),
                    });
                }

                let split_borders: Vec<_> = if app_data.active_modal.is_some() {
                    Vec::new()
                } else if app_data.pane_runtime.is_tiling() {
                    app_data
                        .tab_manager
                        .active_mux()
                        .visible_borders()
                        .iter()
                        .map(|b| forge_renderer::renderer::SplitBorderRenderInput {
                            rect: forge_renderer::renderer::PaneRenderRect {
                                x: b.rect.x,
                                y: b.rect.y,
                                width: b.rect.width,
                                height: b.rect.height,
                            },
                            color: [0.3, 0.3, 0.3, 1.0], // Simple gray for now
                        })
                        .collect()
                } else if app_data.config.window.gap == 0 {
                    let mut dividers = Vec::new();
                    for (index, first) in pane_spans.iter().enumerate() {
                        if first.is_floating {
                            continue;
                        }
                        for second in &pane_spans[index + 1..] {
                            if second.is_floating {
                                continue;
                            }
                            let first_rect = forge_renderer::renderer::PaneRenderRect::new(
                                first.rect.x,
                                first.rect.y,
                                first.rect.width,
                                first.rect.height,
                            );
                            let second_rect = forge_renderer::renderer::PaneRenderRect::new(
                                second.rect.x,
                                second.rect.y,
                                second.rect.width,
                                second.rect.height,
                            );
                            if let Some(rect) = forge_renderer::renderer::adjacent_pane_divider(
                                first_rect,
                                second_rect,
                                metrics.effective_cell_w as f32,
                                metrics.effective_cell_h as f32,
                            ) {
                                dividers.push(
                                    forge_renderer::renderer::SplitBorderRenderInput {
                                        rect,
                                        color: [0.3, 0.3, 0.3, 1.0],
                                    },
                                );
                            }
                        }
                    }
                    dividers
                } else {
                    Vec::new()
                };

                renderer.set_ligature_config(app_data.config.font.ligatures.clone());
                let (needs_recreate, render_succeeded) = match renderer.render_panes(
                    &pane_render_inputs,
                    &split_borders,
                    clear_color,
                    metrics.effective_cell_w as f32,
                    metrics.effective_cell_h as f32,
                    scrollbar_state,
                    app_data.context_menu.as_ref().map(|cm| {
                        cm.render_data(
                            win_w,
                            win_h,
                            metrics.effective_cell_w as f32,
                            metrics.effective_cell_h as f32,
                            color_to_render_array(app_data.config.theme.parsed_popup_background),
                        )
                    }),
                    app_data.config.render.context_menu_transparent,
                    app_data.config.render.braille_style,
                    app_data.config.window.gap as f32,
                    app_data.config.window.pane_outline_width,
                    {
                        let c = app_data
                            .config
                            .theme
                            .parsed_pane_outline_active
                            .to_srgb_linear();
                        [c.r, c.g, c.b, c.a]
                    },
                    {
                        let c = app_data
                            .config
                            .theme
                            .parsed_pane_outline_inactive
                            .to_srgb_linear();
                        [c.r, c.g, c.b, c.a]
                    },
                    effective_pane_padding,
                    None,
                    command_completion_indicator,
                    statusbar_hover,
                ) {
                    Ok(n) => (n, true),
                    Err(ForgeError::Vulkan(msg)) if msg == "Surface lost" => {
                        tracing::error!("Surface lost during rendering.");
                        app_data.wayland_state.running = false;
                        (false, false)
                    }
                    Err(e) => {
                        tracing::error!("Render error: {}", e);
                        (false, false)
                    }
                };
                if render_succeeded {
                    if let Some(generation) = opening_frame_generation {
                        if let Some(indicator) = app_data
                            .command_completion_indicators
                            .values_mut()
                            .find(|indicator| indicator.generation == generation)
                        {
                            indicator.opening_frame_presented = true;
                        }
                    }
                }
                if !app_data.first_vulkan_text_frame_logged {
                    app_data.first_vulkan_text_frame_logged = true;
                    tracing::debug!(
                        "[PROFILER] First Vulkan text frame took: {:?}",
                        app_data.startup_start.elapsed()
                    );
                }
                drop(grid_refs);
                drop(sb);

                if !frame_should_mark_clean(needs_recreate) {
                    if let Some(window) = app_data.wayland_state.window.as_ref() {
                        let _ = renderer.recreate_swapchain(window.size.width, window.size.height);
                    }
                    // Swapchain recreated: mark all dirty so next frame re-uploads everything.
                    app_data.pane_io.send_ui_command(
                        crate::mux::io::PtyWorkerCommand::MarkAllDirty(active_pane),
                    );
                }
                // On successful render: no need to MarkAllClean — generate_snapshot() already
                // cleared dirty flags in the PTY thread immediately upon snapshot capture.

                let scroll_animation_still_active = match &app_data.pane_runtime {
                    crate::mux::PaneRuntime::Scrolling(manager) => {
                        manager.active_scroll_animation_active(std::time::Instant::now())
                    }
                    crate::mux::PaneRuntime::Tiling => false,
                };
                let cursor_trail_still_active = app_data.renderer.as_ref().is_some_and(|renderer| {
                    renderer.cursor_trail_wants_redraw(std::time::Instant::now())
                });
                app_data.wayland_state.force_redraw = needs_scrollbar_redraw
                    || needs_statusbar_hover_redraw
                    || needs_recreate
                    || scroll_animation_still_active
                    || cursor_trail_still_active;
                if needs_scrollbar_redraw
                    || needs_statusbar_hover_redraw
                    || needs_recreate
                    || scroll_animation_still_active
                    || cursor_trail_still_active
                {
                    app_data.loop_signal.clone().wakeup();
                }
            }
        }

        if app_data.wayland_state.needs_flush {
            let _ = app_data.wayland_state.conn.flush();
            app_data.wayland_state.needs_flush = false;
        }
    }

    tracing::info!("Event loop exited cleanly.");
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
pub struct GridMetrics {
    pub cols: usize,
    pub rows: usize,
    pub pad_x: f64,
    pub pad_y: f64,
    pub effective_cell_w: f64,
    pub effective_cell_h: f64,
    pub scale_x: f64,
    pub scale_y: f64,

    pub sb_enabled: bool,
    pub sb_y: f64,
    pub sb_x: f64,
    pub sb_cols: usize,
    pub sidebar_cols: usize,
    pub sidebar_width: f64,
}

// Window, statusbar, sidebar, and native-cell inputs are independent geometry sources.
#[allow(clippy::too_many_arguments)]
pub fn compute_grid_metrics(
    win_w: f64,
    win_h: f64,
    pad_cfg: &forge_core::config_registry::PaddingConfig,
    center_grid: bool,
    statusbar_cfg: &forge_core::config_registry::StatusbarConfig,
    sidebar_cols: usize,
    native_cell_w: f64,
    native_cell_h: f64,
) -> GridMetrics {
    let sidebar_width = (sidebar_cols as f64 * native_cell_w).min((win_w - native_cell_w).max(0.0));
    let content_win_w = (win_w - sidebar_width).max(native_cell_w);
    let avail_w =
        (content_win_w - pad_cfg.left as f64 - pad_cfg.right as f64).max(native_cell_w);
    let mut avail_h = (win_h - pad_cfg.top as f64 - pad_cfg.bottom as f64).max(native_cell_h);

    let mut sb_y = 0.0;

    if statusbar_cfg.enabled {
        if statusbar_cfg.placement == forge_core::config_registry::StatusbarPlacement::Absolute {
            avail_h = (win_h - pad_cfg.top as f64 - pad_cfg.bottom as f64 - native_cell_h)
                .max(native_cell_h);
            if statusbar_cfg.position == forge_core::config_registry::StatusbarPosition::Top {
                sb_y = 0.0;
            } else {
                sb_y = win_h - native_cell_h;
            }
        } else {
            avail_h = (avail_h - native_cell_h).max(native_cell_h);
        }
    }

    let cols = (avail_w / native_cell_w).max(1.0) as usize;
    let rows = (avail_h / native_cell_h).max(1.0) as usize;

    let mut effective_cell_w = native_cell_w;
    let mut effective_cell_h = native_cell_h;
    let mut scale_x = 1.0;
    let mut scale_y = 1.0;
    let mut pad_x = sidebar_width + pad_cfg.left as f64;
    let mut pad_y = pad_cfg.top as f64;
    let mut sb_x = pad_x;
    let mut sb_cols = cols;

    if statusbar_cfg.enabled
        && statusbar_cfg.placement == forge_core::config_registry::StatusbarPlacement::Absolute
    {
        sb_x = sidebar_width;
        sb_cols = (content_win_w / native_cell_w).ceil().max(1.0) as usize;
        if statusbar_cfg.position == forge_core::config_registry::StatusbarPosition::Top {
            pad_y += native_cell_h;
        }
    }

    if !center_grid {
        effective_cell_w = avail_w / cols as f64;
        effective_cell_h = avail_h / rows as f64;
        scale_x = effective_cell_w / native_cell_w;
        scale_y = effective_cell_h / native_cell_h;
    } else {
        // Center mode: center the grid in the remaining space
        let remaining_w = avail_w - (cols as f64 * native_cell_w);
        let remaining_h = avail_h - (rows as f64 * native_cell_h);
        pad_x += (remaining_w / 2.0).floor();
        pad_y += (remaining_h / 2.0).floor();
    }

    if statusbar_cfg.enabled
        && statusbar_cfg.placement == forge_core::config_registry::StatusbarPlacement::Inside
    {
        if statusbar_cfg.position == forge_core::config_registry::StatusbarPosition::Top {
            sb_y = pad_y;
            pad_y += native_cell_h;
        } else {
            sb_y = pad_y + (rows as f64 * effective_cell_h);
        }
        sb_x = pad_x;
        sb_cols = cols;
    }

    GridMetrics {
        cols,
        rows,
        pad_x,
        pad_y,
        effective_cell_w,
        effective_cell_h,
        scale_x,
        scale_y,
        sb_enabled: statusbar_cfg.enabled,
        sb_y,
        sb_x,
        sb_cols,
        sidebar_cols,
        sidebar_width,
    }
}

#[cfg(test)]
mod metric_tests {
    use super::*;
    use forge_core::config_registry::PaddingConfig;
    use std::time::{Duration, Instant};

    fn padding() -> PaddingConfig {
        PaddingConfig {
            top: 4,
            bottom: 4,
            left: 4,
            right: 4,
        }
    }

    fn command_indicator(
        success: bool,
        program_name: &str,
        exit_code: i32,
        shown_at: Instant,
    ) -> CommandCompletionIndicator {
        CommandCompletionIndicator {
            success,
            program_name: std::sync::Arc::from(program_name),
            exit_text: (exit_code != 0).then(|| std::sync::Arc::from(format!("[{exit_code}]"))),
            generation: 1,
            tab_id: crate::mux::TabId::new(1),
            shown_at,
            opening_frame_presented: false,
            dismissed_at: None,
            expires_at: None,
        }
    }

    #[test]
    fn runtime_reload_keeps_restart_required_options() {
        let current = forge_core::config_registry::ForgeConfig::default();
        let mut requested = current.clone();
        requested.font.family = "/tmp/other.ttf".to_string();
        requested.font.size = current.font.size + 2.0;
        requested.shell.program = "/bin/other-shell".to_string();
        requested.window.width = current.window.width + 100;
        requested.window.center_on_launch = true;
        requested.window.opacity = 0.42;
        requested.theme.parsed_background.r = requested.theme.parsed_background.r.wrapping_add(1);

        let applied = runtime_reload_config(&current, requested);

        assert_eq!(applied.font.family, current.font.family);
        assert_eq!(applied.font.size, current.font.size);
        assert_eq!(applied.shell, current.shell);
        assert_eq!(applied.window.width, current.window.width);
        assert_eq!(
            applied.window.center_on_launch,
            current.window.center_on_launch
        );
        assert_eq!(applied.window.opacity, 0.42);
        assert_ne!(
            applied.theme.parsed_background,
            current.theme.parsed_background
        );
    }

    #[test]
    fn center_keeps_native_cell_size_and_centers_leftover_pixels() {
        let metrics = compute_grid_metrics(
            101.0,
            50.0,
            &padding(),
            true,
            &forge_core::config_registry::StatusbarConfig::default(),
            0,
            10.0,
            20.0,
        );

        assert_eq!(metrics.cols, 9);
        assert_eq!(metrics.rows, 1);
        assert_eq!(metrics.effective_cell_w, 10.0);
        assert_eq!(metrics.effective_cell_h, 20.0);
        assert_eq!(metrics.scale_x, 1.0);
        assert_eq!(metrics.scale_y, 1.0);
        assert_eq!(metrics.pad_x, 5.0);
        assert_eq!(metrics.pad_y, 5.0);
    }

    #[test]
    fn fill_expands_cell_geometry_and_anchors_to_configured_padding() {
        let metrics = compute_grid_metrics(
            101.0,
            50.0,
            &padding(),
            false,
            &forge_core::config_registry::StatusbarConfig::default(),
            0,
            10.0,
            20.0,
        );

        assert_eq!(metrics.cols, 9);
        assert_eq!(metrics.rows, 1);
        assert!((metrics.effective_cell_w - (93.0 / 9.0)).abs() < 0.000000000001);
        assert_eq!(metrics.effective_cell_h, 22.0);
        assert!((metrics.scale_x - (93.0 / 90.0)).abs() < 0.000000000001);
        assert!((metrics.scale_y - 1.1).abs() < 0.000000000001);
        assert_eq!(metrics.pad_x, 4.0);
        assert_eq!(metrics.pad_y, 4.0);
    }

    #[test]
    fn sidebar_reserves_left_columns_and_shifts_statusbar() {
        let metrics = compute_grid_metrics(
            300.0,
            80.0,
            &padding(),
            true,
            &forge_core::config_registry::StatusbarConfig::default(),
            4,
            10.0,
            20.0,
        );

        assert_eq!(metrics.sidebar_cols, 4);
        assert_eq!(metrics.sidebar_width, 40.0);
        assert!(metrics.pad_x >= 40.0);
        assert!(metrics.sb_x >= 40.0);
    }

    #[test]
    fn command_indicator_rect_is_bottom_centered_in_content_viewport() {
        let statusbar = forge_core::config_registry::StatusbarConfig {
            enabled: true,
            position: forge_core::config_registry::StatusbarPosition::Top,
            placement: forge_core::config_registry::StatusbarPlacement::Inside,
            ..forge_core::config_registry::StatusbarConfig::default()
        };
        let metrics =
            compute_grid_metrics(300.0, 120.0, &padding(), true, &statusbar, 4, 10.0, 20.0);

        let rect = command_completion_indicator_rect(metrics, 2, 7.5, 5.0).unwrap();
        let viewport_width = metrics.cols as f32 * metrics.effective_cell_w as f32;
        let viewport_height = metrics.rows as f32 * metrics.effective_cell_h as f32;

        assert_eq!(
            rect.x + rect.width * 0.5,
            metrics.pad_x as f32 + viewport_width * 0.5
        );
        assert_eq!(rect.width, 35.0);
        assert_eq!(rect.height, 30.0);
        assert!(rect.x >= metrics.sidebar_width as f32);
        assert!(rect.y >= metrics.pad_y as f32);
        assert!(rect.y + rect.height <= metrics.pad_y as f32 + viewport_height);
    }

    #[test]
    fn command_indicator_stays_above_bottom_statusbar_after_resize() {
        let statusbar = forge_core::config_registry::StatusbarConfig {
            enabled: true,
            position: forge_core::config_registry::StatusbarPosition::Bottom,
            placement: forge_core::config_registry::StatusbarPlacement::Inside,
            ..forge_core::config_registry::StatusbarConfig::default()
        };

        for width in [240.0, 480.0] {
            let metrics =
                compute_grid_metrics(width, 180.0, &padding(), true, &statusbar, 4, 10.0, 20.0);
            let rect = command_completion_indicator_rect(metrics, 8, 7.5, 5.0).unwrap();
            let content_center =
                metrics.pad_x as f32 + metrics.cols as f32 * metrics.effective_cell_w as f32 * 0.5;

            assert_eq!(rect.x + rect.width * 0.5, content_center);
            assert!(rect.y + rect.height <= metrics.sb_y as f32);
            assert!(rect.x >= metrics.sidebar_width as f32);
        }
    }

    #[test]
    fn command_indicator_layout_adds_program_and_failure_suffix() {
        let metrics = compute_grid_metrics(
            300.0,
            120.0,
            &padding(),
            true,
            &forge_core::config_registry::StatusbarConfig::default(),
            0,
            10.0,
            20.0,
        );
        let mut parsed_ansi_colors =
            forge_core::config_registry::ThemeConfig::default().parsed_ansi_colors;
        parsed_ansi_colors[1] = forge_core::color::Color {
            r: 4,
            g: 5,
            b: 6,
            a: 255,
        };
        let theme = forge_core::config_registry::ThemeConfig {
            parsed_foreground: forge_core::color::Color {
                r: 9,
                g: 8,
                b: 7,
                a: 255,
            },
            parsed_ansi_colors,
            ..forge_core::config_registry::ThemeConfig::default()
        };
        let shown_at = Instant::now();
        let indicator = command_indicator(false, "cargo", 101, shown_at);

        let config = forge_core::config_registry::CommandCompletionIndicatorConfig::default();
        let render = command_completion_indicator_layout(
            metrics,
            &indicator,
            &theme,
            &config,
            shown_at + COMMAND_INDICATOR_CIRCLE_HOLD + COMMAND_INDICATOR_EXPAND,
        )
        .unwrap();

        assert_eq!(render.command.as_ref(), "cargo");
        assert_eq!(render.exit_text.as_deref(), Some("[101]"));
        assert_eq!(render.rect.width, 135.0);
        assert_eq!(render.corner_radius, render.rect.height * 0.5);
        assert_eq!(
            render.background_color,
            Some(color_to_render_array(theme.parsed_popup_background))
        );
        assert_eq!(
            render.text_color,
            color_to_render_array(theme.parsed_foreground)
        );
        assert_eq!(
            render.failure_color,
            color_to_render_array(theme.parsed_ansi_colors[1])
        );
    }

    #[test]
    fn command_indicator_layout_truncates_command_to_viewport() {
        let metrics = compute_grid_metrics(
            90.0,
            60.0,
            &PaddingConfig {
                top: 0,
                bottom: 0,
                left: 0,
                right: 0,
            },
            true,
            &forge_core::config_registry::StatusbarConfig::default(),
            0,
            10.0,
            20.0,
        );
        let shown_at = Instant::now();
        let indicator = command_indicator(false, "very-long-command", 2, shown_at);

        let config = forge_core::config_registry::CommandCompletionIndicatorConfig::default();
        let render = command_completion_indicator_layout(
            metrics,
            &indicator,
            &forge_core::config_registry::ThemeConfig::default(),
            &config,
            shown_at + COMMAND_INDICATOR_CIRCLE_HOLD + COMMAND_INDICATOR_EXPAND,
        )
        .unwrap();

        assert_eq!(render.exit_text.as_deref(), Some("[2]"));
        assert_eq!(render.command.chars().count(), 2);
        assert!(render.rect.width <= 90.0);
        assert!(render.rect.x >= 0.0);
        assert!(render.rect.x + render.rect.width <= 90.0);
    }

    #[test]
    fn command_indicator_uses_shared_popup_color_with_independent_transparency() {
        let metrics = compute_grid_metrics(
            300.0,
            120.0,
            &padding(),
            true,
            &forge_core::config_registry::StatusbarConfig::default(),
            0,
            10.0,
            20.0,
        );
        let shown_at = Instant::now();
        let indicator = command_indicator(true, "cargo", 0, shown_at);
        let mut theme = forge_core::config_registry::ThemeConfig::default();
        let popup = forge_core::color::Color {
            r: 1,
            g: 2,
            b: 3,
            a: 128,
        };
        theme.parsed_popup_background = popup;
        let mut config = forge_core::config_registry::CommandCompletionIndicatorConfig::default();
        assert_eq!(
            command_completion_indicator_layout(metrics, &indicator, &theme, &config, shown_at)
                .unwrap()
                .background_color,
            Some(color_to_render_array(popup))
        );

        config.transparent = true;
        assert_eq!(
            command_completion_indicator_layout(metrics, &indicator, &theme, &config, shown_at)
                .unwrap()
                .background_color,
            None
        );
    }

    #[test]
    fn command_indicator_uses_theme_ansi_green_and_red() {
        let mut theme = forge_core::config_registry::ThemeConfig::default();
        theme.parsed_ansi_colors[2] = forge_core::color::Color {
            r: 1,
            g: 2,
            b: 3,
            a: 255,
        };
        theme.parsed_ansi_colors[1] = forge_core::color::Color {
            r: 4,
            g: 5,
            b: 6,
            a: 255,
        };

        let (success, failure) = command_completion_indicator_colors(&theme);

        assert_eq!(success, theme.parsed_ansi_colors[2]);
        assert_eq!(failure, theme.parsed_ansi_colors[1]);
    }

    #[test]
    fn shell_integration_controls_command_completion_tracking() {
        let mut config = forge_core::config_registry::ForgeConfig::default();
        assert!(command_completion_tracking_enabled_for_config(&config));

        config.shell.integration_enabled = false;
        assert!(!command_completion_tracking_enabled_for_config(&config));
    }

    #[test]
    fn command_indicator_shows_for_unfocused_pane_tab_or_window() {
        let config = forge_core::config_registry::CommandCompletionIndicatorConfig::default();

        assert!(!command_completion_indicator_should_show(
            &config, 10_000, true, false, false, false, true,
        ));
        assert!(command_completion_indicator_should_show(
            &config, 10_000, true, false, false, true, true,
        ));
        assert!(command_completion_indicator_should_show(
            &config, 10_000, true, false, true, true, true,
        ));
        assert!(command_completion_indicator_should_show(
            &config, 10_000, true, false, false, false, false,
        ));
    }

    #[test]
    fn command_indicator_respects_threshold_existence_and_zoom_mode() {
        let mut config = forge_core::config_registry::CommandCompletionIndicatorConfig::default();

        assert!(!command_completion_indicator_should_show(
            &config, 9_999, true, false, false, true, true,
        ));
        assert!(!command_completion_indicator_should_show(
            &config, 10_000, false, false, false, true, true,
        ));

        config.mode = forge_core::config_registry::CommandCompletionIndicatorMode::DisabledOnZoom;
        assert!(!command_completion_indicator_should_show(
            &config, 10_000, true, true, false, true, true,
        ));
        assert!(command_completion_indicator_should_show(
            &config, 10_000, true, false, false, true, true,
        ));
    }

    #[test]
    fn command_indicator_timeout_expiry_uses_configured_duration() {
        let config = forge_core::config_registry::CommandCompletionIndicatorConfig {
            display_duration_ms: 1_750,
            ..forge_core::config_registry::CommandCompletionIndicatorConfig::default()
        };
        let now = Instant::now();

        let expires_at = command_completion_indicator_expiry(&config, now).unwrap();

        assert_eq!(
            expires_at.saturating_duration_since(now),
            COMMAND_INDICATOR_CIRCLE_HOLD
                + COMMAND_INDICATOR_EXPAND
                + std::time::Duration::from_millis(1_750)
                + COMMAND_INDICATOR_CONTRACT
        );
    }

    #[test]
    fn command_indicator_animation_runs_expand_hold_contract() {
        let shown_at = Instant::now();
        let indicator = command_indicator(true, "cargo", 0, shown_at);
        let config = forge_core::config_registry::CommandCompletionIndicatorConfig {
            display_duration_ms: 1_000,
            ..forge_core::config_registry::CommandCompletionIndicatorConfig::default()
        };

        let initial = command_indicator_visual(&indicator, &config, shown_at);
        assert_eq!(initial.expansion, 0.0);
        assert!(!initial.animating);

        let paused = command_indicator_visual(
            &indicator,
            &config,
            shown_at + COMMAND_INDICATOR_CIRCLE_HOLD / 2,
        );
        assert_eq!(paused.expansion, 0.0);
        assert!(!paused.animating);

        let expanded_at = shown_at + COMMAND_INDICATOR_CIRCLE_HOLD + COMMAND_INDICATOR_EXPAND;
        let held = command_indicator_visual(&indicator, &config, expanded_at);
        assert_eq!(held.expansion, 1.0);
        assert!(!held.animating);

        let dismiss_at = expanded_at + Duration::from_millis(1_000);
        let contracting = command_indicator_visual(
            &indicator,
            &config,
            dismiss_at + COMMAND_INDICATOR_CONTRACT / 2,
        );
        assert!(contracting.expansion > 0.0 && contracting.expansion < 1.0);
        assert!(contracting.animating);

        let finished =
            command_indicator_visual(&indicator, &config, dismiss_at + COMMAND_INDICATOR_CONTRACT);
        assert_eq!(finished.expansion, 0.0);
        assert!(!finished.animating);
    }

    #[test]
    fn command_indicator_requests_one_exact_final_expansion_frame() {
        let shown_at = Instant::now();
        let mut indicator = command_indicator(true, "cargo", 0, shown_at);
        let config = forge_core::config_registry::CommandCompletionIndicatorConfig::default();
        let expanded_at = shown_at + COMMAND_INDICATOR_CIRCLE_HOLD + COMMAND_INDICATOR_EXPAND;

        assert!(command_indicator_needs_frame(
            &indicator,
            &config,
            expanded_at
        ));
        indicator.opening_frame_presented = true;
        assert!(!command_indicator_needs_frame(
            &indicator,
            &config,
            expanded_at
        ));
    }

    #[test]
    fn command_indicator_layout_expands_from_centered_circle_without_jump() {
        let metrics = compute_grid_metrics(
            300.0,
            120.0,
            &padding(),
            true,
            &forge_core::config_registry::StatusbarConfig::default(),
            0,
            10.0,
            20.0,
        );
        let shown_at = Instant::now();
        let indicator = command_indicator(true, "cargo", 0, shown_at);
        let theme = forge_core::config_registry::ThemeConfig::default();
        let config = forge_core::config_registry::CommandCompletionIndicatorConfig::default();
        let initial =
            command_completion_indicator_layout(metrics, &indicator, &theme, &config, shown_at)
                .unwrap();
        let expanded = command_completion_indicator_layout(
            metrics,
            &indicator,
            &theme,
            &config,
            shown_at + COMMAND_INDICATOR_CIRCLE_HOLD + COMMAND_INDICATOR_EXPAND,
        )
        .unwrap();

        assert_eq!(initial.rect.width, initial.rect.height);
        assert_eq!(initial.corner_radius, initial.rect.height * 0.5);
        assert_eq!(
            initial.rect.x + initial.rect.width * 0.5,
            expanded.rect.x + expanded.rect.width * 0.5
        );
        assert!(expanded.rect.width > initial.rect.width);
    }

    #[test]
    fn command_indicator_on_interaction_has_no_expiry() {
        let config = forge_core::config_registry::CommandCompletionIndicatorConfig {
            dismissal:
                forge_core::config_registry::CommandCompletionIndicatorDismissal::OnInteraction,
            ..forge_core::config_registry::CommandCompletionIndicatorConfig::default()
        };

        assert_eq!(
            command_completion_indicator_expiry(&config, Instant::now()),
            None
        );
    }

    #[test]
    fn scrollbar_overlay_does_not_redraw_without_scrollback() {
        let now = Instant::now();

        assert!(!scrollbar_overlay_wants_redraw(
            false, 0, 1.0, 1.0, true, false, now, now, now,
        ));
    }

    #[test]
    fn scrollbar_overlay_does_not_redraw_in_alt_buffer() {
        let now = Instant::now();

        assert!(!scrollbar_overlay_wants_redraw(
            true, 100, 1.0, 1.0, true, false, now, now, now,
        ));
    }

    #[test]
    fn scrollbar_overlay_redraws_during_reveal_delay() {
        let now = Instant::now();
        let recent = now - Duration::from_millis(100);

        assert!(scrollbar_overlay_wants_redraw(
            false, 100, 0.0, 0.0, false, false, recent, recent, now,
        ));
    }

    #[test]
    fn scrollbar_overlay_redraws_while_visible_or_interactive() {
        let now = Instant::now();
        let old = now - Duration::from_secs(2);

        assert!(scrollbar_overlay_wants_redraw(
            false, 100, 0.25, 0.0, false, false, old, old, now,
        ));
        assert!(scrollbar_overlay_wants_redraw(
            false, 100, 0.0, 0.0, true, false, old, old, now,
        ));
        assert!(scrollbar_overlay_wants_redraw(
            false, 100, 0.0, 0.0, false, true, old, old, now,
        ));
    }

    #[test]
    fn scrollbar_overlay_stops_after_idle_fade_settles() {
        let now = Instant::now();
        let old = now - Duration::from_secs(2);

        assert!(!scrollbar_overlay_wants_redraw(
            false, 100, 0.0, 0.0, false, false, old, old, now,
        ));
    }

    #[test]
    fn passive_pointer_motion_has_no_effect_in_alt_buffer() {
        assert!(!pointer_motion_has_effect(
            true, 100, true, None, None, false,
        ));
    }

    #[test]
    fn passive_pointer_motion_has_no_effect_without_scrollback() {
        assert!(!pointer_motion_has_effect(
            false, 0, false, None, None, false,
        ));
    }

    #[test]
    fn pointer_motion_has_effect_for_scrollbar_selection_and_drag_reporting() {
        assert!(pointer_motion_has_effect(
            false, 100, false, None, None, false,
        ));
        assert!(pointer_motion_has_effect(
            true,
            0,
            false,
            None,
            Some((2, 3)),
            false,
        ));
        assert!(pointer_motion_has_effect(
            true,
            0,
            true,
            Some(272),
            None,
            false,
        ));
        assert!(pointer_motion_has_effect(
            false, 100, false, None, None, true,
        ));
    }

    #[test]
    fn frame_redraw_predicate_skips_idle_frames() {
        assert!(!frame_wants_redraw(
            false, false, false, false, false, false, false
        ));
    }

    #[test]
    fn queued_redraw_does_not_poll_while_frame_callback_is_pending() {
        assert!(!redraw_can_run_immediately(false, true, false));
    }

    #[test]
    fn queued_redraw_polls_once_frame_gate_opens() {
        assert!(redraw_can_run_immediately(false, true, true));
    }

    #[test]
    fn structural_redraw_can_bypass_closed_frame_gate() {
        assert!(redraw_can_run_immediately(true, true, false));
    }

    #[test]
    fn pending_wayland_callback_is_not_duplicated() {
        assert!(!frame_callback_request_needed(true));
        assert!(frame_callback_request_needed(false));
    }

    #[test]
    fn frame_redraw_predicate_renders_for_each_dirty_source() {
        assert!(frame_wants_redraw(true, false, false, false, false, false, false));
        assert!(frame_wants_redraw(false, true, false, false, false, false, false));
        assert!(frame_wants_redraw(false, false, true, false, false, false, false));
        assert!(frame_wants_redraw(false, false, false, true, false, false, false));
        assert!(frame_wants_redraw(false, false, false, false, true, false, false));
        assert!(frame_wants_redraw(false, false, false, false, false, true, false));
        assert!(frame_wants_redraw(false, false, false, false, false, false, true));
    }

    #[test]
    fn recreated_swapchain_frame_keeps_rows_dirty_for_new_geometry() {
        assert!(!frame_should_mark_clean(true));
        assert!(frame_should_mark_clean(false));
    }
}
