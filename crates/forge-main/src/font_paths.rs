use forge_core::config_registry::FontConfig;
use forge_core::{ForgeError, Result};
use forge_renderer::font::{atlas::GlyphAtlas, builtin, rasterizer::FontRasterizer, FontData};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontSource {
    BuiltinRegular,
    BuiltinBold,
    BuiltinItalic,
    BuiltinBoldItalic,
    File(PathBuf),
}

impl FontSource {
    pub fn label(&self) -> String {
        match self {
            Self::BuiltinRegular => "builtin:regular".into(),
            Self::BuiltinBold => "builtin:bold".into(),
            Self::BuiltinItalic => "builtin:italic".into(),
            Self::BuiltinBoldItalic => "builtin:bold-italic".into(),
            Self::File(path) => path.display().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFiles {
    pub builtin: bool,
    pub regular: FontSource,
    pub bold: Option<FontSource>,
    pub italic: Option<FontSource>,
    pub bold_italic: Option<FontSource>,
    pub fallbacks: Vec<PathBuf>,
}

pub fn resolve_font_files(config: &FontConfig) -> Option<FontFiles> {
    let family = config.family.trim();
    if family.is_empty() || family.eq_ignore_ascii_case("builtin") {
        return Some(FontFiles {
            builtin: true,
            regular: FontSource::BuiltinRegular,
            bold: Some(FontSource::BuiltinBold),
            italic: Some(FontSource::BuiltinItalic),
            bold_italic: Some(FontSource::BuiltinBoldItalic),
            // The embedded Nerd Font already contains the symbols Forge ships
            // for. Avoid spawning fontconfig on the default startup path.
            fallbacks: Vec::new(),
        });
    }

    let regular = resolve_configured_font(family, "Regular")?;
    let bold = resolve_optional_style(config.bold_family.as_deref(), family, "Bold");
    let italic = resolve_optional_style(config.italic_family.as_deref(), family, "Italic");
    let bold_italic =
        resolve_optional_style(config.bold_italic_family.as_deref(), family, "Bold Italic");
    let primary_paths: Vec<_> = [
        &regular,
        bold.as_ref().unwrap_or(&regular),
        italic.as_ref().unwrap_or(&regular),
    ]
    .into_iter()
    .filter_map(|source| match source {
        FontSource::File(path) => Some(path.clone()),
        _ => None,
    })
    .collect();

    Some(FontFiles {
        builtin: false,
        regular,
        bold,
        italic,
        bold_italic,
        fallbacks: resolve_fallbacks(config.nerd_fonts, &primary_paths),
    })
}

pub fn load_font_data(config: &FontConfig) -> Result<FontData> {
    let files = resolve_font_files(config).unwrap_or_else(|| {
        tracing::warn!("Font family '{}' was not found, falling back to embedded font", config.family);
        let mut fallback_config = config.clone();
        fallback_config.family = "builtin".to_string();
        resolve_font_files(&fallback_config).expect("builtin font must resolve")
    });
    let regular_bytes = LoadedFontBytes::read(&files.regular)?;
    let bold_bytes = load_optional_bytes(files.bold.as_ref());
    let italic_bytes = load_optional_bytes(files.italic.as_ref());
    let bold_italic_bytes = load_optional_bytes(files.bold_italic.as_ref());
    let fallback_bytes: Vec<_> = files
        .fallbacks
        .iter()
        .filter_map(|path| {
            let source = FontSource::File(path.clone());
            LoadedFontBytes::read(&source)
                .map_err(|error| tracing::warn!(font = %path.display(), %error, "Failed to load fallback font"))
                .ok()
        })
        .collect();
    let descriptor = forge_renderer::font::atlas::GlyphAtlasDescriptor::from_font_bytes(
        regular_bytes.as_slice(),
        bold_bytes.as_ref().map(LoadedFontBytes::as_slice),
        italic_bytes.as_ref().map(LoadedFontBytes::as_slice),
        bold_italic_bytes.as_ref().map(LoadedFontBytes::as_slice),
        config.size,
        false,
    );

    let cached_atlas = if files.builtin && config.size.to_bits() == builtin::DEFAULT_SIZE.to_bits()
    {
        Some(
            GlyphAtlas::from_cache_bytes(builtin::default_atlas(), &descriptor).map_err(
                |error| ForgeError::Other(format!("embedded font atlas is invalid: {error}")),
            )?,
        )
    } else {
        load_custom_atlas_cache(&descriptor)
    };

    let (regular, bold, italic, bold_italic, atlas) = if let Some(atlas) = cached_atlas {
        tracing::debug!("Loaded font atlas without parsing or rasterizing font faces");
        let metrics = (
            atlas.font_cell_width,
            atlas.font_cell_height,
            atlas.font_baseline,
        );
        (
            regular_bytes.into_lazy_rasterizer(metrics),
            bold_bytes.map(|bytes| bytes.into_lazy_rasterizer(metrics)),
            italic_bytes.map(|bytes| bytes.into_lazy_rasterizer(metrics)),
            bold_italic_bytes.map(|bytes| bytes.into_lazy_rasterizer(metrics)),
            atlas,
        )
    } else {
        let regular = regular_bytes.into_rasterizer(config.size)?;
        let bold = into_optional_rasterizer(bold_bytes, config.size);
        let italic = into_optional_rasterizer(italic_bytes, config.size);
        let bold_italic = into_optional_rasterizer(bold_italic_bytes, config.size);
        let atlas = GlyphAtlas::build(
            &regular,
            bold.as_ref(),
            italic.as_ref(),
            bold_italic.as_ref(),
            config.size,
            false,
        )?;
        persist_custom_atlas_cache(&atlas);
        (regular, bold, italic, bold_italic, atlas)
    };
    let metrics = (
        atlas.font_cell_width,
        atlas.font_cell_height,
        atlas.font_baseline,
    );
    let fallbacks = fallback_bytes
        .into_iter()
        .map(|bytes| bytes.into_lazy_rasterizer(metrics))
        .collect();

    // A cached atlas deliberately leaves FontDue uninitialized. Ligatures can
    // still create shaped glyph IDs at runtime, so perform that one-time parse
    // here on the existing font-loading thread instead of stalling rendering.
    if config.ligatures.enabled {
        let r_reg = regular.clone();
        let r_bold = bold.clone();
        let r_italic = italic.clone();
        let r_bold_italic = bold_italic.clone();
        std::thread::spawn(move || {
            let _span = tracing::info_span!("ligature_warmup").entered();
            r_reg.prepare_dynamic_rasterization();
            if let Some(rasterizer) = r_bold {
                rasterizer.prepare_dynamic_rasterization();
            }
            if let Some(rasterizer) = r_italic {
                rasterizer.prepare_dynamic_rasterization();
            }
            if let Some(rasterizer) = r_bold_italic {
                rasterizer.prepare_dynamic_rasterization();
            }
            tracing::debug!("Background ligature parsing complete");
        });
    }

    Ok(FontData {
        regular,
        bold,
        italic,
        bold_italic,
        fallbacks,
        px_size: config.size,
        atlas,
    })
}

enum LoadedFontBytes {
    Static(&'static [u8]),
    Owned(Vec<u8>),
}

impl LoadedFontBytes {
    fn read(source: &FontSource) -> Result<Self> {
        match source {
            FontSource::BuiltinRegular => Ok(Self::Static(builtin::regular())),
            FontSource::BuiltinBold => Ok(Self::Static(builtin::bold())),
            FontSource::BuiltinItalic => Ok(Self::Static(builtin::italic())),
            FontSource::BuiltinBoldItalic => Ok(Self::Static(builtin::bold_italic())),
            FontSource::File(path) => std::fs::read(path).map(Self::Owned).map_err(|error| {
                ForgeError::Other(format!("failed to read font {}: {error}", path.display()))
            }),
        }
    }

    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Static(bytes) => bytes,
            Self::Owned(bytes) => bytes,
        }
    }

    fn into_rasterizer(self, size: f32) -> Result<FontRasterizer> {
        match self {
            Self::Static(bytes) => FontRasterizer::from_static_bytes(bytes, size),
            Self::Owned(bytes) => FontRasterizer::from_owned_bytes(bytes, size),
        }
    }

    fn into_lazy_rasterizer(self, metrics: (u32, u32, u32)) -> FontRasterizer {
        match self {
            Self::Static(bytes) => FontRasterizer::from_static_bytes_with_metrics(
                bytes, metrics.0, metrics.1, metrics.2,
            ),
            Self::Owned(bytes) => FontRasterizer::from_owned_bytes_with_metrics(
                bytes, metrics.0, metrics.1, metrics.2,
            ),
        }
    }
}

fn load_optional_bytes(source: Option<&FontSource>) -> Option<LoadedFontBytes> {
    source.and_then(|source| {
        LoadedFontBytes::read(source)
            .map_err(
                |error| tracing::warn!(font = %source.label(), %error, "Failed to load font style"),
            )
            .ok()
    })
}

fn into_optional_rasterizer(bytes: Option<LoadedFontBytes>, size: f32) -> Option<FontRasterizer> {
    bytes.and_then(|bytes| {
        bytes
            .into_rasterizer(size)
            .map_err(|error| tracing::warn!(%error, "Failed to parse font style"))
            .ok()
    })
}

fn custom_atlas_cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("forge/font-atlas.fga"))
}

