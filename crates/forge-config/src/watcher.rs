use crate::actor::ActorMessage;
use crossbeam_channel::Sender;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

const RELOAD_DEBOUNCE: Duration = Duration::from_millis(150);

/// Spawns a background thread to watch the config file for changes.
pub fn spawn_config_watcher(
    config_path: PathBuf,
    actor_tx: Sender<ActorMessage>,
) -> Option<notify::RecommendedWatcher> {
    // 1. Get the parent directory of the config file.
    let parent_dir = config_path.parent()?;

    // notify uses std::sync::mpsc for its event channel.
    let (tx, rx) = channel();

    // 2. Create the watcher.
    let mut watcher = notify::RecommendedWatcher::new(tx, notify::Config::default())
        .map_err(|e| tracing::warn!("Failed to create watcher: {}", e))
        .ok()?;

    // 3. Watch the parent directory, not the file itself.
    // Why? Text editors often do atomic saves (write to temp file, then rename/move over the original).
    // Watching the file directly breaks when it gets replaced.
    watcher
        .watch(parent_dir, RecursiveMode::NonRecursive)
        .map_err(|e| tracing::warn!("Failed to watch config dir: {}", e))
        .ok()?;

    // 4. Spawn a thread to process watcher events.
    std::thread::Builder::new()
        .name("forge-watcher".to_string())
        .spawn(move || {
            tracing::debug!("Config watcher thread started.");
            let mut last_reload = Instant::now() - RELOAD_DEBOUNCE;
            while let Ok(res) = rx.recv() {
                match res {
                    Ok(Event { kind, paths, .. }) => {
                        // explicitly handle modify, create, and name change (rename/move) events
                        if matches!(
                            kind,
                            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Any
                        ) && paths.contains(&config_path)
                        {
                            let now = Instant::now();
                            if now.duration_since(last_reload) < RELOAD_DEBOUNCE {
                                tracing::trace!("Debounced config reload event.");
                                continue;
                            }
                            last_reload = now;
                            tracing::info!("Config file changed, triggering reload.");
                            // Use try_send to avoid deadlocks if the channel is full
                            let _ = actor_tx.try_send(ActorMessage::Reload);
                        }
                    }
                    Err(e) => tracing::warn!("Watcher error: {:?}", e),
                }
            }
            tracing::debug!("Config watcher thread exiting.");
        })
        .expect("Failed to spawn watcher thread");

    Some(watcher)
}
