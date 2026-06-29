# Rendering Optimization Implementation Plan

## Summary

This plan is based on `./notes/techniques.md`, `./notes/techniques_analysis.md`, and the code evidence cited in those reports. The realistic optimization path is to finish the renderer work that is already partially implemented before adding larger features.

The highest-impact optimizations are:

1. Preserve the current dirty-frame skipping and make it measurable.
2. Replace per-frame Vulkan vertex-buffer map/unmap with persistent mapped/ring-buffer upload.
3. Complete dirty rendering by tracking row vertex ranges and reducing full-buffer rewrite work.
4. Add cell grid diffing so TUIs that repaint unchanged cells do not dirty rows unnecessarily.
5. Improve scroll handling so full-screen scrolls do not force avoidable retessellation.

This plan intentionally delays SDF/MSDF text, GPU path rendering, compute glyph rasterization, row texture caches, daemon mode, and full emoji/shaping work. Those are either explicitly not recommended by the analysis report or too risky before the core renderer upload/damage path is stable.

## Current Renderer State

- **Rendering backend:** Vulkan via `ash`. Evidence is in `crates/forge-renderer/src/renderer.rs`, `device.rs`, `surface.rs`, `swapchain.rs`, `render_pass.rs`, and `pipeline.rs`.
- **Text rendering path:** PTY bytes are parsed into `ScreenBuffer`, visible rows are passed through `crates/forge-main/src/event_loop.rs`, `GridTessellator::tessellate` builds/caches row vertices, and `Renderer::render_grid` submits one large triangle-list draw.
- **Glyph caching state:** A CPU-rasterized glyph atlas exists. `FontRasterizer` uses `fontdue`; `GlyphAtlas::build` pre-rasterizes a set of characters; `Texture::new` uploads the atlas. There is no dynamic glyph insertion, atlas eviction, or multi-atlas paging.
- **Dirty rendering state:** `ScreenBuffer::dirty_rows` and `ScreenBuffer::has_dirty_rows` exist. `GridTessellator` rebuilds only dirty row tessellation. However, `Renderer::render_grid` still concatenates all cached rows, maps/unmaps the Vulkan vertex memory, uploads all vertices, clears the full render target, and draws the full vertex list for each rendered frame.
- **Damage tracking state:** Normal Vulkan frames do not use Wayland damage tracking. The only known `damage_buffer` call is in `crates/forge-main/src/wayland/shm_buffer.rs::present` for the startup SHM frame.
- **Known incomplete optimizations:** dirty row rendering, cursor blink throttling, full terminal surface cache concept, scroll-copy optimization, and braille caching/procedural handling are partial or incomplete depending on scope.
- **Major performance problems:** full vertex-buffer upload per rendered frame, per-frame map/unmap, full render-pass clear, over-dirtying rows for identical cell writes, broad dirtying during scroll, and large full atlas construction.

## Priority Order

The order below favors low-risk improvements that build on existing architecture:

1. First preserve and measure existing dirty-frame behavior so later changes can be validated.
2. Then reduce the hot Vulkan upload cost without changing visual output.
3. Then complete the dirty-row model through row metadata and partial upload readiness.
4. Then reduce false dirtiness in the PTY/screen-buffer layer.
5. Then optimize scrolling, which depends on correct dirty-row semantics.
6. Only after that, evaluate Wayland damage and dynamic atlas work.
7. Defer shaping, emoji, instancing, and row texture caches until after the core renderer is stable and profiled.

## Implementation Plan Table

