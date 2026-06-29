# Text Rendering Techniques Analysis Report

## Summary

Forge already has several important terminal-rendering fundamentals: a Vulkan renderer, CPU-rasterized glyph atlas, row-level dirty tracking, cached per-row tessellation, batched quad submission, manual/procedural box drawing, block element rendering, braille rendering, cursor blink throttling, and recent passive mouse-motion filtering.

The biggest gaps are not exotic font techniques. The highest-impact remaining work is lower-level renderer efficiency: avoid full vertex-buffer upload on every rendered frame, add Wayland/compositor damage where possible, reduce over-dirtying in the screen buffer, improve scroll behavior, and add a real dynamic glyph-cache strategy. SDF/MSDF, full GPU path rendering, compute glyph rasterization, and daemon mode are not the right next steps for a terminal body-text renderer.

Important nuance: dirty rows are partially implemented. `ScreenBuffer` tracks dirty rows and `GridTessellator` only rebuilds dirty row tessellation, but `Renderer::render_grid` still concatenates and uploads the full cached vertex list and records a full-screen render pass every rendered frame.

## Current Renderer Overview

- Renderer backend: Vulkan via `ash`. Evidence: `Renderer::new` in `crates/forge-renderer/src/renderer.rs`, Vulkan instance/device/swapchain setup in `device.rs`, `surface.rs`, `swapchain.rs`, `render_pass.rs`, and `pipeline.rs`.
- Text rendering path: terminal output is parsed into `ScreenBuffer` cells, visible rows are passed from `crates/forge-main/src/event_loop.rs` to `Renderer::render_grid`, then `GridTessellator::tessellate` builds/caches row vertices and the renderer draws one large triangle-list vertex buffer.
- Glyph loading/rasterization path: `FontRasterizer` uses `fontdue` in `crates/forge-renderer/src/font/rasterizer.rs`; `GlyphAtlas::build` rasterizes chars into an RGBA atlas in `font/atlas.rs`; the atlas is uploaded as a Vulkan texture in `Texture::new`.
- Caching behavior: `GridTessellator` stores `RowTessellation` per row and only rebuilds rows flagged dirty. The atlas is prebuilt in one shot. There is no code evidence for lazy dynamic glyph insertion, atlas eviction, or multiple atlas pages.
- Frame update behavior: `event_loop.rs` gates rendering on `ScreenBuffer::has_dirty_rows`, `force_redraw`, or scrollbar overlay animation. Cursor blink marks the cursor row dirty. Mouse motion is filtered by `pointer_motion_has_effect`.
- Dirty/damage behavior: row dirty tracking exists in `ScreenBuffer::dirty_rows`; selection and cursor dirtiness are row-scoped. There is no Vulkan Wayland damage tracking in the render path. The only `damage_buffer` use found is for the startup SHM first frame in `wayland/shm_buffer.rs`.
- Known performance-sensitive areas: `Renderer::render_grid` maps/unmaps and uploads the full combined vertex buffer each frame; the render pass clears the full swapchain image; scrolling marks entire scroll regions dirty; writes mark rows dirty even if the same cell contents are written again; the atlas may be very large in full mode because it pre-rasterizes broad Unicode/PUA/emoji ranges.

## Technique Status Table

| Technique | Status | Priority | Difficulty | Risk | Expected Benefit |
|---|---|---:|---|---|---|
| Glyph atlas / texture atlas | Already implemented | P1 | Medium | Medium | Strong: avoids per-frame glyph rasterization |
| Pre-rasterized atlas | Already implemented | P2 | Low | Medium | Good startup-after-load and rendering throughput; current full atlas may be heavy |
| Dynamic glyph cache | Not implemented but realistic | P1 | High | High | Better Unicode coverage and lower startup/memory than huge full atlas |
| Dirty row / dirty cell rendering | Partially implemented | P1 | Medium | Medium | Strong CPU reduction; cell-level tracking could reduce overwork |
| Wayland damage tracking | Not implemented but realistic | P1 | High | High | Lower compositor/GPU work if integrated correctly |
| Instanced rendering | Not implemented but realistic | P2 | High | Medium | Lower vertex bandwidth, but current single draw call already batches |
| Batched quads | Already implemented | P2 | Low | Low | Good baseline GPU path |
| Persistent mapped buffers / ring buffers | Not implemented but realistic | P1 | Medium | Medium | Avoids per-frame map/unmap and improves frame upload path |
| Scroll-copy optimization | Partially implemented | P1 | High | High | Large benefit for logs/build output; current scrolling dirties broad ranges |
| Cell grid diffing | Not implemented but realistic | P1 | Medium | Medium | Useful for TUIs that repaint same contents |
| Run-based rendering | Not implemented but realistic | P3 | Medium | Medium | Possible CPU/vertex reduction, less urgent with cached rows |
| Shaping cache | Partially implemented | P3 | Very High | Very High | Needed for complex scripts/ligatures, but not wired into renderer |
| Subpixel-position cache | Not implemented and probably not worth it | P4 | High | High | Little value with integer-snapped terminal cells |
| Monospace cell snapping | Already implemented | P1 | Low | Medium | Important for crisp text |
| Separate background and foreground rendering | Already implemented | P2 | Low | Low | Reduces default background quads and preserves layering |
| Special renderer for box drawing | Already implemented | P1 | Medium | Medium | Strong correctness benefit for `nvim`, `tmux`, `btop` borders |
| Special renderer for block elements | Already implemented | P1 | Medium | Medium | Strong TUI/chart correctness benefit |
| Braille cache optimization | Partially implemented | P2 | Medium | Medium | Procedural braille exists; atlas pre-cache strategy not needed yet |
| SDF fonts | Not implemented and probably not worth it | P4 | High | High | Poor fit for small terminal body text |
| MSDF / MTSDF fonts | Not implemented and probably not worth it | P4 | Very High | High | Overkill for terminal cells |
| CPU rasterization + GPU composition | Already implemented | P1 | Low | Low | Correct production-style base |
| Full GPU path rendering | Not implemented and probably not worth it | P4 | Very High | Very High | Overkill, likely worse for small text |
| Compute shader glyph rasterization | Not implemented and probably not worth it | P4 | Very High | Very High | High complexity, uncertain quality |
| Line cache / row texture cache | Not implemented but realistic | P3 | High | High | Potentially useful, but current Vulkan path favors vertex row cache first |
| Full terminal surface cache | Partially implemented | P2 | Medium | Medium | Frame skipping exists; no offscreen surface cache |
| Cursor blink throttling | Partially implemented | P1 | Medium | Medium | Cursor row dirtied, but full vertex upload/frame still happens |
| Mouse-move throttling | Already implemented | P1 | Low | Low | Avoids no-op redraw/work for passive motion |
| Atlas eviction / multi-atlas strategy | Not implemented but realistic | P2 | High | High | Needed for scalable Unicode/emoji support |
| Separate emoji rendering path | Not implemented but realistic | P3 | Very High | High | Needed for correct color emoji eventually |
| Server / daemon mode | Not implemented and probably not worth it | P4 | Very High | High | Startup benefit possible, but premature |

