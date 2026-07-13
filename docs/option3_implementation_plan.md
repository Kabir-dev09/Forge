# Option 3: Lock-Free Snapshot Implementation Plan

## Goal
Eliminate `RwLock<ScreenBuffer>` to achieve absolute maximum terminal throughput and wait-free guarantees. The PTY thread will exclusively own the terminal state and publish lightweight snapshots to the renderer via atomic pointers (`arc_swap::ArcSwap`).

## Architecture Changes
1. **Ownership**: `Pane` will no longer hold `Arc<RwLock<ScreenBuffer>>`. Instead, the PTY thread's `PaneState` will uniquely own `ScreenBuffer`.
2. **Snapshotting**: We will introduce a lightweight `RenderSnapshot` struct containing only the data necessary for the renderer (the visible grid, cursor, selection state, scroll offsets).
3. **Atomic Swap (`arc-swap`)**: `Pane` will hold `Arc<arc_swap::ArcSwap<RenderSnapshot>>`. The PTY thread will periodically update this snapshot.
4. **Asynchronous UI Commands**: Mouse selection, scrolling, and theme updates triggered by the Wayland event loop will be sent to the PTY thread via the `PtyWorkerCommand` channel.

## Step-by-Step Implementation

### Phase 1: Define the Lock-Free Structures
- Add `arc-swap` dependency to `Cargo.toml`.
- Define `RenderSnapshot` in `forge-pty/src/lib.rs` or `forge-main/src/mux/mod.rs`. It must contain everything the renderer needs to draw a single frame (visible grid, cursor, dirty rows, scrollbar info).
- Add a helper method to `ScreenBuffer` to generate a `RenderSnapshot`.

### Phase 2: Update the PTY Worker (`io.rs`)
- Expand `PtyWorkerCommand` to include UI actions: `ScrollUp`, `ScrollDown`, `ScrollPageUp`, `ScrollPageDown`, `ScrollToTop`, `ScrollToBottom`, `UpdateSelection`, `ClearSelection`, `UpdateTheme`, `MarkAllDirty`, `MarkAllClean`.
- Add an `ArcSwap<RenderSnapshot>` to `PaneState` inside the worker.
- After processing PTY reads or executing a UI command, generate a new `RenderSnapshot` and atomically swap it into the `ArcSwap`.
- Notify the main thread via `loop_signal.wakeup()`.

### Phase 3: Update `Pane` and `MuxState` (`pane.rs`, `state.rs`)
- Replace `screen_buffer: Arc<RwLock<ScreenBuffer>>` with `snapshot: Arc<arc_swap::ArcSwap<RenderSnapshot>>` inside `Pane`.
- Replace direct `screen_buffer.write()` calls in `state.rs` with channel sends to `PaneIoRegistry`.

### Phase 4: Refactor Renderer (`event_loop.rs`)
- Remove all `screen_buffer.read()` logic during rendering.
- Read the latest state instantly using `pane.snapshot.load()`.
- Route Wayland scroll wheel and pointer selection events into `PaneIoRegistry::send_ui_command(pane_id, command)`.
- Replace `pane.screen_buffer.write().unwrap().mark_all_clean()` with a UI command to the PTY thread.

### Phase 5: Verification & Testing
- Run `cargo check` and `cargo test`.
- Verify terminal resizing doesn't panic on the snapshot grid sizes.
- Test extremely high-output commands (`cat large_file`) to verify the shell runs unblocked.
- Test scroll wheel and mouse selection to verify input latency is acceptable.

## Fallback & Risk Management
Since moving scrolling and selection to an asynchronous channel introduces a 1-frame input latency (which is standard in modern terminals), we will ensure the Wayland frame callbacks schedule redraws correctly upon UI interactions.
