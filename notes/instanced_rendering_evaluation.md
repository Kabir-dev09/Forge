# Instanced Rendering Evaluation

## Status

Instanced rendering is not recommended as the next implementation step.

Forge should keep the current batched triangle-list renderer for now. Instancing can be reconsidered
only after render stats show vertex bandwidth or full-frame vertex submission is still a bottleneck
after the completed dirty-row, scroll-reuse, run-merge, persistent-buffer, and partial-upload work.

## Current Renderer Evidence

- `crates/forge-renderer/src/pipeline.rs::GlyphVertex` is a per-vertex format.
- `GlyphVertex::get_binding_description` uses `vk::VertexInputRate::VERTEX`.
- `crates/forge-renderer/src/shaders/grid.vert.glsl` passes per-vertex position, texture
  coordinate, foreground color, and background color through to the fragment shader.
- `crates/forge-renderer/src/renderer.rs::render_grid` uses one `cmd_draw` with
  `instance_count = 1`.
- `GridTessellator` already merges adjacent same-background cells into fewer background quads.
- Partial vertex upload now copies only changed compatible row ranges into each in-flight vertex
  buffer region.

## Potential Benefit

The current `GlyphVertex` is 48 bytes:

- position: 2 floats
- texture coordinate: 2 floats
- foreground color: 4 floats
- background color: 4 floats

A normal quad uses 6 vertices, so a full quad costs about 288 bytes of vertex data before upload.

An instanced design could draw a static 6-vertex unit quad and store per-quad data as one instance.
That could reduce upload bandwidth for full rebuilds, especially on large windows with many visible
glyphs.

## Why It Is Not the Best Next Step

Instancing would not be a small renderer change. It would require:

- a new instance struct
- a second vertex binding with `vk::VertexInputRate::INSTANCE`
- vertex shader changes to compute quad corners from `gl_VertexIndex`
- careful preservation of procedural glyph local coordinates
- updated row range metadata based on instances instead of vertices
- updated partial upload tracking
- updated tests for box drawing, block elements, braille, powerline glyphs, cursor quads, selection,
  scrollbar overlay, and background runs

The current renderer has already reduced the original cost in safer ways:

- row-level tessellation reuse
- scroll-row reuse
- background run merging
- persistent mapped buffer
- per-frame partial vertex upload
- dynamic atlas narrowing

Because of those changes, the remaining benefit of instancing is uncertain without profiling data.

## Main Risks

- Procedural glyph rendering depends on interpolated texture coordinates and negative procedural IDs.
- Box drawing, braille, powerline shapes, scrollbar pills, and solid backgrounds all share the same
  vertex path today.
- A vertex-format change can easily break shader alignment or Vulkan pipeline creation.
- Row range and partial upload logic would need to be rewritten around instance ranges.
- Draw order must remain exactly the same: backgrounds, scrollbar overlay, foregrounds.

## Recommendation

Do not implement instanced rendering now.

Keep the current batched quad renderer until profiling shows one of these facts:

1. Full-frame vertex upload remains a major CPU cost.
2. Vertex bandwidth remains high after partial upload.
3. GPU vertex processing is measurable as a bottleneck.
4. Large-window fast-output workloads still regress after the safer optimizations.

If those are proven, implement instancing as a separate renderer experiment behind a feature flag or
debug switch, not as an immediate replacement.

## Suggested Future Prototype

If profiling justifies it later:

1. Add a `GlyphInstance` struct.
2. Keep `GlyphVertex` path as the fallback.
3. Build an instance-only pipeline beside the current pipeline.
4. Use a static unit-quad vertex buffer.
5. Encode per-instance pixel/NDC rect, UV rect/procedural ID, foreground, and background.
6. Draw backgrounds, scrollbar, and foregrounds as separate instance ranges.
7. Compare:
   - bytes uploaded
   - CPU frame time
   - GPU frame time
   - visual correctness in shell, `nvim`, `btop`, `tmux`, box drawing, braille, and Nerd Font output

## Decision

Instanced rendering is a valid future optimization, but it is not currently worth implementing.
The existing renderer should remain unchanged until render stats prove that vertex bandwidth is still
a real bottleneck.
