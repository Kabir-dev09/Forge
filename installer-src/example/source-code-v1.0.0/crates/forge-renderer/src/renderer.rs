use ash::{vk, Entry, Instance, Device};
use forge_core::{Result, ForgeError};
use super::{instance::*, device::*, surface::*, swapchain::*, framebuffer::*, sync::*, pipeline::*, texture::*};
use super::font::{rasterizer::FontRasterizer, atlas::GlyphAtlas};
use super::grid_tessellator::GridTessellator;
use std::ptr;

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
    pub max_vertices: usize,

    pub cell_width: u32,
    pub cell_height: u32,
    pub baseline: u32,
}

impl Renderer {
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
        tracing::info!("[PROFILER] select_physical_device took: {:?}", t_phys.elapsed());

        let t_log = std::time::Instant::now();
        let (device, graphics_queue, present_queue) =
            create_logical_device(&instance, physical_device, &queue_indices)?;
        tracing::info!("[PROFILER] create_logical_device took: {:?}", t_log.elapsed());

        let t_swap = std::time::Instant::now();
        let surface_details = SurfaceDetails::query(&surface_loader, physical_device, surface)?;
        let swapchain = Swapchain::new(
            &instance, &device, surface,
            &surface_details, &queue_indices, width, height,
        )?;
        tracing::info!("[PROFILER] Swapchain::new took: {:?}", t_swap.elapsed());

        let t_pipe = std::time::Instant::now();
        let render_pass = super::render_pass::create_render_pass(&device, swapchain.format)?;
        let pipeline = Pipeline::new(&device, render_pass)?;
        tracing::info!("[PROFILER] Pipeline::new took: {:?}", t_pipe.elapsed());

        let framebuffers = create_framebuffers(&device, render_pass, &swapchain.image_views, swapchain.extent)?;

        let command_pool = create_command_pool(&device, queue_indices.graphics)?;
        let command_buffers = allocate_command_buffers(
            &device, command_pool, MAX_FRAMES_IN_FLIGHT as u32
        )?;

        let sync = SyncPrimitives::new(&device)?;
        tracing::info!("Vulkan Initialization took: {:?}", vk_start.elapsed());

        // Font and Atlas (Dummy initialization for fast boot)
        let mut atlas = GlyphAtlas {
            atlas_width: 1,
            atlas_height: 1,
            pixels: vec![255], // 1x1 solid white pixel
            glyphs: std::collections::HashMap::new(),
            glyphs_bold: std::collections::HashMap::new(),
        };
        // Add a dummy ' ' glyph so the renderer doesn't panic if it looks something up
        atlas.glyphs.insert(' ', super::font::atlas::GlyphMetrics {
            u0: 0.0, v0: 0.0, u1: 1.0, v1: 1.0, width: 0, height: 0, bearing_x: 0, bearing_y: 0
        });

        let atlas_texture = Texture::new(
            &instance, physical_device, &device, command_pool, graphics_queue,
            1, 1, &atlas.pixels
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
            device.create_descriptor_pool(&pool_info, None)
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
            device.allocate_descriptor_sets(&alloc_info)
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
        let max_vertices = 100_000;
        let vertex_buffer_size = (max_vertices * std::mem::size_of::<GlyphVertex>()) as vk::DeviceSize;
        let (vertex_buffer, vertex_memory) = super::texture::create_buffer(
            &instance, physical_device, &device, vertex_buffer_size,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
        )?;

        let tessellator = GridTessellator::new(max_vertices / 12);

        tracing::info!("Vulkan renderer initialized.");

        Ok(Self {
            entry, instance, surface_loader, surface,
            physical_device, device, graphics_queue, present_queue,
            queue_indices, swapchain, render_pass, pipeline, framebuffers,
            command_pool, command_buffers, sync, current_frame: 0,
            atlas, atlas_texture, descriptor_pool, descriptor_set,
            tessellator, vertex_buffer, vertex_memory, max_vertices,
            cell_width, cell_height, baseline,
        })
    }

