use forge_core::{ForgeError, Result};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use crate::wayland::window::WaylandWindow;
use xkbcommon::xkb;

pub struct WaylandGlobals {
    pub compositor: wl_compositor::WlCompositor,
    pub shm: wl_shm::WlShm,
    pub xdg_wm_base: xdg_wm_base::XdgWmBase,
    pub wl_seat: Option<wl_seat::WlSeat>,
    pub data_device_manager: Option<wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager>,
    pub zxdg_decoration_manager: Option<wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1>,
    pub cursor_shape_manager: Option<wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1>,
}

#[derive(Debug, Clone, Copy)]
pub enum PointerEvent {
    Motion { x: f64, y: f64 },
    Press { button: u32 },
    Release { button: u32 },
    Axis { amount: f64 },
}

pub struct RepeatingKey {
    pub key: u32,
    pub bytes: Vec<u8>,
    pub next_repeat_time: std::time::Instant,
}

pub struct WaylandState {
    pub globals: WaylandGlobals,
    pub window: Option<WaylandWindow>,
    pub shm_buffer: Option<crate::wayland::shm_buffer::ShmBuffer>,
    pub conn: Connection,
    pub running: bool,
    pub is_fullscreen: bool,
    pub xkb_context: xkb::Context,
    pub xkb_state: Option<xkb::State>,
    pub key_sender: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    pub pointer_sender: Option<std::sync::mpsc::SyncSender<PointerEvent>>,
    pub pointer: Option<wayland_client::protocol::wl_pointer::WlPointer>,
    pub pointer_serial: u32,
    pub cursor_hidden: bool,
    pub hide_mouse_when_typing: bool,
    pub is_alt_buffer: bool,
    pub clipboard: Option<crate::wayland::clipboard::ClipboardManager>,
    pub frame_ready: bool,
    pub frame_callback_pending: bool,
    pub needs_flush: bool,
    pub force_redraw: bool,
    pub repeat_info: Option<(i32, i32)>, // (rate, delay)
    pub repeating_key: Option<RepeatingKey>,
    pub keybindings: std::collections::HashMap<forge_core::bindings::KeyStroke, forge_core::bindings::Action>,
}

pub fn connect_wayland() -> Result<(WaylandState, EventQueue<WaylandState>)> {
    let conn = Connection::connect_to_env()
        .map_err(|e| ForgeError::Wayland(format!("Connection::connect_to_env failed: {}", e)))?;

    let (globals_list, event_queue) = registry_queue_init::<WaylandState>(&conn)
        .map_err(|e| ForgeError::Wayland(format!("registry_queue_init failed: {}", e)))?;

    let qh = event_queue.handle();

    let compositor: wl_compositor::WlCompositor = globals_list
        .bind(&qh, 4..=6, ())
        .map_err(|e| ForgeError::Wayland(format!("Failed to bind wl_compositor: {}", e)))?;

    let shm: wl_shm::WlShm = globals_list
        .bind(&qh, 1..=1, ())
        .map_err(|e| ForgeError::Wayland(format!("Failed to bind wl_shm: {}", e)))?;

    let xdg_wm_base: xdg_wm_base::XdgWmBase = globals_list
        .bind(&qh, 2..=5, ())
        .map_err(|e| ForgeError::Wayland(format!("Failed to bind xdg_wm_base: {}", e)))?;

    let wl_seat: Option<wl_seat::WlSeat> = globals_list.bind(&qh, 7..=8, ()).ok();
    
    let data_device_manager: Option<wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager> = 
        globals_list.bind(&qh, 1..=3, ()).ok();

    let zxdg_decoration_manager = globals_list.bind(&qh, 1..=1, ()).ok();

    let cursor_shape_manager = globals_list.bind(&qh, 1..=1, ()).ok();

    let globals = WaylandGlobals {
        compositor,
        shm,
        xdg_wm_base,
        wl_seat,
        data_device_manager,
        zxdg_decoration_manager,
        cursor_shape_manager,
    };

    let state = WaylandState {
        globals,
        window: None,
        shm_buffer: None,
        conn: conn.clone(),
        running: true,
        is_fullscreen: false,
        xkb_context: xkb::Context::new(xkb::CONTEXT_NO_FLAGS),
        xkb_state: None,
        key_sender: None,
        pointer_sender: None,
        pointer: None,
        pointer_serial: 0,
        cursor_hidden: false,
        hide_mouse_when_typing: false,
        is_alt_buffer: false,
        clipboard: None,
        frame_ready: true,
        frame_callback_pending: false,
        needs_flush: false,
        force_redraw: false,
        repeat_info: None,
        repeating_key: None,
        keybindings: std::collections::HashMap::new(),
    };

    Ok((state, event_queue))
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: <wl_registry::WlRegistry as wayland_client::Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_compositor::WlCompositor,
        _event: <wl_compositor::WlCompositor as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        tracing::trace!("wl_compositor event");
    }
}

