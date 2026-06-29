use forge_core::config_registry::BlurConfig;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

const FORGE_APP_ID: &str = "dev.forge.terminal";
const MANAGED_RULE_BEGIN: &str = "// Forge terminal compositor blur rule";
const MANAGED_RULE_END: &str = "// End Forge terminal compositor blur rule";

pub fn ensure_rule_after_launch(config: &BlurConfig) {
    if !config.enabled || !is_niri_session() {
        return;
    }

    std::thread::Builder::new()
        .name("forge-niri-blur-rule".to_string())
        .spawn(|| {
            if let Err(err) = ensure_rule_in_default_config() {
                tracing::debug!(?err, "Unable to ensure Niri blur window-rule");
            }
        })
        .ok();
}

fn is_niri_session() -> bool {
    std::env::var_os("NIRI_SOCKET").is_some()
        || env_contains_niri("XDG_CURRENT_DESKTOP")
        || env_contains_niri("XDG_SESSION_DESKTOP")
}

fn env_contains_niri(name: &str) -> bool {
    std::env::var(name)
        .map(|value| value.to_ascii_lowercase().contains("niri"))
        .unwrap_or(false)
}

fn ensure_rule_in_default_config() -> std::io::Result<()> {
    let Some(config_dir) = dirs::config_dir() else {
        return Ok(());
    };
    ensure_rule_in_file(&config_dir.join("niri/config.kdl"))
}

fn ensure_rule_in_file(path: &Path) -> std::io::Result<()> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    if has_forge_blur_rule(&contents) {
        return Ok(());
    }

    let rule = forge_blur_rule();
    let separator = if contents.ends_with('\n') { "" } else { "\n" };
    let mut file = OpenOptions::new().append(true).open(path)?;
    file.write_all(separator.as_bytes())?;
    file.write_all(rule.as_bytes())?;
    Ok(())
}

fn has_forge_blur_rule(contents: &str) -> bool {
    if contents.contains(MANAGED_RULE_BEGIN) && contents.contains(MANAGED_RULE_END) {
        return true;
    }

    window_rule_blocks(contents).any(|block| {
        block.contains(FORGE_APP_ID)
            && block.contains("background-effect")
            && block.contains("blur true")
    })
}

fn window_rule_blocks(contents: &str) -> impl Iterator<Item = &str> {
    let mut blocks = Vec::new();
    let mut search_from = 0;

    while let Some(relative_start) = contents[search_from..].find("window-rule") {
        let start = search_from + relative_start;
        let Some(open_relative) = contents[start..].find('{') else {
            break;
        };
        let open = start + open_relative;
        let mut depth = 0usize;

        for (offset, ch) in contents[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        let end = open + offset + ch.len_utf8();
                        blocks.push(&contents[start..end]);
                        search_from = end;
                        break;
                    }
                }
                _ => {}
            }
        }

        if search_from <= start {
            break;
        }
    }

    blocks.into_iter()
}

fn forge_blur_rule() -> String {
    format!(
        r#"{MANAGED_RULE_BEGIN}
window-rule {{
    match app-id="{FORGE_APP_ID}"
    draw-border-with-background false
    background-effect {{
        blur true
    }}
}}
{MANAGED_RULE_END}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn detects_existing_managed_rule() {
        let contents = format!("{MANAGED_RULE_BEGIN}\nwindow-rule {{}}\n{MANAGED_RULE_END}\n");

        assert!(has_forge_blur_rule(&contents));
    }

    #[test]
    fn detects_existing_forge_blur_rule() {
        let contents = r#"
window-rule {
    match app-id="dev.forge.terminal"
    background-effect {
        blur true
    }
}
"#;

        assert!(has_forge_blur_rule(contents));
    }

    #[test]
    fn appends_rule_when_missing() {
        let path = temp_config_path();
        fs::write(&path, "window-rule {\n    match app-id=\"other\"\n}\n").unwrap();

        ensure_rule_in_file(&path).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains(FORGE_APP_ID));
        assert!(contents.contains("blur true"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn ignores_blur_rule_for_different_window() {
        let contents = r#"
window-rule {
    match app-id="dev.forge.terminal"
}

window-rule {
    match app-id="com.mitchellh.ghostty"
    background-effect {
        blur true
    }
}
"#;

        assert!(!has_forge_blur_rule(contents));
    }

    #[test]
    fn does_not_append_duplicate_rule() {
        let path = temp_config_path();
        fs::write(&path, forge_blur_rule()).unwrap();

        ensure_rule_in_file(&path).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(contents.matches(FORGE_APP_ID).count(), 1);

        let _ = fs::remove_file(path);
    }

    fn temp_config_path() -> std::path::PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("forge-niri-blur-rule-{id}.kdl"))
    }
}
