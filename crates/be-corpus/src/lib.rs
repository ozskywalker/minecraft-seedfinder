//! `be-corpus` — ground-truth corpus storage and accuracy reporting (PLAN §4).
//!
//! The plan establishes that **no usable public Bedrock seed corpus exists**, so this
//! project builds its own. This crate:
//!
//! - Defines the **corpus fixture format** (JSON) recording, per
//!   `(version, seed, structure)`, the *observed* structure position from the real
//!   game (via BDS `/locate`).
//! - Computes **per-structure, per-version accuracy** by recomputing the predicted
//!   position with `be-struct` and comparing to the observed one.
//!
//! ## How "prediction" is defined here
//!
//! BDS `/locate` returns the nearest structure of a type. Rather than require a full
//! nearest-structure search (Phase 4), we back out which region `/locate` reported
//! (`region_of_block`) and check that `be-struct`'s placement for **that region**
//! matches the game. This is exactly the Phase 0 gate: "does the generator agree with
//! the game region-by-region?", and it directly surfaces the [UNCONFIRMED]
//! anchor-vs-centre offset via the mean signed offset columns.

pub mod accuracy;
pub mod corpus;
pub mod scattered;
pub mod verify;

pub use accuracy::{
    compute_accuracy, compute_biome_agreement, AccuracyReport, BiomeAgreementReport,
    StructureAccuracy,
};
pub use be_struct::Version;
pub use corpus::{BiomeSample, BlockPos, Corpus, Sample};
pub use scattered::{
    predict_scattered_type, primary_gate_biome, scattered_samples, SCATTERED_IDS, TEMPLE_LOCATE_ID,
};
pub use verify::{compare, predict_for_region, Verdict, ANCHOR_STRUCTURES};

/// Map a model structure id to the `/locate structure <id>` id accepted by Bedrock.
///
/// Most structure ids are identical between the model and the game, but not all:
/// `woodland_mansion` is the model's id while Bedrock's `/locate` id is `mansion`
/// (using `woodland_mansion` makes `/locate` return "No valid structure found" — a
/// silent false-negative that invalidates any mansion verification built on the wrong
/// id; see PLAN §2.8).
pub fn locate_id(structure: &str) -> &str {
    match structure {
        "woodland_mansion" => "mansion",
        _ => structure,
    }
}

/// A version string parsed enough to answer "is the biome namespace required?".
///
/// Bedrock requires the `minecraft:` namespace on `/locate biome` ids since 1.21.100
/// (PLAN §4). Returns `true` for versions at or above that gate.
pub fn biome_namespace_required(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    let at = |idx: usize| {
        parts
            .get(idx)
            .and_then(|p| p.parse::<u32>().ok())
            .unwrap_or(0)
    };
    let (major, minor, patch) = (at(0), at(1), at(2));
    (major, minor, patch) >= (1, 21, 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biome_namespace_gate() {
        assert!(!biome_namespace_required("1.21.40"));
        assert!(!biome_namespace_required("1.21.99"));
        assert!(biome_namespace_required("1.21.100"));
        assert!(biome_namespace_required("1.21.120"));
        assert!(biome_namespace_required("1.22.0"));
        assert!(!biome_namespace_required("1.20.80"));
    }

    #[test]
    fn locate_id_maps_mansion() {
        assert_eq!(locate_id("woodland_mansion"), "mansion");
        // Every other anchor structure's model id is its /locate id.
        for id in crate::verify::ANCHOR_STRUCTURES {
            if id == "woodland_mansion" {
                continue;
            }
            assert_eq!(locate_id(id), id, "unexpected mapping for {id}");
        }
    }
}
