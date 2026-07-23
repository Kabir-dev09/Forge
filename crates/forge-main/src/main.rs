use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Log Levels Guide:
/// - TRACE — hot path events (per-frame, per-byte). Enable only for profiling.
/// - DEBUG — component lifecycle events (startup, initialization, teardown).
/// - INFO  — user-visible status (window opened, config loaded, plugin installed).
/// - WARN  — recoverable problems (invalid config value, plugin load failure).
/// - ERROR — unrecoverable failures that require shutdown.
fn init_logging() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("forge=info,warn"));

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true).with_thread_ids(true))
        .with(env_filter)
        .init();
}

fn main() {
    init_logging();
    forge_core::crash::install_panic_handler();

    let result = std::panic::catch_unwind(run);

    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::error!("Forge terminated with error: {}", e);
            std::process::exit(1);
        }
        Err(_) => {
            tracing::error!("Forge terminated due to a panic. See crash.log for details.");
            std::process::exit(1);
        }
    }
}

pub mod confirm_modal;
pub mod context_menu;
pub mod event_loop;
mod font_paths;
pub mod mux;
pub mod sidebar;
pub mod statusbar;
pub mod wayland;

fn run() -> forge_core::Result<()> {
    tracing::info!("Forge starting...");
    let total_start = std::time::Instant::now();

    // --- Config Actor (Spawn Early in background) ---
    let t_config = std::time::Instant::now();
    let config_path = dirs::config_dir()
        .unwrap_or_default()
        .join("forge/config.toml");

    let config_handle = {
        let _span = tracing::debug_span!("startup.spawn_config_actor").entered();
        forge_config::actor::spawn_config_actor(config_path.clone())
    };
    tracing::debug!(
        "[PROFILER] TOML Config Actor Spawn took: {:?}",
        t_config.elapsed()
    );

    // --- Fast-path startup ---
    let t_fast_path = std::time::Instant::now();
    let early_config = {
        let _span = tracing::debug_span!("startup.read_early_config").entered();
        forge_core::config_registry::EarlyStartupConfig::load(&config_path)
    };

    // --- Wayland Connection ---
    let (mut wayland_state, mut event_queue) = {
        let _span = tracing::debug_span!("startup.connect_wayland").entered();
        crate::wayland::connect_wayland()?
    };

    // --- Window Creation ---
    let initial_size = forge_core::geometry::Size {
        width: early_config.window.width,
        height: early_config.window.height,
    };

    let window = {
        let _span = tracing::debug_span!(
            "startup.create_window",
            width = initial_size.width,
            height = initial_size.height
        )
        .entered();
        crate::wayland::window::WaylandWindow::new(
            &wayland_state.globals.compositor,
            &wayland_state.globals.xdg_wm_base,
            wayland_state.globals.zxdg_decoration_manager.as_ref(),
            &event_queue.handle(),
            initial_size,
            "Forge",
        )?
    };

    wayland_state.window = Some(window);

    // Wait for compositor to configure the window.
    {
        let _span = tracing::debug_span!("startup.wait_for_configure").entered();
        while !wayland_state.window.as_ref().is_some_and(|w| w.configured) {
            event_queue
                .blocking_dispatch(&mut wayland_state)
                .map_err(|e| forge_core::ForgeError::Wayland(e.to_string()))?;
        }
    }

    // --- SHM First Frame ---
    let bg_color = crate::statusbar::parse_hex_color(&early_config.theme.background)
        .unwrap_or(forge_core::color::Color { r: 26, g: 27, b: 38, a: 255 });
    let (bg_r, bg_g, bg_b) = (bg_color.r, bg_color.g, bg_color.b);
    let bg_a = (early_config.window.opacity * 255.0) as u8;

    let window_size = wayland_state
        .window
        .as_ref()
        .ok_or_else(|| {
            forge_core::ForgeError::Wayland(
                "Wayland window was not initialized before first frame".to_string(),
            )
        })?
        .size;
    let shm_buf = {
        let _span = tracing::debug_span!(
            "startup.present_shm_first_frame",
            width = window_size.width,
            height = window_size.height
        )
        .entered();
        let mut shm_buf = crate::wayland::shm_buffer::ShmBuffer::new(
            &wayland_state.globals.shm,
            &event_queue.handle(),
            window_size,
        )?;
        shm_buf.fill_color(bg_r, bg_g, bg_b, bg_a);
        let surface = &wayland_state
            .window
            .as_ref()
            .ok_or_else(|| {
                forge_core::ForgeError::Wayland(
                    "Wayland surface was not available for SHM first frame".to_string(),
                )
            })?
            .surface;
        shm_buf.present(surface);
        event_queue
            .flush()
            .map_err(|e| forge_core::ForgeError::Wayland(e.to_string()))?;
        shm_buf
    };

    // Store shm_buffer to keep it alive
    wayland_state.shm_buffer = Some(shm_buf);

    tracing::info!("Window appeared in {:?}.", total_start.elapsed());
    tracing::debug!(
        "[PROFILER] Fast-Path Cache & Wayland SHM First Frame took: {:?}",
        t_fast_path.elapsed()
    );

    // Wait for the background config actor to finish reading config.toml
    // (This usually completes instantly because it was spawned at the very beginning)
    let config = {
        let _span = tracing::debug_span!("startup.receive_initial_config").entered();
        config_handle
            .rx
            .recv()
            .map(|u| u.config)
            .unwrap_or_default()
    };
    tracing::info!("Configuration loaded.");

    {
        let compositor = wayland_state.globals.compositor.clone();
        let kde_blur_manager = wayland_state.globals.kde_blur_manager.clone();
        if let Some(window) = wayland_state.window.as_mut() {
            let blur_status = window.blur.apply(
                &window.surface,
                &compositor,
                kde_blur_manager.as_ref(),
                &event_queue.handle(),
                window.size,
                &config.blur,
            );
            tracing::debug!(?blur_status, "Initial Wayland blur state applied");
        }
    }
    crate::wayland::niri_blur_rule::ensure_rule_after_launch(&config.blur);

    let wl_display_ptr =
        wayland_backend::client::Backend::display_ptr(&wayland_state.conn.backend())
            as *mut std::ffi::c_void;
    use wayland_client::Proxy;
    let wl_surface_ptr = {
        let window = wayland_state.window.as_ref().ok_or_else(|| {
            forge_core::ForgeError::Wayland(
                "Wayland surface was not available for Vulkan initialization".to_string(),
            )
        })?;
        wayland_backend::client::ObjectId::as_ptr(&window.surface.id()) as *mut std::ffi::c_void
    };

    let t_vulkan = std::time::Instant::now();
    let cell_w = 10;
    let cell_h = 20;
    let baseline = 16;

    let mut renderer = {
        let _span = tracing::debug_span!(
            "startup.create_vulkan_renderer",
            width = window_size.width,
            height = window_size.height,
            cell_width = cell_w,
            cell_height = cell_h,
            baseline = baseline
        )
        .entered();
        forge_renderer::Renderer::new(
            wl_display_ptr,
            wl_surface_ptr,
            window_size.width,
            window_size.height,
            cell_w,
            cell_h,
            baseline,
        )?
    };
    renderer.set_ligature_config(config.font.ligatures.clone());
    renderer.set_cursor_trail_config(&config.cursor.trail);
    tracing::debug!(
        "[PROFILER] Vulkan Boot (Renderer::new) took: {:?}",
        t_vulkan.elapsed()
    );

    let metrics = crate::event_loop::compute_grid_metrics(
        window_size.width as f64,
        window_size.height as f64,
        &config.window.padding,
        config.window.center_grid,
        &config.statusbar,
        0,
        cell_w as f64,
        cell_h as f64,
    );
    let cols = metrics.cols;
    let rows = metrics.rows;
    let mut winsize = forge_pty::pty::size_to_winsize(
        forge_core::geometry::Size {
            width: window_size.width,
            height: window_size.height,
        },
        1,
        1,
    );
    winsize.ws_col = cols as u16;
    winsize.ws_row = rows as u16;
    winsize.ws_xpixel = (cols as f64 * metrics.effective_cell_w) as u16;
    winsize.ws_ypixel = (rows as f64 * metrics.effective_cell_h) as u16;

    let pty = {
        let _span = tracing::debug_span!(
            "startup.spawn_pty",
            cols = cols,
            rows = rows,
            shell = %config.shell.program
        )
        .entered();
        forge_pty::Pty::spawn(&config.shell, winsize)?
    };
    tracing::info!(
        "PTY spawned. Shell: {}, Cols: {}, Rows: {}",
        config.shell.program,
        cols,
        rows
    );

    let mut screen_buffer = forge_pty::ScreenBuffer::new(
        cols,
        rows,
        config.scrollback.lines.unwrap_or(100_000),
        config.theme.parsed_foreground,
        config.theme.parsed_background,
    );
    screen_buffer.palette = config.theme.parsed_ansi_colors;
    let vte_processor = forge_pty::VteProcessor::new();

    let (key_tx, key_rx) = std::sync::mpsc::sync_channel(1024);
    wayland_state.key_sender = Some(key_tx);

    let (pointer_tx, pointer_rx) = std::sync::mpsc::sync_channel(1024);
    wayland_state.pointer_sender = Some(pointer_tx);

    let (paste_tx, paste_rx) = std::sync::mpsc::sync_channel(1024);

    if let Some(clipboard) = wayland_state.clipboard.as_mut() {
        clipboard.paste_sender = Some(paste_tx);
    }

    // Once the Vulkan first frame is submitted, drop the SHM buffer.
    let clear_color_linear = forge_core::color::Color {
        r: bg_r,
        g: bg_g,
        b: bg_b,
        a: bg_a,
    }
    .to_srgb_linear();
    let clear_color = [
        clear_color_linear.r,
        clear_color_linear.g,
        clear_color_linear.b,
        clear_color_linear.a,
    ];

    // Request frame callback BEFORE Vulkan commit so it attaches to this frame.
    if let Some(window) = wayland_state.window.as_ref() {
        crate::wayland::frame_callback::request_frame_callback(
            &window.surface,
            &event_queue.handle(),
        );
        wayland_state.frame_callback_pending = true;
    }

    let needs_recreate = {
        let _span = tracing::debug_span!("startup.initial_vulkan_clear").entered();
        match renderer.render_clear(clear_color) {
            Ok(needs) => needs,
            Err(forge_core::ForgeError::Vulkan(msg)) if msg == "Surface lost" => {
                tracing::error!("Surface lost during initial render.");
                wayland_state.running = false;
                false
            }
            Err(e) => return Err(e),
        }
    };
    if needs_recreate {
        renderer.recreate_swapchain(window_size.width, window_size.height)?;
    }
    // Drop the SHM buffer
    drop(wayland_state.shm_buffer.take());
    tracing::info!("SHM→Vulkan handover complete.");

    // --- Create Event Loop here to get LoopSignal ---
    let event_loop: calloop::EventLoop<crate::event_loop::AppData> =
        calloop::EventLoop::try_new().map_err(|e| forge_core::ForgeError::Other(e.to_string()))?;
    let loop_signal = event_loop.get_signal();

    // --- Background Font Loading ---
    let (font_tx, font_rx) = std::sync::mpsc::sync_channel(1);
    let loop_sig_font = loop_signal.clone();
    let font_config = config.font.clone();
    std::thread::spawn(move || {
        let font_start = std::time::Instant::now();
        match crate::font_paths::load_font_data(&font_config) {
            Ok(font_data) => {
                tracing::info!(
                    "Font data prepared in {:?} (family={})",
                    font_start.elapsed(),
                    font_config.family
                );
                let _ = font_tx.send(font_data);
                loop_sig_font.wakeup();
            }
            Err(error) => {
                tracing::warn!(%error, "Failed to prepare font data; keeping startup dummy atlas")
            }
        }
    });

    tracing::debug!(
        "[PROFILER] TOTAL TTFF PRE-LOOP took: {:?}",
        total_start.elapsed()
    );

    // Proxy thread to wake up event loop after an explicit config reload.
    let (config_tx2, config_rx2) = crossbeam_channel::unbounded();
    let loop_sig_cfg = loop_signal.clone();
    let orig_cfg_rx = config_handle.rx;
    std::thread::spawn(move || {
        while let Ok(update) = orig_cfg_rx.recv() {
            let _ = config_tx2.send(update);
            loop_sig_cfg.wakeup();
        }
    });

    let snapshot = std::sync::Arc::new(arc_swap::ArcSwap::from_pointee(
        screen_buffer.generate_snapshot(),
    ));
    let mut mux = crate::mux::MuxState::single_pane(
        pty,
        snapshot.clone(),
        crate::mux::GridSize::new(cols, rows),
    );
    let startup_content_rect = crate::mux::PaneRect::new(
        metrics.pad_x as f32,
        metrics.pad_y as f32,
        (cols as f64 * metrics.effective_cell_w) as f32,
        (rows as f64 * metrics.effective_cell_h) as f32,
    );
    if let Err(err) = mux.relayout(crate::mux::LayoutParams::new(
        startup_content_rect,
        metrics.effective_cell_w as f32,
        metrics.effective_cell_h as f32,
        config.window.gap as f32,
        forge_core::config_registry::PaddingConfig::default(),
    )) {
        tracing::warn!(
            ?err,
            "Initial mux relayout failed; using startup grid metrics"
        );
    }

    // --- Main Event Loop ---
    crate::event_loop::run_event_loop(
        event_loop,
        wayland_state,
        event_queue,
        mux,
        vte_processor,
        screen_buffer,
        key_rx,
        pointer_rx,
        paste_rx,
        config,
        Some(renderer),
        Some(font_rx),
        Some(config_rx2),
        total_start,
    )?;

    tracing::info!("Event loop exited. Forge shutting down.");
    Ok(())
}
