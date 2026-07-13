# Tabs System Implementation Plan

## 1. Goal and Non-Goals

### Goals
The primary goal is to implement a robust, highly responsive, and resource-conscious tabs system for the Forge terminal emulator. The implementation must ensure complete isolation between tabs in terms of pane layout and active focus. Performance is critical: inactive tabs must have a near-zero resource footprint regarding the rendering pipeline.

### Non-Goals
This implementation strictly focuses on the core tab architecture. It does NOT include:
- Tab bar UI styling, animations, or complex visual decorations.
- Status bar redesigns.
- Theming or icon support for tabs.
- Custom detached windows per tab (tabs belong to a single OS window).

## 2. Research Summary

Advanced terminal emulators (like Ghostty, WezTerm, Alacritty muxes) and modern code editors (like VS Code) implement tabs using a strict decoupling of the workspace state from the rendering pipeline.

**Key principles drawn from research:**
1. **Lazy Rendering:** Inactive tabs never participate in the render loop. They do not trigger GPU uploads, frame scheduling, or cursor blink timers.
2. **Background Processing:** Background processes in inactive tabs must not block. PTY polling continues in the background, updating the `ScreenBuffer` and setting dirty flags, but these dirty flags do not trigger a frame request.
3. **Workspace Encapsulation:** Each tab acts as a standalone multiplexer (mux) tree. Split pane layouts, active pane tracking, and scrollback are completely encapsulated within the tab's state.
4. **Immediate Switch:** Switching tabs avoids complete re-initialization. Instead, it flags the newly active tab's buffer as fully dirty to force an immediate, single-frame redraw from the existing text buffer state.

## 3. Current Forge Architecture

Forge currently operates with a single workspace paradigm:
- **`MuxState` (`crates/forge-main/src/mux/state.rs`):** Manages a single layout tree (`LayoutNode`) and a flat map of `Pane`s. It tracks the `active_pane`.
- **`Pane` (`crates/forge-main/src/mux/pane.rs`):** Holds its `PaneId`, geometry (`PaneRect`), an `Arc<RwLock<ScreenBuffer>>`, and an `Option<Pty>`.
- **`AppData` (`crates/forge-main/src/event_loop.rs`):** Holds the global `MuxState`, Wayland connection state, input receivers, and the renderer.
- **Event Loop & Rendering (`crates/forge-main/src/event_loop.rs`):** The event loop checks if *any* pane in the `MuxState` has dirty rows to request a frame (`frame_wants_redraw`).
- **PTY Polling:** Handled by `calloop` via `PaneIoRegistry`. When a PTY has output, it updates the `ScreenBuffer` and marks rows dirty.

## 4. Proposed Tab Architecture

We will introduce a `TabManager` that sits between `AppData` and `MuxState`.

**Core Data Structures:**
```rust
pub struct TabId(pub u64);

pub struct Tab {
    pub id: TabId,
    pub mux: MuxState,
    // Future expansion: tab title, color, etc.
}

pub struct TabManager {
    pub tabs: Vec<Tab>,
    pub active_tab_index: usize,
    pub next_tab_id: u64,
}
```

**Architecture Changes:**
1. **`AppData` Update:** Replace `pub mux: MuxState` with `pub tab_manager: TabManager`.
2. **Active Tab Handling:** All keyboard inputs, mouse inputs, and render calls will be routed through `app_data.tab_manager.active_tab_mut()`.
3. **Global PTY Registry:** `PaneIoRegistry` will remain global in `AppData`. When `calloop` triggers a PTY read, it will look up the pane by iterating through all tabs (or via a global `PaneId -> TabId` mapping) to write to the correct `ScreenBuffer`.
4. **Renderer Caches:** The Vulkan renderer (`forge_renderer::Renderer`) caches glyphs in a font atlas. This atlas is agnostic to tabs and can be safely shared. The grid layout and vertex buffers are rebuilt per-frame based on the active tab's `grid_refs`.

## 5. Inactive Tab Resource Policy

To meet strict efficiency requirements, inactive tabs must adhere to the following rules:

* **PTY Reads:** `calloop` MUST continue polling inactive PTYs. Output is processed into the `ScreenBuffer` to prevent background processes from blocking on full buffers.
* **Dirty Marking:** PTY reads in inactive tabs will mark rows as dirty in their respective `ScreenBuffer`.
* **Rendering:** `frame_wants_redraw` in `event_loop.rs` MUST ONLY check `app_data.tab_manager.active_tab().mux` for dirty rows. Inactive tab dirty rows must **never** trigger a Wayland frame request.
* **Tessellation & GPU Uploads:** Skipped entirely. The `renderer.render_panes` function will only be passed the `MuxState` of the active tab.
* **Cursor Blinking & Scrollbars:** Visual animations are tied to the event loop's frame generator. Only the active tab's cursor and scrollbar state are calculated.
* **Window Resize:** When the OS window resizes, the active tab is resized immediately. Inactive tabs should be lazily resized when they become active, or batched silently in memory without triggering a redraw.

## 6. Keybind Plan

Basic keybinds will be routed through Forge's `config_registry.rs`. 
Professional defaults (following standard modern terminal practices):

