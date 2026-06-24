use wayland_client::{
    protocol::{wl_surface::WlSurface, wl_compositor::WlCompositor},
    QueueHandle,
};
use wayland_protocols::xdg::shell::client::{
    xdg_surface::XdgSurface,
    xdg_toplevel::XdgToplevel,
    xdg_wm_base::XdgWmBase,
};
use forge_core::geometry::Size;
use forge_core::Result;
use super::connection::WaylandState;

pub struct WaylandWindow {
    pub surface: WlSurface,
    pub xdg_surface: XdgSurface,
    pub xdg_toplevel: XdgToplevel,
    pub decoration: Option<wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1>,
    pub configured: bool,
    pub size: Size,
}

impl WaylandWindow {
    pub fn new(
        compositor: &WlCompositor,
        xdg_wm_base: &XdgWmBase,
        decoration_manager: Option<&wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1>,
        qh: &QueueHandle<WaylandState>,
        initial_size: Size,
        title: &str,
    ) -> Result<Self> {
        let surface = compositor.create_surface(qh, ());
        let xdg_surface = xdg_wm_base.get_xdg_surface(&surface, qh, ());
        let xdg_toplevel = xdg_surface.get_toplevel(qh, ());
        
        xdg_toplevel.set_title(title.to_string());
        xdg_toplevel.set_app_id("dev.forge.terminal".to_string());
        
        let decoration = if let Some(mgr) = decoration_manager {
            let decor = mgr.get_toplevel_decoration(&xdg_toplevel, qh, ());
            decor.set_mode(wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode::ServerSide);
            Some(decor)
        } else {
            None
        };
        
        surface.commit();
        
        Ok(WaylandWindow {
            surface,
            xdg_surface,
            xdg_toplevel,
            decoration,
            configured: false,
            size: initial_size,
        })
    }
}
