use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Deserialize, Clone)]
pub struct NvimBufferInfo {
    pub id: u32,
    pub path: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum PluginMessage {
    #[serde(rename = "handshake")]
    Handshake { pane_id: u64 },
    #[serde(rename = "sync_state")]
    SyncState { current_buffer: u32, buffers: Vec<NvimBufferInfo> },
    #[serde(rename = "buffer_added")]
    BufferAdded { buffer_id: u32, path: String, name: String },
    #[serde(rename = "buffer_removed")]
    BufferRemoved { buffer_id: u32 },
    #[serde(rename = "buffer_entered")]
    BufferEntered { buffer_id: u32 },
    #[serde(rename = "buffer_renamed")]
    BufferRenamed { buffer_id: u32, name: String },
    #[serde(skip)]
    Disconnected,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum CommandMessage {
    #[serde(rename = "focus_buffer")]
    FocusBuffer { buffer_id: u32 },
    #[serde(rename = "focus_dashboard")]
    FocusDashboard,
}

pub struct NvimIpcServer {
    pub socket_path: String,
    // (pane_id, message)
    pub receiver: std::sync::mpsc::Receiver<(u64, PluginMessage)>,
    // pane_id -> UnixStream for sending commands back
    pub clients: Arc<Mutex<std::collections::HashMap<u64, UnixStream>>>,
}

impl NvimIpcServer {
    pub fn new(loop_signal: calloop::LoopSignal) -> anyhow::Result<Self> {
        let pid = std::process::id();
        let socket_path = format!("/tmp/forge-ipc-{}.sock", pid);

        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path)?;

        let (tx, rx) = std::sync::mpsc::channel();
        let clients = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let clients_clone = clients.clone();

        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let tx = tx.clone();
                        let loop_signal = loop_signal.clone();
                        let clients = clients_clone.clone();
                        thread::spawn(move || {
                            Self::handle_client(stream, tx, loop_signal, clients);
                        });
                    }
                    Err(err) => {
                        tracing::error!("Nvim IPC accept error: {}", err);
                        break;
                    }
                }
            }
        });

        Ok(Self {
            socket_path,
            receiver: rx,
            clients,
        })
    }

    fn handle_client(
        stream: UnixStream,
        tx: std::sync::mpsc::Sender<(u64, PluginMessage)>,
        loop_signal: calloop::LoopSignal,
        clients: Arc<Mutex<std::collections::HashMap<u64, UnixStream>>>,
    ) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        let mut identified_pane_id: Option<u64> = None;

        while let Ok(bytes) = reader.read_line(&mut line) {
            if bytes == 0 {
                break; // EOF
            }

            if let Ok(msg) = serde_json::from_str::<PluginMessage>(&line) {
                if let PluginMessage::Handshake { pane_id } = msg {
                    identified_pane_id = Some(pane_id);
                    let mut lock = clients.lock().unwrap();
                    lock.insert(pane_id, stream.try_clone().unwrap());
                }

                if let Some(pane_id) = identified_pane_id {
                    let _ = tx.send((pane_id, msg));
                    loop_signal.wakeup();
                }
            } else {
                tracing::warn!("Failed to parse nvim msg: {}", line);
            }
            line.clear();
        }

        // Cleanup on disconnect
        if let Some(pane_id) = identified_pane_id {
            let mut lock = clients.lock().unwrap();
            lock.remove(&pane_id);
            let _ = tx.send((pane_id, PluginMessage::Disconnected));
            loop_signal.wakeup();
        }
    }

    pub fn send_command(&self, pane_id: u64, cmd: CommandMessage) {
        if let Ok(mut lock) = self.clients.lock() {
            if let Some(stream) = lock.get_mut(&pane_id) {
                if let Ok(mut json) = serde_json::to_string(&cmd) {
                    json.push('\n');
                    let _ = stream.write_all(json.as_bytes());
                }
            }
        }
    }
}
