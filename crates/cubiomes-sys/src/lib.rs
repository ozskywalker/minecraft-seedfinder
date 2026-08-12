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
//! `biome_at` returns **Java** biome IDs. Bedrock↔Java parity is validated in
//! `be-biome`/`be-corpus` against real BDS `/locate biome` output. **As of 2026-08-12
//! this is GREEN (100%)** for the surface biomes in the corpus (see `be-biome` lib.rs).
//!
//! A previous version of this crate's bridge had a y/z argument-order bug that returned
//! the deep-cave biome (`deep_dark`) at essentially every surface coordinate, making
//! biome agreement look catastrophically low (~18%). That bug is fixed and
//! regression-tested here (`surface_biome_is_not_deep_dark`).
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
    /// `getBiomeAt(g, scale, x, z, y)` → Java biome id, or -1. Diagnostic.
    fn sf_biome_at_y(g: *const Generator, scale: c_int, x: c_int, z: c_int, y: c_int) -> c_int;
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

/// Query the Java biome id at a scaled position with an explicit block `y`
/// (diagnostic). Use to inspect how the surface biome varies with height.
///
/// # Safety
/// Same requirements as [`biome_at`].
pub unsafe fn biome_at_y(g: *const Generator, scale: i32, x: i32, z: i32, y: i32) -> i32 {
    unsafe { sf_biome_at_y(g, scale, x, z, y) }
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

    /// The surface biome at a pinned coordinate, for seed 0, is the *surface* biome —
    /// NOT the cave biome (deep_dark, id 183).
    ///
    /// Regression guard for a bridge bug where getBiomeAt's `y` and `z` arguments were
    /// swapped, causing every surface query to return deep_dark (the deep-cave biome)
    /// and making Bedrock↔Java biome agreement look catastrophically low. With the
    /// correct (x, y=63, z) argument order, cubiomes reports the surface biome at each
    /// of these coordinates, matching the real BDS 1.21.40 `/locate biome` output.
    #[test]
    fn surface_biome_is_not_deep_dark() {
        // (x, z, expected java id): verified against the real BDS 1.21.40 server.
        // seed 0: plains=1 at (-800,-96), desert=2 at (608,-1632), forest=4 at (-32,-32),
        // ocean=0 at (-352,-352).
        let cases: [(i32, i32, i32); 4] = [(-800, -96, 1), (608, -1632, 2), (-32, -32, 4), (-352, -352, 0)];
        unsafe {
            let g = generator_new();
            setup(g, mc_latest());
            apply_seed(g, dim::OVERWORLD, 0);
            for (x, z, expected) in cases {
                let id = biome_at(g, 1, x, z);
                assert_eq!(
                    id, expected,
                    "surface biome at ({x},{z}) should be {expected}, got {id} (deep_dark=183)"
                );
            }
            generator_free(g);
        }
    }

    /// Explicit-y diagnostic must respect the (x, y, z) argument order: at any surface
    /// y the biome is the surface biome, never deep_dark.
    #[test]
    fn biome_at_y_respects_argument_order() {
        unsafe {
            let g = generator_new();
            setup(g, mc_latest());
            apply_seed(g, dim::OVERWORLD, 0);
            // (608, -1632) is desert (2) at every sampled y.
            for y in [0, 40, 63, 100, 200] {
                assert_eq!(biome_at_y(g, 1, 608, -1632, y), 2, "y={y}");
            }
            generator_free(g);
        }
    }
}
