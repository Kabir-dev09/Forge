use wayland_client::{
    protocol::{wl_callback::{self, WlCallback}, wl_surface::WlSurface},
    Dispatch, QueueHandle,
};
use crate::wayland::connection::WaylandState;

/// Requests a frame callback from the compositor.
/// Call this after every frame commit.
pub fn request_frame_callback(surface: &WlSurface, qh: &QueueHandle<WaylandState>) -> WlCallback {
    surface.frame(qh, ())
}

impl Dispatch<WlCallback, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _callback: &WlCallback,
        event: wl_callback::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { callback_data: _ } = event {
            // The compositor is ready for the next frame.
            state.frame_ready = true;
            state.frame_callback_pending = false;
            tracing::trace!("Wayland frame callback fired.");
        }
    }
}
