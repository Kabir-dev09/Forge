use super::rasterizer::{font_identity, FontRasterizer};
use forge_core::Result;
use std::collections::HashMap;

const ATLAS_COLUMNS: u32 = 128;
const DYNAMIC_GLYPH_SLOTS: u32 = 1024;
const ATLAS_CACHE_MAGIC: &[u8; 8] = b"FORGEFA1";
const ATLAS_CACHE_VERSION: u32 = 3;
const MAX_CACHED_ATLAS_BYTES: usize = 256 * 1024 * 1024;
const SCROLLING_OVERFLOW_GLYPHS: [char; 4] = ['', '', '', ''];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FontStyle {
    pub const fn from_flags(is_bold: bool, is_italic: bool) -> Self {
        match (is_bold, is_italic) {
            (false, false) => Self::Regular,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (true, true) => Self::BoldItalic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub c: char,
    pub style: FontStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapedGlyphKey {
    pub glyph_id: u16,
    pub style: FontStyle,
    pub font_hash: u64,
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

pub enum ShapedGlyphInsertResult {
    AlreadyPresent,
    AtlasFull,
    Missing,
    Inserted(Option<DynamicGlyphUpdate>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
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
    pub italic_font_hash: Option<u64>,
    pub bold_italic_font_hash: Option<u64>,
    pub px_size_bits: u32,
    pub fast_mode: bool,
}

impl GlyphAtlasDescriptor {
    pub fn dummy() -> Self {
        Self {
            regular_font_hash: 0,
            bold_font_hash: None,
            italic_font_hash: None,
            bold_italic_font_hash: None,
            px_size_bits: 0,
            fast_mode: true,
        }
    }

    pub fn new(
        rasterizer: &FontRasterizer,
        bold_rasterizer: Option<&FontRasterizer>,
        italic_rasterizer: Option<&FontRasterizer>,
        bold_italic_rasterizer: Option<&FontRasterizer>,
        px_size: f32,
        fast_mode: bool,
    ) -> Self {
        Self {
            regular_font_hash: rasterizer.identity(),
            bold_font_hash: bold_rasterizer.map(FontRasterizer::identity),
            italic_font_hash: italic_rasterizer.map(FontRasterizer::identity),
            bold_italic_font_hash: bold_italic_rasterizer.map(FontRasterizer::identity),
            px_size_bits: px_size.to_bits(),
            fast_mode,
        }
    }

    pub fn from_font_bytes(
        regular: &[u8],
        bold: Option<&[u8]>,
        italic: Option<&[u8]>,
        bold_italic: Option<&[u8]>,
        px_size: f32,
        fast_mode: bool,
    ) -> Self {
        Self {
            regular_font_hash: font_identity(regular),
            bold_font_hash: bold.map(font_identity),
            italic_font_hash: italic.map(font_identity),
            bold_italic_font_hash: bold_italic.map(font_identity),
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
    pub glyphs_italic: HashMap<char, GlyphMetrics>,
    pub glyphs_bold_italic: HashMap<char, GlyphMetrics>,
    pub shaped_glyphs: HashMap<ShapedGlyphKey, GlyphMetrics>,
    pub descriptor: GlyphAtlasDescriptor,
    pub font_cell_width: u32,
    pub font_cell_height: u32,
    pub font_baseline: u32,
    pub(crate) atlas_cell_width: u32,
    pub(crate) atlas_cell_height: u32,
    pub(crate) next_dynamic_slot: u32,
    pub(crate) total_slots: u32,
}

impl GlyphAtlas {
    pub fn descriptor_for(
        rasterizer: &FontRasterizer,
        bold_rasterizer: Option<&FontRasterizer>,
        italic_rasterizer: Option<&FontRasterizer>,
        bold_italic_rasterizer: Option<&FontRasterizer>,
        px_size: f32,
        fast_mode: bool,
    ) -> GlyphAtlasDescriptor {
        GlyphAtlasDescriptor::new(
            rasterizer,
            bold_rasterizer,
            italic_rasterizer,
            bold_italic_rasterizer,
            px_size,
            fast_mode,
        )
    }

    pub fn from_cache_bytes(bytes: &[u8], expected: &GlyphAtlasDescriptor) -> Result<Self> {
        if bytes.len() > MAX_CACHED_ATLAS_BYTES {
            return Err(forge_core::ForgeError::Other(
                "font atlas cache is too large".into(),
            ));
        }
        let mut reader = CacheReader::new(bytes);
        if reader.take(ATLAS_CACHE_MAGIC.len())? != ATLAS_CACHE_MAGIC {
            return Err(forge_core::ForgeError::Other(
                "invalid font atlas cache magic".into(),
            ));
        }
        if reader.u32()? != ATLAS_CACHE_VERSION {
            return Err(forge_core::ForgeError::Other(
                "unsupported font atlas cache version".into(),
            ));
        }
        let descriptor = GlyphAtlasDescriptor {
            regular_font_hash: reader.u64()?,
            bold_font_hash: reader.optional_u64()?,
            italic_font_hash: reader.optional_u64()?,
            bold_italic_font_hash: reader.optional_u64()?,
            px_size_bits: reader.u32()?,
            fast_mode: reader.u8()? != 0,
        };
        if &descriptor != expected {
            return Err(forge_core::ForgeError::Other(
                "font atlas cache does not match configured fonts".into(),
            ));
        }
        let atlas_width = reader.u32()?;
        let atlas_height = reader.u32()?;
        let atlas_cell_width = reader.u32()?;
        let atlas_cell_height = reader.u32()?;
        let font_cell_width = reader.u32()?;
        let font_cell_height = reader.u32()?;
        let font_baseline = reader.u32()?;
        let next_dynamic_slot = reader.u32()?;
        let total_slots = reader.u32()?;
        let glyphs = reader.glyph_map()?;
        let glyphs_bold = reader.glyph_map()?;
        let glyphs_italic = reader.glyph_map()?;
        let glyphs_bold_italic = reader.glyph_map()?;
        let pixel_len = reader.u64()? as usize;
        let expected_pixels = (atlas_width as usize)
            .checked_mul(atlas_height as usize)
            .and_then(|value| value.checked_mul(4))
            .ok_or_else(|| {
                forge_core::ForgeError::Other("font atlas dimensions overflow".into())
            })?;
        if pixel_len != expected_pixels || pixel_len > MAX_CACHED_ATLAS_BYTES {
            return Err(forge_core::ForgeError::Other(
                "invalid font atlas pixel length".into(),
            ));
        }
        let pixels = reader.take(pixel_len)?.to_vec();
        if !reader.is_empty() || next_dynamic_slot > total_slots {
            return Err(forge_core::ForgeError::Other(
                "invalid trailing font atlas data".into(),
            ));
        }
        Ok(Self {
            pixels,
            atlas_width,
            atlas_height,
            glyphs,
            glyphs_bold,
            glyphs_italic,
            glyphs_bold_italic,
            shaped_glyphs: HashMap::new(),
            descriptor,
            font_cell_width,
            font_cell_height,
            font_baseline,
            atlas_cell_width,
            atlas_cell_height,
            next_dynamic_slot,
            total_slots,
        })
    }

    pub fn to_cache_bytes(&self) -> Vec<u8> {
        let map_bytes = (self.glyphs.len()
            + self.glyphs_bold.len()
            + self.glyphs_italic.len()
            + self.glyphs_bold_italic.len())
            * 40;
        let mut out = Vec::with_capacity(128 + map_bytes + self.pixels.len());
        out.extend_from_slice(ATLAS_CACHE_MAGIC);
        push_u32(&mut out, ATLAS_CACHE_VERSION);
        push_u64(&mut out, self.descriptor.regular_font_hash);
        push_optional_u64(&mut out, self.descriptor.bold_font_hash);
        push_optional_u64(&mut out, self.descriptor.italic_font_hash);
        push_optional_u64(&mut out, self.descriptor.bold_italic_font_hash);
        push_u32(&mut out, self.descriptor.px_size_bits);
        out.push(u8::from(self.descriptor.fast_mode));
        for value in [
            self.atlas_width,
            self.atlas_height,
            self.atlas_cell_width,
            self.atlas_cell_height,
            self.font_cell_width,
            self.font_cell_height,
            self.font_baseline,
            self.next_dynamic_slot,
            self.total_slots,
        ] {
            push_u32(&mut out, value);
        }
        push_glyph_map(&mut out, &self.glyphs);
        push_glyph_map(&mut out, &self.glyphs_bold);
        push_glyph_map(&mut out, &self.glyphs_italic);
        push_glyph_map(&mut out, &self.glyphs_bold_italic);
        push_u64(&mut out, self.pixels.len() as u64);
        out.extend_from_slice(&self.pixels);
        out
    }

    pub fn dummy_for_bench() -> Self {
        let mut glyphs = HashMap::new();
        for c in 0x20_u8..=0x7e {
            glyphs.insert(
                c as char,
                GlyphMetrics {
                    u0: 0.0,
                    v0: 0.0,
                    u1: 1.0,
                    v1: 1.0,
                    width: 8,
                    height: 16,
                    bearing_y: 14,
                    bearing_x: 0,
                },
            );
        }
        Self {
            pixels: vec![255; 4],
            atlas_width: 1,
            atlas_height: 1,
            glyphs,
            glyphs_bold: HashMap::new(),
            glyphs_italic: HashMap::new(),
            glyphs_bold_italic: HashMap::new(),
            shaped_glyphs: HashMap::new(),
            descriptor: GlyphAtlasDescriptor::dummy(),
            font_cell_width: 8,
            font_cell_height: 16,
            font_baseline: 14,
            atlas_cell_width: 8,
            atlas_cell_height: 16,
            next_dynamic_slot: 0,
            total_slots: 100,
        }
    }
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_optional_u64(out: &mut Vec<u8>, value: Option<u64>) {
    out.push(u8::from(value.is_some()));
    push_u64(out, value.unwrap_or(0));
}

fn push_glyph_map(out: &mut Vec<u8>, map: &HashMap<char, GlyphMetrics>) {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_unstable_by_key(|(character, _)| **character as u32);
    push_u32(out, entries.len() as u32);
    for (character, metrics) in entries {
        push_u32(out, *character as u32);
        for value in [metrics.u0, metrics.v0, metrics.u1, metrics.v1] {
            push_u32(out, value.to_bits());
        }
        push_u32(out, metrics.width);
        push_u32(out, metrics.height);
        push_u32(out, metrics.bearing_y as u32);
        push_u32(out, metrics.bearing_x as u32);
    }
}

struct CacheReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> CacheReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.offset.checked_add(len).ok_or_else(|| {
            forge_core::ForgeError::Other("font atlas cache offset overflow".into())
        })?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| forge_core::ForgeError::Other("truncated font atlas cache".into()))?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn optional_u64(&mut self) -> Result<Option<u64>> {
        let present = self.u8()? != 0;
        let value = self.u64()?;
        Ok(present.then_some(value))
    }

    fn glyph_map(&mut self) -> Result<HashMap<char, GlyphMetrics>> {
        let count = self.u32()? as usize;
        if count > 1_000_000 {
            return Err(forge_core::ForgeError::Other(
                "font atlas glyph count is invalid".into(),
            ));
        }
        let mut map = HashMap::with_capacity(count);
        for _ in 0..count {
            let character = char::from_u32(self.u32()?).ok_or_else(|| {
                forge_core::ForgeError::Other("font atlas contains an invalid character".into())
            })?;
            let metrics = GlyphMetrics {
                u0: f32::from_bits(self.u32()?),
                v0: f32::from_bits(self.u32()?),
                u1: f32::from_bits(self.u32()?),
                v1: f32::from_bits(self.u32()?),
                width: self.u32()?,
                height: self.u32()?,
                bearing_y: self.u32()? as i32,
                bearing_x: self.u32()? as i32,
            };
            map.insert(character, metrics);
        }
        Ok(map)
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
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
        chars.extend(SCROLLING_OVERFLOW_GLYPHS);
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
        italic_rasterizer: Option<&FontRasterizer>,
        bold_italic_rasterizer: Option<&FontRasterizer>,
        px_size: f32,
        fast_mode: bool,
    ) -> Result<Self> {
        let build_start = std::time::Instant::now();
        let descriptor = GlyphAtlasDescriptor::new(
            rasterizer,
            bold_rasterizer,
            italic_rasterizer,
            bold_italic_rasterizer,
            px_size,
            fast_mode,
        );
        // Fast mode stays ASCII-only for immediate startup. Full mode keeps a
        // compact common set and relies on procedural drawing/dynamic insertion
        // for box, block, braille, PUA, emoji, and less common scripts.
        let chars = static_atlas_chars(fast_mode);

        // Fixed column layout keeps glyph UVs stable and leaves a bounded
        // dynamic area for glyphs discovered after startup.
        let cols = ATLAS_COLUMNS;
        let font_count = 1
            + u32::from(bold_rasterizer.is_some())
            + u32::from(italic_rasterizer.is_some())
            + u32::from(bold_italic_rasterizer.is_some());
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
        let mut glyphs_italic = HashMap::new();
        let mut glyphs_bold_italic = HashMap::new();

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
        if let Some(i_rast) = italic_rasterizer {
            rasterize_set(i_rast, &mut glyphs_italic);
        }
        if let Some(bi_rast) = bold_italic_rasterizer {
            rasterize_set(bi_rast, &mut glyphs_bold_italic);
        }

        tracing::debug!(
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
            glyphs_italic,
            glyphs_bold_italic,
            shaped_glyphs: HashMap::new(),
            descriptor,
            font_cell_width: rasterizer.cell_width,
            font_cell_height: rasterizer.cell_height,
            font_baseline: rasterizer.baseline,
            atlas_cell_width: atlas_cell_w,
            atlas_cell_height: atlas_cell_h,
            next_dynamic_slot: static_glyphs,
            total_slots,
        })
    }

    pub fn insert_shaped_glyph(
        &mut self,
        key: ShapedGlyphKey,
        rasterizer: &FontRasterizer,
        bold_rasterizer: Option<&FontRasterizer>,
        italic_rasterizer: Option<&FontRasterizer>,
        bold_italic_rasterizer: Option<&FontRasterizer>,
        px_size: f32,
    ) -> ShapedGlyphInsertResult {
        if self.shaped_glyphs.contains_key(&key) {
            return ShapedGlyphInsertResult::AlreadyPresent;
        }

        if self.next_dynamic_slot >= self.total_slots {
            return ShapedGlyphInsertResult::AtlasFull;
        }

        let active_rasterizer = match key.style {
            FontStyle::Regular if rasterizer.identity() == key.font_hash => rasterizer,
            FontStyle::Bold
                if bold_rasterizer.is_some_and(|r| r.identity() == key.font_hash) =>
            {
                bold_rasterizer.unwrap()
            }
            FontStyle::Italic
                if italic_rasterizer.is_some_and(|r| r.identity() == key.font_hash) =>
            {
                italic_rasterizer.unwrap()
            }
            FontStyle::BoldItalic
                if bold_italic_rasterizer.is_some_and(|r| r.identity() == key.font_hash) =>
            {
                bold_italic_rasterizer.unwrap()
            }
            _ => {
                return ShapedGlyphInsertResult::Missing;
            }
        };

        let slot = self.next_dynamic_slot;
        self.next_dynamic_slot += 1;

        let col = slot % ATLAS_COLUMNS;
        let row = slot / ATLAS_COLUMNS;
        let cell_x = col * self.atlas_cell_width;
        let cell_y = row * self.atlas_cell_height;
        let (metrics, bitmap) = active_rasterizer.rasterize_glyph_id(key.glyph_id, px_size);
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
        self.shaped_glyphs.insert(key, glyph_metrics);

        if blit_w == 0 || blit_h == 0 {
            return ShapedGlyphInsertResult::Inserted(None);
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

        ShapedGlyphInsertResult::Inserted(Some(DynamicGlyphUpdate {
            x: cell_x,
            y: cell_y,
            width: blit_w,
            height: blit_h,
            pixels,
        }))
    }

    pub fn get_shaped(&self, key: ShapedGlyphKey) -> Option<&GlyphMetrics> {
        self.shaped_glyphs.get(&key)
    }

    // Keep the borrowed font sources explicit; this is a hot path and a wrapper
    // object would add plumbing without reducing state or improving ownership.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_dynamic_glyph(
        &mut self,
        key: GlyphKey,
        rasterizer: &FontRasterizer,
        bold_rasterizer: Option<&FontRasterizer>,
        italic_rasterizer: Option<&FontRasterizer>,
        bold_italic_rasterizer: Option<&FontRasterizer>,
        fallback_rasterizers: &[FontRasterizer],
        px_size: f32,
    ) -> DynamicGlyphInsertResult {
        if self.get_exact(key.c, key.style).is_some() {
            return DynamicGlyphInsertResult::AlreadyPresent;
        }

        if self.next_dynamic_slot >= self.total_slots {
            return DynamicGlyphInsertResult::AtlasFull;
        }

        let styled = match key.style {
            FontStyle::Regular => None,
            FontStyle::Bold => bold_rasterizer,
            FontStyle::Italic => italic_rasterizer,
            FontStyle::BoldItalic => bold_italic_rasterizer,
        };
        let active_rasterizer = if styled.is_some_and(|r| r.has_glyph(key.c)) {
            styled.unwrap()
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

        match key.style {
            FontStyle::Bold if !self.glyphs_bold.is_empty() => {
                self.glyphs_bold.insert(key.c, glyph_metrics);
            }
            FontStyle::Italic if !self.glyphs_italic.is_empty() => {
                self.glyphs_italic.insert(key.c, glyph_metrics);
            }
            FontStyle::BoldItalic if !self.glyphs_bold_italic.is_empty() => {
                self.glyphs_bold_italic.insert(key.c, glyph_metrics);
            }
            _ => {
                self.glyphs.insert(key.c, glyph_metrics);
            }
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

    pub fn get_exact(&self, c: char, style: FontStyle) -> Option<&GlyphMetrics> {
        let styled = match style {
            FontStyle::Regular => None,
            FontStyle::Bold => self.glyphs_bold.get(&c),
            FontStyle::Italic => self.glyphs_italic.get(&c),
            FontStyle::BoldItalic => self.glyphs_bold_italic.get(&c),
        };
        styled.or_else(|| self.glyphs.get(&c))
    }

    pub fn fallback(&self) -> Option<&GlyphMetrics> {
        self.glyphs.get(&'?')
    }

    pub fn get(&self, c: char, style: FontStyle) -> Option<&GlyphMetrics> {
        self.get_exact(c, style).or_else(|| self.fallback())
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
        assert!(SCROLLING_OVERFLOW_GLYPHS
            .iter()
            .all(|glyph| chars.contains(glyph)));
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
        let mut atlas = GlyphAtlas::build(&rasterizer, None, None, None, 16.0, true).unwrap();
        let key = GlyphKey {
            c: 'Ω',
            style: FontStyle::Regular,
        };

        assert!(atlas.get_exact(key.c, key.style).is_none());
        let update = match atlas.insert_dynamic_glyph(key, &rasterizer, None, None, None, &[], 16.0)
        {
            DynamicGlyphInsertResult::Inserted(Some(update)) => update,
            _ => panic!("visible glyph should produce an atlas update"),
        };

        assert!(atlas.get_exact(key.c, key.style).is_some());
        assert!(update.width > 0);
        assert!(update.height > 0);
        assert_eq!(
            update.pixels.len(),
            (update.width * update.height * 4) as usize
        );
    }

    #[test]
    fn shaped_glyph_insert_uses_glyph_id_key() {
        let rasterizer = test_rasterizer();
        let mut atlas = GlyphAtlas::build(&rasterizer, None, None, None, 16.0, true).unwrap();
        let key = ShapedGlyphKey {
            glyph_id: rasterizer.glyph_index('A'),
            style: FontStyle::Regular,
            font_hash: rasterizer.identity(),
        };

        assert!(atlas.get_shaped(key).is_none());
        let update = match atlas.insert_shaped_glyph(key, &rasterizer, None, None, None, 16.0) {
            ShapedGlyphInsertResult::Inserted(Some(update)) => update,
            _ => panic!("visible shaped glyph should produce an atlas update"),
        };

        assert!(atlas.get_shaped(key).is_some());
        assert!(update.width > 0);
        assert!(update.height > 0);
    }

    #[test]
    fn dynamic_glyph_insert_is_idempotent_for_existing_glyph() {
        let rasterizer = test_rasterizer();
        let mut atlas = GlyphAtlas::build(&rasterizer, None, None, None, 16.0, true).unwrap();
        let key = GlyphKey {
            c: 'A',
            style: FontStyle::Regular,
        };

        assert!(atlas.get_exact(key.c, key.style).is_some());
        assert!(matches!(
            atlas.insert_dynamic_glyph(key, &rasterizer, None, None, None, &[], 16.0),
            DynamicGlyphInsertResult::AlreadyPresent
        ));
    }

    #[test]
    fn dynamic_glyph_insert_reports_full_atlas() {
        let rasterizer = test_rasterizer();
        let mut atlas = GlyphAtlas::build(&rasterizer, None, None, None, 16.0, true).unwrap();
        atlas.next_dynamic_slot = atlas.total_slots;

        assert_eq!(atlas.dynamic_slots_remaining(), 0);
        assert!(matches!(
            atlas.insert_dynamic_glyph(
                GlyphKey {
                    c: 'Ω',
                    style: FontStyle::Regular,
                },
                &rasterizer,
                None,
                None,
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
        let mut atlas = GlyphAtlas::build(&rasterizer, None, None, None, 16.0, true).unwrap();

        let result = atlas.insert_dynamic_glyph(
            GlyphKey {
                c: '★',
                style: FontStyle::Regular,
            },
            &rasterizer,
            None,
            None,
            None,
            &[fallback],
            16.0,
        );

        assert!(matches!(
            result,
            DynamicGlyphInsertResult::Inserted(Some(_))
        ));
        assert!(atlas.get_exact('★', FontStyle::Regular).is_some());
    }

    #[test]
    fn dynamic_glyph_insert_reports_missing_when_no_font_has_glyph() {
        let rasterizer = test_rasterizer();
        let mut atlas = GlyphAtlas::build(&rasterizer, None, None, None, 16.0, true).unwrap();

        assert!(matches!(
            atlas.insert_dynamic_glyph(
                GlyphKey {
                    c: '★',
                    style: FontStyle::Regular,
                },
                &rasterizer,
                None,
                None,
                None,
                &[],
                16.0,
            ),
            DynamicGlyphInsertResult::Missing
        ));
        assert!(atlas.get_exact('★', FontStyle::Regular).is_none());
    }

    #[test]
    fn cache_round_trip_preserves_all_style_maps_and_pixels() {
        let regular = test_rasterizer();
        let italic = FontRasterizer::from_bytes(super::super::builtin::italic(), 16.0).unwrap();
        let atlas = GlyphAtlas::build(&regular, None, Some(&italic), None, 16.0, true).unwrap();
        let bytes = atlas.to_cache_bytes();
        let restored = GlyphAtlas::from_cache_bytes(&bytes, &atlas.descriptor).unwrap();

        assert_eq!(restored.pixels, atlas.pixels);
        assert_eq!(restored.glyphs, atlas.glyphs);
        assert_eq!(restored.glyphs_italic, atlas.glyphs_italic);
        assert_eq!(restored.atlas_width, atlas.atlas_width);
        assert_eq!(restored.next_dynamic_slot, atlas.next_dynamic_slot);
    }

    #[test]
    fn cache_rejects_a_different_font_size() {
        let regular = test_rasterizer();
        let atlas = GlyphAtlas::build(&regular, None, None, None, 16.0, true).unwrap();
        let mut other = atlas.descriptor.clone();
        other.px_size_bits = 15.0_f32.to_bits();

        assert!(GlyphAtlas::from_cache_bytes(&atlas.to_cache_bytes(), &other).is_err());
    }

    #[test]
    fn italic_style_uses_real_italic_atlas_metrics() {
        let regular = test_rasterizer();
        let italic = FontRasterizer::from_bytes(super::super::builtin::italic(), 16.0).unwrap();
        let atlas = GlyphAtlas::build(&regular, None, Some(&italic), None, 16.0, true).unwrap();

        assert_ne!(
            atlas.descriptor.regular_font_hash,
            atlas.descriptor.italic_font_hash.unwrap()
        );
        assert_eq!(
            atlas.get_exact('A', FontStyle::Italic),
            atlas.glyphs_italic.get(&'A')
        );
    }
}
