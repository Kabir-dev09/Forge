pub mod atlas;
pub mod builtin;
pub mod ligature;
pub mod rasterizer;
pub mod shaper;

use atlas::GlyphAtlas;
use rasterizer::FontRasterizer;

pub struct FontData {
    pub regular: FontRasterizer,
    pub bold: Option<FontRasterizer>,
    pub italic: Option<FontRasterizer>,
    pub bold_italic: Option<FontRasterizer>,
    pub fallbacks: Vec<FontRasterizer>,
    pub px_size: f32,
    pub atlas: GlyphAtlas,
}
