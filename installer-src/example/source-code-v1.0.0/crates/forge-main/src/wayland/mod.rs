pub mod connection;
pub mod window;
pub mod shm_buffer;
pub mod seat;
pub mod frame_callback;
pub mod clipboard;
pub use connection::{WaylandState, connect_wayland};
