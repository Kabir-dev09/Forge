# Native Multiplexing Pane Implementation Plan

## Goal

Add fully terminal-emulator-native pane splitting as the foundation for a future native multiplexer.

The first milestone is deliberately limited to:

- Vertical pane splitting.
- Horizontal pane splitting.
- Multiple independent terminal panes in one OS window.
- One PTY, parser, screen buffer, cursor, scrollback, and viewport per pane.
- Active pane tracking.
- Keyboard input routed to the active pane.
- Mouse focus selection by pane.
- Correct resize propagation to every pane.
- Rendering all visible panes through the existing renderer and shared font/glyph resources.

This milestone must not implement tabs, drag resizing, pane persistence, remote domains, detach/reattach, pane zoom, layout save/restore, or a complex status bar. The design below keeps those features possible later without forcing them into the first change.

## Current Codebase Analysis

### Repository structure

The current project is split into focused crates:

- `crates/forge-main`: startup, Wayland integration, event loop, input routing, resize handling, config reload handling, context menu integration.
- `crates/forge-pty`: PTY spawning/resizing, VTE parsing, terminal screen state, scrollback, selection, alt-screen state.
- `crates/forge-renderer`: Vulkan renderer, glyph atlas, grid tessellation, shaders, context-menu drawing, row-level render caching.
- `crates/forge-core`: shared data structures, colors, config registry, keybindings.
- `crates/forge-config`: Lua config extraction and default config examples.

The current architecture is single-pane at the top level. The first multiplexer milestone should therefore replace "one global terminal" with "one active mux containing one or more panes" while keeping single-pane behavior identical.

### Startup flow

Relevant files/functions:

- `crates/forge-main/src/main.rs`
  - Creates the Wayland connection/window.
  - Creates the Vulkan renderer.
  - Computes initial grid metrics with `event_loop::compute_grid_metrics`.
  - Spawns one `forge_pty::Pty` through `Pty::spawn`.
  - Creates one `forge_pty::ScreenBuffer`.
  - Creates one `forge_pty::VteProcessor`.
  - Passes the single PTY, screen buffer, and parser into `run_event_loop`.

This is the first major integration point. After the mux change, startup should create a `MuxState` with one initial pane instead of passing a single PTY/screen/parser directly into the event loop.

### Terminal state model

Relevant files/functions:

- `crates/forge-pty/src/screen_buffer.rs`
  - `ScreenBuffer`
  - `ScreenBuffer::new`
  - `ScreenBuffer::resize_reflow`
  - `ScreenBuffer::visible_row`
  - `ScreenBuffer::mark_all_dirty`
  - `ScreenBuffer::mark_all_clean`
  - `ScreenBuffer::has_dirty_rows`
  - `ScreenBuffer::take_pending_scroll`
  - selection methods
  - alt-screen methods

`ScreenBuffer` already owns the state that must become pane-local:

- Main grid.
- Scrollback.
- Cursor.
- Selection.
- Dirty rows.
- Current/default colors and attributes.
- ANSI palette.
- Alt-screen state.
- Mouse tracking state.
- Bracketed paste state.
- Pending scroll reuse data.
- Resize/reflow state.

This is a good foundation for panes. Each pane should own its own `ScreenBuffer`; no screen state should remain global except shared configuration.

### VTE parser model

Relevant files/functions:

- `crates/forge-pty/src/vte_parser.rs`
  - `VteProcessor`
  - `VteProcessor::process`

`VteProcessor` contains parser and charset state. It must be pane-local. Sharing one parser across panes would corrupt escape-sequence state when multiple PTYs produce output interleaved in time.

### PTY model

Relevant files/functions:

- `crates/forge-pty/src/pty.rs`
  - `Pty`
  - `Pty::spawn`
  - `Pty::read`
  - `Pty::write_all`
  - `Pty::resize`
  - `Pty::try_wait`

`Pty` already encapsulates the master FD, child PID, and terminal size. It is the correct low-level primitive for pane-local processes.

Important observations:

- `Pty::spawn` creates a nonblocking PTY master.
- `Pty::resize` uses the existing terminal-size update path and sends `SIGWINCH`.
- `Pty::write_all` currently retries on `EAGAIN` with a short sleep. That is acceptable for small keyboard/paste writes, but it is not ideal for a high-end multiplexer with many panes. The first milestone may keep it for minimal risk, but the plan should leave room for event-driven write readiness and per-pane output queues later.

### Current event loop

Relevant files/functions:

- `crates/forge-main/src/event_loop.rs`
  - `AppData`
  - `run_event_loop`
  - `compute_grid_metrics`
  - `pointer_motion_has_effect`
  - `pointer_layout_metrics`

`AppData` currently stores:

- One `pty`.
- One `Arc<RwLock<ScreenBuffer>>`.
- One key receiver.
- One paste receiver.
- One pointer receiver.
- One renderer.
- Global pointer/scrollbar/context-menu state.
- Global cursor blink state.
- Global cached grid metrics.

