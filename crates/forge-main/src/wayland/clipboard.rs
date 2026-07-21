use crate::wayland::connection::WaylandState;
use std::io::Read;
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::sync::Arc;
use wayland_client::{
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::WlDataDeviceManager,
        wl_data_offer::WlDataOffer, wl_data_source::WlDataSource, wl_seat::WlSeat,
    },
    Connection, Dispatch, Proxy, QueueHandle,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipboardMime {
    Utf8Text,
    PlainText,
    Utf8String,
    Text,
    String,
}

impl ClipboardMime {
    fn from_offered(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("text/plain;charset=utf-8") {
            Some(Self::Utf8Text)
        } else if value.eq_ignore_ascii_case("text/plain") {
            Some(Self::PlainText)
        } else if value.eq_ignore_ascii_case("UTF8_STRING") {
            Some(Self::Utf8String)
        } else if value.eq_ignore_ascii_case("TEXT") {
            Some(Self::Text)
        } else if value.eq_ignore_ascii_case("STRING") {
            Some(Self::String)
        } else {
            None
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Utf8Text => "text/plain;charset=utf-8",
            Self::PlainText => "text/plain",
            Self::Utf8String => "UTF8_STRING",
            Self::Text => "TEXT",
            Self::String => "STRING",
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::Utf8Text => 5,
            Self::PlainText => 4,
            Self::Utf8String => 3,
            Self::Text => 2,
            Self::String => 1,
        }
    }
}

struct PendingOffer {
    offer: WlDataOffer,
    mime: Option<ClipboardMime>,
}

fn preferred_mime(current: Option<ClipboardMime>, offered: &str) -> Option<ClipboardMime> {
    let offered = ClipboardMime::from_offered(offered)?;
    match current {
        Some(current) if current.priority() >= offered.priority() => Some(current),
        _ => Some(offered),
    }
}

pub struct ClipboardManager {
    pub manager: WlDataDeviceManager,
    pub device: Option<WlDataDevice>,
    pub current_offer: Option<WlDataOffer>,
    current_mime: Option<ClipboardMime>,
    pending_offers: Vec<PendingOffer>,
    pending_paste: bool,
    pub current_source: Option<WlDataSource>,
    owned_text: Option<Arc<[u8]>>,
    pub paste_sender: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    pub loop_signal: Option<calloop::LoopSignal>,
}

impl ClipboardManager {
    pub fn new(manager: WlDataDeviceManager) -> Self {
        Self {
            manager,
            device: None,
            current_offer: None,
            current_mime: None,
            pending_offers: Vec::new(),
            pending_paste: false,
            current_source: None,
            owned_text: None,
            paste_sender: None,
            loop_signal: None,
        }
    }

    pub fn init_device(&mut self, seat: &WlSeat, qh: &QueueHandle<WaylandState>) {
        self.device = Some(self.manager.get_data_device(seat, qh, ()));
    }

    /// Installs a new Wayland selection source. The return value tells the caller
    /// whether the connection has protocol requests waiting to be flushed.
    pub fn set_clipboard(
        &mut self,
        text: String,
        serial: u32,
        qh: &QueueHandle<WaylandState>,
    ) -> bool {
        let Some(device) = &self.device else {
            tracing::warn!("Cannot copy because the Wayland data device is unavailable");
            return false;
        };

        let text: Arc<[u8]> = Arc::from(text.into_bytes());
        let source = self.manager.create_data_source(qh, text.clone());
        source.offer("text/plain;charset=utf-8".to_string());
        source.offer("text/plain".to_string());
        device.set_selection(Some(&source), serial);

        self.owned_text = Some(text);
        self.current_source = Some(source);
        true
    }

    pub fn request_paste(&mut self) -> bool {
        // A compositor round trip is neither necessary nor reliable immediately
        // after this client takes ownership. Serve our own selection directly.
        if let Some(text) = self.owned_text.as_deref() {
            self.pending_paste = false;
            deliver_paste(
                self.paste_sender.as_ref(),
                self.loop_signal.as_ref(),
                normalize_paste_bytes(text),
            );
            return false;
        }

        if let (Some(offer), Some(mime)) = (&self.current_offer, self.current_mime) {
            self.pending_paste = false;
            let mut fds = [0_i32; 2];
            if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
                tracing::warn!("Failed to create clipboard transfer pipe");
                return false;
            }

            offer.receive(mime.as_str().to_string(), unsafe {
                std::os::fd::BorrowedFd::borrow_raw(fds[1])
            });
            unsafe { libc::close(fds[1]) };

            let read_fd = fds[0];
            let paste_sender = self.paste_sender.clone();
            let loop_signal = self.loop_signal.clone();
            std::thread::spawn(move || {
                let mut file = unsafe { std::fs::File::from_raw_fd(read_fd) };
                let mut content = Vec::new();
                if file.read_to_end(&mut content).is_ok() {
                    let content = normalize_paste_bytes(&content);
                    deliver_paste(paste_sender.as_ref(), loop_signal.as_ref(), content);
                }
            });
            true
        } else if self.current_offer.is_none() {
            // The initial selection arrives asynchronously after get_data_device.
            self.pending_paste = true;
            false
        } else {
            self.pending_paste = false;
            tracing::warn!("Clipboard selection does not advertise a supported text format");
            false
        }
    }
}

