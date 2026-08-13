//! Region-seed formula (§2.4).
//!
//! ```text
//! regionSeed = (worldSeed + regX*341873128712 + regZ*132897987541 + salt) mod 2^32
//! mt[0]      = regionSeed & 0xFFFFFFFF
//! regX       = floorDiv(chunkX, spacing)          // floor, not trunc — negatives matter
//! ```

/// Euclidean-style floor division (rounds toward negative infinity).
///
/// Rust's built-in `/` truncates toward zero, which is wrong for negative dividends:
/// `(-7) / 32 == 0` with truncation but `floorDiv(-7, 32) == -1`. The game floors.
///
/// # Panics
/// Panics if `b == 0`.
pub fn floor_div(a: i64, b: i64) -> i64 {
    assert!(b != 0, "floor_div: division by zero");
    let q = a / b;
    let r = a % b;
    // If there is a non-zero remainder and the signs disagree, floor rounds down.
    if r != 0 && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

/// Compute the 32-bit region seed for a structure region.
///
/// `reg_x`/`reg_z` are region coordinates (already floored from chunk coords),
/// `salt` is the per-structure salt from the version table, and `world_seed` is the
/// full 64-bit world seed. The result is masked to the low 32 bits, which is exactly
/// the value used to seed the RNG.
///
/// All arithmetic is done in `i128` to avoid overflow on the `reg * 3.4e11` products.
pub fn region_seed(world_seed: u64, reg_x: i64, reg_z: i64, salt: u32) -> u64 {
    let acc = world_seed as i128
        + (reg_x as i128) * 341_873_128_712i128
        + (reg_z as i128) * 132_897_987_541i128
        + (salt as i128);
    (acc & 0xFFFF_FFFF) as u64
}

/// Which structure region a block coordinate falls in: `floorDiv(block >> 4,
/// spacing)`. Used to back out the region a `/locate` result points at, so the
/// generator can be checked against the game region-by-region (PLAN §4 / Phase 0).
pub fn region_of_block(block: i64, spacing: u32) -> i64 {
    floor_div(block >> 4, spacing as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_div_positive() {
        assert_eq!(floor_div(7, 32), 0);
        assert_eq!(floor_div(32, 32), 1);
        assert_eq!(floor_div(64, 32), 2);
    }

    #[test]
    fn floor_div_negative_truncates_toward_neg_inf() {
        // Truncation would give 0; floor gives -1. This is the silent-corruption trap.
        assert_eq!(floor_div(-7, 32), -1);
        assert_eq!(floor_div(-32, 32), -1);
        assert_eq!(floor_div(-33, 32), -2);
        assert_eq!(floor_div(-64, 32), -2);
    }

    #[test]
    fn floor_div_negative_divisor() {
        assert_eq!(floor_div(7, -32), -1);
        assert_eq!(floor_div(-7, -32), 0);
    }

    #[test]
    fn floor_div_exact_multiples() {
        assert_eq!(floor_div(-64, 32), -2);
        assert_eq!(floor_div(0, 32), 0);
    }

    /// The classic property of floor division: a == b*q + r with 0 <= r < |b|.
    #[test]
    fn floor_div_invariant() {
        for a in [-1000i64, -999, -7, -1, 0, 1, 7, 999, 1000] {
            for b in [1i64, 2, 16, 24, 26, 32, 34, 80] {
                let q = floor_div(a, b);
                let r = a - b * q;
                assert!(
                    (0..b).contains(&r),
                    "a={a} b={b} -> q={q} r={r} not in [0,{b})"
                );
            }
        }
    }

    /// Masking: the result is always within 32 bits regardless of sign of inputs.
    #[test]
    fn region_seed_is_32_bit_masked() {
        for &(wx, rx, rz, s) in &[
            (0u64, 0i64, 0i64, 0u32),
            (0xFFFF_FFFF_FFFF_FFFF, 1, 1, 0),
            (0, -1, -1, 0),
            (0, 0, 0, 0xFFFF_FFFF),
            (1234, 12345, -54321, 14357617),
            (0xDEAD_BEEF_CAFE_F00D, -9999, 8888, 10387312),
        ] {
            let r = region_seed(wx, rx, rz, s);
            assert!(r <= 0xFFFF_FFFF, "region seed out of 32-bit range: {r:#x}");
        }
    }

    /// Deterministic spot values, pinned so a refactor that silently changes the
    /// formula (e.g. switching to i64 wraparound or forgetting the mask) fails.
    #[test]
    fn region_seed_known_values() {
        // Hand-computed from the formula.
        assert_eq!(region_seed(0, 0, 0, 0), 0);
        assert_eq!(region_seed(0, 0, 0, 1), 1);
        // worldSeed + salt, masked to 32 bits
        assert_eq!(region_seed(0x1_0000_0000, 0, 0, 0), 0);
        // A non-trivial combination, pinned from the formula directly.
        assert_eq!(
            region_seed(1234, 3, -2, 14357617),
            ((1234i128 + 3 * 341_873_128_712i128 - 2 * 132_897_987_541i128 + 14_357_617i128)
                & 0xFFFF_FFFF) as u64
        );
    }

    #[test]
    fn region_of_block_basic() {
        // block 0 -> chunk 0 -> region 0
        assert_eq!(region_of_block(0, 32), 0);
        // block 511 (chunk 31) -> region 0
        assert_eq!(region_of_block(511, 32), 0);
        // block 512 (chunk 32) -> region 1
        assert_eq!(region_of_block(512, 32), 1);
    }

    #[test]
    fn region_of_block_negative() {
        // chunk -1 -> floor(-1/32) = -1
        assert_eq!(region_of_block(-16, 32), -1);
        assert_eq!(region_of_block(-512, 32), -1);
        assert_eq!(region_of_block(-513, 32), -2);
    }

    #[test]
    fn region_of_block_inverse_of_placement() {
        // A structure placed in region (rx, rz) has a block pos whose region_of_block
        // is (rx, rz) for a non-empty spacing window.
        let v = crate::Version::builtin_1_21_40();
        for rx in -3i64..=3 {
            for rz in -3i64..=3 {
                let s = &v.structures["village"];
                let (bx, bz) = crate::placement::structure_block_pos(
                    0xDEAD_BEEF,
                    rx,
                    rz,
                    s.salt,
                    s.spacing,
                    s.chunk_range,
                    s.distribution(),
                );
                assert_eq!(region_of_block(bx, s.spacing), rx);
                assert_eq!(region_of_block(bz, s.spacing), rz);
            }
        }
    }
}