`run_event_loop` currently starts one background PTY reader thread for the single PTY. That thread reads bytes, processes them through one `VteProcessor`, mutates one `ScreenBuffer`, and wakes the calloop signal. This is the highest-risk area for native multiplexing because scaling this model by adding one reader thread per pane would work initially but would not be the most professional long-term design.

Recommended first-class design:

- Keep one central event loop.
- Register every pane PTY master FD with calloop.
- Read only the PTYs that are ready.
- Process PTY output for the corresponding pane.
- Coalesce redraw requests into one frame.

This matches the existing event-driven direction of the project and avoids unnecessary per-pane idle CPU cost.

If calloop integration proves too invasive during implementation, a temporary `PaneIoWorker` fallback can be used, but it should be treated as an intermediate step and not the target architecture.

### Current input/keybinding model

Relevant files/functions:

- `crates/forge-core/src/bindings.rs`
  - `KeyStroke`
  - `Action`
  - `KeyStroke::parse`
- `crates/forge-core/src/config_registry.rs`
  - default keybinding registration in `ForgeConfig::default`
- `crates/forge-config/src/default_config.lua`
  - documented keybinding examples
- `crates/forge-main/src/wayland/seat.rs`
  - keyboard event decoding
  - keybinding interception
  - pointer event forwarding

Current actions include copy, paste, fullscreen toggle, and zoom actions. Pane actions should be added here rather than hardcoded in the event loop.

The important architectural issue is that Wayland keyboard handling currently handles some actions directly in `WaylandState` and sends raw byte input through a channel. Split actions need to mutate `MuxState`, so the event loop must receive actions explicitly.

Recommended change:

```rust
enum InputEvent {
    Bytes(Vec<u8>),
    Action(forge_core::bindings::Action),
}
```

Then Wayland key handling can send either terminal bytes or an action to the event loop. Actions that need access to mux state, such as split commands, should be handled in `event_loop.rs`.

### Current rendering path

Relevant files/functions:

- `crates/forge-renderer/src/renderer.rs`
  - `Renderer`
  - `Renderer::render_grid`
  - `GlyphAtlas`
  - `GridTessellator`
- `crates/forge-renderer/src/grid_tessellator.rs`
  - `GridTessellator`
  - `GridTessellator::tessellate`
  - `RowTessellation`
  - context-menu tessellation

The renderer currently accepts one grid and one set of dirty rows. It owns shared GPU resources, a glyph atlas, and one `GridTessellator`.

Current optimizations include:

- Shared glyph atlas.
- Dynamic glyph insertion.
- Row-level dirty handling in the screen buffer.
- Row tessellation caches.
- Cursor/selection row invalidation.
- Scroll reuse support through `pending_scroll`.

For native panes, renderer resources must stay shared. The renderer should not create a font atlas, Vulkan pipeline, or vertex buffer per pane.

The main renderer change should be to accept a list of pane render inputs:

```rust
struct PaneRenderInput<'a> {
    pane_id: PaneId,
    rect: PaneRect,
    content_origin_px: (f32, f32),
    cell_width: f32,
    cell_height: f32,
    grid: &'a [&'a [Cell]],
    dirty_rows: &'a [bool],
    cursor: Option<(usize, usize)>,
    cursor_style: CursorStyle,
    cursor_visible: bool,
    selection: Option<SelectionRange>,
    scroll_event: Option<ScrollEvent>,
}
```

The existing `render_grid` path can remain as the single-pane compatibility wrapper during migration.

### Resize/layout behavior

Relevant files/functions:

- `crates/forge-main/src/event_loop.rs`
  - `compute_grid_metrics`
  - window resize branch in `run_event_loop`
- `crates/forge-pty/src/screen_buffer.rs`
  - `ScreenBuffer::resize_reflow`
- `crates/forge-pty/src/pty.rs`
  - `Pty::resize`

Current resize behavior computes a single terminal rows/columns pair from the whole window. With panes, resize becomes:

1. Compute the full content area for all panes.
2. Recursively assign pane pixel rectangles from the split tree.
3. Convert each pane rectangle to rows/columns using current cell metrics.
4. Call `resize_reflow` and `Pty::resize` for each pane whose rows/columns changed.
5. Mark affected panes dirty.

The recent Nushell table resizing issues make this area especially sensitive. The plan must keep all terminal row/column sizes synchronized with the PTY sizes. The renderer must not merely clip or visually scale output; the application inside each pane must receive the correct terminal dimensions.

### Selection, scrollback, cursor, and context menu

Selection, scrollback, alt-screen state, bracketed paste, cursor style, and mouse tracking are all currently stored in the single `ScreenBuffer`. With panes:

- Selection must be pane-local.
- Scrollback must be pane-local.
- Alt-screen must be pane-local.
- Mouse reporting must be sent only to the relevant pane.
- Right-click context menu actions should operate on the pane under the click or the active pane.
- Cursor blink should affect only the active pane for the first milestone.

