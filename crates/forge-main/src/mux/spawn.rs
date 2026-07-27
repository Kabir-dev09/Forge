use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};

use calloop::LoopSignal;
use forge_core::config_registry::ShellConfig;
use forge_pty::Winsize;

use super::PaneId;

pub struct PtySpawnRequest {
    pub pane_id: PaneId,
    pub winsize: Winsize,
    pub working_directory: Option<PathBuf>,
}

pub struct PtySpawnCompletion {
    pub pane_id: PaneId,
    pub result: Result<forge_pty::Pty, String>,
}

pub struct PtySpawnService {
    request_sender: Sender<SpawnCommand>,
    completion_receiver: Receiver<PtySpawnCompletion>,
    cancelled: Arc<Mutex<HashSet<PaneId>>>,
}

enum SpawnCommand {
    Spawn(PtySpawnRequest),
    Dispose(forge_pty::Pty),
}

impl PtySpawnService {
    pub fn new(shell: ShellConfig, loop_signal: LoopSignal) -> std::io::Result<Self> {
        let (request_sender, request_receiver) = mpsc::channel::<SpawnCommand>();
        let (completion_sender, completion_receiver) = mpsc::channel();
        let cancelled = Arc::new(Mutex::new(HashSet::new()));
        let worker_cancelled = cancelled.clone();

        std::thread::Builder::new()
            .name("forge-pty-spawn".to_string())
            .spawn(move || {
                let prepared =
                    forge_pty::PreparedPtyCommand::new(&shell).map_err(|error| error.to_string());
                while let Ok(request) = request_receiver.recv() {
                    match request {
                        SpawnCommand::Spawn(request) => {
                            let result = match &prepared {
                                Ok(command) => forge_pty::Pty::spawn_prepared_in_dir(
                                    command,
                                    request.winsize,
                                    request.working_directory.as_deref(),
                                )
                                .map_err(|error| error.to_string()),
                                Err(error) => Err(error.clone()),
                            };
                            let was_cancelled = worker_cancelled
                                .lock()
                                .is_ok_and(|mut panes| panes.remove(&request.pane_id));
                            if was_cancelled {
                                if let Ok(pty) = result {
                                    pty.terminate_and_reap();
                                }
                                continue;
                            }
                            if let Err(completion) = completion_sender.send(PtySpawnCompletion {
                                pane_id: request.pane_id,
                                result,
                            }) {
                                if let Ok(pty) = completion.0.result {
                                    pty.terminate_and_reap();
                                }
                                break;
                            }
                            loop_signal.wakeup();
                        }
                        SpawnCommand::Dispose(pty) => pty.terminate_and_reap(),
                    }
                }
            })?;

        Ok(Self {
            request_sender,
            completion_receiver,
            cancelled,
        })
    }

    pub fn spawn(&self, request: PtySpawnRequest) -> Result<(), String> {
        self.request_sender
            .send(SpawnCommand::Spawn(request))
            .map_err(|error| format!("PTY spawn service stopped: {}", error))
    }

    pub fn cancel(&self, pane_id: PaneId) {
        if let Ok(mut panes) = self.cancelled.lock() {
            panes.insert(pane_id);
        }
    }

    pub fn acknowledge_completion(&self, pane_id: PaneId) {
        if let Ok(mut panes) = self.cancelled.lock() {
            panes.remove(&pane_id);
        }
    }

    pub fn dispose(&self, pty: forge_pty::Pty) {
        if let Err(error) = self.request_sender.send(SpawnCommand::Dispose(pty)) {
            if let SpawnCommand::Dispose(pty) = error.0 {
                pty.terminate_and_reap();
            }
        }
    }

    pub fn try_recv(&self) -> Result<PtySpawnCompletion, TryRecvError> {
        self.completion_receiver.try_recv()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_service_returns_a_pty_without_blocking_the_requester() {
        let event_loop = calloop::EventLoop::<()>::try_new().unwrap();
        let service = PtySpawnService::new(
            ShellConfig {
                program: "/bin/sh".to_string(),
                args: vec!["-c".to_string(), "exit 0".to_string()],
                integration_enabled: false,
                ..ShellConfig::default()
            },
            event_loop.get_signal(),
        )
        .unwrap();
        service
            .spawn(PtySpawnRequest {
                pane_id: PaneId::new(7),
                winsize: Winsize {
                    ws_row: 24,
                    ws_col: 80,
                    ws_xpixel: 800,
                    ws_ypixel: 480,
                },
                working_directory: None,
            })
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match service.try_recv() {
                Ok(completion) => {
                    assert_eq!(completion.pane_id, PaneId::new(7));
                    assert!(completion.result.is_ok());
                    break;
                }
                Err(TryRecvError::Empty) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                result => panic!("spawn service did not complete: {:?}", result.err()),
            }
        }
    }
}
