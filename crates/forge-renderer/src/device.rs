use ash::{vk, Instance, Device};
use forge_core::{Result, ForgeError};

/// Required device extensions.
pub const REQUIRED_DEVICE_EXTENSIONS: &[&std::ffi::CStr] = &[
    ash::khr::swapchain::NAME,
];

pub struct QueueFamilyIndices {
    pub graphics: u32,
    pub present: u32,  // May be same as graphics
}

/// Selects the best available physical device.
/// Prefers discrete GPU, then integrated, then any other.
/// The device must support the required extensions and have a graphics queue.
pub fn select_physical_device(
    instance: &Instance,
    surface: vk::SurfaceKHR,
    surface_loader: &ash::khr::surface::Instance,
) -> Result<(vk::PhysicalDevice, QueueFamilyIndices)> {
    let devices = unsafe { instance.enumerate_physical_devices() }
        .map_err(|e| ForgeError::Vulkan(format!("Failed to enumerate devices: {}", e)))?;

    if devices.is_empty() {
        return Err(ForgeError::Vulkan("No Vulkan-capable devices found".to_string()));
    }

    let mut best: Option<(vk::PhysicalDevice, QueueFamilyIndices, u32)> = None;

    for &device in &devices {
        let props = unsafe { instance.get_physical_device_properties(device) };
        let name = unsafe { std::ffi::CStr::from_ptr(props.device_name.as_ptr()) };
        tracing::debug!("Evaluating GPU: {:?} (type: {:?})", name, props.device_type);

        // Check required extensions
        if !check_device_extensions(instance, device) {
            tracing::debug!("  → Skipping: missing required extensions");
            continue;
        }

        // Find queue families
        let queue_families = unsafe {
            instance.get_physical_device_queue_family_properties(device)
        };

        let graphics_family = queue_families.iter().enumerate().find(|(_, qf)| {
            qf.queue_flags.contains(vk::QueueFlags::GRAPHICS)
        }).map(|(i, _)| i as u32);

        let present_family = queue_families.iter().enumerate().find(|(i, _)| {
            unsafe {
                surface_loader.get_physical_device_surface_support(device, *i as u32, surface)
                    .unwrap_or(false)
            }
        }).map(|(i, _)| i as u32);

        let (Some(gfx), Some(present)) = (graphics_family, present_family) else {
            tracing::debug!("  → Skipping: no suitable queue families");
            continue;
        };

        let score = match props.device_type {
            vk::PhysicalDeviceType::DISCRETE_GPU => 1000,
            vk::PhysicalDeviceType::INTEGRATED_GPU => 100,
            _ => 1,
        };

        tracing::debug!("  → Score: {}", score);

        if best.as_ref().is_none_or(|b| score > b.2) {
            best = Some((device, QueueFamilyIndices { graphics: gfx, present }, score));
        }
    }

    best.map(|(d, q, _)| (d, q))
        .ok_or_else(|| ForgeError::Vulkan("No suitable Vulkan device found".to_string()))
}

fn check_device_extensions(instance: &Instance, device: vk::PhysicalDevice) -> bool {
    let available = match unsafe {
        instance.enumerate_device_extension_properties(device)
    } {
        Ok(e) => e,
        Err(_) => return false,
    };

    REQUIRED_DEVICE_EXTENSIONS.iter().all(|required| {
        available.iter().any(|ext| {
            let name = unsafe { std::ffi::CStr::from_ptr(ext.extension_name.as_ptr()) };
            name == *required
        })
    })
}

/// Creates the logical device and retrieves the graphics and present queues.
pub fn create_logical_device(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    indices: &QueueFamilyIndices,
) -> Result<(Device, vk::Queue, vk::Queue)> {
    let queue_priority = 1.0f32;

    // Deduplicate queue families (graphics and present may be the same)
    let unique_queues: std::collections::HashSet<u32> = [indices.graphics, indices.present]
        .iter().cloned().collect();

    let queue_infos: Vec<vk::DeviceQueueCreateInfo> = unique_queues.iter().map(|&qf| {
        vk::DeviceQueueCreateInfo {
            queue_family_index: qf,
            queue_count: 1,
            p_queue_priorities: &queue_priority,
            ..Default::default()
        }
    }).collect();

    // Enable required features
    let features = vk::PhysicalDeviceFeatures {
        sampler_anisotropy: vk::TRUE,
        ..Default::default()
    };

    let ext_names: Vec<*const i8> = REQUIRED_DEVICE_EXTENSIONS
        .iter()
        .map(|s| s.as_ptr())
        .collect();

    let device_info = vk::DeviceCreateInfo {
        queue_create_info_count: queue_infos.len() as u32,
        p_queue_create_infos: queue_infos.as_ptr(),
        enabled_extension_count: ext_names.len() as u32,
        pp_enabled_extension_names: ext_names.as_ptr(),
        p_enabled_features: &features,
        ..Default::default()
    };

    let device = unsafe {
        instance.create_device(physical_device, &device_info, None)
            .map_err(|e| ForgeError::Vulkan(format!("Failed to create logical device: {}", e)))?
    };

    let graphics_queue = unsafe { device.get_device_queue(indices.graphics, 0) };
    let present_queue = unsafe { device.get_device_queue(indices.present, 0) };

    Ok((device, graphics_queue, present_queue))
}

/// Creates a command pool for the graphics queue family.
/// Commands allocated from this pool can be re-recorded each frame.
pub fn create_command_pool(
    device: &Device,
    graphics_family: u32,
) -> Result<vk::CommandPool> {
    let pool_info = vk::CommandPoolCreateInfo {
        flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
        queue_family_index: graphics_family,
        ..Default::default()
    };
    unsafe {
        device.create_command_pool(&pool_info, None)
            .map_err(|e| ForgeError::Vulkan(format!("Failed to create command pool: {}", e)))
    }
}

/// Allocates `count` primary command buffers from the given pool.
pub fn allocate_command_buffers(
    device: &Device,
    command_pool: vk::CommandPool,
    count: u32,
) -> Result<Vec<vk::CommandBuffer>> {
    let alloc_info = vk::CommandBufferAllocateInfo {
        command_pool,
        level: vk::CommandBufferLevel::PRIMARY,
        command_buffer_count: count,
        ..Default::default()
    };
    unsafe {
        device.allocate_command_buffers(&alloc_info)
            .map_err(|e| ForgeError::Vulkan(format!("Failed to allocate command buffers: {}", e)))
    }
}

pub fn find_memory_type(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    type_filter: u32,
    properties: vk::MemoryPropertyFlags,
) -> Result<u32> {
    let mem_properties = unsafe { instance.get_physical_device_memory_properties(physical_device) };
    for i in 0..mem_properties.memory_type_count {
        if (type_filter & (1 << i)) != 0
            && (mem_properties.memory_types[i as usize].property_flags & properties) == properties
        {
            return Ok(i);
        }
    }
    Err(ForgeError::Vulkan("Failed to find suitable memory type".into()))
}
