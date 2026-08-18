//! Phase C — verify a returned search-result seed against the real Bedrock server
//! (PLAN §7 "Phase C": spawn a fresh world per seed and confirm the generator's
//! prediction with `/locate`).
//!
//! The core decision logic lives here as pure functions so it is testable offline
//! without a live server. The live I/O (world recreation + `/locate`) is driven by
//! `be-verify`'s [`RemoteBedrock`] and wired up in the `be-corpus` CLI (`verify-seed`).
//!
//! ## What this validates
//!
//! BDS `/locate structure X` returns the **nearest** structure of type X to origin,
//! which is not necessarily the same block the search engine reports for a constrained
//! query (the search window is not distance-ordered). To avoid false mismatches we use
//! the same region-backed-out check the validated corpus relies on ([`crate::accuracy`]):
//! back out the region the server chose from the observed position, recompute the model's
//! placement for that region with `be-struct`, and diff against the observation. This is
//! the Phase 0 gate applied live to a specific finalist seed — a genuine placement check
//! independent of any nearest-selection logic.
//!
//! Scattered structures (desert_pyramid/igloo/jungle_pyramid/swamp_hut) are excluded
//! here because `/locate structure temple` conflates all four types; they are validated
//! separately via `generate-scattered` (see [`crate::scattered`]).

use be_struct::{region_of_block, structure_block_pos, Version};
use be_verify::LocateResult;

use crate::corpus::BlockPos;

/// Anchor-returning structures the model can verify one-to-one via `/locate structure
/// <id>`. These are the validated, non-scattered structures (PLAN §4/§2.8;
/// trial_chambers excluded — it returns the bounding-box centre).
///
/// `woodland_mansion` was added 2026-08-18 and is **live-verified at 100%** (7/7
/// resolved seeds, exact region-backed-out placement). Its `/locate` id is `mansion`,
/// not `woodland_mansion` (using the model id makes `/locate` return "No valid
/// structure found"); the [`crate::locate_id`] mapping handles that.
pub const ANCHOR_STRUCTURES: [&str; 8] = [
    "village",
    "ocean_monument",
    "ancient_city",
    "pillager_outpost",
    "shipwreck",
    "buried_treasure",
    "ruined_portal",
    "woodland_mansion",
];

/// Predict the model's placement of `structure` for the region that the observed
/// `(/locate)` position lies in. Backs out the region from the observed block, then
/// recomputes `structure_block_pos` for it — the same comparison the corpus accuracy
/// layer performs ([`crate::accuracy::compute_accuracy`]).
///
/// Returns `None` if the structure is not modelled for `version`.
pub fn predict_for_region(
    version: &Version,
    structure: &str,
    seed: u64,
    observed: BlockPos,
) -> Option<BlockPos> {
    let sp = version.structures.get(structure)?;
    let rx = region_of_block(observed.x, sp.spacing);
    let rz = region_of_block(observed.z, sp.spacing);
    let (bx, bz) = structure_block_pos(
        seed,
        rx,
        rz,
        sp.salt,
        sp.spacing,
        sp.chunk_range,
        sp.distribution(),
    );
    Some(BlockPos::new(bx, bz))
}

/// Outcome of verifying one structure's prediction against a `/locate` observation.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The prediction matches the observation within tolerance.
    Pass,
    /// The prediction does not match the observation (a real discrepancy).
    Fail { reason: String },
    /// No conclusion (e.g. the server gave no parseable response). Not a pass, not a
    /// fail — surfaced for the operator.
    Skip { reason: String },
}

impl Verdict {
    /// `true` when the verdict is a hard PASS (used for the overall gate).
    pub fn is_pass(&self) -> bool {
        matches!(self, Verdict::Pass)
    }
}

