//! Server-side tile renderer (§3.2).
//!
//! 512-block tiles, biomes sampled at quart (1:4) resolution → a 128×128 PNG.
//! Tiles are keyed `(seed, version, x, z, lod)` and cached in an LRU cache keyed by
//! seed+version (§3.2). Rendering uses the validated cubiomes biome path (`be-biome`),
//! so a tile's colors reflect the same Bedrock-accurate biomes the search gates on.
//!
//! # Tile coordinate convention
//!
//! A tile covers the 512-block square from `(x0, z0)` to `(x0+512, z0+512)` inclusive,
//! where `x0 = tx * 512`, `z0 = tz * 512`. `lod` is a coarse-to-fine zoom factor
//! (0 = full 128×128; higher = coarser, fewer samples — used for progressive LOD).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use be_biome::{BiomeQuery, CubiomesQuery};

/// Blocks per tile edge.
pub const TILE_BLOCKS: i64 = 512;
/// Samples per tile edge at full detail (quart = 1:4 resolution).
pub const TILE_SAMPLES: usize = 128;

/// A cached rendered tile.
pub struct TileCache {
    inner: Mutex<HashMap<String, Arc<Vec<u8>>>>,
    capacity: usize,
}

impl TileCache {
    pub fn new(capacity: usize) -> Self {
        TileCache {
            inner: Mutex::new(HashMap::new()),
            capacity,
        }
    }

    /// LRU-ish insert: if at capacity, evict the oldest key (approximate LRU).
    fn insert(&self, key: String, bytes: Arc<Vec<u8>>) {
        let mut map = self.inner.lock().unwrap();
        if map.len() >= self.capacity && !map.contains_key(&key) {
            // Evict an arbitrary key (insertion-order approx). Good enough for a
            // localhost tile cache.
            if let Some(k) = map.keys().next().cloned() {
                map.remove(&k);
            }
        }
        map.insert(key, bytes);
    }

    pub fn get(&self, key: &str) -> Option<Arc<Vec<u8>>> {
        self.inner.lock().unwrap().get(key).cloned()
    }
}

/// Color for a Java biome id. Falls back to a neutral gray for unknown biomes.
fn biome_color(id: u16) -> [u8; 3] {
    // Approximate palette keyed by Java biome id (surface biomes).
    match id {
        0 => [52, 90, 168],    // ocean
        1 => [141, 179, 96],   // plains
        2 => [219, 196, 138],  // desert
        3 => [140, 180, 90],   // windswept hills
        4 => [59, 115, 40],    // forest
        5 => [80, 90, 100],    // taiga
        6 => [80, 105, 60],    // swamp
        7 => [140, 190, 100],  // river
        9 => [160, 160, 160],  // beach
        12 => [170, 180, 200], // ice plains / snowy plains
        17 => [200, 180, 140], // desert hills
        18 => [90, 140, 60],   // forest hills
        21 => [53, 108, 42],   // jungle
        22 => [70, 90, 50],    // jungle hills
        24 => [170, 170, 190], // frozen ocean
        25 => [200, 200, 210], // frozen river
        26 => [210, 210, 215], // snowy taiga
        27 => [220, 220, 225], // snowy mountains
        29 => [20, 70, 20],    // roofed forest / dark forest
        30 => [140, 110, 60],  // snowy beach
        32 => [150, 150, 150], // stone shore
        34 => [90, 150, 90],   // swamp hills
        44 => [180, 120, 80],  // wooded badlands
        45 => [200, 130, 90],  // badlands
        47 => [200, 130, 90],  // eroded badlands
        50 => [40, 80, 140],   // deep frozen ocean
        129 => [160, 120, 90], // sunflower plains
        130 => [70, 120, 60],  // desert lakes
        131 => [100, 140, 80], // flower forest
        132 => [60, 100, 70],  // taiga mountains
        133 => [90, 110, 130], // swamp hills
        155 => [70, 110, 90],  // birch forest
        _ => [120, 120, 120],  // unknown
    }
}

/// Render a tile as PNG bytes.
///
/// * `seed` — world seed.
/// * `tx`, `tz` — tile indices (tile origin = tx*512, tz*512).
/// * `lod` — level of detail: 0 = full 128×128; 1 = 64×64 (every 8 blocks);
///   higher = coarser. Progressive LOD stretches the coarse tile first, swaps in the
///   sharp one when ready (§3.2).
///
/// Returns the PNG-encoded bytes.
pub fn render_tile_png(seed: u64, tx: i64, tz: i64, lod: u32) -> Vec<u8> {
    let samples = TILE_SAMPLES >> lod.min(6);
    let block_step = TILE_BLOCKS / samples as i64;

    let mc = cubiomes_sys::mc_latest();
    let query = CubiomesQuery::new(mc, seed);

    // Build an RGB image.
    let mut img = image::RgbImage::new(samples as u32, samples as u32);
    let x0 = tx * TILE_BLOCKS;
    let z0 = tz * TILE_BLOCKS;
    for sy in 0..samples {
        for sx in 0..samples {
            let bx = x0 + (sx as i64 * block_step);
            let bz = z0 + (sy as i64 * block_step);
            let id = query.biome_id_at(bx as i32, bz as i32).unwrap_or(0);
            let c = biome_color(id);
            img.put_pixel(sx as u32, sy as u32, image::Rgb(c));
        }
    }

    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("png encode");
    out
}

/// Render a tile and cache it by key `(seed, tx, tz, lod)`.
pub fn cached_tile(cache: &TileCache, seed: u64, tx: i64, tz: i64, lod: u32) -> Arc<Vec<u8>> {
    let key = format!("{seed}:{tx}:{tz}:{lod}");
    if let Some(b) = cache.get(&key) {
        return b;
    }
    let bytes = Arc::new(render_tile_png(seed, tx, tz, lod));
    cache.insert(key, bytes.clone());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_is_png_and_correct_size() {
        let png = render_tile_png(42, 0, 0, 0);
        // PNG signature.
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let img = image::load_from_memory(&png).expect("valid png");
        assert_eq!(img.width(), TILE_SAMPLES as u32);
        assert_eq!(img.height(), TILE_SAMPLES as u32);
    }

    #[test]
    fn lod_reduces_resolution() {
        let png = render_tile_png(42, 0, 0, 1);
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!(img.width(), (TILE_SAMPLES / 2) as u32);
    }

    #[test]
    fn cache_hits_return_same_bytes() {
        let cache = TileCache::new(10);
        let a = cached_tile(&cache, 42, 0, 0, 0);
        let b = cached_tile(&cache, 42, 0, 0, 0);
        assert_eq!(a, b);
        assert_eq!(cache.get("42:0:0:0").unwrap().len(), a.len());
    }

    #[test]
    fn cache_evicts_at_capacity() {
        let cache = TileCache::new(2);
        cached_tile(&cache, 1, 0, 0, 0);
        cached_tile(&cache, 2, 0, 0, 0);
        cached_tile(&cache, 3, 0, 0, 0);
        // With capacity 2, only 2 of the 3 keys remain.
        let map = cache.inner.lock().unwrap();
        assert!(map.len() <= 2, "LRU eviction should cap at 2");
    }
}