| Order | Optimization | Current Status | Priority | Difficulty | Risk | Expected Benefit |
|---:|---|---|---:|---|---|---|
| 1 | Baseline measurement and dirty-frame guardrails | Partially implemented | P0 | Low | Low | Prevents regressions and proves later wins |
| 2 | Persistent mapped/ring vertex buffer | Not implemented but realistic | P1 | Medium | Medium | Reduces per-frame CPU overhead and sync risk |
| 3 | Row vertex range metadata and partial upload foundation | Partially implemented | P1 | Medium | Medium | Completes dirty-row renderer architecture |
| 4 | Cell grid diffing | Not implemented but realistic | P1 | Medium | Medium | Reduces unnecessary redraws in TUIs |
| 5 | Cursor blink minimal redraw completion | Partially implemented | P1 | Medium | Medium | Improves idle CPU/GPU behavior during blinking |
| 6 | Scroll optimization, phase 1: CPU row/tessellation reuse | Partially implemented | P1 | High | High | Improves fast output and scrolling workloads |
| 7 | Wayland damage tracking design and guarded implementation | Not implemented but realistic | P2 | High | High | Reduces compositor work if full redraw assumptions are removed |
| 8 | Dynamic glyph cache, phase 1 | Not implemented but realistic | P2 | High | High | Improves Unicode with lower startup/memory cost |
| 9 | Multi-atlas/eviction strategy | Not implemented but realistic | P2 | High | High | Scales Unicode/emoji glyph coverage |
| 10 | Pre-rasterized atlas narrowing | Already implemented but broad | P2 | Medium | Medium | Reduces background font-build and atlas upload cost |
| 11 | Run-based background/render simplification | Not implemented but realistic | P3 | Medium | Medium | Reduces vertices in some common rows |
| 12 | Instanced rendering evaluation | Not implemented but realistic | P3 | High | Medium | Possible vertex bandwidth reduction |
| 13 | Shaping cache integration investigation | Partially implemented but unused | P3 | Very High | Very High | Complex-script and ligature correctness |
| 14 | Emoji rendering path investigation | Not implemented but realistic | P3 | Very High | High | Correct color emoji and sequences |

## Detailed Implementation Plan

### 1. Baseline Measurement and Dirty-Frame Guardrails

**Current status:** Partially implemented. The event loop already skips frames unless `ScreenBuffer::has_dirty_rows`, `force_redraw`, or scrollbar overlay redraw is active.  
**Why it matters:** Every later optimization needs a before/after baseline. Without measurement, changes can look clean but fail to improve real workloads.  
**Expected benefit:** No direct rendering speedup, but lowers risk and makes performance claims verifiable.  
**Files/modules likely affected:** `crates/forge-main/src/event_loop.rs`, `crates/forge-renderer/benches/tessellation_baseline.rs`, possibly new benchmark or trace helper under `crates/forge-renderer/benches/` or `crates/forge-pty/benches/`.  
**Implementation steps:**
1. Add or document a repeatable local benchmark checklist for idle shell, cursor blink, rapid mouse motion, fast output, and `btop`/`nvim`.
2. Add lightweight counters behind a debug/profiling flag for frames submitted, dirty rows counted, vertices uploaded, and bytes uploaded.
3. Ensure counters are disabled by default and do not affect release hot paths.
4. Record baseline numbers before changing renderer upload behavior.
5. Add tests for redraw predicates where pure functions exist.
**Difficulty:** Low  
**Risk level:** Low  
**Testing strategy:** `cargo test --workspace`; compare existing benchmarks; manually profile idle and fast-output behavior with the same terminal size before and after.  
**Rollback strategy:** Remove profiling counters and benchmark-only additions; no behavior changes should be involved.  
**Priority:** P0  
**Dependencies:** None.

### 2. Persistent Mapped/Ring Vertex Buffer

