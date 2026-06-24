use calloop::{EventLoop, Interest, Mode, PostAction};
use calloop_wayland_source::WaylandSource;
use wayland_client::EventQueue;
use forge_core::{ForgeError, Result};
use crate::wayland::connection::WaylandState;
use calloop::generic::Generic;

pub struct AppData {
    pub wayland_state: WaylandState,
    pub loop_signal: calloop::LoopSignal,
    pub pty: Option<forge_pty::Pty>,
    pub screen_buffer: std::sync::Arc<std::sync::RwLock<forge_pty::ScreenBuffer>>,
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
    pub font_atlas_receiver: Option<std::sync::mpsc::Receiver<(forge_renderer::font::rasterizer::FontRasterizer, forge_renderer::font::atlas::GlyphAtlas)>>,
    pub cursor_visible_phase: bool,
    pub last_cursor_blink: std::time::Instant,
    pub config_rx: Option<crossbeam_channel::Receiver<forge_config::ConfigUpdate>>,
    pub watcher: Option<notify::RecommendedWatcher>,
    pub last_scrollbar_state: Option<(f64, f64)>,
    pub last_mouse_activity: std::time::Instant,
    pub mouse_started_moving: std::time::Instant,
    pub is_hovering_edge: bool,
    pub current_thumb_width: f32,
    pub current_track_opacity: f32,
    pub current_thumb_opacity: f32,
    pub is_dragging_scrollbar: bool,
    pub scrollbar_drag_offset_y: f64,
}

