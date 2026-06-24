use ash::{vk, Device};
use forge_core::{Result, ForgeError};

/// Creates one framebuffer per swapchain image view.
/// Each framebuffer references the render pass and the corresponding image view.
pub fn create_framebuffers(
    device: &Device,
    render_pass: vk::RenderPass,
    image_views: &[vk::ImageView],
    extent: vk::Extent2D,
) -> Result<Vec<vk::Framebuffer>> {
    image_views.iter().map(|&view| {
        let attachments = [view];
        let framebuffer_info = vk::FramebufferCreateInfo {
            render_pass,
            attachment_count: attachments.len() as u32,
            p_attachments: attachments.as_ptr(),
            width: extent.width,
            height: extent.height,
            layers: 1,
            ..Default::default()
        };
        unsafe {
            device.create_framebuffer(&framebuffer_info, None)
                .map_err(|e| ForgeError::Vulkan(format!("Failed to create framebuffer: {}", e)))
        }
    }).collect()
}

pub fn destroy_framebuffers(device: &Device, framebuffers: &[vk::Framebuffer]) {
    unsafe {
        for &fb in framebuffers {
            device.destroy_framebuffer(fb, None);
        }
    }
}
