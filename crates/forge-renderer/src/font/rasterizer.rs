use fontdue::{Font, FontSettings};
use forge_core::{ForgeError, Result};
use std::borrow::Cow;
use std::sync::OnceLock;

use std::sync::Arc;

#[derive(Clone)]
pub struct FontRasterizer {
    font: Arc<OnceLock<Font>>,
    identity: u64,
    pub cell_width: u32,
    pub cell_height: u32,
    pub baseline: u32,
    pub bytes: Cow<'static, [u8]>,
}

impl FontRasterizer {
    pub fn identity(&self) -> u64 {
        self.identity
    }

    pub fn update_size(&mut self, px_size: f32) -> Result<()> {
        let font = self.parsed_font();
        let (metrics_m, _) = font.rasterize('M', px_size);
        let line_metrics = font
            .horizontal_line_metrics(px_size)
            .ok_or_else(|| ForgeError::Other("Failed to get font line metrics".to_string()))?;

        self.cell_width = metrics_m.advance_width.ceil() as u32;
        self.cell_height = line_metrics.new_line_size.ceil() as u32;
        self.baseline = line_metrics.ascent.ceil() as u32;
        Ok(())
    }

    pub fn from_bytes(font_data: &[u8], px_size: f32) -> Result<Self> {
        Self::from_cow(Cow::Owned(font_data.to_vec()), px_size)
    }

    pub fn from_owned_bytes(font_data: Vec<u8>, px_size: f32) -> Result<Self> {
        Self::from_cow(Cow::Owned(font_data), px_size)
    }

    pub fn from_static_bytes(font_data: &'static [u8], px_size: f32) -> Result<Self> {
        Self::from_cow(Cow::Borrowed(font_data), px_size)
    }

    fn from_cow(font_data: Cow<'static, [u8]>, px_size: f32) -> Result<Self> {
        let identity = font_identity(font_data.as_ref());
        let font = Font::from_bytes(font_data.as_ref(), FontSettings::default())
            .map_err(|e| ForgeError::Other(e.to_string()))?;

        let (metrics_m, _) = font.rasterize('M', px_size);
        let line_metrics = font
            .horizontal_line_metrics(px_size)
            .ok_or_else(|| ForgeError::Other("Failed to get font line metrics".to_string()))?;

        let cell_width = metrics_m.advance_width.ceil() as u32;
        let cell_height = line_metrics.new_line_size.ceil() as u32;
        let baseline = line_metrics.ascent.ceil() as u32;

        let parsed = Arc::new(OnceLock::new());
        let _ = parsed.set(font);
        Ok(Self {
            font: parsed,
            identity,
            cell_width,
            cell_height,
            baseline,
            bytes: font_data,
        })
    }

    pub fn from_static_bytes_with_metrics(
        font_data: &'static [u8],
        cell_width: u32,
        cell_height: u32,
        baseline: u32,
    ) -> Self {
        Self::from_cow_with_metrics(Cow::Borrowed(font_data), cell_width, cell_height, baseline)
    }

    pub fn from_owned_bytes_with_metrics(
        font_data: Vec<u8>,
        cell_width: u32,
        cell_height: u32,
        baseline: u32,
    ) -> Self {
        Self::from_cow_with_metrics(Cow::Owned(font_data), cell_width, cell_height, baseline)
    }

    fn from_cow_with_metrics(
        bytes: Cow<'static, [u8]>,
        cell_width: u32,
        cell_height: u32,
        baseline: u32,
    ) -> Self {
        let identity = font_identity(bytes.as_ref());
        Self {
            font: Arc::new(OnceLock::new()),
            identity,
            cell_width,
            cell_height,
            baseline,
            bytes,
        }
    }

    pub fn parsed_font(&self) -> &Font {
        self.font.get_or_init(|| {
            Font::from_bytes(self.bytes.as_ref(), FontSettings::default())
                .unwrap_or_else(|error| {
                    tracing::warn!(%error, "Failed to parse font bytes, falling back to bundled font");
                    Font::from_bytes(super::builtin::regular(), FontSettings::default())
                        .expect("bundled font must parse")
                })
        })
    }

    /// Parses the face ahead of dynamic glyph rasterization.
    ///
    /// Cached atlases provide metrics without parsing the font. Callers that
    /// enable runtime glyph insertion can prepare that one-time work off the
    /// render thread.
    pub fn prepare_dynamic_rasterization(&self) {
        let _ = self.parsed_font();
    }

    pub fn rasterize_char(&self, c: char, px_size: f32) -> (fontdue::Metrics, Vec<u8>) {
        self.parsed_font().rasterize(c, px_size)
    }

    pub fn rasterize_glyph_id(&self, glyph_id: u16, px_size: f32) -> (fontdue::Metrics, Vec<u8>) {
        self.parsed_font().rasterize_indexed(glyph_id, px_size)
    }

    pub fn has_glyph(&self, c: char) -> bool {
        self.parsed_font().has_glyph(c)
    }

    pub fn glyph_index(&self, c: char) -> u16 {
        self.parsed_font().lookup_glyph_index(c)
    }

    #[cfg(test)]
    pub(crate) fn is_parsed(&self) -> bool {
        self.font.get().is_some()
    }
}

/// Stable identity shared by atlas caches, shaping keys, and rasterizers.
pub(crate) fn font_identity(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_rasterization_can_be_prepared_before_first_glyph() {
        let rasterizer = FontRasterizer::from_static_bytes_with_metrics(
            super::super::builtin::regular(),
            8,
            16,
            12,
        );

        assert!(!rasterizer.is_parsed());
        rasterizer.prepare_dynamic_rasterization();
        assert!(rasterizer.is_parsed());
    }
}
