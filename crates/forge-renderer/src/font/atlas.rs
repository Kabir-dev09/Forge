use super::rasterizer::FontRasterizer;
use forge_core::Result;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

const ATLAS_COLUMNS: u32 = 128;
const DYNAMIC_GLYPH_SLOTS: u32 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub c: char,
    pub is_bold: bool,
}

pub struct DynamicGlyphUpdate {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub enum DynamicGlyphInsertResult {
    AlreadyPresent,
    AtlasFull,
    Missing,
    Inserted(Option<DynamicGlyphUpdate>),
}

#[derive(Debug, Clone, Copy)]
pub struct GlyphMetrics {
    pub u0: f32,
    pub v0: f32, // Top-left UV
    pub u1: f32,
    pub v1: f32,        // Bottom-right UV
    pub width: u32,     // Actual glyph pixel width
    pub height: u32,    // Actual glyph pixel height
    pub bearing_y: i32, // Distance from baseline to top of glyph
    pub bearing_x: i32, // Distance from cell left to glyph left
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphAtlasDescriptor {
    pub regular_font_hash: u64,
    pub bold_font_hash: Option<u64>,
    pub px_size_bits: u32,
    pub fast_mode: bool,
}

impl GlyphAtlasDescriptor {
    pub fn dummy() -> Self {
        Self {
            regular_font_hash: 0,
            bold_font_hash: None,
            px_size_bits: 0,
            fast_mode: true,
        }
    }

    pub fn new(
        rasterizer: &FontRasterizer,
        bold_rasterizer: Option<&FontRasterizer>,
        px_size: f32,
        fast_mode: bool,
    ) -> Self {
        Self {
            regular_font_hash: font_hash(&rasterizer.bytes),
            bold_font_hash: bold_rasterizer.map(|rasterizer| font_hash(&rasterizer.bytes)),
            px_size_bits: px_size.to_bits(),
            fast_mode,
        }
    }
}

pub struct GlyphAtlas {
    pub pixels: Vec<u8>, // RGBA (R=G=B=255, A=coverage) to simplify Vulkan format matching
    pub atlas_width: u32,
    pub atlas_height: u32,
    pub glyphs: HashMap<char, GlyphMetrics>,
    pub glyphs_bold: HashMap<char, GlyphMetrics>,
    pub descriptor: GlyphAtlasDescriptor,
    pub(crate) atlas_cell_width: u32,
    pub(crate) atlas_cell_height: u32,
    pub(crate) next_dynamic_slot: u32,
    pub(crate) total_slots: u32,
}

fn font_hash(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn push_char_range(chars: &mut Vec<char>, start: u32, end: u32) {
    chars.extend((start..=end).filter_map(std::char::from_u32));
}

fn static_atlas_chars(fast_mode: bool) -> Vec<char> {
    let mut chars = Vec::new();
    push_char_range(&mut chars, 0x20, 0x7E);

    if !fast_mode {
        push_char_range(&mut chars, 0x00A0, 0x017F); // Latin-1 Supplement + Latin Extended-A
        push_char_range(&mut chars, 0x2000, 0x206F); // General Punctuation
        push_char_range(&mut chars, 0x20A0, 0x20CF); // Currency Symbols
        push_char_range(&mut chars, 0x2100, 0x214F); // Letterlike Symbols
        push_char_range(&mut chars, 0x2190, 0x21FF); // Arrows
    }

    chars
}

impl GlyphAtlas {
    pub fn clear_pixels(&mut self) {
        self.pixels = Vec::new();
        self.pixels.shrink_to_fit();
    }

    pub fn dynamic_slots_used(&self) -> u32 {
        self.next_dynamic_slot.min(self.total_slots)
    }

    pub fn dynamic_slots_remaining(&self) -> u32 {
        self.total_slots.saturating_sub(self.next_dynamic_slot)
    }

    pub fn build(
        rasterizer: &FontRasterizer,
        bold_rasterizer: Option<&FontRasterizer>,
        px_size: f32,
        fast_mode: bool,
    ) -> Result<Self> {
        let build_start = std::time::Instant::now();
        let descriptor = GlyphAtlasDescriptor::new(rasterizer, bold_rasterizer, px_size, fast_mode);
        // Fast mode stays ASCII-only for immediate startup. Full mode keeps a
        // compact common set and relies on procedural drawing/dynamic insertion
        // for box, block, braille, PUA, emoji, and less common scripts.
        let chars = static_atlas_chars(fast_mode);

        // Fixed column layout keeps glyph UVs stable and leaves a bounded
        // dynamic area for glyphs discovered after startup.
        let cols = ATLAS_COLUMNS;
        let font_count = if bold_rasterizer.is_some() { 2 } else { 1 };
        let static_glyphs = chars.len() as u32 * font_count;
        let total_slots = static_glyphs + DYNAMIC_GLYPH_SLOTS;
        let rows = total_slots.div_ceil(cols);
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

                        if dst_x >= atlas_width || dst_y >= atlas_height {
                            continue;
                        }

                        let dst_idx = ((dst_y * atlas_width + dst_x) * 4) as usize;
                        let coverage = bitmap.get(src_idx).copied().unwrap_or(0);

                        pixels[dst_idx] = coverage;
                        pixels[dst_idx + 1] = coverage;
                        pixels[dst_idx + 2] = coverage;
                        pixels[dst_idx + 3] = coverage;
                    }
                }

                map.insert(
                    c,
                    GlyphMetrics {
                        u0: cell_x as f32 / atlas_width as f32,
                        v0: cell_y as f32 / atlas_height as f32,
                        u1: (cell_x + blit_w) as f32 / atlas_width as f32,
                        v1: (cell_y + blit_h) as f32 / atlas_height as f32,
                        width: blit_w,
                        height: blit_h,
                        bearing_y: metrics.ymin + metrics.height as i32,
                        bearing_x: metrics.xmin,
                    },
                );
                current_idx += 1;
            }
        };