* `NewTab`: `Ctrl + Shift + T`
* `CloseTab`: `Ctrl + Shift + W`
* `NextTab`: `Ctrl + PageDown` or `Ctrl + Tab`
* `PreviousTab`: `Ctrl + PageUp` or `Ctrl + Shift + Tab`
* `SwitchTab1` to `SwitchTab9`: `Alt + 1` through `Alt + 9`
* `MoveTabLeft`: `Ctrl + Shift + PageUp`
* `MoveTabRight`: `Ctrl + Shift + PageDown`

## 7. Implementation Phases

**Phase 1: Add Tab Data Structures**
* **Goal:** Introduce `Tab` and `TabManager` structs.
* **Files:** `crates/forge-main/src/mux/tab.rs` (new), `crates/forge-main/src/mux/mod.rs`.
* **Verification:** Compile check. No behavioral changes.

**Phase 2: Encapsulate MuxState into TabManager**
* **Goal:** Replace `AppData::mux` with `AppData::tab_manager` containing a single default tab.
* **Files:** `crates/forge-main/src/event_loop.rs`, `crates/forge-main/src/app_data.rs`.
* **Verification:** Single-pane and multi-pane (splits) behavior remains identical. 

**Phase 3: Global Pane Lookup for PTY Polling**
* **Goal:** Ensure `PaneIoRegistry` can find panes inside any tab when a background PTY fires.
* **Files:** `crates/forge-main/src/mux/io.rs`, `crates/forge-main/src/event_loop.rs` (`process_pane_pty_read`).
* **Verification:** Background commands (e.g., `sleep 2; echo done`) still update the buffer.

**Phase 4: Add Tab Creation and Closing**
* **Goal:** Implement logic to spawn a new tab (creating a new `MuxState` and root PTY) and close tabs.
* **Files:** `crates/forge-main/src/mux/tab.rs`, `crates/forge-main/src/event_loop.rs`.
* **Verification:** Memory footprint grows slightly on creation and shrinks on close. Prevent closing the last tab (or exit the app).

**Phase 5: Implement Tab Switching & Redraw Suppression**
* **Goal:** Allow changing the `active_tab_index`. Ensure switching forces a full redraw (`mark_all_dirty` on the new active tab). Ensure inactive tabs do NOT trigger `frame_wants_redraw`.
* **Files:** `crates/forge-main/src/event_loop.rs`.
* **Verification:** Run `htop` in an inactive tab. Verify CPU/GPU usage drops to near-zero as no frames are rendered.

**Phase 6: Input Routing & Resize Handling**
* **Goal:** Route keyboard/mouse events strictly to the active tab. Apply window resize events to the active tab, and lazily apply to inactive tabs upon activation.
* **Files:** `crates/forge-main/src/event_loop.rs`.
* **Verification:** Typing does not bleed into inactive tabs. Resizing the window and switching tabs shows the correct dimensions.

**Phase 7: Config & Keybinds Integration**
* **Goal:** Wire the configured keybinds to the TabManager actions.
* **Files:** `crates/forge-core/src/config_registry.rs`, `crates/forge-main/src/event_loop.rs`.

**Phase 8: Testing and Cleanup**
* **Goal:** Write unit tests for `TabManager` and verify resource cleanup (PTY descriptors dropped).

## 8. Performance and Resource Plan

**Metrics to track:**
1. **Idle CPU Usage:** Measure CPU usage with 10 tabs running `htop` in the background. Expectation: < 1% CPU rendering overhead, matching the baseline of raw PTY read parsing.
2. **Tab Switch Latency:** Measure time taken from keypress to frame dispatch. Must remain < 5ms.
3. **Memory Profile:** Run memory profiler (e.g. `valgrind` or `heaptrack`) when creating/closing 50 tabs to ensure `ScreenBuffer` and `Pty` drops correctly without leaks.

## 9. Edge Cases

* **Closing the active tab:** Focus must gracefully fallback to the previous tab (or adjacent tab).
* **Closing the last tab:** Should initiate application shutdown (`wayland_state.running = false`).
* **Background heavy output (e.g., `cat /dev/urandom`):** Inactive tabs will parse bytes and hit the `ScreenBuffer` scrollback limit. Since rendering is bypassed, this should only stress the CPU's string parsing, not the GPU.
* **Window resize while tabs exist:** The active tab resizes instantly. When switching to an inactive tab, its layout must be recalculated before its first frame is requested to avoid flickering or crashing due to mismatched grid bounds.
* **Crashed PTY in inactive tab:** The `calloop` event handler must catch EOF, mark the pane as dead, and clean it up without crashing the active view.

## 10. Tests

**Required Automated Tests:**
1. **Unit:** `tab_manager_creates_and_closes_tabs`
2. **Unit:** `closing_active_tab_shifts_focus`
3. **Unit:** `global_pane_lookup_resolves_across_tabs`
4. **Integration:** `inactive_tab_dirty_rows_do_not_trigger_redraw`
5. **Integration:** `switching_tabs_marks_new_tab_fully_dirty`
6. **Integration:** `resizing_applies_correctly_on_tab_switch`

## 11. Final Recommended Plan

Future agents should execute this plan strictly in the order of the **Implementation Phases**. 
Start by implementing `TabManager` as a pure data structure wrapper around the existing `MuxState` to guarantee zero regressions for the single-tab use case. 
Once wrapped, systematically decouple the event loop's redraw request (`has_dirty_rows`) from the global pool, restricting it solely to the `active_tab`. Only then should UI actions (keybinds) for creating and switching tabs be wired in. This ensures performance and stability are guaranteed at every step.