**Current status:** Not implemented. `Renderer::render_grid` maps and unmaps `vertex_memory` every rendered frame.  
**Why it matters:** This is a hot-path cost for every rendered frame, including cursor blink and small row updates. It is a contained renderer optimization that should not change visual output.  
**Expected benefit:** Lower CPU overhead per frame, fewer Vulkan memory-map calls, reduced chance of CPU/GPU synchronization stalls.  
**Files/modules likely affected:** `crates/forge-renderer/src/renderer.rs`, `crates/forge-renderer/src/texture.rs::create_buffer`, `crates/forge-renderer/src/sync.rs`, possibly a small helper type in `crates/forge-renderer/src/renderer.rs` or a new buffer module.  
**Implementation steps:**
1. Introduce a renderer-owned mapped vertex-buffer abstraction that stores `vk::Buffer`, `vk::DeviceMemory`, mapped pointer, capacity, and per-frame offsets.
2. Allocate enough capacity for all frames in flight or maintain ring regions sized to worst-case current vertex capacity.
3. Map once during creation and unmap during renderer drop or buffer recreation.
4. Copy vertices into the current frame's region instead of offset zero.
5. Bind the vertex buffer with the current region offset in `cmd_bind_vertex_buffers`.
6. On capacity growth or swapchain resize, wait idle, destroy old buffer safely, allocate/map the new buffer, and preserve existing resize behavior.
7. Keep memory host-visible/coherent initially to avoid adding explicit flush complexity.
8. Add tests where possible for capacity calculation and keep runtime validation through `cargo test --workspace`.
**Difficulty:** Medium  
**Risk level:** Medium  
**Testing strategy:** `cargo test --workspace`; run fast output (`cat large_file`, `cargo build`) and cursor blink; verify no Vulkan validation errors; compare uploaded byte counters and CPU profile.  
**Rollback strategy:** Keep the old map/copy/unmap code path in a small helper until the new path is verified; revert to single-buffer upload if validation or corruption appears.  
**Priority:** P1  
**Dependencies:** Baseline measurement.

### 3. Row Vertex Range Metadata and Partial Upload Foundation

**Current status:** Partially implemented. `GridTessellator` caches row `bg_vertices` and `fg_vertices`, but `self.vertices` is rebuilt from all rows and the full vertex list is uploaded.  
**Why it matters:** Dirty rows currently save tessellation CPU but not full upload/render bandwidth. Row range metadata is the bridge between dirty-row tracking and true partial uploads/damage.  
**Expected benefit:** Better renderer architecture, lower CPU memory-copy work once partial upload is enabled, and required foundation for cursor/damage improvements.  
**Files/modules likely affected:** `crates/forge-renderer/src/grid_tessellator.rs`, `crates/forge-renderer/src/renderer.rs`, `crates/forge-renderer/src/pipeline.rs` if vertex layout changes later.  
**Implementation steps:**
1. Extend `GridTessellator` with row vertex range metadata: background start/count and foreground start/count after final assembly.
2. Preserve existing drawing order: all backgrounds, scrollbar overlay, then all foregrounds.
3. Add a stable row generation/version value so renderer can know which row vertex data changed.
4. Initially keep full draw behavior unchanged while exposing metadata for tests.
5. Add tests that dirtying one row only rebuilds that row and updates that row's version/range.
6. After metadata is verified, allow renderer upload code to detect whether a full upload is required or whether changed row slices can be copied into stable offsets.
7. If stable offsets require too much compaction complexity, stop after metadata and reassess before changing upload semantics.
**Difficulty:** Medium  
**Risk level:** Medium  
**Testing strategy:** Existing tessellator tests; new tests for row range stability, cursor dirty rows, selection dirty rows, scrollbar overlay; `cargo test -p forge-renderer`; visual checks in `nvim`, `tmux`, and shell.  
**Rollback strategy:** Keep `self.vertices` full assembly as canonical until partial upload is proven; remove metadata without changing rendering output.  
**Priority:** P1  
**Dependencies:** Persistent mapped buffer is preferred first, but metadata can be developed independently if kept behavior-neutral.

### 4. Cell Grid Diffing

**Current status:** Not implemented. `ScreenBuffer::write_grapheme`, `write_ascii_run`, erase/fill operations, and some cursor movement paths mark rows dirty unconditionally.  
**Why it matters:** TUIs often rewrite the same characters/styles repeatedly. Diffing avoids dirtying rows when the resulting cell is unchanged.  
**Expected benefit:** Lower CPU work and fewer rendered frames/dirty rows in `btop`, `htop`, `nvim`, `vim`, and `tmux`.  
**Files/modules likely affected:** `crates/forge-pty/src/screen_buffer.rs`, `crates/forge-core/src/cell.rs`, `crates/forge-pty/src/vte_parser.rs` tests.  
**Implementation steps:**
1. Add a helper like `set_cell_if_changed(row, col, cell)` that compares old/new `Cell` values and marks the row dirty only on change.
2. Add a helper for default-cell fills that only marks dirty if any changed cell was modified.
3. Update `write_grapheme` and `write_ascii_run` to use the helper.
4. Handle wide characters carefully: the leading wide cell and placeholder cell both need comparison and correct clearing of overwritten trailing cells.
5. Update erase, insert/delete chars, erase line/screen, and theme update paths incrementally.
6. Do not change cursor movement dirtying until cursor rendering tests are explicit; cursor dirtiness is visual even when cell data does not change.
7. Add tests for repeated identical writes, repeated erase, wide char overwrite, selection dirty rows, and SGR/style changes.
**Difficulty:** Medium  
**Risk level:** Medium  
**Testing strategy:** `cargo test -p forge-pty`; targeted tests for dirty row arrays; run `btop`, `htop`, `nvim`, `tmux`; verify no stale characters after overwrites and wide chars.  
**Rollback strategy:** Keep helper localized; revert call sites to unconditional assignment/dirtying if stale rendering appears.  
**Priority:** P1  
**Dependencies:** None strictly, but baseline counters should exist first to measure benefit.