## Detailed Technique Analysis

### 1. Glyph Atlas / Texture Atlas

**Status:** Already implemented  
**Evidence:** `GlyphAtlas` stores atlas pixels, dimensions, and glyph maps; `Texture::new` uploads atlas pixels to a Vulkan sampled image; `grid.frag.glsl` samples `glyph_atlas`.  
**Relevant files/functions:** `crates/forge-renderer/src/font/atlas.rs::GlyphAtlas::build`, `GlyphAtlas::get`; `crates/forge-renderer/src/texture.rs::Texture::new`; `crates/forge-renderer/src/renderer.rs::update_font_data`; `crates/forge-renderer/src/shaders/grid.frag.glsl`.  
**Expected benefit:** Already provides the main benefit: glyphs are rasterized once per atlas build rather than every frame.  
**Implementation difficulty:** Medium, already done.  
**Risk level:** Medium, because atlas size and Unicode fallback behavior affect correctness.  
**Performance impact:** Strong positive for steady-state rendering.  
**Memory impact:** Potentially high in full mode because broad Unicode, PUA, and emoji ranges are pre-rasterized.  
**Recommendation:** Keep this architecture. Improve it with dynamic/multi-atlas behavior rather than replacing it.  
**Priority:** P1.

### 2. Pre-Rasterized Atlas

**Status:** Already implemented  
**Evidence:** `GlyphAtlas::build` rasterizes ASCII in `fast_mode`; full mode rasterizes ASCII, Latin, Greek, Cyrillic, punctuation/box/block/braille, PUA, supplemental PUA, and emoji/symbol ranges. `main.rs` builds full atlas on a background thread with `fast_mode=false`.  
**Relevant files/functions:** `crates/forge-renderer/src/font/atlas.rs::GlyphAtlas::build`; `crates/forge-main/src/main.rs` background font loading.  
**Expected benefit:** Fast lookup for common and many uncommon glyphs after the full atlas arrives.  
**Implementation difficulty:** Low for current version.  
**Risk level:** Medium: full pre-rasterization can cost startup/background CPU, GPU memory, and atlas upload time.  
**Performance impact:** Good steady-state, but may be wasteful for rare glyph ranges.  
**Memory impact:** High in full mode due to very broad preloaded ranges and bold duplication.  
**Recommendation:** Keep ASCII/common pre-rasterization, but narrow the full preloaded set once dynamic glyph insertion exists.  
**Priority:** P2.

### 3. Dynamic Glyph Cache

**Status:** Not implemented but realistic  
**Evidence:** Atlas glyph maps are built once in `GlyphAtlas::build`; `GlyphAtlas::get` only reads existing maps and falls back to `'?'`. No insertion path or atlas update path for a missing glyph was found.  
**Relevant files/functions:** `font/atlas.rs::GlyphAtlas::get`, `renderer.rs::update_font_data`, `grid_tessellator.rs` atlas lookups.  
**Expected benefit:** Better Unicode compatibility with lower startup cost and smaller initial atlas. Rare glyphs can be loaded on demand.  
**Implementation difficulty:** High. Requires atlas packing, texture updates, missing-glyph queueing, synchronization, and fallback behavior.  
**Risk level:** High. Incorrect texture updates can cause stalls or missing glyphs; live atlas mutation affects renderer safety.  
**Performance impact:** Positive after implementation; first-use glyph misses introduce small latency.  
**Memory impact:** Positive if it replaces broad pre-rasterization; needs eviction or paging for worst cases.  
**Recommendation:** Implement after the vertex upload/persistent buffer work. Use a hybrid atlas: ASCII/common/procedural first, dynamic Unicode second.  
**Priority:** P1.