fn load_custom_atlas_cache(
    descriptor: &forge_renderer::font::atlas::GlyphAtlasDescriptor,
) -> Option<GlyphAtlas> {
    let path = custom_atlas_cache_path()?;
    let bytes = std::fs::read(path).ok()?;
    GlyphAtlas::from_cache_bytes(&bytes, descriptor).ok()
}

fn persist_custom_atlas_cache(atlas: &GlyphAtlas) {
    let Some(path) = custom_atlas_cache_path() else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if let Err(error) = std::fs::create_dir_all(parent) {
        tracing::debug!(%error, "Could not create font atlas cache directory");
        return;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let bytes = atlas.to_cache_bytes();
    if let Err(error) = std::fs::write(&temporary, bytes) {
        tracing::debug!(%error, "Could not write font atlas cache");
        return;
    }
    if let Err(error) = std::fs::rename(&temporary, &path) {
        tracing::debug!(%error, "Could not atomically replace font atlas cache");
        let _ = std::fs::remove_file(temporary);
    }
}

fn resolve_optional_style(value: Option<&str>, family: &str, style: &str) -> Option<FontSource> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => resolve_configured_font(value, style),
        None => fontconfig_match_family(family, style).map(FontSource::File),
    }
}

fn resolve_configured_font(value: &str, style: &str) -> Option<FontSource> {
    let path = expand_home(value);
    if path.is_file() {
        return Some(FontSource::File(path));
    }
    fontconfig_match_family(value, style).map(FontSource::File)
}