### 5. Cursor Blink Minimal Redraw Completion

**Current status:** Partially implemented. Cursor blink marks the cursor row dirty and `GridTessellator` tracks old/new cursor rows, but renderer still performs full vertex upload and full render target clear/draw for the frame.  
**Why it matters:** Cursor blink is an idle periodic wakeup. It should be one of the cheapest render paths.  
**Expected benefit:** Lower idle CPU/GPU usage while cursor blinking is enabled.  
**Files/modules likely affected:** `crates/forge-main/src/event_loop.rs`, `crates/forge-pty/src/screen_buffer.rs`, `crates/forge-renderer/src/grid_tessellator.rs`, `crates/forge-renderer/src/renderer.rs`.  
**Implementation steps:**
1. Preserve current cursor blink scheduling and row dirtying.
2. Use row vertex metadata to identify old/new cursor row updates.
3. Avoid full retessellation outside affected rows, which should already be true.
4. After partial upload support exists, update only affected row vertex ranges.
5. After damage tracking exists, damage only old/new cursor row or cursor rect if safe.
6. Test block, beam, underline, visible/invisible phases, cursor moved while blinking, and app cursor blink override.
**Difficulty:** Medium  
**Risk level:** Medium  
**Testing strategy:** Unit tests for cursor row dirtiness; visual checks for all cursor styles; idle CPU observation with blink on/off.  
**Rollback strategy:** Revert to row dirty/full upload behavior while keeping cursor correctness.  
**Priority:** P1  
**Dependencies:** Row vertex metadata; partial upload and/or damage tracking for full benefit.

### 6. Scroll Optimization, Phase 1: CPU Row/Tessellation Reuse

**Current status:** Partially implemented. Full-screen scroll uses `VecDeque::pop_front/push_back`, but `scroll_up_in_region` marks the full region dirty. Partial-region scrolls use remove/insert and mark broad ranges dirty.  
**Why it matters:** Terminal output scrolls constantly during `cat`, logs, `cargo build`, shells, and TUI panes.  
**Expected benefit:** Lower tessellation and upload work during continuous scrolling.  
**Files/modules likely affected:** `crates/forge-pty/src/screen_buffer.rs`, `crates/forge-renderer/src/grid_tessellator.rs`, `crates/forge-main/src/event_loop.rs`.  
**Implementation steps:**
1. Add explicit scroll-event metadata to `ScreenBuffer` only after deciding how renderer will consume it; do not infer from dirty rows alone.
2. For full-screen scroll up/down, track the scroll count and newly exposed rows.
3. Teach `GridTessellator` to reindex cached row tessellations for full-screen scrolls where cell geometry/padding/font did not change.
4. Mark only newly exposed rows and cursor-affected rows dirty when safe.
5. Keep partial scroll regions conservative at first; mark full region dirty until tests cover region behavior.
6. Ensure scrollback viewport scrolling (`view_scroll_up/down`) remains full viewport dirty because visible rows are changing from history.
7. Add tests for full-screen scroll, scroll regions, alt buffer, wrapped rows, and scrollback.
**Difficulty:** High  
**Risk level:** High  
**Testing strategy:** `cargo test -p forge-pty -p forge-renderer`; visual fast-scroll tests; `cat large_file`, `cargo build`, `journalctl` equivalent, `nvim` split scrolling, `tmux` panes.  
**Rollback strategy:** Keep broad dirty marking as fallback; disable tessellation reindexing behind a feature/debug flag during validation.  
**Priority:** P1  
**Dependencies:** Row vertex metadata; cell diffing helpful but not required.

