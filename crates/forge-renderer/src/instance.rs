use ash::{vk, Entry, Instance};
use forge_core::{ForgeError, Result};
use std::ffi::{CStr, CString};

/// Required instance extensions for Wayland rendering.
pub const REQUIRED_INSTANCE_EXTENSIONS: &[&CStr] = &[
    ash::khr::surface::NAME,
    ash::khr::wayland_surface::NAME,
    // In debug builds only:
    // ash::ext::debug_utils::NAME,
];

/// Creates the Vulkan Entry (loads the Vulkan library).
/// Returns Err if Vulkan is not available on this system.
pub fn create_entry() -> Result<Entry> {
    unsafe { Entry::load() }
        .map_err(|e| ForgeError::Vulkan(format!("Failed to load Vulkan: {}", e)))
}

/// Creates the Vulkan instance with the required extensions.
pub fn create_instance(entry: &Entry) -> Result<Instance> {
    // unwrap() is safe because these literals contain no null bytes
    let app_name = CString::new("Forge").unwrap();
    let engine_name = CString::new("ForgeRenderer").unwrap();

    let app_info = vk::ApplicationInfo {
        p_application_name: app_name.as_ptr(),
        application_version: vk::make_api_version(0, 0, 1, 0),
        p_engine_name: engine_name.as_ptr(),
        engine_version: vk::make_api_version(0, 0, 1, 0),
        api_version: vk::API_VERSION_1_3,
        ..Default::default()
    };

    // Build extension name pointers
    let ext_names: Vec<*const i8> = REQUIRED_INSTANCE_EXTENSIONS
        .iter()
        .map(|s| s.as_ptr())
        .collect();

    // In debug builds, add validation layers if available:
    #[cfg(debug_assertions)]
    let layer_names = {
        let available_layers =
            unsafe { entry.enumerate_instance_layer_properties() }.unwrap_or_default();
        let has_validation = available_layers.iter().any(|layer| {
            let name = unsafe { CStr::from_ptr(layer.layer_name.as_ptr()) };
            name == c"VK_LAYER_KHRONOS_validation"
        });

        if has_validation {
            // unwrap() is safe because the literal contains no null bytes
            vec![CString::new("VK_LAYER_KHRONOS_validation").unwrap()]
        } else {
            tracing::warn!("Vulkan validation layers not found. Running without them.");
            vec![]
        }
    };

    #[cfg(not(debug_assertions))]
    let layer_names: Vec<CString> = vec![];

    let layer_ptrs: Vec<*const i8> = layer_names.iter().map(|s| s.as_ptr()).collect();

    let create_info = vk::InstanceCreateInfo {
        p_application_info: &app_info,
        enabled_extension_count: ext_names.len() as u32,
        pp_enabled_extension_names: ext_names.as_ptr(),
        enabled_layer_count: layer_ptrs.len() as u32,
        pp_enabled_layer_names: layer_ptrs.as_ptr(),
        ..Default::default()
    };

    unsafe {
        entry
            .create_instance(&create_info, None)
            .map_err(|e| ForgeError::Vulkan(format!("Failed to create Vulkan instance: {}", e)))
    }
}

/// Logs available Vulkan instance extensions at DEBUG level.
pub fn log_instance_extensions(entry: &Entry) {
    match unsafe { entry.enumerate_instance_extension_properties(None) } {
        Ok(exts) => {
            tracing::debug!("Available Vulkan instance extensions: {}", exts.len());
            for ext in &exts {
                let name = unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) };
                tracing::trace!("  Extension: {:?}", name);
            }
        }
        Err(e) => tracing::warn!("Could not enumerate Vulkan extensions: {}", e),
    }
}
