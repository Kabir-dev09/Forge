use ash::{vk, Instance};
use forge_core::{Result, ForgeError};

/// Creates a Vulkan surface from a Wayland wl_display and wl_surface pointer.
/// These pointers come from the wayland-client objects.
pub fn create_wayland_surface(
    entry: &ash::Entry,
    instance: &Instance,
    wl_display: *mut std::ffi::c_void,
    wl_surface: *mut std::ffi::c_void,
) -> Result<vk::SurfaceKHR> {
    let wayland_surface_loader = ash::khr::wayland_surface::Instance::new(entry, instance);

    let create_info = vk::WaylandSurfaceCreateInfoKHR {
        display: wl_display,
        surface: wl_surface,
        ..Default::default()
    };

    unsafe {
        wayland_surface_loader.create_wayland_surface(&create_info, None)
            .map_err(|e| ForgeError::Vulkan(format!("Failed to create Wayland surface: {}", e)))
    }
}

/// Queries surface capabilities, formats, and present modes.
pub struct SurfaceDetails {
    pub capabilities: vk::SurfaceCapabilitiesKHR,
    pub formats: Vec<vk::SurfaceFormatKHR>,
    pub present_modes: Vec<vk::PresentModeKHR>,
}

impl SurfaceDetails {
    pub fn query(
        surface_loader: &ash::khr::surface::Instance,
        physical_device: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
    ) -> Result<Self> {
        unsafe {
            let capabilities = surface_loader
                .get_physical_device_surface_capabilities(physical_device, surface)
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
            let formats = surface_loader
                .get_physical_device_surface_formats(physical_device, surface)
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
            let present_modes = surface_loader
                .get_physical_device_surface_present_modes(physical_device, surface)
                .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
            Ok(SurfaceDetails { capabilities, formats, present_modes })
        }
    }

    /// Selects the best surface format.
    /// Prefers B8G8R8A8_SRGB with COLOR_SPACE_SRGB_NONLINEAR.
    /// Falls back to the first available format.
    pub fn choose_format(&self) -> Result<vk::SurfaceFormatKHR> {
        if self.formats.is_empty() {
            return Err(ForgeError::Vulkan("No surface formats available".to_string()));
        }

        Ok(self.formats.iter().find(|f| {
            f.format == vk::Format::B8G8R8A8_SRGB
                && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        }).copied().unwrap_or(self.formats[0]))
    }

    /// Selects the best present mode.
    /// Prefers MAILBOX (low latency, no tearing) over FIFO (v-sync).
    /// Falls back to FIFO (guaranteed available).
    pub fn choose_present_mode(&self) -> vk::PresentModeKHR {
        if self.present_modes.contains(&vk::PresentModeKHR::MAILBOX) {
            vk::PresentModeKHR::MAILBOX
        } else {
            vk::PresentModeKHR::FIFO
        }
    }

    /// Chooses the swapchain extent (resolution).
    /// Uses the surface capabilities to clamp the desired size.
    pub fn choose_extent(&self, desired_width: u32, desired_height: u32) -> vk::Extent2D {
        if self.capabilities.current_extent.width != u32::MAX {
            self.capabilities.current_extent
        } else {
            vk::Extent2D {
                width: desired_width.clamp(
                    self.capabilities.min_image_extent.width,
                    self.capabilities.max_image_extent.width,
                ),
                height: desired_height.clamp(
                    self.capabilities.min_image_extent.height,
                    self.capabilities.max_image_extent.height,
                ),
            }
        }
    }
}