### 4. Dirty Row / Dirty Cell Rendering

**Status:** Partially implemented  
**Evidence:** `ScreenBuffer` has `dirty_rows: Vec<bool>`, `has_dirty_rows`, `mark_all_clean`, cursor/selection dirty helpers. `GridTessellator` only rebuilds rows in `actual_dirty`. There is no dirty cell bitset. `Renderer::render_grid` still uploads all combined vertices after tessellation.  
**Relevant files/functions:** `crates/forge-pty/src/screen_buffer.rs`; `crates/forge-renderer/src/grid_tessellator.rs::tessellate`; `crates/forge-main/src/event_loop.rs` redraw gate; `crates/forge-renderer/src/renderer.rs::render_grid`.  
**Expected benefit:** Already reduces CPU tessellation for idle and row-local changes. More benefit remains in upload and damage paths.  
**Implementation difficulty:** Medium for refining rows; High for dirty cells/range uploads.  
**Risk level:** Medium because selection, cursor, wide chars, scrollback, and resize can invalidate neighboring cells/rows.  
**Performance impact:** Strong positive if completed through upload/damage.  
**Memory impact:** Low for row bits; moderate for cell/range bitsets.  
**Recommendation:** Keep row tracking as the main model. Add cell/range diffing only where it clearly reduces TUI repaint cost.  
**Priority:** P1.

### 5. Wayland Damage Tracking

**Status:** Not implemented but realistic  
**Evidence:** The Vulkan render path presents through the swapchain and clears the full render area. The only `surface.damage_buffer` call found is `ShmBuffer::present` for the startup SHM first frame, not normal Vulkan frames.  
**Relevant files/functions:** `crates/forge-main/src/wayland/shm_buffer.rs::present`; `crates/forge-renderer/src/renderer.rs::render_grid`; `crates/forge-main/src/wayland/frame_callback.rs`.  
**Expected benefit:** Could reduce compositor work for row-local updates, cursor blink, and selection changes.  
**Implementation difficulty:** High with Vulkan swapchains, because compositor damage semantics and swapchain present behavior must be handled correctly.  
**Risk level:** High. Wrong damage can leave stale pixels on some compositors.  
**Performance impact:** Potentially positive on compositors that honor buffer damage; less useful if every Vulkan frame still clears full swapchain.  
**Memory impact:** Low.  
**Recommendation:** Implement only after renderer can preserve/copy previous frame contents or scissor redraws safely.  
**Priority:** P1.

### 6. Instanced Rendering

**Status:** Not implemented but realistic  
**Evidence:** `GlyphVertex::get_binding_description` uses `vk::VertexInputRate::VERTEX`; `cmd_draw(..., instance_count=1)` is used. There is no instance buffer or instance-rate binding.  
**Relevant files/functions:** `crates/forge-renderer/src/pipeline.rs::GlyphVertex`; `crates/forge-renderer/src/renderer.rs::render_grid`; `grid.vert.glsl`.  
**Expected benefit:** Could reduce vertex bandwidth from 6 vertices per quad to one instance per glyph/rect.  
**Implementation difficulty:** High. Requires shader and vertex format redesign.  
**Risk level:** Medium. Mostly renderer-local, but affects all procedural glyph paths.  
**Performance impact:** Moderate positive for large grids and high refresh output; current single draw call already avoids per-glyph draw overhead.  
**Memory impact:** Positive: smaller per-frame upload.  
**Recommendation:** Consider after persistent mapped buffers and dynamic atlas. Current batched quads are acceptable for now.  
**Priority:** P2.

### 7. Batched Quads

**Status:** Already implemented  
**Evidence:** `GridTessellator` pushes six `GlyphVertex` values per quad into row vectors; `Renderer::render_grid` issues one `cmd_draw` for the whole vertex list.  
**Relevant files/functions:** `grid_tessellator.rs::tessellate`; `renderer.rs::render_grid`; `pipeline.rs::GlyphVertex`.  
**Expected benefit:** Avoids one draw call per cell/glyph.  
**Implementation difficulty:** Low, already done.  
**Risk level:** Low.  
**Performance impact:** Strong positive baseline.  
**Memory impact:** Moderate because every quad is expanded to six full vertices.  
**Recommendation:** Keep until profiling proves instance conversion is necessary.  
**Priority:** P2.

### 8. Persistent Mapped Buffers / Ring Buffers

**Status:** Not implemented but realistic  
**Evidence:** `Renderer::render_grid` calls `device.map_memory`, copies all vertices, then `device.unmap_memory` for each rendered frame. The vertex buffer is host-visible/coherent but not persistently mapped.  
**Relevant files/functions:** `crates/forge-renderer/src/renderer.rs::render_grid`; `texture.rs::create_buffer`.  
**Expected benefit:** Reduces CPU overhead and synchronization risk in the hot render path.  
**Implementation difficulty:** Medium.  
**Risk level:** Medium. Must respect frames-in-flight and avoid overwriting GPU-read ranges.  
**Performance impact:** Meaningful for every rendered frame.  
**Memory impact:** Slightly higher if implemented as ring-buffer regions.  
**Recommendation:** High-value next renderer optimization. Implement before more complex rendering redesigns.  
**Priority:** P1.

