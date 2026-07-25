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
    InvalidOverride {
        key: String,
        reason: String,
    },
    UnknownOption {
        key: String,
        suggestion: Option<String>,
    },
    Validation {
        path: PathBuf,
        errors: Vec<forge_core::config_registry::ConfigError>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigOverride {
    pub key: String,
    pub value: String,
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
            Self::InvalidOverride { key, reason } => {
                write!(f, "invalid configuration override `{key}`: {reason}")
            }
            Self::UnknownOption { key, suggestion } => {
                write!(f, "unknown configuration option `{key}`")?;
                if let Some(suggestion) = suggestion {
                    write!(f, "\nhint: did you mean `{suggestion}`?")?;
                }
                Ok(())
            }
            Self::Validation { path, errors } => {
                write!(f, "invalid configuration in {}", path.display())?;
                for error in errors {
                    write!(f, "\n  - {error}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ConfigLoadError {}

pub fn parse_config_file(path: &Path) -> Result<ForgeConfig, ConfigLoadError> {
    let merged = load_merged_toml(path)?;
    let mut config: ForgeConfig = merged.try_into().map_err(|source| ConfigLoadError::Toml {
        path: path.to_path_buf(),
        source,
    })?;
    let errors = config.validate();
    for err in errors {
        tracing::warn!("Config validation error in {}: {}", path.display(), err);
    }
    Ok(config)
}

pub fn parse_config_file_strict(
    path: &Path,
    overrides: &[ConfigOverride],
    strict_file: bool,
) -> Result<ForgeConfig, ConfigLoadError> {
    let mut merged = load_merged_toml(path)?;
    let mut override_tree = Value::Table(Map::new());
    for config_override in overrides {
        apply_override(&mut override_tree, config_override)?;
    }

    if !overrides.is_empty() {
        // Validate only CLI-provided paths strictly. Existing config files remain
        // compatible with Forge's established unknown-field behavior.
        let mut unknown = Vec::new();
        let mut override_config: ForgeConfig =
            serde_ignored::deserialize(override_tree.clone(), |path| {
                unknown.push(path.to_string());
            })
            .map_err(|source| ConfigLoadError::Toml {
                path: path.to_path_buf(),
                source,
            })?;
        if let Some(key) = unknown.into_iter().next() {
            return Err(ConfigLoadError::UnknownOption {
                suggestion: nearest_known_path(&key),
                key,
            });
        }
        let mut errors = override_config.strict_validation_errors();
        errors.extend(override_config.validate());
        if !errors.is_empty() {
            return Err(ConfigLoadError::Validation {
                path: PathBuf::from("command line"),
                errors,
            });
        }
        merge_values(&mut merged, override_tree);
    }

    let mut config: ForgeConfig = if strict_file {
        let mut unknown = Vec::new();
        let config = serde_ignored::deserialize(merged, |unknown_path| {
            unknown.push(unknown_path.to_string());
        })
        .map_err(|source| ConfigLoadError::Toml {
            path: path.to_path_buf(),
            source,
        })?;
        if let Some(key) = unknown.into_iter().next() {
            return Err(ConfigLoadError::UnknownOption {
                suggestion: nearest_known_path(&key),
                key,
            });
        }
        config
    } else {
        merged.try_into().map_err(|source| ConfigLoadError::Toml {
            path: path.to_path_buf(),
            source,
        })?
    };
    if strict_file {
        let mut errors = config.strict_validation_errors();
        errors.extend(config.validate());
        if !errors.is_empty() {
            return Err(ConfigLoadError::Validation {
                path: path.to_path_buf(),
                errors,
            });
        }
    } else {
        for error in config.validate() {
            tracing::warn!("Config validation error in {}: {}", path.display(), error);
        }
    }
    Ok(config)
}

fn apply_override(
    root: &mut Value,
    config_override: &ConfigOverride,
) -> Result<(), ConfigLoadError> {
    let source = format!("value = {}", config_override.value);
    let mut wrapper =
        source
            .parse::<Value>()
            .map_err(|error| ConfigLoadError::InvalidOverride {
                key: config_override.key.clone(),
                reason: error.message().to_string(),
            })?;
    let value = wrapper
        .as_table_mut()
        .and_then(|table| table.remove("value"))
        .ok_or_else(|| ConfigLoadError::InvalidOverride {
            key: config_override.key.clone(),
            reason: "the value is not valid TOML".to_string(),
        })?;

    let mut segments = config_override.key.split('.').peekable();
    let mut current = root;
    while let Some(segment) = segments.next() {
        let table = current
            .as_table_mut()
            .ok_or_else(|| ConfigLoadError::InvalidOverride {
                key: config_override.key.clone(),
                reason: format!("`{segment}` is nested below a non-table value"),
            })?;
        if segments.peek().is_none() {
            table.insert(segment.to_string(), value);
            return Ok(());
        }
        current = table
            .entry(segment.to_string())
            .or_insert_with(|| Value::Table(Map::new()));
    }

    Err(ConfigLoadError::InvalidOverride {
        key: config_override.key.clone(),
        reason: "the configuration key is empty".to_string(),
    })
}

fn nearest_known_path(unknown: &str) -> Option<String> {
    let defaults = Value::try_from(ForgeConfig::default()).ok()?;
    let mut paths = Vec::new();
    collect_leaf_paths(&defaults, "", &mut paths);
    paths
        .into_iter()
        .map(|candidate| {
            let distance = edit_distance(unknown, &candidate);
            (candidate, distance)
        })
        .filter(|(_, distance)| *distance <= 3)
        .min_by_key(|(_, distance)| *distance)
        .map(|(candidate, _)| candidate)
}

fn collect_leaf_paths(value: &Value, prefix: &str, output: &mut Vec<String>) {
    let Some(table) = value.as_table() else {
        if !prefix.is_empty() {
            output.push(prefix.to_string());
        }
        return;
    };
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if value.is_table() {
            collect_leaf_paths(value, &path, output);
        } else {
            output.push(path);
        }
    }
}

fn edit_distance(left: &str, right: &str) -> usize {
    let mut previous: Vec<usize> = (0..=right.chars().count()).collect();
    let mut current = vec![0; previous.len()];
    for (left_index, left_char) in left.chars().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right.chars().enumerate() {
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + usize::from(left_char != right_char));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.chars().count()]
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
        write(&dir, "theme.toml", "[theme]\nbackground = \"#111111\"\n");
        write(&dir, "config.toml", "imports = [\"theme.toml\"]\n");

        let config = parse_config_file(&dir.join("config.toml")).unwrap();
        assert_eq!(config.theme.parsed_background.r, 17);

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
        write(&dir, "theme.toml", "[theme]\nforeground = \"#111111\"\n");
        write(
            &dir,
            "theme-overrides.toml",
            "[theme]\nforeground = \"#222222\"\n",
        );
        write(
            &dir,
            "config.toml",
            "imports = [\"theme.toml\", \"theme-overrides.toml\"]\n",
        );

        let config = parse_config_file(&dir.join("config.toml")).unwrap();
        assert_eq!(config.theme.parsed_foreground.r, 34);

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
            "[theme]\nbackground = \"#010203\"\n",
        );
        write(
            &dir,
            "theme.toml",
            "imports = [\"colors/catppuccin.toml\"]\n",
        );
        write(&dir, "config.toml", "imports = [\"theme.toml\"]\n");

        let config = parse_config_file(&dir.join("config.toml")).unwrap();
        assert_eq!(config.theme.parsed_background.r, 1);
        assert_eq!(config.theme.parsed_background.g, 2);
        assert_eq!(config.theme.parsed_background.b, 3);

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

    #[test]
    fn strict_overrides_preserve_toml_types() {
        let dir = temp_dir("typed_overrides");
        write(&dir, "config.toml", "");
        let config = parse_config_file_strict(
            &dir.join("config.toml"),
            &[
                ConfigOverride {
                    key: "font.size".to_string(),
                    value: "15".to_string(),
                },
                ConfigOverride {
                    key: "window.opacity".to_string(),
                    value: "0.85".to_string(),
                },
                ConfigOverride {
                    key: "window.center_on_launch".to_string(),
                    value: "true".to_string(),
                },
                ConfigOverride {
                    key: "cursor.trail.enabled".to_string(),
                    value: "true".to_string(),
                },
                ConfigOverride {
                    key: "shell.integration_enabled".to_string(),
                    value: "false".to_string(),
                },
                ConfigOverride {
                    key: "font.family".to_string(),
                    value: "\"Fira Code\"".to_string(),
                },
                ConfigOverride {
                    key: "font.ligatures.features".to_string(),
                    value: "[\"liga\", \"calt\"]".to_string(),
                },
            ],
            true,
        )
        .unwrap();

        assert_eq!(config.font.size, 15.0);
        assert_eq!(config.window.opacity, 0.85);
        assert!(config.window.center_on_launch);
        assert!(config.cursor.trail.enabled);
        assert!(!config.shell.integration_enabled);
        assert_eq!(config.font.family, "Fira Code");
        assert_eq!(config.font.ligatures.features, ["liga", "calt"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn overrides_apply_after_imports_and_later_overrides_win() {
        let dir = temp_dir("override_precedence");
        write(&dir, "base.toml", "[font]\nsize = 12\n");
        write(
            &dir,
            "config.toml",
            "imports = [\"base.toml\"]\n[font]\nsize = 14\n",
        );
        let config = parse_config_file_strict(
            &dir.join("config.toml"),
            &[
                ConfigOverride {
                    key: "font.size".to_string(),
                    value: "16".to_string(),
                },
                ConfigOverride {
                    key: "font.size".to_string(),
                    value: "18".to_string(),
                },
            ],
            true,
        )
        .unwrap();

        assert_eq!(config.font.size, 18.0);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn unknown_override_is_rejected_with_a_suggestion() {
        let dir = temp_dir("unknown_override");
        write(&dir, "config.toml", "");
        let error = parse_config_file_strict(
            &dir.join("config.toml"),
            &[ConfigOverride {
                key: "font.szie".to_string(),
                value: "14".to_string(),
            }],
            true,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ConfigLoadError::UnknownOption {
                ref key,
                suggestion: Some(ref suggestion),
            } if key == "font.szie" && suggestion == "font.size"
        ));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn malformed_type_and_out_of_range_override_are_errors() {
        let dir = temp_dir("invalid_overrides");
        write(&dir, "config.toml", "");
        let type_error = parse_config_file_strict(
            &dir.join("config.toml"),
            &[ConfigOverride {
                key: "font.size".to_string(),
                value: "\"large\"".to_string(),
            }],
            true,
        )
        .unwrap_err();
        assert!(matches!(type_error, ConfigLoadError::Toml { .. }));

        let range_error = parse_config_file_strict(
            &dir.join("config.toml"),
            &[ConfigOverride {
                key: "font.size".to_string(),
                value: "100".to_string(),
            }],
            true,
        )
        .unwrap_err();
        assert!(matches!(range_error, ConfigLoadError::Validation { .. }));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn malformed_toml_override_value_is_rejected() {
        let dir = temp_dir("malformed_override");
        write(&dir, "config.toml", "");
        let error = parse_config_file_strict(
            &dir.join("config.toml"),
            &[ConfigOverride {
                key: "font.ligatures.features".to_string(),
                value: "[\"liga\"".to_string(),
            }],
            true,
        )
        .unwrap_err();
        assert!(matches!(error, ConfigLoadError::InvalidOverride { .. }));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn default_file_compatibility_is_preserved_when_using_overrides() {
        let dir = temp_dir("default_compatibility");
        write(
            &dir,
            "config.toml",
            "[legacy_future_section]\nenabled = true\n[keybinds]\nunknown_action = \"ctrl+x\"\n",
        );
        let config = parse_config_file_strict(
            &dir.join("config.toml"),
            &[ConfigOverride {
                key: "font.size".to_string(),
                value: "17".to_string(),
            }],
            false,
        )
        .unwrap();

        assert_eq!(config.font.size, 17.0);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn explicitly_selected_file_uses_strict_validation() {
        let dir = temp_dir("strict_custom_file");
        write(&dir, "config.toml", "[theme]\nbackground = \"invalid\"\n");
        let error = parse_config_file_strict(&dir.join("config.toml"), &[], true).unwrap_err();

        assert!(matches!(error, ConfigLoadError::Validation { .. }));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn explicitly_selected_file_rejects_unknown_options() {
        let dir = temp_dir("strict_custom_unknown");
        write(&dir, "config.toml", "[font]\nszie = 14\n");
        let error = parse_config_file_strict(&dir.join("config.toml"), &[], true).unwrap_err();

        assert!(matches!(
            error,
            ConfigLoadError::UnknownOption {
                ref key,
                suggestion: Some(ref suggestion),
            } if key == "font.szie" && suggestion == "font.size"
        ));
        std::fs::remove_dir_all(dir).ok();
    }
}
