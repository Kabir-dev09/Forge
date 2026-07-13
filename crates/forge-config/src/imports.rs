use forge_core::config_registry::ForgeConfig;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use toml::{map::Map, Value};

#[derive(Debug)]
pub enum ConfigLoadError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Toml {
        path: PathBuf,
        source: toml::de::Error,
    },
    ImportCycle {
        path: PathBuf,
    },
}

impl fmt::Display for ConfigLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to read config {}: {}", path.display(), source)
            }
            Self::Toml { path, source } => {
                write!(f, "failed to parse config {}: {}", path.display(), source)
            }
            Self::ImportCycle { path } => {
                write!(f, "config import cycle involving {}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigLoadError {}

pub fn parse_config_file(path: &Path) -> Result<ForgeConfig, ConfigLoadError> {
    let merged = load_merged_toml(path)?;
    let mut config = ForgeConfig::default();
    crate::extractor::extract_config(&merged, &mut config);
    config.validate();
    Ok(config)
}

pub fn load_merged_toml(path: &Path) -> Result<Value, ConfigLoadError> {
    let mut cache = HashMap::new();
    let mut stack = HashSet::new();
    load_merged_toml_inner(path, &mut cache, &mut stack)
}

fn load_merged_toml_inner(
    path: &Path,
    cache: &mut HashMap<PathBuf, Value>,
    stack: &mut HashSet<PathBuf>,
) -> Result<Value, ConfigLoadError> {
    let canonical = canonical_config_path(path)?;

    if let Some(cached) = cache.get(&canonical) {
        return Ok(cached.clone());
    }

    if !stack.insert(canonical.clone()) {
        return Err(ConfigLoadError::ImportCycle { path: canonical });
    }

    let source = std::fs::read_to_string(&canonical).map_err(|source| ConfigLoadError::Io {
        path: canonical.clone(),
        source,
    })?;
    let current = source
        .parse::<Value>()
        .map_err(|source| ConfigLoadError::Toml {
            path: canonical.clone(),
            source,
        })?;

    let mut merged = Value::Table(Map::new());
    let base_dir = canonical.parent().unwrap_or_else(|| Path::new("."));
    for import in import_paths(&current) {
        let import_path = base_dir.join(import);
        let imported = load_merged_toml_inner(&import_path, cache, stack)?;
        merge_values(&mut merged, imported);
    }

    let mut current_without_imports = current;
    if let Value::Table(table) = &mut current_without_imports {
        table.remove("imports");
    }
    merge_values(&mut merged, current_without_imports);

    stack.remove(&canonical);
    cache.insert(canonical, merged.clone());
    Ok(merged)
}

fn canonical_config_path(path: &Path) -> Result<PathBuf, ConfigLoadError> {
    std::fs::canonicalize(path).map_err(|source| ConfigLoadError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn import_paths(root: &Value) -> Vec<&str> {
    root.get("imports")
        .and_then(Value::as_array)
        .map(|imports| imports.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn merge_values(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Table(base_table), Value::Table(overlay_table)) => {
            merge_tables(base_table, overlay_table);
        }
        (base, overlay) => {
            *base = overlay;
        }
    }
}

fn merge_tables(base: &mut Map<String, Value>, overlay: Map<String, Value>) {
    for (key, overlay_value) in overlay {
        match base.get_mut(&key) {
            Some(base_value) => merge_values(base_value, overlay_value),
            None => {
                base.insert(key, overlay_value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "forge_config_imports_{}_{}",
            test_name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, relative: &str, source: &str) {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, source).unwrap();
    }

    #[test]
    fn loads_one_imported_file() {
        let dir = temp_dir("one");
        write(&dir, "theme.toml", "[colors]\nbackground = \"#111111\"\n");
        write(&dir, "config.toml", "imports = [\"theme.toml\"]\n");

        let config = parse_config_file(&dir.join("config.toml")).unwrap();
        assert_eq!(config.theme.background.r, 17);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn loads_multiple_imports_in_order() {
        let dir = temp_dir("order");
        write(&dir, "first.toml", "[font]\nsize = 12\n");
        write(&dir, "second.toml", "[font]\nsize = 18\n");
        write(
            &dir,
            "config.toml",
            "imports = [\"first.toml\", \"second.toml\"]\n",
        );

        let config = parse_config_file(&dir.join("config.toml")).unwrap();
        assert_eq!(config.font.size, 18.0);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn later_imports_override_earlier_imports() {
        let dir = temp_dir("override_import");
        write(&dir, "theme.toml", "[colors]\nforeground = \"#111111\"\n");
        write(
            &dir,
            "theme-overrides.toml",
            "[colors]\nforeground = \"#222222\"\n",
        );
        write(
            &dir,
            "config.toml",
            "imports = [\"theme.toml\", \"theme-overrides.toml\"]\n",
        );

        let config = parse_config_file(&dir.join("config.toml")).unwrap();
        assert_eq!(config.theme.foreground.r, 34);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn main_config_overrides_imported_values() {
        let dir = temp_dir("main_override");
        write(
            &dir,
            "font.toml",
            "[font]\nfamily = \"Imported\"\nsize = 12\n",
        );
        write(
            &dir,
            "config.toml",
            "imports = [\"font.toml\"]\n[font]\nsize = 20\n",
        );

        let config = parse_config_file(&dir.join("config.toml")).unwrap();
        assert_eq!(config.font.family, "Imported");
        assert_eq!(config.font.size, 20.0);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn resolves_imports_relative_to_containing_file() {
        let dir = temp_dir("relative");
        write(&dir, "themes/base.toml", "[window]\nwidth = 1200\n");
        write(
            &dir,
            "profiles/main.toml",
            "imports = [\"../themes/base.toml\"]\n",
        );

        let config = parse_config_file(&dir.join("profiles/main.toml")).unwrap();
        assert_eq!(config.window.width, 1200);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn supports_nested_imports() {
        let dir = temp_dir("nested");
        write(
            &dir,
            "colors/catppuccin.toml",
            "[colors]\nbackground = \"#010203\"\n",
        );
        write(
            &dir,
            "theme.toml",
            "imports = [\"colors/catppuccin.toml\"]\n",
        );
        write(&dir, "config.toml", "imports = [\"theme.toml\"]\n");

        let config = parse_config_file(&dir.join("config.toml")).unwrap();
        assert_eq!(config.theme.background.r, 1);
        assert_eq!(config.theme.background.g, 2);
        assert_eq!(config.theme.background.b, 3);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn detects_import_cycles() {
        let dir = temp_dir("cycle");
        write(&dir, "a.toml", "imports = [\"b.toml\"]\n");
        write(&dir, "b.toml", "imports = [\"a.toml\"]\n");

        let err = parse_config_file(&dir.join("a.toml")).unwrap_err();
        assert!(matches!(err, ConfigLoadError::ImportCycle { .. }));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_import_is_an_error() {
        let dir = temp_dir("missing");
        write(&dir, "config.toml", "imports = [\"missing.toml\"]\n");

        let err = parse_config_file(&dir.join("config.toml")).unwrap_err();
        assert!(matches!(err, ConfigLoadError::Io { .. }));

        std::fs::remove_dir_all(dir).ok();
    }
}