### 9. Scroll-Copy Optimization

**Status:** Partially implemented  
**Evidence:** `ScreenBuffer::scroll_up_in_region` uses `VecDeque::pop_front/push_back` for full-screen scrolling and keeps scrollback in a ring, which avoids copying every row in that common case. However it marks the entire scroll region dirty, and the renderer does not copy existing GPU content upward. Partial-region scrolling uses `VecDeque::remove/insert`.  
**Relevant files/functions:** `crates/forge-pty/src/screen_buffer.rs::scroll_up_in_region`, `scroll_down_in_region`, `insert_lines`, `delete_lines`; `renderer.rs::render_grid`.  
**Expected benefit:** Very high for large output streams and TUIs that scroll panes.  
**Implementation difficulty:** High if including renderer/GPU scroll-copy; Medium for cleaner CPU dirty-row handling.  
**Risk level:** High due to scroll regions, alt buffer, scrollback, wrapped rows, and selection.  
**Performance impact:** Potentially strong.  
**Memory impact:** Low to moderate.  
**Recommendation:** First improve CPU-side dirty marking for full-screen scrolls; defer GPU image-copy until damage/partial redraw design is stable.  
**Priority:** P1.

### 10. Cell Grid Diffing

**Status:** Not implemented but realistic  
**Evidence:** `write_grapheme`, `write_ascii_run`, erase operations, and cursor movement mark rows dirty unconditionally after assignment. There is no old/new `Cell` comparison before dirtying. `Cell` implements `PartialEq`, so the data structure supports comparison.  
**Relevant files/functions:** `crates/forge-pty/src/screen_buffer.rs::write_grapheme`, `write_ascii_run`, erase methods; `crates/forge-core/src/cell.rs::Cell`.  
**Expected benefit:** Good for `btop`, `htop`, `nvim`, `tmux`, and apps that repaint unchanged cells.  
**Implementation difficulty:** Medium.  
**Risk level:** Medium. Must handle cursor movement, wide placeholders, wrapped rows, and attributes correctly.  
**Performance impact:** Positive for TUI steady-state CPU and renderer work.  
**Memory impact:** Low.  
**Recommendation:** Implement after tests around wide characters and selection dirtiness.  
**Priority:** P1.

### 11. Run-Based Rendering

**Status:** Not implemented but realistic  
**Evidence:** `GridTessellator` iterates cell-by-cell and emits quads per background/glyph/procedural element. There is no run grouping by style or glyph sequence.  
**Relevant files/functions:** `crates/forge-renderer/src/grid_tessellator.rs::tessellate`.  
**Expected benefit:** May reduce background quads and allow better batching for same-style text.  
**Implementation difficulty:** Medium.  
**Risk level:** Medium because terminals require per-cell positioning, wide chars, selection, cursor, and procedural symbols.  
**Performance impact:** Moderate, mostly CPU/vertex count.  
**Memory impact:** Positive if it reduces emitted vertices.  
**Recommendation:** Optional after core upload/damage issues. Row caching already reduces repeated per-cell work.  
**Priority:** P3.

### 12. Shaping Cache

**Status:** Partially implemented  
**Evidence:** `font/shaper.rs` defines `ShaperCache`, `TextRunKey`, and uses `rustybuzz::shape`, but repository searches did not show `ShaperCache` used by `Renderer`, `GridTessellator`, or atlas lookup. Current rendering uses `char` keys and one glyph per cell.  
**Relevant files/functions:** `crates/forge-renderer/src/font/shaper.rs`; `grid_tessellator.rs`; `font/atlas.rs`.  
**Expected benefit:** Required for ligatures, Arabic, Indic scripts, and advanced Unicode correctness.  
**Implementation difficulty:** Very High. Terminal cells make shaping harder than normal UI text.  
**Risk level:** Very High. Can break cursor positioning, selection, column width, and app compatibility.  
**Performance impact:** Mixed. Cache helps repeated runs, but shaping adds complexity and CPU work.  
**Memory impact:** Moderate for shaped-run cache.  
**Recommendation:** Delay until the renderer and Unicode width model are more mature. Treat current module as a prototype.  
**Priority:** P3.

### 13. Subpixel-Position Cache

**Status:** Not implemented and probably not worth it  
**Evidence:** Text placement is rounded in `GridTessellator` via `text_origin`, `g_x`, and `g_y`; texture sampler uses nearest filtering. No subpixel buckets exist.  
**Relevant files/functions:** `grid_tessellator.rs::text_origin`, `push_atlas_glyph`; `texture.rs::Texture::new`.  
**Expected benefit:** Limited for a terminal that prioritizes crisp integer-aligned cells.  
**Implementation difficulty:** High.  
**Risk level:** High for cache growth and blur.  
**Performance impact:** Likely negative or marginal.  
**Memory impact:** Negative if multiple subpixel variants are cached.  
**Recommendation:** Avoid for terminal body text.  
**Priority:** P4.

### 14. Monospace Cell Snapping

