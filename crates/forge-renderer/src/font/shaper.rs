use super::ligature::LigatureStyleKey;
use super::rasterizer::FontRasterizer;
use forge_core::config_registry::LigatureMode;
use rustybuzz::{shape, Face, Feature, UnicodeBuffer};
use std::collections::{HashMap, VecDeque};
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextRunKey {
    pub text: String,
    pub font_hash: u64,
    pub px_size_bits: u32,
    pub style: LigatureStyleKey,
    pub features: Vec<String>,
    pub mode: LigatureMode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShapedGlyph {
    pub glyph_id: u16,
    pub x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    pub cluster: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShapedRun {
    pub glyphs: Vec<ShapedGlyph>,
    pub char_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ShaperCacheEntry {
    Positive(ShapedRun),
    Negative,
}

pub struct ShaperCache {
    cache: HashMap<TextRunKey, ShaperCacheEntry>,
    insertion_order: VecDeque<TextRunKey>,
    max_entries: usize,
}

impl Default for ShaperCache {
    fn default() -> Self {
        Self::new(4096)
    }
}

impl ShaperCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            cache: HashMap::new(),
            insertion_order: VecDeque::new(),
            max_entries: max_entries.clamp(64, 65_536),
        }
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.insertion_order.clear();
    }

    pub fn set_max_entries(&mut self, max_entries: usize) {
        let max_entries = max_entries.clamp(64, 65_536);
        self.max_entries = max_entries;
        while self.cache.len() > self.max_entries {
            self.evict_oldest();
        }
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn shape_run(
        &mut self,
        key: TextRunKey,
        rasterizer: &FontRasterizer,
        bold_rasterizer: Option<&FontRasterizer>,
        italic_rasterizer: Option<&FontRasterizer>,
        bold_italic_rasterizer: Option<&FontRasterizer>,
        px_size: f32,
    ) -> &ShaperCacheEntry {
        if self.cache.contains_key(&key) {
            return self.cache.get(&key).expect("cache key just checked");
        }

        if self.cache.len() >= self.max_entries {
            self.evict_oldest();
        }

        let entry = shape_uncached(
            &key,
            rasterizer,
            bold_rasterizer,
            italic_rasterizer,
            bold_italic_rasterizer,
            px_size,
        )
        .unwrap_or(ShaperCacheEntry::Negative);
        self.insertion_order.push_back(key.clone());
        self.cache.insert(key.clone(), entry);
        self.cache.get(&key).expect("cache entry just inserted")
    }

    fn evict_oldest(&mut self) {
        while let Some(oldest) = self.insertion_order.pop_front() {
            if self.cache.remove(&oldest).is_some() {
                break;
            }
        }
    }

    #[cfg(test)]
    fn insert_for_test(&mut self, key: TextRunKey, entry: ShaperCacheEntry) {
        if self.cache.len() >= self.max_entries {
            self.evict_oldest();
        }
        self.insertion_order.push_back(key.clone());
        self.cache.insert(key, entry);
    }
}

fn shape_uncached(
    key: &TextRunKey,
    rasterizer: &FontRasterizer,
    bold_rasterizer: Option<&FontRasterizer>,
    italic_rasterizer: Option<&FontRasterizer>,
    bold_italic_rasterizer: Option<&FontRasterizer>,
    px_size: f32,
) -> Option<ShaperCacheEntry> {
    let active_rasterizer = match (key.style.is_bold(), key.style.is_italic()) {
        (false, false) => rasterizer,
        (true, false) => bold_rasterizer.unwrap_or(rasterizer),
        (false, true) => italic_rasterizer.unwrap_or(rasterizer),
        (true, true) => bold_italic_rasterizer
            .or(italic_rasterizer)
            .or(bold_rasterizer)
            .unwrap_or(rasterizer),
    };
    let face = Face::from_slice(&active_rasterizer.bytes, 0)?;
    let features = parse_features(&key.features);
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(&key.text);

    let glyph_buffer = shape(&face, &features, buffer);
    let infos = glyph_buffer.glyph_infos();
    let positions = glyph_buffer.glyph_positions();
    if infos.is_empty() || infos.len() != positions.len() {
        return Some(ShaperCacheEntry::Negative);
    }

    let units_per_em = face.units_per_em() as f32;
    if units_per_em <= 0.0 {
        return Some(ShaperCacheEntry::Negative);
    }
    let scale = px_size / units_per_em;
    let char_count = key.text.chars().count();
    let mut glyphs = Vec::with_capacity(infos.len());

    for (info, pos) in infos.iter().zip(positions.iter()) {
        glyphs.push(ShapedGlyph {
            glyph_id: info.glyph_id as u16,
            x_advance: pos.x_advance as f32 * scale,
            x_offset: pos.x_offset as f32 * scale,
            y_offset: pos.y_offset as f32 * scale,
            cluster: info.cluster,
        });
    }

    if shaped_run_is_useful(&key.text, &glyphs, active_rasterizer) {
        Some(ShaperCacheEntry::Positive(ShapedRun { glyphs, char_count }))
    } else {
        Some(ShaperCacheEntry::Negative)
    }
}

fn parse_features(features: &[String]) -> Vec<Feature> {
    features
        .iter()
        .filter_map(|feature| Feature::from_str(feature).ok())
        .collect()
}

fn shaped_run_is_useful(text: &str, glyphs: &[ShapedGlyph], rasterizer: &FontRasterizer) -> bool {
    let char_count = text.chars().count();
    if char_count < 2 {
        return false;
    }
    if glyphs.len() < char_count {
        return true;
    }
    if glyphs.len() != char_count {
        return false;
    }

    text.chars()
        .zip(glyphs.iter())
        .any(|(c, glyph)| rasterizer.glyph_index(c) != glyph.glyph_id)
}

impl LigatureStyleKey {
    pub fn is_bold(&self) -> bool {
        self.flags & forge_core::cell::Cell::FLAG_BOLD != 0
    }

    pub fn is_italic(&self) -> bool {
        self.flags & forge_core::cell::Cell::FLAG_ITALIC != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::atlas::{
        FontStyle, GlyphAtlas, ShapedGlyphInsertResult, ShapedGlyphKey,
    };
    use crate::font::ligature::LigatureStyleKey;
    use forge_core::cell::Cell;
    use forge_core::color::Color;

    fn rasterizer() -> FontRasterizer {
        FontRasterizer::from_bytes(
            include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf"),
            16.0,
        )
        .expect("bundled test font should load")
    }

    fn style() -> LigatureStyleKey {
        LigatureStyleKey::from_cell(&Cell {
            c: '!',
            fg: Color::WHITE,
            bg: Color::BLACK,
            flags: 0,
        })
    }

    fn key_with_rasterizer(
        text: &str,
        features: &[&str],
        rasterizer: &FontRasterizer,
    ) -> TextRunKey {
        TextRunKey {
            text: text.to_string(),
            font_hash: rasterizer.identity(),
            px_size_bits: 16.0f32.to_bits(),
            style: style(),
            features: features.iter().map(|feature| feature.to_string()).collect(),
            mode: LigatureMode::CursorAware,
        }
    }

    fn key(text: &str, features: &[&str]) -> TextRunKey {
        let rasterizer = rasterizer();
        key_with_rasterizer(text, features, &rasterizer)
    }

    #[test]
    fn cache_bounds_entries() {
        let mut cache = ShaperCache::new(64);
        let rasterizer = rasterizer();
        for i in 0..80 {
            let mut key = key_with_rasterizer("plain", &["liga", "calt"], &rasterizer);
            key.text = format!("plain{i}");
            cache.insert_for_test(key, ShaperCacheEntry::Negative);
        }
        assert!(cache.len() <= 64);
    }

    #[test]
    fn negative_entries_are_cached() {
        let rasterizer = rasterizer();
        let mut cache = ShaperCache::new(64);
        let key = key("plain", &["liga", "calt"]);
        assert!(matches!(
            cache.shape_run(key.clone(), &rasterizer, None, None, None, 16.0),
            ShaperCacheEntry::Negative
        ));
        assert_eq!(cache.len(), 1);
        assert!(matches!(
            cache.shape_run(key, &rasterizer, None, None, None, 16.0),
            ShaperCacheEntry::Negative
        ));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn feature_set_is_part_of_cache_key() {
        let enabled = key("!=", &["liga", "calt"]);
        let disabled = key("!=", &["-liga", "-calt"]);
        assert_ne!(enabled, disabled);
    }

    #[test]
    fn cluster_metadata_is_preserved_for_positive_entries() {
        let rasterizer = rasterizer();
        let mut cache = ShaperCache::new(64);
        let entry = cache.shape_run(
            key("!=", &["liga", "calt"]),
            &rasterizer,
            None,
            None,
            None,
            16.0,
        );
        if let ShaperCacheEntry::Positive(run) = entry {
            assert!(!run.glyphs.is_empty());
            assert!(run.glyphs.iter().all(|glyph| glyph.cluster < 2));
        }
    }

    #[test]
    fn contextual_alternate_ligatures_are_positive_entries() {
        let rasterizer = rasterizer();
        let mut cache = ShaperCache::new(64);
        assert!(matches!(
            cache.shape_run(
                key("!=", &["liga", "clig", "calt"]),
                &rasterizer,
                None,
                None,
                None,
                16.0
            ),
            ShaperCacheEntry::Positive(_)
        ));
    }

    #[test]
    fn shaped_glyph_identity_is_accepted_by_the_atlas() {
        let rasterizer = rasterizer();
        let mut cache = ShaperCache::new(64);
        let ShaperCacheEntry::Positive(run) = cache.shape_run(
            key_with_rasterizer("!=", &["liga", "clig", "calt"], &rasterizer),
            &rasterizer,
            None,
            None,
            None,
            16.0,
        ) else {
            panic!("bundled font should shape the ligature candidate");
        };
        let glyph = run.glyphs.first().expect("shaped run should contain a glyph");
        let key = ShapedGlyphKey {
            glyph_id: glyph.glyph_id,
            style: FontStyle::Regular,
            font_hash: rasterizer.identity(),
        };
        let mut atlas = GlyphAtlas::build(&rasterizer, None, None, None, 16.0, true).unwrap();

        assert!(matches!(
            atlas.insert_shaped_glyph(key, &rasterizer, None, None, None, 16.0),
            ShapedGlyphInsertResult::Inserted(_)
        ));
    }
}
