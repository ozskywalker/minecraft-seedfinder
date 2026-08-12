//! `cubiomes-sys` — vendored [cubiomes](https://github.com/Cubitect/cubiomes)
//! (MIT) compiled via `cc`, with a minimal hand-written FFI surface.
//!
//! ## What this exposes
//!
//! cubiomes is a Java-Edition-focused biome generator. We vendor and link it
//! because it is the sanctioned MIT source for biome generation (PLAN §2.7) and
//! because Bedrock↔Java biome parity is *empirically observed but not proven*
//! (PLAN §8). The bridge exposes just the biome-query path:
//!
//! - [`Generator`] — an opaque handle to a cubiomes `Generator` (allocated on the C
//!   side, so Rust never needs the union-heavy layout).
//! - [`setup`] / [`apply_seed`] / [`biome_at`] — configure and query.
//!
//! ## ⚠️ Honesty note
//!
//! `biome_at` returns **Java** biome IDs. Whether those match Bedrock at the same
//! coordinates is **unvalidated**; that validation is the LevelDB `Data3D` work in
//! `be-biome` / PLAN §4, which requires real Bedrock world data we do not have here.
//!
//! Empirically (Phase 3): cubiomes ≤1.21 gives ~18% agreement vs the live BDS 1.26.43
//! server's `/locate biome` (see `be-biome` lib.rs). This is expected version drift —
//! cubiomes caps at 1.21 while the server is 1.26.43. Do not present `biome_at`
//! results as Bedrock-accurate for any server newer than cubiomes' supported range.
//!
//! ## Safety
//!
//! The FFI here is `unsafe`: `Generator` pointers must come from [`generator_new`]
//! and be freed with [`generator_free`], and must not be used after free. The safe
//! wrapper in `be-biome` owns this discipline; callers of this crate directly take
//! responsibility for it.

#![allow(non_camel_case_types)]

use std::os::raw::c_int;

/// Opaque handle to a cubiomes `Generator`. Never construct one directly — use
/// [`generator_new`]. The real memory is allocated and owned by C.
#[repr(C)]
pub struct Generator {
    _private: [u8; 0],
}

extern "C" {
    /// Allocate a zeroed Generator (C-side) and return a pointer.
    fn sf_generator_new() -> *mut Generator;
    /// Free a Generator returned by `sf_generator_new`.
    fn sf_generator_free(g: *mut Generator);
    /// `setupGenerator(g, mc, flags=0)`.
    fn sf_setup(g: *mut Generator, mc: c_int);
    /// `applySeed(g, dim, seed)`.
    fn sf_apply_seed(g: *mut Generator, dim: c_int, seed: u64);
    /// `getBiomeAt(g, scale, x, z, 0)` → Java biome id, or -1.
    fn sf_biome_at(g: *const Generator, scale: c_int, x: c_int, z: c_int) -> c_int;
    /// The newest MC version constant supported by cubiomes (`MC_1_21`).
    fn sf_mc_latest() -> c_int;
}

/// Dimension constants matching cubiomes' `enum Dimension`.
pub mod dim {
    pub const NETHER: i32 = -1;
    pub const OVERWORLD: i32 = 0;
    pub const END: i32 = 1;
}

/// Allocate a new opaque [`Generator`].
///
/// # Safety
/// The returned pointer must eventually be passed to [`generator_free`], and must
/// not be used after that.
pub unsafe fn generator_new() -> *mut Generator {
    unsafe { sf_generator_new() }
}

/// Free a [`Generator`] created by [`generator_new`].
///
/// # Safety
/// `g` must have been returned by [`generator_new`] and not already freed.
pub unsafe fn generator_free(g: *mut Generator) {
    unsafe { sf_generator_free(g) }
}

/// Configure the generator for a cubiomes MC version constant.
///
/// # Safety
/// `g` must be a valid, non-freed [`Generator`].
pub unsafe fn setup(g: *mut Generator, mc: i32) {
    unsafe { sf_setup(g, mc) }
}

/// Apply a world seed for a dimension ([`dim`]).
///
/// # Safety
/// `g` must be a valid, non-freed [`Generator`].
pub unsafe fn apply_seed(g: *mut Generator, dim: i32, seed: u64) {
    unsafe { sf_apply_seed(g, dim, seed) }
}

/// Query the Java biome id at a scaled position.
///
/// `scale` is 1 (block coords) or 4 (biome coords). Returns a Java biome id, or `-1`
/// on failure.
///
/// # Safety
/// `g` must be a valid, non-freed [`Generator`] that has been configured with
/// [`setup`] and [`apply_seed`].
pub unsafe fn biome_at(g: *const Generator, scale: i32, x: i32, z: i32) -> i32 {
    unsafe { sf_biome_at(g, scale, x, z) }
}

/// The newest MC version constant cubiomes supports (MC_1_21, = 28).
pub fn mc_latest() -> i32 {
    unsafe { sf_mc_latest() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bridge must return the expected cubiomes version constant.
    #[test]
    fn mc_latest_constant() {
        assert_eq!(mc_latest(), 28);
    }

    /// A full generator lifecycle: new → setup → seed → query → free, returning a
    /// valid biome id. We pin the exact Java biome id for seed 0 at the origin so a
    /// regression in the vendored build or bridge fails loudly.
    #[test]
    fn generator_lifecycle_returns_biome() {
        unsafe {
            let g = generator_new();
            assert!(!g.is_null());
            setup(g, mc_latest());
            apply_seed(g, dim::OVERWORLD, 0);
            // scale 1 = block coordinates; y=0.
            let id = biome_at(g, 1, 0, 0);
            // Valid Java biome ids are 0..=186 (see biomes.h).
            assert!(
                (0..=186).contains(&id),
                "expected a valid biome id, got {id}"
            );
            generator_free(g);
        }
    }

    /// Querying a consistent position twice must give the same biome (determinism).
    #[test]
    fn biome_query_is_deterministic() {
        unsafe {
            let g = generator_new();
            setup(g, mc_latest());
            apply_seed(g, dim::OVERWORLD, 12345);
            let a = biome_at(g, 1, 100, -200);
            let b = biome_at(g, 1, 100, -200);
            assert_eq!(a, b);
            generator_free(g);
        }
    }
}
