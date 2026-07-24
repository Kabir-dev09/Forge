use crate::types::{ConfigChangeSet, ConfigUpdate};
use crossbeam_channel::{bounded, Receiver, Sender};
use forge_core::config_registry::ForgeConfig;
use std::path::PathBuf;
use std::thread;

#[derive(Debug, Clone)]
pub struct ConfigSource {
    pub path: PathBuf,
    pub overrides: Vec<crate::imports::ConfigOverride>,
    pub strict: bool,
    pub create_if_missing: bool,
}

impl ConfigSource {
    pub fn standard(path: PathBuf) -> Self {
        Self {
            path,
            overrides: Vec::new(),
            strict: false,
            create_if_missing: true,
        }
    }
}

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
    spawn_actor(ConfigSource::standard(config_path), None)
}

pub fn spawn_config_actor_with_initial(
    source: ConfigSource,
    initial_config: ForgeConfig,
) -> ConfigActorHandle {
    spawn_actor(source, Some(initial_config))
}

fn spawn_actor(source: ConfigSource, initial_config: Option<ForgeConfig>) -> ConfigActorHandle {
    let (main_tx, actor_rx) = bounded(16);
    let (actor_tx, main_rx) = bounded(16);

    let thread_handle = thread::Builder::new()
        .name("forge-config".to_string())
        .spawn(move || {
            actor_loop(source, initial_config, actor_rx, actor_tx);
        })
        .expect("Failed to spawn config actor thread");

    ConfigActorHandle {
        tx: main_tx,
        rx: main_rx,
        thread_handle: Some(thread_handle),
    }
}

fn actor_loop(
    source: ConfigSource,
    initial_config: Option<ForgeConfig>,
    rx: Receiver<ActorMessage>,
    tx: Sender<ConfigUpdate>,
) {
    tracing::debug!("Config Actor thread started.");

    // Auto-create default config if missing (off the main thread to prevent blocking)
    if initial_config.is_none() && source.create_if_missing && !source.path.exists() {
        if let Some(parent) = source.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let default_config = include_str!("../../../forge_config_example.toml");
        if let Err(e) = std::fs::write(&source.path, default_config) {
            tracing::warn!("Failed to write default config to {:?}: {}", source.path, e);
        } else {
            tracing::info!("Created default config file at {:?}", source.path);
        }
    }

    // Load initial config
    let initial_config = initial_config
        .or_else(|| load_config(&source))
        .unwrap_or_else(|| {
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
                if let Some(config) = load_config(&source) {
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

pub fn load_config_source(
    source: &ConfigSource,
) -> Result<ForgeConfig, crate::imports::ConfigLoadError> {
    if source.create_if_missing && !source.path.exists() {
        tracing::info!(
            "No config found at {:?}, generating default config.",
            source.path
        );
        if let Some(parent) = source.path.parent() {
            std::fs::create_dir_all(parent).map_err(|source_error| {
                crate::imports::ConfigLoadError::Io {
                    path: parent.to_path_buf(),
                    source: source_error,
                }
            })?;
        }
        std::fs::write(&source.path, DEFAULT_CONFIG).map_err(|source_error| {
            crate::imports::ConfigLoadError::Io {
                path: source.path.clone(),
                source: source_error,
            }
        })?;
    }

    if source.strict || !source.overrides.is_empty() {
        crate::imports::parse_config_file_strict(&source.path, &source.overrides, source.strict)
    } else {
        crate::imports::parse_config_file(&source.path)
    }
}

fn load_config(source: &ConfigSource) -> Option<ForgeConfig> {
    match load_config_source(source) {
        Ok(config) => Some(config),
        Err(e) => {
            tracing::warn!("TOML config load error in {:?}: {}", source.path, e);
            None
        }
    }
}