### 7. Wayland Damage Tracking Design and Guarded Implementation

**Current status:** Not implemented for Vulkan frames. Startup SHM uses `surface.damage_buffer`, but normal rendering does not.  
**Why it matters:** Dirty rendering inside Forge still may cause the compositor to handle larger areas than necessary.  
**Expected benefit:** Lower compositor/GPU work for cursor blink, small row updates, and selection changes on compositors that honor damage.  
**Files/modules likely affected:** `crates/forge-main/src/wayland/frame_callback.rs`, `crates/forge-main/src/event_loop.rs`, `crates/forge-renderer/src/renderer.rs`, possibly Wayland surface handling in `window.rs`.  
**Implementation steps:**
1. Write a design note first: determine whether Vulkan swapchain presentation path can safely use Wayland surface damage with current full render-pass clear.
2. Do not implement damage while every render clears the full swapchain.
3. Once renderer can preserve unchanged pixels or safely redraw scissored regions, compute damage rectangles from dirty row ranges, cursor rows, selection rows, and scrollbar overlay.
4. Add a config/debug switch to force full damage for fallback.
5. Test on at least the user's compositor and document compositor-specific behavior.
6. If stale pixels occur, rollback to full damage/present immediately.
**Difficulty:** High  
**Risk level:** High  
**Testing strategy:** Manual compositor tests; resize, cursor blink, fast output, selection, scrollbar, alt buffer; visual stale-pixel checks.  
**Rollback strategy:** Runtime switch to full-surface damage/no damage; keep old full render path.  
**Priority:** P2  
**Dependencies:** Partial redraw model or offscreen/surface preservation strategy.

### 8. Dynamic Glyph Cache, Phase 1

**Current status:** Not implemented. Atlas is built in one shot and `GlyphAtlas::get` falls back to `'?'` when a glyph is missing.  
**Why it matters:** Current full pre-rasterization is broad and can be expensive; dynamic insertion supports rare Unicode without preloading everything.  
**Expected benefit:** Better startup/memory tradeoff and better rare Unicode compatibility.  
**Files/modules likely affected:** `crates/forge-renderer/src/font/atlas.rs`, `font/rasterizer.rs`, `renderer.rs`, `texture.rs`, `grid_tessellator.rs`, `crates/forge-main/src/main.rs` font loading.  
**Implementation steps:**
1. Define `GlyphKey` explicitly: character or glyph identity, bold state, font descriptor, size, and relevant style flags.
2. Add missing-glyph detection that records misses during tessellation without blocking the render path.
3. Add an atlas page/packer abstraction for dynamic glyph slots.
4. Add a safe texture update path for new glyph rectangles, likely using staging buffer and `vkCmdCopyBufferToImage`.
5. For phase 1, do not evict; allocate one dynamic page and fall back when full.
6. Ensure missing glyph insertion wakes/re-renders affected rows after upload.
7. Add tests for missing glyph discovery and atlas map updates; manually test Greek/Cyrillic/Nerd Font symbols.
**Difficulty:** High  
**Risk level:** High  
**Testing strategy:** Unit tests for atlas packing; renderer smoke tests; Unicode files; `nvim` with Nerd Font icons; fallback glyph behavior.  
**Rollback strategy:** Keep current full atlas path; disable dynamic insertion and fall back to current atlas build.  
**Priority:** P2  
**Dependencies:** Persistent buffer not required, but renderer should be stable first.

### 9. Multi-Atlas/Eviction Strategy

