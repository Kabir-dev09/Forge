use crate::types::{ConfigChangeSet, ConfigUpdate};
use crossbeam_channel::{bounded, Receiver, Sender};
use forge_core::config_registry::ForgeConfig;
use mlua::{Lua, Table};
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

/// Spawns the Lua configuration actor on a dedicated background thread.
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
        let default_config = include_str!("../../../forge_config_example.lua");
        if let Err(e) = std::fs::write(&config_path, default_config) {
            tracing::warn!("Failed to write default config to {:?}: {}", config_path, e);
        } else {
            tracing::info!("Created default config file at {:?}", config_path);
        }
    }

    // Initialize Lua VM
    let lua = Lua::new();

    // Load initial config
    let initial_config = load_and_eval(&lua, &config_path).unwrap_or_else(|| {
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
                if let Some(config) = load_and_eval(&lua, &config_path) {
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

const DEFAULT_CONFIG: &str = include_str!("default_config.lua");

fn load_and_eval(lua: &Lua, config_path: &PathBuf) -> Option<ForgeConfig> {
    // 1. Read file contents. If missing, write the default config and use it.
    let source = std::fs::read_to_string(config_path).unwrap_or_else(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            tracing::info!(
                "No config found at {:?}, generating default config.",
                config_path
            );
            if let Some(parent) = config_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(config_path, DEFAULT_CONFIG);
        } else {
            tracing::info!(
                "Failed to read config at {:?}: {}, using default.",
                config_path,
                e
            );
        }
        DEFAULT_CONFIG.to_string()
    });

    // 2. Evaluate Lua code.
    let result = lua.load(&source).eval::<Table>();
    let table = match result {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("Lua config parse/eval error in {:?}: {}", config_path, e);
            return None; // Keep previous state on error
        }
    };

    // 3. Extract values from the table to build ForgeConfig.
    let mut config = ForgeConfig::default();
    crate::extractor::extract_config(table, &mut config);

    // 4. Validate limits.
    config.validate();

    Some(config)
}
