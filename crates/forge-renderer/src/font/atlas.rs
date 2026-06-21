use std::collections::HashMap;
use forge_core::Result;
use super::rasterizer::FontRasterizer;

#[derive(Debug, Clone, Copy)]
pub struct GlyphMetrics {
    pub u0: f32, pub v0: f32, // Top-left UV
    pub u1: f32, pub v1: f32, // Bottom-right UV
    pub width: u32,           // Actual glyph pixel width
    pub height: u32,          // Actual glyph pixel height
    pub bearing_y: i32,       // Distance from baseline to top of glyph
    pub bearing_x: i32,       // Distance from cell left to glyph left
}

pub struct GlyphAtlas {
    pub pixels: Vec<u8>, // RGBA (R=G=B=255, A=coverage) to simplify Vulkan format matching
    pub atlas_width: u32,
    pub atlas_height: u32,
    pub glyphs: HashMap<char, GlyphMetrics>,
    pub glyphs_bold: HashMap<char, GlyphMetrics>,
}

impl GlyphAtlas {
    pub fn clear_pixels(&mut self) {
        self.pixels = Vec::new();
        self.pixels.shrink_to_fit();
    }

    pub fn build(rasterizer: &FontRasterizer, bold_rasterizer: Option<&FontRasterizer>, px_size: f32, fast_mode: bool) -> Result<Self> {
        // Fast mode: Only rasterize ASCII to boot instantly (under 20ms)
        // Full mode: Rasterize ASCII + Box Drawing + 6,400+ Nerd Font icons (takes ~800ms)
        let chars: Vec<char> = if fast_mode {
            (0x20..=0x7E).filter_map(std::char::from_u32).collect()
        } else {
            (0x20..=0x017F)           // ASCII + Latin-1 + Latin Extended-A
                .chain(0x0370..=0x03FF) // Greek and Coptic
                .chain(0x0400..=0x04FF) // Cyrillic
                .chain(0x2000..=0x2BFF) // Punctuation, Arrows, Math, Box, Block, Braille, Misc
                .chain(0xE000..=0xF8FF) // PUA (Nerd Fonts Seti, Devicons, FA, etc)
                .chain(0xF0000..=0xF1AF0) // Material Design Icons (Supplementary PUA-A)
                .chain(0x1F000..=0x1FAFF) // Emojis and Symbols
                .filter_map(std::char::from_u32).collect()
        };

        // 128 columns layout to accommodate ~13,600 characters gracefully
        let cols = 128u32;
        let font_count = if bold_rasterizer.is_some() { 2 } else { 1 };
        let total_glyphs = chars.len() as u32 * font_count;
        let rows = total_glyphs.div_ceil(cols);
        let cell_w = rasterizer.cell_width;
        let cell_h = rasterizer.cell_height;
        
        // Use larger cells in the atlas to allow oversized Nerd Font icons
        let atlas_cell_w = cell_w * 3;
        let atlas_cell_h = cell_h * 2;
        
        let atlas_width = cols * atlas_cell_w;
        let atlas_height = rows * atlas_cell_h;
        let mut pixels = vec![0u8; (atlas_width * atlas_height * 4) as usize];
        let mut glyphs = HashMap::new();
        let mut glyphs_bold = HashMap::new();

        let mut current_idx = 0;

        let mut rasterize_set = |rast: &FontRasterizer, map: &mut HashMap<char, GlyphMetrics>| {
            for &c in chars.iter() {
                let col = current_idx % cols;
                let row = current_idx / cols;
                let (metrics, bitmap) = rast.rasterize_char(c, px_size);

                let cell_x = col * atlas_cell_w;
                let cell_y = row * atlas_cell_h;

                let blit_w = (metrics.width as u32).min(atlas_cell_w);
                let blit_h = (metrics.height as u32).min(atlas_cell_h);

                for py in 0..blit_h {
                    for px in 0..blit_w {
                        let src_idx = (py * (metrics.width as u32) + px) as usize;
                        let dst_x = cell_x + px;
                        let dst_y = cell_y + py;
                        
                        if dst_x >= atlas_width || dst_y >= atlas_height { continue; }
                        
                        let dst_idx = ((dst_y * atlas_width + dst_x) * 4) as usize;
                        let coverage = bitmap.get(src_idx).copied().unwrap_or(0);
                        
                        pixels[dst_idx] = coverage;
                        pixels[dst_idx + 1] = coverage;
                        pixels[dst_idx + 2] = coverage;
                        pixels[dst_idx + 3] = coverage;
                    }
                }

                map.insert(c, GlyphMetrics {
                    u0: cell_x as f32 / atlas_width as f32,
                    v0: cell_y as f32 / atlas_height as f32,
                    u1: (cell_x + blit_w) as f32 / atlas_width as f32,
                    v1: (cell_y + blit_h) as f32 / atlas_height as f32,
                    width: blit_w,
                    height: blit_h,
                    bearing_y: metrics.ymin + metrics.height as i32,
                    bearing_x: metrics.xmin,
                });
                current_idx += 1;
            }
        };

        rasterize_set(rasterizer, &mut glyphs);
        if let Some(b_rast) = bold_rasterizer {
            rasterize_set(b_rast, &mut glyphs_bold);
        }

        Ok(Self { pixels, atlas_width, atlas_height, glyphs, glyphs_bold })
    }

    pub fn get(&self, c: char, is_bold: bool) -> Option<&GlyphMetrics> {
        if is_bold && !self.glyphs_bold.is_empty() {
            if let Some(m) = self.glyphs_bold.get(&c) {
                return Some(m);
            }
        }
        self.glyphs.get(&c).or_else(|| self.glyphs.get(&'?'))
    }
}