**Current status:** Not implemented. Single atlas texture and single descriptor set are used.  
**Why it matters:** A dynamic glyph cache needs a strategy when the atlas fills, especially with Unicode and emoji.  
**Expected benefit:** Scalable glyph coverage without unbounded memory or enormous startup atlas.  
**Files/modules likely affected:** `font/atlas.rs`, `renderer.rs`, `pipeline.rs`, `grid_tessellator.rs`, descriptor set layout in `pipeline.rs`, texture ownership in `texture.rs`.  
**Implementation steps:**
1. Decide between multiple sampled textures, texture arrays, or separate atlas pages.
2. Extend glyph metrics with page/index information.
3. Update vertex data or draw batching to include atlas page selection.
4. Start with no eviction and multiple pages; add LRU only after correctness is proven.
5. If eviction is added, track row/glyph dependencies so evicted glyphs invalidate affected rows.
6. Keep ASCII/common atlas non-evictable.
**Difficulty:** High  
**Risk level:** High  
**Testing strategy:** Stress with many Unicode scripts and PUA icons; atlas-full tests; memory usage observation; fallback behavior.  
**Rollback strategy:** Cap dynamic pages and fall back to `'?'`; retain static atlas path.  
**Priority:** P2  
**Dependencies:** Dynamic glyph cache phase 1.

### 10. Pre-Rasterized Atlas Narrowing

**Current status:** Already implemented but broad. Full mode pre-rasterizes Latin, Greek, Cyrillic, punctuation, box/block/braille, PUA, supplementary PUA, and emoji/symbol ranges.  
**Why it matters:** Once dynamic glyphs exist, broad pre-rasterization wastes startup/background CPU, GPU memory, and upload time.  
**Expected benefit:** Faster background font load, lower memory, smaller texture upload.  
**Files/modules likely affected:** `crates/forge-renderer/src/font/atlas.rs::GlyphAtlas::build`, `crates/forge-main/src/main.rs` font-loading mode selection, startup cache if descriptor changes.  
**Implementation steps:**
1. Define a minimal static set: ASCII, common punctuation, maybe selected Powerline/Nerd Font and symbols that are frequently used before dynamic cache warms.
2. Exclude box/block/braille if procedural renderer handles them fully.
3. Move rare Unicode to dynamic atlas.
4. Update atlas descriptor/cache version if needed.
5. Benchmark startup/background atlas build time and first-use Unicode behavior.
**Difficulty:** Medium  
**Risk level:** Medium  
**Testing strategy:** Startup timing; shell prompt with Nerd Font icons; `nvim` dashboard; Unicode samples; missing glyph fallback.  
**Rollback strategy:** Restore broader pre-rasterized ranges if dynamic misses are too visible.  
**Priority:** P2  
**Dependencies:** Dynamic glyph cache and multi-atlas basics.

### 11. Run-Based Background/Render Simplification

**Current status:** Not implemented. Tessellation is cell-by-cell.  
**Why it matters:** Adjacent same-style backgrounds and text runs can reduce emitted vertices and CPU work.  
**Expected benefit:** Moderate benefit in rows with large same-color spans, selections, and TUI backgrounds.  
**Files/modules likely affected:** `crates/forge-renderer/src/grid_tessellator.rs`.  
**Implementation steps:**
1. Start with background run merging only; do not merge glyph rendering initially.
2. Merge adjacent same-color background cells into one quad when no cursor/selection exception splits the run.
3. Add tests for selection boundaries, cursor block background, wide glyph overflow, and default background skipping.
4. Consider foreground run metadata later only if profiling justifies it.
**Difficulty:** Medium  
**Risk level:** Medium  
**Testing strategy:** Tessellator vertex-count tests; visual checks for selection/cursor/TUI backgrounds.  
**Rollback strategy:** Keep current per-cell background emission path as fallback.  
**Priority:** P3  
**Dependencies:** None, but should wait until higher-priority dirty/upload work.

### 12. Instanced Rendering Evaluation

