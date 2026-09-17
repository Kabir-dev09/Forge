# Forge File Responsibility Map

This map highlights the most critical files in the Forge codebase and outlines their architectural responsibilities based on forensic analysis.

## `forge-main` (Application & UI)
- **`src/main.rs`**
  - *Responsibility:* Application entry point. Bootstraps the Wayland connection, initializes the Vulkan renderer, spawns the PTY worker thread, launches the async font loader, creates the initial multiplexer state, and hands control over to the main event loop.
- **`src/event_loop.rs`**
  - *Responsibility:* Contains the main UI thread `calloop` event loop (`run_event_loop`, ~7000 lines). Handles Wayland input, window resizing, rendering triggers, config reloading, context menus, and high-level layout management (tabs, splits, scrolling).
- **`src/wayland/connection.rs`**
  - *Responsibility:* Wayland global registry setup. Binds to core protocols (`wl_compositor`, `wl_shm`, `xdg_wm_base`, `wl_seat`) and manages the overarching `WaylandState`.
- **`src/wayland/seat.rs`**
  - *Responsibility:* Implements Wayland input handling. Translates raw `wl_keyboard` events via `xkbcommon` into Forge keybindings or raw PTY input. Dispatches `wl_pointer` events for mouse interactions.
- **`src/mux/io.rs`**
  - *Responsibility:* Contains the PTY Worker thread (`PaneIoRegistry::run_worker`). Multiplexes non-blocking I/O across all active PTY file descriptors, feeding read bytes to the VTE parser and processing commands from the UI thread.
- **`src/font_paths.rs`**
  - *Responsibility:* Font discovery. Uses `fc-match` (fontconfig) to locate font files on the host system based on family and style, and resolves fallback fonts based on charset.

## `forge-pty` (Terminal State & Parsing)
- **`src/pty.rs`**
  - *Responsibility:* OS-level process management. Uses `fork` and `execvpe` to spawn shells attached to a pseudoterminal. Manages shell integration injection.
- **`src/screen_buffer.rs`**
  - *Responsibility:* Core terminal state model. Maintains a grid of `Cell`s (`VecDeque<Row>`), tracks cursor position, handles the scrollback buffer, and maintains `dirty_generations` for rendering optimization.
- **`src/vte_parser.rs`**
  - *Responsibility:* Implements the `vte::Perform` trait. Translates escape sequences parsed by the `vte` state machine into concrete actions on the `ScreenBuffer`.

## `forge-renderer` (Graphics & Fonts)
- **`src/renderer.rs`**
  - *Responsibility:* Vulkan rendering backend. Manages the swapchain, pipelines, command buffers, and descriptor sets. Coordinates the upload of dynamically rasterized glyphs to the GPU texture atlas.
- **`src/grid_tessellator.rs`**
  - *Responsibility:* Converts a `RenderSnapshot` of the terminal grid into Vulkan vertex data (`GlyphVertex`). Implements dirty-row tracking to only rebuild vertices for modified rows, and identifies missing glyphs that need dynamic rasterization.
- **`src/font/rasterizer.rs`**
  - *Responsibility:* Wraps `fontdue` to parse font files and rasterize individual characters or glyph IDs into coverage bitmaps and metrics.
- **`src/font/shaper.rs`**
  - *Responsibility:* Wraps `rustybuzz` to provide text shaping (ligatures). Converts strings of text into a series of shaped glyphs and their layout positions, caching the results in a `ShaperCache`.
- **`src/font/atlas.rs`**
  - *Responsibility:* Manages the `GlyphAtlas`, a 2D packing of rasterized glyphs. Handles both statically pre-rendered glyphs and a fixed number (1024) of dynamically inserted glyphs. Supports serialization to disk for caching.

## `forge-core` (Shared Primitives)
- **`src/cell.rs`**
  - *Responsibility:* Defines the fundamental `Cell` struct representing a single terminal coordinate (contains a `char`, foreground/background colors, and style flags).
- **`src/config_registry.rs`**
  - *Responsibility:* Defines the TOML configuration schema for Forge (fonts, colors, keybindings, etc.).
