use super::device::QueueFamilyIndices;
use super::surface::SurfaceDetails;
use ash::{vk, Device, Instance};
use forge_core::{ForgeError, Result};

pub struct Swapchain {
    pub loader: ash::khr::swapchain::Device,
    pub handle: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub image_views: Vec<vk::ImageView>,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
}

fn choose_composite_alpha(caps: &vk::SurfaceCapabilitiesKHR) -> vk::CompositeAlphaFlagsKHR {
    if caps
        .supported_composite_alpha
        .contains(vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED)
    {
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED
    } else {
        vk::CompositeAlphaFlagsKHR::OPAQUE
    }
}

impl Swapchain {
    pub fn new(
        instance: &Instance,
        device: &Device,
        surface: vk::SurfaceKHR,
        surface_details: &SurfaceDetails,
        queue_indices: &QueueFamilyIndices,
        desired_width: u32,
        desired_height: u32,
    ) -> Result<Self> {
        let format = surface_details.choose_format()?;
        let present_mode = surface_details.choose_present_mode();
        let extent = surface_details.choose_extent(desired_width, desired_height);

        // Use one more image than the minimum for triple-buffering when possible.
        let mut image_count = surface_details.capabilities.min_image_count + 1;
        if surface_details.capabilities.max_image_count > 0
            && image_count > surface_details.capabilities.max_image_count
        {
            image_count = surface_details.capabilities.max_image_count;
        }

        let (sharing_mode, queue_family_indices) =
            if queue_indices.graphics != queue_indices.present {
                (
                    vk::SharingMode::CONCURRENT,
                    vec![queue_indices.graphics, queue_indices.present],
                )
            } else {
                (vk::SharingMode::EXCLUSIVE, vec![])
            };

        let composite_alpha = choose_composite_alpha(&surface_details.capabilities);

        let create_info = vk::SwapchainCreateInfoKHR {
            surface,
            min_image_count: image_count,
            image_format: format.format,
            image_color_space: format.color_space,
            image_extent: extent,
            image_array_layers: 1,
            image_usage: vk::ImageUsageFlags::COLOR_ATTACHMENT,
            image_sharing_mode: sharing_mode,
            queue_family_index_count: queue_family_indices.len() as u32,
            p_queue_family_indices: queue_family_indices.as_ptr(),
            pre_transform: surface_details.capabilities.current_transform,
            composite_alpha,
            present_mode,
            clipped: vk::TRUE,
            ..Default::default()
        };

        let loader = ash::khr::swapchain::Device::new(instance, device);
        let handle = unsafe {
            loader.create_swapchain(&create_info, None)
                .map_err(|e| {
                    ForgeError::Vulkan(format!(
                        "Failed to create swapchain (extent={}x{}, image_count={}, format={:?}, present_mode={:?}): {}",
                        extent.width,
                        extent.height,
                        image_count,
                        format.format,
                        present_mode,
                        e
                    ))
                })?
        };

        let images = unsafe {
            loader.get_swapchain_images(handle).map_err(|e| {
                ForgeError::Vulkan(format!(
                    "Failed to get swapchain images after creation: {}",
                    e
                ))
            })?
        };

        let image_views = Self::create_image_views(device, &images, format.format)?;

        tracing::info!(
            "Swapchain created: {}x{}, {} images, format={:?}, present={:?}",
            extent.width,
            extent.height,
            images.len(),
            format.format,
            present_mode
        );

        Ok(Self {
            loader,
            handle,
            images,
            image_views,
            format: format.format,
            extent,
        })
    }

    fn create_image_views(
        device: &Device,
        images: &[vk::Image],
        format: vk::Format,
    ) -> Result<Vec<vk::ImageView>> {
        images
            .iter()
            .map(|&image| {
                let create_info = vk::ImageViewCreateInfo {
                    image,
                    view_type: vk::ImageViewType::TYPE_2D,
                    format,
                    components: vk::ComponentMapping {
                        r: vk::ComponentSwizzle::IDENTITY,
                        g: vk::ComponentSwizzle::IDENTITY,
                        b: vk::ComponentSwizzle::IDENTITY,
                        a: vk::ComponentSwizzle::IDENTITY,
                    },
                    subresource_range: vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    },
                    ..Default::default()
                };
                unsafe {
                    device.create_image_view(&create_info, None).map_err(|e| {
                        ForgeError::Vulkan(format!(
                            "Failed to create swapchain image view for format {:?}: {}",
                            format, e
                        ))
                    })
                }
            })
            .collect()
    }

    /// Destroys all swapchain resources. Call before dropping or recreating.
    pub fn destroy(&mut self, device: &Device) {
        unsafe {
            for &view in &self.image_views {
                device.destroy_image_view(view, None);
            }
            self.loader.destroy_swapchain(self.handle, None);
        }
        tracing::debug!("Swapchain destroyed.");
    }
}
