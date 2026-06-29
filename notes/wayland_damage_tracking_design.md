# Wayland Damage Tracking Design

## Status

Wayland damage tracking is not safe to enable in the current Vulkan render path.

The renderer still clears and redraws the full swapchain image for each submitted frame. Because of
that, advertising partial surface damage would be misleading: the compositor could legally reuse
pixels outside the damaged region while Forge has not preserved those pixels in the newly presented
swapchain image.

This note is the completion point for optimization plan item 7 until the renderer has a true partial
redraw model.

## Current Code Evidence

- Startup SHM presentation uses full-buffer damage in
  `crates/forge-main/src/wayland/shm_buffer.rs::present`.
- Normal Vulkan frames do not call `wl_surface.damage_buffer`.
- `crates/forge-renderer/src/render_pass.rs::create_render_pass` configures the swapchain color
  attachment with `vk::AttachmentLoadOp::CLEAR` and `vk::ImageLayout::UNDEFINED`.
- `crates/forge-renderer/src/renderer.rs::render_grid` begins a full render pass, clears the full
  swapchain extent, sets a full-extent viewport/scissor, and draws the assembled vertex list.
- `GridTessellator` exposes row ranges and rebuilt-row metadata, but the submitted frame is still a
  full-image replacement.

## Why Partial Damage Is Unsafe Now

Wayland damage describes which parts of the newly attached buffer changed. It does not guarantee that
unchanged pixels outside the damaged region are copied from an older buffer.

With the current swapchain path:

1. A swapchain image is acquired.
2. The render pass starts from `UNDEFINED`.
3. The color attachment is cleared across the full render area.
4. Forge draws the terminal vertex list.
5. The image is presented.

If Forge sent damage for only the cursor row or dirty rows while the image contents outside that
region were not preserved, some compositors could show stale, cleared, or undefined pixels outside
the damage rectangle.

## Required Prerequisites

Partial Wayland damage should only be implemented after one of these renderer models exists:

1. **Full redraw with full damage**
   - Current behavior.
   - Safe, simple, and portable.
   - Does not reduce compositor work.

2. **Preserved swapchain image contents**
   - Render pass uses `LOAD_OP_LOAD` where valid.
   - Previous frame contents are preserved or copied into the acquired image.
   - Dirty regions are redrawn and damaged.
   - Requires careful handling of swapchain image rotation and undefined initial contents.

3. **Offscreen terminal surface cache**
   - Terminal is rendered into an owned offscreen image that Forge controls.
   - Dirty regions update the offscreen image.
   - Full or partial copy to swapchain happens with known contents.
   - More memory and complexity, but safer than assuming swapchain preservation.

4. **Scissored full-state redraw**
   - Renderer redraws only damaged rectangles, but source image contents outside those rectangles
     must still be valid.
   - This still requires preservation or an offscreen cache.

## Damage Rectangle Sources

Once the renderer can preserve unchanged pixels safely, damage can be computed from:

- `GridTessellator::rebuilt_rows`
- Cursor old/new rows
- Selection old/new rows
- Scrollbar overlay bounds
- Resize/full redraw events
- Font atlas updates
- Theme/background changes
- Alt-buffer enter/exit
- Scrollback viewport changes

Initial implementation should use row-aligned rectangles rather than cell-level rectangles:

- Simpler and less error-prone.
- Good match for row tessellation and cursor dirtiness.
- Avoids excessive `damage_buffer` calls.

## Guarded Implementation Plan

When prerequisites are met:

1. Add a config/debug switch with default `full` behavior:
   - `full`: no partial damage; whole surface is considered changed.
   - `row`: row-aligned damage rectangles.
   - `off`: disable explicit damage calls if needed for compositor compatibility.
2. Keep full damage for:
   - first Vulkan frame
   - swapchain recreation
   - resize
   - font updates
   - theme/background changes
   - alt-buffer transitions
   - scrollback viewport jumps
3. Start with row-aligned damage only for cursor blink and ordinary dirty rows.
4. Merge adjacent damaged rows into larger rectangles.
5. Use `damage_buffer` coordinates in buffer pixels, matching the swapchain extent.
6. If any stale pixels appear, immediately fall back to full damage.

## Compatibility Risks

- Wayland compositor damage behavior can vary.
- Vulkan swapchain images rotate; an acquired image may not contain the previous frame.
- Transparent window opacity and background clearing make stale pixels highly visible.
- Scrollbar overlay and cursor blink touch small regions but depend on the rest of the image being
  valid.
- Resize and fractional padding changes require full invalidation.

## Decision

Do not enable partial Wayland damage yet.

The correct next prerequisite is a renderer preservation model: either an offscreen terminal surface
cache or a proven swapchain-image preservation/copy strategy. Until then, Forge should continue using
the safe full-frame Vulkan presentation path.