**Status:** Already implemented  
**Evidence:** `text_origin` floors inset and rounds origin; glyph position and dimensions are rounded; underline/strikethrough positions are rounded/floored. `compute_grid_metrics` controls effective cell size and padding.  
**Relevant files/functions:** `crates/forge-renderer/src/grid_tessellator.rs::tessellate`; `crates/forge-main/src/event_loop.rs::compute_grid_metrics`.  
**Expected benefit:** Important for crisp text and aligned TUI geometry.  
**Implementation difficulty:** Low, already present.  
**Risk level:** Medium because fill-padding cell scaling can interact with glyph placement.  
**Performance impact:** Neutral to positive.  
**Memory impact:** Neutral.  
**Recommendation:** Preserve this design. Any future scaling should keep glyph bitmaps native-sized and pixel-aligned.  
**Priority:** P1.

### 15. Separate Background and Foreground Rendering

**Status:** Already implemented  
**Evidence:** `RowTessellation` stores `bg_vertices` and `fg_vertices`. Tessellation appends all row backgrounds, then scrollbar, then foregrounds. Default background is handled via render-pass clear; background quads are emitted only for non-default backgrounds, selection, or block cursor.  
**Relevant files/functions:** `crates/forge-renderer/src/grid_tessellator.rs::RowTessellation`, `tessellate`; `renderer.rs::render_grid`.  
**Expected benefit:** Reduces vertices/fill for default-background cells and keeps correct layering.  
**Implementation difficulty:** Low, already done.  
**Risk level:** Low.  
**Performance impact:** Positive.  
**Memory impact:** Positive versus drawing backgrounds for every cell.  
**Recommendation:** Keep. Consider merging adjacent same-color backgrounds later if profiling shows need.  
**Priority:** P2.

### 16. Special Renderer for Box Drawing

**Status:** Already implemented  
**Evidence:** `decode_box_drawing` maps U+2500..U+257F into encoded line data; `grid.frag.glsl` procedurally renders lines/arcs when `proc_id <= -100 && > -500`; DEC special graphics translates ASCII line-drawing mode to box chars.  
**Relevant files/functions:** `crates/forge-renderer/src/grid_tessellator.rs::decode_box_drawing`; `crates/forge-renderer/src/shaders/grid.frag.glsl`; `crates/forge-pty/src/vte_parser.rs::translate_dec_special_graphics`.  
**Expected benefit:** Strong visual correctness for `nvim`, `tmux`, dashboards, borders, and tables.  
**Implementation difficulty:** Medium, already present.  
**Risk level:** Medium because off-by-one pixel math affects border continuity.  
**Performance impact:** Positive/neutral; avoids font inconsistencies but adds shader branches for procedural quads.  
**Memory impact:** Positive because box drawing does not require atlas glyphs.  
**Recommendation:** Keep and test with Unicode-heavy TUIs.  
**Priority:** P1.

### 17. Special Renderer for Block Elements

**Status:** Already implemented  
**Evidence:** `GridTessellator` handles U+2580..U+259F as rectangle geometry instead of atlas glyphs.  
**Relevant files/functions:** `crates/forge-renderer/src/grid_tessellator.rs` block element match arm.  
**Expected benefit:** Strong for charts, progress bars, `btop`, and block UI.  
**Implementation difficulty:** Medium, already present.  
**Risk level:** Medium because exact fractional blocks must align to cell dimensions.  
**Performance impact:** Positive/neutral.  
**Memory impact:** Positive versus atlas glyphs.  
**Recommendation:** Keep. Add visual tests for block ranges if possible.  
**Priority:** P1.

### 18. Braille Cache Optimization

**Status:** Partially implemented  
**Evidence:** Braille U+2800..U+28FF is detected in `GridTessellator` and rendered procedurally in `grid.frag.glsl`, with `BrailleStyle` push constants. There is no separate braille atlas/cache, but procedural rendering may be better than pre-caching.  
**Relevant files/functions:** `grid_tessellator.rs` braille branch; `grid.frag.glsl` `proc_id <= -500`; `crates/forge-core/src/config_registry.rs::BrailleStyle`; `renderer.rs::render_grid`.  
**Expected benefit:** Good for `btop` graphs and terminal plots.  
**Implementation difficulty:** Medium, mostly done.  
**Risk level:** Medium due to visual style and pixel alignment.  
**Performance impact:** Positive for memory, possibly small shader cost.  
**Memory impact:** Positive because no atlas range needed.  
**Recommendation:** Treat as implemented procedurally. No separate cache is needed unless profiling shows shader cost.  
**Priority:** P2.

### 19. SDF Fonts

**Status:** Not implemented and probably not worth it  
**Evidence:** Atlas stores coverage bitmaps from `fontdue`; fragment shader samples coverage and does alpha correction. No distance-field generation or SDF shader path was found.  
**Relevant files/functions:** `font/rasterizer.rs`, `font/atlas.rs`, `grid.frag.glsl`.  
**Expected benefit:** Good for scalable game/UI text, weak for small terminal text.  
**Implementation difficulty:** High.  
**Risk level:** High for text sharpness and hinting.  
**Performance impact:** Likely not better for terminal body text.  
**Memory impact:** Could reduce variants but may need larger textures or shader work.  
**Recommendation:** Avoid for terminal body text.  
**Priority:** P4.

### 20. MSDF / MTSDF Fonts

