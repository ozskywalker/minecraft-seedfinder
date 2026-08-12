//! Version tables as data (§2.5, §3).
//!
//! Structure parameters are data, not code: a `versions/*.json` file per supported
//! version. This keeps the crate pluggable (a new Bedrock release is a new JSON file)
//! and lets corpus regression fail loudly when Mojang silently changes a parameter.
//!
//! Only `1.21.x` is populated in v1, per the settled decision in §1.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::placement::Distribution;

/// A single structure's placement parameters for one version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Structure {
    /// MT19937 seeding salt (added into the region seed).
    pub salt: u32,
    /// Region spacing, in chunks.
    pub spacing: u32,
    /// Placement range within the region, in chunks.
    pub chunk_range: u32,
    /// `"linear"` (2 draws) or `"triangular"` (4 draws).
    pub distribution: String,
    /// Biome gates: the structure may only generate in these biomes.
    #[serde(default)]
    pub biomes: Vec<String>,
    /// Other structures that share this placement slot (shared salt + spacing).
    #[serde(default)]
    pub shares_slot_with: Vec<String>,
    /// Provenance + confidence (PLAN §2.7 policy: constants are facts, expression is
    /// not copied).
    pub provenance: String,
    pub confidence: String,
}

impl Structure {
    /// `separation = spacing - chunk_range` (§2.5).
    pub fn separation(&self) -> u32 {
        self.spacing - self.chunk_range
    }

    pub fn distribution(&self) -> Distribution {
        match self.distribution.as_str() {
            "linear" => Distribution::Linear,
            "triangular" => Distribution::Triangular,
            other => panic!("unknown distribution {other:?} for {self:?}"),
        }
    }
}

/// A full version table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Version {
    /// Human version string, e.g. "1.21.40".
    pub version: String,
    /// Bit width of world seeds (64 since 1.18.30).
    pub seed_bits: u32,
    /// Free-text note about which real server version validated these params.
    #[serde(default)]
    pub validated_against: String,
    pub structures: BTreeMap<String, Structure>,
}

impl Version {
    /// Parse a version table from JSON text.
    pub fn from_json(json: &str) -> Result<Version, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Parse from a file path.
    pub fn load(path: &str) -> Result<Version, Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(path)?;
        Ok(Version::from_json(&json)?)
    }

    /// The built-in 1.21.x table, embedded from `versions/1.21.40.json` at compile
    /// time (the path is relative to this source file: ../../../versions).
    pub fn builtin_1_21_40() -> Version {
        Version::from_json(include_str!("../../../versions/1.21.40.json"))
            .expect("embedded 1.21.40.json must parse")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_version_parses() {
        let v = Version::builtin_1_21_40();
        assert_eq!(v.version, "1.21.40");
        assert_eq!(v.seed_bits, 64);
        // Core structures present.
        for key in [
            "desert_pyramid",
            "village",
            "ocean_monument",
            "woodland_mansion",
        ] {
            assert!(v.structures.contains_key(key), "missing {key}");
        }
    }

    #[test]
    fn shared_salt_slot_is_consistent() {
        let v = Version::builtin_1_21_40();
        let scattered = [
            "desert_pyramid",
            "igloo",
            "jungle_pyramid",
            "swamp_hut",
        ];
        let params = scattered.map(|k| &v.structures[k]);
        // All four share salt 14357617 and spacing 32 (PLAN §2.5).
        for s in &params {
            assert_eq!(s.salt, 14357617);
            assert_eq!(s.spacing, 32);
        }
        // The slot-sharing references are mutual.
        for k in scattered {
            let s = &v.structures[k];
            for other in &s.shares_slot_with {
                assert!(scattered.contains(&other.as_str()), "{other} not in scattered set");
                assert!(v.structures[other].shares_slot_with.contains(&k.to_string()));
            }
        }
    }

    #[test]
    fn separation_is_spacing_minus_chunk_range() {
        let v = Version::builtin_1_21_40();
        assert_eq!(v.structures["village"].separation(), 34 - 26);
        assert_eq!(v.structures["desert_pyramid"].separation(), 32 - 24);
        assert_eq!(v.structures["woodland_mansion"].separation(), 80 - 60);
    }

    #[test]
    fn distribution_parses() {
        let v = Version::builtin_1_21_40();
        assert_eq!(
            v.structures["desert_pyramid"].distribution(),
            Distribution::Linear
        );
        assert_eq!(
            v.structures["village"].distribution(),
            Distribution::Triangular
        );
    }

    #[test]
    fn every_structure_has_provenance_and_confidence() {
        let v = Version::builtin_1_21_40();
        for (k, s) in &v.structures {
            assert!(!s.provenance.trim().is_empty(), "{k} missing provenance");
            assert!(!s.confidence.trim().is_empty(), "{k} missing confidence");
        }
    }
}
