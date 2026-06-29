use super::font::{
    atlas::{DynamicGlyphInsertResult, GlyphAtlas, GlyphKey},
    rasterizer::FontRasterizer,
};
use super::grid_tessellator::{GridTessellator, RowVertexRanges, VertexRange};
use super::{
    device::*, framebuffer::*, instance::*, pipeline::*, surface::*, swapchain::*, sync::*,
    texture::*,
};
use ash::{vk, Device, Entry, Instance};
use forge_core::{ForgeError, Result};
use std::collections::HashSet;
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

fn vertex_region_size(max_vertices: usize) -> vk::DeviceSize {
    let vertex_bytes = (max_vertices * std::mem::size_of::<GlyphVertex>()) as vk::DeviceSize;
    align_device_size(vertex_bytes, VERTEX_BUFFER_REGION_ALIGNMENT)
}

fn vertex_buffer_size(max_vertices: usize) -> vk::DeviceSize {
    vertex_region_size(max_vertices) * MAX_FRAMES_IN_FLIGHT as vk::DeviceSize
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderFrameStats {
    pub dirty_rows: usize,
    pub vertices: usize,
    pub bytes_uploaded: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub frames_submitted: u64,
    pub dirty_rows: u64,
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
    initialized: bool,
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
) -> VertexUploadPlan {
    let Some(state) = state else {
        return VertexUploadPlan::Full;
    };

    if !state.initialized
        || state.vertex_count != vertex_count
        || state.row_ranges.len() != row_ranges.len()
        || state.row_generations.len() != row_ranges.len()
        || state.scrollbar_range != scrollbar_range
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
    pub vertex_buffer: vk::Buffer,
    pub vertex_memory: vk::DeviceMemory,
    pub vertex_mapped_ptr: *mut std::ffi::c_void,
    pub vertex_region_size: vk::DeviceSize,
    pub max_vertices: usize,
    pub render_stats: Option<RenderStats>,
    frame_upload_states: Vec<FrameVertexUploadState>,
    reported_missing_glyphs: HashSet<GlyphKey>,
    unsupported_dynamic_glyphs: HashSet<GlyphKey>,
    dynamic_atlas_full_reported: bool,
    font_rasterizer: Option<FontRasterizer>,
    bold_font_rasterizer: Option<FontRasterizer>,
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
                        tracing::warn!("Dynamic glyph atlas is full! Some glyphs will be missing.");
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
                        is_bold = key.is_bold,
                        "Glyph is missing from primary, bold, and fallback fonts"
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
        stats.dirty_rows += frame_stats.dirty_rows as u64;
        stats.vertices_uploaded += frame_stats.vertices as u64;
        stats.bytes_uploaded += frame_stats.bytes_uploaded as u64;
        stats.last_frame = frame_stats;

        if stats.frames_submitted == 1 || stats.frames_submitted % RENDER_STATS_LOG_INTERVAL == 0 {
            tracing::info!(
                frames_submitted = stats.frames_submitted,
                total_dirty_rows = stats.dirty_rows,
                total_vertices_uploaded = stats.vertices_uploaded,
                total_bytes_uploaded = stats.bytes_uploaded,
                last_dirty_rows = stats.last_frame.dirty_rows,
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
        tracing::info!("[PROFILER] create_instance took: {:?}", t_inst.elapsed());

        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
        let surface = create_wayland_surface(&entry, &instance, wl_display, wl_surface)?;

        let t_phys = std::time::Instant::now();
        let (physical_device, queue_indices) =
            select_physical_device(&instance, surface, &surface_loader)?;
        tracing::info!(
            "[PROFILER] select_physical_device took: {:?}",
            t_phys.elapsed()
        );

        let t_log = std::time::Instant::now();
        let (device, graphics_queue, present_queue) =
            create_logical_device(&instance, physical_device, &queue_indices)?;
        tracing::info!(
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
        tracing::info!("[PROFILER] Swapchain::new took: {:?}", t_swap.elapsed());

        let t_pipe = std::time::Instant::now();
        let render_pass = super::render_pass::create_render_pass(&device, swapchain.format)?;
        let pipeline = Pipeline::new(&device, render_pass)?;
        tracing::info!("[PROFILER] Pipeline::new took: {:?}", t_pipe.elapsed());

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
            descriptor: super::font::atlas::GlyphAtlasDescriptor::dummy(),
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
            vertex_buffer,
            vertex_memory,
            vertex_mapped_ptr,
            vertex_region_size,
            max_vertices,
            render_stats,
            frame_upload_states: vec![FrameVertexUploadState::default(); MAX_FRAMES_IN_FLIGHT],
            reported_missing_glyphs: HashSet::new(),
            unsupported_dynamic_glyphs: HashSet::new(),
            dynamic_atlas_full_reported: false,
            font_rasterizer: None,
            bold_font_rasterizer: None,
            fallback_font_rasterizers: Vec::new(),
            font_px_size: baseline as f32,
            cell_width,
            cell_height,
            baseline,
        })
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

    #[allow(clippy::too_many_arguments)]
    pub fn render_grid(
        &mut self,
        grid: &[&[forge_core::cell::Cell]],
        dirty_rows: &[bool],
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
        scroll_event: Option<super::grid_tessellator::ScrollEvent>,
        braille_style: forge_core::config_registry::BrailleStyle,
    ) -> Result<bool> {
        let _span = tracing::trace_span!(
            "renderer.render_grid",
            rows = grid.len(),
            cols = grid.first().map(|row| row.len()).unwrap_or(0),
            dirty_rows = dirty_rows.iter().filter(|&&dirty| dirty).count(),
            width = self.swapchain.extent.width,
            height = self.swapchain.extent.height
        )
        .entered();

        // Always clear self.tessellator.vertices, always re-tessellate the entire screen, and always upload the full buffer.
        self.tessellator.tessellate(
            grid,
            dirty_rows,
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
            scroll_event,
        );
        let missing_glyphs: Vec<GlyphKey> =
            self.tessellator.missing_glyphs().iter().copied().collect();
        if self.insert_dynamic_glyphs(&missing_glyphs)? {
            let all_dirty = vec![true; grid.len()];
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
                None,
            );
        }
        for key in self.tessellator.missing_glyphs() {
            if self.reported_missing_glyphs.insert(*key) {
                tracing::debug!(
                    char = %key.c,
                    codepoint = format_args!("U+{:04X}", key.c as u32),
                    is_bold = key.is_bold,
                    "Glyph missing from current atlas; fallback glyph used"
                );
            }
        }
        tracing::debug!(
            vertices = self.tessellator.vertices.len(),
            bytes = self.tessellator.vertices.len() * std::mem::size_of::<GlyphVertex>(),
            "Renderer tessellation output"
        );
        self.ensure_vertex_capacity(self.tessellator.vertices.len())?;

        let frame = self.current_frame;
        let vertex_buffer_offset = self.vertex_region_size * frame as vk::DeviceSize;

        // The current frame's ring-buffer region is reusable only after its
        // fence has completed.
        unsafe {
            let _fence_span =
                tracing::trace_span!("renderer.wait_for_frame_fence", frame = frame).entered();
            self.device
                .wait_for_fences(&[self.sync.in_flight_fences[frame]], true, u64::MAX)
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
        }

        let upload_plan = self.plan_vertex_upload(frame);
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
        self.update_frame_upload_state(frame);

        let frame_stats = RenderFrameStats {
            dirty_rows: dirty_rows.iter().filter(|&&dirty| dirty).count(),
            vertices: self.tessellator.vertices.len(),
            bytes_uploaded,
        };

        // We need to pass the vertex buffer and descriptor set down to the frame rendering logic.
        // I will implement a custom frame render for grid drawing.

        // 2. Acquire next image
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

        // Record command buffer
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
                render_area: vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: self.swapchain.extent,
                },
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

                let scissor = vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: self.swapchain.extent,
                };
                self.device.cmd_set_scissor(cmd, 0, &[scissor]);

                self.device
                    .cmd_draw(cmd, self.tessellator.vertices.len() as u32, 1, 0, 0);
            }

            self.device.cmd_end_render_pass(cmd);
            self.device
                .end_command_buffer(cmd)
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
        }

        // Submit
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

        // Present
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
        self.fallback_font_rasterizers = fallback_rasterizers;
        self.reported_missing_glyphs.clear();
        self.unsupported_dynamic_glyphs.clear();
        self.dynamic_atlas_full_reported = false;

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
    fn vertex_upload_plan_falls_back_to_full_for_uninitialized_frame() {
        let row_ranges = vec![RowVertexRanges {
            bg: VertexRange { start: 0, count: 6 },
            fg: VertexRange { start: 6, count: 6 },
            generation: 1,
        }];

        assert_eq!(
            plan_vertex_upload_for_state(None, 12, &row_ranges, None),
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
            initialized: true,
        };

        assert_eq!(
            plan_vertex_upload_for_state(Some(&state), 24, &new_ranges, None),
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
            initialized: true,
        };

        assert_eq!(
            plan_vertex_upload_for_state(Some(&state), 18, &new_ranges, None),
            VertexUploadPlan::Full
        );
    }
}
