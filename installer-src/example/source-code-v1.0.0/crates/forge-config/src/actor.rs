use std::thread;
use std::path::PathBuf;
use mlua::{Lua, Table};
use crossbeam_channel::{Sender, Receiver, bounded};
use forge_core::config_registry::ForgeConfig;
use crate::types::ConfigUpdate;

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

    // Initialize Lua VM
    let lua = Lua::new();

    // Load initial config
    let initial_update = load_and_eval(&lua, &config_path).unwrap_or_else(|| ConfigUpdate {
        config: forge_core::config_registry::ForgeConfig::default(),
    });
    let _ = tx.send(initial_update);

    // Event loop
    while let Ok(msg) = rx.recv() {
        match msg {
            ActorMessage::Shutdown => break,
            ActorMessage::Reload => {
                if let Some(update) = load_and_eval(&lua, &config_path) {
                    let _ = tx.send(update);
                }
            }
        }
    }

    tracing::debug!("Config Actor thread shutting down.");
}

fn load_and_eval(lua: &Lua, config_path: &PathBuf) -> Option<ConfigUpdate> {
    // 1. Read file contents. If missing, use a default string:
    //    `return { font = { size = 14.0 }, window = { opacity = 0.9 } }`
    let source = std::fs::read_to_string(config_path).unwrap_or_else(|_| {
        tracing::info!("No config found at {:?}, using default.", config_path);
        "return {}".to_string()
    });

    // 2. Evaluate Lua code.
    let result = lua.load(&source).eval::<Table>();
    let table = match result {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("Lua config error: {}", e);
            return None; // Keep previous state on error
        }
    };

    // 3. Extract values from the table to build ForgeConfig.
    let mut config = ForgeConfig::default();
    crate::extractor::extract_config(table, &mut config);

    // 4. Validate limits.
    config.validate();

    Some(ConfigUpdate { config })
}
