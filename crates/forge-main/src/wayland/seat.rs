use wayland_client::protocol::{wl_keyboard, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use xkbcommon::xkb;
use std::os::unix::io::AsRawFd;

use super::connection::WaylandState;

impl Dispatch<wl_seat::WlSeat, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            let capabilities = match capabilities {
                wayland_client::WEnum::Value(c) => c,
                _ => return,
            };
            if capabilities.contains(wl_seat::Capability::Keyboard) {
                seat.get_keyboard(qh, ());
                tracing::info!("Keyboard capability acquired.");
            }
            if capabilities.contains(wl_seat::Capability::Pointer) {
                seat.get_pointer(qh, ());
                tracing::info!("Pointer capability acquired.");
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _keyboard: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                let format = match format {
                    wayland_client::WEnum::Value(f) => f,
                    _ => return,
                };
                if format == wl_keyboard::KeymapFormat::XkbV1 {
                    let mmap = unsafe {
                        memmap2::MmapOptions::new()
                            .len(size as usize)
                            .map(fd.as_raw_fd())
                    };
                    
                    if let Ok(mmap) = mmap {
                        let keymap_bytes = mmap.split(|&b| b == 0).next().unwrap_or(&mmap);
                        
                        if let Ok(keymap_str) = std::str::from_utf8(keymap_bytes) {
                            let keymap = xkb::Keymap::new_from_string(
                                &state.xkb_context,
                                keymap_str.to_string(),
                                xkb::KEYMAP_FORMAT_TEXT_V1,
                                xkb::KEYMAP_COMPILE_NO_FLAGS,
                            );
                            
                            if let Some(km) = keymap {
                                state.xkb_state = Some(xkb::State::new(&km));
                                tracing::info!("xkb keymap loaded successfully.");
                            } else {
                                tracing::error!("Failed to parse xkb keymap string.");
                            }
                        } else {
                            tracing::error!("Keymap was not valid utf-8");
                        }
                    } else {
                        tracing::error!("Failed to mmap the keymap fd.");
                    }
                }
            }
            
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                tracing::info!("Keyboard repeat info: rate={}, delay={}", rate, delay);
                state.repeat_info = Some((rate, delay));
            }
            
            wl_keyboard::Event::Modifiers { mods_depressed, mods_latched, mods_locked, group, .. } => {
                if let Some(xkb_state) = &mut state.xkb_state {
                    xkb_state.update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                }
            }
            
            wl_keyboard::Event::Key { key, state: key_state, .. } => {
                let key_state = match key_state {
                    wayland_client::WEnum::Value(s) => s,
                    _ => return,
                };
                if key_state == wl_keyboard::KeyState::Pressed {
                    let keycode = key + 8; 
                    
                    if let Some(xkb_state) = &state.xkb_state {
                        let keysym = xkb_state.key_get_one_sym(keycode.into());
                        let utf8 = xkb_state.key_get_utf8(keycode.into());
                        
                        tracing::info!("Key pressed: sym={:?}, char={:?}", keysym, utf8);
                        
                        let mut bytes = Vec::new();
                        let keysym_u32: u32 = keysym.into();
                        let ctrl_active = xkb_state.mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
                        let shift_active = xkb_state.mod_name_is_active(xkb::MOD_NAME_SHIFT, xkb::STATE_MODS_EFFECTIVE);
                        let alt_active = xkb_state.mod_name_is_active(xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE);
                        let logo_active = xkb_state.mod_name_is_active(xkb::MOD_NAME_LOGO, xkb::STATE_MODS_EFFECTIVE);
                        
                        let mut active_modifiers = forge_core::bindings::modifiers::NONE;
                        if ctrl_active { active_modifiers |= forge_core::bindings::modifiers::CTRL; }
                        if shift_active { active_modifiers |= forge_core::bindings::modifiers::SHIFT; }
                        if alt_active { active_modifiers |= forge_core::bindings::modifiers::ALT; }
                        if logo_active { active_modifiers |= forge_core::bindings::modifiers::LOGO; }
                        
                        let mut normalized_keysym = keysym_u32;
                        if normalized_keysym >= 0x0041 && normalized_keysym <= 0x005A {
                            // Convert uppercase ASCII keysyms to lowercase
                            normalized_keysym += 0x0020;
                        }

                        let keystroke = forge_core::bindings::KeyStroke {
                            modifiers: active_modifiers,
                            keysym: normalized_keysym,
                        };
                        
                        if let Some(action) = state.keybindings.get(&keystroke) {
                            match action {
                                forge_core::bindings::Action::Copy => {
                                    tracing::info!("Intercepted Copy via keybind");
                                }
                                forge_core::bindings::Action::Paste => {
                                    tracing::info!("[PASTE TIMING] Intercepted Paste at {:?}", std::time::Instant::now());
                                    if let Some(clip) = &state.clipboard {
                                        clip.request_paste();
                                        state.needs_flush = true;
                                    }
                                }
                                forge_core::bindings::Action::ToggleFullscreen => {
                                    tracing::info!("ToggleFullscreen requested");
                                    if let Some(window) = &state.window {
                                        if state.is_fullscreen {
                                            window.xdg_toplevel.unset_fullscreen();
                                            state.is_fullscreen = false;
                                        } else {
                                            window.xdg_toplevel.set_fullscreen(None);
                                            state.is_fullscreen = true;
                                        }
                                        state.needs_flush = true;
                                    }
                                }
                            }
                        } else {
                            match keysym_u32 {
                                xkbcommon::xkb::keysyms::KEY_Return => bytes.extend_from_slice(b"\r"),
                                xkbcommon::xkb::keysyms::KEY_BackSpace => bytes.extend_from_slice(b"\x7f"),
                                xkbcommon::xkb::keysyms::KEY_Tab => bytes.extend_from_slice(b"\t"),
                                xkbcommon::xkb::keysyms::KEY_Escape => bytes.extend_from_slice(b"\x1b"),
                                xkbcommon::xkb::keysyms::KEY_Up => bytes.extend_from_slice(b"\x1b[A"),
                                xkbcommon::xkb::keysyms::KEY_Down => bytes.extend_from_slice(b"\x1b[B"),
                                xkbcommon::xkb::keysyms::KEY_Right => bytes.extend_from_slice(b"\x1b[C"),
                                xkbcommon::xkb::keysyms::KEY_Left => bytes.extend_from_slice(b"\x1b[D"),
                                xkbcommon::xkb::keysyms::KEY_Delete => bytes.extend_from_slice(b"\x1b[3~"),
                                xkbcommon::xkb::keysyms::KEY_Home => bytes.extend_from_slice(b"\x1b[H"),
                                xkbcommon::xkb::keysyms::KEY_End => bytes.extend_from_slice(b"\x1b[F"),
                                _ => {
                                    if ctrl_active && !utf8.is_empty() {
                                        let c = utf8.chars().next().unwrap();
                                        if c.is_ascii_alphabetic() {
                                            let byte = c.to_ascii_lowercase() as u8 - b'a' + 1;
                                            bytes.push(byte);
                                        } else {
                                            bytes.extend_from_slice(utf8.as_bytes());
                                        }
                                    } else if !utf8.is_empty() {
                                        bytes.extend_from_slice(utf8.as_bytes());
                                    }
                                }
                            }
                        }
                        
                        if !bytes.is_empty() {
                            if state.hide_mouse_when_typing && !state.cursor_hidden {
                                if let Some(pointer) = &state.pointer {
                                    state.cursor_hidden = true;
                                    pointer.set_cursor(state.pointer_serial, None, 0, 0);
                                }
                            }
                            
                            if let Some(tx) = &state.key_sender {
                                let _ = tx.send(bytes.clone());
                            }
                            
                            // Start auto-repeat if enabled
                            if let Some((rate, delay)) = state.repeat_info {
                                if rate > 0 {
                                    state.repeating_key = Some(crate::wayland::connection::RepeatingKey {
                                        key,
                                        bytes,
                                        next_repeat_time: std::time::Instant::now() + std::time::Duration::from_millis(delay as u64),
                                    });
                                }
                            }
                        }
                    }
                } else if key_state == wl_keyboard::KeyState::Released {
                    if let Some(repeating) = &state.repeating_key {
                        if repeating.key == key {
                            state.repeating_key = None;
                        }
                    }
                }
            }
            
            wl_keyboard::Event::Leave { .. } => {
                state.repeating_key = None;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xkb_keymap_loading() {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap_str = "xkb_keymap { \
            xkb_keycodes  { include \"xfree86+aliases(qwerty)\" }; \
            xkb_types     { include \"complete\" }; \
            xkb_compat    { include \"complete\" }; \
            xkb_symbols   { include \"pc+us+inet(evdev)\" }; \
            xkb_geometry  { include \"pc(pc105)\" }; \
        };";
        let keymap = xkb::Keymap::new_from_string(
            &context,
            keymap_str.to_string(),
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        );
        assert!(keymap.is_some(), "Failed to load valid keymap from string");
    }
}

