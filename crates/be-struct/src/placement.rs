//! Structure placement (§2.4).
//!
//! Given the region coordinates and the structure's spacing/chunk-range, compute the
//! structure's block position. The RNG is seeded with the region seed and the offset
//! is drawn with exact order:
//!
//! ```text
//! linear     (n=2): x = mNextInt(range); z = mNextInt(range)
//! triangular (n=4): x1,x2,z1,z2 = four draws IN THAT ORDER
//!                   x = (x1+x2)>>1 ; z = (z1+z2)>>1
//! blockPos = ((regX*spacing + chunkInRegionX) << 4) + 8
//! ```

use be_rng::{first_n_into, MersenneTwister, STREAM_BUF};

use crate::region::region_seed;

/// The distribution (draw shape) of a structure's placement RNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribution {
    /// Two draws: `x`, `z`.
    Linear,
    /// Four draws in order `x1,x2,z1,z2`, each pair averaged.
    Triangular,
}

/// Compute the block position of a structure given its region.
///
/// * `world_seed` — full 64-bit world seed.
/// * `reg_x`, `reg_z` — structure-region coordinates (floored).
/// * `salt`, `spacing`, `chunk_range` — from the version table.
/// * `dist` — linear or triangular.
///
/// Returns the `(block_x, block_z)` of the structure's chunk anchor
/// (chunk origin block + 8, i.e. the block at the centre of the anchor chunk).
pub fn structure_block_pos(
    world_seed: u64,
    reg_x: i64,
    reg_z: i64,
    salt: u32,
    spacing: u32,
    chunk_range: u32,
    dist: Distribution,
) -> (i64, i64) {
    let rseed = region_seed(world_seed, reg_x, reg_z, salt) as u32;

    let (offset_x, offset_z) = match dist {
        Distribution::Linear => {
            // Reference implementation uses the full generator; our streaming path is
            // proven equal for the first n outputs.
            let mut rng = MersenneTwister::new(rseed);
            (rng.next_int(chunk_range), rng.next_int(chunk_range))
        }
        Distribution::Triangular => {
            let mut rng = MersenneTwister::new(rseed);
            let x1 = rng.next_int(chunk_range);
            let x2 = rng.next_int(chunk_range);
            let z1 = rng.next_int(chunk_range);
            let z2 = rng.next_int(chunk_range);
            (((x1 + x2) >> 1), ((z1 + z2) >> 1))
        }
    };

    let chunk_x = reg_x * spacing as i64 + offset_x as i64;
    let chunk_z = reg_z * spacing as i64 + offset_z as i64;
    ((chunk_x << 4) + 8, (chunk_z << 4) + 8)
}

