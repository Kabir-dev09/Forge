//! Forge Vulkan Rendering Backend
pub mod instance;
pub mod device;
pub mod surface;
pub mod swapchain;
pub mod render_pass;
pub mod framebuffer;
pub mod sync;
pub mod frame;
pub mod pipeline;
pub mod font;
pub mod texture;
pub mod renderer;
pub mod grid_tessellator;

pub use renderer::Renderer;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(not(feature = "vulkan-test"), ignore)]
    fn test_vulkan_initialization() {
        // Initialize logging so we can see the debug output
        let _ = tracing_subscriber::fmt::try_init();

        let entry = instance::create_entry().expect("Failed to load Vulkan Entry");
        instance::log_instance_extensions(&entry);

        let instance = instance::create_instance(&entry).expect("Failed to create Vulkan Instance");

        // Enumerate physical devices
        let devices = unsafe { instance.enumerate_physical_devices() }.expect("Failed to enumerate devices");
        assert!(!devices.is_empty(), "No Vulkan devices found on this system.");

        tracing::info!("Found {} Vulkan devices.", devices.len());
        for &device in &devices {
            let props = unsafe { instance.get_physical_device_properties(device) };
            let name = unsafe { std::ffi::CStr::from_ptr(props.device_name.as_ptr()) };
            tracing::info!("Device: {:?} (type: {:?})", name, props.device_type);
        }

        // We can't fully test select_physical_device because we don't have a Wayland surface here,
        // but verifying entry and instance creation is enough for this module's basic tests.

        unsafe {
            instance.destroy_instance(None);
        }
    }
}