    pub fn render_clear(&mut self, clear_color: [f32; 4]) -> Result<bool> {
        super::frame::render_frame(
            &self.device, &self.swapchain, self.render_pass,
            &self.framebuffers, &self.command_buffers, &self.sync,
            self.graphics_queue, self.present_queue,
            &mut self.current_frame, clear_color,
        )
    }

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
        scale_x: f32,
        scale_y: f32,
        scrollbar: Option<(f32, f32, f32, f32, f32, f32)>,
        braille_style: forge_core::config_registry::BrailleStyle,
    ) -> Result<bool> {
        // Always clear self.tessellator.vertices, always re-tessellate the entire screen, and always upload the full buffer.
        let effective_baseline = self.baseline as f32 * scale_y;
        self.tessellator.tessellate(
            grid,
            dirty_rows,
            &self.atlas,
            effective_cell_w,
            effective_cell_h,
            effective_baseline,
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
            scale_x,
            scale_y,
            scrollbar,
        );

        // Upload vertices
        if !self.tessellator.vertices.is_empty() {
            unsafe {
                let data_size = (self.tessellator.vertices.len() * std::mem::size_of::<GlyphVertex>()) as vk::DeviceSize;
                let data_ptr = self.device.map_memory(self.vertex_memory, 0, data_size, vk::MemoryMapFlags::empty())
                    .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
                ptr::copy_nonoverlapping(self.tessellator.vertices.as_ptr(), data_ptr as *mut GlyphVertex, self.tessellator.vertices.len());
                self.device.unmap_memory(self.vertex_memory);
            }
        }

        // We need to pass the vertex buffer and descriptor set down to the frame rendering logic.
        // I will implement a custom frame render for grid drawing.
        
        let frame = self.current_frame;
        // 1. Wait for fence
        unsafe {
            self.device.wait_for_fences(&[self.sync.in_flight_fences[frame]], true, u64::MAX)
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
        }

        // 2. Acquire next image
        let (image_index, suboptimal) = unsafe {
            match self.swapchain.loader.acquire_next_image(
                self.swapchain.handle,
                u64::MAX,
                self.sync.image_available_semaphores[frame],
                vk::Fence::null(),
            ) {
                Ok(result) => result,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return Ok(true),
                Err(vk::Result::ERROR_SURFACE_LOST_KHR) => return Err(ForgeError::Vulkan("Surface lost".to_string())),
                Err(e) => return Err(ForgeError::Vulkan(format!("acquire_next_image failed: {}", e))),
            }
        };

        if image_index as usize >= self.framebuffers.len() {
            return Err(ForgeError::Vulkan("Image index out of bounds".to_string()));
        }

        unsafe {
            self.device.reset_fences(&[self.sync.in_flight_fences[frame]])
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
        }

        // Record command buffer
        let cmd = self.command_buffers[frame];
        unsafe {
            self.device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;

            let begin_info = vk::CommandBufferBeginInfo {
                flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
                ..Default::default()
            };
            self.device.begin_command_buffer(cmd, &begin_info)
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;

            let clear_value = vk::ClearValue {
                color: vk::ClearColorValue { float32: clear_color },
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

            self.device.cmd_begin_render_pass(cmd, &render_pass_begin, vk::SubpassContents::INLINE);

            if !self.tessellator.vertices.is_empty() {
                self.device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline.graphics_pipeline);
                
                let config_flags = match braille_style {
                    forge_core::config_registry::BrailleStyle::Solid => 1,
                    forge_core::config_registry::BrailleStyle::Dots => 0,
                };
                let pc = crate::pipeline::PushConstants {
                    cell_size: [self.cell_width as f32, self.cell_height as f32],
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

                self.device.cmd_bind_vertex_buffers(cmd, 0, &[self.vertex_buffer], &[0]);
                self.device.cmd_bind_descriptor_sets(
                    cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline.pipeline_layout,
                    0, &[self.descriptor_set], &[]
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

                self.device.cmd_draw(cmd, self.tessellator.vertices.len() as u32, 1, 0, 0);
            }

            self.device.cmd_end_render_pass(cmd);
            self.device.end_command_buffer(cmd)
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
            self.device.queue_submit(self.graphics_queue, &[submit_info], self.sync.in_flight_fences[frame])
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
            match self.swapchain.loader.queue_present(self.present_queue, &present_info) {
                Ok(true) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => true,
                Ok(false) => suboptimal,
                Err(vk::Result::ERROR_SURFACE_LOST_KHR) => return Err(ForgeError::Vulkan("Surface lost".to_string())),
                Err(e) => return Err(ForgeError::Vulkan(format!("queue_present failed: {}", e))),
            }
        };

        self.current_frame = (frame + 1) % MAX_FRAMES_IN_FLIGHT;
        Ok(needs_recreate)
    }

    /// Recreates the swapchain (e.g., after window resize).
    pub fn recreate_swapchain(&mut self, width: u32, height: u32) -> Result<()> {
        unsafe { self.device.device_wait_idle().map_err(|e| ForgeError::Vulkan(e.to_string()))? };
        let cols = (width as f64 / self.cell_width as f64).ceil() as usize;
        let rows = (height as f64 / self.cell_height as f64).ceil() as usize;
        let required_vertices = (cols * rows + 1000) * 12;
        if required_vertices > self.max_vertices {
            unsafe {
                self.device.destroy_buffer(self.vertex_buffer, None);
                self.device.free_memory(self.vertex_memory, None);
            }
            self.max_vertices = required_vertices.max(self.max_vertices * 2);
            let vertex_buffer_size = (self.max_vertices * std::mem::size_of::<GlyphVertex>()) as vk::DeviceSize;
            let (new_buffer, new_memory) = super::texture::create_buffer(
                &self.instance, self.physical_device, &self.device, vertex_buffer_size,
                vk::BufferUsageFlags::VERTEX_BUFFER,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
            )?;
            self.vertex_buffer = new_buffer;
            self.vertex_memory = new_memory;
            self.tessellator = GridTessellator::new(self.max_vertices / 12);
        }

        destroy_framebuffers(&self.device, &self.framebuffers);
        self.swapchain.destroy(&self.device);

        let surface_details = SurfaceDetails::query(&self.surface_loader, self.physical_device, self.surface)?;
        self.swapchain = Swapchain::new(
            &self.instance, &self.device, self.surface,
            &surface_details, &self.queue_indices, width, height,
        )?;
        self.framebuffers = create_framebuffers(
            &self.device, self.render_pass,
            &self.swapchain.image_views, self.swapchain.extent,
        )?;
        tracing::info!("Swapchain recreated: {}x{}", width, height);
        self.current_frame = 0;
        Ok(())
    }

    pub fn update_font_data(&mut self, rasterizer: FontRasterizer, mut atlas: GlyphAtlas) -> Result<()> {
        unsafe { self.device.device_wait_idle() }.map_err(|e| ForgeError::Vulkan(e.to_string()))?;
        
        self.atlas_texture.destroy(&self.device);
        
        self.atlas_texture = Texture::new(
            &self.instance, self.physical_device, &self.device, self.command_pool, self.graphics_queue,
            atlas.atlas_width, atlas.atlas_height, &atlas.pixels
        )?;
        
        // Update cached metrics dynamically if they changed
        self.cell_width = rasterizer.cell_width;
        self.cell_height = rasterizer.cell_height;
        self.baseline = rasterizer.baseline;

        atlas.clear_pixels(); // Free the RAM! We only need it on the GPU.
        self.atlas = atlas;
        
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

            self.device.destroy_buffer(self.vertex_buffer, None);
            self.device.free_memory(self.vertex_memory, None);

            self.device.destroy_descriptor_pool(self.descriptor_pool, None);
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
