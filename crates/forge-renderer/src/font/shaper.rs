use rustybuzz::{Face, UnicodeBuffer, shape};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextRunKey {
    pub text: String,
    pub is_bold: bool,
}

#[derive(Clone, Debug)]
pub struct ShapedGlyph {
    pub glyph_id: u16,
    pub x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

pub struct ShaperCache {
    cache: HashMap<TextRunKey, Vec<ShapedGlyph>>,
}

impl Default for ShaperCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ShaperCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn shape_run(&mut self, key: &TextRunKey, rasterizer: &super::rasterizer::FontRasterizer, bold_rasterizer: Option<&super::rasterizer::FontRasterizer>, px_size: f32) -> &[ShapedGlyph] {
        if !self.cache.contains_key(key) {
            let active_rasterizer = if key.is_bold {
                bold_rasterizer.unwrap_or(rasterizer)
            } else {
                rasterizer
            };

            let face = Face::from_slice(&active_rasterizer.bytes, 0).expect("Invalid font bytes");
            let mut buffer = UnicodeBuffer::new();
            buffer.push_str(&key.text);
            
            let glyph_buffer = shape(&face, &[], buffer);
            let infos = glyph_buffer.glyph_infos();
            let positions = glyph_buffer.glyph_positions();
            
            let mut shaped_glyphs = Vec::with_capacity(infos.len());
            
            // rustybuzz uses font units, we need to scale to pixels
            let units_per_em = face.units_per_em() as f32;
            let scale = px_size / units_per_em;
            
            for (info, pos) in infos.iter().zip(positions.iter()) {
                shaped_glyphs.push(ShapedGlyph {
                    glyph_id: info.glyph_id as u16,
                    x_advance: pos.x_advance as f32 * scale,
                    x_offset: pos.x_offset as f32 * scale,
                    y_offset: pos.y_offset as f32 * scale,
                });
            }
            
            self.cache.insert(key.clone(), shaped_glyphs);
        }
        self.cache.get(key).unwrap()
    }
}