**Status:** Not implemented and probably not worth it  
**Evidence:** No MSDF generation dependency or RGB distance-field shader path was found.  
**Relevant files/functions:** No relevant implementation beyond bitmap atlas in `font/atlas.rs` and `grid.frag.glsl`.  
**Expected benefit:** Mostly for large scalable labels; not terminal body text.  
**Implementation difficulty:** Very High.  
**Risk level:** High.  
**Performance impact:** Uncertain; likely worse complexity/performance tradeoff for small text.  
**Memory impact:** Usually higher per glyph than single-channel coverage.  
**Recommendation:** Avoid unless Forge later adds zoomable non-terminal UI text.  
**Priority:** P4.

### 21. CPU Rasterization + GPU Composition

**Status:** Already implemented  
**Evidence:** `fontdue` rasterizes glyphs on CPU; Vulkan texture atlas and quads compose glyphs on GPU.  
**Relevant files/functions:** `font/rasterizer.rs::FontRasterizer`; `font/atlas.rs::GlyphAtlas::build`; `texture.rs::Texture::new`; `renderer.rs::render_grid`.  
**Expected benefit:** Best practical architecture for a terminal.  
**Implementation difficulty:** Low, already present.  
**Risk level:** Low.  
**Performance impact:** Strong positive.  
**Memory impact:** Moderate atlas memory.  
**Recommendation:** Keep as the core text architecture.  
**Priority:** P1.

### 22. Full GPU Path Rendering

**Status:** Not implemented and probably not worth it  
**Evidence:** No outline/path/tessellation font renderer was found; renderer uses bitmap atlas and procedural geometry only for terminal symbols.  
**Relevant files/functions:** `font/atlas.rs`, `grid_tessellator.rs`, `pipeline.rs`.  
**Expected benefit:** Low for fixed-size terminal text.  
**Implementation difficulty:** Very High.  
**Risk level:** Very High.  
**Performance impact:** Likely worse for small text and many glyphs.  
**Memory impact:** Could reduce atlas memory but increase GPU work.  
**Recommendation:** Avoid.  
**Priority:** P4.

### 23. Compute Shader Glyph Rasterization

**Status:** Not implemented and probably not worth it  
**Evidence:** Pipeline contains vertex and fragment shaders only; no compute pipeline, compute shader source, or glyph compute path was found.  
**Relevant files/functions:** `pipeline.rs::Pipeline::new`; `renderer/src/shaders`.  
**Expected benefit:** Unclear and probably low for this project.  
**Implementation difficulty:** Very High.  
**Risk level:** Very High.  
**Performance impact:** Uncertain; hinting and small text quality are difficult.  
**Memory impact:** Uncertain.  
**Recommendation:** Avoid for now.  
**Priority:** P4.

### 24. Line Cache / Row Texture Cache

**Status:** Not implemented but realistic  
**Evidence:** There is a per-row vertex cache (`RowTessellation`) but no per-row texture cache or offscreen row images.  
**Relevant files/functions:** `grid_tessellator.rs::RowTessellation`; absence of row texture resources in `renderer.rs`.  
**Expected benefit:** Could make composition cheap for mostly-static rows, especially with cursor/selection overlays.  
**Implementation difficulty:** High in Vulkan due to texture allocation, resize, damage, and overlay handling.  
**Risk level:** High.  
**Performance impact:** Potentially positive, but may increase complexity and memory.  
**Memory impact:** Higher: one or more textures per row or atlas-like row surfaces.  
**Recommendation:** Not a near-term priority for the current Vulkan design. Consider only after profiling.  
**Priority:** P3.

### 25. Full Terminal Surface Cache

**Status:** Partially implemented  
**Evidence:** Event loop skips rendering when there are no dirty rows, no force redraw, and no scrollbar overlay redraw. There is no offscreen terminal texture or explicit reuse of a rendered terminal surface. Swapchain frames are redrawn when needed.  
**Relevant files/functions:** `event_loop.rs` redraw gate; `renderer.rs::render_grid`.  
**Expected benefit:** Frame skipping already gives the biggest idle CPU win. A true offscreen cache would matter for partial damage/cursor overlays.  
**Implementation difficulty:** Medium to High.  
**Risk level:** Medium.  
**Performance impact:** Positive if paired with damage tracking; less useful alone.  
**Memory impact:** Higher due to offscreen surface.  
**Recommendation:** Keep frame skipping. Defer full offscreen surface cache until damage strategy is designed.  
**Priority:** P2.

### 26. Cursor Blink Throttling

**Status:** Partially implemented  
**Evidence:** `event_loop.rs` schedules blink timeout and marks only the cursor viewport row dirty via `mark_cursor_viewport_row_dirty`. `GridTessellator` tracks `last_cursor` and `last_cursor_visible` to dirty old/new cursor rows. However `Renderer::render_grid` still uploads all vertices and renders the full swapchain frame.  
**Relevant files/functions:** `event_loop.rs` cursor blink block; `screen_buffer.rs::mark_cursor_viewport_row_dirty`; `grid_tessellator.rs` cursor dirty handling.  
**Expected benefit:** Already avoids retessellating unrelated rows; further benefit requires partial uploads/damage.  
**Implementation difficulty:** Medium to complete.  
**Risk level:** Medium because cursor style, blink overrides, and selection interact.  
**Performance impact:** Current benefit is moderate; potential benefit is high for idle blinking.  
**Memory impact:** Low.  
**Recommendation:** Complete through renderer upload/damage improvements.  
**Priority:** P1.

