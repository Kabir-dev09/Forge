use super::font::{
    atlas::{
        DynamicGlyphInsertResult, GlyphAtlas, GlyphKey, ShapedGlyphInsertResult, ShapedGlyphKey,
    },
    rasterizer::FontRasterizer,
    shaper::ShaperCache,
};
use super::grid_tessellator::{
    ContextMenuRenderData, GridTessellator, LigatureRenderContext, PixelRect, RowVertexRanges,
    StatusbarHoverRenderData, VertexRange,
};
use super::{
    device::*, framebuffer::*, instance::*, pipeline::*, surface::*, swapchain::*, sync::*,
    texture::*,
};
use ash::{vk, Device, Entry, Instance};
use forge_core::{config_registry::LigatureConfig, ForgeError, Result};
use std::collections::{HashMap, HashSet};
use std::ptr;

const MIN_VERTEX_CAPACITY: usize = 100_000;
const VERTICES_PER_CELL_BUDGET: usize = 18;
const EXTRA_VERTEX_BUDGET: usize = 2_048;
const RENDER_STATS_LOG_INTERVAL: u64 = 120;
const VERTEX_BUFFER_REGION_ALIGNMENT: vk::DeviceSize = 256;
const DYNAMIC_GLYPHS_PER_FRAME: usize = 16;

fn estimate_vertex_capacity(width: u32, height: u32, cell_width: u32, cell_height: u32) -> usize {
    let cols = (width as f64 / cell_width.max(1) as f64).ceil() as usize;
    let rows = (height as f64 / cell_height.max(1) as f64).ceil() as usize;
    (cols * rows * VERTICES_PER_CELL_BUDGET + EXTRA_VERTEX_BUDGET).max(MIN_VERTEX_CAPACITY)
}

fn align_device_size(value: vk::DeviceSize, alignment: vk::DeviceSize) -> vk::DeviceSize {
    debug_assert!(alignment.is_power_of_two());
    (value + alignment - 1) & !(alignment - 1)
}

fn outline_segments_around_gap(
    start: f32,
    length: f32,
    gap_start: f32,
    gap_length: f32,
) -> [(f32, f32); 2] {
    if length <= 0.0 {
        return [(start, 0.0), (start, 0.0)];
    }
    if gap_length <= 0.0 {
        return [(start, length), (start + length, 0.0)];
    }

    let end = start + length;
    let gap_start = gap_start.clamp(start, end);
    let gap_end = (gap_start + gap_length).clamp(start, end);
    [
        (start, (gap_start - start).max(0.0)),
        (gap_end, (end - gap_end).max(0.0)),
    ]
}

fn append_segmented_horizontal_outline(
    tessellator: &mut GridTessellator,
    vp_w: f32,
    vp_h: f32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
    gap: Option<(f32, f32)>,
) {
    let segments = if let Some((gap_start, gap_width)) = gap {
        outline_segments_around_gap(x, width, gap_start, gap_width)
    } else {
        [(x, width), (x + width, 0.0)]
    };
    for (segment_x, segment_width) in segments {
        if segment_width > 0.0 {
            tessellator.append_solid_rect(
                vp_w,
                vp_h,
                PixelRect::new(segment_x, y, segment_width, height),
                color,
            );
        }
    }
}

fn append_segmented_vertical_outline(
    tessellator: &mut GridTessellator,
    vp_w: f32,
    vp_h: f32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
    gap: Option<(f32, f32)>,
) {
    let segments = if let Some((gap_start, gap_height)) = gap {
        outline_segments_around_gap(y, height, gap_start, gap_height)
    } else {
        [(y, height), (y + height, 0.0)]
    };
    for (segment_y, segment_height) in segments {
        if segment_height > 0.0 {
            tessellator.append_solid_rect(
                vp_w,
                vp_h,
                PixelRect::new(x, segment_y, width, segment_height),
                color,
            );
        }
    }
}

fn vertex_region_size(max_vertices: usize) -> vk::DeviceSize {
    let vertex_bytes = (max_vertices * std::mem::size_of::<GlyphVertex>()) as vk::DeviceSize;
    align_device_size(vertex_bytes, VERTEX_BUFFER_REGION_ALIGNMENT)
}

fn vertex_buffer_size(max_vertices: usize) -> vk::DeviceSize {
    vertex_region_size(max_vertices) * MAX_FRAMES_IN_FLIGHT as vk::DeviceSize
}

fn rect_to_scissor(rect: PaneRenderRect, extent: vk::Extent2D) -> Option<vk::Rect2D> {
    if !rect.has_positive_area() {
        return None;
    }

    let x0 = rect.x.floor().max(0.0).min(extent.width as f32) as i32;
    let y0 = rect.y.floor().max(0.0).min(extent.height as f32) as i32;
    let x1 = (rect.x + rect.width)
        .ceil()
        .max(0.0)
        .min(extent.width as f32) as i32;
    let y1 = (rect.y + rect.height)
        .ceil()
        .max(0.0)
        .min(extent.height as f32) as i32;

    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    Some(vk::Rect2D {
        offset: vk::Offset2D { x: x0, y: y0 },
        extent: vk::Extent2D {
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
        },
    })
}

fn command_indicator_text_clip(
    popup_rect: PaneRenderRect,
    dot_center_x: f32,
    dot_size: f32,
    text_x: f32,
    cell_width: f32,
) -> Option<PaneRenderRect> {
    if popup_rect.width <= popup_rect.height + 0.5 {
        return None;
    }

    let dot_gap = cell_width * 0.25;
    let left = popup_rect
        .x
        .max(text_x)
        .max(dot_center_x + dot_size * 0.5 + dot_gap);
    let right = popup_rect.x + popup_rect.width;
    (right > left).then(|| {
        PaneRenderRect::new(
            left,
            popup_rect.y,
            right - left,
            popup_rect.height,
        )
    })
}

