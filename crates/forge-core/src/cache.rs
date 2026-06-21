use bytemuck::{Pod, Zeroable};
use crate::color::Color;
use crate::config_registry::ForgeConfig;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use memmap2::Mmap;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct StartupCache {
    pub magic: [u8; 4],
    pub checksum: u32,
    pub version: u32,
    pub background_color: Color,
    pub window_width: u32,
    pub window_height: u32,
    pub opacity: u8,
    pub _pad: [u8; 3],
    pub cell_width: u32,
    pub cell_height: u32,
    pub baseline: u32,
}

const _: () = assert!(std::mem::size_of::<StartupCache>() == 40);

pub fn cache_path() -> PathBuf {
    if let Ok(cache_home) = env::var("XDG_CACHE_HOME") {
        if !cache_home.is_empty() {
            let mut path = PathBuf::from(cache_home);
            path.push("forge");
            path.push("startup_state.bin");
            return path;
        }
    }
    
    if let Ok(home) = env::var("HOME") {
        if !home.is_empty() {
            let mut path = PathBuf::from(home);
            path.push(".cache");
            path.push("forge");
            path.push("startup_state.bin");
            return path;
        }
    }

    PathBuf::from("/tmp/forge_startup_cache.bin")
}

fn compute_checksum(cache: &StartupCache) -> u32 {
    let bytes = bytemuck::bytes_of(cache);
    let mut sum: u32 = 0;
    for &b in &bytes[8..] {
        sum = sum.wrapping_add(b as u32);
    }
    sum
}

pub fn read_startup_cache() -> Option<StartupCache> {
    let path = cache_path();
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(_) => return None,
    };
    
    let metadata = match file.metadata() {
        Ok(m) => m,
        Err(_) => return None,
    };
    
    if metadata.len() as usize != std::mem::size_of::<StartupCache>() {
        return None;
    }
    
    let mmap = match unsafe { Mmap::map(&file) } {
        Ok(m) => m,
        Err(_) => return None,
    };
    
    let cache: &StartupCache = match bytemuck::try_from_bytes(&mmap) {
        Ok(c) => c,
        Err(_) => return None,
    };
    
    if cache.magic != *b"FRG\x01" {
        return None;
    }
    
    if cache.version != 1 {
        return None;
    }
    
    if cache.checksum != compute_checksum(cache) {
        return None;
    }
    
    Some(*cache)
}

pub fn write_startup_cache(cache: &StartupCache) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            tracing::warn!("Failed to create cache directory {}: {}", parent.display(), e);
            return;
        }
    }
    
    let tmp_path = path.with_extension("tmp");
    let mut file = match File::create(&tmp_path) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("Failed to create temp cache file {}: {}", tmp_path.display(), e);
            return;
        }
    };
    
    let bytes = bytemuck::bytes_of(cache);
    if let Err(e) = file.write_all(bytes) {
        tracing::warn!("Failed to write cache to {}: {}", tmp_path.display(), e);
        let _ = fs::remove_file(&tmp_path);
        return;
    }
    
    if let Err(e) = file.flush() {
        tracing::warn!("Failed to flush cache to {}: {}", tmp_path.display(), e);
        let _ = fs::remove_file(&tmp_path);
        return;
    }
    
    // Note on atomic rename:
    // fs::rename is atomic on Linux as long as both paths are on the same filesystem.
    // TOCTOU risk: If a second process writes to this file simultaneously, one of the renames
    // will silently overwrite the other. This is acceptable here because the cache is
    // purely best-effort for startup acceleration. If the cache is ever corrupted or lost,
    // the terminal simply falls back to default settings without crashing.
    if let Err(e) = fs::rename(&tmp_path, &path) {
        tracing::warn!("Failed to rename temp cache from {} to {}: {}", tmp_path.display(), path.display(), e);
        let _ = fs::remove_file(&tmp_path);
    }
}

impl StartupCache {
    pub fn new_cache(config: &ForgeConfig, cell_width: u32, cell_height: u32, baseline: u32) -> Self {
        let mut cache = StartupCache {
            magic: *b"FRG\x01",
            checksum: 0,
            version: 2, // Bumped version to invalidate old caches
            background_color: config.theme.background,
            window_width: config.window.width,
            window_height: config.window.height,
            opacity: (config.window.opacity * 255.0) as u8,
            _pad: [0; 3],
            cell_width,
            cell_height,
            baseline,
        };
        cache.checksum = compute_checksum(&cache);
        cache
    }

    pub fn background_f32(&self) -> [f32; 4] {
        [
            self.background_color.r as f32 / 255.0,
            self.background_color.g as f32 / 255.0,
            self.background_color.b as f32 / 255.0,
            self.opacity as f32 / 255.0,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_registry::ForgeConfig;

    #[test]
    fn roundtrip_cache() {
        let config = ForgeConfig::default();
        let cache = StartupCache::new_cache(&config, 10, 20, 16);
        
        let tmp = std::env::temp_dir().join("forge_cache_test.bin");
        let bytes = bytemuck::bytes_of(&cache);
        // .unwrap() is acceptable in tests
        std::fs::write(&tmp, bytes).unwrap();
        
        let data = std::fs::read(&tmp).unwrap();
        let read_back: &StartupCache = bytemuck::from_bytes(&data);
        
        assert_eq!(cache.version, read_back.version);
        assert_eq!(cache.window_width, read_back.window_width);
        assert_eq!(cache.cell_width, read_back.cell_width);
        
        std::fs::remove_file(&tmp).ok();
    }
}
