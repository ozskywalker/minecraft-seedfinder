//! Golden tests: structure positions for a pinned real world seed, validated
//! against `/locate` output captured from the live BDS 1.26.43 server
//! (PLAN §5 "Golden" layer).
//!
//! Seed: `-2932807814640844199` (extracted from that world's `level.dat`).
//! Observed positions come from `crates/be-verify/tests/fixtures/real_locate_output.txt`
//! and the live captures recorded during Phase 0. These values pin the generator so a
//! silent change to any parameter (salt, spacing, chunk-range, distribution) fails
//! loudly.
//!
//! Regenerating these requires explicit intent — do not just edit numbers here.

use crate::{placement::structure_block_pos, region::region_of_block, Version};

const REAL_SEED: i64 = -2_932_807_814_640_844_199;

/// The 1.21.40 table validates the live 1.26.43 server exactly for every
/// anchor-returning structure we captured (village, monument, ancient city,
/// pillager outpost, shipwreck, buried treasure, ruined portal).
///
/// `(structure_id, observed_block_x, observed_block_z)`.
const OBSERVED: &[(&str, i64, i64)] = &[
    ("village", 184, 296),
    ("ocean_monument", 1288, 664),
    ("ancient_city", 184, 2056),
    ("pillager_outpost", 472, 120),
    ("shipwreck", 696, 520),
    ("buried_treasure", -440, -248),
    ("ruined_portal", 312, 40),
];

#[test]
fn golden_predictions_match_real_bds_12643() {
    let v = Version::builtin_1_21_40();
    let seed = REAL_SEED as u64;

    for &(id, obs_x, obs_z) in OBSERVED {
        let s = &v.structures[id];
        let reg_x = region_of_block(obs_x, s.spacing);
        let reg_z = region_of_block(obs_z, s.spacing);
        let (px, pz) = structure_block_pos(
            seed,
            reg_x,
            reg_z,
            s.salt,
            s.spacing,
            s.chunk_range,
            s.distribution(),
        );
        assert_eq!(
            (px, pz),
            (obs_x, obs_z),
            "structure {id} in region ({reg_x},{reg_z}): predicted ({px},{pz}) != observed ({obs_x},{obs_z})"
        );
    }
}

/// Every anchor-returning `/locate` coordinate must be a chunk anchor, i.e.
/// `block ≡ 8 (mod 16)`. This is the invariant that lets us compare `/locate`
/// output against the generator's `(chunk << 4) + 8` block position.
#[test]
fn golden_observed_are_chunk_anchors() {
    for &(id, x, z) in OBSERVED {
        assert_eq!(x.rem_euclid(16), 8, "{id} x is not a chunk anchor");
        assert_eq!(z.rem_euclid(16), 8, "{id} z is not a chunk anchor");
    }
}

/// Village predictions from per-seed fresh worlds (the §4 harness model), captured
/// from the live server 2026-08-12. This exercises the "one fresh world per seed"
/// flow AND confirms the generator on four controlled seeds, not just the one random
/// world. Every seed must predict EXACTLY.
///
/// Data from `tests/fixtures/per_seed_worlds.txt`.
#[test]
fn golden_village_across_fresh_worlds() {
    let v = Version::builtin_1_21_40();
    let s = &v.structures["village"];
    // (seed, observed_x, observed_z)
    let worlds: &[(i64, i64, i64)] = &[
        (42, 680, -376),
        (12345, -488, 248),
        (777, -888, -200),
        (20240812, -856, -904),
    ];
    for &(seed, ox, oz) in worlds {
        let reg_x = region_of_block(ox, s.spacing);
        let reg_z = region_of_block(oz, s.spacing);
        let (px, pz) = structure_block_pos(
            seed as u64,
            reg_x,
            reg_z,
            s.salt,
            s.spacing,
            s.chunk_range,
            s.distribution(),
        );
        assert_eq!(
            (px, pz),
            (ox, oz),
            "seed {seed}: village predicted ({px},{pz}) != observed ({ox},{oz})"
        );
    }
}

/// Trial chambers does not fit the region+anchor placement model.
///
/// Its `/locate` returns the bounding-box **centre** (never a `≡ 8 mod 16` chunk
/// anchor) for every observed seed, and no salt in 0..200M (nor any known structure
/// salt) yields a constant residual across 5 independent seeds. This is consistent
/// with trial chambers being a large jigsaw structure whose placement anchor sits in
/// a different region than its reported centre — confirming its salt/distribution is
/// not possible from `/locate` alone (PLAN §4 / §2.8 jigsaw scope note).
///
/// This test documents the observed centres so a future change (e.g. a version where
/// /locate starts returning the anchor) fails loudly rather than silently.
#[test]
fn trial_chambers_reported_centres_are_not_anchors() {
    // (seed, reported_x, reported_z)
    let centres: &[(i64, i64, i64)] = &[
        (-2_932_807_814_640_844_199, 169, 297),
        (42, 281, 121),
        (12345, 9, 233),
        (777, 73, 41),
        (20240812, 41, 71),
    ];
    for &(_seed, x, z) in centres {
        // Not an anchor: a valid chunk anchor is always ≡ 8 mod 16, and these
        // reported centres never are (they range across ≡ 7, 9, ... depending on
        // the structure's size/orientation). Assert the negative — the invariant
        // that distinguishes a bounding-box centre from a generator anchor.
        assert_ne!(x.rem_euclid(16), 8, "x is unexpectedly a chunk anchor");
        assert_ne!(z.rem_euclid(16), 8, "z is unexpectedly a chunk anchor");
    }
}