## External Architecture Notes

This plan uses concepts from mature terminal multiplexers and terminal emulators, without copying implementation code.

- tmux models panes as separate pseudo terminals inside a window. Its manual describes windows split into rectangular panes, each pane being a separate PTY: https://man7.org/linux/man-pages/man1/tmux.1.html
- WezTerm exposes a mux model with panes, tabs, windows, domains, and split actions such as `SplitHorizontal`: https://wezterm.org/multiplexing.html and https://wezterm.org/config/lua/keyassignment/SplitHorizontal.html
- kitty supports tabs, windows, and multiple layouts, including arbitrary horizontal and vertical splits: https://sw.kovidgoyal.net/kitty/overview/
- The existing project already uses calloop; `calloop::generic::Generic` can wrap FD-backed types as event sources: https://docs.rs/calloop/latest/calloop/generic/struct.Generic.html

Key design lessons:

- A pane should be a real terminal with a real PTY, not a visual subdivision of one terminal.
- Layout should be separate from terminal state.
- Rendering should be shared and batched, not one renderer per pane.
- PTY IO should be readiness-driven.
- Split trees are a better initial fit than a fixed grid because they naturally represent nested splits.

## Recommended Architecture

### Module placement

For the first milestone, add a new module under `forge-main`:

```text
crates/forge-main/src/mux/
```

Recommended files:

```text
crates/forge-main/src/mux/mod.rs
crates/forge-main/src/mux/pane.rs
crates/forge-main/src/mux/layout.rs
crates/forge-main/src/mux/state.rs
crates/forge-main/src/mux/io.rs
```

Reasoning:

- The mux initially needs tight integration with the existing event loop, PTY, screen buffer, and config.
- Keeping it inside `forge-main` avoids prematurely creating a crate boundary that may need to change.
- The module should not depend on renderer internals. It should expose render inputs, not render directly.
- Once the API is stable, it can be extracted into a `forge-mux` crate.

### Core data structures

Use the following structure as the target design, adapted to the project's exact style during implementation:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal, // top/bottom
    Vertical,   // left/right
}

#[derive(Debug, Clone, Copy)]
pub struct PaneRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: usize,
    pub rows: usize,
}