fn full_scissor(extent: vk::Extent2D) -> vk::Rect2D {
    vk::Rect2D {
        offset: vk::Offset2D { x: 0, y: 0 },
        extent,
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderFrameStats {
    pub dirty_generations: usize,
    pub vertices: usize,
    pub bytes_uploaded: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaneRenderId(pub u64);

impl PaneRenderId {
    fn is_synthetic(self) -> bool {
        self.0 >= u64::MAX - 4096
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaneRenderRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl PaneRenderRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn has_positive_area(self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }
}

pub struct PaneRenderInput<'a> {
    pub pane_id: PaneRenderId,
    pub rect: PaneRenderRect,
    pub opacity: f32,
    pub layer: PaneRenderLayer,
    pub apply_pane_padding: bool,
    pub grid: &'a [&'a [forge_core::cell::Cell]],
    pub dirty_generations: &'a [u64],
    pub cursor: Option<(usize, usize)>,
    pub cursor_style: forge_core::config_registry::CursorStyle,
    pub cursor_visible_phase: bool,
    pub selection: Option<forge_core::cell::SelectionRange>,
    pub default_bg: [f32; 4],
    pub cursor_color: [f32; 4],
    pub selection_bg: [f32; 4],
    pub viewport_offset: f64,
    pub scroll_event: Option<super::grid_tessellator::ScrollEvent>,
    pub scroll_id: u64,
    pub is_active: bool,
    pub overflow_indicators: PaneOverflowIndicators,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommandCompletionIndicatorRenderData {
    pub rect: PaneRenderRect,
    pub cell_width: f32,
    pub cell_height: f32,
    pub content_x: f32,
    pub content_y: f32,
    pub dot_center_x: f32,
    pub corner_radius: f32,
    pub background_color: Option<[f32; 4]>,
    pub dot_color: [f32; 4],
    pub text_color: [f32; 4],
    pub failure_color: [f32; 4],
    pub command: std::sync::Arc<str>,
    pub exit_text: Option<std::sync::Arc<str>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaneOverflowIndicators {
    pub above: bool,
    pub below: bool,
    pub left: bool,
    pub right: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaneRenderLayer {
    #[default]
    Normal,
    AfterModalDim,
    Floating,
    Modal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitBorderRenderInput {
    pub rect: PaneRenderRect,
    pub color: [f32; 4],
}

fn materialize_split_divider(rect: PaneRenderRect, thickness: f32) -> Option<PaneRenderRect> {
    let thickness = thickness.max(1.0);
    if rect.width <= 0.0 && rect.height > 0.0 {
        return Some(PaneRenderRect::new(
            rect.x - thickness * 0.5,
            rect.y,
            thickness,
            rect.height,
        ));
    }
    if rect.height <= 0.0 && rect.width > 0.0 {
        return Some(PaneRenderRect::new(
            rect.x,
            rect.y - thickness * 0.5,
            rect.width,
            thickness,
        ));
    }
    rect.has_positive_area().then_some(rect)
}

pub fn adjacent_pane_divider(
    first: PaneRenderRect,
    second: PaneRenderRect,
    max_horizontal_gap: f32,
    max_vertical_gap: f32,
) -> Option<PaneRenderRect> {
    let overlap_top = first.y.max(second.y);
    let overlap_bottom = (first.y + first.height).min(second.y + second.height);
    if overlap_bottom > overlap_top {
        let (left, right) = if first.x <= second.x {
            (first, second)
        } else {
            (second, first)
        };
        let gap = right.x - (left.x + left.width);
        if gap >= -f32::EPSILON && gap <= max_horizontal_gap + f32::EPSILON {
            return Some(PaneRenderRect::new(
                left.x + left.width + gap.max(0.0) * 0.5,
                overlap_top,
                0.0,
                overlap_bottom - overlap_top,
            ));
        }
    }

    let overlap_left = first.x.max(second.x);
    let overlap_right = (first.x + first.width).min(second.x + second.width);
    if overlap_right > overlap_left {
        let (top, bottom) = if first.y <= second.y {
            (first, second)
        } else {
            (second, first)
        };
        let gap = bottom.y - (top.y + top.height);
        if gap >= -f32::EPSILON && gap <= max_vertical_gap + f32::EPSILON {
            return Some(PaneRenderRect::new(
                overlap_left,
                top.y + top.height + gap.max(0.0) * 0.5,
                overlap_right - overlap_left,
                0.0,
            ));
        }
    }

    None
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub frames_submitted: u64,
    pub dirty_generations: u64,
    pub vertices_uploaded: u64,
    pub bytes_uploaded: u64,
    pub dynamic_glyph_attempts: u64,
    pub dynamic_glyph_insertions: u64,
    pub dynamic_glyph_already_present: u64,
    pub dynamic_glyph_capacity_failures: u64,
    pub dynamic_glyph_missing_from_fonts: u64,
    pub last_frame: RenderFrameStats,
}

#[derive(Clone, Debug, Default)]
struct FrameVertexUploadState {
    vertex_count: usize,
    row_ranges: Vec<RowVertexRanges>,
    row_generations: Vec<u64>,
    scrollbar_range: Option<VertexRange>,
    context_menu_range: Option<VertexRange>,
    context_menu_fingerprint: u64,
    initialized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RenderDrawBatch {
    start: usize,
    count: usize,
    scissor: vk::Rect2D,
    is_opaque: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum VertexUploadPlan {
    Full,
    Partial(Vec<VertexRange>),
}

fn plan_vertex_upload_for_state(
    state: Option<&FrameVertexUploadState>,
    vertex_count: usize,
    row_ranges: &[RowVertexRanges],
    scrollbar_range: Option<VertexRange>,
    context_menu_range: Option<VertexRange>,
    context_menu_fingerprint: u64,
) -> VertexUploadPlan {
    let Some(state) = state else {
        return VertexUploadPlan::Full;
    };

    if !state.initialized
        || state.vertex_count != vertex_count
        || state.row_ranges.len() != row_ranges.len()
        || state.row_generations.len() != row_ranges.len()
        || state.scrollbar_range != scrollbar_range
        || state.context_menu_range != context_menu_range
        || state.context_menu_fingerprint != context_menu_fingerprint
    {
        return VertexUploadPlan::Full;
    }

    let ranges_compatible = state
        .row_ranges
        .iter()
        .zip(row_ranges)
        .all(|(old, new)| old.bg == new.bg && old.fg == new.fg);
    if !ranges_compatible {
        return VertexUploadPlan::Full;
    }

    let mut ranges = Vec::new();
    for (row_idx, row_range) in row_ranges.iter().enumerate() {
        if state.row_generations[row_idx] == row_range.generation {
            continue;
        }
        if row_range.bg.count > 0 {
            ranges.push(row_range.bg);
        }
        if row_range.fg.count > 0 {
            ranges.push(row_range.fg);
        }
    }

    if let Some(scrollbar_range) = scrollbar_range {
        ranges.push(scrollbar_range);
    }
    if let Some(context_menu_range) = context_menu_range {
        ranges.push(context_menu_range);
    }

    VertexUploadPlan::Partial(ranges)
}

/// Represents the full Vulkan rendering stack.
/// Note: This struct contains raw pointers (`ash` handles) and is inherently `!Send` and `!Sync`.
/// Do not manually implement `Send` unless you architect a strictly thread-safe Vulkan command submission layer.
/// For now, all rendering strictly runs on the main thread.
pub struct Renderer {
    pub entry: Entry,
    pub instance: Instance,
    pub surface_loader: ash::khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,
    pub graphics_queue: vk::Queue,
    pub present_queue: vk::Queue,
    pub queue_indices: QueueFamilyIndices,
    pub swapchain: Swapchain,
    pub render_pass: vk::RenderPass,
    pub pipeline: Pipeline,
    pub framebuffers: Vec<vk::Framebuffer>,
    pub command_pool: vk::CommandPool,
    pub command_buffers: Vec<vk::CommandBuffer>,
    pub sync: SyncPrimitives,
    pub current_frame: usize,

    pub atlas: GlyphAtlas,
    pub atlas_texture: Texture,
    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_set: vk::DescriptorSet,

    pub tessellator: GridTessellator,
    pane_tessellators: HashMap<PaneRenderId, GridTessellator>,
    pub vertex_buffer: vk::Buffer,
    pub vertex_memory: vk::DeviceMemory,
    pub vertex_mapped_ptr: *mut std::ffi::c_void,
    pub vertex_region_size: vk::DeviceSize,
    pub max_vertices: usize,
    pub render_stats: Option<RenderStats>,
    frame_upload_states: Vec<FrameVertexUploadState>,
    reported_missing_glyphs: HashSet<GlyphKey>,
    unsupported_dynamic_glyphs: HashSet<GlyphKey>,
    unsupported_shaped_glyphs: HashSet<ShapedGlyphKey>,
    dynamic_atlas_full_reported: bool,
    shaped_atlas_full_reported: bool,
    ligature_config: LigatureConfig,
    ligature_shaper: ShaperCache,
    font_rasterizer: Option<FontRasterizer>,
    bold_font_rasterizer: Option<FontRasterizer>,
    italic_font_rasterizer: Option<FontRasterizer>,
    bold_italic_font_rasterizer: Option<FontRasterizer>,
    fallback_font_rasterizers: Vec<FontRasterizer>,
    font_px_size: f32,

    pub cell_width: u32,
    pub cell_height: u32,
    pub baseline: u32,
}

impl Renderer {
    fn plan_vertex_upload(&self, frame: usize) -> VertexUploadPlan {
        plan_vertex_upload_for_state(
            self.frame_upload_states.get(frame),
            self.tessellator.vertices.len(),
            &self.tessellator.row_ranges,
            self.tessellator.scrollbar_range,
            self.tessellator.context_menu_range,
            self.tessellator.context_menu_fingerprint,
        )
    }

    fn update_frame_upload_state(&mut self, frame: usize) {
        if self.frame_upload_states.len() < MAX_FRAMES_IN_FLIGHT {
            self.frame_upload_states
                .resize_with(MAX_FRAMES_IN_FLIGHT, FrameVertexUploadState::default);
        }

        let state = &mut self.frame_upload_states[frame];
        state.vertex_count = self.tessellator.vertices.len();
        state.row_ranges.clear();
        state
            .row_ranges
            .extend_from_slice(&self.tessellator.row_ranges);
        state.row_generations.clear();
        state.row_generations.extend(
            self.tessellator
                .row_ranges
                .iter()
                .map(|ranges| ranges.generation),
        );
        state.scrollbar_range = self.tessellator.scrollbar_range;
        state.context_menu_range = self.tessellator.context_menu_range;
        state.context_menu_fingerprint = self.tessellator.context_menu_fingerprint;
        state.initialized = true;
    }

    unsafe fn copy_vertex_range_to_frame(
        &self,
        frame_offset: vk::DeviceSize,
        range: VertexRange,
    ) -> usize {
        if range.count == 0 {
            return 0;
        }

        let vertex_size = std::mem::size_of::<GlyphVertex>();
        let dst_byte_offset = frame_offset as usize + range.start * vertex_size;
        let data_ptr = (self.vertex_mapped_ptr as *mut u8).add(dst_byte_offset) as *mut GlyphVertex;
        ptr::copy_nonoverlapping(
            self.tessellator.vertices.as_ptr().add(range.start),
            data_ptr,
            range.count,
        );
        range.count * vertex_size
    }

    fn insert_dynamic_glyphs(&mut self, keys: &[GlyphKey]) -> Result<bool> {
        let Some(rasterizer) = self.font_rasterizer.as_ref() else {
            return Ok(false);
        };
        let mut inserted = false;
        let mut attempts = 0u64;
        let mut insertions = 0u64;
        let mut already_present = 0u64;
        let mut capacity_failures = 0u64;

        let mut missing_from_fonts = 0u64;

        let keys_to_insert: Vec<GlyphKey> = keys
            .iter()
            .copied()
            .filter(|key| !self.unsupported_dynamic_glyphs.contains(key))
            .take(DYNAMIC_GLYPHS_PER_FRAME)
            .collect();

        let mut updates_to_apply = Vec::new();

        for key in keys_to_insert {
            attempts += 1;
            match self.atlas.insert_dynamic_glyph(
                key,
                rasterizer,
                self.bold_font_rasterizer.as_ref(),
                self.italic_font_rasterizer.as_ref(),
                self.bold_italic_font_rasterizer.as_ref(),
                &self.fallback_font_rasterizers,
                self.font_px_size,
            ) {
                DynamicGlyphInsertResult::Inserted(update) => {
                    insertions += 1;
                    if let Some(update) = update {
                        updates_to_apply.push(update);
                    }
                    inserted = true;
                }
                DynamicGlyphInsertResult::AlreadyPresent => {
                    already_present += 1;
                    inserted = true;
                }
                DynamicGlyphInsertResult::AtlasFull => {
                    capacity_failures += 1;
                    if !self.dynamic_atlas_full_reported {
                        let msg = "WARNING: Dynamic glyph atlas is full! Some glyphs will be missing. Please restart the terminal or use a font with more coverage.";
                        eprintln!("{}", msg);
                        tracing::warn!("{}", msg);
                        self.dynamic_atlas_full_reported = true;
                    }
                    break;
                }
                DynamicGlyphInsertResult::Missing => {
                    missing_from_fonts += 1;
                    self.unsupported_dynamic_glyphs.insert(key);
                    tracing::debug!(
                        char = %key.c,
                        codepoint = format_args!("U+{:04X}", key.c as u32),
                        style = ?key.style,
                        "Glyph is missing from configured and fallback fonts"
                    );
                }
            }
        }

        if !updates_to_apply.is_empty() {
            let regions: Vec<super::texture::TextureRegion> = updates_to_apply
                .iter()
                .map(|u| super::texture::TextureRegion {
                    x: u.x,
                    y: u.y,
                    width: u.width,
                    height: u.height,
                    pixels: &u.pixels,
                })
                .collect();

            self.atlas_texture.update_regions(
                &self.instance,
                self.physical_device,
                &self.device,
                self.command_pool,
                self.graphics_queue,
                &regions,
            )?;
        }

        if let Some(stats) = self.render_stats.as_mut() {
            stats.dynamic_glyph_attempts += attempts;
            stats.dynamic_glyph_insertions += insertions;
            stats.dynamic_glyph_already_present += already_present;
            stats.dynamic_glyph_capacity_failures += capacity_failures;
            stats.dynamic_glyph_missing_from_fonts += missing_from_fonts;
        }

        if attempts > 0 {
            tracing::debug!(
                attempts,
                insertions,
                already_present,
                capacity_failures,
                missing_from_fonts,
                used_slots = self.atlas.dynamic_slots_used(),
                remaining_slots = self.atlas.dynamic_slots_remaining(),
                "Dynamic glyph insertion batch complete"
            );
        }

        Ok(inserted)
    }

    fn insert_shaped_glyphs(&mut self, keys: &[ShapedGlyphKey]) -> Result<bool> {
        let Some(rasterizer) = self.font_rasterizer.as_ref() else {
            return Ok(false);
        };
        let keys_to_insert: Vec<ShapedGlyphKey> = keys
            .iter()
            .copied()
            .filter(|key| !self.unsupported_shaped_glyphs.contains(key))
            .take(DYNAMIC_GLYPHS_PER_FRAME)
            .collect();

        let mut inserted = false;
        let mut updates_to_apply = Vec::new();
        for key in keys_to_insert {
            match self.atlas.insert_shaped_glyph(
                key,
                rasterizer,
                self.bold_font_rasterizer.as_ref(),
                self.italic_font_rasterizer.as_ref(),
                self.bold_italic_font_rasterizer.as_ref(),
                self.font_px_size,
            ) {
                ShapedGlyphInsertResult::Inserted(update) => {
                    if let Some(update) = update {
                        updates_to_apply.push(update);
                    }
                    inserted = true;
                }
                ShapedGlyphInsertResult::AlreadyPresent => {
                    inserted = true;
                }
                ShapedGlyphInsertResult::AtlasFull => {
                    if !self.shaped_atlas_full_reported {
                        let msg = "WARNING: Dynamic glyph atlas is full; some shaped ligatures will fall back.";
                        eprintln!("{}", msg);
                        tracing::warn!("{}", msg);
                        self.shaped_atlas_full_reported = true;
                    }
                    break;
                }
                ShapedGlyphInsertResult::Missing => {
                    self.unsupported_shaped_glyphs.insert(key);
                }
            }
        }

        if !updates_to_apply.is_empty() {
            let regions: Vec<super::texture::TextureRegion> = updates_to_apply
                .iter()
                .map(|u| super::texture::TextureRegion {
                    x: u.x,
                    y: u.y,
                    width: u.width,
                    height: u.height,
                    pixels: &u.pixels,
                })
                .collect();

            self.atlas_texture.update_regions(
                &self.instance,
                self.physical_device,
                &self.device,
                self.command_pool,
                self.graphics_queue,
                &regions,
            )?;
        }

        Ok(inserted)
    }

    pub fn set_ligature_config(&mut self, mut config: LigatureConfig) {
        config.normalize();
        if self.ligature_config == config {
            return;
        }
        self.ligature_config = config;
        self.ligature_shaper
            .set_max_entries(self.ligature_config.cache_entries);
        self.ligature_shaper.clear();
    }

    pub fn has_real_font_metrics(&self) -> bool {
        self.font_rasterizer.is_some()
    }

    fn create_mapped_vertex_buffer(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &Device,
        max_vertices: usize,
    ) -> Result<(
        vk::Buffer,
        vk::DeviceMemory,
        *mut std::ffi::c_void,
        vk::DeviceSize,
    )> {
        let region_size = vertex_region_size(max_vertices);
        let buffer_size = vertex_buffer_size(max_vertices);
        let (buffer, memory) = super::texture::create_buffer(
            instance,
            physical_device,
            device,
            buffer_size,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        let mapped_ptr = unsafe {
            match device.map_memory(memory, 0, buffer_size, vk::MemoryMapFlags::empty()) {
                Ok(ptr) => ptr,
                Err(e) => {
                    device.destroy_buffer(buffer, None);
                    device.free_memory(memory, None);
                    return Err(ForgeError::Vulkan(e.to_string()));
                }
            }
        };

        Ok((buffer, memory, mapped_ptr, region_size))
    }

    fn record_render_stats(&mut self, frame_stats: RenderFrameStats) {
        let Some(stats) = self.render_stats.as_mut() else {
            return;
        };

        stats.frames_submitted += 1;
        stats.dirty_generations += frame_stats.dirty_generations as u64;
        stats.vertices_uploaded += frame_stats.vertices as u64;
        stats.bytes_uploaded += frame_stats.bytes_uploaded as u64;
        stats.last_frame = frame_stats;

        if stats.frames_submitted == 1 || stats.frames_submitted % RENDER_STATS_LOG_INTERVAL == 0 {
            tracing::info!(
                frames_submitted = stats.frames_submitted,
                total_dirty_generations = stats.dirty_generations,
                total_vertices_uploaded = stats.vertices_uploaded,
                total_bytes_uploaded = stats.bytes_uploaded,
                last_dirty_generations = stats.last_frame.dirty_generations,
                last_vertices = stats.last_frame.vertices,
                last_bytes_uploaded = stats.last_frame.bytes_uploaded,
                dynamic_glyph_attempts = stats.dynamic_glyph_attempts,
                dynamic_glyph_insertions = stats.dynamic_glyph_insertions,
                dynamic_glyph_already_present = stats.dynamic_glyph_already_present,
                dynamic_glyph_capacity_failures = stats.dynamic_glyph_capacity_failures,
                dynamic_glyph_missing_from_fonts = stats.dynamic_glyph_missing_from_fonts,
                "Forge render stats"
            );
        }
    }

    fn ensure_vertex_capacity(&mut self, required_vertices: usize) -> Result<()> {
        if required_vertices <= self.max_vertices {
            return Ok(());
        }

        let grown_capacity = self.max_vertices + (self.max_vertices / 2);
        let new_capacity = required_vertices.max(grown_capacity);
        tracing::debug!(
            old_vertices = self.max_vertices,
            required_vertices,
            new_vertices = new_capacity,
            "Growing renderer vertex buffer"
        );

        unsafe {
            // TODO(PERF-05): `device_wait_idle` causes a GPU stall on vertex buffer grow.
            // Implement a double-buffered or deferred-free vertex buffer system to avoid blocking.
            self.device
                .device_wait_idle()
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
            self.device.unmap_memory(self.vertex_memory);
            self.device.destroy_buffer(self.vertex_buffer, None);
            self.device.free_memory(self.vertex_memory, None);
        }

        let (new_buffer, new_memory, new_mapped_ptr, new_region_size) =
            Self::create_mapped_vertex_buffer(
                &self.instance,
                self.physical_device,
                &self.device,
                new_capacity,
            )?;
        self.vertex_buffer = new_buffer;
        self.vertex_memory = new_memory;
        self.vertex_mapped_ptr = new_mapped_ptr;
        self.vertex_region_size = new_region_size;
        self.max_vertices = new_capacity;
        self.frame_upload_states.clear();
        self.frame_upload_states
            .resize_with(MAX_FRAMES_IN_FLIGHT, FrameVertexUploadState::default);
        Ok(())
    }

    /// Creates the full Vulkan rendering stack.
    /// `wl_display` and `wl_surface` are raw pointers from wayland-client objects.
    pub fn new(
        wl_display: *mut std::ffi::c_void,
        wl_surface: *mut std::ffi::c_void,
        width: u32,
        height: u32,
        cell_width: u32,
        cell_height: u32,
        baseline: u32,
    ) -> Result<Self> {
        let vk_start = std::time::Instant::now();
        let entry = create_entry()?;
        log_instance_extensions(&entry);

        let t_inst = std::time::Instant::now();
        let instance = create_instance(&entry)?;
        tracing::debug!("[PROFILER] create_instance took: {:?}", t_inst.elapsed());

        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
        let surface = create_wayland_surface(&entry, &instance, wl_display, wl_surface)?;

        let t_phys = std::time::Instant::now();
        let (physical_device, queue_indices) =
            select_physical_device(&instance, surface, &surface_loader)?;
        tracing::debug!(
            "[PROFILER] select_physical_device took: {:?}",
            t_phys.elapsed()
        );

        let t_log = std::time::Instant::now();
        let (device, graphics_queue, present_queue) =
            create_logical_device(&instance, physical_device, &queue_indices)?;
        tracing::debug!(
            "[PROFILER] create_logical_device took: {:?}",
            t_log.elapsed()
        );

        let t_swap = std::time::Instant::now();
        let surface_details = SurfaceDetails::query(&surface_loader, physical_device, surface)?;
        let swapchain = Swapchain::new(
            &instance,
            &device,
            surface,
            &surface_details,
            &queue_indices,
            width,
            height,
        )?;
        tracing::debug!("[PROFILER] Swapchain::new took: {:?}", t_swap.elapsed());

        let t_pipe = std::time::Instant::now();
        let render_pass = super::render_pass::create_render_pass(&device, swapchain.format)?;
        let pipeline = Pipeline::new(&device, render_pass)?;
        tracing::debug!("[PROFILER] Pipeline::new took: {:?}", t_pipe.elapsed());

        let framebuffers = create_framebuffers(
            &device,
            render_pass,
            &swapchain.image_views,
            swapchain.extent,
        )?;

        let command_pool = create_command_pool(&device, queue_indices.graphics)?;
        let command_buffers =
            allocate_command_buffers(&device, command_pool, MAX_FRAMES_IN_FLIGHT as u32)?;

        let sync = SyncPrimitives::new(&device)?;
        tracing::info!("Vulkan Initialization took: {:?}", vk_start.elapsed());

        // Font and Atlas (Dummy initialization for fast boot)
        let mut atlas = GlyphAtlas {
            atlas_width: 1,
            atlas_height: 1,
            pixels: vec![255], // 1x1 solid white pixel
            glyphs: std::collections::HashMap::new(),
            glyphs_bold: std::collections::HashMap::new(),
            glyphs_italic: std::collections::HashMap::new(),
            glyphs_bold_italic: std::collections::HashMap::new(),
            shaped_glyphs: std::collections::HashMap::new(),
            descriptor: super::font::atlas::GlyphAtlasDescriptor::dummy(),
            font_cell_width: cell_width,
            font_cell_height: cell_height,
            font_baseline: baseline,
            atlas_cell_width: 1,
            atlas_cell_height: 1,
            next_dynamic_slot: 1,
            total_slots: 1,
        };
        // Add a dummy ' ' glyph so the renderer doesn't panic if it looks something up
        atlas.glyphs.insert(
            ' ',
            super::font::atlas::GlyphMetrics {
                u0: 0.0,
                v0: 0.0,
                u1: 1.0,
                v1: 1.0,
                width: 0,
                height: 0,
                bearing_x: 0,
                bearing_y: 0,
            },
        );

        let atlas_texture = Texture::new(
            &instance,
            physical_device,
            &device,
            command_pool,
            graphics_queue,
            1,
            1,
            &atlas.pixels,
        )?;
        atlas.clear_pixels(); // We don't need the RAM copy anymore!

        // Descriptor Pool
        let pool_sizes = [vk::DescriptorPoolSize {
            ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            descriptor_count: 1,
        }];
        let pool_info = vk::DescriptorPoolCreateInfo {
            pool_size_count: pool_sizes.len() as u32,
            p_pool_sizes: pool_sizes.as_ptr(),
            max_sets: 1,
            ..Default::default()
        };
        let descriptor_pool = unsafe {
            device
                .create_descriptor_pool(&pool_info, None)
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?
        };

        let layouts = [pipeline.descriptor_set_layout];
        let alloc_info = vk::DescriptorSetAllocateInfo {
            descriptor_pool,
            descriptor_set_count: 1,
            p_set_layouts: layouts.as_ptr(),
            ..Default::default()
        };
        let descriptor_set = unsafe {
            device
                .allocate_descriptor_sets(&alloc_info)
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?[0]
        };

        // Write descriptor set
        let image_info = vk::DescriptorImageInfo {
            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            image_view: atlas_texture.view,
            sampler: atlas_texture.sampler,
        };
        let write_desc = vk::WriteDescriptorSet {
            dst_set: descriptor_set,
            dst_binding: 0,
            dst_array_element: 0,
            descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            descriptor_count: 1,
            p_image_info: &image_info,
            ..Default::default()
        };
        unsafe { device.update_descriptor_sets(&[write_desc], &[]) };

        // Vertex Buffer
        let max_vertices = estimate_vertex_capacity(width, height, cell_width, cell_height);
        let (vertex_buffer, vertex_memory, vertex_mapped_ptr, vertex_region_size) =
            Self::create_mapped_vertex_buffer(&instance, physical_device, &device, max_vertices)?;

        let tessellator = GridTessellator::new(max_vertices / 12);
        let render_stats = if std::env::var_os("FORGE_RENDER_STATS").is_some() {
            Some(RenderStats::default())
        } else {
            None
        };

        tracing::info!("Vulkan renderer initialized.");

        Ok(Self {
            entry,
            instance,
            surface_loader,
            surface,
            physical_device,
            device,
            graphics_queue,
            present_queue,
            queue_indices,
            swapchain,
            render_pass,
            pipeline,
            framebuffers,
            command_pool,
            command_buffers,
            sync,
            current_frame: 0,
            atlas,
            atlas_texture,
            descriptor_pool,
            descriptor_set,
            tessellator,
            pane_tessellators: HashMap::new(),
            vertex_buffer,
            vertex_memory,
            vertex_mapped_ptr,
            vertex_region_size,
            max_vertices,
            render_stats,
            frame_upload_states: vec![FrameVertexUploadState::default(); MAX_FRAMES_IN_FLIGHT],
            reported_missing_glyphs: HashSet::new(),
            unsupported_dynamic_glyphs: HashSet::new(),
            unsupported_shaped_glyphs: HashSet::new(),
            dynamic_atlas_full_reported: false,
            shaped_atlas_full_reported: false,
            ligature_config: LigatureConfig::default(),
            ligature_shaper: ShaperCache::default(),
            font_rasterizer: None,
            bold_font_rasterizer: None,
            italic_font_rasterizer: None,
            bold_italic_font_rasterizer: None,
            fallback_font_rasterizers: Vec::new(),
            font_px_size: baseline as f32,
            cell_width,
            cell_height,
            baseline,
        })
    }

    pub fn update_font_size(&mut self, px_size: f32) -> Result<()> {
        let _span = tracing::trace_span!("renderer.update_font_size", size = px_size).entered();
        if (self.font_px_size - px_size).abs() < 0.1 {
            return Ok(());
        }

        if let Some(r) = self.font_rasterizer.as_mut() {
            r.update_size(px_size)?;
            self.cell_width = r.cell_width;
            self.cell_height = r.cell_height;
            self.baseline = r.baseline;
        }

        if let Some(r) = self.bold_font_rasterizer.as_mut() {
            r.update_size(px_size)?;
        }
        if let Some(r) = self.italic_font_rasterizer.as_mut() {
            r.update_size(px_size)?;
        }
        if let Some(r) = self.bold_italic_font_rasterizer.as_mut() {
            r.update_size(px_size)?;
        }

        for r in self.fallback_font_rasterizers.iter_mut() {
            r.update_size(px_size)?;
        }

        self.font_px_size = px_size;
        self.ligature_shaper.clear();
        self.unsupported_shaped_glyphs.clear();
        self.shaped_atlas_full_reported = false;

        if let Some(r) = self.font_rasterizer.as_ref() {
            self.atlas = GlyphAtlas::build(
                r,
                self.bold_font_rasterizer.as_ref(),
                self.italic_font_rasterizer.as_ref(),
                self.bold_italic_font_rasterizer.as_ref(),
                px_size,
                false, // Full build in fast time since rasterization is already done
            )?;

            // Destroy old texture properly
            self.atlas_texture.destroy(&self.device);

            // Create new texture
            self.atlas_texture = Texture::new(
                &self.instance,
                self.physical_device,
                &self.device,
                self.command_pool,
                self.graphics_queue,
                self.atlas.atlas_width,
                self.atlas.atlas_height,
                &self.atlas.pixels,
            )?;

            // Update descriptor set
            let image_info = vk::DescriptorImageInfo {
                image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                image_view: self.atlas_texture.view,
                sampler: self.atlas_texture.sampler,
            };
            let write_desc = vk::WriteDescriptorSet {
                dst_set: self.descriptor_set,
                dst_binding: 0,
                dst_array_element: 0,
                descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                descriptor_count: 1,
                p_image_info: &image_info,
                ..Default::default()
            };
            unsafe { self.device.update_descriptor_sets(&[write_desc], &[]) };
        }

        // Invalidate vertex caches so everything gets redrawn with new metrics
        for state in &mut self.frame_upload_states {
            state.initialized = false;
        }

        self.tessellator.scrollbar_range = None;
        self.tessellator.context_menu_range = None;

        Ok(())
    }

    pub fn render_clear(&mut self, clear_color: [f32; 4]) -> Result<bool> {
        let _span = tracing::trace_span!(
            "renderer.render_clear",
            width = self.swapchain.extent.width,
            height = self.swapchain.extent.height
        )
        .entered();
        super::frame::render_frame(
            &self.device,
            &self.swapchain,
            self.render_pass,
            &self.framebuffers,
            &self.command_buffers,
            &self.sync,
            self.graphics_queue,
            self.present_queue,
            &mut self.current_frame,
            clear_color,
        )
    }

    fn submit_tessellated_vertices(
        &mut self,
        clear_color: [f32; 4],
        effective_cell_w: f32,
        effective_cell_h: f32,
        braille_style: forge_core::config_registry::BrailleStyle,
        dirty_generations: usize,
        draw_batches: &[RenderDrawBatch],
    ) -> Result<bool> {
        tracing::trace!(
            vertices = self.tessellator.vertices.len(),
            bytes = self.tessellator.vertices.len() * std::mem::size_of::<GlyphVertex>(),
            "Renderer tessellation output"
        );
        self.ensure_vertex_capacity(self.tessellator.vertices.len())?;

        let frame = self.current_frame;
        let vertex_buffer_offset = self.vertex_region_size * frame as vk::DeviceSize;

        unsafe {
            let _fence_span =
                tracing::trace_span!("renderer.wait_for_frame_fence", frame = frame).entered();
            self.device
                .wait_for_fences(&[self.sync.in_flight_fences[frame]], true, u64::MAX)
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
        }

        let upload_plan = if draw_batches.is_empty() {
            self.plan_vertex_upload(frame)
        } else {
            VertexUploadPlan::Full
        };
        let mut bytes_uploaded = 0usize;

        if !self.tessellator.vertices.is_empty() {
            let _upload_span = tracing::trace_span!(
                "renderer.upload_vertices",
                vertices = self.tessellator.vertices.len(),
                offset = vertex_buffer_offset,
                plan = match &upload_plan {
                    VertexUploadPlan::Full => "full",
                    VertexUploadPlan::Partial(_) => "partial",
                }
            )
            .entered();
            unsafe {
                match &upload_plan {
                    VertexUploadPlan::Full => {
                        let data_size = (self.tessellator.vertices.len()
                            * std::mem::size_of::<GlyphVertex>())
                            as vk::DeviceSize;
                        debug_assert!(data_size <= self.vertex_region_size);
                        let data_ptr = (self.vertex_mapped_ptr as *mut u8)
                            .add(vertex_buffer_offset as usize)
                            as *mut GlyphVertex;
                        ptr::copy_nonoverlapping(
                            self.tessellator.vertices.as_ptr(),
                            data_ptr,
                            self.tessellator.vertices.len(),
                        );
                        bytes_uploaded = data_size as usize;
                    }
                    VertexUploadPlan::Partial(ranges) => {
                        for range in ranges {
                            bytes_uploaded +=
                                self.copy_vertex_range_to_frame(vertex_buffer_offset, *range);
                        }
                    }
                }
            }
        }

        if draw_batches.is_empty() {
            self.update_frame_upload_state(frame);
        } else if let Some(state) = self.frame_upload_states.get_mut(frame) {
            state.initialized = false;
        }

        let frame_stats = RenderFrameStats {
            dirty_generations,
            vertices: self.tessellator.vertices.len(),
            bytes_uploaded,
        };

        let (image_index, suboptimal) = unsafe {
            let _acquire_span =
                tracing::trace_span!("renderer.acquire_next_image", frame = frame).entered();
            match self.swapchain.loader.acquire_next_image(
                self.swapchain.handle,
                u64::MAX,
                self.sync.image_available_semaphores[frame],
                vk::Fence::null(),
            ) {
                Ok(result) => result,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return Ok(true),
                Err(vk::Result::ERROR_SURFACE_LOST_KHR) => {
                    return Err(ForgeError::Vulkan("Surface lost".to_string()))
                }
                Err(e) => {
                    return Err(ForgeError::Vulkan(format!(
                        "acquire_next_image failed: {}",
                        e
                    )))
                }
            }
        };

        if image_index as usize >= self.framebuffers.len() {
            return Err(ForgeError::Vulkan("Image index out of bounds".to_string()));
        }

        unsafe {
            self.device
                .reset_fences(&[self.sync.in_flight_fences[frame]])
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
        }

        let cmd = self.command_buffers[frame];
        unsafe {
            self.device
                .reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;

            let begin_info = vk::CommandBufferBeginInfo {
                flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
                ..Default::default()
            };
            self.device
                .begin_command_buffer(cmd, &begin_info)
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;

            let clear_value = vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: clear_color,
                },
            };
            let render_pass_begin = vk::RenderPassBeginInfo {
                render_pass: self.render_pass,
                framebuffer: self.framebuffers[image_index as usize],
                render_area: full_scissor(self.swapchain.extent),
                clear_value_count: 1,
                p_clear_values: &clear_value,
                ..Default::default()
            };

            self.device
                .cmd_begin_render_pass(cmd, &render_pass_begin, vk::SubpassContents::INLINE);

            if !self.tessellator.vertices.is_empty() {
                self.device.cmd_bind_pipeline(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipeline.graphics_pipeline,
                );

                let config_flags = match braille_style {
                    forge_core::config_registry::BrailleStyle::Solid => 1,
                    forge_core::config_registry::BrailleStyle::Dots => 0,
                };
                let pc = crate::pipeline::PushConstants {
                    cell_size: [effective_cell_w, effective_cell_h],
                    config_flags,
                    _pad: 0,
                };
                self.device.cmd_push_constants(
                    cmd,
                    self.pipeline.pipeline_layout,
                    vk::ShaderStageFlags::FRAGMENT,
                    0,
                    bytemuck::bytes_of(&pc),
                );

                self.device.cmd_bind_vertex_buffers(
                    cmd,
                    0,
                    &[self.vertex_buffer],
                    &[vertex_buffer_offset],
                );
                self.device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipeline.pipeline_layout,
                    0,
                    &[self.descriptor_set],
                    &[],
                );

                let viewport = vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: self.swapchain.extent.width as f32,
                    height: self.swapchain.extent.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                };
                self.device.cmd_set_viewport(cmd, 0, &[viewport]);

                if draw_batches.is_empty() {
                    let scissor = full_scissor(self.swapchain.extent);
                    self.device.cmd_set_scissor(cmd, 0, &[scissor]);
                    self.device
                        .cmd_draw(cmd, self.tessellator.vertices.len() as u32, 1, 0, 0);
                } else {
                    let mut current_is_opaque = false;
                    for batch in draw_batches {
                        if batch.count == 0 {
                            continue;
                        }
                        if batch.is_opaque != current_is_opaque {
                            current_is_opaque = batch.is_opaque;
                            let pipeline = if current_is_opaque {
                                self.pipeline.opaque_pipeline
                            } else {
                                self.pipeline.graphics_pipeline
                            };
                            self.device.cmd_bind_pipeline(
                                cmd,
                                vk::PipelineBindPoint::GRAPHICS,
                                pipeline,
                            );
                        }
                        self.device.cmd_set_scissor(cmd, 0, &[batch.scissor]);
                        self.device
                            .cmd_draw(cmd, batch.count as u32, 1, batch.start as u32, 0);
                    }
                }
            }

            self.device.cmd_end_render_pass(cmd);
            self.device
                .end_command_buffer(cmd)
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
        }

        let wait_semaphores = [self.sync.image_available_semaphores[frame]];
        let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let signal_semaphores = [self.sync.render_finished_semaphores[frame]];
        let submit_info = vk::SubmitInfo {
            wait_semaphore_count: 1,
            p_wait_semaphores: wait_semaphores.as_ptr(),
            p_wait_dst_stage_mask: wait_stages.as_ptr(),
            command_buffer_count: 1,
            p_command_buffers: &cmd,
            signal_semaphore_count: 1,
            p_signal_semaphores: signal_semaphores.as_ptr(),
            ..Default::default()
        };
        unsafe {
            let _submit_span = tracing::trace_span!(
                "renderer.queue_submit",
                frame = frame,
                image_index = image_index
            )
            .entered();
            self.device
                .queue_submit(
                    self.graphics_queue,
                    &[submit_info],
                    self.sync.in_flight_fences[frame],
                )
                .map_err(|e| ForgeError::Vulkan(format!("queue_submit failed: {}", e)))?;
        }

        let swapchains = [self.swapchain.handle];
        let image_indices = [image_index];
        let present_info = vk::PresentInfoKHR {
            wait_semaphore_count: 1,
            p_wait_semaphores: signal_semaphores.as_ptr(),
            swapchain_count: 1,
            p_swapchains: swapchains.as_ptr(),
            p_image_indices: image_indices.as_ptr(),
            ..Default::default()
        };
        let needs_recreate = unsafe {
            let _present_span = tracing::trace_span!(
                "renderer.queue_present",
                frame = frame,
                image_index = image_index
            )
            .entered();
            match self
                .swapchain
                .loader
                .queue_present(self.present_queue, &present_info)
            {
                Ok(true) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => true,
                Ok(false) => suboptimal,
                Err(vk::Result::ERROR_SURFACE_LOST_KHR) => {
                    return Err(ForgeError::Vulkan("Surface lost".to_string()))
                }
                Err(e) => return Err(ForgeError::Vulkan(format!("queue_present failed: {}", e))),
            }
        };

        self.current_frame = (frame + 1) % MAX_FRAMES_IN_FLIGHT;
        self.record_render_stats(frame_stats);
        Ok(needs_recreate)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_grid(
        &mut self,
        grid: &[&[forge_core::cell::Cell]],
        dirty_generations: &[u64],
        cursor: Option<(usize, usize)>,
        cursor_style: forge_core::config_registry::CursorStyle,
        cursor_visible_phase: bool,
        selection: Option<forge_core::cell::SelectionRange>,
        default_bg: [f32; 4],
        clear_color: [f32; 4],
        cursor_color: [f32; 4],
        selection_bg: [f32; 4],
        pad_x: f32,
        pad_y: f32,
        effective_cell_w: f32,
        effective_cell_h: f32,
        _scale_x: f32,
        _scale_y: f32,
        scrollbar: Option<(f32, f32, f32, f32, f32, f32)>,
        context_menu: Option<ContextMenuRenderData<'_>>,
        context_menu_transparent: bool,
        scroll_event: Option<super::grid_tessellator::ScrollEvent>,
        braille_style: forge_core::config_registry::BrailleStyle,
    ) -> Result<bool> {
        let _span = tracing::trace_span!(
            "renderer.render_grid",
            rows = grid.len(),
            cols = grid.first().map(|row| row.len()).unwrap_or(0),
            dirty_generations = dirty_generations.iter().filter(|&&dirty| dirty > 0).count(),
            width = self.swapchain.extent.width,
            height = self.swapchain.extent.height
        )
        .entered();

        let ligatures_enabled = self.ligature_config.enabled && self.font_rasterizer.is_some();
        let mut shaper = std::mem::take(&mut self.ligature_shaper);
        {
            let mut ligature_context = if ligatures_enabled {
                Some(LigatureRenderContext {
                    config: &self.ligature_config,
                    shaper: &mut shaper,
                    rasterizer: self.font_rasterizer.as_ref().unwrap(),
                    bold_rasterizer: self.bold_font_rasterizer.as_ref(),
                    italic_rasterizer: self.italic_font_rasterizer.as_ref(),
                    bold_italic_rasterizer: self.bold_italic_font_rasterizer.as_ref(),
                    px_size: self.font_px_size,
                })
            } else {
                None
            };
            self.tessellator.tessellate(
                grid,
                dirty_generations,
                &self.atlas,
                effective_cell_w,
                effective_cell_h,
                self.cell_width as f32,
                self.cell_height as f32,
                self.baseline as f32,
                self.swapchain.extent.width as f32,
                self.swapchain.extent.height as f32,
                default_bg,
                cursor_color,
                cursor,
                cursor_style,
                cursor_visible_phase,
                selection,
                selection_bg,
                pad_x,
                pad_y,
                scrollbar,
                context_menu,
                context_menu_transparent,
                scroll_event,
                0,
                None,
                ligature_context.as_mut(),
            );
        }
        self.ligature_shaper = shaper;
        let missing_glyphs: Vec<GlyphKey> =
            self.tessellator.missing_glyphs().iter().copied().collect();
        let missing_shaped_glyphs: Vec<ShapedGlyphKey> = self
            .tessellator
            .missing_shaped_glyphs()
            .iter()
            .copied()
            .collect();
        let inserted_dynamic_glyphs = self.insert_dynamic_glyphs(&missing_glyphs)?;
        let inserted_shaped_glyphs = self.insert_shaped_glyphs(&missing_shaped_glyphs)?;
        let atlas_changed = inserted_dynamic_glyphs || inserted_shaped_glyphs;
        if atlas_changed {
            let all_dirty = vec![1; grid.len()];
            let mut shaper = std::mem::take(&mut self.ligature_shaper);
            let mut ligature_context = if ligatures_enabled {
                Some(LigatureRenderContext {
                    config: &self.ligature_config,
                    shaper: &mut shaper,
                    rasterizer: self.font_rasterizer.as_ref().unwrap(),
                    bold_rasterizer: self.bold_font_rasterizer.as_ref(),
                    italic_rasterizer: self.italic_font_rasterizer.as_ref(),
                    bold_italic_rasterizer: self.bold_italic_font_rasterizer.as_ref(),
                    px_size: self.font_px_size,
                })
            } else {
                None
            };
            self.tessellator.tessellate(
                grid,
                &all_dirty,
                &self.atlas,
                effective_cell_w,
                effective_cell_h,
                self.cell_width as f32,
                self.cell_height as f32,
                self.baseline as f32,
                self.swapchain.extent.width as f32,
                self.swapchain.extent.height as f32,
                default_bg,
                cursor_color,
                cursor,
                cursor_style,
                cursor_visible_phase,
                selection,
                selection_bg,
                pad_x,
                pad_y,
                scrollbar,
                context_menu,
                context_menu_transparent,
                None,
                0,
                None,
                ligature_context.as_mut(),
            );
            self.ligature_shaper = shaper;
        }
        for key in self.tessellator.missing_glyphs() {
            if self.reported_missing_glyphs.insert(*key) {
                tracing::trace!(
                    char = %key.c,
                    codepoint = format_args!("U+{:04X}", key.c as u32),
                    style = ?key.style,
                    "Glyph missing from current atlas; fallback glyph used"
                );
            }
        }
        self.submit_tessellated_vertices(
            clear_color,
            effective_cell_w,
            effective_cell_h,
            braille_style,
            dirty_generations.iter().filter(|&&dirty| dirty > 0).count(),
            &[],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_panes(
        &mut self,
        panes: &[PaneRenderInput<'_>],
        split_borders: &[SplitBorderRenderInput],
        clear_color: [f32; 4],
        effective_cell_w: f32,
        effective_cell_h: f32,
        scrollbar: Option<(f32, f32, f32, f32, f32, f32)>,
        context_menu: Option<ContextMenuRenderData<'_>>,
        context_menu_transparent: bool,
        braille_style: forge_core::config_registry::BrailleStyle,
        gap: f32,
        outline_width: f32,
        active_outline_color: [f32; 4],
        inactive_outline_color: [f32; 4],
        pane_padding: forge_core::config_registry::PaddingConfig,
        modal_dim_color: Option<[f32; 4]>,
        command_completion_indicator: Option<CommandCompletionIndicatorRenderData>,
        statusbar_hover: Option<StatusbarHoverRenderData>,
    ) -> Result<bool> {
        let _span = tracing::trace_span!(
            "renderer.render_panes",
            panes = panes.len(),
            split_borders = split_borders.len(),
            width = self.swapchain.extent.width,
            height = self.swapchain.extent.height
        )
        .entered();

        let active_ids: HashSet<_> = panes.iter().map(|pane| pane.pane_id).collect();
        let has_real_panes = panes.iter().any(|pane| !pane.pane_id.is_synthetic());
        if has_real_panes {
            self.pane_tessellators
                .retain(|pane_id, _| active_ids.contains(pane_id));
        }

        let vp_w = self.swapchain.extent.width as f32;
        let vp_h = self.swapchain.extent.height as f32;

        let ligatures_enabled = self.ligature_config.enabled && self.font_rasterizer.is_some();
        let mut shaper = std::mem::take(&mut self.ligature_shaper);
        {
            let mut ligature_context = if ligatures_enabled {
                Some(LigatureRenderContext {
                    config: &self.ligature_config,
                    shaper: &mut shaper,
                    rasterizer: self.font_rasterizer.as_ref().unwrap(),
                    bold_rasterizer: self.bold_font_rasterizer.as_ref(),
                    italic_rasterizer: self.italic_font_rasterizer.as_ref(),
                    bold_italic_rasterizer: self.bold_italic_font_rasterizer.as_ref(),
                    px_size: self.font_px_size,
                })
            } else {
                None
            };
            for pane in panes {
                let origin_x = if pane.apply_pane_padding {
                    pane.rect.x + pane_padding.left as f32
                } else {
                    pane.rect.x
                };
                let origin_y = if pane.apply_pane_padding {
                    pane.rect.y + pane_padding.top as f32
                } else {
                    pane.rect.y
                };
                let tessellator = self
                    .pane_tessellators
                    .entry(pane.pane_id)
                    .or_insert_with(|| GridTessellator::new(self.max_vertices / 12));
                tessellator.tessellate(
                    pane.grid,
                    pane.dirty_generations,
                    &self.atlas,
                    effective_cell_w,
                    effective_cell_h,
                    self.cell_width as f32,
                    self.cell_height as f32,
                    self.baseline as f32,
                    vp_w,
                    vp_h,
                    pane.default_bg,
                    pane.cursor_color,
                    pane.cursor,
                    pane.cursor_style,
                    pane.cursor_visible_phase,
                    pane.selection,
                    pane.selection_bg,
                    origin_x,
                    origin_y,
                    None,
                    None,
                    false,
                    pane.scroll_event,
                    pane.scroll_id,
                    None,
                    ligature_context.as_mut(),
                );
            }
        }
        self.ligature_shaper = shaper;

        let missing_glyphs: Vec<GlyphKey> = self
            .pane_tessellators
            .iter()
            .filter(|(pane_id, _)| active_ids.contains(pane_id))
            .flat_map(|(_, tessellator)| tessellator.missing_glyphs().iter().copied())
            .collect();
        let missing_shaped_glyphs: Vec<ShapedGlyphKey> = self
            .pane_tessellators
            .iter()
            .filter(|(pane_id, _)| active_ids.contains(pane_id))
            .flat_map(|(_, tessellator)| tessellator.missing_shaped_glyphs().iter().copied())
            .collect();
        let inserted_dynamic_glyphs = self.insert_dynamic_glyphs(&missing_glyphs)?;
        let inserted_shaped_glyphs = self.insert_shaped_glyphs(&missing_shaped_glyphs)?;
        if inserted_dynamic_glyphs || inserted_shaped_glyphs {
            let mut shaper = std::mem::take(&mut self.ligature_shaper);
            let mut ligature_context = if ligatures_enabled {
                Some(LigatureRenderContext {
                    config: &self.ligature_config,
                    shaper: &mut shaper,
                    rasterizer: self.font_rasterizer.as_ref().unwrap(),
                    bold_rasterizer: self.bold_font_rasterizer.as_ref(),
                    italic_rasterizer: self.italic_font_rasterizer.as_ref(),
                    bold_italic_rasterizer: self.bold_italic_font_rasterizer.as_ref(),
                    px_size: self.font_px_size,
                })
            } else {
                None
            };
            for pane in panes {
                let all_dirty = vec![1; pane.grid.len()];
                if let Some(tessellator) = self.pane_tessellators.get_mut(&pane.pane_id) {
                    let origin_x = if pane.apply_pane_padding {
                        pane.rect.x + pane_padding.left as f32
                    } else {
                        pane.rect.x
                    };
                    let origin_y = if pane.apply_pane_padding {
                        pane.rect.y + pane_padding.top as f32
                    } else {
                        pane.rect.y
                    };
                    tessellator.tessellate(
                        pane.grid,
                        &all_dirty,
                        &self.atlas,
                        effective_cell_w,
                        effective_cell_h,
                        self.cell_width as f32,
                        self.cell_height as f32,
                        self.baseline as f32,
                        vp_w,
                        vp_h,
                        pane.default_bg,
                        pane.cursor_color,
                        pane.cursor,
                        pane.cursor_style,
                        pane.cursor_visible_phase,
                        pane.selection,
                        pane.selection_bg,
                        origin_x,
                        origin_y,
                        None,
                        None,
                        false,
                        None,
                        0,
                        None,
                        ligature_context.as_mut(),
                    );
                }
            }
            self.ligature_shaper = shaper;
        }

        let pane_missing_glyphs: Vec<GlyphKey> = self
            .pane_tessellators
            .iter()
            .filter(|(pane_id, _)| active_ids.contains(pane_id))
            .flat_map(|(_, tessellator)| tessellator.missing_glyphs().iter().copied())
            .collect();

        self.tessellator.prepare_composite_frame();
        let mut draw_batches = Vec::with_capacity(panes.len() + 1);

        let mut after_modal_dim_batches = Vec::new();
        let mut floating_batches = Vec::new();
        let mut modal_batches = Vec::new();
        for pane in panes {
            let Some(scissor) = rect_to_scissor(pane.rect, self.swapchain.extent) else {
                continue;
            };
            let Some(tessellator) = self.pane_tessellators.get(&pane.pane_id) else {
                continue;
            };
            if pane.layer == PaneRenderLayer::AfterModalDim {
                after_modal_dim_batches.push((pane.pane_id, scissor));
                continue;
            }
            if pane.layer == PaneRenderLayer::Floating {
                floating_batches.push((pane, scissor));
                continue;
            }
            if pane.layer == PaneRenderLayer::Modal {
                modal_batches.push((pane.pane_id, scissor));
                continue;
            }
            let start = self.tessellator.vertices.len();
            if pane.opacity < 1.0 {
                let mut faded_vertices = tessellator.vertices.clone();
                for v in &mut faded_vertices {
                    v.fg_color[3] *= pane.opacity;
                    v.bg_color[3] *= pane.opacity;
                }
                self.tessellator
                    .vertices
                    .extend_from_slice(&faded_vertices);
            } else {
                self.tessellator
                    .vertices
                    .extend_from_slice(&tessellator.vertices);
            }
            let count = self.tessellator.vertices.len() - start;
            if count > 0 {
                draw_batches.push(RenderDrawBatch {
                    start,
                    count,
                    scissor,
                    is_opaque: false,
                });
            }
        }

        let overlay_start = self.tessellator.vertices.len();

        if gap <= 0.0 {
            for border in split_borders {
                let color = border.color;
                let color = if color[3] > 0.0 {
                    color
                } else {
                    inactive_outline_color
                };
                if let Some(rect) = materialize_split_divider(border.rect, outline_width) {
                    self.tessellator.append_solid_rect(
                        vp_w,
                        vp_h,
                        PixelRect::new(rect.x, rect.y, rect.width, rect.height),
                        color,
                    );
                }
            }
        } else if outline_width > 0.0 {
            let num_real_panes = panes.iter().filter(|p| !p.pane_id.is_synthetic() && p.layer != PaneRenderLayer::Floating).count();
            if num_real_panes > 1 {
                for pane in panes {
                    if pane.pane_id.is_synthetic() || pane.layer == PaneRenderLayer::Floating {
                        continue;
                    }
                    let rect = pane.rect;
                    let current_border_color = if pane.is_active {
                        active_outline_color
                    } else {
                        inactive_outline_color
                    };
                    let w = outline_width;
                    let horizontal_gap_width = effective_cell_w * 3.0;
                    let vertical_gap_height = effective_cell_h;
                    let horizontal_gap_x =
                        rect.x + ((rect.width - horizontal_gap_width) * 0.5).max(0.0);
                    let vertical_gap_y =
                        rect.y + ((rect.height - vertical_gap_height) * 0.5).max(0.0);
                    append_segmented_horizontal_outline(
                        &mut self.tessellator,
                        vp_w,
                        vp_h,
                        rect.x,
                        rect.y - w,
                        rect.width,
                        w,
                        current_border_color,
                        pane.overflow_indicators
                            .above
                            .then_some((horizontal_gap_x, horizontal_gap_width)),
                    );
                    append_segmented_horizontal_outline(
                        &mut self.tessellator,
                        vp_w,
                        vp_h,
                        rect.x,
                        rect.y + rect.height,
                        rect.width,
                        w,
                        current_border_color,
                        pane.overflow_indicators
                            .below
                            .then_some((horizontal_gap_x, horizontal_gap_width)),
                    );
                    append_segmented_vertical_outline(
                        &mut self.tessellator,
                        vp_w,
                        vp_h,
                        rect.x - w,
                        rect.y - w,
                        w,
                        rect.height + w * 2.0,
                        current_border_color,
                        pane.overflow_indicators
                            .left
                            .then_some((vertical_gap_y, vertical_gap_height)),
                    );
                    append_segmented_vertical_outline(
                        &mut self.tessellator,
                        vp_w,
                        vp_h,
                        rect.x + rect.width,
                        rect.y - w,
                        w,
                        rect.height + w * 2.0,
                        current_border_color,
                        pane.overflow_indicators
                            .right
                            .then_some((vertical_gap_y, vertical_gap_height)),
                    );
                }
            }
        }
        self.tessellator
            .append_statusbar_hover_overlay(vp_w, vp_h, statusbar_hover);
        self.tessellator
            .append_scrollbar_overlay(vp_w, vp_h, scrollbar);
        if let Some(color) = modal_dim_color.filter(|color| color[3] > 0.0) {
            self.tessellator.append_solid_rect(
                vp_w,
                vp_h,
                PixelRect::new(0.0, 0.0, vp_w, vp_h),
                color,
            );
        }
        for (pane_id, _scissor) in after_modal_dim_batches {
            let Some(tessellator) = self.pane_tessellators.get(&pane_id) else {
                continue;
            };
            self.tessellator
                .vertices
                .extend_from_slice(&tessellator.vertices);
        }
        let overlay_count = self.tessellator.vertices.len() - overlay_start;
        if overlay_count > 0 {
            draw_batches.push(RenderDrawBatch {
                start: overlay_start,
                count: overlay_count,
                scissor: full_scissor(self.swapchain.extent),
                is_opaque: false,
            });
        }

        for (pane, scissor) in floating_batches {
            let Some(tessellator) = self.pane_tessellators.get(&pane.pane_id) else {
                continue;
            };
            let start_bg = self.tessellator.vertices.len();
            self.tessellator.append_solid_rect(
                vp_w,
                vp_h,
                PixelRect::new(
                    pane.rect.x,
                    pane.rect.y,
                    pane.rect.width,
                    pane.rect.height,
                ),
                clear_color,
            );
            let count_bg = self.tessellator.vertices.len() - start_bg;
            if count_bg > 0 {
                draw_batches.push(RenderDrawBatch {
                    start: start_bg,
                    count: count_bg,
                    scissor,
                    is_opaque: true,
                });
            }

            let start = self.tessellator.vertices.len();
            if pane.opacity < 1.0 {
                let mut faded_vertices = tessellator.vertices.clone();
                for v in &mut faded_vertices {
                    v.fg_color[3] *= pane.opacity;
                    v.bg_color[3] *= pane.opacity;
                }
                self.tessellator
                    .vertices
                    .extend_from_slice(&faded_vertices);
            } else {
                self.tessellator
                    .vertices
                    .extend_from_slice(&tessellator.vertices);
            }
            let count = self.tessellator.vertices.len() - start;
            if count > 0 {
                draw_batches.push(RenderDrawBatch {
                    start,
                    count,
                    scissor,
                    is_opaque: false,
                });
            }

            if outline_width > 0.0 && !pane.pane_id.is_synthetic() {
                let rect = pane.rect;
                let current_border_color = if pane.is_active {
                    active_outline_color
                } else {
                    inactive_outline_color
                };
                let w = outline_width;
                let horizontal_gap_width = effective_cell_w * 3.0;
                let vertical_gap_height = effective_cell_h;
                let horizontal_gap_x = rect.x + ((rect.width - horizontal_gap_width) * 0.5).max(0.0);
                let vertical_gap_y = rect.y + ((rect.height - vertical_gap_height) * 0.5).max(0.0);

                let start_border = self.tessellator.vertices.len();
                append_segmented_horizontal_outline(
                    &mut self.tessellator,
                    vp_w, vp_h,
                    rect.x, rect.y - w, rect.width, w, current_border_color,
                    pane.overflow_indicators.above.then_some((horizontal_gap_x, horizontal_gap_width)),
                );
                append_segmented_horizontal_outline(
                    &mut self.tessellator,
                    vp_w, vp_h,
                    rect.x, rect.y + rect.height, rect.width, w, current_border_color,
                    pane.overflow_indicators.below.then_some((horizontal_gap_x, horizontal_gap_width)),
                );
                append_segmented_vertical_outline(
                    &mut self.tessellator,
                    vp_w, vp_h,
                    rect.x - w, rect.y - w, w, rect.height + w * 2.0, current_border_color,
                    pane.overflow_indicators.left.then_some((vertical_gap_y, vertical_gap_height)),
                );
                append_segmented_vertical_outline(
                    &mut self.tessellator,
                    vp_w, vp_h,
                    rect.x + rect.width, rect.y - w, w, rect.height + w * 2.0, current_border_color,
                    pane.overflow_indicators.right.then_some((vertical_gap_y, vertical_gap_height)),
                );
                let count_border = self.tessellator.vertices.len() - start_border;
                if count_border > 0 {
                    draw_batches.push(RenderDrawBatch {
                        start: start_border,
                        count: count_border,
                        scissor: full_scissor(self.swapchain.extent),
                        is_opaque: false,
                    });
                }
            }
        }

        for (pane_id, scissor) in modal_batches {
            let Some(tessellator) = self.pane_tessellators.get(&pane_id) else {
                continue;
            };
            let start = self.tessellator.vertices.len();
            self.tessellator
                .vertices
                .extend_from_slice(&tessellator.vertices);
            let count = self.tessellator.vertices.len() - start;
            if count > 0 {
                draw_batches.push(RenderDrawBatch {
                    start,
                    count,
                    scissor,
                    is_opaque: false,
                });
            }
        }

        if let Some(indicator) = command_completion_indicator {
            let popup_scissor = rect_to_scissor(indicator.rect, self.swapchain.extent);
            let chrome_start = self.tessellator.vertices.len();
            if let Some(background) = indicator.background_color {
                self.tessellator.append_rounded_rect(
                    vp_w,
                    vp_h,
                    PixelRect::new(
                        indicator.rect.x,
                        indicator.rect.y,
                        indicator.rect.width,
                        indicator.rect.height,
                    ),
                    indicator.corner_radius,
                    background,
                );
            }
            let cell_h = indicator.cell_height;
            let cell_w = indicator.cell_width.max(1.0);
            let dot_size = cell_h * 0.5;
            let dot_padding = ((cell_h - dot_size) * 0.5).min(1.0);
            let dot_x = indicator.dot_center_x - dot_size * 0.5;
            let dot_y = indicator.content_y + (cell_h - dot_size) * 0.5;
            self.tessellator.append_circle(
                vp_w,
                vp_h,
                dot_x,
                dot_y,
                dot_size,
                dot_padding,
                indicator.dot_color,
            );
            let text_x = indicator.content_x + cell_w * 2.0;
            let chrome_count = self.tessellator.vertices.len() - chrome_start;
            if chrome_count > 0 {
                if let Some(scissor) = popup_scissor {
                    draw_batches.push(RenderDrawBatch {
                        start: chrome_start,
                        count: chrome_count,
                        scissor,
                        is_opaque: false,
                    });
                }
            }

            if let Some(text_clip) = command_indicator_text_clip(
                indicator.rect,
                indicator.dot_center_x,
                dot_size,
                text_x,
                cell_w,
            ) {
                let text_start = self.tessellator.vertices.len();
                self.tessellator.append_overlay_text(
                    &self.atlas,
                    vp_w,
                    vp_h,
                    text_x,
                    indicator.content_y,
                    cell_w,
                    cell_h,
                    indicator.command.as_ref(),
                    indicator.text_color,
                );
                if let Some(exit_text) = indicator.exit_text.as_deref() {
                    let exit_x = text_x + indicator.command.chars().count() as f32 * cell_w;
                    self.tessellator.append_overlay_text(
                        &self.atlas,
                        vp_w,
                        vp_h,
                        exit_x,
                        indicator.content_y,
                        cell_w,
                        cell_h,
                        exit_text,
                        indicator.failure_color,
                    );
                }
                let text_count = self.tessellator.vertices.len() - text_start;
                if text_count > 0 {
                    if let Some(scissor) = rect_to_scissor(text_clip, self.swapchain.extent) {
                        draw_batches.push(RenderDrawBatch {
                            start: text_start,
                            count: text_count,
                            scissor,
                            is_opaque: false,
                        });
                    }
                }
            }
        }

        let cm_start = self.tessellator.vertices.len();
        self.tessellator.append_context_menu_overlay(
            &self.atlas,
            self.baseline as f32,
            effective_cell_w,
            effective_cell_h,
            vp_w,
            vp_h,
            context_menu,
            context_menu_transparent,
        );
        let cm_count = self.tessellator.vertices.len() - cm_start;
        if cm_count > 0 {
            draw_batches.push(RenderDrawBatch {
                start: cm_start,
                count: cm_count,
                scissor: full_scissor(self.swapchain.extent),
                is_opaque: false,
            });
        }

        for key in pane_missing_glyphs
            .iter()
            .chain(self.tessellator.missing_glyphs().iter())
        {
            if self.reported_missing_glyphs.insert(*key) {
                tracing::trace!(
                    char = %key.c,
                    codepoint = format_args!("U+{:04X}", key.c as u32),
                    style = ?key.style,
                    "Glyph missing from current atlas; fallback glyph used"
                );
            }
        }

        self.submit_tessellated_vertices(
            clear_color,
            effective_cell_w,
            effective_cell_h,
            braille_style,
            panes
                .iter()
                .map(|pane| {
                    pane.dirty_generations
                        .iter()
                        .filter(|&&dirty| dirty > 0)
                        .count()
                })
                .sum(),
            &draw_batches,
        )
    }

    /// Recreates the swapchain (e.g., after window resize).
    pub fn recreate_swapchain(&mut self, width: u32, height: u32) -> Result<()> {
        unsafe {
            self.device
                .device_wait_idle()
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?
        };
        let required_vertices =
            estimate_vertex_capacity(width, height, self.cell_width, self.cell_height);
        self.ensure_vertex_capacity(required_vertices)?;

        destroy_framebuffers(&self.device, &self.framebuffers);
        self.swapchain.destroy(&self.device);

        let surface_details =
            SurfaceDetails::query(&self.surface_loader, self.physical_device, self.surface)?;
        self.swapchain = Swapchain::new(
            &self.instance,
            &self.device,
            self.surface,
            &surface_details,
            &self.queue_indices,
            width,
            height,
        )?;
        self.framebuffers = create_framebuffers(
            &self.device,
            self.render_pass,
            &self.swapchain.image_views,
            self.swapchain.extent,
        )?;
        tracing::info!("Swapchain recreated: {}x{}", width, height);
        self.current_frame = 0;
        Ok(())
    }

    pub fn update_font_data(
        &mut self,
        rasterizer: FontRasterizer,
        bold_rasterizer: Option<FontRasterizer>,
        italic_rasterizer: Option<FontRasterizer>,
        bold_italic_rasterizer: Option<FontRasterizer>,
        fallback_rasterizers: Vec<FontRasterizer>,
        px_size: f32,
        mut atlas: GlyphAtlas,
    ) -> Result<()> {
        unsafe { self.device.device_wait_idle() }.map_err(|e| ForgeError::Vulkan(e.to_string()))?;

        self.atlas_texture.destroy(&self.device);

        self.atlas_texture = Texture::new(
            &self.instance,
            self.physical_device,
            &self.device,
            self.command_pool,
            self.graphics_queue,
            atlas.atlas_width,
            atlas.atlas_height,
            &atlas.pixels,
        )?;

        // Update cached metrics dynamically if they changed
        self.cell_width = rasterizer.cell_width;
        self.cell_height = rasterizer.cell_height;
        self.baseline = rasterizer.baseline;
        self.font_px_size = px_size;

        atlas.clear_pixels(); // Free the RAM! We only need it on the GPU.
        self.atlas = atlas;
        self.font_rasterizer = Some(rasterizer);
        self.bold_font_rasterizer = bold_rasterizer;
        self.italic_font_rasterizer = italic_rasterizer;
        self.bold_italic_font_rasterizer = bold_italic_rasterizer;
        self.fallback_font_rasterizers = fallback_rasterizers;
        self.reported_missing_glyphs.clear();
        self.unsupported_dynamic_glyphs.clear();
        self.unsupported_shaped_glyphs.clear();
        self.dynamic_atlas_full_reported = false;
        self.shaped_atlas_full_reported = false;
        self.ligature_shaper.clear();

        let image_info = vk::DescriptorImageInfo {
            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            image_view: self.atlas_texture.view,
            sampler: self.atlas_texture.sampler,
        };
        let write_desc = vk::WriteDescriptorSet {
            dst_set: self.descriptor_set,
            dst_binding: 0,
            dst_array_element: 0,
            descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            descriptor_count: 1,
            p_image_info: &image_info,
            ..Default::default()
        };
        unsafe { self.device.update_descriptor_sets(&[write_desc], &[]) };

        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().ok();

            self.device.unmap_memory(self.vertex_memory);
            self.device.destroy_buffer(self.vertex_buffer, None);
            self.device.free_memory(self.vertex_memory, None);

            self.device
                .destroy_descriptor_pool(self.descriptor_pool, None);
            self.atlas_texture.destroy(&self.device);

            self.sync.destroy(&self.device);
            self.device.destroy_command_pool(self.command_pool, None);
            destroy_framebuffers(&self.device, &self.framebuffers);
            self.pipeline.destroy(&self.device);
            self.device.destroy_render_pass(self.render_pass, None);
            self.swapchain.destroy(&self.device);
            self.surface_loader.destroy_surface(self.surface, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
        tracing::debug!("Vulkan renderer dropped and all resources destroyed.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_region_size_is_aligned() {
        let region_size = vertex_region_size(1);

        assert_eq!(region_size % VERTEX_BUFFER_REGION_ALIGNMENT, 0);
        assert!(region_size >= std::mem::size_of::<GlyphVertex>() as vk::DeviceSize);
    }

    #[test]
    fn vertex_buffer_size_allocates_one_region_per_in_flight_frame() {
        let max_vertices = 4096;

        assert_eq!(
            vertex_buffer_size(max_vertices),
            vertex_region_size(max_vertices) * MAX_FRAMES_IN_FLIGHT as vk::DeviceSize
        );
    }

    #[test]
    fn pane_rect_scissor_clamps_to_swapchain_extent() {
        let scissor = rect_to_scissor(
            PaneRenderRect::new(-4.5, 8.2, 40.0, 21.2),
            vk::Extent2D {
                width: 100,
                height: 50,
            },
        )
        .unwrap();

        assert_eq!(scissor.offset, vk::Offset2D { x: 0, y: 8 });
        assert_eq!(
            scissor.extent,
            vk::Extent2D {
                width: 36,
                height: 22
            }
        );
    }

    #[test]
    fn pane_rect_scissor_rejects_empty_or_outside_rects() {
        let extent = vk::Extent2D {
            width: 100,
            height: 50,
        };

        assert!(rect_to_scissor(PaneRenderRect::new(10.0, 10.0, 0.0, 5.0), extent).is_none());
        assert!(rect_to_scissor(PaneRenderRect::new(120.0, 10.0, 4.0, 5.0), extent).is_none());
    }

    #[test]
    fn command_indicator_text_clip_stays_clear_of_moving_dot() {
        let popup = PaneRenderRect::new(20.0, 30.0, 120.0, 30.0);
        let clip = command_indicator_text_clip(popup, 75.0, 10.0, 50.0, 10.0).unwrap();

        assert_eq!(clip.x, 82.5);
        assert_eq!(clip.x + clip.width, popup.x + popup.width);
        assert!(clip.x > 75.0 + 5.0);
    }

    #[test]
    fn command_indicator_text_clip_disappears_before_crossing_dot() {
        let popup = PaneRenderRect::new(80.0, 30.0, 30.0, 30.0);

        assert!(command_indicator_text_clip(popup, 105.0, 10.0, 50.0, 10.0).is_none());
    }

    #[test]
    fn command_indicator_circle_never_exposes_text() {
        let circle = PaneRenderRect::new(80.0, 30.0, 30.0, 30.0);

        assert!(command_indicator_text_clip(circle, 95.0, 10.0, 92.0, 10.0).is_none());
    }

    #[test]
    fn pane_render_id_marks_overlay_ids_as_synthetic() {
        assert!(!PaneRenderId(1).is_synthetic());
        assert!(!PaneRenderId(42).is_synthetic());
        assert!(PaneRenderId(u64::MAX).is_synthetic());
        assert!(PaneRenderId(u64::MAX - 1).is_synthetic());
        assert!(PaneRenderId(u64::MAX - 3).is_synthetic());
    }

    #[test]
    fn vertex_upload_plan_falls_back_to_full_for_uninitialized_frame() {
        let row_ranges = vec![RowVertexRanges {
            bg: VertexRange { start: 0, count: 6 },
            fg: VertexRange { start: 6, count: 6 },
            generation: 1,
        }];

        assert_eq!(
            plan_vertex_upload_for_state(None, 12, &row_ranges, None, None, 0),
            VertexUploadPlan::Full
        );
    }

    #[test]
    fn vertex_upload_plan_uploads_only_changed_compatible_rows() {
        let old_ranges = vec![
            RowVertexRanges {
                bg: VertexRange { start: 0, count: 6 },
                fg: VertexRange {
                    start: 12,
                    count: 6,
                },
                generation: 1,
            },
            RowVertexRanges {
                bg: VertexRange { start: 6, count: 6 },
                fg: VertexRange {
                    start: 18,
                    count: 6,
                },
                generation: 3,
            },
        ];
        let mut new_ranges = old_ranges.clone();
        new_ranges[1].generation = 4;
        let state = FrameVertexUploadState {
            vertex_count: 24,
            row_ranges: old_ranges,
            row_generations: vec![1, 3],
            scrollbar_range: None,
            context_menu_range: None,
            context_menu_fingerprint: 0,
            initialized: true,
        };

        assert_eq!(
            plan_vertex_upload_for_state(Some(&state), 24, &new_ranges, None, None, 0),
            VertexUploadPlan::Partial(vec![new_ranges[1].bg, new_ranges[1].fg])
        );
    }

    #[test]
    fn vertex_upload_plan_falls_back_to_full_when_ranges_shift() {
        let old_ranges = vec![RowVertexRanges {
            bg: VertexRange { start: 0, count: 6 },
            fg: VertexRange { start: 6, count: 6 },
            generation: 1,
        }];
        let new_ranges = vec![RowVertexRanges {
            bg: VertexRange {
                start: 0,
                count: 12,
            },
            fg: VertexRange {
                start: 12,
                count: 6,
            },
            generation: 2,
        }];
        let state = FrameVertexUploadState {
            vertex_count: 18,
            row_ranges: old_ranges,
            row_generations: vec![1],
            scrollbar_range: None,
            context_menu_range: None,
            context_menu_fingerprint: 0,
            initialized: true,
        };

        assert_eq!(
            plan_vertex_upload_for_state(Some(&state), 18, &new_ranges, None, None, 0),
            VertexUploadPlan::Full
        );
    }

    #[test]
    fn vertex_upload_plan_falls_back_to_full_when_context_menu_changes() {
        let row_ranges = vec![RowVertexRanges {
            bg: VertexRange { start: 0, count: 6 },
            fg: VertexRange { start: 6, count: 6 },
            generation: 1,
        }];
        let state = FrameVertexUploadState {
            vertex_count: 18,
            row_ranges: row_ranges.clone(),
            row_generations: vec![1],
            scrollbar_range: None,
            context_menu_range: Some(VertexRange {
                start: 12,
                count: 6,
            }),
            context_menu_fingerprint: 1,
            initialized: true,
        };

        assert_eq!(
            plan_vertex_upload_for_state(
                Some(&state),
                18,
                &row_ranges,
                None,
                Some(VertexRange {
                    start: 12,
                    count: 6,
                }),
                2,
            ),
            VertexUploadPlan::Full
        );
    }

    #[test]
    fn outline_segments_leave_gap_for_overflow_icon() {
        let segments = outline_segments_around_gap(0.0, 100.0, 40.0, 30.0);

        assert_eq!(segments, [(0.0, 40.0), (70.0, 30.0)]);
    }

    #[test]
    fn outline_segments_clamp_gap_to_border_bounds() {
        let segments = outline_segments_around_gap(10.0, 50.0, 0.0, 30.0);

        assert_eq!(segments, [(10.0, 0.0), (40.0, 20.0)]);
    }

    #[test]
    fn overflow_gap_sizes_are_minimal() {
        let cell_w = 10.0_f32;
        let cell_h = 22.0_f32;

        assert_eq!(cell_w * 3.0, 30.0);
        assert_eq!(cell_h, 22.0);
    }

    #[test]
    fn zero_width_and_height_borders_become_visible_dividers() {
        assert_eq!(
            materialize_split_divider(PaneRenderRect::new(40.0, 5.0, 0.0, 30.0), 1.0),
            Some(PaneRenderRect::new(39.5, 5.0, 1.0, 30.0))
        );
        assert_eq!(
            materialize_split_divider(PaneRenderRect::new(5.0, 40.0, 30.0, 0.0), 1.0),
            Some(PaneRenderRect::new(5.0, 39.5, 30.0, 1.0))
        );
    }

    #[test]
    fn adjacent_panes_get_only_a_shared_divider() {
        let left = PaneRenderRect::new(0.0, 0.0, 40.0, 30.0);
        let right = PaneRenderRect::new(50.0, 0.0, 40.0, 30.0);
        assert_eq!(
            adjacent_pane_divider(left, right, 10.0, 20.0),
            Some(PaneRenderRect::new(45.0, 0.0, 0.0, 30.0))
        );

        let top = PaneRenderRect::new(0.0, 0.0, 40.0, 30.0);
        let bottom = PaneRenderRect::new(0.0, 50.0, 40.0, 30.0);
        assert_eq!(
            adjacent_pane_divider(top, bottom, 10.0, 20.0),
            Some(PaneRenderRect::new(0.0, 40.0, 40.0, 0.0))
        );

        let distant = PaneRenderRect::new(51.0, 0.0, 40.0, 30.0);
        assert_eq!(adjacent_pane_divider(left, distant, 10.0, 20.0), None);
    }
}