fn normalize_paste_bytes(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        match input[index] {
            b'\r' if input.get(index + 1) == Some(&b'\n') => {
                output.push(b'\r');
                index += 2;
            }
            b'\n' => {
                output.push(b'\r');
                index += 1;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    output
}

fn deliver_paste(
    sender: Option<&std::sync::mpsc::SyncSender<Vec<u8>>>,
    loop_signal: Option<&calloop::LoopSignal>,
    content: Vec<u8>,
) {
    if let Some(sender) = sender {
        if sender.send(content).is_ok() {
            if let Some(loop_signal) = loop_signal {
                loop_signal.wakeup();
            }
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
                wayland_client::protocol::wl_data_device::Event::DataOffer { id } => {
                    clip.pending_offers.push(PendingOffer {
                        offer: id,
                        mime: None,
                    });
                }
                wayland_client::protocol::wl_data_device::Event::Selection { id } => {
                    if let Some(id) = id {
                        let pending_index = clip
                            .pending_offers
                            .iter()
                            .position(|pending| pending.offer.id() == id.id());
                        clip.current_mime = pending_index
                            .map(|index| clip.pending_offers.swap_remove(index).mime)
                            .flatten();
                        clip.current_offer = Some(id);
                    } else {
                        clip.current_offer = None;
                        clip.current_mime = None;
                        clip.pending_paste = false;
                        clip.owned_text = None;
                    }
                    clip.pending_offers.clear();
                    if clip.pending_paste {
                        state.needs_flush |= clip.request_paste();
                    }
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
        state: &mut Self,
        offer: &WlDataOffer,
        event: wayland_client::protocol::wl_data_offer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_data_offer::Event::Offer { mime_type } = event {
            if let Some(clip) = &mut state.clipboard {
                if let Some(pending) = clip
                    .pending_offers
                    .iter_mut()
                    .find(|pending| pending.offer.id() == offer.id())
                {
                    pending.mime = preferred_mime(pending.mime, &mime_type);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_mime_selection_accepts_common_initial_offer_formats() {
        assert_eq!(
            preferred_mime(None, "text/plain"),
            Some(ClipboardMime::PlainText)
        );
        assert_eq!(
            preferred_mime(None, "UTF8_STRING"),
            Some(ClipboardMime::Utf8String)
        );
        assert_eq!(
            preferred_mime(None, "text/plain;charset=UTF-8"),
            Some(ClipboardMime::Utf8Text)
        );
    }

    #[test]
    fn clipboard_mime_selection_keeps_highest_priority_text_format() {
        let mime = preferred_mime(None, "STRING");
        let mime = preferred_mime(mime, "text/plain");
        let mime = preferred_mime(mime, "UTF8_STRING");
        let mime = preferred_mime(mime, "text/plain;charset=utf-8");

        assert_eq!(mime, Some(ClipboardMime::Utf8Text));
    }

    #[test]
    fn clipboard_mime_selection_ignores_non_text_formats() {
        assert_eq!(preferred_mime(None, "image/png"), None);
    }

    #[test]
    fn paste_newlines_are_normalized_in_one_pass() {
        assert_eq!(
            normalize_paste_bytes(b"one\r\ntwo\nthree\r"),
            b"one\rtwo\rthree\r"
        );
    }
}

impl Dispatch<WlDataSource, Arc<[u8]>> for WaylandState {
    fn event(
        state: &mut Self,
        source: &WlDataSource,
        event: wayland_client::protocol::wl_data_source::Event,
        data: &Arc<[u8]>,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wayland_client::protocol::wl_data_source::Event::Send { mime_type: _, fd } => {
                let text = data.clone();
                std::thread::spawn(move || {
                    let mut file = unsafe { std::fs::File::from_raw_fd(fd.into_raw_fd()) };
                    let _ = std::io::Write::write_all(&mut file, &text);
                });
            }
            wayland_client::protocol::wl_data_source::Event::Cancelled => {
                if let Some(clip) = &mut state.clipboard {
                    if clip
                        .current_source
                        .as_ref()
                        .is_some_and(|current| current.id() == source.id())
                    {
                        clip.current_source = None;
                        clip.owned_text = None;
                    }
                }
            }
            _ => {}
        }
    }
}
