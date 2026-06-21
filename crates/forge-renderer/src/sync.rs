use ash::{vk, Device};
use forge_core::{Result, ForgeError};

/// Maximum number of frames that can be in-flight simultaneously.
/// 2 is the standard choice for double-buffered rendering.
pub const MAX_FRAMES_IN_FLIGHT: usize = 2;

pub struct SyncPrimitives {
    /// Signaled when an image has been acquired from the swapchain. One per frame.
    pub image_available_semaphores: Vec<vk::Semaphore>,
    /// Signaled when rendering to the image is complete. One per frame.
    pub render_finished_semaphores: Vec<vk::Semaphore>,
    /// Fences to prevent CPU from getting too far ahead of GPU. One per frame.
    pub in_flight_fences: Vec<vk::Fence>,
}

impl SyncPrimitives {
    pub fn new(device: &Device) -> Result<Self> {
        let semaphore_info = vk::SemaphoreCreateInfo::default();
        let fence_info = vk::FenceCreateInfo {
            // Start fences in signaled state so the first frame doesn't wait forever.
            flags: vk::FenceCreateFlags::SIGNALED,
            ..Default::default()
        };

        let mut image_available_semaphores = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);
        let mut render_finished_semaphores = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);
        let mut in_flight_fences = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);

        for _ in 0..MAX_FRAMES_IN_FLIGHT {
            unsafe {
                image_available_semaphores.push(
                    device.create_semaphore(&semaphore_info, None)
                        .map_err(|e| ForgeError::Vulkan(e.to_string()))?
                );
                render_finished_semaphores.push(
                    device.create_semaphore(&semaphore_info, None)
                        .map_err(|e| ForgeError::Vulkan(e.to_string()))?
                );
                in_flight_fences.push(
                    device.create_fence(&fence_info, None)
                        .map_err(|e| ForgeError::Vulkan(e.to_string()))?
                );
            }
        }

        Ok(Self { image_available_semaphores, render_finished_semaphores, in_flight_fences })
    }

    pub fn destroy(&self, device: &Device) {
        unsafe {
            for &s in &self.image_available_semaphores { device.destroy_semaphore(s, None); }
            for &s in &self.render_finished_semaphores { device.destroy_semaphore(s, None); }
            for &f in &self.in_flight_fences { device.destroy_fence(f, None); }
        }
    }
}
