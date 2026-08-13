//! Safe biome query API.
//!
//! A [`BiomeQuery`] answers "what is the biome numeric id at block (x, z)?".
//! [`CubiomesQuery`] implements it over the vendored cubiomes FFI and owns the
//! `Generator` pointer, freeing it on drop (RAII). It returns **Java** biome ids.

use cubiomes_sys::{apply_seed, biome_at, generator_free, generator_new, setup, Generator};

/// A source of biome numeric ids at block coordinates.
pub trait BiomeQuery {
    /// The biome numeric id at block coordinates `(x, z)`, or `None` if the query
    /// failed. Interpretation of the id (Java vs Bedrock) depends on the backend.
    fn biome_id_at(&self, x: i32, z: i32) -> Option<u16>;
}

/// Return the Java biome name at block `(x, z)` for a query + id map.
///
/// This is the bridge used by the §8 parity validation: it reports what cubiomes
/// (Java) thinks the biome is, by name, at the exact coordinate where `/locate biome`
/// reported a Bedrock biome. Compares against the requested biome name.
pub fn java_biome_name_at(
    query: &dyn BiomeQuery,
    map: &crate::map::BiomeIdMap,
    x: i32,
    z: i32,
) -> Option<String> {
    let id = query.biome_id_at(x, z)?;
    map.java_name(id).map(|n| n.to_string())
}

/// A biome query backed by cubiomes (Java biome ids).
///
/// Owns the cubiomes `Generator`; freed on drop. `mc` is a cubiomes MC version
/// constant (e.g. [`cubiomes_sys::mc_latest`] for 1.21.x); `seed` is the world seed.
pub struct CubiomesQuery {
    g: *mut Generator,
}

impl CubiomesQuery {
    /// Create a query for the Overworld of the given seed and cubiomes MC version.
    pub fn new(mc: i32, seed: u64) -> CubiomesQuery {
        unsafe {
            let g = generator_new();
            assert!(!g.is_null(), "cubiomes failed to allocate a Generator");
            setup(g, mc);
            apply_seed(g, cubiomes_sys::dim::OVERWORLD, seed);
            CubiomesQuery { g }
        }
    }

    /// Re-seed this generator with a different world seed, reusing the same
    /// `Generator` allocation. This is the fast path for Phase B: sweeping many high
    /// 32-bit halves for one structural candidate without allocating a generator per
    /// seed.
    pub fn set_seed(&mut self, seed: u64) {
        unsafe {
            apply_seed(self.g, cubiomes_sys::dim::OVERWORLD, seed);
        }
    }
}

impl BiomeQuery for CubiomesQuery {
    fn biome_id_at(&self, x: i32, z: i32) -> Option<u16> {
        // scale 1 = block coordinates. The bridge samples the SURFACE biome (y=63),
        // which is what `/locate biome` reports (see cubiomes-sys bridge.c).
        let id = unsafe { biome_at(self.g, 1, x, z) };
        if id < 0 {
            None
        } else {
            Some(id as u16)
        }
    }
}

impl Drop for CubiomesQuery {
    fn drop(&mut self) {
        unsafe { generator_free(self.g) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_valid_ids() {
        let q = CubiomesQuery::new(cubiomes_sys::mc_latest(), 1234);
        // Overworld ids are in a valid range; just ensure we get something.
        for &(x, z) in &[(0, 0), (10, 10), (-50, 30)] {
            let id = q.biome_id_at(x, z);
            assert!(id.is_some(), "biome at ({x},{z})");
            let id = id.unwrap();
            assert!(id <= 200, "unexpected id {id}");
        }
    }

    #[test]
    fn deterministic() {
        let q = CubiomesQuery::new(cubiomes_sys::mc_latest(), 999);
        assert_eq!(q.biome_id_at(100, -200), q.biome_id_at(100, -200));
    }

    /// Dropping must not crash (RAII frees the C Generator).
    #[test]
    fn drop_is_safe() {
        let q = CubiomesQuery::new(cubiomes_sys::mc_latest(), 1);
        drop(q);
        let q2 = CubiomesQuery::new(cubiomes_sys::mc_latest(), 2);
        assert!(q2.biome_id_at(0, 0).is_some());
    }
}
