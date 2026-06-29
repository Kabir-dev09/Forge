use forge_core::config_registry::FontConfig;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFiles {
    pub regular: PathBuf,
    pub bold: Option<PathBuf>,
    pub fallbacks: Vec<PathBuf>,
}

pub fn resolve_font_files(config: &FontConfig) -> Option<FontFiles> {
    resolve_font_files_with_dirs(config, bundled_font_dirs())
}

fn resolve_font_files_with_dirs<I>(config: &FontConfig, bundled_dirs: I) -> Option<FontFiles>
where
    I: IntoIterator<Item = PathBuf>,
{
    let bundled_dirs: Vec<PathBuf> = bundled_dirs.into_iter().collect();
    let regular = configured_font_path(&config.family)
        .or_else(|| find_bundled_font(&bundled_dirs, "JetBrainsMono-Regular.ttf"))?;

    let bold = config
        .bold_family
        .as_deref()
        .and_then(configured_font_path)
        .or_else(|| find_bundled_font(&bundled_dirs, "JetBrainsMono-Bold.ttf"));

    let mut fallbacks = Vec::new();
    if config.nerd_fonts {
        for charset in ["2605", "25CF", "2713", "E0B0"] {
            if let Some(path) = fontconfig_match_charset(charset) {
                push_unique_font_path(&mut fallbacks, path);
            }
        }
    }
    fallbacks.retain(|path| path != &regular && Some(path) != bold.as_ref());

    Some(FontFiles {
        regular,
        bold,
        fallbacks,
    })
}

fn configured_font_path(value: &str) -> Option<PathBuf> {
    if value.trim().is_empty() {
        return None;
    }

    let path = expand_home(value.trim());
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

fn expand_home(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(value)
}

fn find_bundled_font(dirs: &[PathBuf], file_name: &str) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(file_name))
        .find(|path| path.is_file())
}

fn fontconfig_match_charset(charset: &str) -> Option<PathBuf> {
    let output = std::process::Command::new("fc-match")
        .arg(format!(":charset={charset}"))
        .arg("--format")
        .arg("%{file}")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8(output.stdout).ok()?;
    let path = PathBuf::from(path.trim());
    path.is_file().then_some(path)
}

fn push_unique_font_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn bundled_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(workspace_root) = manifest_dir.parent().and_then(|path| path.parent()) {
        dirs.push(workspace_root.join("assets/fonts"));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            dirs.push(exe_dir.join("assets/fonts"));
            dirs.push(exe_dir.join("../share/forge/assets/fonts"));
        }
    }

    dirs.push(PathBuf::from("/usr/share/forge/assets/fonts"));
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "forge_font_paths_{}_{}",
            test_name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolves_configured_regular_and_bold_paths() {
        let dir = tmp_dir("configured");
        let regular = dir.join("regular.ttf");
        let bold = dir.join("bold.ttf");
        std::fs::write(&regular, b"regular").unwrap();
        std::fs::write(&bold, b"bold").unwrap();

        let mut config = FontConfig::default();
        config.family = regular.to_string_lossy().into_owned();
        config.bold_family = Some(bold.to_string_lossy().into_owned());
        config.nerd_fonts = false;

        let files = resolve_font_files_with_dirs(&config, Vec::new()).unwrap();
        assert_eq!(files.regular, regular);
        assert_eq!(files.bold, Some(bold));
        assert!(files.fallbacks.is_empty());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn falls_back_to_bundled_fonts() {
        let dir = tmp_dir("bundled");
        let regular = dir.join("JetBrainsMono-Regular.ttf");
        let bold = dir.join("JetBrainsMono-Bold.ttf");
        std::fs::write(&regular, b"regular").unwrap();
        std::fs::write(&bold, b"bold").unwrap();

        let config = FontConfig::default();
        let files = resolve_font_files_with_dirs(&config, vec![dir.clone()]).unwrap();
        assert_eq!(files.regular, regular);
        assert_eq!(files.bold, Some(bold));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn returns_none_when_no_regular_font_exists() {
        let config = FontConfig::default();
        assert!(resolve_font_files_with_dirs(&config, Vec::new()).is_none());
    }

    #[test]
    fn push_unique_font_path_deduplicates_paths() {
        let mut paths = Vec::new();
        let path = PathBuf::from("/tmp/example-font.ttf");

        push_unique_font_path(&mut paths, path.clone());
        push_unique_font_path(&mut paths, path);

        assert_eq!(paths.len(), 1);
    }
}