/// Streaming variant of [`structure_block_pos`] that uses the memory-lean
/// `first_n` twist instead of the full 624-word generator. Must produce identical
/// results (proven by property test against the full generator).
///
/// Hot path: uses a fixed stack buffer via [`be_rng::first_n_into`], so computing a
/// structure position performs **no heap allocation** — the seed-lookup sweep calls
/// this once per (structure, region) per seed.
pub fn structure_block_pos_streaming(
    world_seed: u64,
    reg_x: i64,
    reg_z: i64,
    salt: u32,
    spacing: u32,
    chunk_range: u32,
    dist: Distribution,
) -> (i64, i64) {
    let rseed = region_seed(world_seed, reg_x, reg_z, salt) as u32;
    let mut buf = [0u32; STREAM_BUF];

    let (offset_x, offset_z) = match dist {
        Distribution::Linear => {
            let raw = first_n_into(rseed, 2, &mut buf);
            (
                be_rng::next_int(chunk_range, raw[0]),
                be_rng::next_int(chunk_range, raw[1]),
            )
        }
        Distribution::Triangular => {
            let raw = first_n_into(rseed, 4, &mut buf);
            let x1 = be_rng::next_int(chunk_range, raw[0]);
            let x2 = be_rng::next_int(chunk_range, raw[1]);
            let z1 = be_rng::next_int(chunk_range, raw[2]);
            let z2 = be_rng::next_int(chunk_range, raw[3]);
            (((x1 + x2) >> 1), ((z1 + z2) >> 1))
        }
    };

    let chunk_x = reg_x * spacing as i64 + offset_x as i64;
    let chunk_z = reg_z * spacing as i64 + offset_z as i64;
    ((chunk_x << 4) + 8, (chunk_z << 4) + 8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Distribution::*;

    /// Both the full-generator and streaming paths must agree exactly. This is the
    /// integration point between be-rng and be-struct.
    #[test]
    fn streaming_equals_full_generator() {
        let dists = [Linear, Triangular];
        let params = [
            (14357617u32, 32u32, 24u32), // scattered (linear)
            (10387312, 34, 26),          // village (triangular)
            (10387313, 32, 27),          // monument (triangular)
            (10387319, 80, 60),          // mansion (triangular)
            (165745296, 80, 56),         // outpost (triangular)
        ];
        for &dist in &dists {
            for &(salt, spacing, cr) in &params {
                for wx in [0u64, 1, 5489, 0xFFFF_FFFF, 0x1234_5678_9ABC_DEF0] {
                    for &(rx, rz) in &[(0i64, 0i64), (1, -1), (-3, 2), (40, 40)] {
                        let full = structure_block_pos(wx, rx, rz, salt, spacing, cr, dist);
                        let streamed =
                            structure_block_pos_streaming(wx, rx, rz, salt, spacing, cr, dist);
                        assert_eq!(
                            full, streamed,
                            "seed {wx:#x} reg({rx},{rz}) salt {salt} dist {dist:?}"
                        );
                    }
                }
            }
        }
    }

    /// Triangular draw order: x = (x1+x2)>>1 must come from x1,x2 before z1,z2. A
    /// swap of z/x order changes the result; pin a known case.
    #[test]
    fn triangular_draw_order_matters() {
        // Triangular: x draws happen before z draws. If someone accidentally draws
        // z first, the result changes. Pin values so a reorder fails loudly.
        let rseed = region_seed(0, 0, 0, 10387313) as u32;
        let mut rng = MersenneTwister::new(rseed);
        let x1 = rng.next_int(27);
        let x2 = rng.next_int(27);
        let z1 = rng.next_int(27);
        let z2 = rng.next_int(27);
        let expect_x = ((x1 + x2) >> 1) as i64;
        let expect_z = ((z1 + z2) >> 1) as i64;
        let (bx, bz) = structure_block_pos(0, 0, 0, 10387313, 32, 27, Triangular);
        // block pos = (chunk << 4) + 8 with chunk == regX*spacing + offset
        assert_eq!(bx >> 4, expect_x, "x offset");
        assert_eq!(bz >> 4, expect_z, "z offset");
    }

    /// block pos formula: ((reg*spacing + offset) << 4) + 8.
    #[test]
    fn block_pos_formula() {
        let (bx, bz) = structure_block_pos(0, 0, 0, 14357617, 32, 24, Linear);
        // offset in [0, 24); chunk == offset; block == (offset<<4)+8
        let ox = (bx - 8) >> 4;
        let oz = (bz - 8) >> 4;
        assert!((0..24).contains(&ox));
        assert!((0..24).contains(&oz));
        assert_eq!(bx, (ox << 4) + 8);
        assert_eq!(bz, (oz << 4) + 8);
    }

    /// Offsets stay within the chunk-range for triangular (averaging keeps them in
    /// range).
    #[test]
    fn triangular_offset_in_range() {
        for seed in 0..200u64 {
            let (bx, bz) = structure_block_pos(seed, 1, -1, 10387319, 80, 60, Triangular);
            let ox = ((bx - 8) >> 4) - 80; // subtract regX*spacing (regX=1)
            let oz = ((bz - 8) >> 4) + 80; // regZ=-1 -> +80
            assert!((0..60).contains(&ox), "seed {seed} ox {ox}");
            assert!((0..60).contains(&oz), "seed {seed} oz {oz}");
        }
    }
}
