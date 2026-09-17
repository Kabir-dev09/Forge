# Forge Execution Traces

This document details step-by-step execution traces for critical lifecycle events in Forge, derived from forensic codebase analysis.

## Trace 1: Application Startup

1. **Initialization:**
   - Entry point: `forge-main/src/main.rs:run`.
   - Logging and tracing are initialized.
   - Command-line arguments are parsed.
   - `forge-core::config_registry` parses the TOML config file.
   - A background thread (`config_watcher`) is spawned to monitor `forge.toml` for runtime changes.

2. **Wayland & Vulkan Bootstrap:**
   - `wayland::connection::connect_wayland()` establishes the display connection, binds globals, and sets up `WaylandState`.
   - The primary Wayland window is created, but kept unmapped initially.
   - A temporary SHM (Shared Memory) buffer is mapped and attached to the window surface. A flat background color frame is submitted immediately to satisfy Wayland compositor requirements and provide visually instantaneous startup.
   - The heavy Vulkan initialization (`forge_renderer::Renderer::new`) occurs.

3. **Font Loading (Async):**
   - A background thread is spawned to run `font_paths::load_font_data()`.
   - It invokes `fc-match` to locate the configured font and fallbacks.
   - `FontRasterizer` parses the fonts via `fontdue`.
   - `GlyphAtlas::build` statically rasterizes ASCII characters into a RAM buffer.

4. **Terminal Setup:**
   - `forge-pty::pty::Pty::spawn_in_dir` forks the process, sets up the PTY master/slave fds, and `execvpe`s the user's shell (injecting shell integration scripts if enabled).
   - `forge-pty::ScreenBuffer` and `VteProcessor` are instantiated.
   - `MuxState` is created to track the first tab and pane.

5. **Worker Handoff & Main Loop:**
   - `PaneIoRegistry::run_worker` is spawned on a dedicated thread, taking ownership of the PTY fd.
   - The font loader thread finishes and sends `FontData` over a channel.
   - The main thread enters `event_loop::run_event_loop`, handing control to `calloop`.

## Trace 2: Window Resize

1. **Wayland Event:**
   - Compositor sends `xdg_toplevel::Event::Configure { width, height }`.
   - Handled in `wayland/connection.rs`. Updates `state.window.size`.
2. **Event Loop Detection:**
   - `run_event_loop` detects that `wayland_state.window.size` differs from `last_window_size`.
3. **Renderer Reconfiguration:**
   - `Renderer::recreate_swapchain(width, height)` is called, rebuilding framebuffers and image views.
4. **Layout Recalculation:**
   - `MuxState::relayout` calculates new pixel bounds for all panes, tabs, and splits based on the new total window size.
5. **PTY Worker Notification:**
   - Main thread sends `PtyWorkerCommand::Resize(pane, cols, rows, px_w, px_h)` via channel to the PTY worker thread.
6. **PTY Update:**
   - PTY Worker receives the command.
   - Calls `ScreenBuffer::resize_reflow(cols, rows)`, which reflows wrapped lines and reallocates the grid.
   - Calls the OS-level `ioctl(TIOCSWINSZ)` on the PTY master fd to notify the child shell (causing a `SIGWINCH`).
   - Forces a full redraw by incrementing all `dirty_generations`.

## Trace 3: Dynamic Glyph Rasterization

1. **Missing Glyph Encountered:**
   - During `GridTessellator::tessellate`, a character (e.g., 'Ω') is not found in the `GlyphAtlas`.
   - It is added to the `missing_glyphs` HashSet.
2. **Upload Trigger:**
   - Before submitting vertices to Vulkan, `Renderer::submit_tessellated_vertices` notices `missing_glyphs` is not empty.
   - Calls `Renderer::insert_dynamic_glyphs(keys)`.
3. **Rasterization:**
   - `GlyphAtlas::insert_dynamic_glyph` selects the next available slot in the atlas grid (`next_dynamic_slot`).
   - `FontRasterizer` uses `fontdue` to rasterize 'Ω' at the current `px_size`.
   - The coverage bitmap is converted to an RGBA pixel array representing the sub-region.
4. **GPU Upload:**
   - A `DynamicGlyphUpdate` is generated containing the pixels and coordinates.
   - `atlas_texture.update_regions` maps GPU memory or issues a staging buffer copy via `vkCmdCopyBufferToImage` to patch the live texture atlas on the GPU without rebuilding the whole texture.
5. **Caching:**
   - When the application closes (or periodically), `persist_custom_atlas_cache` writes the updated atlas state back to `~/.cache/forge/font-atlas.fga`.
