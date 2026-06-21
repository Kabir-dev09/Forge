use fontdue::{Font, FontSettings};
use forge_core::{Result, ForgeError};

pub struct FontRasterizer {
    pub font: Font,
    pub cell_width: u32,
    pub cell_height: u32,
    pub baseline: u32,
    pub bytes: Vec<u8>,
}

impl FontRasterizer {
    pub fn from_bytes(font_data: &[u8], px_size: f32) -> Result<Self> {
        let t_parse = std::time::Instant::now();
        let font = Font::from_bytes(font_data, FontSettings::default())
            .map_err(|e| ForgeError::Other(e.to_string()))?;
        tracing::info!("[PROFILER] fontdue::Font::from_bytes took: {:?}", t_parse.elapsed());

        // Measure 'M' for width
        let t_rasterize = std::time::Instant::now();
        let (metrics_m, _) = font.rasterize('M', px_size);
        tracing::info!("[PROFILER] rasterize('M') took: {:?}", t_rasterize.elapsed());

        let t_metrics = std::time::Instant::now();
        let line_metrics = font.horizontal_line_metrics(px_size)
            .ok_or_else(|| ForgeError::Other("Failed to get font line metrics".to_string()))?;
        tracing::info!("[PROFILER] horizontal_line_metrics took: {:?}", t_metrics.elapsed());

        let cell_width = metrics_m.advance_width.ceil() as u32;
        let cell_height = line_metrics.new_line_size.ceil() as u32;
        let baseline = line_metrics.ascent.ceil() as u32;

        Ok(Self { font, cell_width, cell_height, baseline, bytes: font_data.to_vec() })
    }

    pub fn rasterize_char(&self, c: char, px_size: f32) -> (fontdue::Metrics, Vec<u8>) {
        self.font.rasterize(c, px_size)
    }
}
