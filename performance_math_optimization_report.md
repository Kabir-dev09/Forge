# Forge Performance & Math Optimization Report

> Deep analysis of CPU/GPU workload, redundant computation, and optimization opportunities.
> Based on a full read of all source files in `crates/forge-renderer`, `crates/forge-pty`, `crates/forge-main`, and `crates/forge-core`.

---

## 1. Architecture Overview

Forge is a Vulkan-based Wayland terminal emulator. The rendering pipeline is:

1. **PTY Thread** → reads raw bytes → `vte` parser → `ScreenBuffer` mutations (background thread)
2. **Wayland Event Loop** (`event_loop.rs`) → polls Wayland events, channels, and `ScreenBuffer`
3. **Tessellator** (`grid_tessellator.rs`) → CPU: iterates grid cells, emits `GlyphVertex` quads
4. **Renderer** (`renderer.rs`) → uploads vertices to Vulkan HOST_COHERENT buffer, records a single `vkCmdDraw`, submits
5. **Fragment Shader** (`grid.frag.glsl`) → draws background blocks, atlas glyphs, procedural shapes (box-drawing, braille, Powerline, scrollbar)

Positive aspects already in place:
- Dirty-row tracking avoids re-tessellating clean rows
- Scroll reuse copies & translates existing row vertices instead of full retessellation
- Partial vertex upload via generation counters (only changed rows are DMA'd to the GPU)
- `HOST_COHERENT` mapped vertex buffer (no explicit flush needed)
- Background PTY parsing thread keeps the event loop free
- Alt-buffer bypass skips scrollbar logic entirely
- Motion coalescing drains the pointer channel before processing

---

## 2. Expensive CPU Calculations

### 2.1 sRGB Linearization Per Cell Per Frame
**File:** `crates/forge-core/src/color.rs`, `Color::to_srgb_linear()` (line 41)  
**Called from:** `grid_tessellator.rs`, `color_to_f32` closure, line 318–321

Every dirty cell invokes `to_srgb_linear()` twice — once for fg, once for bg. The function uses `f32::powf(2.4)`, which is a **transcendental** operation. On a 200-column × 50-row terminal with one fully dirty screen (e.g., during `cat` of a large file), this is 20,000 `powf(2.4)` calls per frame.

**Current code:**
```rust
let linearize = |c: u8| -> f32 {
    let f = c as f32 / 255.0;
    if f <= 0.04045 { f / 12.92 }
    else { ((f + 0.055) / 1.055).powf(2.4) }
    .clamp(0.0, 1.0)
};
```

**Problem:** Input is a `u8` — only 256 possible values. Every call recomputes the same powf. No lookup table is used.

**Fix:** Precompute a `[f32; 256]` lookup table at startup. The linearization for any byte value is then an array index — O(1) with no floating-point division.

**Benefit:** Eliminates all `powf` calls from the hot render path.  
**Impact:** HIGH (fast typing, scrolling, cat output — any frame with dirty cells)  
**Difficulty:** Easy  
**Risk:** None — the table is constant math. Validate against existing color unit test.

---

### 2.2 `color_to_f32` Closure Evaluated Unconditionally Per Cell
**File:** `grid_tessellator.rs`, lines 452–453 inside the main cell loop

```rust
let mut fg = color_to_f32(cell.fg);
let mut bg = color_to_f32(cell.bg);
```

This runs for every non-null cell, even if the cell's color is identical to its neighbor. No inter-cell color caching exists. For a terminal full of text with the same fg/bg, this is N redundant conversions per frame.

**Fix:** Track `last_fg`/`last_bg` `Color` values and skip conversion if the cell's raw color matches the previous one. A simple 4-byte struct compare.

**Impact:** MEDIUM  
**Difficulty:** Easy  
**Risk:** None.

---

### 2.3 Selection Range Normalization Inside Inner Cell Loop
**File:** `grid_tessellator.rs`, lines 458–476

```rust
let (s_r, s_c, e_r, e_c) = if sel.start_row < sel.end_row
    || (sel.start_row == sel.end_row && sel.start_col <= sel.end_col) {
    (sel.start_row, sel.start_col, sel.end_row, sel.end_col)
} else {
    (sel.end_row, sel.end_col, sel.start_row, sel.start_col)
};
```

This normalization is computed for **every cell** in the grid when a selection is active — `cols × rows` times per frame.

**Fix:** Normalize the selection once before the outer row loop and store it in a local variable.

**Impact:** MEDIUM (only during selection drag)  
**Difficulty:** Easy  
**Risk:** None.

---

### 2.4 `text_origin` Recomputes Frame-Level Constants Per Glyph
**File:** `grid_tessellator.rs`, lines 373–377

```rust
let text_origin = |px_x: f32, px_y: f32| -> [f32; 2] {
    let inset_x = ((cell_w - native_cell_w).max(0.0) * 0.5).floor();
    let inset_y = ((cell_h - native_cell_h).max(0.0) * 0.5).floor();
    [(px_x + inset_x).round(), (px_y + inset_y).round()]
};
```

`inset_x` and `inset_y` depend only on `cell_w`, `native_cell_w`, `cell_h`, `native_cell_h` — all frame-level constants. Yet they are recomputed inside the closure for each glyph emitted.

**Fix:** Compute `inset_x` and `inset_y` once before the main loop.

**Impact:** LOW-MEDIUM (all glyph-bearing cells)  
**Difficulty:** Easy  
**Risk:** None.

---

### 2.5 Underline and Strikethrough Redundantly Recompute `origin_y`
**File:** `grid_tessellator.rs`, lines 722–740

`text_origin(px_x, px_y)` is called again for underline/strikethrough, recomputing `inset_x`/`inset_y`. Also, `origin_y` is independently computed in `push_atlas_glyph` (line 389) and again in the underline/strikethrough path (lines 722, 735).

**Fix:** Compute `origin_y` once per cell and reuse it across glyph, underline, and strikethrough paths.

**Impact:** LOW  
**Difficulty:** Easy  
**Risk:** None.

---

### 2.6 `compute_grid_metrics` Called Multiple Times Per Frame Iteration
**File:** `crates/forge-main/src/event_loop.rs`

`compute_grid_metrics` is called at:
- Line 214 (`pointer_layout_metrics`, on every pointer motion/drag event)
- Line 570 (font size change path)
- Line 663 (window resize path)
- Line 1248 (every render frame)

Metrics depend only on window size and cell size — both rarely change. During a mouse drag, the function runs on every motion event.

**Fix:** Cache `GridMetrics` in `AppData`. Invalidate only when `last_window_size` or `cell_width`/`cell_height` changes.

**Impact:** MEDIUM (during active mouse drag)  
**Difficulty:** Easy  
**Risk:** Low. Must invalidate on font/resize events.

---

### 2.7 sRGB Conversion of Theme Colors Every Rendered Frame
**File:** `event_loop.rs`, lines 1134–1149

```rust
let bg_color = app_data.config.theme.background.to_srgb_linear();      // powf!
let cursor_color = app_data.config.theme.cursor_color.to_srgb_linear(); // powf!
let selection_bg_color = app_data.config.theme.selection_bg.to_srgb_linear(); // powf!
```

Three `powf(2.4)` calls per rendered frame, even though theme colors only change on config reload.

**Fix:** Cache the linearized `[f32; 4]` arrays in `AppData`. Invalidate only in the config update path (line 619).

**Impact:** LOW per frame but continuous at 60+ fps  
**Difficulty:** Easy  
**Risk:** None if cache is invalidated on config change.

---

### 2.8 `dirty_rows.clone()` on Every Render Frame
**File:** `event_loop.rs`, line 1270

```rust
renderer.render_grid(&grid_refs, &sb.dirty_rows.clone(), ...)
```

Clones the entire dirty rows vector on every frame while holding the `RwLock` read guard. The `RwLock` is held for the duration, so a shared reference is safe.

**Fix:** Pass `&sb.dirty_rows` directly.

**Impact:** LOW (removes 50–200 byte allocation + copy per frame)  
**Difficulty:** Trivial  
**Risk:** None.

---

### 2.9 `grid_refs` Vec Allocation Per Frame
**File:** `event_loop.rs`, line 1152

```rust
let grid_refs: Vec<&[forge_core::cell::Cell]> = (0..sb.rows()).map(|i| sb.visible_row(i)).collect();
```

Allocates a new `Vec` of slice references on every frame.

**Fix:** Pre-allocate in `AppData` and reuse (clear + repopulate each frame).

**Impact:** LOW  
**Difficulty:** Easy  
**Risk:** Lifetime care required.

---

### 2.10 Braille Pattern `fwidth()` Per Fragment
**File:** `grid.frag.glsl`, lines 111–112

```glsl
float w = 1.0 / fwidth(local.x);
float h = 1.0 / fwidth(local.y);
```

`fwidth()` is a derivative instruction (`dFdx + dFdy`), requiring two neighboring fragment evaluations. It is used only to recover physical pixel dimensions — information already available in `pc.cell_size`.

**Fix:** Replace with `pc.cell_size.x` and `pc.cell_size.y`, eliminating derivative instructions.

**Impact:** MEDIUM for braille-heavy content (btop uses braille bars)  
**Difficulty:** Easy  
**Risk:** None — `pc.cell_size` already carries this.

---

### 2.11 Box-Drawing Decoded in Fragment Shader (Divergent Branching)
**File:** `grid.frag.glsl`, lines 28–108

Box-drawing glyphs pack geometry data into `v_tex_coord.x` and decode it in the fragment shader using 16+ conditional branches. Every pixel inside a box-drawing cell runs this decoding.

The CPU-side `decode_box_drawing` table lookup (`grid_tessellator.rs` line 936) is already efficient. The GPU-side decoding is the bottleneck for box-drawing heavy content (file managers, btop grids, tmux borders).

**Potential Fix (advanced):** Rasterize box-drawing characters to the glyph atlas at startup, eliminating GPU-side decoding. Trades startup atlas build time for GPU simplification.

**Impact:** MEDIUM (content-dependent)  
**Difficulty:** Hard  
**Risk:** Atlas memory pressure. Requires careful slot budgeting.

---

### 2.12 `write_grapheme` Allocates a `String` Per Character
**File:** `crates/forge-pty/src/vte_parser.rs`, line 18

```rust
fn print(&mut self, c: char) {
    let c = self.charsets.translate(c);
    self.buffer.write_grapheme(&c.to_string()); // heap allocation per character!
}
```

Every printable character from the PTY causes a heap allocation. Under high-throughput output (`cat` of a large file), this produces hundreds of thousands of allocations per second.

**Fix:**
```rust
let mut buf = [0u8; 4];
let s = c.encode_utf8(&mut buf);
self.buffer.write_grapheme(s);
```

**Impact:** HIGH (any high-throughput terminal output, fast scrolling)  
**Difficulty:** Easy  
**Risk:** Low. API change in `write_grapheme` required, but the vte `print()` path always receives a single `char`, so `encode_utf8` into `[u8; 4]` is correct.

---

### 2.13 `UnicodeWidthStr::width` Has No ASCII Fast-Path
**File:** `crates/forge-pty/src/screen_buffer.rs`, line 334

```rust
pub fn write_grapheme(&mut self, grapheme: &str) {
    let display_width = UnicodeWidthStr::width(grapheme);
```

For ASCII characters (the vast majority of terminal output), `display_width` is always 1. The unicode-width crate still performs a table scan.

**Fix:** Check `grapheme.is_ascii()` first and short-circuit to `display_width = 1`.

**Impact:** MEDIUM (ASCII-heavy workloads)  
**Difficulty:** Easy  
**Risk:** None — ASCII width is always 1 by specification.

---

### 2.14 `format!()` in Mouse Escape Sequence Generation
**File:** `event_loop.rs`, lines 841, 929, 978, 1016

```rust
let seq = format!("\x1b[<{};{};{}M", btn_code, col_1, row_1);
```

Allocates a `String` on the heap per mouse event. During continuous drag with SGR mouse tracking enabled, this is a high-frequency repeated allocation.

**Fix:** Write into a pre-allocated `Vec<u8>` or `ArrayString` stack buffer.

**Impact:** LOW-MEDIUM (only with mouse tracking enabled)  
**Difficulty:** Easy  
**Risk:** None.

---

### 2.15 `Vec` Allocation in `insert_dynamic_glyphs`
**File:** `renderer.rs`, line 273

```rust
let keys_to_insert: Vec<GlyphKey> = keys.iter().copied()
    .filter(...)
    .take(DYNAMIC_GLYPHS_PER_FRAME)
    .collect();
```

Allocates a `Vec` every frame that contains missing glyphs. `DYNAMIC_GLYPHS_PER_FRAME` is 16, so this allocation is small but avoidable.

**Fix:** Use a fixed-size stack array `[GlyphKey; 16]` with a count, or a reusable buffer in `Renderer`.

**Impact:** LOW  
**Difficulty:** Easy  
**Risk:** None.

---

## 3. Expensive GPU / Rendering Operations

### 3.1 Single Monolithic `vkCmdDraw` For All Vertices
**File:** `renderer.rs`, line 1014

```rust
self.device.cmd_draw(cmd, self.tessellator.vertices.len() as u32, 1, 0, 0);
```

All vertices (backgrounds, foregrounds, all rows, scrollbar) submit in a single draw call. Even during idle with only a cursor blink (1 row changed), the GPU runs the full fragment shader for every vertex.

**Alternative:** Multiple draw calls with per-dirty-row dynamic scissor rects would limit GPU work to changed regions.

**Impact:** MEDIUM during idle  
**Difficulty:** Hard (multiple draw calls, coordination with damage tracking)  
**Risk:** Medium complexity.

---

### 3.2 Full Vertex Buffer Re-flatten Every Frame
**File:** `grid_tessellator.rs`, line 749

```rust
self.vertices.clear();
```

Followed by re-filling `self.vertices` from all row vertex stores. Even when only 1 row changed, this O(total_cells) pass runs every frame. The generation system then skips the GPU upload for unchanged rows, but the CPU-side flatten still happens.

**Fix:** Track per-row start/count in `self.vertices`. Only re-flatten rows present in `rebuilt_rows`. Unchanged rows retain their existing position in the flat buffer.

**Impact:** HIGH for idle/low-update scenarios (cursor blink, single-line output)  
**Difficulty:** Medium  
**Risk:** Medium — must interoperate correctly with the scroll-reuse rotation logic.

---

### 3.3 Dynamic Glyph Atlas Update: Per-Glyph Blocking GPU Submit
**File:** `texture.rs`, `update_region()` (line 202)

Every new glyph inserted into the atlas calls `update_region` once:
1. Allocates a Vulkan staging buffer
2. Maps and copies pixel data
3. Submits a command buffer with pipeline barrier + copy
4. Calls `vkQueueWaitIdle` — **blocking GPU synchronization**

With up to `DYNAMIC_GLYPHS_PER_FRAME = 16` glyphs, this means up to **16 synchronous GPU stalls** mid-frame before the actual render.

**Fix:** Batch all atlas updates into a single command buffer submission with a single staging buffer. Issue multiple `vkCmdCopyBufferToImage` regions, then one pipeline barrier.

**Impact:** HIGH when new glyphs appear (startup, first Unicode paste, CJK content)  
**Difficulty:** Medium  
**Risk:** Low. Slightly more complex staging management.

---

### 3.4 Scrollbar and Braille `sqrt()` in Fragment Shader
**File:** `grid.frag.glsl`, lines 122, 196

```glsl
float dist = sqrt(dx*dx + dy*dy); // scrollbar pill shape
float d = length(vec2(px - cx, py - cy)); // braille dot distance
```

`sqrt()` is expensive in fragment shaders. For the scrollbar it runs every pixel in the scrollbar quad. For braille, every pixel in each braille character cell.

**Fix (scrollbar):** Use squared distance comparisons where possible. Only `smoothstep` boundaries truly need `dist`; reformulate thresholds in squared space.

**Fix (braille):** Since `pc.cell_size` provides cell dimensions, dot center coordinates are computable as known constants. Use squared distance throughout.

**Impact:** LOW-MEDIUM  
**Difficulty:** Medium  
**Risk:** Minor visual differences if not carefully rederived.

---

### 3.5 Powerline Triangle: `fwidth()` Anti-Aliasing
**File:** `grid.frag.glsl`, lines 237–239

```glsl
float pixel_w = fwidth(v_tex_coord.x);
float alpha = smoothstep(edge, -edge, d);
```

`fwidth()` requires derivative computation across fragment quads. In a Powerline-heavy shell (tmux, starship), dozens of Powerline separators appear each frame.

**Fix:** Since tex_coord range is 0–1 across the cell and `pc.cell_size.x` gives the pixel width, `fwidth` can be replaced with `1.0 / pc.cell_size.x` to get screen-space edge width, avoiding derivative instructions.

**Impact:** LOW  
**Difficulty:** Medium  
**Risk:** Slight visual differences at non-standard DPI.

---

## 4. Redundant or Duplicated Calculations

### 4.1 `Instant::now()` Called Twice in One Render Pass
**File:** `event_loop.rs`, lines 1088, 1190

Two separate `Instant::now()` system calls in the same render iteration may return slightly different values and are redundant.

**Fix:** Capture `now` once and reuse.

**Impact:** Very LOW  
**Difficulty:** Trivial  
**Risk:** None.

---

### 4.2 `use_alt_buffer` Lock Acquired Multiple Times Per Iteration
**File:** `event_loop.rs`, lines 437, 695, 1027, 1028, 1052, 1053, 1084

`screen_buffer.read().unwrap().use_alt_buffer` is acquired 7 times across one iteration.

**Fix:** Cache in a local at the top of the render section and pass to sub-handlers.

**Impact:** LOW  
**Difficulty:** Trivial  
**Risk:** Value must not be stale within one iteration.

---

### 4.3 `missing_glyphs` HashSet for Small Sets
**File:** `grid_tessellator.rs`, line 116

`HashSet::insert` is called for each missing glyph. The set is typically empty or tiny (≤16 entries), where `HashMap` hash overhead exceeds a linear `Vec` scan.

**Fix:** Use `Vec<GlyphKey>` with dedup after tessellation.

**Impact:** LOW-MEDIUM  
**Difficulty:** Easy  
**Risk:** None.

---

### 4.4 Redundant `vertices.clear()` + Full Re-flatten
**File:** `grid_tessellator.rs`, line 749

Same as §3.2 — `self.vertices.clear()` followed by complete re-population runs O(total_cells) every frame regardless of how many rows changed.

---

## 5. Work That Can Be Avoided

### 5.1 Frame Callback Before Renderer Exists
**File:** `event_loop.rs`, line 1111

A Wayland frame callback is requested regardless of whether `renderer` is `Some`. When the renderer is `None` (pre-initialization), the callback fires with no rendering happening, causing a wakeup → callback request → wakeup loop.

**Fix:** Guard the request behind `renderer.is_some()`.

**Impact:** LOW  
**Difficulty:** Trivial  
**Risk:** None.

---

### 5.2 Write Lock Acquisition in Hot Typing Path
**File:** `event_loop.rs`, lines 61–63

```rust
fn mark_cursor_row_dirty(screen_buffer: &Arc<RwLock<ScreenBuffer>>) {
    let mut sb = screen_buffer.write().unwrap();
    sb.mark_cursor_viewport_row_dirty();
}
```

Called every key press AND every blink tick. Acquiring a write lock blocks the PTY parser thread. At fast typing speeds this is measurable.

**Alternative:** `AtomicBool` "cursor_needs_redraw" flag; absorb it into the next frame render without a write lock.

**Impact:** LOW (contention only noticeable at very high key rates)  
**Difficulty:** Medium  
**Risk:** Slight change in cursor redraw timing.

---

### 5.3 Redundant `use_alt_buffer` Lock in Scroll Handler
**File:** `event_loop.rs`, lines 1028, 1052

```rust
let use_alt_buffer = app_data.screen_buffer.read().unwrap().use_alt_buffer;
```

Called inside the scroll handler after `use_alt` was already captured at line 695. Pass the already-captured value instead.

**Impact:** Very LOW  
**Difficulty:** Trivial  
**Risk:** None.

---

## 6. Caching and Precomputation Summary

| Item | Location | Current Cost | Proposed | Impact |
|---|---|---|---|---|
| sRGB LUT | `color.rs` | `powf(2.4)` per call | `[f32; 256]` at startup | **HIGH** |
| Theme color linearization | `event_loop.rs:1134` | 3× `powf` per frame | Cached on config change | **MEDIUM** |
| `GridMetrics` | `event_loop.rs:209` | Recomputed on drag + frame | Cached, invalidated on resize | **MEDIUM** |
| `inset_x`, `inset_y` | `grid_tessellator.rs:373` | Per glyph | Per frame (before loop) | **MEDIUM** |
| Selection normalization | `grid_tessellator.rs:458` | Per cell | Per frame (before row loop) | **MEDIUM** |
| `char→String` in `print()` | `vte_parser.rs:18` | Heap alloc per char | Stack `encode_utf8` | **HIGH** |
| `ndc()` scale constants | `grid_tessellator.rs:314` | Division per call | Precompute `2.0/vp_w`, `2.0/vp_h` | LOW |
| `origin_y` per cell | `grid_tessellator.rs:722` | Recomputed per path | Compute once, reuse | LOW |

---

## 7. Batching Opportunities

### 7.1 Glyph Atlas Update Batching
As in §3.3 — batch all per-frame atlas pixel uploads into one staging buffer and one command submission.

### 7.2 Mouse Escape Sequence Batching
Accumulate escape sequences into a `Vec<u8>` during the pointer event loop and flush once after all events are processed. Reduces syscall count during drag.

**Impact:** LOW  
**Difficulty:** Easy  
**Risk:** None.

---

## 8. Optimization Opportunity Summary Table

| # | Category | File / Location | Impact | Difficulty | Risk |
|---|---|---|---|---|---|
| 1 | sRGB LUT | `color.rs` → all callers | **HIGH** | Easy | None |
| 2 | `write_grapheme` stack alloc | `vte_parser.rs:18` | **HIGH** | Easy | Low |
| 3 | Avoid full vertex re-flatten | `grid_tessellator.rs:749` | **HIGH** | Medium | Medium |
| 4 | Batch atlas texture uploads | `texture.rs:202`, `renderer.rs:282` | **HIGH** | Medium | Low |
| 5 | Cache linearized theme colors | `event_loop.rs:1134` | **MEDIUM** | Easy | None |
| 6 | Cache `GridMetrics` | `event_loop.rs:209` | **MEDIUM** | Easy | Low |
| 7 | Selection normalize pre-loop | `grid_tessellator.rs:458` | **MEDIUM** | Easy | None |
| 8 | Precompute `inset_x/y` | `grid_tessellator.rs:373` | **MEDIUM** | Easy | None |
| 9 | `fwidth` → `pc.cell_size` (braille) | `grid.frag.glsl:111` | **MEDIUM** | Easy | None |
| 10 | `UnicodeWidthStr` ASCII fast-path | `screen_buffer.rs:334` | **MEDIUM** | Easy | None |
| 11 | Per-cell color caching | `grid_tessellator.rs:452` | **MEDIUM** | Easy | None |
| 12 | Skip `dirty_rows.clone()` | `event_loop.rs:1270` | LOW | Trivial | None |
| 13 | Cache `ndc` scale constants | `grid_tessellator.rs:314` | LOW | Easy | None |
| 14 | Reuse `origin_y` per cell | `grid_tessellator.rs:722` | LOW | Easy | None |
| 15 | Coalesce mouse escape writes | `event_loop.rs:841–1016` | LOW | Easy | None |
| 16 | Limit `use_alt_buffer` lock reads | `event_loop.rs` multiple | LOW | Trivial | None |
| 17 | `Instant::now()` dedup | `event_loop.rs:1088/1190` | LOW | Trivial | None |
| 18 | Dynamic glyphs: stack array | `renderer.rs:273` | LOW | Easy | None |
| 19 | Frame callback guard | `event_loop.rs:1111` | LOW | Trivial | None |
| 20 | Box-drawing atlas rasterization | `grid_tessellator.rs` + atlas | MEDIUM | Hard | Medium |
| 21 | Scrollbar/braille sqrt removal | `grid.frag.glsl:122,196` | LOW-MEDIUM | Medium | Low |
| 22 | Powerline `fwidth` removal | `grid.frag.glsl:237` | LOW | Medium | Low |

---

## 9. Recommended Implementation Order

### Phase 1 — Do Immediately (Easy, High Impact, No Risk)

1. **sRGB LUT** — eliminates `powf(2.4)` from every cell color conversion. Single 256-entry constant array precomputed at startup.
2. **`write_grapheme` stack alloc** — replace `c.to_string()` with `char::encode_utf8()` into `[u8; 4]`. Single-line change in `vte_parser.rs`.
3. **Cache linearized theme colors** — add 3 `[f32; 4]` fields to `AppData`; populate at config load; use in event loop.
4. **Skip `dirty_rows.clone()`** — remove `.clone()` at `event_loop.rs:1270`.
5. **Selection normalization pre-loop** — move 10 lines out of the inner cell loop.
6. **Precompute `inset_x`, `inset_y`** — move 2 lines outside the closure.
7. **`UnicodeWidthStr` ASCII fast-path** — add 1 `if grapheme.is_ascii()` check.
8. **`fwidth` → `pc.cell_size` in braille** — 2-line shader change.

### Phase 2 — Do Soon (Medium Impact, Low-Medium Difficulty)

9. **Cache `GridMetrics`** — add `cached_metrics: Option<GridMetrics>` to `AppData`.
10. **Batch dynamic glyph atlas uploads** — refactor `insert_dynamic_glyphs` and `update_region`.
11. **Per-cell color caching** — `last_fg`/`last_bg` inside the row loop.
12. **Pre-allocate `grid_refs` Vec** — store in `AppData`, reuse each frame.

### Phase 3 — Profile First

13. **Avoid full vertex re-flatten** — verify with `perf` that this is the bottleneck. Complexity warrants measurement first.
14. **GPU draw call splitting** — only implement if profiling shows GPU-side idle during cursor-only redraws.
15. **Box-drawing atlas rasterization** — only if profiling shows GPU fragment overhead from box-drawing content.
16. **Scrollbar/Powerline shader sqrt/fwidth removal** — minor; profile first.

---

## 10. Benchmarking Scenarios

| Scenario                                           | What to Measure                                     |
| -------------------------------------------------- | --------------------------------------------------- |
| **Idle terminal** (no output, cursor blinking)     | Frame time, CPU usage, GPU vertex count             |
| **Rapid typing**                                   | PTY throughput, event loop latency                  |
| **`cat` of a 10 MB file**                          | PTY parser throughput, sRGB conversion count        |
| **`htop`/`btop`** (full-screen TUI redraw at 1 Hz) | Tessellation time, braille fragment cost            |
| **`vim` editing**                                  | Alt-buffer detection, dirty row granularity         |
| **Mouse drag selection**                           | `GridMetrics` cache hit rate, pointer event latency |
| **Window resize**                                  | Swapchain recreation time, reflow time              |
| **Scrolling 10K lines of scrollback**              | Scroll-reuse efficiency, vertex upload bytes        |
| **Powerline-heavy prompt (starship/tmux)**         | Powerline fragment shader cost                      |
| **First frame with CJK or emoji**                  | Atlas upload batching benefit, staging buffer count |

**Profiling tools:**
- `cargo flamegraph` or `perf record` for CPU hotspots
- `renderdoc` or `nvidia nsight` for GPU frame analysis
- `FORGE_RENDER_STATS=1` for built-in stats logging (already implemented)

---

## 11. Notes on Correctness and Risk

- The **sRGB LUT** produces bit-identical results to the current `powf` path for all 256 `u8` inputs. Validate with the existing `test_to_srgb_linear()` unit test.
- The **`write_grapheme` stack change** correctly handles all code points since vte's `print()` always delivers a single `char`; `encode_utf8` into `[u8; 4]` covers all UTF-8 cases.
- The **vertex re-flatten optimization** (§3.2) is the highest-complexity change. It must interoperate correctly with scroll-reuse's rotation of `row_vertex_stores`. Measure carefully before and after.
- **Shader changes** should be validated visually at multiple font sizes and display scales before commit.

---

*Report generated: 2026-06-28. All line numbers reference the source as read during this analysis.*