use wayland_client::protocol::wl_pointer;
use crate::wayland::connection::PointerEvent;

impl Dispatch<wl_pointer::WlPointer, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _pointer: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { serial, surface: _, surface_x, surface_y } => {
                state.pointer = Some(_pointer.clone());
                state.pointer_serial = serial;
                
                if state.cursor_hidden {
                    _pointer.set_cursor(serial, None, 0, 0);
                }
                
                if let Some(tx) = &state.pointer_sender {
                    let _ = tx.send(PointerEvent::Enter { x: surface_x, y: surface_y });
                }
            }
            wl_pointer::Event::Leave { .. } => {
                state.pointer = None;
                if let Some(tx) = &state.pointer_sender {
                    let _ = tx.send(PointerEvent::Leave);
                }
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                if state.cursor_hidden {
                    state.cursor_hidden = false;
                    if let Some(shape_manager) = &state.globals.cursor_shape_manager {
                        let device = shape_manager.get_pointer(_pointer, _qh, ());
                        let shape = if state.is_hovering_edge {
                            wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape::Default
                        } else if state.is_alt_buffer {
                            wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape::Default
                        } else {
                            wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape::Text
                        };
                        device.set_shape(state.pointer_serial, shape);
                        device.destroy();
                    }
                }
                
                if let Some(tx) = &state.pointer_sender {
                    let _ = tx.send(PointerEvent::Motion { x: surface_x, y: surface_y });
                }
            }
            wl_pointer::Event::Button { button, state: btn_state, .. } => {
                if let Some(tx) = &state.pointer_sender {
                    let evt = match btn_state {
                        wayland_client::WEnum::Value(wl_pointer::ButtonState::Pressed) => PointerEvent::Press { button },
                        wayland_client::WEnum::Value(wl_pointer::ButtonState::Released) => PointerEvent::Release { button },
                        _ => return,
                    };
                    let _ = tx.send(evt);
                }
            }
            wl_pointer::Event::Axis { axis: wayland_client::WEnum::Value(wl_pointer::Axis::VerticalScroll), value, .. } => {
                if let Some(tx) = &state.pointer_sender {
                    let _ = tx.send(PointerEvent::Axis { amount: value });
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
        _event: <wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1,
        _event: <wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}