impl Dispatch<wl_shm::WlShm, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm::WlShm,
        event: <wl_shm::WlShm as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let wl_shm::Event::Format { format } = event {
            tracing::trace!("Supported SHM format: {:?}", format);
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for WaylandState {
    fn event(
        state: &mut Self,
        proxy: &xdg_wm_base::XdgWmBase,
        event: <xdg_wm_base::XdgWmBase as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            proxy.pong(serial);
            state.needs_flush = true;
            tracing::trace!("xdg_wm_base pong sent");
        }
    }
}



impl Dispatch<wl_surface::WlSurface, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_surface::WlSurface,
        event: <wl_surface::WlSurface as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_surface::Event::Enter { .. } => {
                tracing::debug!("wl_surface entered monitor");
            }
            wl_surface::Event::Leave { .. } => {
                tracing::debug!("wl_surface left monitor");
            }
            _ => {
                tracing::trace!("wl_surface event: {:?}", event);
            }
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for WaylandState {
    fn event(
        state: &mut Self,
        proxy: &xdg_surface::XdgSurface,
        event: <xdg_surface::XdgSurface as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            proxy.ack_configure(serial);
            state.needs_flush = true;
            if let Some(window) = &mut state.window {
                window.configured = true;
            }
            tracing::debug!("XDG surface configured, serial={}", serial);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _proxy: &xdg_toplevel::XdgToplevel,
        event: <xdg_toplevel::XdgToplevel as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { width, height, states } => {
                let mut is_maximized = false;
                let mut is_fullscreen = false;
                let mut is_resizing = false;
                let mut is_activated = false;
                
                // Parse states
                for state_bytes in states.chunks_exact(4) {
                    if let Ok(state_val) = TryInto::<[u8; 4]>::try_into(state_bytes) {
                        let state = u32::from_ne_bytes(state_val);
                        match state {
                            2 => is_maximized = true, // xdg_toplevel::State::Maximized
                            3 => is_fullscreen = true, // xdg_toplevel::State::Fullscreen
                            4 => is_resizing = true, // xdg_toplevel::State::Resizing
                            5 => is_activated = true, // xdg_toplevel::State::Activated
                            _ => {}
                        }
                    }
                }

                tracing::debug!("XDG toplevel configured, width={}, height={}, maximized={}, fullscreen={}, resizing={}, activated={}", width, height, is_maximized, is_fullscreen, is_resizing, is_activated);

                if width > 0 && height > 0 {
                    if let Some(window) = &mut state.window {
                        window.size.width = width as u32;
                        window.size.height = height as u32;
                    }
                }
            }
            xdg_toplevel::Event::Close => {
                state.running = false;
                tracing::debug!("XDG toplevel close requested");
            }
            _ => {
                tracing::trace!("xdg_toplevel event: {:?}", event);
            }
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_buffer::WlBuffer,
        event: <wl_buffer::WlBuffer as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            tracing::trace!("wl_buffer released");
        }
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm_pool::WlShmPool,
        _event: <wl_shm_pool::WlShmPool as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl wayland_client::Dispatch<wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
        _event: <wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl wayland_client::Dispatch<wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1,
        _event: <wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}