### 27. Mouse-Move Throttling

**Status:** Already implemented  
**Evidence:** `pointer_motion_has_effect` classifies passive motion; the pointer loop coalesces no-op `Motion` events and skips layout/cell-coordinate work; `seat.rs` uses nonblocking `try_send` for motion.  
**Relevant files/functions:** `crates/forge-main/src/event_loop.rs::pointer_motion_has_effect`, pointer event loop; `crates/forge-main/src/wayland/seat.rs` motion handling.  
**Expected benefit:** Avoids unnecessary redraw scheduling and hot-path work during passive mouse movement, especially in alt-buffer apps like `vim`/`btop`.  
**Implementation difficulty:** Low, already present.  
**Risk level:** Low to Medium. Button/axis events must remain lossless.  
**Performance impact:** Positive for rapid passive motion; compositor event delivery still costs something.  
**Memory impact:** Neutral.  
**Recommendation:** Keep; add live profiling if regressions are suspected.  
**Priority:** P1.

### 28. Atlas Eviction / Multi-Atlas Strategy

**Status:** Not implemented but realistic  
**Evidence:** `GlyphAtlas` contains a single pixel buffer and two `HashMap<char, GlyphMetrics>` maps. There is no LRU, page list, atlas generation, or eviction metadata.  
**Relevant files/functions:** `crates/forge-renderer/src/font/atlas.rs::GlyphAtlas`; `renderer.rs::update_font_data`.  
**Expected benefit:** Important for large Unicode and emoji support without huge startup atlas.  
**Implementation difficulty:** High.  
**Risk level:** High. Requires renderer descriptor/pipeline support for multiple textures or texture arrays.  
**Performance impact:** Positive for startup/memory; runtime misses require careful handling.  
**Memory impact:** Positive if it replaces enormous full atlas; slightly higher metadata.  
**Recommendation:** Pair with dynamic glyph cache. Use separate common/static and dynamic pages.  
**Priority:** P2.

### 29. Separate Emoji Rendering Path

**Status:** Not implemented but realistic  
**Evidence:** Full atlas range includes emoji/symbol codepoints, but `fontdue` bitmap glyph handling is monochrome coverage; there is no color emoji rasterizer, sequence shaping, image atlas, or separate pipeline. Rendering is single `char` per cell.  
**Relevant files/functions:** `font/atlas.rs::GlyphAtlas::build`; `screen_buffer.rs::write_grapheme`; `grid_tessellator.rs` glyph lookup.  
**Expected benefit:** Needed for correct color emoji and emoji sequences.  
**Implementation difficulty:** Very High.  
**Risk level:** High due to Unicode grapheme clusters, widths, fallback fonts, and color glyph formats.  
**Performance impact:** Mixed; separate cache helps once implemented.  
**Memory impact:** Higher due to color image atlas.  
**Recommendation:** Delay. First fix Unicode fallback/dynamic glyph infrastructure.  
**Priority:** P3.

### 30. Server / Daemon Mode

**Status:** Not implemented and probably not worth it  
**Evidence:** `main.rs` starts one process, one Wayland window, one PTY, and one renderer. No server/client binary, IPC, shared cache daemon, or multi-window manager was found.  
**Relevant files/functions:** `crates/forge-main/src/main.rs`; no server modules found.  
**Expected benefit:** Could reduce repeated startup cost and share font/cache resources.  
**Implementation difficulty:** Very High.  
**Risk level:** High. Adds process lifecycle, IPC, security, and crash isolation issues.  
**Performance impact:** Startup-only; not relevant to steady-state rendering.  
**Memory impact:** Mixed. Shared cache helps, daemon consumes baseline memory.  
**Recommendation:** Avoid until the single-window renderer is mature.  
**Priority:** P4.

## Highest-Impact Recommendations

1. **Persistent mapped/ring vertex buffers**
   - Why it matters: `Renderer::render_grid` maps/unmaps and uploads all vertices every rendered frame.
   - Problem solved: per-frame CPU overhead and potential synchronization stalls.
   - Why realistic: localized to `renderer.rs` and buffer ownership.
   - Affected code: `Renderer`, `SyncPrimitives`, `texture.rs::create_buffer`.
   - First step: keep the existing vertex format, but persistently map a frames-in-flight ring buffer.

2. **Complete dirty rendering through upload/damage**
   - Why it matters: row tessellation is cached, but upload/render pass are still full-frame.
   - Problem solved: cursor blink and small row updates still perform too much GPU/CPU work.
   - Why realistic: existing `dirty_rows` is a good foundation.
   - Affected code: `ScreenBuffer`, `GridTessellator`, `Renderer::render_grid`, Wayland frame scheduling.
   - First step: track dirty vertex ranges or row offsets before attempting compositor damage.

3. **Cell grid diffing**
   - Why it matters: many TUIs repaint unchanged cells.
   - Problem solved: avoids marking rows dirty for identical writes.
   - Why realistic: `Cell` already implements `PartialEq`.
   - Affected code: `ScreenBuffer::write_grapheme`, `write_ascii_run`, erase/fill paths.
   - First step: add small helper that assigns a cell and marks dirty only on change.

