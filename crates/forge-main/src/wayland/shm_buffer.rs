use super::connection::WaylandState;
use forge_core::geometry::Size;
use forge_core::{ForgeError, Result};
use std::os::fd::AsFd;
use wayland_client::{
    protocol::{
        wl_buffer::WlBuffer,
        wl_shm::{self, WlShm},
        wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    },
    QueueHandle,
};

pub struct ShmBuffer {
    pub pool: WlShmPool,
    pub buffer: WlBuffer,
    /// Raw pointer to the mapped memory. We own this mapping.
    data: *mut u8,
    pub size: Size,
    pub stride: i32,
    pub pool_size: usize,
}

// SAFETY: The *mut u8 pointer is valid for the lifetime of ShmBuffer and
// is only accessed from the main thread.
unsafe impl Send for ShmBuffer {}

impl ShmBuffer {
    pub fn new(shm: &WlShm, qh: &QueueHandle<WaylandState>, size: Size) -> Result<Self> {
        if size.width == 0 || size.height == 0 {
            return Err(ForgeError::Wayland(
                "Cannot create SHM buffer with zero dimensions".to_string(),
            ));
        }

        let stride = (size.width as i32) * 4; // 4 bytes per pixel
        let pool_size = (stride as usize) * (size.height as usize);

        let memfd = rustix::fs::memfd_create(
            "forge-shm",
            rustix::fs::MemfdFlags::CLOEXEC | rustix::fs::MemfdFlags::ALLOW_SEALING,
        )
        .map_err(|e| ForgeError::Wayland(format!("memfd_create failed: {}", e)))?;

        rustix::fs::ftruncate(&memfd, pool_size as u64)
            .map_err(|e| ForgeError::Wayland(format!("ftruncate failed: {}", e)))?;

        let data = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                pool_size,
                rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
                rustix::mm::MapFlags::SHARED,
                &memfd,
                0,
            )
        }
        .map_err(|e| ForgeError::Wayland(format!("mmap failed: {}", e)))?;

        let pool = shm.create_pool(memfd.as_fd(), pool_size as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            size.width as i32,
            size.height as i32,
            stride,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );

        Ok(ShmBuffer {
            pool,
            buffer,
            data: data as *mut u8,
            size,
            stride,
            pool_size,
        })
    }

    pub fn fill_color(&mut self, r: u8, g: u8, b: u8, a: u8) {
        // Write ARGB8888 pixels (little-endian: stored as B, G, R, A).
        let pixel: u32 = ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        let count = self.pool_size / 4;
        unsafe {
            let pixels = std::slice::from_raw_parts_mut(self.data as *mut u32, count);
            pixels.fill(pixel);
        }
    }

    pub fn present(&self, surface: &WlSurface) {
        surface.attach(Some(&self.buffer), 0, 0);
        surface.damage_buffer(0, 0, self.size.width as i32, self.size.height as i32);
        surface.commit();
    }
}

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        unsafe {
            if let Err(e) = rustix::mm::munmap(self.data as *mut _, self.pool_size) {
                tracing::warn!("ShmBuffer munmap failed: {}", e);
            }
        }
    }
}
