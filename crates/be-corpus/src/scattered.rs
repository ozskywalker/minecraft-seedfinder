//! Shared-salt "scattered" structure set (PLAN §2.5) — desert_pyramid, igloo,
//! jungle_pyramid, swamp_hut.
//!
//! These four are a single placement slot: they all share salt `14357617`, spacing 32,
//! chunk_range 24 and the linear distribution, and `/locate structure temple` returns
//! one position per seed conflating all four types (the type is decided by biome).
//! Because they share the placement math, a single observed `temple` position validates
//! the placement of all four at once — we record that observation under each id and
//! `be-corpus report` scores each against the (identical) `be-struct` prediction.
//!
//! This is the #3 "shared-salt scattered set" validation. It was previously
//! `[UNCONFIRMED]`/`medium` in `versions/1.21.40.json`; capturing a corpus with
//! `generate-scattered` and reporting 100% upgrades that confidence.

use be_struct::{region_of_block, structure_block_pos, Version};

use crate::corpus::{BlockPos, Sample};

/// The four scattered structures that share the `temple` placement slot.
pub const SCATTERED_IDS: [&str; 4] = ["desert_pyramid", "igloo", "jungle_pyramid", "swamp_hut"];

/// The `/locate` id that resolves to the shared scattered slot.
pub const TEMPLE_LOCATE_ID: &str = "temple";

/// Turn one observed `temple` `/locate` position (from a live server) into a corpus
/// `Sample` per scattered structure id. The same observed position is recorded under
/// each id because the placement math is shared; `be-corpus report` then verifies each
/// against `be-struct`.
///
/// Structures absent from `version.structures` are skipped. Returns an empty vec if no
/// scattered id is modelled.
pub fn scattered_samples(
    version: &Version,
    version_str: &str,
    seed: u64,
    observed: BlockPos,
) -> Vec<Sample> {
    SCATTERED_IDS
        .iter()
        .filter(|id| version.structures.contains_key(**id))
        .map(|id| Sample {
            version: version_str.to_string(),
            seed,
            structure: (*id).to_string(),
            observed,
        })
        .collect()
}

/// Given an observed `temple` position, recompute each scattered id's predicted
/// position for the region the observation lies in, returning `(id, predicted)`
/// pairs. Callers use this to sanity-check that the observation backs out to a region
/// consistent with the placement model (i.e. predicted ≈ observed).
pub fn predicted_for_region(
    version: &Version,
    seed: u64,
    observed: BlockPos,
) -> Vec<(&'static str, (i64, i64))> {
    SCATTERED_IDS
        .iter()
        .filter_map(|id| {
            let sp = version.structures.get(*id)?;
            let rx = region_of_block(observed.x, sp.spacing);
            let rz = region_of_block(observed.z, sp.spacing);
            let pred = structure_block_pos(
                seed,
                rx,
                rz,
                sp.salt,
                sp.spacing,
                sp.chunk_range,
                sp.distribution(),
            );
            Some((*id, pred))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version() -> Version {
        Version::builtin_1_21_40()
    }

    #[test]
    fn shared_placement_math_is_identical_across_scattered_ids() {
        // All four ids must predict the SAME position for a given seed + region
        // (shared salt/spacing/chunk_range/distribution). Pick an arbitrary region.
        let v = version();
        let preds = predicted_for_region(&v, 12345678, BlockPos::new(1000, -2000));
        assert_eq!(preds.len(), 4, "all four scattered ids modelled");
        let first = preds[0].1;
        for (id, p) in &preds {
            assert_eq!(*p, first, "id {id} diverged from shared placement");
        }
    }

    #[test]
    fn scattered_samples_record_observation_under_each_id() {
        let v = version();
        let observed = BlockPos::new(512, 512);
        let samples = scattered_samples(&v, "1.21.40", 7, observed);
        let ids: Vec<&str> = samples.iter().map(|s| s.structure.as_str()).collect();
        assert_eq!(ids, SCATTERED_IDS);
        for s in &samples {
            assert_eq!(s.observed, observed);
            assert_eq!(s.seed, 7);
            assert_eq!(s.version, "1.21.40");
        }
    }

    /// The observed `temple` position must back out to a region whose predicted
    /// position is within one spacing grid of the observation (i.e. the observation is
    /// consistent with our placement model, not wildly off).
    #[test]
    fn observed_position_is_consistent_with_prediction() {
        let v = version();
        // Fabricate an observation that equals the shared prediction for a region.
        let seed = 4242u64;
        let sp = &v.structures["desert_pyramid"];
        let (bx, bz) = structure_block_pos(
            seed,
            2,
            -1,
            sp.salt,
            sp.spacing,
            sp.chunk_range,
            sp.distribution(),
        );
        let observed = BlockPos::new(bx, bz);
        let preds = predicted_for_region(&v, seed, observed);
        for (_id, p) in &preds {
            let dist =
                (((p.0 - observed.x) as f64).powi(2) + ((p.1 - observed.z) as f64).powi(2)).sqrt();
            assert_eq!(
                dist, 0.0,
                "prediction must equal observation for this fabricated sample"
            );
        }
    }
}
