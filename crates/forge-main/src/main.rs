use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Log Levels Guide:
/// - TRACE — hot path events (per-frame, per-byte). Enable only for profiling.
/// - DEBUG — component lifecycle events (startup, initialization, teardown).
/// - INFO  — user-visible status (window opened, config loaded, plugin installed).
/// - WARN  — recoverable problems (invalid config value, plugin load failure).
/// - ERROR — unrecoverable failures that require shutdown.
fn init_logging() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("forge=debug,warn"));

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true).with_thread_ids(true))
        .with(env_filter)
        .init();
}

fn main() {
    init_logging();
    forge_core::crash::install_panic_handler();

    let result = std::panic::catch_unwind(|| run());

    if result.is_err() {
        tracing::error!("Forge terminated due to a panic. See crash.log for details.");
        std::process::exit(1);
    }
}

pub mod event_loop;
mod font_paths;
pub mod wayland;

fn run() -> forge_core::Result<()> {
    tracing::info!("Forge starting...");
    let total_start = std::time::Instant::now();

    // --- Config Actor (Spawn Early in background) ---
    let t_lua = std::time::Instant::now();
    let config_path = dirs::config_dir()
        .unwrap_or_default()
        .join("forge/config.lua");

    let config_handle = {
        let _span = tracing::debug_span!("startup.spawn_config_actor").entered();
        forge_config::actor::spawn_config_actor(config_path.clone())
    };
    tracing::info!(
        "[PROFILER] Lua Config Actor Spawn took: {:?}",
        t_lua.elapsed()
    );

    // --- Fast-path startup ---
    let t_fast_path = std::time::Instant::now();
    let cache = {
        let _span = tracing::debug_span!("startup.read_cache").entered();
        forge_core::cache::read_startup_cache()
    };

    // --- Wayland Connection ---
    let (mut wayland_state, mut event_queue) = {
        let _span = tracing::debug_span!("startup.connect_wayland").entered();
        crate::wayland::connect_wayland()?
    };

    // --- Window Creation ---
    let initial_size = cache
        .as_ref()
        .map(|c| forge_core::geometry::Size {
            width: c.window_width,
            height: c.window_height,
        })
        .unwrap_or(forge_core::geometry::Size {
            width: 800,
            height: 600,
        });

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
    let (bg_r, bg_g, bg_b) = cache
        .as_ref()
        .map(|c| {
            (
                c.background_color.r,
                c.background_color.g,
                c.background_color.b,
            )
        })
        .unwrap_or((26, 27, 38));
    let bg_a = cache.as_ref().map(|c| c.opacity).unwrap_or(255);

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

    tracing::info!("First frame presented. Entering event loop.");
    tracing::info!(
        "[PROFILER] Fast-Path Cache & Wayland SHM First Frame took: {:?}",
        t_fast_path.elapsed()
    );

    // Wait for the background config actor to finish reading config.lua
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
    let cell_w = cache.as_ref().map(|c| c.cell_width).unwrap_or(10);
    let cell_h = cache.as_ref().map(|c| c.cell_height).unwrap_or(20);
    let baseline = cache.as_ref().map(|c| c.baseline).unwrap_or(16);

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
    tracing::info!(
        "[PROFILER] Vulkan Boot (Renderer::new) took: {:?}",
        t_vulkan.elapsed()
    );

    let metrics = crate::event_loop::compute_grid_metrics(
        window_size.width as f64,
        window_size.height as f64,
        &config.window.padding,
        config.window.padding_balance,
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
        config.scrollback.lines,
        config.theme.foreground,
        config.theme.background,
    );
    screen_buffer.palette = config.theme.ansi_colors;
    let vte_processor = forge_pty::VteProcessor::new();

    let (key_tx, key_rx) = std::sync::mpsc::sync_channel(1024);
    wayland_state.key_sender = Some(key_tx);

    let (pointer_tx, pointer_rx) = std::sync::mpsc::sync_channel(1024);
    wayland_state.pointer_sender = Some(pointer_tx);

    let (paste_tx, paste_rx) = std::sync::mpsc::sync_channel(1024);

    if let Some(wl_seat) = wayland_state.globals.wl_seat.as_ref() {
        if let Some(data_device_manager) = wayland_state.globals.data_device_manager.clone() {
            let mut clip = crate::wayland::clipboard::ClipboardManager::new(data_device_manager);
            clip.init_device(wl_seat, &event_queue.handle());
            clip.paste_sender = Some(paste_tx);
            wayland_state.clipboard = Some(clip);
        } else {
            tracing::warn!(
                "Wayland data device manager is unavailable; clipboard integration disabled."
            );
        }
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
        let Some(font_files) = crate::font_paths::resolve_font_files(&font_config) else {
            tracing::warn!("No usable font file found. Keeping startup dummy font atlas.");
            return;
        };

        let font_size = font_config.size;
        match std::fs::read(&font_files.regular)
            .map_err(|e| forge_core::ForgeError::Other(e.to_string()))
            .and_then(|font_bytes| {
                forge_renderer::font::rasterizer::FontRasterizer::from_bytes(&font_bytes, font_size)
            }) {
            Ok(font_rasterizer) => {
                let bold_rasterizer = font_files.bold.as_ref().and_then(|bold_path| {
                    std::fs::read(bold_path)
                        .map_err(|e| {
                            tracing::warn!(
                                "Failed to read bold font {}: {}",
                                bold_path.display(),
                                e
                            );
                            e
                        })
                        .ok()
                        .and_then(|bold_bytes| {
                            match forge_renderer::font::rasterizer::FontRasterizer::from_bytes(
                                &bold_bytes,
                                font_size,
                            ) {
                                Ok(rasterizer) => Some(rasterizer),
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to load bold font {}: {}",
                                        bold_path.display(),
                                        e
                                    );
                                    None
                                }
                            }
                        })
                });
                let fallback_rasterizers: Vec<_> = font_files
                    .fallbacks
                    .iter()
                    .filter_map(|fallback_path| {
                        std::fs::read(fallback_path)
                            .map_err(|e| {
                                tracing::warn!(
                                    "Failed to read fallback font {}: {}",
                                    fallback_path.display(),
                                    e
                                );
                                e
                            })
                            .ok()
                            .and_then(|font_bytes| {
                                match forge_renderer::font::rasterizer::FontRasterizer::from_bytes(
                                    &font_bytes,
                                    font_size,
                                ) {
                                    Ok(rasterizer) => Some(rasterizer),
                                    Err(e) => {
                                        tracing::warn!(
                                            "Failed to load fallback font {}: {}",
                                            fallback_path.display(),
                                            e
                                        );
                                        None
                                    }
                                }
                            })
                    })
                    .collect();

                match forge_renderer::font::atlas::GlyphAtlas::build(
                    &font_rasterizer,
                    bold_rasterizer.as_ref(),
                    font_size,
                    false,
                ) {
                    Ok(full_atlas) => {
                        tracing::info!(
                            "Background full font rasterization took: {:?}",
                            font_start.elapsed()
                        );
                        let _ = font_tx.send((
                            font_rasterizer,
                            bold_rasterizer,
                            fallback_rasterizers,
                            font_size,
                            full_atlas,
                        ));
                        loop_sig_font.wakeup();
                    }
                    Err(e) => tracing::warn!("Failed to build font atlas: {}", e),
                }
            }
            Err(e) => tracing::warn!(
                "Failed to load regular font {}: {}. Keeping startup dummy font atlas.",
                font_files.regular.display(),
                e
            ),
        }
    });

    tracing::info!(
        "[PROFILER] TOTAL TTFF PRE-LOOP took: {:?}",
        total_start.elapsed()
    );

    // 2. Spawn config watcher.
    // Keep the watcher alive by storing it in AppData.
    let watcher =
        forge_config::watcher::spawn_config_watcher(config_path, config_handle.tx.clone());

    // Proxy thread to wake up event loop when config changes
    let (config_tx2, config_rx2) = crossbeam_channel::unbounded();
    let loop_sig_cfg = loop_signal.clone();
    let orig_cfg_rx = config_handle.rx;
    std::thread::spawn(move || {
        while let Ok(update) = orig_cfg_rx.recv() {
            let _ = config_tx2.send(update);
            loop_sig_cfg.wakeup();
        }
    });

    // --- Main Event Loop ---
    crate::event_loop::run_event_loop(
        event_loop,
        wayland_state,
        event_queue,
        pty,
        std::sync::Arc::new(std::sync::RwLock::new(screen_buffer)),
        vte_processor,
        key_rx,
        pointer_rx,
        paste_rx,
        config,
        Some(renderer),
        Some(font_rx),
        Some(config_rx2),
        watcher,
        total_start,
    )?;

    tracing::info!("Event loop exited. Forge shutting down.");
    Ok(())
}
