use serde::Deserialize;
use std::process::{Command, Stdio};
use std::time::Duration;

pub const FORGE_APP_ID: &str = "dev.forge.terminal";
pub const CENTERED_FORGE_APP_ID: &str = "dev.forge.terminal.centered";
const WINDOW_REGISTRATION_RETRY: Duration = Duration::from_millis(16);

#[derive(Deserialize)]
struct NiriWindow {
    id: u64,
    app_id: Option<String>,
    pid: Option<i32>,
    #[serde(default)]
    is_floating: bool,
}

pub fn window_app_id(enabled: bool, fullscreen: bool) -> &'static str {
    if !should_attempt_centering(enabled, fullscreen) {
        return FORGE_APP_ID;
    }
    app_id_for_supported_compositor(std::env::var_os("NIRI_SOCKET").is_some())
}

fn app_id_for_supported_compositor(niri_available: bool) -> &'static str {
    if niri_available {
        CENTERED_FORGE_APP_ID
    } else {
        FORGE_APP_ID
    }
}

/// Requests one compositor-managed centering operation after the initial frame is mapped.
///
/// Standard xdg-shell deliberately does not expose absolute positioning for top-level windows.
/// Niri provides targeted IPC actions; unsupported compositors keep their normal placement.
pub fn center_window_once(enabled: bool, fullscreen: bool, app_id: &'static str) {
    // Keep the default path to one boolean check; even environment lookup is skipped.
    if !should_attempt_centering(enabled, fullscreen) {
        return;
    }
    if std::env::var_os("NIRI_SOCKET").is_none() {
        return;
    }

    let pid = std::process::id() as i32;
    let _ = std::thread::Builder::new()
        .name("forge-center-window".to_string())
        .spawn(move || {
            if center_niri_window(pid, app_id) {
                return;
            }

            // The Wayland commit and Niri IPC use separate sockets. Give Niri one frame to
            // publish a just-mapped window, then make one final bounded attempt.
            std::thread::sleep(WINDOW_REGISTRATION_RETRY);
            if !center_niri_window(pid, app_id) {
                tracing::debug!(
                    "Window centering was requested, but Niri did not expose this Forge window"
                );
            }
        });
}

fn should_attempt_centering(enabled: bool, fullscreen: bool) -> bool {
    enabled && !fullscreen
}

fn center_niri_window(pid: i32, app_id: &str) -> bool {
    let output = match Command::new("niri")
        .args(["msg", "--json", "windows"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => output.stdout,
        _ => return false,
    };

    let Some(window) = niri_window(&output, pid, app_id) else {
        return false;
    };
    if !window.is_floating {
        tracing::debug!(
            "Niri centered-launch rule is not active; preserving the compositor's tiled placement"
        );
        return true;
    }

    run_niri_window_action("center-window", window.id)
}

fn run_niri_window_action(action: &str, window_id: u64) -> bool {
    Command::new("niri")
        .args(["msg", "action", action, "--id", &window_id.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn niri_window(output: &[u8], pid: i32, app_id: &str) -> Option<NiriWindow> {
    serde_json::from_slice::<Vec<NiriWindow>>(output)
        .ok()?
        .into_iter()
        .find(|window| window.pid == Some(pid) && window.app_id.as_deref() == Some(app_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_and_fullscreen_paths_do_no_work() {
        assert!(!should_attempt_centering(false, false));
        assert!(!should_attempt_centering(true, true));
        assert!(should_attempt_centering(true, false));
    }

    #[test]
    fn dedicated_app_id_is_used_only_for_centered_niri_launches() {
        assert_eq!(app_id_for_supported_compositor(false), FORGE_APP_ID);
        assert_eq!(app_id_for_supported_compositor(true), CENTERED_FORGE_APP_ID);
    }

    #[test]
    fn selects_window_by_pid_and_forge_app_id() {
        let windows = br#"[
            {"id": 3, "app_id": "dev.forge.terminal", "pid": 42, "is_floating": false},
            {"id": 7, "app_id": "other.app", "pid": 91, "is_floating": false},
            {"id": 9, "app_id": "dev.forge.terminal", "pid": 91, "is_floating": true}
        ]"#;

        let window = niri_window(windows, 91, FORGE_APP_ID).unwrap();
        assert_eq!(window.id, 9);
        assert!(window.is_floating);
    }

    #[test]
    fn rejects_other_processes_and_malformed_responses() {
        let windows = br#"[{"id": 3, "app_id": "dev.forge.terminal", "pid": 42}]"#;

        assert!(niri_window(windows, 91, FORGE_APP_ID).is_none());
        assert!(niri_window(b"not json", 42, FORGE_APP_ID).is_none());
    }
}
