# Forge Terminal Emulator - Codebase Analysis

## 1. High-Level Architecture Overview
Forge is a high-performance terminal emulator built in Rust, utilizing a multi-process/multi-threaded architecture to separate UI rendering from terminal state management and PTY I/O.
The codebase is structured as a Cargo workspace with the following main crates:
- `forge-main`: The main application loop, Wayland window management, input handling, and multiplexer logic (tabs, panes).
- `forge-core`: Shared types, configuration, and bindings.
- `forge-pty`: Terminal state (`ScreenBuffer`), PTY process management, and VTE escape sequence parsing.
- `forge-renderer`: Vulkan-based hardware-accelerated rendering, font shaping, and glyph atlases.

**Key Design Principle:** Strict separation of the UI thread and the PTY worker thread. Communication happens lock-free or via channels, avoiding mutex contention on the hot path.

## 2. PTY and Process Management
- **Spawning:** PTYs are spawned using `nix::pty::openpty` and a traditional `fork()`/`execvpe()` mechanism in `forge-pty/src/pty.rs`.
- **Worker Thread:** A dedicated background thread (`PaneIoRegistry::run_worker` in `forge-main/src/mux/io.rs`) runs a `calloop` event loop to manage all active PTY file descriptors.
- **I/O:** The worker thread polls the PTYs using non-blocking I/O. Read bytes are passed to the `VteProcessor`.
- **Shell Integration:** Custom initialization scripts for various shells (bash, zsh, fish, nu) are injected via temporary files to enable features like working directory tracking and command completion tracking.

## 3. Terminal State and Parsing
- **State Representation:** `forge_pty::ScreenBuffer` represents the terminal state. It uses a `VecDeque<Row>` for the active grid and a `Scrollback` ring buffer for history. Each `Cell` stores a single `char` (Unicode scalar), foreground/background colors, and style flags (bold, italic, etc.).
- **VTE Parsing:** Uses the `vte` crate. `forge_pty::vte_parser::TerminalPerformer` implements `vte::Perform`, translating parsed escape sequences into mutations on the `ScreenBuffer` (e.g., cursor movement, text insertion, SGR color changes).
- **Fast Path:** An ASCII fast path (`VteProcessor::process_with_ascii_fast_path`) batches printable ASCII and newlines, bypassing the state machine overhead for standard text.

## 4. Concurrency Model
- **UI / Main Thread:** Runs the massive UI event loop (also `calloop` based) in `forge-main/src/event_loop.rs`. It handles Wayland input (keys, pointer), Vulkan rendering, layout (tabs/splits), and config reloads.
- **PTY Worker Thread:** Handles all PTY reads/writes and terminal state mutations.
- **Synchronization:** The worker thread mutates the `ScreenBuffer` and periodically generates a `RenderSnapshot`. This snapshot is shared with the main thread lock-free using `arc-swap::ArcSwap<RenderSnapshot>`. The main thread reads this atomic pointer to get a consistent view of the terminal for rendering, avoiding locking the `ScreenBuffer`.
- **Channels:** Input events from the UI thread (keystrokes, resize commands) are sent to the PTY worker via `std::sync::mpsc::sync_channel` and `calloop::channel`.

## 5. Wayland and Windowing Layer
- **Protocol Management:** Uses `wayland-client`. Connection and global registry setup are in `forge-main/src/wayland/connection.rs`.
- **Input:** Keyboard and pointer events are processed via `wl_seat`, `wl_keyboard`, and `wl_pointer` protocols (`forge-main/src/wayland/seat.rs`).
- **Key Mapping:** `xkbcommon` is used to translate raw keycodes into keysyms and utf8 strings. These are mapped against the user's `Keybindings` to trigger internal actions or sent as raw bytes to the PTY worker.

## 6. Rendering Pipeline
- **Backend:** `ash`-based Vulkan renderer (`forge-renderer`).
- **Tessellation:** `GridTessellator` takes a `RenderSnapshot` and converts the grid of `Cell`s into Vulkan `GlyphVertex` arrays.
- **Double Buffering:** The renderer uses double-buffered command buffers and fences (`FrameVertexUploadState`) to overlap CPU tessellation with GPU execution.
- **Optimization:** `ScreenBuffer` tracks `dirty_generations` per row. The `GridTessellator` only rebuilds vertex data for rows that have changed since the last frame, significantly reducing CPU overhead.

## 7. Font and Glyph Pipeline
- **Discovery and Loading:** Uses `fc-match` (`fontconfig`) to discover system fonts in `forge-main/src/font_paths.rs`. Fonts are loaded asynchronously in a background thread to keep startup fast.
- **Parsing and Rasterization:** Uses `fontdue` (`FontRasterizer`) to parse font files and rasterize glyphs into coverage bitmaps.
- **Shaping:** `rustybuzz` is used in `forge-renderer/src/font/shaper.rs` to support font ligatures. The shaper groups contiguous characters and maps them to shaped glyph IDs.
- **Glyph Atlas:** `GlyphAtlas` dynamically caches rasterized glyphs in a single Vulkan texture. The atlas can hold up to 1024 dynamic glyphs. Missing glyphs are collected during tessellation and uploaded via `atlas_texture.update_regions`.
- **Eviction:** The dynamic glyph cache is append-only. If it fills up (`next_dynamic_slot >= total_slots`), new unique glyphs are ignored and logged as a warning. The atlas is persisted to disk (`~/.cache/forge/font-atlas.fga`) for faster subsequent startups.

## 8. Configuration
- Configuration is deserialized from TOML using `serde` (`forge-core/src/config_registry.rs`).
- A background thread (`config_watcher`) uses `notify` to watch the config file for changes. Changes are sent to the main thread via a channel and applied dynamically via a difference check (`ConfigChangeSet`).

## 9. Error Handling
- Defines a centralized `ForgeError` enum using `thiserror` in `forge-core/src/lib.rs`.
- PTY spawn failures inject an error message directly into the terminal screen buffer so the user sees it visually without crashing the app.
