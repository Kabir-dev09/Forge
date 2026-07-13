use crate::types::{ConfigChangeSet, ConfigUpdate};
use crossbeam_channel::{bounded, Receiver, Sender};
use forge_core::config_registry::ForgeConfig;
use std::path::PathBuf;
use std::thread;

/// Messages sent from the Main Thread to the Config Actor.
pub enum ActorMessage {
    Reload, // Force a config reload
    Shutdown,
}

pub struct ConfigActorHandle {
    pub tx: Sender<ActorMessage>,
    pub rx: Receiver<ConfigUpdate>,
    pub thread_handle: Option<thread::JoinHandle<()>>,
}

/// Spawns the configuration actor on a dedicated background thread.
/// Returns a handle for bidirectional communication.
pub fn spawn_config_actor(config_path: PathBuf) -> ConfigActorHandle {
    let (main_tx, actor_rx) = bounded(16);
    let (actor_tx, main_rx) = bounded(16);

    let thread_handle = thread::Builder::new()
        .name("forge-config".to_string())
        .spawn(move || {
            actor_loop(config_path, actor_rx, actor_tx);
        })
        .expect("Failed to spawn config actor thread");

    ConfigActorHandle {
        tx: main_tx,
        rx: main_rx,
        thread_handle: Some(thread_handle),
    }
}

fn actor_loop(config_path: PathBuf, rx: Receiver<ActorMessage>, tx: Sender<ConfigUpdate>) {
    tracing::debug!("Config Actor thread started.");

    // Auto-create default config if missing (off the main thread to prevent blocking)
    if !config_path.exists() {
        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let default_config = include_str!("../../../forge_config_example.toml");
        if let Err(e) = std::fs::write(&config_path, default_config) {
            tracing::warn!("Failed to write default config to {:?}: {}", config_path, e);
        } else {
            tracing::info!("Created default config file at {:?}", config_path);
        }
    }

    // Load initial config
    let initial_config = load_config(&config_path).unwrap_or_else(|| {
        tracing::warn!("Initial config load failed. Falling back to defaults.");
        forge_core::config_registry::ForgeConfig::default()
    });
    let _ = tx.send(ConfigUpdate {
        config: initial_config.clone(),
        changes: ConfigChangeSet::all(),
    });
    let mut current_config = initial_config;

    // Event loop
    while let Ok(msg) = rx.recv() {
        match msg {
            ActorMessage::Shutdown => break,
            ActorMessage::Reload => {
                if let Some(config) = load_config(&config_path) {
                    let changes = ConfigChangeSet::between(&current_config, &config);
                    if changes.any() {
                        current_config = config.clone();
                        let _ = tx.send(ConfigUpdate { config, changes });
                    } else {
                        tracing::debug!("Config reload produced no changes.");
                    }
                }
            }
        }
    }

    tracing::debug!("Config Actor thread shutting down.");
}

const DEFAULT_CONFIG: &str = include_str!("default_config.toml");

fn load_config(config_path: &PathBuf) -> Option<ForgeConfig> {
    if !config_path.exists() {
        tracing::info!(
            "No config found at {:?}, generating default config.",
            config_path
        );
        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(config_path, DEFAULT_CONFIG);
    }

    match crate::imports::parse_config_file(config_path) {
        Ok(config) => Some(config),
        Err(e) => {
            tracing::warn!("TOML config load error in {:?}: {}", config_path, e);
            None
        }
    }
}