pub enum LayoutNode {
    Leaf(PaneId),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

pub struct Pane {
    pub id: PaneId,
    pub pty: forge_pty::Pty,
    pub screen: forge_pty::ScreenBuffer,
    pub parser: forge_pty::VteProcessor,
    pub rect: PaneRect,
    pub grid_size: GridSize,
    pub lifecycle: PaneLifecycle,
    pub dirty_layout: bool,
}

pub struct MuxState {
    pub root: LayoutNode,
    pub panes: HashMap<PaneId, Pane>,
    pub active_pane: PaneId,
    pub next_pane_id: u64,
    pub layout_generation: u64,
}
```

Keep ownership simple:

- `MuxState` owns all panes.
- Each `Pane` owns its own PTY, `ScreenBuffer`, and `VteProcessor`.
- `LayoutNode` references panes by `PaneId`.
- Renderer receives borrowed pane render inputs for the current frame only.

Do not wrap each pane screen in `Arc<RwLock<_>>` unless the IO implementation truly requires cross-thread mutation. If PTY IO moves into the calloop event loop, panes can be mutated directly by the event loop, which is cleaner and less error-prone.

## Layout Algorithm

### Terminology

- Pane: one independent terminal instance with its own PTY and terminal state.
- Split node: an internal layout node that divides a rectangle between two children.
- Active pane: the pane receiving keyboard input.
- Pane rect: the pixel rectangle assigned to a pane.
- Pane grid size: the columns/rows derived from the pane rect and cell metrics.
- Split border: the visual separator between panes.

### Axis behavior

- `SplitAxis::Vertical` divides space left/right with a vertical border.
- `SplitAxis::Horizontal` divides space top/bottom with a horizontal border.

### Recursive layout steps

1. Start with the full terminal content area.
2. Recursively walk the `LayoutNode`.
3. For a leaf, assign the current rectangle to the referenced pane.
4. For a split:
   - Clamp ratio to a safe range, for example `0.10..=0.90`.
   - Subtract split border thickness from the available axis.
   - Divide the remaining size according to the ratio.
   - Snap child rectangles and border rectangles to integer physical pixels.
   - Recurse into each child.
5. Convert each pane rectangle to terminal rows/columns:
   - `cols = floor(rect.width / effective_cell_width)`
   - `rows = floor(rect.height / effective_cell_height)`
6. Reject or avoid layouts that would create invalid panes.
7. If a pane's rows/columns changed, call `ScreenBuffer::resize_reflow` and `Pty::resize`.
8. Mark resized panes dirty.

### Minimum pane constraints

Initial constants:

```text
MIN_PANE_COLS = 10
MIN_PANE_ROWS = 3
SPLIT_BORDER_PX = 1
```

The exact values can become config later. For the first milestone, hardcoded internal constants are acceptable if they are localized in `mux/layout.rs`.

A split command must be rejected if either resulting child would be below the minimum size. Rejection should be silent or logged at debug level; it must not corrupt layout state.

### Cell metrics and padding

The first milestone should preserve the existing global window padding behavior. Recommended approach:

- Continue using `compute_grid_metrics` for the full window to get effective cell metrics and outer padding.
- Treat the mux content area as the area inside outer padding.
- Use the same effective cell width/height for every pane.
- Do not independently stretch cells per pane.
- Keep leftover pixels inside each pane as pane-local trailing space or distribute them only through rectangle snapping, not through per-pane font scaling.

This avoids reintroducing text blur and Nushell table misalignment. Pane content should align to integer cell positions and never render text at fractional cell origins.

## PTY and Event Loop Strategy

### Target design

Use one central event loop for all panes.

For each pane:

- Register the pane's PTY master FD with calloop.
- Read from the PTY only when the FD is readable.
- Process bytes through that pane's `VteProcessor`.
- Mutate that pane's `ScreenBuffer`.
- Write parser responses back to the same pane's PTY.
- Mark only that pane dirty.
- Wake or schedule one frame for all accumulated changes.

This avoids:

- One render loop per pane.
- One busy polling loop per pane.
- Unnecessary CPU usage while idle.
- Global redraws for unrelated pane output.

### Suggested calloop integration

The existing event loop already uses calloop. Investigate replacing the current single PTY reader thread with a calloop FD source.

Potential source:

```rust
calloop::generic::Generic
```

This can wrap FD-backed types and provide readiness events. The implementation must respect Rust ownership rules. If `Pty` cannot be directly moved into the event source while still needed in `MuxState`, use a small stable wrapper that registers a duplicated FD for readiness and keeps the owning `Pty` in the pane.

### Practical first implementation path

1. Introduce `MuxState` while still using the existing single PTY path.
2. Move the single PTY/screen/parser into the initial pane.
3. Convert the current PTY reader thread into a `PaneIo` abstraction.
4. Then change `PaneIo` to handle multiple pane FDs.

If calloop FD registration is straightforward, implement it immediately. If it is risky, use a temporary per-pane `PaneIoWorker` with these constraints:

- One worker owns/readers only a pane FD.
- Worker sends `PaneOutput { pane_id, bytes }` to the event loop.
- Event loop performs VTE parsing and `ScreenBuffer` mutation.
- Worker does not render.
- Worker does not mutate shared screen state.
- Worker exits cleanly when the pane closes.

This fallback is less ideal than calloop FD readiness, but it is safer than sharing `ScreenBuffer` locks across many threads.

### PTY resize

When a pane rect changes:

1. Calculate new rows/columns from the pane rect.
2. If unchanged, do nothing.
3. If changed:
   - call `screen.resize_reflow(new_cols, new_rows)`.
   - call `pty.resize(new_cols, new_rows, pixel_width, pixel_height)`.
   - mark the pane dirty.

Order matters. The screen buffer and PTY must stay synchronized so applications like Nushell, `nvim`, `btop`, and `tmux` do not render for one size while the emulator displays another.

## Rendering and Damage Strategy

### Renderer architecture

Render all visible panes in one unified renderer pass.

Do not create:

- one Vulkan renderer per pane,
- one glyph atlas per pane,
- one font rasterizer per pane,
- one swapchain per pane.

Shared resources:

- `Renderer`
- Vulkan device/pipeline/swapchain
- `GlyphAtlas`
- dynamic glyph cache
- font metrics
- frame vertex buffers

Pane-local render state:

- Dirty rows.
- Cursor position/style.
- Selection.
- Scroll event.
- Tessellation row cache, if per-pane caching is introduced.

### Renderer API migration

Recommended staged migration:

1. Keep `Renderer::render_grid` as a compatibility wrapper.
2. Add a new `Renderer::render_panes` API.
3. Implement `render_grid` by creating one `PaneRenderInput` and forwarding to `render_panes`.
4. Refactor `GridTessellator` so row caches are keyed by `PaneId` or split into a `PaneTessellationCache`.

This avoids breaking single-pane rendering while making multipane rendering first-class.

### Pane clipping

Each pane must render only inside its assigned rectangle.

The renderer should:

- Clear/draw each pane background within its pane rect.
- Clip foreground glyphs to pane cell bounds.
- Avoid drawing cells beyond `pane.grid_size.cols`.
- Avoid drawing rows beyond `pane.grid_size.rows`.
- Render split borders after pane backgrounds.
- Render overlay UI such as the context menu after panes and borders.

This is especially important for terminal applications that produce row content wider than the current visible columns. The terminal state can preserve overflow information for correct reflow, but the renderer must not display beyond the pane's current visible grid.

### Dirty strategy

Pane-aware dirty state should be layered on top of the existing dirty rows:

- `ScreenBuffer::dirty_rows` remains the source of row-level text changes.
- `Pane::dirty_layout` marks a pane dirty due to rect/grid changes.
- `MuxState::layout_generation` increments when layout changes.
- Active-pane changes dirty the old and new pane borders/cursor rows.
- Context-menu changes dirty only the affected overlay region.

Initial acceptable behavior:

- On split or window resize, force a full redraw of the window for correctness.
- On normal PTY output, redraw only if at least one pane has dirty rows.
- On cursor blink, dirty only the active pane cursor row.

Future improvement:

- Convert dirty rows to pane dirty rectangles.
- Use Wayland damage regions per pane rect.
- Reuse scrolled vertex ranges per pane independently.

### Split borders

First milestone border rules:

- Border thickness: 1 physical pixel.
- Rendered by the renderer as simple quads.
- Color from theme or a localized default, with active-pane border optional.
- Borders are not terminal cells and should not affect PTY rows/columns except by consuming pixel space in layout.

## Input and Focus Strategy

### Keyboard input

Keyboard bytes go only to `MuxState::active_pane`.

Recommended flow:

1. Wayland keyboard handling converts an event to either:
   - terminal bytes, or
   - a configured `Action`.
2. Event loop receives `InputEvent`.
3. For `InputEvent::Bytes`, write bytes to the active pane's PTY.
4. For `InputEvent::Action`, dispatch to a mux/window action handler.

Action handling:

- `Copy`: copy selection from active pane, or clicked pane if the context menu opened on a different pane.
- `Paste`: paste into active pane.
- `ZoomIn/ZoomOut/ZoomReset`: update global font metrics, then relayout all panes.
- `SplitVertical`: split active pane left/right.
- `SplitHorizontal`: split active pane top/bottom.

### Repeating keys

The current repeating key state stores terminal bytes. With panes:

- Store the target pane ID alongside repeating bytes.
- Cancel repeat when active pane changes.
- Do not repeat actions such as split commands.

This prevents focus changes from redirecting a held key unexpectedly.

### Mouse input

Mouse handling must become pane-aware.

Recommended rules for milestone 1:

- Left click inside a pane focuses that pane.
- Terminal selection starts inside the focused/clicked pane.
- Selection does not cross pane boundaries.
- Right-click opens the context menu for the pane under the pointer.
- Mouse reporting events are sent only to the pane under the pointer, or to the pane that captured the drag.
- Scroll wheel scrolls the pane under the pointer.
- Clicks on split borders do nothing in milestone 1.

Coordinate translation:

```text
global pointer x/y
-> hit-test pane rect
-> pane-local x/y
-> col/row using cell metrics and pane content origin
-> terminal mouse protocol coordinates
```

### Context menu

The current context menu should stay one global overlay, but actions must include the target pane:

```rust
struct ContextMenuState {
    target_pane: Option<PaneId>,
    ...
}
```

Copy and paste should operate on `target_pane.unwrap_or(active_pane)`.

## First Split Commands and Keybindings

### New actions

Add actions to `crates/forge-core/src/bindings.rs`:

```rust
SplitVertical
SplitHorizontal
```

Recommended default keybinds:

```text
Ctrl+Shift+Backslash -> SplitVertical
Ctrl+Shift+Minus     -> SplitHorizontal
```

Before implementation, verify how `KeyStroke::parse` handles backslash. If it does not support a readable name, add a `backslash` alias instead of relying on Lua string escaping.

### Split command behavior

`split_active_pane(axis)` should:

1. Find the active pane.
2. Validate that the active pane can be split into two valid minimum pane sizes.
3. Spawn a new pane using the configured shell.
4. Replace the active leaf in the layout tree with a split node.
5. Use ratio `0.5` for the first milestone.
6. Recalculate layout.
7. Resize both affected panes.
8. Mark affected panes and split borders dirty.
9. Focus the new pane.
10. Schedule one redraw.

If spawning the new shell fails:

- Do not mutate the layout.
- Keep focus on the original pane.
- Log an error in the existing project style.

## Step-by-Step Implementation Phases

### Phase 1: Preparation and tests

Objective:

Add pure data structures and tests without changing runtime behavior.

Likely files:

- `crates/forge-main/src/mux/mod.rs`
- `crates/forge-main/src/mux/layout.rs`
- `crates/forge-main/src/mux/pane.rs`
- `crates/forge-main/src/mux/state.rs`
- `crates/forge-main/src/main.rs` only to expose the module

Steps:

1. Add `PaneId`, `SplitAxis`, `PaneRect`, `GridSize`, `LayoutNode`.
2. Add pure layout calculation functions.
3. Add unit tests for simple, vertical, horizontal, and nested layouts.
4. Add minimum pane validation tests.
5. Do not yet connect to `run_event_loop`.

Success criteria:

- `cargo test -p forge-main mux` passes.
- No runtime behavior changes.

### Phase 2: Introduce `MuxState` with one pane

Objective:

Replace global single terminal fields in `AppData` with a mux containing one pane, while preserving behavior.

Likely files:

- `crates/forge-main/src/main.rs`
- `crates/forge-main/src/event_loop.rs`
- `crates/forge-main/src/mux/state.rs`
- `crates/forge-main/src/mux/pane.rs`

Steps:

1. Create the initial `Pane` from the existing `Pty`, `ScreenBuffer`, and `VteProcessor`.
2. Store it in `MuxState`.
3. Change helper functions like `selected_text`, cursor dirty marking, paste handling, and scroll handling to use `active_pane`.
4. Keep the existing single-pane render call through a compatibility adapter.
5. Build and manually verify single-pane behavior.

Success criteria:

- Single-pane launch looks and behaves exactly as before.
- Copy/paste still works.
- Resize still works.
- No multipane UI is visible yet.

### Phase 3: Pane layout engine integration

Objective:

Compute pane rectangles and rows/columns from the split tree.

Likely files:

- `crates/forge-main/src/event_loop.rs`
- `crates/forge-main/src/mux/layout.rs`
- `crates/forge-main/src/mux/state.rs`

Steps:

1. Add `MuxState::relayout(content_rect, cell_metrics)`.
2. Apply the current full-window content rect to the single root pane.
3. Replace direct global rows/cols calculations with active/full mux layout calculations.
4. On window resize or font metrics changes, relayout all panes.
5. Resize only panes whose grid size changed.
6. Mark affected panes dirty.

Success criteria:

- Single-pane resize remains correct.
- Nushell `ls` table resize behavior is unchanged.
- `nvim`, `btop`, and shell output still resize correctly.

### Phase 4: Event-driven pane PTY IO

Objective:

Handle multiple pane PTYs without unnecessary idle CPU usage.

Likely files:

- `crates/forge-main/src/event_loop.rs`
- `crates/forge-main/src/mux/io.rs`
- `crates/forge-main/src/mux/state.rs`
- `crates/forge-pty/src/pty.rs` only if a small FD/accessor is needed

Steps:

1. Add a `PaneIoRegistry` abstraction.
2. Register each pane PTY master FD with calloop if practical.
3. On readiness, read from the corresponding pane until `WouldBlock`.
4. Process output with that pane's `VteProcessor`.
5. Write parser responses back to that pane.
6. Mark that pane dirty and schedule one frame.
7. Remove the old single-PTY reader thread.
8. Add cleanup/unregister handling for pane exit/close.

Success criteria:

- No PTY output is lost.
- Multiple panes can produce output independently.
- Idle CPU remains near zero.
- No one-thread-per-pane design unless explicitly accepted as a temporary fallback.

### Phase 5: Multi-pane rendering

Objective:

Render all visible panes in one renderer pass with shared resources.

Likely files:

- `crates/forge-renderer/src/renderer.rs`
- `crates/forge-renderer/src/grid_tessellator.rs`
- `crates/forge-main/src/event_loop.rs`
- `crates/forge-main/src/mux/state.rs`

Steps:

1. Define a render input type for panes.
2. Add `Renderer::render_panes`.
3. Keep `Renderer::render_grid` as a wrapper for compatibility.
4. Add pane-origin support to tessellation.
5. Add pane clipping to prevent overflow outside pane rects.
6. Add split border rendering.
7. Ensure context menu overlays render after pane content.
8. Mark all panes dirty on renderer swapchain recreation.

Success criteria:

- One pane renders identically to before.
- Multiple panes do not overlap.
- Text stays crisp.
- Box drawing and Nushell tables stay aligned.
- No stale pixels after split or resize.

### Phase 6: Input and focus routing

Objective:

Route all input to the correct pane.

Likely files:

- `crates/forge-main/src/event_loop.rs`
- `crates/forge-main/src/wayland/seat.rs`
- `crates/forge-main/src/wayland/connection.rs`
- `crates/forge-main/src/context_menu.rs`
- `crates/forge-main/src/mux/state.rs`

Steps:

1. Replace raw key byte channel with `InputEvent` or an equivalent enum.
2. Dispatch keyboard bytes to the active pane.
3. Dispatch paste to the active pane.
4. Hit-test mouse coordinates against pane rects.
5. Focus panes on click.
6. Translate mouse coordinates to pane-local rows/columns.
7. Keep selection pane-local.
8. Route context-menu copy/paste to the target pane.

Success criteria:

- Typing affects only the focused pane.
- Paste affects only the focused pane.
- Click focuses a pane.
- Mouse selection does not cross pane boundaries.
- Mouse reporting remains correct in fullscreen applications.

### Phase 7: Split actions and keybindings

Objective:

Expose vertical and horizontal splits through the existing action/keybinding system.

Likely files:

- `crates/forge-core/src/bindings.rs`
- `crates/forge-core/src/config_registry.rs`
- `crates/forge-config/src/default_config.lua`
- `crates/forge-main/src/event_loop.rs`
- `crates/forge-main/src/mux/state.rs`

Steps:

1. Add `Action::SplitVertical` and `Action::SplitHorizontal`.
2. Add parser aliases for needed key names if missing.
3. Add default keybindings, respecting existing `disable_default_keybindings`.
4. Document examples in `default_config.lua`.
5. Dispatch split actions from the event loop.
6. Implement `MuxState::split_active_pane`.
7. Focus the new pane after split.

Success criteria:

- Configurable split keybindings work.
- Defaults work if enabled.
- Disabling default keybindings still disables them.
- Split failures do not corrupt state.

### Phase 8: Pane lifecycle cleanup

Objective:

Make pane creation and cleanup robust enough for future closing support.

Likely files:

- `crates/forge-main/src/mux/state.rs`
- `crates/forge-main/src/mux/io.rs`
- `crates/forge-pty/src/pty.rs`
- `crates/forge-main/src/event_loop.rs`

Steps:

1. Track pane child process state.
2. Detect EOF/EIO from PTY reads.
3. Mark pane exited without crashing.
4. Ensure PTY event sources are unregistered when panes are removed later.
5. Ensure all owned FDs are dropped exactly once.
6. Add tests for layout tree replacement/removal helpers even if close is not exposed yet.

Success criteria:

- Exited panes do not crash the event loop.
- Resource ownership is clear.
- Future pane close can be implemented without rewriting the mux.

### Phase 9: Verification and stabilization

Objective:

Prove correctness before moving to advanced multiplexer features.

Commands:

```text
cargo fmt
cargo check
cargo test
cargo test -p forge-pty
cargo test -p forge-renderer
cargo test -p forge-main
```

Manual verification:

- Single pane launch.
- Vertical split.
- Horizontal split.
- Nested splits.
- Typing into focused pane only.
- Paste into focused pane only.
- `echo $COLUMNS $LINES` in every pane.
- `stty size` in every pane.
- Window resize with multiple panes.
- Nushell `ls` in every pane.
- `nvim` in a pane.
- `vim` in a pane.
- `btop` in a pane.
- `htop` in a pane.
- `tmux` inside a pane.
- Scrollback in each pane.
- Alt-screen enter/exit in each pane.
- Right-click menu in each pane.
- Idle CPU before/after.

## First Milestone Acceptance Criteria

The milestone is complete only when:

- A single pane behaves exactly as before.
- `Ctrl+Shift+Backslash` can split the active pane vertically.
- `Ctrl+Shift+Minus` can split the active pane horizontally.
- New panes spawn independent shells.
- Each pane owns an independent PTY and terminal state.
- Pane rows/columns are correct after split and resize.
- Input goes only to the active pane.
- Clicking a pane focuses it.
- Rendering is clipped to pane bounds.
- Split borders render without stale pixels.
- Dirty rendering remains event-driven.
- Idle CPU does not meaningfully increase with multiple idle panes.
- Existing fixes for text clarity, resizing, context menu, blur/transparency, and Nushell table rendering are not regressed.

## Performance Strategy

### Minimal idle CPU usage

The design avoids idle CPU cost by:

- Keeping one central event loop.
- Reading PTYs only on readiness events.
- Rendering only when panes are dirty.
- Recalculating layout only on split, close, resize, font metric change, or future ratio changes.
- Keeping cursor blink limited to the active pane for the first milestone.

### Avoiding unnecessary redraws

- PTY output in Pane A marks only Pane A dirty.
- Cursor blink dirties only active pane cursor rows.
- Focus change dirties old/new active pane borders and cursor rows.
- Split/resize can force a full redraw initially for correctness, then be optimized later.
- No pane should schedule frames while idle.

### Why split tree instead of naive grid

A split tree directly represents user intent:

- Repeated nested splits are natural.
- Ratios belong to split nodes.
- Pane resize later can modify one split node without recalculating a global grid.
- Arbitrary layouts are easier than with a fixed row/column matrix.
- It matches how terminal split layouts are commonly modeled conceptually.

### Shared rendering resources

The renderer remains global. Panes share:

- Glyph atlas.
- Font rasterizers.
- Vulkan pipelines.
- Vertex upload buffers.
- Shaders.

Only lightweight pane render descriptors and dirty state are pane-local.

### CPU spike reduction during resize/split

- Coalesce all pane resize work into one relayout pass.
- Resize only panes whose grid size changes.
- Mark dirty once after the layout pass.
- Submit one frame after the batch.
- Avoid per-pane renderer recreation.

## Risk Analysis and Mitigations

### PTY lifecycle bugs

Risk:

- Leaked FDs, orphaned child processes, double-close, stale calloop sources.

Mitigation:

- Make `Pane` the single owner of `Pty`.
- Centralize registration/unregistration in `PaneIoRegistry`.
- Add explicit pane lifecycle states.
- Test pane spawn failure and PTY EOF paths.

### Resize desynchronization

Risk:

- The screen buffer, PTY size, and renderer disagree about rows/columns, causing broken Nushell tables, `nvim`, or `btop`.

Mitigation:

- Use one relayout function as the only source of pane grid sizes.
- Call `ScreenBuffer::resize_reflow` and `Pty::resize` together.
- Render using the same `GridSize`.
- Add manual `stty size` verification per pane.

### Terminal state corruption

Risk:

- Sharing one VTE parser or screen buffer across panes corrupts state.

Mitigation:

- Each pane owns its own `VteProcessor` and `ScreenBuffer`.
- Do not process output from multiple PTYs through shared parser state.

### Stale render damage

Risk:

- Old pane pixels remain after split/resize.

Mitigation:

- Force full redraw on layout generation changes in milestone 1.
- Mark all panes dirty after swapchain recreation or font metric changes.
- Only optimize damage regions after correctness is stable.

### Focus/input routing bugs

Risk:

- Input goes to the wrong pane.

Mitigation:

- Route keyboard input only through `MuxState::active_pane`.
- Add tests for focus changes.
- Store target pane for mouse drags/context menus.
- Cancel repeating keys on focus change.

### Mouse selection conflicts

Risk:

- Selection crosses pane boundaries or conflicts with mouse reporting.

Mitigation:

- Keep selection pane-local.
- Capture the pane at mouse-down for selection drags.
- Translate all mouse coordinates through pane hit-testing.

### Alternate-screen edge cases

Risk:

- Fullscreen apps in one pane affect scrollback/cursor state in another pane.

Mitigation:

- Alt-screen state already lives in `ScreenBuffer`; keeping one screen buffer per pane isolates it.
- Verify `nvim`, `btop`, and `tmux` in different panes simultaneously.

### Excessive redraws

Risk:

- A naive implementation redraws all panes for every PTY byte or mouse move.

Mitigation:

- Preserve dirty-row checks.
- Coalesce PTY reads.
- Use the existing pointer motion filtering model.
- Never add per-pane render loops.

### Nested split complexity

Risk:

- Recursive layout becomes hard to reason about as features are added.

Mitigation:

- Keep layout pure and unit tested.
- Store only ratio/axis/children in split nodes.
- Keep pane state outside the tree.

## Testing Checklist

### Automated tests

- Layout with one pane fills content rect.
- Vertical split produces left/right valid child rects.
- Horizontal split produces top/bottom valid child rects.
- Nested split rects do not overlap.
- Split border thickness is accounted for.
- Minimum size rejection works.
- Layout is deterministic across repeated recalculation.
- Pane IDs remain stable after splitting.
- Replacing a leaf in the layout tree works.
- Invalid pane IDs are rejected or handled safely.
- Keybinding parsing supports split shortcuts.

### Manual tests

- Single-pane behavior unchanged.
- Split vertically.
- Split horizontally.
- Create nested splits.
- Type in each pane.
- Click to focus panes.
- Paste into focused pane.
- Run `echo $COLUMNS $LINES` in every pane.
- Run `stty size` in every pane.
- Run Nushell `ls` before and after resizing.
- Run repeated `ls`.
- Run `clear`.
- Run `nvim`.
- Run `vim`.
- Run `btop`.
- Run `htop`.
- Run `tmux`.
- Test scrollback in each pane.
- Test right-click menu in each pane.
- Test window resizing repeatedly.
- Test font zoom with panes.
- Test compositor blur/transparency still behaves as before.
- Measure idle CPU with one pane and several panes.

## Future Expansion Plan

After the first milestone is stable, future features should build on the same architecture:

1. Pane focus navigation by direction.
2. Pane close.
3. Pane resize commands.
4. Mouse drag to resize split borders.
5. Pane zoom.
6. Tabs, with each tab owning a `MuxState` or a root layout tree.
7. Session save/restore of layout tree and cwd.
8. Persistent local mux server.
9. Remote/session domains.
10. Detach/reattach.
11. Active pane title/status metadata.
12. Pane-aware command palette or context menu items.

Do not start these until the first split milestone is stable and verified.

## Final Recommended Implementation Order

1. Add pure mux layout/data model and unit tests.
2. Wrap the existing single terminal in `MuxState` with no behavior change.
3. Integrate pane relayout with existing grid metrics and resize logic.
4. Convert PTY IO to a pane-aware event-driven abstraction.
5. Add renderer support for multiple pane render inputs and split borders.
6. Add pane-aware input routing and mouse hit-testing.
7. Add split actions and configurable keybindings.
8. Add lifecycle cleanup for pane spawn/exit failures.
9. Run full automated and manual verification.

## Stop Point

This document is a plan only. No pane implementation should begin until this plan is reviewed and approved.