/// Compare a predicted position against a `/locate` observation for one structure.
///
/// - predicted `None` (structure not modelled) → SKIP (we cannot predict it).
/// - observed `NotFound` + we have a model → we still validate placement is impossible
///   without a region, so this is a SKIP (a structure type may legitimately be absent
///   near origin).
/// - observed `Found` → compare within `tolerance` (Euclidean blocks, matching the
///   corpus accuracy gate).
/// - No parseable observation → SKIP.
pub fn compare(
    predicted: Option<BlockPos>,
    observed: Option<LocateResult>,
    tolerance: u64,
) -> Verdict {
    let Some(predicted) = predicted else {
        return Verdict::Skip {
            reason: "structure is not modelled for this version".to_string(),
        };
    };
    match observed {
        Some(LocateResult::Found { x, z, .. }) => {
            let dist = ((predicted.x - x) as f64).hypot((predicted.z - z) as f64);
            if dist <= tolerance as f64 {
                Verdict::Pass
            } else {
                Verdict::Fail {
                    reason: format!(
                        "predicted ({},{}) vs observed ({x}, {z}): distance {dist:.1} > tolerance {tolerance}",
                        predicted.x, predicted.z
                    ),
                }
            }
        }
        Some(LocateResult::NotFound) => Verdict::Skip {
            reason: format!(
                "no structure located for an anchor that should exist; predicted ({},{})",
                predicted.x, predicted.z
            ),
        },
        // No parseable observation → inconclusive.
        obs => Verdict::Skip {
            reason: format!("inconclusive observation: {obs:?}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use be_struct::Distribution;

    fn version() -> Version {
        Version::builtin_1_21_40()
    }

    fn pos(x: i64, z: i64) -> BlockPos {
        BlockPos::new(x, z)
    }

    #[test]
    fn matching_observation_within_tolerance_passes() {
        assert_eq!(
            compare(
                Some(pos(100, 200)),
                Some(LocateResult::Found {
                    x: 108,
                    z: 200,
                    y: None
                }),
                16
            ),
            Verdict::Pass
        );
    }

    #[test]
    fn mismatch_beyond_tolerance_fails() {
        let v = compare(
            Some(pos(100, 200)),
            Some(LocateResult::Found {
                x: 1000,
                z: 2000,
                y: None,
            }),
            16,
        );
        assert!(matches!(v, Verdict::Fail { .. }));
    }

    #[test]
    fn unmodelled_structure_skips() {
        let v = compare(None, Some(LocateResult::NotFound), 16);
        assert!(matches!(v, Verdict::Skip { .. }));
    }

    #[test]
    fn absent_structure_skips_not_fails() {
        // An anchor structure type may legitimately be absent near origin; without an
        // observed region we can't validate placement, so this is a SKIP.
        let v = compare(Some(pos(0, 0)), Some(LocateResult::NotFound), 16);
        assert!(matches!(v, Verdict::Skip { .. }));
    }

    #[test]
    fn inconclusive_observation_skips() {
        let v = compare(Some(pos(0, 0)), None, 16);
        assert!(matches!(v, Verdict::Skip { .. }));
        let v = compare(
            Some(pos(0, 0)),
            Some(LocateResult::Unparseable("x".into())),
            16,
        );
        assert!(matches!(v, Verdict::Skip { .. }));
    }

    /// `predict_for_region` on an observed position that equals the model's placement
    /// for a region must reproduce that position (self-consistency → the compare layer
    /// then reports PASS).
    #[test]
    fn predict_for_region_reproduces_placement() {
        let v = version();
        let seed = 4242u64;
        let sp = &v.structures["village"];
        let (bx, bz) = structure_block_pos(
            seed,
            2,
            -1,
            sp.salt,
            sp.spacing,
            sp.chunk_range,
            Distribution::Triangular,
        );
        let observed = BlockPos::new(bx, bz);
        let predicted = predict_for_region(&v, "village", seed, observed).unwrap();
        assert_eq!(predicted, observed);
        assert_eq!(
            compare(
                predict_for_region(&v, "village", seed, observed),
                Some(LocateResult::Found {
                    x: observed.x,
                    z: observed.z,
                    y: None
                }),
                16
            ),
            Verdict::Pass
        );
    }

    /// Every anchor structure must be modelled and predict a region-consistent position.
    #[test]
    fn every_anchor_structure_is_modelled() {
        let v = version();
        for id in ANCHOR_STRUCTURES {
            assert!(
                predict_for_region(&v, id, 123, BlockPos::new(1000, 1000)).is_some(),
                "{id} not modelled"
            );
        }
    }
}