fn expand_home(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(value)
}

fn normalize_family(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn fontconfig_match_family(family: &str, style: &str) -> Option<PathBuf> {
    let output = std::process::Command::new("fc-match")
        .arg(format!("{family}:style={style}"))
        .arg("--format")
        .arg("%{family}\n%{file}")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let output = String::from_utf8(output.stdout).ok()?;
    let mut lines = output.lines();
    let matched_family = lines.next()?;
    let requested = normalize_family(family);
    let matched = matched_family.split(',').any(|candidate| {
        let candidate = normalize_family(candidate);
        candidate == requested || candidate.starts_with(&requested)
    });
    if !matched {
        tracing::warn!(
            family,
            style,
            matched_family,
            "Configured font family was not found"
        );
        return None;
    }
    let path = PathBuf::from(lines.next()?.trim());
    path.is_file().then_some(path)
}

fn resolve_fallbacks(enabled: bool, primary_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut fallbacks = Vec::new();
    if enabled {
        for charset in ["2605", "25CF", "2713", "E0B0"] {
            if let Some(path) = fontconfig_match_charset(charset) {
                if !primary_paths.contains(&path) && !fallbacks.contains(&path) {
                    fallbacks.push(path);
                }
            }
        }
    }
    fallbacks
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
    let path = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uses_embedded_four_style_family() {
        let files = resolve_font_files(&FontConfig::default()).unwrap();
        assert!(files.builtin);
        assert_eq!(files.regular, FontSource::BuiltinRegular);
        assert_eq!(files.bold, Some(FontSource::BuiltinBold));
        assert_eq!(files.italic, Some(FontSource::BuiltinItalic));
        assert_eq!(files.bold_italic, Some(FontSource::BuiltinBoldItalic));
    }

    #[test]
    fn default_font_data_uses_prebuilt_real_italic_atlas() {
        let data = load_font_data(&FontConfig::default()).unwrap();

        assert!(data.bold.is_some());
        assert!(data.italic.is_some());
        assert!(data.bold_italic.is_some());
        assert!(!data.atlas.glyphs_italic.is_empty());
        assert!(!data.atlas.glyphs_bold_italic.is_empty());
        for glyph in ['', '', '', ''] {
            assert!(data
                .atlas
                .get_exact(glyph, forge_renderer::font::atlas::FontStyle::Regular)
                .is_some());
        }
    }

    #[test]
    fn configured_path_remains_supported() {
        let path = std::env::temp_dir().join(format!("forge-font-{}.ttf", std::process::id()));
        std::fs::write(&path, b"font").unwrap();
        let config = FontConfig {
            family: path.to_string_lossy().into_owned(),
            nerd_fonts: false,
            ..FontConfig::default()
        };
        let files = resolve_font_files(&config).unwrap();
        assert_eq!(files.regular, FontSource::File(path.clone()));
        std::fs::remove_file(path).ok();
    }
}