impl AsMut<WaylandState> for AppData {
    fn as_mut(&mut self) -> &mut WaylandState {
        &mut self.wayland_state
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_event_loop(
    mut event_loop: EventLoop<AppData>,
    wayland_state: WaylandState,
    event_queue: EventQueue<WaylandState>,
    pty: forge_pty::Pty,
    screen_buffer: std::sync::Arc<std::sync::RwLock<forge_pty::ScreenBuffer>>,
    vte_processor: forge_pty::VteProcessor,
    key_receiver: std::sync::mpsc::Receiver<Vec<u8>>,
    pointer_receiver: std::sync::mpsc::Receiver<crate::wayland::connection::PointerEvent>,
    paste_receiver: std::sync::mpsc::Receiver<Vec<u8>>,
    config: forge_core::config_registry::ForgeConfig,
    renderer: Option<forge_renderer::Renderer>,
    font_atlas_receiver: Option<std::sync::mpsc::Receiver<(forge_renderer::font::rasterizer::FontRasterizer, forge_renderer::font::atlas::GlyphAtlas)>>,
    config_rx: Option<crossbeam_channel::Receiver<forge_config::ConfigUpdate>>,
    watcher: Option<notify::RecommendedWatcher>,
) -> Result<()> {

    let loop_handle = event_loop.handle();
    let loop_signal = event_loop.get_signal();

    let queue_handle = event_queue.handle();

    // We can't flush yet, but `wayland_state` is updated. We'll set needs_flush below.
    let initial_window_size = wayland_state.window.as_ref().map(|w| w.size).unwrap_or(forge_core::geometry::Size { width: 0, height: 0 });
    let mut wayland_state = wayland_state;
    wayland_state.keybindings = config.keybindings.clone();
    wayland_state.hide_mouse_when_typing = config.behavior.hide_mouse_when_typing;
    wayland_state.frame_callback_pending = true;
    wayland_state.needs_flush = false;
    let _ = wayland_state.conn.flush();

    // Give the clipboard manager a clone of the loop_signal
    if let Some(clip) = wayland_state.clipboard.as_mut() {
        clip.loop_signal = Some(loop_signal.clone());
    }

    let source = WaylandSource::new(wayland_state.conn.clone(), event_queue);
    loop_handle.insert_source(source, |(), queue, app_data| {
        queue.dispatch_pending(&mut app_data.wayland_state)
    }).map_err(|e| ForgeError::Wayland(e.to_string()))?;

    let loop_signal_clone = loop_signal.clone();
    let mut app_data = AppData {
        wayland_state,
        loop_signal,
        pty: Some(pty),
        screen_buffer,
        key_receiver,
        pointer_receiver,
        paste_receiver,
        config,
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
        font_atlas_receiver,
        cursor_visible_phase: true,
        last_cursor_blink: std::time::Instant::now(),
        config_rx,
        watcher,
        last_scrollbar_state: None,
        last_mouse_activity: std::time::Instant::now(),
        mouse_started_moving: std::time::Instant::now(),
        is_hovering_edge: false,
        current_thumb_width: 5.0,
        current_track_opacity: 0.0,
        current_thumb_opacity: 0.0,
        is_dragging_scrollbar: false,
        scrollbar_drag_offset_y: 0.0,
    };

    let pty_fd = app_data.pty.as_ref().unwrap().master_fd.try_clone()
        .map_err(|e| ForgeError::Other(format!("Failed to clone PTY fd: {}", e)))?;
    let sb_clone = app_data.screen_buffer.clone();
    let shell_exited = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shell_exited_clone = shell_exited.clone();
    
    // Spawn the high-priority background thread for parsing PTY output
    std::thread::spawn(move || {
        use std::os::unix::io::AsRawFd;
        let mut read_buf = [0u8; 65536]; // Large buffer for infinite throughput
        let mut vte_processor = vte_processor;
        let raw_fd = pty_fd.as_raw_fd();
        // Remove O_NONBLOCK since we are in a dedicated thread now.
        let mut flags = unsafe { libc::fcntl(raw_fd, libc::F_GETFL) };
        flags &= !libc::O_NONBLOCK;
        unsafe { libc::fcntl(raw_fd, libc::F_SETFL, flags) };
        
        loop {
            let n = unsafe { libc::read(raw_fd, read_buf.as_mut_ptr() as *mut libc::c_void, read_buf.len()) };
            if n <= 0 {
                // If it's EAGAIN/EWOULDBLOCK, we should poll or sleep, but since we remove O_NONBLOCK, it should just block.
                // If it really returns 0, the shell exited.
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::WouldBlock {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                }
                break;
            }
            let data = &read_buf[..n as usize];
            let mut sb = sb_clone.write().unwrap();
            let responses = vte_processor.process(data, &mut *sb);
            sb.view_scroll_to_bottom();
            drop(sb); // Release the lock before writing to PTY (avoid deadlocks)
            
            if !responses.is_empty() {
                unsafe { libc::write(raw_fd, responses.as_ptr() as *const libc::c_void, responses.len()) };
            }
            // Signal the Wayland loop that new frames are ready
            loop_signal_clone.wakeup();
        }
        tracing::info!("PTY background thread exited.");
        shell_exited_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        loop_signal_clone.wakeup();
    });

    while app_data.wayland_state.running {
        if shell_exited.load(std::sync::atomic::Ordering::SeqCst) {
            app_data.wayland_state.running = false;
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

        let cursor_blink = app_data.screen_buffer.write().unwrap().cursor_blink_override.unwrap_or(app_data.config.cursor.blink);
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


        event_loop.dispatch(timeout, &mut app_data).map_err(|e| ForgeError::Other(e.to_string()))?;
        if app_data.wayland_state.is_alt_buffer != app_data.screen_buffer.write().unwrap().use_alt_buffer {
            app_data.wayland_state.is_alt_buffer = app_data.screen_buffer.write().unwrap().use_alt_buffer;
            // The cursor shape will naturally update on the next pointer motion or enter event.
        }
        if cursor_blink {
            let blink_rate = app_data.config.cursor.blink_rate_ms as u128;
            if app_data.last_cursor_blink.elapsed().as_millis() >= blink_rate {
                app_data.cursor_visible_phase = !app_data.cursor_visible_phase;
                app_data.last_cursor_blink = std::time::Instant::now();
                let r = app_data.screen_buffer.write().unwrap().cursor.row;
                if r < app_data.screen_buffer.read().unwrap().dirty_rows.len() {
                    app_data.screen_buffer.write().unwrap().dirty_rows[r] = true;
                }
            }
        } else {
            if !app_data.cursor_visible_phase {
                app_data.cursor_visible_phase = true;
                let r = app_data.screen_buffer.write().unwrap().cursor.row;
                if r < app_data.screen_buffer.read().unwrap().dirty_rows.len() {
                    app_data.screen_buffer.write().unwrap().dirty_rows[r] = true;
                }
            }
        }

        if !app_data.wayland_state.running {
            app_data.loop_signal.stop();
            break;
        }

        // Process repeating key
        if let Some(repeating) = &mut app_data.wayland_state.repeating_key {
            let now = std::time::Instant::now();
            if now >= repeating.next_repeat_time {
                if let Some(pty) = app_data.pty.as_mut() {
                    let _ = pty.write_all(&repeating.bytes);
                }
                if let Some((rate, _)) = app_data.wayland_state.repeat_info {
                    if rate > 0 {
                        repeating.next_repeat_time = now + std::time::Duration::from_millis(1000 / rate as u64);
                    }
                }
                
                // Typing trap
                app_data.cursor_visible_phase = true;
                app_data.last_cursor_blink = std::time::Instant::now();
                let r = app_data.screen_buffer.write().unwrap().cursor.row;
                if r < app_data.screen_buffer.read().unwrap().dirty_rows.len() {
                    app_data.screen_buffer.write().unwrap().dirty_rows[r] = true;
                }
            }
        }

        while let Ok(input) = app_data.key_receiver.try_recv() {
            if app_data.screen_buffer.write().unwrap().selection.take().is_some() {
                app_data.screen_buffer.write().unwrap().mark_all_dirty();
            }
            if let Some(pty) = app_data.pty.as_mut() {
                let _ = pty.write_all(&input);
            }
            
            // Typing trap
            app_data.cursor_visible_phase = true;
            app_data.last_cursor_blink = std::time::Instant::now();
            let r = app_data.screen_buffer.write().unwrap().cursor.row;
            if r < app_data.screen_buffer.read().unwrap().dirty_rows.len() {
                app_data.screen_buffer.write().unwrap().dirty_rows[r] = true;
            }
        }

        while let Ok(bytes) = app_data.paste_receiver.try_recv() {
            if app_data.screen_buffer.write().unwrap().selection.take().is_some() {
                app_data.screen_buffer.write().unwrap().mark_all_dirty();
            }
            tracing::info!("[PASTE TIMING] Event loop received from paste_receiver at {:?}", std::time::Instant::now());
            if let Some(pty) = app_data.pty.as_mut() {
                if app_data.screen_buffer.read().unwrap().bracketed_paste {
                    let mut wrapped = Vec::with_capacity(bytes.len() + 12);
                    wrapped.extend_from_slice(b"\x1b[200~");
                    wrapped.extend_from_slice(&bytes);
                    wrapped.extend_from_slice(b"\x1b[201~");
                    let _ = pty.write_all(&wrapped);
                } else {
                    let _ = pty.write_all(&bytes);
                }
                tracing::info!("[PASTE TIMING] Event loop wrote to PTY at {:?}", std::time::Instant::now());
            }

            // Typing trap
            app_data.cursor_visible_phase = true;
            app_data.last_cursor_blink = std::time::Instant::now();
            let r = app_data.screen_buffer.write().unwrap().cursor.row;
            if r < app_data.screen_buffer.read().unwrap().dirty_rows.len() {
                app_data.screen_buffer.write().unwrap().dirty_rows[r] = true;
            }
        }

        if let Some(rx) = app_data.font_atlas_receiver.as_ref() {
            if let Ok((rasterizer, atlas)) = rx.try_recv() {
                tracing::info!("Received full FontData from background thread!");
                if let Some(renderer) = app_data.renderer.as_mut() {
                    let old_cell_w = renderer.cell_width;
                    let old_cell_h = renderer.cell_height;
                    
                    if let Err(e) = renderer.update_font_data(rasterizer, atlas) {
                        tracing::error!("Failed to update font atlas: {}", e);
                    } else {
                        app_data.screen_buffer.write().unwrap().mark_all_dirty();
                        
                        let cache = forge_core::cache::StartupCache::new_cache(
                            &app_data.config,
                            renderer.cell_width,
                            renderer.cell_height,
                            renderer.baseline,
                        );
                        forge_core::cache::write_startup_cache(&cache);

                        // Reflow if font dimensions changed!
                        if old_cell_w != renderer.cell_width || old_cell_h != renderer.cell_height {
                            if let Some(window) = app_data.wayland_state.window.as_ref() {
                                let cell_w = renderer.cell_width as f64;
                                let cell_h = renderer.cell_height as f64;
                                if cell_w > 0.0 && cell_h > 0.0 {
                                    let metrics = compute_grid_metrics(
                                        window.size.width as f64,
                                        window.size.height as f64,
                                        &app_data.config.window.padding,
                                        app_data.config.window.padding_balance,
                                        cell_w,
                                        cell_h,
                                    );
                                    let new_cols = metrics.cols;
                                    let new_rows = metrics.rows;
                                    app_data.screen_buffer.write().unwrap().resize_reflow(new_cols, new_rows);
                                    if let Some(pty) = app_data.pty.as_mut() {
                                        let px_w = (new_cols as f64 * metrics.effective_cell_w) as u16;
                                        let px_h = (new_rows as f64 * metrics.effective_cell_h) as u16;
                                        let _ = pty.resize(new_cols as u16, new_rows as u16, px_w, px_h);
                                    }
                                }
                            }
                        }
                    }
                }
                app_data.font_atlas_receiver = None; // Drop receiver once loaded
            }
        }

        if let Some(rx) = app_data.config_rx.as_ref() {
            // Drain the channel and only keep the LAST update
            let mut latest_update = None;
            while let Ok(update) = rx.try_recv() {
                latest_update = Some(update);
            }

            if let Some(update) = latest_update {
                tracing::info!("Applying live config update.");

                // 1. Check what changed.
                let old_theme = app_data.config.theme.clone();
                let new_theme = update.config.theme.clone();

                // 2. Update the config in AppData.
                app_data.config = update.config;
                app_data.wayland_state.keybindings = app_data.config.keybindings.clone();
                app_data.wayland_state.hide_mouse_when_typing = app_data.config.behavior.hide_mouse_when_typing;

                // 3. Apply changes.
                // Trigger theme update if any colors changed
                if old_theme != new_theme {
                    app_data.screen_buffer.write().unwrap().update_theme(new_theme.foreground, new_theme.background, new_theme.ansi_colors);

                    if let Some(renderer) = app_data.renderer.as_ref() {
                        let cache = forge_core::cache::StartupCache::new_cache(
                            &app_data.config,
                            renderer.cell_width,
                            renderer.cell_height,
                            renderer.baseline,
                        );
                        forge_core::cache::write_startup_cache(&cache);
                    }
                }

                // Note: Live-reloading font size or window dimensions requires swapchain
                // recreation or Wayland resize requests. Add a TODO for that.
            }
        }

        if let Some(window) = app_data.wayland_state.window.as_ref() {
            let win_size = window.size;
            if win_size != app_data.last_window_size {
                if let Some(renderer) = app_data.renderer.as_mut() {
                    let _ = renderer.recreate_swapchain(win_size.width, win_size.height);
                    app_data.screen_buffer.write().unwrap().mark_all_dirty(); // Force re-render on new swapchain images
                    
                    let cell_w = renderer.cell_width as f64;
                    let cell_h = renderer.cell_height as f64;
                    if cell_w > 0.0 && cell_h > 0.0 {
                        let metrics = compute_grid_metrics(
                            win_size.width as f64,
                            win_size.height as f64,
                            &app_data.config.window.padding,
                            app_data.config.window.padding_balance,
                            cell_w,
                            cell_h,
                        );
                        let new_cols = metrics.cols;
                        let new_rows = metrics.rows;
                        let (cur_cols, cur_rows) = {
                            let sb = app_data.screen_buffer.read().unwrap();
                            (sb.cols(), sb.rows())
                        };
                        if new_cols != cur_cols || new_rows != cur_rows {
                            app_data.screen_buffer.write().unwrap().resize_reflow(new_cols, new_rows);
                            if let Some(pty) = app_data.pty.as_mut() {
                                let px_w = (new_cols as f64 * metrics.effective_cell_w) as u16;
                                let px_h = (new_rows as f64 * metrics.effective_cell_h) as u16;
                                let _ = pty.resize(new_cols as u16, new_rows as u16, px_w, px_h);
                            }
                        }
                    }
                }
                app_data.last_window_size = win_size;
            }
        }

        let (cell_w, cell_h, pad_x, pad_y) = if let (Some(renderer), Some(window)) = (app_data.renderer.as_ref(), app_data.wayland_state.window.as_ref()) {
            let metrics = compute_grid_metrics(
                window.size.width as f64,
                window.size.height as f64,
                &app_data.config.window.padding,
                app_data.config.window.padding_balance,
                renderer.cell_width as f64,
                renderer.cell_height as f64,
            );
            (metrics.effective_cell_w, metrics.effective_cell_h, metrics.pad_x, metrics.pad_y)
        } else {
            (10.0, 20.0, 0.0, 0.0) // fallback
        };
        
        let (mouse_tracking_enabled, mouse_sgr_mode, use_alt) = {
            let sb = app_data.screen_buffer.read().unwrap();
            (sb.mouse_tracking_enabled, sb.mouse_sgr_mode, sb.use_alt_buffer)
        };

        while let Ok(evt) = app_data.pointer_receiver.try_recv() {

            use crate::wayland::connection::PointerEvent;
            match evt {
                PointerEvent::Enter { x, y } | PointerEvent::Motion { x, y } => {
                    let mut needs_redraw = false;
                    let now = std::time::Instant::now();
                    
                    if !use_alt {
                        if now.duration_since(app_data.last_mouse_activity).as_secs_f32() > 0.5 {
                            app_data.mouse_started_moving = now;
                            needs_redraw = true;
                        }
                        app_data.last_mouse_activity = now;
                    }
                    
                    let mut new_hovering = false;
                    if let Some(window) = app_data.wayland_state.window.as_ref() {
                        new_hovering = x > window.size.width as f64 - 24.0;
                    }
                    
                    if use_alt {
                        new_hovering = false;
                    }
                    
                    if new_hovering != app_data.is_hovering_edge {
                        app_data.is_hovering_edge = new_hovering;
                        needs_redraw = true;

                        if let Some(pointer) = &app_data.wayland_state.pointer {
                            if let Some(shape_manager) = &app_data.wayland_state.globals.cursor_shape_manager {
                                let device = shape_manager.get_pointer(pointer, &app_data.queue_handle, ());
                                let shape = if new_hovering {
                                    wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape::Default
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
                    
                    if !use_alt {
                        let active_secs = now.duration_since(app_data.mouse_started_moving).as_secs_f32();
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
                    
                    let col_1 = ((x - pad_x) / cell_w).max(0.0) as usize + 1;
                    let row_1 = ((y - pad_y) / cell_h).max(0.0) as usize + 1;

                    if mouse_tracking_enabled {
                        if let Some(btn) = app_data.active_mouse_button {
                            if col_1 != app_data.last_mouse_col || row_1 != app_data.last_mouse_row {
                                let btn_code = match btn {
                                    272 => 0,
                                    274 => 1,
                                    273 => 2,
                                    _ => 0,
                                } + 32; // Drag flag
                                if mouse_sgr_mode {
                                    let seq = format!("\x1b[<{};{};{}M", btn_code, col_1, row_1);
                                    if let Some(pty) = app_data.pty.as_mut() {
                                        let _ = pty.write_all(seq.as_bytes());
                                    }
                                }
                                app_data.last_mouse_col = col_1;
                                app_data.last_mouse_row = row_1;
                            }
                        }
                    } else {
                        if let Some((start_col, start_row)) = app_data.drag_start {
                            let col = ((x - pad_x) / cell_w).max(0.0) as usize;
                            let row = ((y - pad_y) / cell_h).max(0.0) as usize;
                            let mut sb = app_data.screen_buffer.write().unwrap();
                            if col != start_col || row != start_row || sb.selection.is_some() {
                                if sb.selection.is_none() {
                                    sb.selection = Some(forge_core::cell::SelectionRange {
                                        start_col,
                                        start_row,
                                        end_col: col,
                                        end_row: row,
                                    });
                                    sb.dirty_rows.fill(true);
                                } else if let Some(sel) = &mut sb.selection {
                                    if sel.end_row != row || sel.end_col != col {
                                        sel.end_row = row;
                                        sel.end_col = col;
                                        sb.dirty_rows.fill(true);
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
                                        let mut scroll_ratio = 1.0 - (new_thumb_y - track_top) / available_travel_space;
                                        scroll_ratio = scroll_ratio.clamp(0.0, 1.0);
                                        
                                        let history_lines = app_data.screen_buffer.write().unwrap().scrollback_len() as f64;
                                        let new_offset = (scroll_ratio * history_lines).round() as usize;
                                        
                                        app_data.screen_buffer.write().unwrap().scroll_offset = new_offset;
                                        app_data.screen_buffer.write().unwrap().dirty_rows.fill(true);
                                        app_data.loop_signal.wakeup();
                                    }
                                }
                            }
                        }
                    }
                }
                PointerEvent::Leave => {
                    if !use_alt {
                        app_data.is_hovering_edge = false;
                        app_data.is_dragging_scrollbar = false;
                        app_data.current_thumb_opacity = 0.0;
                        app_data.current_track_opacity = 0.0;
                        app_data.wayland_state.force_redraw = true;
                        app_data.loop_signal.wakeup();
                    }
                }
                PointerEvent::Press { button } => {
                    let col_1 = ((app_data.pointer_x - pad_x) / cell_w).max(0.0) as usize + 1;
                    let row_1 = ((app_data.pointer_y - pad_y) / cell_h).max(0.0) as usize + 1;
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
                            if let Some(pty) = app_data.pty.as_mut() {
                                let _ = pty.write_all(seq.as_bytes());
                            }
                        }
                    } else {
                        if button == 272 { // Left click
                            if app_data.is_hovering_edge {
                                if let Some((thumb_y, thumb_height)) = app_data.last_scrollbar_state {
                                    if app_data.pointer_y >= thumb_y && app_data.pointer_y <= thumb_y + thumb_height {
                                        app_data.is_dragging_scrollbar = true;
                                        app_data.scrollbar_drag_offset_y = app_data.pointer_y - thumb_y;
                                        continue;
                                    }
                                }
                            }

                            let col = ((app_data.pointer_x - pad_x) / cell_w).max(0.0) as usize;
                            let row = ((app_data.pointer_y - pad_y) / cell_h).max(0.0) as usize;
                            app_data.drag_start = Some((col, row));
                            app_data.screen_buffer.write().unwrap().selection = None; // clear previous selection on click
                            app_data.screen_buffer.write().unwrap().dirty_rows.fill(true);
                        } else if button == 274 { // Middle click
                            if let Some(clip) = &app_data.wayland_state.clipboard {
                                clip.request_paste();
                            }
                        }
                    }
                }
                PointerEvent::Release { button } => {
                    app_data.active_mouse_button = None;
                    let col_1 = ((app_data.pointer_x - pad_x) / cell_w).max(0.0) as usize + 1;
                    let row_1 = ((app_data.pointer_y - pad_y) / cell_h).max(0.0) as usize + 1;

                    if mouse_tracking_enabled {
                        let btn_code = match button {
                            272 => 0, // Left
                            274 => 1, // Middle
                            273 => 2, // Right
                            _ => 0,
                        };
                        if mouse_sgr_mode {
                            let seq = format!("\x1b[<{};{};{}m", btn_code, col_1, row_1);
                            if let Some(pty) = app_data.pty.as_mut() {
                                let _ = pty.write_all(seq.as_bytes());
                            }
                        }
                    } else {
                        if button == 272 { // Left click
                            app_data.drag_start = None;
                            app_data.is_dragging_scrollbar = false;
                            if app_data.config.behavior.copy_on_select {
                                let selection_opt = app_data.screen_buffer.read().unwrap().selection;
                                if let Some(sel) = selection_opt {
                                    let text = app_data.screen_buffer.read().unwrap().get_text_in_range(sel);
                                    if !text.is_empty() {
                                        if let Some(clip) = &app_data.wayland_state.clipboard {
                                            clip.set_clipboard(text, 0, &app_data.queue_handle); // Needs proper serial
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                PointerEvent::Axis { mut amount } => {
                    if app_data.is_hovering_edge {
                        amount *= 5.0;
                    }
                    if mouse_tracking_enabled {
                        let col_1 = ((app_data.pointer_x - pad_x) / cell_w).max(0.0) as usize + 1;
                        let row_1 = ((app_data.pointer_y - pad_y) / cell_h).max(0.0) as usize + 1;
                        
                        let btn_code = if amount > 0.0 { 65 } else { 64 };
                        if mouse_sgr_mode {
                            let seq = format!("\x1b[<{};{};{}M", btn_code, col_1, row_1);
                            if let Some(pty) = app_data.pty.as_mut() {
                                let _ = pty.write_all(seq.as_bytes());
                            }
                        }
                    } else {
                        app_data.scroll_accum += amount;
                        let threshold = 10.0;
                        if app_data.scroll_accum >= threshold {
                            let lines = (app_data.scroll_accum / threshold) as usize;
                            app_data.scroll_accum -= lines as f64 * threshold;
                            if app_data.screen_buffer.read().unwrap().use_alt_buffer {
                                if let Some(pty) = app_data.pty.as_mut() {
                                    for _ in 0..lines {
                                        let _ = pty.write_all(b"\x1b[B"); // Down arrow
                                    }
                                }
                            } else {
                                app_data.screen_buffer.write().unwrap().view_scroll_down(lines);
                            }
                        } else if app_data.scroll_accum <= -threshold {
                            let lines = (-app_data.scroll_accum / threshold) as usize;
                            app_data.scroll_accum += lines as f64 * threshold;
                            if app_data.screen_buffer.read().unwrap().use_alt_buffer {
                                if let Some(pty) = app_data.pty.as_mut() {
                                    for _ in 0..lines {
                                        let _ = pty.write_all(b"\x1b[A"); // Up arrow
                                    }
                                }
                            } else {
                                app_data.screen_buffer.write().unwrap().view_scroll_up(lines);
                            }
                        }
                    }
                }
            }
        }

        let wants_redraw = app_data.screen_buffer.read().unwrap().has_dirty_rows() || app_data.wayland_state.force_redraw;
        if app_data.wayland_state.frame_ready && wants_redraw {
            app_data.wayland_state.frame_ready = false;
            app_data.wayland_state.frame_callback_pending = false;

            if let Some(window) = app_data.wayland_state.window.as_ref() {
                if !app_data.wayland_state.frame_callback_pending {
                    crate::wayland::frame_callback::request_frame_callback(&window.surface, &app_data.queue_handle);
                    app_data.wayland_state.frame_callback_pending = true;
                    app_data.wayland_state.needs_flush = true;
                }
            }

            if let Some(renderer) = app_data.renderer.as_mut() {
                let sb = app_data.screen_buffer.read().unwrap();
                let cursor_row_in_viewport = sb.cursor.row as isize + sb.scroll_offset as isize;
                let cursor = if cursor_row_in_viewport < sb.rows() as isize {
                    Some((sb.cursor.col, cursor_row_in_viewport as usize))
                } else {
                    None
                };
                let bg_color = app_data.config.theme.background.to_srgb_linear();
                let final_alpha = bg_color.a * app_data.config.window.opacity;
                let default_bg = [bg_color.r, bg_color.g, bg_color.b, bg_color.a];
                let clear_color = [bg_color.r * final_alpha, bg_color.g * final_alpha, bg_color.b * final_alpha, final_alpha];
                
                let cursor_color = app_data.config.theme.cursor_color.to_srgb_linear();
                let cursor_color_arr = [cursor_color.r, cursor_color.g, cursor_color.b, cursor_color.a];
                

                let grid_refs: Vec<&[forge_core::cell::Cell]> = (0..sb.rows())
                    .map(|i| sb.visible_row(i))
                    .collect();
                
                let (win_w, win_h) = if let Some(window) = app_data.wayland_state.window.as_ref() {
                    (window.size.width as f64, window.size.height as f64)
                } else {
                    (800.0, 600.0)
                };

                let mut scrollbar_state = None;
                let mut needs_scrollbar_redraw = false;
                // DA06k/l - Minimal Scrollbar Math
                if !sb.use_alt_buffer {
                    let visible_screen_lines = sb.rows() as f64;
                    let history_lines = sb.scrollback_len() as f64;
                    let total_lines = visible_screen_lines + history_lines;
                    
                    if total_lines > visible_screen_lines {
                        let thumb_height_percentage = visible_screen_lines / total_lines;
                        let minimum_thumb_height_px = 20.0_f64;
                        let thumb_height_pixels = minimum_thumb_height_px.max(win_h * thumb_height_percentage);
                        
                        let viewport_offset = sb.scroll_offset as f64;
                        let scroll_ratio = if history_lines > 0.0 {
                            viewport_offset / history_lines
                        } else {
                            0.0
                        };
                        let track_top = 4.0;
                        let track_bottom = win_h - 4.0;
                        let usable_track_height = track_bottom - track_top;
                        
                        let available_travel_space = usable_track_height - thumb_height_pixels;
                        let thumb_y = track_top + available_travel_space - (scroll_ratio * available_travel_space);
                        
                        let idle_secs = std::time::Instant::now().duration_since(app_data.last_mouse_activity).as_secs_f32();
                        let active_secs = std::time::Instant::now().duration_since(app_data.mouse_started_moving).as_secs_f32();
                        let mut target_thumb_opacity = if active_secs >= 0.25 && idle_secs < 0.5 { 1.0 } else { 0.0 };
                        let mut target_track_opacity = 0.0;
                        let mut target_thumb_width = 5.0;
                        
                        if app_data.is_hovering_edge || app_data.is_dragging_scrollbar {
                            target_track_opacity = 1.0;
                            target_thumb_width = 9.0;
                            target_thumb_opacity = 1.0;
                        }
                        
                        app_data.current_thumb_width += (target_thumb_width - app_data.current_thumb_width) * 0.2;
                        app_data.current_track_opacity += (target_track_opacity - app_data.current_track_opacity) * 0.2;
                        app_data.current_thumb_opacity += (target_thumb_opacity - app_data.current_thumb_opacity) * 0.2;
                        
                        let is_animating = 
                            (target_thumb_width - app_data.current_thumb_width).abs() > 0.01 ||
                            (target_track_opacity - app_data.current_track_opacity).abs() > 0.01 ||
                            (target_thumb_opacity - app_data.current_thumb_opacity).abs() > 0.01 ||
                            (idle_secs < 0.5 && app_data.current_thumb_opacity > 0.01);

                        if is_animating {
                            needs_scrollbar_redraw = true;
                        }

                        let thumb_x = win_w as f32 - 4.0 - app_data.current_thumb_width;
                        let thumb_opacity = app_data.current_thumb_opacity;
                        let track_opacity = app_data.current_track_opacity;
                        let thumb_width = app_data.current_thumb_width;
                        
                        
                        if thumb_opacity > 0.01 || track_opacity > 0.01 {
                            scrollbar_state = Some((thumb_y as f32, thumb_height_pixels as f32, thumb_width, thumb_x, thumb_opacity, track_opacity));
                        }
                        app_data.last_scrollbar_state = Some((thumb_y as f64, thumb_height_pixels as f64));
                    }
                }

                let metrics = compute_grid_metrics(
                    win_w,
                    win_h,
                    &app_data.config.window.padding,
                    app_data.config.window.padding_balance,
                    renderer.cell_width as f64,
                    renderer.cell_height as f64,
                );
                let selection_bg_color = app_data.config.theme.selection_bg.to_srgb_linear();
                let selection_bg_arr = [selection_bg_color.r, selection_bg_color.g, selection_bg_color.b, selection_bg_color.a];
                let cursor_style = sb.cursor_style_override.clone().unwrap_or_else(|| app_data.config.cursor.style.clone());
                let cursor_visible_phase = app_data.cursor_visible_phase;
                
                let needs_recreate = match renderer.render_grid(
                    &grid_refs, 
                    &sb.dirty_rows.clone(),
                    cursor, 
                    cursor_style,
                    cursor_visible_phase,
                    sb.selection, 
                    default_bg,
                    clear_color,
                    cursor_color_arr,
                    selection_bg_arr,
                    metrics.pad_x as f32,
                    metrics.pad_y as f32,
                    metrics.effective_cell_w as f32,
                    metrics.effective_cell_h as f32,
                    metrics.scale_x as f32,
                    metrics.scale_y as f32,
                    scrollbar_state,
                    app_data.config.render.braille_style,
                ) {
                    Ok(n) => n,
                    Err(ForgeError::Vulkan(msg)) if msg == "Surface lost" => {
                        tracing::error!("Surface lost during rendering.");
                        app_data.wayland_state.running = false;
                        false
                    }
                    Err(e) => {
                        tracing::error!("Render error: {}", e);
                        false
                    }
                };
                drop(grid_refs);
                drop(sb);
                
                if needs_recreate {
                    if let Some(window) = app_data.wayland_state.window.as_ref() {
                        let _ = renderer.recreate_swapchain(window.size.width, window.size.height);
                    }
                }
                
                app_data.screen_buffer.write().unwrap().mark_all_clean();
                app_data.wayland_state.force_redraw = needs_scrollbar_redraw;
                if needs_scrollbar_redraw {
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

pub struct GridMetrics {
    pub cols: usize,
    pub rows: usize,
    pub pad_x: f64,
    pub pad_y: f64,
    pub effective_cell_w: f64,
    pub effective_cell_h: f64,
    pub scale_x: f64,
    pub scale_y: f64,
}

pub fn compute_grid_metrics(
    win_w: f64,
    win_h: f64,
    pad_cfg: &forge_core::config_registry::PaddingConfig,
    pad_balance: forge_core::config_registry::PaddingBalance,
    native_cell_w: f64,
    native_cell_h: f64,
) -> GridMetrics {
    let avail_w = (win_w - pad_cfg.left as f64 - pad_cfg.right as f64).max(native_cell_w);
    let avail_h = (win_h - pad_cfg.top as f64 - pad_cfg.bottom as f64).max(native_cell_h);
    
    let cols = (avail_w / native_cell_w).max(1.0) as usize;
    let rows = (avail_h / native_cell_h).max(1.0) as usize;
    
    let mut effective_cell_w = native_cell_w;
    let mut effective_cell_h = native_cell_h;
    let mut scale_x = 1.0;
    let mut scale_y = 1.0;
    let mut pad_x = pad_cfg.left as f64;
    let mut pad_y = pad_cfg.top as f64;
    
    if pad_balance == forge_core::config_registry::PaddingBalance::Fill {
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
    
    GridMetrics {
        cols,
        rows,
        pad_x,
        pad_y,
        effective_cell_w,
        effective_cell_h,
        scale_x,
        scale_y,
    }
}
