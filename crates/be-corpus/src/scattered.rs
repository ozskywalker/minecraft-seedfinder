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

/// Predict which scattered structure type occupies the temple slot at `temple`, from
/// the biome at that anchor (PLAN §2.5: the four scattered structures are a single
/// slot disambiguated purely by biome; §2.6: placement runs a biome-validity check).
///
/// `biome_at` maps `(x, z)` to a biome numeric id (e.g. cubiomes), and `resolve` maps
/// a gate biome name to the same numeric id space. Returns the scattered id whose gate
/// contains the anchor biome, or `None` if the anchor biome is in no scattered gate.
///
/// ⚠️ Honesty (PLAN §2.6, §8): the game's biome-validity check samples a *region*, not
/// necessarily the single anchor point, and its exact coordinates remain `[UNCONFIRMED]`.
/// So this prediction is a strong signal, not a proof — see `report-scattered-type`.
pub fn predict_scattered_type<B, R>(
    version: &Version,
    temple: BlockPos,
    biome_at: B,
    resolve: R,
) -> Option<&'static str>
where
    B: Fn(i64, i64) -> Option<u16>,
    R: Fn(&str) -> Option<u16>,
{
    let biome_id = biome_at(temple.x, temple.z)?;
    SCATTERED_IDS
        .iter()
        .find(|id| {
            version
                .structures
                .get(**id)
                .map(|sp| sp.biomes.iter().any(|b| resolve(b) == Some(biome_id)))
                .unwrap_or(false)
        })
        .copied()
}

/// The primary gate biome used to probe a scattered structure's type on the server via
/// `/locate biome` (the first biome in each scattered structure's gate list).
pub fn primary_gate_biome<'a>(version: &'a Version, id: &str) -> Option<&'a str> {
    version
        .structures
        .get(id)
        .and_then(|sp| sp.biomes.first())
        .map(String::as_str)
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

    /// `predict_scattered_type` maps an anchor biome to the unique scattered gate that
    /// contains it, and returns `None` for an anchor biome in no scattered gate.
    #[test]
    fn predict_scattered_type_maps_anchor_biome_to_gate() {
        let v = version();
        // Fake biome ids keyed by name; distinct per gate.
        let id = |name: &str| -> u16 {
            match name {
                "desert" => 1,
                "desert_hills" => 2,
                "desert_lakes" => 3,
                "icePlains" => 10,
                "coldTaiga" => 11,
                "jungle" => 20,
                "jungle_hills" => 21,
                "swamp" => 30,
                "swampland" => 31,
                _ => 0,
            }
        };
        // biome_at returns the id for a fake name derived from the passed position.
        let resolve = |name: &str| -> Option<u16> { Some(id(name)) };

        // Anchor in desert -> desert_pyramid.
        let biome_at = |_x: i64, _z: i64| -> Option<u16> { Some(id("desert")) };
        assert_eq!(
            predict_scattered_type(&v, BlockPos::new(0, 0), biome_at, resolve),
            Some("desert_pyramid")
        );

        // Anchor in jungle -> jungle_pyramid.
        let biome_at = |_x: i64, _z: i64| -> Option<u16> { Some(id("jungle")) };
        assert_eq!(
            predict_scattered_type(&v, BlockPos::new(0, 0), biome_at, resolve),
            Some("jungle_pyramid")
        );

        // Anchor in swamp -> swamp_hut.
        let biome_at = |_x: i64, _z: i64| -> Option<u16> { Some(id("swampland")) };
        assert_eq!(
            predict_scattered_type(&v, BlockPos::new(0, 0), biome_at, resolve),
            Some("swamp_hut")
        );

        // Anchor in a biome in no scattered gate (e.g. plains) -> None.
        let biome_at = |_x: i64, _z: i64| -> Option<u16> { Some(99) };
        assert_eq!(
            predict_scattered_type(&v, BlockPos::new(0, 0), biome_at, resolve),
            None
        );
    }

    /// `primary_gate_biome` returns the first gate biome of a scattered structure.
    #[test]
    fn primary_gate_biome_is_first_gate_entry() {
        let v = version();
        assert_eq!(primary_gate_biome(&v, "desert_pyramid"), Some("desert"));
        assert_eq!(primary_gate_biome(&v, "igloo"), Some("icePlains"));
        assert_eq!(primary_gate_biome(&v, "jungle_pyramid"), Some("jungle"));
        assert_eq!(primary_gate_biome(&v, "swamp_hut"), Some("swamp"));
        assert_eq!(primary_gate_biome(&v, "not_scattered"), None);
    }
}
