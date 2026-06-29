# Multi-Atlas and Eviction Implementation Plan

## Objective

Implement a scalable glyph atlas strategy for Forge after the current dynamic glyph cache has been validated. The goal is to support more Unicode and private-use glyphs than the current reserved dynamic slot area can hold, without returning to a very large startup atlas.

This is not scheduled for immediate implementation. It should be picked up only after the current single-atlas dynamic cache shows real exhaustion or unacceptable fallback behavior in normal terminal workloads.

## Why It Matters

Forge now supports dynamic glyph insertion into reserved slots in the existing atlas. That is enough for many shells and TUI applications, but it has a hard capacity limit. Heavy Unicode, multiple scripts, Nerd Font-heavy prompts, and future emoji/fallback font support can exceed a single dynamic area.

Multi-atlas support matters when:
- `GlyphAtlas::insert_dynamic_glyph` starts returning capacity failures in real use.
- Users see fallback glyphs after the dynamic area is full.
- Pre-rasterized atlas narrowing moves more glyphs to runtime insertion.
- Future fallback fonts or emoji paths require separate texture ownership.

## Files and Modules Likely Affected

- `crates/forge-renderer/src/font/atlas.rs`
- `crates/forge-renderer/src/font/rasterizer.rs`
- `crates/forge-renderer/src/grid_tessellator.rs`
- `crates/forge-renderer/src/pipeline.rs`
- `crates/forge-renderer/src/renderer.rs`
- `crates/forge-renderer/src/texture.rs`
- `crates/forge-renderer/src/shaders/grid.vert.glsl`
- `crates/forge-renderer/src/shaders/grid.frag.glsl`

Possible later dependencies:
- `crates/forge-main/src/main.rs`
- `crates/forge-core/src/config_registry.rs`
- fallback font or emoji modules if introduced later

## Recommended Design Direction

Start with multiple atlas pages and no eviction.

Avoid true eviction initially. Eviction requires tracking which rows reference which glyphs and invalidating/rebuilding affected cached row vertices when a glyph is removed. That adds high correctness risk. A capped multi-page cache with fallback on exhaustion is simpler, safer, and enough to prove the renderer model.

Recommended page model:
- Keep page 0 as the static atlas.
- Add dynamic pages as separate textures or a texture array.
- Keep ASCII/common glyphs non-evictable.
- Store page identity in glyph metadata.
- Batch or split draws by page if the shader cannot dynamically select a page cheaply.

## Implementation Steps

1. Add instrumentation before changing renderer architecture.
   - Count dynamic glyph insert attempts.
   - Count successful inserts.
   - Count capacity failures.
   - Count unique missing glyph keys per session.
   - Keep counters behind an environment flag or render stats structure.

2. Decide texture representation.
   - Option A: texture array with one descriptor and a page/layer index.
   - Option B: multiple sampled textures/descriptors and one draw per page.
   - Prefer texture arrays if the maximum page size can remain uniform.
   - Prefer multiple descriptors if pages need independent sizes or formats later.

3. Extend glyph metadata.
   - Add `atlas_page: u16` or equivalent to `GlyphMetrics`.
   - Keep existing UV coordinates page-local.
   - Ensure procedural glyphs still use negative UVs and do not require a page.

4. Extend vertex data or batching.
   - If using texture arrays, add a compact page/layer field to vertices or encode it safely in existing data only if precision is guaranteed.
   - If using multiple descriptors, keep vertices unchanged but group draw ranges by atlas page.
   - Avoid changing vertex format until the page strategy is decided.

5. Add page allocation.
   - Create an `AtlasPage` structure with texture ownership, slot cursor, dimensions, and glyph maps or page index.
   - Keep the static atlas page immutable.
   - Add dynamic pages only when the current dynamic page is full.
   - Cap page count with a conservative constant or config value.

6. Add GPU upload support for new pages.
   - Reuse `Texture::new` for page creation.
   - Reuse `Texture::update_region` for glyph insertion into existing pages.
   - Update descriptor sets safely when pages are added.
   - Wait idle only when required; avoid per-glyph global stalls if possible.

7. Add invalidation and rerender behavior.
   - New page insertion must trigger retessellation of rows that previously used fallback glyphs.
   - If no eviction is implemented, existing rows remain valid once a glyph exists.
   - If eviction is added later, row/glyph dependency tracking becomes mandatory.

8. Add fallback-on-cap behavior.
   - When page cap is reached, keep rendering fallback glyphs.
   - Log capacity exhaustion once per font/size/page cap combination.
   - Do not crash or stall the renderer.

9. Only then consider eviction.
   - Add LRU timestamps per glyph.
   - Never evict ASCII/common static glyphs.
   - Track row-to-glyph references or glyph-to-row references.
   - Dirty every affected row after eviction.
   - Validate heavily with resize, scroll reuse, selection, and cursor movement.

## Risks

- Descriptor layout changes can break Vulkan pipeline creation.
- Vertex format changes can break shader compatibility and alignment.
- Multiple draw batches can complicate row range metadata and future damage tracking.
- Eviction can create stale row vertices that sample the wrong glyph.
- Texture array support may have device limits or format constraints.
- Fallback fonts and emoji may need different raster formats than the current grayscale atlas.
- Incorrect page selection can produce visually random glyphs.

## Testing Strategy

Unit tests:
- Atlas page allocation.
- Insert until current page is full.
- Allocate next page.
- Respect page cap.
- Preserve page 0/static glyphs.
- Glyph metrics include correct page and UV values.

Renderer tests:
- Shader/pipeline creation with updated descriptor layout.
- Vertex layout size/alignment tests if changed.
- Texture update tests for non-zero page indices where possible.

Manual visual tests:
- Shell prompt with Nerd Font icons.
- `nvim`/NvChad dashboard and file tree.
- `btop` and `htop`.
- Unicode samples: Greek, Cyrillic, box drawing, braille, CJK if supported by font.
- Long session that forces dynamic page growth.
- Resize after dynamic page insertion.
- Scrollback and selection after page growth.

Performance tests:
- Startup time before/after.
- First-use latency for a page allocation.
- Memory usage per page.
- Frame time during many dynamic glyph insertions.

## Rollback Strategy

- Keep the current single-atlas dynamic slot path available until multi-page rendering is stable.
- Gate multi-atlas behind an internal constant or config/debug flag initially.
- Fall back to current single-atlas behavior if page allocation, descriptor update, or shader page selection fails.
- Do not remove the static atlas path.
- Avoid eviction in the first implementation so rollback does not need row dependency cleanup.

## Dependencies

- Dynamic glyph cache phase 1 must remain stable.
- Pre-rasterized atlas narrowing should be complete or at least understood.
- Renderer row caching and scroll reuse should be stable to make invalidation bugs easier to isolate.
- If texture arrays are chosen, device capability checks must be added.
- If multiple descriptor bindings are chosen, pipeline layout and draw batching design must be completed first.

## Expected Impact

Behavior impact:
- More Unicode and private-use glyphs can render without falling back after the first dynamic page fills.
- Future fallback fonts and emoji work gets a cleaner texture ownership model.

Performance impact:
- Startup can stay fast because rare glyphs do not need pre-rasterization.
- Runtime memory grows with actual glyph use instead of broad static ranges.
- First use of a new page may cost a texture allocation and descriptor update.
- If implemented as multiple draw batches, draw-call count can increase slightly but should remain modest for terminal workloads.

## Implementation Priority

Priority: P2, but only after evidence shows the current dynamic slot reserve is insufficient.

Do not implement eviction until non-evicting multi-page support is proven correct.
