use ash::{vk, Instance, Device};
use forge_core::{Result, ForgeError};
use std::ptr;
use super::device::find_memory_type;

pub struct Texture {
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    pub sampler: vk::Sampler,
}

impl Texture {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &Device,
        command_pool: vk::CommandPool,
        graphics_queue: vk::Queue,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Self> {
        let image_size = (width * height * 4) as vk::DeviceSize;

        // 1. Create staging buffer
        let (staging_buffer, staging_memory) = create_buffer(
            instance, physical_device, device,
            image_size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        // 2. Copy pixels to staging buffer
        unsafe {
            let data_ptr = device.map_memory(staging_memory, 0, image_size, vk::MemoryMapFlags::empty())
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
            ptr::copy_nonoverlapping(pixels.as_ptr(), data_ptr as *mut u8, pixels.len());
            device.unmap_memory(staging_memory);
        }

        // 3. Create image
        let image_info = vk::ImageCreateInfo {
            image_type: vk::ImageType::TYPE_2D,
            extent: vk::Extent3D { width, height, depth: 1 },
            mip_levels: 1,
            array_layers: 1,
            format: vk::Format::R8G8B8A8_UNORM,
            tiling: vk::ImageTiling::OPTIMAL,
            initial_layout: vk::ImageLayout::UNDEFINED,
            usage: vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            samples: vk::SampleCountFlags::TYPE_1,
            ..Default::default()
        };

        let image = unsafe {
            device.create_image(&image_info, None).map_err(|e| ForgeError::Vulkan(e.to_string()))?
        };

        let mem_requirements = unsafe { device.get_image_memory_requirements(image) };
        let memory_type = find_memory_type(
            instance, physical_device,
            mem_requirements.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        let alloc_info = vk::MemoryAllocateInfo {
            allocation_size: mem_requirements.size,
            memory_type_index: memory_type,
            ..Default::default()
        };

        let memory = unsafe {
            device.allocate_memory(&alloc_info, None).map_err(|e| ForgeError::Vulkan(e.to_string()))?
        };

        unsafe {
            device.bind_image_memory(image, memory, 0).map_err(|e| ForgeError::Vulkan(e.to_string()))?;
        }

        // 4. Transition image layout to TRANSFER_DST_OPTIMAL
        transition_image_layout(
            device, command_pool, graphics_queue,
            image, vk::Format::R8G8B8A8_UNORM,
            vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        )?;

        // 5. Copy buffer to image
        copy_buffer_to_image(
            device, command_pool, graphics_queue,
            staging_buffer, image, width, height,
        )?;

        // 6. Transition image layout to SHADER_READ_ONLY_OPTIMAL
        transition_image_layout(
            device, command_pool, graphics_queue,
            image, vk::Format::R8G8B8A8_UNORM,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        )?;

        // 7. Cleanup staging buffer
        unsafe {
            device.destroy_buffer(staging_buffer, None);
            device.free_memory(staging_memory, None);
        }

        // 8. Create Image View
        let view_info = vk::ImageViewCreateInfo {
            image,
            view_type: vk::ImageViewType::TYPE_2D,
            format: vk::Format::R8G8B8A8_UNORM,
            subresource_range: vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            },
            ..Default::default()
        };
        let view = unsafe {
            device.create_image_view(&view_info, None).map_err(|e| ForgeError::Vulkan(e.to_string()))?
        };

        // 9. Create Sampler
        let sampler_info = vk::SamplerCreateInfo {
            mag_filter: vk::Filter::NEAREST,
            min_filter: vk::Filter::NEAREST,
            address_mode_u: vk::SamplerAddressMode::CLAMP_TO_EDGE,
            address_mode_v: vk::SamplerAddressMode::CLAMP_TO_EDGE,
            address_mode_w: vk::SamplerAddressMode::CLAMP_TO_EDGE,
            anisotropy_enable: vk::FALSE,
            max_anisotropy: 1.0,
            border_color: vk::BorderColor::INT_OPAQUE_BLACK,
            unnormalized_coordinates: vk::FALSE,
            compare_enable: vk::FALSE,
            compare_op: vk::CompareOp::ALWAYS,
            mipmap_mode: vk::SamplerMipmapMode::NEAREST,
            mip_lod_bias: 0.0,
            min_lod: 0.0,
            max_lod: 0.0,
            ..Default::default()
        };
        let sampler = unsafe {
            device.create_sampler(&sampler_info, None).map_err(|e| ForgeError::Vulkan(e.to_string()))?
        };

        Ok(Self { image, memory, view, sampler })
    }

    pub fn destroy(&self, device: &Device) {
        unsafe {
            device.destroy_sampler(self.sampler, None);
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

pub fn create_buffer(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    device: &Device,
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
    properties: vk::MemoryPropertyFlags,
) -> Result<(vk::Buffer, vk::DeviceMemory)> {
    let buffer_info = vk::BufferCreateInfo {
        size,
        usage,
        sharing_mode: vk::SharingMode::EXCLUSIVE,
        ..Default::default()
    };

    let buffer = unsafe {
        device.create_buffer(&buffer_info, None).map_err(|e| ForgeError::Vulkan(e.to_string()))?
    };

    let mem_requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    let memory_type = find_memory_type(instance, physical_device, mem_requirements.memory_type_bits, properties)?;

    let alloc_info = vk::MemoryAllocateInfo {
        allocation_size: mem_requirements.size,
        memory_type_index: memory_type,
        ..Default::default()
    };

    let memory = unsafe {
        device.allocate_memory(&alloc_info, None).map_err(|e| ForgeError::Vulkan(e.to_string()))?
    };

    unsafe {
        device.bind_buffer_memory(buffer, memory, 0).map_err(|e| ForgeError::Vulkan(e.to_string()))?;
    }

    Ok((buffer, memory))
}

fn begin_single_time_commands(device: &Device, command_pool: vk::CommandPool) -> Result<vk::CommandBuffer> {
    let alloc_info = vk::CommandBufferAllocateInfo {
        level: vk::CommandBufferLevel::PRIMARY,
        command_pool,
        command_buffer_count: 1,
        ..Default::default()
    };

    let command_buffer = unsafe {
        device.allocate_command_buffers(&alloc_info).map_err(|e| ForgeError::Vulkan(e.to_string()))?[0]
    };

    let begin_info = vk::CommandBufferBeginInfo {
        flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
        ..Default::default()
    };

    unsafe {
        device.begin_command_buffer(command_buffer, &begin_info).map_err(|e| ForgeError::Vulkan(e.to_string()))?;
    }

    Ok(command_buffer)
}

fn end_single_time_commands(
    device: &Device,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    command_buffer: vk::CommandBuffer,
) -> Result<()> {
    unsafe {
        device.end_command_buffer(command_buffer).map_err(|e| ForgeError::Vulkan(e.to_string()))?;
    }

    let command_buffers = [command_buffer];
    let submit_info = vk::SubmitInfo {
        command_buffer_count: 1,
        p_command_buffers: command_buffers.as_ptr(),
        ..Default::default()
    };

    unsafe {
        device.queue_submit(graphics_queue, &[submit_info], vk::Fence::null())
            .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
        device.queue_wait_idle(graphics_queue)
            .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
        device.free_command_buffers(command_pool, &command_buffers);
    }

    Ok(())
}

fn transition_image_layout(
    device: &Device,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    image: vk::Image,
    _format: vk::Format,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
) -> Result<()> {
    let command_buffer = begin_single_time_commands(device, command_pool)?;

    let (src_access_mask, dst_access_mask, source_stage, destination_stage) = match (old_layout, new_layout) {
        (vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL) => (
            vk::AccessFlags::empty(),
            vk::AccessFlags::TRANSFER_WRITE,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
        ),
        (vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL) => (
            vk::AccessFlags::TRANSFER_WRITE,
            vk::AccessFlags::SHADER_READ,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
        ),
        _ => return Err(ForgeError::Vulkan("Unsupported layout transition".into())),
    };

    let barrier = vk::ImageMemoryBarrier {
        old_layout,
        new_layout,
        src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
        dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
        image,
        subresource_range: vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        },
        src_access_mask,
        dst_access_mask,
        ..Default::default()
    };

    unsafe {
        device.cmd_pipeline_barrier(
            command_buffer,
            source_stage,
            destination_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );
    }

    end_single_time_commands(device, command_pool, graphics_queue, command_buffer)
}

fn copy_buffer_to_image(
    device: &Device,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    buffer: vk::Buffer,
    image: vk::Image,
    width: u32,
    height: u32,
) -> Result<()> {
    let command_buffer = begin_single_time_commands(device, command_pool)?;

    let region = vk::BufferImageCopy {
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image_subresource: vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        },
        image_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
        image_extent: vk::Extent3D { width, height, depth: 1 },
    };

    unsafe {
        device.cmd_copy_buffer_to_image(
            command_buffer,
            buffer,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &[region],
        );
    }

    end_single_time_commands(device, command_pool, graphics_queue, command_buffer)
}