4. **Dynamic glyph cache with multi-atlas strategy**
   - Why it matters: current full atlas preloads a very broad range.
   - Problem solved: lowers startup/background cost and improves rare Unicode behavior.
   - Why realistic: current atlas abstraction can be extended, but it is a significant renderer feature.
   - Affected code: `GlyphAtlas`, `Texture`, descriptor sets, `GridTessellator`.
   - First step: introduce a missing-glyph queue and dynamic atlas page metadata.

5. **Scroll optimization**
   - Why it matters: terminal output scrolls constantly.
   - Problem solved: avoids retessellating/redrawing large regions when content shifts.
   - Why realistic: CPU full-screen scroll already uses `VecDeque`; next gains are dirty-row/range and later GPU copy.
   - Affected code: `ScreenBuffer::scroll_up_in_region`, renderer row cache, damage tracking.
   - First step: distinguish full-screen scroll from partial scroll region and preserve/reindex row tessellation where safe.

## Techniques That Are Probably Not Worth It

- **SDF/MSDF fonts:** Good for scalable game UI, not ideal for small terminal body text. Forge already uses target-size CPU rasterization and pixel snapping, which is the more appropriate foundation.
- **Full GPU path rendering:** Overly complex and unlikely to outperform atlas text for dense small glyphs.
- **Compute shader glyph rasterization:** Interesting but high risk; font hinting and small-size quality are hard.
- **Subpixel-position cache:** Conflicts with the crisp integer-cell model and can explode cache size.
- **Server/daemon mode:** Could reduce startup later, but it adds major lifecycle and IPC complexity before core rendering is finished.
- **Row texture cache:** Not bad in principle, but it is not the best next step for Forge’s current Vulkan vertex-cache architecture.

## Suggested Implementation Order

```text
1. Keep and verify current dirty frame skipping.
2. Implement persistent mapped/ring vertex buffers.
3. Add row vertex offset/range metadata so only changed ranges can be updated later.
4. Add cell grid diffing for writes/erase paths.
5. Improve scroll handling: reindex row tessellation for full-screen scroll where safe.
6. Design partial redraw/damage model; only then add Wayland damage tracking.
7. Add dynamic glyph cache and split atlas pages.
8. Narrow full pre-rasterized atlas once dynamic Unicode works.
9. Add atlas eviction/multi-atlas policy.
10. Investigate shaping cache integration for ligatures/complex scripts.
11. Add emoji path after shaping/fallback font infrastructure exists.
12. Consider instanced rendering only if vertex bandwidth remains a bottleneck.
```

## Risks and Compatibility Notes

- **Unicode:** Current `ScreenBuffer::write_grapheme` stores only the first char of a grapheme and skips zero-width combining graphemes. Complex clusters are not fully supported.
- **Ligatures:** `ShaperCache` exists but is not wired into rendering; current atlas lookup is `char`-based.
- **Emoji:** No color emoji or emoji sequence path exists. Preloading emoji codepoint ranges does not equal correct emoji rendering.
- **Box drawing:** Strong current support through DEC special graphics translation and procedural shader rendering. Still needs visual regression checks because shader pixel math is sensitive.
- **Block characters:** Implemented procedurally; useful for `btop` and progress bars. Needs tests under different font sizes and padding modes.
- **Braille:** Procedural braille exists with configurable style. This is good for `btop`, but visual density may differ from font-rendered braille.
- **Nerd Font symbols:** Atlas full mode includes PUA ranges and Powerline-style procedural handling exists for selected codepoints. Dynamic fallback and atlas size remain concerns.
- **Terminal apps:** `vim`, `nvim`, `btop`, `htop`, and `tmux` rely on alt buffer, mouse modes, box drawing, and frequent row updates. Cell diffing and scroll optimization would help most.
- **Wayland compositor behavior:** Damage tracking must be tested across compositors. Incorrect damage with Vulkan can cause stale pixels.
- **GPU fallback behavior:** Renderer requires Vulkan/Wayland surface creation. No non-Vulkan text renderer was found beyond startup SHM clear.
- **Resizing:** `resize_reflow` is substantial and marks all rows dirty. Any row cache or damage work must invalidate correctly on resize.
- **Cursor blinking:** Cursor-row dirtying exists, but full vertex upload/frame render still occurs. Partial upload/damage is required for maximal idle efficiency.
- **Mouse movement:** Passive motion is now filtered/coalesced. Selection drag, scrollbar drag, and mouse reporting remain active paths and should not be throttled incorrectly.

## Final Recommendation

Implement first: persistent mapped/ring vertex buffers, then complete the dirty-row model through upload/range tracking and cell diffing. These are realistic, evidence-backed improvements that fit the current architecture and directly reduce CPU/GPU work.

Delay: dynamic glyph cache/multi-atlas, shaping cache, emoji rendering, and instanced rendering. They are useful but depend on a stable renderer and careful Unicode design.

Avoid for now: SDF/MSDF terminal body text, full GPU path rendering, compute shader glyph rasterization, and daemon mode. They are impressive but not the best tradeoff for Forge’s current terminal renderer.

Needs deeper investigation: Vulkan-compatible Wayland damage tracking and GPU scroll-copy. Both can be valuable, but only after the renderer can avoid full-surface clearing/redrawing safely.
