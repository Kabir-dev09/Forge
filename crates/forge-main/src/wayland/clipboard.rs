use crate::wayland::connection::WaylandState;
use std::io::Read;
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::sync::{Arc, Mutex};
use wayland_client::{
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::WlDataDeviceManager,
        wl_data_offer::WlDataOffer, wl_data_source::WlDataSource, wl_seat::WlSeat,
    },
    Connection, Dispatch, QueueHandle,
};

pub struct ClipboardManager {
    pub manager: WlDataDeviceManager,
    pub device: Option<WlDataDevice>,
    pub current_offer: Option<WlDataOffer>,
    pub stored_text: Arc<Mutex<String>>,
    pub paste_sender: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    pub loop_signal: Option<calloop::LoopSignal>,
}

impl ClipboardManager {
    pub fn new(manager: WlDataDeviceManager) -> Self {
        Self {
            manager,
            device: None,
            current_offer: None,
            stored_text: Arc::new(Mutex::new(String::new())),
            paste_sender: None,
            loop_signal: None,
        }
    }

    pub fn init_device(&mut self, seat: &WlSeat, qh: &QueueHandle<WaylandState>) {
        self.device = Some(self.manager.get_data_device(seat, qh, ()));
    }

    pub fn set_clipboard(&self, text: String, serial: u32, qh: &QueueHandle<WaylandState>) {
        if let Ok(mut lock) = self.stored_text.lock() {
            *lock = text;
        }

        let source = self.manager.create_data_source(qh, ());
        source.offer("text/plain;charset=utf-8".to_string());
        source.offer("text/plain".to_string());

        if let Some(device) = &self.device {
            device.set_selection(Some(&source), serial);
        }
    }

    pub fn request_paste(&self) {
        if let Some(offer) = &self.current_offer {
            let mut fds = [0_i32; 2];
            unsafe { libc::pipe(fds.as_mut_ptr()) };

            tracing::info!(
                "[PASTE TIMING] Sending offer.receive at {:?}",
                std::time::Instant::now()
            );
            offer.receive("text/plain;charset=utf-8".to_string(), unsafe {
                std::os::fd::BorrowedFd::borrow_raw(fds[1])
            });
            unsafe { libc::close(fds[1]) };

            let read_fd = fds[0];
            let paste_sender = self.paste_sender.clone();
            let loop_signal = self.loop_signal.clone();
            std::thread::spawn(move || {
                let mut file = unsafe { std::fs::File::from_raw_fd(read_fd) };
                let mut content = String::new();
                tracing::info!(
                    "[PASTE TIMING] Background thread starting read_to_string at {:?}",
                    std::time::Instant::now()
                );
                if file.read_to_string(&mut content).is_ok() {
                    tracing::info!(
                        "[PASTE TIMING] Background thread finished read_to_string at {:?}",
                        std::time::Instant::now()
                    );
                    let processed_content = content.replace("\r\n", "\r").replace('\n', "\r");
                    if let Some(tx) = paste_sender {
                        let _ = tx.send(processed_content.into_bytes());
                        tracing::info!(
                            "[PASTE TIMING] Background thread sent to paste_sender at {:?}",
                            std::time::Instant::now()
                        );
                        if let Some(sig) = loop_signal {
                            sig.wakeup();
                        }
                    }
                }
            });
        }
    }
}

impl Dispatch<WlDataDeviceManager, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _manager: &WlDataDeviceManager,
        _event: wayland_client::protocol::wl_data_device_manager::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataDevice, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _device: &WlDataDevice,
        event: wayland_client::protocol::wl_data_device::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let Some(clip) = &mut state.clipboard {
            match event {
                wayland_client::protocol::wl_data_device::Event::DataOffer { id: _ } => {
                    // We just received a data offer. It will be passed to Selection event later.
                }
                wayland_client::protocol::wl_data_device::Event::Selection { id } => {
                    clip.current_offer = id;
                }
                _ => {}
            }
        }
    }

    wayland_client::event_created_child!(WaylandState, WlDataDevice, [
        0 => (WlDataOffer, ())
    ]);
}

impl Dispatch<WlDataOffer, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _offer: &WlDataOffer,
        event: wayland_client::protocol::wl_data_offer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_data_offer::Event::Offer { mime_type } = event {
            if mime_type == "text/plain;charset=utf-8" || mime_type == "text/plain" {
                // accept it? It's not strictly necessary for simple clipboard,
                // but good practice. `offer.accept(...)` can be called here.
            }
        }
    }
}

impl Dispatch<WlDataSource, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _source: &WlDataSource,
        event: wayland_client::protocol::wl_data_source::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_data_source::Event::Send { mime_type: _, fd } = event {
            if let Some(clip) = &state.clipboard {
                if let Ok(lock) = clip.stored_text.lock() {
                    let text = lock.clone();
                    std::thread::spawn(move || {
                        let mut file = unsafe { std::fs::File::from_raw_fd(fd.into_raw_fd()) };
                        let _ = std::io::Write::write_all(&mut file, text.as_bytes());
                    });
                }
            }
        }
    }
}