        rasterize_set(rasterizer, &mut glyphs);
        if let Some(b_rast) = bold_rasterizer {
            rasterize_set(b_rast, &mut glyphs_bold);
        }

        tracing::info!(
            "[PROFILER] GlyphAtlas::build took: {:?} (glyphs={}, bold_glyphs={}, fast_mode={})",
            build_start.elapsed(),
            glyphs.len(),
            glyphs_bold.len(),
            fast_mode
        );

        Ok(Self {
            pixels,
            atlas_width,
            atlas_height,
            glyphs,
            glyphs_bold,
            descriptor,
            atlas_cell_width: atlas_cell_w,
            atlas_cell_height: atlas_cell_h,
            next_dynamic_slot: static_glyphs,
            total_slots,
        })
    }

    pub fn insert_dynamic_glyph(
        &mut self,
        key: GlyphKey,
        rasterizer: &FontRasterizer,
        bold_rasterizer: Option<&FontRasterizer>,
        fallback_rasterizers: &[FontRasterizer],
        px_size: f32,
    ) -> DynamicGlyphInsertResult {
        if self.get_exact(key.c, key.is_bold).is_some() {
            return DynamicGlyphInsertResult::AlreadyPresent;
        }

        if self.next_dynamic_slot >= self.total_slots {
            return DynamicGlyphInsertResult::AtlasFull;
        }

        let active_rasterizer = if key.is_bold
            && bold_rasterizer.is_some_and(|rasterizer| rasterizer.has_glyph(key.c))
        {
            bold_rasterizer.unwrap()
        } else if rasterizer.has_glyph(key.c) {
            rasterizer
        } else if let Some(fallback) = fallback_rasterizers
            .iter()
            .find(|fallback| fallback.has_glyph(key.c))
        {
            fallback
        } else {
            return DynamicGlyphInsertResult::Missing;
        };
        let slot = self.next_dynamic_slot;
        self.next_dynamic_slot += 1;

        let col = slot % ATLAS_COLUMNS;
        let row = slot / ATLAS_COLUMNS;
        let cell_x = col * self.atlas_cell_width;
        let cell_y = row * self.atlas_cell_height;
        let (metrics, bitmap) = active_rasterizer.rasterize_char(key.c, px_size);
        let blit_w = (metrics.width as u32).min(self.atlas_cell_width);
        let blit_h = (metrics.height as u32).min(self.atlas_cell_height);

        let glyph_metrics = GlyphMetrics {
            u0: cell_x as f32 / self.atlas_width as f32,
            v0: cell_y as f32 / self.atlas_height as f32,
            u1: (cell_x + blit_w) as f32 / self.atlas_width as f32,
            v1: (cell_y + blit_h) as f32 / self.atlas_height as f32,
            width: blit_w,
            height: blit_h,
            bearing_y: metrics.ymin + metrics.height as i32,
            bearing_x: metrics.xmin,
        };

        if key.is_bold && !self.glyphs_bold.is_empty() {
            self.glyphs_bold.insert(key.c, glyph_metrics);
        } else {
            self.glyphs.insert(key.c, glyph_metrics);
        }

        if blit_w == 0 || blit_h == 0 {
            return DynamicGlyphInsertResult::Inserted(None);
        }

        let mut pixels = vec![0u8; (blit_w * blit_h * 4) as usize];
        for py in 0..blit_h {
            for px in 0..blit_w {
                let src_idx = (py * metrics.width as u32 + px) as usize;
                let dst_idx = ((py * blit_w + px) * 4) as usize;
                let coverage = bitmap.get(src_idx).copied().unwrap_or(0);
                pixels[dst_idx] = coverage;
                pixels[dst_idx + 1] = coverage;
                pixels[dst_idx + 2] = coverage;
                pixels[dst_idx + 3] = coverage;
            }
        }

        DynamicGlyphInsertResult::Inserted(Some(DynamicGlyphUpdate {
            x: cell_x,
            y: cell_y,
            width: blit_w,
            height: blit_h,
            pixels,
        }))
    }

    pub fn get_exact(&self, c: char, is_bold: bool) -> Option<&GlyphMetrics> {
        if is_bold && !self.glyphs_bold.is_empty() {
            if let Some(m) = self.glyphs_bold.get(&c) {
                return Some(m);
            }
        }
        self.glyphs.get(&c)
    }

    pub fn fallback(&self) -> Option<&GlyphMetrics> {
        self.glyphs.get(&'?')
    }

    pub fn get(&self, c: char, is_bold: bool) -> Option<&GlyphMetrics> {
        self.get_exact(c, is_bold).or_else(|| self.fallback())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rasterizer() -> FontRasterizer {
        FontRasterizer::from_bytes(
            include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf"),
            16.0,
        )
        .expect("bundled test font should load")
    }

    fn optional_star_fallback_rasterizer() -> Option<FontRasterizer> {
        std::fs::read("/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf")
            .ok()
            .and_then(|bytes| FontRasterizer::from_bytes(&bytes, 16.0).ok())
            .filter(|rasterizer| rasterizer.has_glyph('★'))
    }

    #[test]
    fn fast_static_atlas_chars_are_ascii_only() {
        let chars = static_atlas_chars(true);

        assert_eq!(chars.len(), (0x7E - 0x20 + 1) as usize);
        assert!(chars.contains(&'A'));
        assert!(!chars.contains(&'Ω'));
        assert!(!chars.contains(&'─'));
    }

    #[test]
    fn full_static_atlas_keeps_common_text_and_excludes_dynamic_ranges() {
        let chars = static_atlas_chars(false);

        assert!(chars.contains(&'A'));
        assert!(chars.contains(&'é'));
        assert!(chars.contains(&'→'));
        assert!(chars.contains(&'€'));
        assert!(!chars.contains(&'Ω'));
        assert!(!chars.contains(&'─'));
        assert!(!chars.contains(&'█'));
        assert!(!chars.contains(&'⣿'));
        assert!(!chars.contains(&'\u{E0B0}'));
        assert!(!chars.contains(&'😀'));
    }

    #[test]
    fn dynamic_glyph_insert_adds_metrics_and_update_region() {
        let rasterizer = test_rasterizer();
        let mut atlas = GlyphAtlas::build(&rasterizer, None, 16.0, true).unwrap();
        let key = GlyphKey {
            c: 'Ω',
            is_bold: false,
        };

        assert!(atlas.get_exact(key.c, key.is_bold).is_none());
        let update = match atlas.insert_dynamic_glyph(key, &rasterizer, None, &[], 16.0) {
            DynamicGlyphInsertResult::Inserted(Some(update)) => update,
            _ => panic!("visible glyph should produce an atlas update"),
        };

        assert!(atlas.get_exact(key.c, key.is_bold).is_some());
        assert!(update.width > 0);
        assert!(update.height > 0);
        assert_eq!(
            update.pixels.len(),
            (update.width * update.height * 4) as usize
        );
    }

    #[test]
    fn dynamic_glyph_insert_is_idempotent_for_existing_glyph() {
        let rasterizer = test_rasterizer();
        let mut atlas = GlyphAtlas::build(&rasterizer, None, 16.0, true).unwrap();
        let key = GlyphKey {
            c: 'A',
            is_bold: false,
        };

        assert!(atlas.get_exact(key.c, key.is_bold).is_some());
        assert!(matches!(
            atlas.insert_dynamic_glyph(key, &rasterizer, None, &[], 16.0),
            DynamicGlyphInsertResult::AlreadyPresent
        ));
    }

    #[test]
    fn dynamic_glyph_insert_reports_full_atlas() {
        let rasterizer = test_rasterizer();
        let mut atlas = GlyphAtlas::build(&rasterizer, None, 16.0, true).unwrap();
        atlas.next_dynamic_slot = atlas.total_slots;

        assert_eq!(atlas.dynamic_slots_remaining(), 0);
        assert!(matches!(
            atlas.insert_dynamic_glyph(
                GlyphKey {
                    c: 'Ω',
                    is_bold: false,
                },
                &rasterizer,
                None,
                &[],
                16.0,
            ),
            DynamicGlyphInsertResult::AtlasFull
        ));
    }

    #[test]
    fn dynamic_glyph_insert_uses_fallback_font_for_star() {
        let rasterizer = test_rasterizer();
        assert!(!rasterizer.has_glyph('★'));
        let Some(fallback) = optional_star_fallback_rasterizer() else {
            return;
        };
        let mut atlas = GlyphAtlas::build(&rasterizer, None, 16.0, true).unwrap();

        let result = atlas.insert_dynamic_glyph(
            GlyphKey {
                c: '★',
                is_bold: false,
            },
            &rasterizer,
            None,
            &[fallback],
            16.0,
        );

        assert!(matches!(
            result,
            DynamicGlyphInsertResult::Inserted(Some(_))
        ));
        assert!(atlas.get_exact('★', false).is_some());
    }

    #[test]
    fn dynamic_glyph_insert_reports_missing_when_no_font_has_glyph() {
        let rasterizer = test_rasterizer();
        let mut atlas = GlyphAtlas::build(&rasterizer, None, 16.0, true).unwrap();

        assert!(matches!(
            atlas.insert_dynamic_glyph(
                GlyphKey {
                    c: '★',
                    is_bold: false,
                },
                &rasterizer,
                None,
                &[],
                16.0,
            ),
            DynamicGlyphInsertResult::Missing
        ));
        assert!(atlas.get_exact('★', false).is_none());
    }
}