**Current status:** Not implemented. Current pipeline uses per-vertex data and one draw call.  
**Why it matters:** Instancing could reduce vertex bandwidth, but the current draw-call count is already low.  
**Expected benefit:** Possible reduction in upload size and memory bandwidth; not guaranteed to beat optimized quads.  
**Files/modules likely affected:** `pipeline.rs`, `grid.vert.glsl`, `grid.frag.glsl`, `grid_tessellator.rs`, `renderer.rs`.  
**Implementation steps:**
1. Do not implement immediately.
2. After persistent buffers and row ranges, profile vertex bandwidth and CPU copy cost.
3. Prototype an instance layout only if vertex bandwidth remains a bottleneck.
4. Compare against optimized batched quads before committing.
**Difficulty:** High  
**Risk level:** Medium  
**Testing strategy:** Benchmark prototype branch; compare visual output for glyphs, box drawing, blocks, braille, scrollbar, cursor.  
**Rollback strategy:** Keep batched quads as default.  
**Priority:** P3  
**Dependencies:** Profiling after P1 renderer work.

### 13. Shaping Cache Integration Investigation

**Current status:** Partially implemented but unused. `font/shaper.rs` exists, but rendering still uses `char` atlas keys and monospace cells.  
**Why it matters:** Needed for complex scripts, ligatures, and better Unicode, but it is a correctness feature more than an immediate performance optimization.  
**Expected benefit:** Better text compatibility for complex scripts and optional ligatures.  
**Files/modules likely affected:** `font/shaper.rs`, `font/atlas.rs`, `screen_buffer.rs`, `grid_tessellator.rs`, Unicode width/grapheme handling.  
**Implementation steps:**
1. Investigate terminal cell model requirements for shaped clusters.
2. Decide whether ligatures are in scope before complex scripts.
3. Design shaped-run storage without breaking cursor addressing and selection.
4. Do not integrate into normal rendering until dynamic glyph infrastructure exists.
**Difficulty:** Very High  
**Risk level:** Very High  
**Testing strategy:** Arabic, Devanagari, Bengali, ligature fonts, emoji sequences, cursor movement, selection.  
**Rollback strategy:** Keep shaping disabled by default behind a config/feature flag.  
**Priority:** P3  
**Dependencies:** Dynamic glyph cache; Unicode model design.

### 14. Emoji Rendering Path Investigation

**Current status:** Not implemented. Current atlas is monochrome coverage and stores one `char` per cell.  
**Why it matters:** Correct color emoji requires fallback fonts, color glyph support, grapheme clusters, and often image atlas handling.  
**Expected benefit:** Compatibility improvement, not a near-term performance win.  
**Files/modules likely affected:** `screen_buffer.rs`, `font/atlas.rs`, `font/rasterizer.rs`, `grid_tessellator.rs`, `renderer.rs`, possibly new emoji/fallback font module.  
**Implementation steps:**
1. Define target emoji support level: monochrome fallback, color bitmap, SVG/layered glyphs, or sequences.
2. Do not modify current renderer until dynamic atlas and shaping/fallback design exist.
3. Prototype separate emoji atlas and rendering path later.
**Difficulty:** Very High  
**Risk level:** High  
**Testing strategy:** Emoji width, sequences, color glyphs, mixed text, selection/cursor alignment.  
**Rollback strategy:** Keep current monochrome/fallback behavior until feature is complete.  
**Priority:** P3  
**Dependencies:** Dynamic atlas, shaping/fallback font design.

## Partially Implemented / Incomplete Optimizations

- **Dirty row rendering:** `ScreenBuffer::dirty_rows` and row tessellation cache exist, but upload/render still operate on the full assembled vertex list.
- **Scroll-copy optimization:** CPU full-screen scroll uses `VecDeque`, but broad dirty marking remains and renderer cache reindexing is not implemented.
- **Full terminal surface cache concept:** frame skipping exists, but no offscreen surface cache or partial redraw preservation exists.
- **Cursor blink throttling:** cursor row dirtying exists, but full-frame upload/render remains.
- **Braille optimization:** procedural braille exists, but it should be validated visually rather than expanded into a separate cache right now.
- **Shaping cache:** module exists but is not integrated; delay until Unicode architecture is ready.

## Highest-Impact New Optimizations

- **Persistent mapped/ring vertex buffer:** most contained renderer hot-path improvement.
- **Cell grid diffing:** likely real benefit in TUI apps that repaint unchanged cells.
- **Dynamic glyph cache:** high value for Unicode/startup, but riskier and should wait until renderer hot path is stable.
- **Wayland damage tracking:** high potential compositor benefit, but only safe after partial redraw design.
- **Multi-atlas strategy:** needed for scalable dynamic glyphs, but depends on dynamic cache.

