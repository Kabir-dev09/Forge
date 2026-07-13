use fontdue::{Font, FontSettings};
use forge_core::{ForgeError, Result};

pub struct FontRasterizer {
    pub font: Font,
    pub cell_width: u32,
    pub cell_height: u32,
    pub baseline: u32,
    pub bytes: Vec<u8>,
}

impl FontRasterizer {
    pub fn update_size(&mut self, px_size: f32) -> Result<()> {
        let (metrics_m, _) = self.font.rasterize('M', px_size);
        let line_metrics = self
            .font
            .horizontal_line_metrics(px_size)
            .ok_or_else(|| ForgeError::Other("Failed to get font line metrics".to_string()))?;

        self.cell_width = metrics_m.advance_width.ceil() as u32;
        self.cell_height = line_metrics.new_line_size.ceil() as u32;
        self.baseline = line_metrics.ascent.ceil() as u32;
        Ok(())
    }

    pub fn from_bytes(font_data: &[u8], px_size: f32) -> Result<Self> {
        let font = Font::from_bytes(font_data, FontSettings::default())
            .map_err(|e| ForgeError::Other(e.to_string()))?;

        let (metrics_m, _) = font.rasterize('M', px_size);
        let line_metrics = font
            .horizontal_line_metrics(px_size)
            .ok_or_else(|| ForgeError::Other("Failed to get font line metrics".to_string()))?;

        let cell_width = metrics_m.advance_width.ceil() as u32;
        let cell_height = line_metrics.new_line_size.ceil() as u32;
        let baseline = line_metrics.ascent.ceil() as u32;

        Ok(Self {
            font,
            cell_width,
            cell_height,
            baseline,
            bytes: font_data.to_vec(),
        })
    }

    pub fn rasterize_char(&self, c: char, px_size: f32) -> (fontdue::Metrics, Vec<u8>) {
        self.font.rasterize(c, px_size)
    }

    pub fn rasterize_glyph_id(&self, glyph_id: u16, px_size: f32) -> (fontdue::Metrics, Vec<u8>) {
        self.font.rasterize_indexed(glyph_id, px_size)
    }

    pub fn has_glyph(&self, c: char) -> bool {
        self.font.has_glyph(c)
    }
}
