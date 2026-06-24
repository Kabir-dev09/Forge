use ash::{vk, Device};
use forge_core::{Result, ForgeError};
use super::sync::{SyncPrimitives, MAX_FRAMES_IN_FLIGHT};
use super::swapchain::Swapchain;

/// Acquires the next swapchain image, records a clear-color command buffer,
/// submits it to the graphics queue, and presents the result.
///
/// Returns `Ok(true)` if the swapchain needs to be recreated (e.g., window resized).
/// Returns `Ok(false)` on success.
#[allow(clippy::too_many_arguments)]
pub fn render_frame(
    device: &Device,
    swapchain: &Swapchain,
    render_pass: vk::RenderPass,
    framebuffers: &[vk::Framebuffer],
    command_buffers: &[vk::CommandBuffer],
    sync: &SyncPrimitives,
    graphics_queue: vk::Queue,
    present_queue: vk::Queue,
    current_frame: &mut usize,
    clear_color: [f32; 4],
) -> Result<bool> {
    debug_assert!(clear_color.iter().all(|&c| c.is_finite()), "NaN or Inf in clear_color");
    let frame = *current_frame;

    // 1. Wait for this frame's fence (ensures we don't re-use in-flight resources).
    unsafe {
        device.wait_for_fences(
            &[sync.in_flight_fences[frame]],
            true,
            u64::MAX,
        ).map_err(|e| ForgeError::Vulkan(e.to_string()))?;
    }

    // 2. Acquire next image from swapchain.
    // Note: u64::MAX timeout means this will block indefinitely. For our single-threaded
    // Wayland event loop, this might block event processing if the compositor stalls.
    // In the future, this should be non-blocking or moved to a dedicated render thread.
    let (image_index, suboptimal) = unsafe {
        match swapchain.loader.acquire_next_image(
            swapchain.handle,
            u64::MAX,
            sync.image_available_semaphores[frame],
            vk::Fence::null(),
        ) {
            Ok(result) => result,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return Ok(true), // Needs recreate
            Err(vk::Result::ERROR_SURFACE_LOST_KHR) => return Err(ForgeError::Vulkan("Surface lost".to_string())),
            Err(e) => return Err(ForgeError::Vulkan(format!("acquire_next_image failed: {}", e))),
        }
    };

    if image_index as usize >= framebuffers.len() {
        tracing::error!("Acquired image index {} exceeds framebuffers len {}", image_index, framebuffers.len());
        return Err(ForgeError::Vulkan("Image index out of bounds".to_string()));
    }

    // 3. Reset fence now that we know we're going to submit work.
    unsafe {
        device.reset_fences(&[sync.in_flight_fences[frame]])
            .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
    }

    // 4. Record command buffer: begin → begin render pass → clear → end render pass → end.
    let cmd = command_buffers[frame];
    record_clear_command(device, cmd, render_pass, framebuffers[image_index as usize], swapchain.extent, clear_color)?;

    // 5. Submit to graphics queue.
    let wait_semaphores = [sync.image_available_semaphores[frame]];
    let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
    let signal_semaphores = [sync.render_finished_semaphores[frame]];
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
        device.queue_submit(graphics_queue, &[submit_info], sync.in_flight_fences[frame])
            .map_err(|e| ForgeError::Vulkan(format!("queue_submit failed: {}", e)))?;
    }

    // 6. Present the image.
    let swapchains = [swapchain.handle];
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
        match swapchain.loader.queue_present(present_queue, &present_info) {
            Ok(true) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => true,
            Ok(false) => suboptimal,
            Err(vk::Result::ERROR_SURFACE_LOST_KHR) => return Err(ForgeError::Vulkan("Surface lost".to_string())),
            Err(e) => return Err(ForgeError::Vulkan(format!("queue_present failed: {}", e))),
        }
    };

    // 7. Advance frame counter.
    *current_frame = (frame + 1) % MAX_FRAMES_IN_FLIGHT;

    Ok(needs_recreate)
}

fn record_clear_command(
    device: &Device,
    cmd: vk::CommandBuffer,
    render_pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    extent: vk::Extent2D,
    clear_color: [f32; 4],
) -> Result<()> {
    unsafe {
        // Reset and begin the command buffer.
        device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
            .map_err(|e| ForgeError::Vulkan(e.to_string()))?;

        let begin_info = vk::CommandBufferBeginInfo {
            flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
            ..Default::default()
        };
        device.begin_command_buffer(cmd, &begin_info)
            .map_err(|e| ForgeError::Vulkan(e.to_string()))?;

        // Begin render pass with clear color.
        let clear_value = vk::ClearValue {
            color: vk::ClearColorValue { float32: clear_color },
        };
        let render_pass_begin = vk::RenderPassBeginInfo {
            render_pass,
            framebuffer,
            render_area: vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent,
            },
            clear_value_count: 1,
            p_clear_values: &clear_value,
            ..Default::default()
        };
        device.cmd_begin_render_pass(cmd, &render_pass_begin, vk::SubpassContents::INLINE);
        // (Draw calls will go here in future steps)
        device.cmd_end_render_pass(cmd);
        device.end_command_buffer(cmd)
            .map_err(|e| ForgeError::Vulkan(e.to_string()))?;
    }
    Ok(())
}