## Optimizations to Delay or Avoid

- **SDF/MSDF fonts:** not appropriate for small crisp terminal body text.
- **Full GPU path rendering:** overkill and high risk for dense terminal glyphs.
- **Compute shader glyph rasterization:** very high complexity with uncertain quality/performance.
- **Subpixel-position cache:** conflicts with integer cell snapping and risks blur/cache explosion.
- **Server/daemon mode:** startup-only benefit and major IPC/lifecycle complexity.
- **Row texture cache:** potentially useful later, but current Vulkan architecture should first complete vertex row caching and upload improvements.
- **Emoji and shaping:** important compatibility work, but too risky before dynamic glyph/fallback infrastructure exists.

## Testing Plan

- **Idle CPU usage:** run shell idle with cursor blink on/off; compare CPU and frame counters.
- **Mouse movement with no scrollbar:** rapid motion in shell with no scrollback, `vim`, and `btop`; verify no unnecessary redraws.
- **Normal shell typing:** type commands, backspace, selections, paste, and prompt redraws.
- **Fast output like `cat large_file`:** observe smooth output, no dropped/stale rows, CPU/GPU behavior.
- **`cargo build` output:** sustained scrolling with colored output.
- **`nvim`:** dashboard, splits, statusline, box drawing, cursor modes, mouse mode.
- **`vim`:** alt buffer, cursor movement, scrolling, selection behavior.
- **`btop`:** braille graphs, block characters, mouse motion, frequent redraws.
- **`htop`:** color rows, scrolling, function key labels.
- **`tmux`:** pane borders, statusline, mouse mode, alternate screen behavior.
- **Resize behavior:** grow/shrink repeatedly; verify `resize_reflow`, PTY resize, padding fill/center behavior, and no stale rows.
- **Cursor blinking:** block, beam, underline, blink override escape sequences.
- **Unicode text:** Greek, Cyrillic, combining marks, wide CJK, Arabic/Indic as known-limited cases.
- **Box drawing characters:** U+2500..U+257F plus DEC special graphics.
- **Block characters:** U+2580..U+259F fractional blocks.
- **Nerd Font symbols:** prompt icons, Powerline separators, PUA icons.

## Verification Checklist

Use after each optimization:

```text
[ ] Project builds successfully
[ ] cargo test --workspace passes
[ ] Relevant crate tests pass individually
[ ] No rendering regression in shell
[ ] No rendering regression in nvim/vim
[ ] No rendering regression in btop/htop
[ ] No rendering regression in tmux
[ ] Idle CPU usage did not increase
[ ] Mouse movement does not trigger unnecessary redraws
[ ] Cursor blink still renders correctly
[ ] Selection still renders and copies correctly
[ ] Resize still works correctly
[ ] Padding and padding-fill behavior remain intact
[ ] Unicode fallback behavior did not regress
[ ] Box drawing remains aligned
[ ] Block characters remain aligned
[ ] Braille rendering remains acceptable
[ ] No memory usage spike
[ ] No Vulkan validation errors
[ ] No stale pixels or damaged regions
[ ] Rollback path is clear before moving to the next optimization
```

## Final Recommended Order

```text
1. Baseline measurement and dirty-frame guardrails
2. Persistent mapped/ring vertex buffer
3. Row vertex range metadata and partial upload foundation
4. Cell grid diffing
5. Cursor blink minimal redraw completion
6. Scroll optimization, phase 1: CPU row/tessellation reuse
7. Wayland damage tracking design and guarded implementation
8. Dynamic glyph cache, phase 1
9. Multi-atlas/eviction strategy
10. Pre-rasterized atlas narrowing
11. Run-based background/render simplification
12. Instanced rendering evaluation
13. Shaping cache integration investigation
14. Emoji rendering path investigation
```

## Stop Point

This is the planning phase only. No renderer behavior should be changed until this plan is reviewed and approved.

After approval, implement one optimization at a time. Do not start the next optimization until the current one builds, passes tests, has been manually checked in relevant terminal apps where possible, and has a clear rollback path.
