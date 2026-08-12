//! Version-aware `/locate` command generation and output parsing.
//!
//! Bedrock syntax (PLAN §4, minecraft.wiki):
//!
//! ```text
//! /locate structure <structure: Structure> [useNewChunksOnly: Boolean]
//! /locate biome <minecraft: Biome>
//! ```
//!
//! - Structure IDs take **no namespace** in Bedrock (`village`, `desert_pyramid`, …).
//! - Biome IDs require a **`minecraft:` namespace since 1.21.100** (`minecraft:plains`
//!   instead of `plains`). Command generation is version-aware.
//!
//! ## Output format (captured from real BDS 1.26.43)
//!
//! The parser is built from **real captured output** (PLAN §4), not docs. Real shapes:
//!
//! ```text
//! The nearest minecraft:village is at block 184, (y?), 296 (348 blocks away)   <- structure
//! The nearest minecraft:desert  is at block 2656, 65, -3232 (4183 blocks away) <- biome (has y)
//! No valid structure found within a reasonable distance                        <- failure
//! ```
//!
//! Note: structure output reports only `x` and `z` with a literal `(y?)`; biome
//! output includes a real `y`. This is the exact behaviour the parser must reproduce.

use serde::{Deserialize, Serialize};

/// What a `/locate` command is targeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocateKind {
    Structure,
    Biome,
}

/// A `/locate` command to send to BDS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocateCommand {
    pub kind: LocateKind,
    /// Structure: no namespace. Biome: may be bare (`plains`) or namespaced
    /// (`minecraft:plains`).
    pub id: String,
    pub use_new_chunks: bool,
}

impl LocateCommand {
    pub fn structure(id: &str) -> Self {
        LocateCommand {
            kind: LocateKind::Structure,
            id: id.to_string(),
            use_new_chunks: false,
        }
    }

    pub fn biome(id: &str) -> Self {
        LocateCommand {
            kind: LocateKind::Biome,
            id: id.to_string(),
            use_new_chunks: false,
        }
    }

    /// Render the command line.
    ///
    /// `biome_namespace_required` should be `true` for Bedrock >= 1.21.100 (biome IDs
    /// must carry the `minecraft:` namespace). Structure IDs are never namespaced.
    pub fn render(&self, biome_namespace_required: bool) -> String {
        match self.kind {
            LocateKind::Structure => {
                let mut s = format!("/locate structure {}", self.id);
                if self.use_new_chunks {
                    s.push_str(" true");
                }
                s
            }
            LocateKind::Biome => {
                let id = if biome_namespace_required && !self.id.contains(':') {
                    format!("minecraft:{}", self.id)
                } else {
                    self.id.clone()
                };
                format!("/locate biome {id}")
            }
        }
    }
}

/// A parsed `/locate` response.
///
/// `y` is `None` for structure locate (Bedrock reports `(y?)`) and `Some` for biome
/// locate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocateResult {
    /// A coordinate triple was found: `x, z` (blocks) and, for biomes, `y`.
    Found {
        x: i64,
        z: i64,
        y: Option<i64>,
    },
    /// The structure/biome could not be located.
    NotFound,
    /// Output did not match a known shape (coordinate nor failure marker).
    Unparseable(String),
}

/// Parse a single `/locate` response line, built from real BDS 1.26 captured output.
///
/// Recognises, in order:
/// 1. a full coordinate triple: `at block <x>, <y>, <z>` (biome locate) → `y=Some`;
/// 2. a structure pair: `at block <x>, (y?), <z>` → `y=None`;
/// 3. failure markers: "no valid structure found", "unable", "could not", "not
///    found", "cannot", "can't".
pub fn parse_locate_output(line: &str) -> LocateResult {
    static FULL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static PAIR: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let full = FULL.get_or_init(|| {
        regex::Regex::new(r"(?i)at block\s+(-?\d+)\s*,\s*(-?\d+)\s*,\s*(-?\d+)")
            .expect("valid full-coordinate regex")
    });
    let pair = PAIR.get_or_init(|| {
        regex::Regex::new(r"(?i)at block\s+(-?\d+)\s*,\s*\(y\?\)\s*,\s*(-?\d+)")
            .expect("valid structure-pair regex")
    });

    if let Some(c) = full.captures(line) {
        return LocateResult::Found {
            x: c[1].parse().expect("digit"),
            z: c[3].parse().expect("digit"),
            y: Some(c[2].parse().expect("digit")),
        };
    }
    if let Some(c) = pair.captures(line) {
        return LocateResult::Found {
            x: c[1].parse().expect("digit"),
            z: c[2].parse().expect("digit"),
            y: None,
        };
    }

    let lower = line.to_lowercase();
    for marker in [
        "no valid structure found",
        "unable",
        "could not",
        "not found",
        "cannot",
        "can't",
    ] {
        if lower.contains(marker) {
            return LocateResult::NotFound;
        }
    }

    LocateResult::Unparseable(line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structure_command_no_namespace() {
        assert_eq!(
            LocateCommand::structure("village").render(false),
            "/locate structure village"
        );
        assert_eq!(
            LocateCommand::structure("desert_pyramid").render(true),
            "/locate structure desert_pyramid"
        );
    }

    #[test]
    fn structure_command_new_chunks_flag() {
        let mut c = LocateCommand::structure("pillager_outpost");
        c.use_new_chunks = true;
        assert_eq!(c.render(false), "/locate structure pillager_outpost true");
    }

    #[test]
    fn biome_command_namespace_version_gating() {
        // < 1.21.100: bare id.
        assert_eq!(LocateCommand::biome("plains").render(false), "/locate biome plains");
        // >= 1.21.100: namespace required.
        assert_eq!(LocateCommand::biome("plains").render(true), "/locate biome minecraft:plains");
        // Already-namespaced ids are left alone.
        assert_eq!(
            LocateCommand::biome("minecraft:plains").render(true),
            "/locate biome minecraft:plains"
        );
    }

    #[test]
    fn parse_found_structure_no_y() {
        // Real BDS 1.26 captured output.
        let line = "The nearest minecraft:village is at block 184, (y?), 296 (348 blocks away)";
        assert_eq!(
            parse_locate_output(line),
            LocateResult::Found { x: 184, z: 296, y: None }
        );
    }

    /// Biome locate reports a real y coordinate.
    #[test]
    fn parse_found_biome_with_y() {
        let line = "The nearest minecraft:desert is at block 2656, 65, -3232 (4183 blocks away)";
        assert_eq!(
            parse_locate_output(line),
            LocateResult::Found { x: 2656, z: -3232, y: Some(65) }
        );
        let line2 = "The nearest minecraft:plains is at block -480, 63, -864 (988 blocks away)";
        assert_eq!(
            parse_locate_output(line2),
            LocateResult::Found { x: -480, z: -864, y: Some(63) }
        );
    }

    /// Negative structure coordinates parse. Real BDS 1.26 captured output.
    #[test]
    fn parse_found_negative_structure() {
        let line = "The nearest minecraft:buried_treasure is at block -440, (y?), -248 (505 blocks away)";
        assert_eq!(
            parse_locate_output(line),
            LocateResult::Found { x: -440, z: -248, y: None }
        );
    }

    /// More real structure captures: shipwreck and monument.
    #[test]
    fn parse_found_more_real_structures() {
        let line = "The nearest minecraft:shipwreck is at block 696, (y?), 520 (868 blocks away)";
        assert_eq!(
            parse_locate_output(line),
            LocateResult::Found { x: 696, z: 520, y: None }
        );
        let line = "The nearest minecraft:monument is at block 1288, (y?), 664 (1449 blocks away)";
        assert_eq!(
            parse_locate_output(line),
            LocateResult::Found { x: 1288, z: 664, y: None }
        );
    }

    /// A timestamp prefix containing numbers must not be mistaken for coordinates.
    #[test]
    fn parse_ignores_timestamp_prefix() {
        let line = "[2026-08-12 12:43:46:912 INFO] The nearest minecraft:village is at block 184, (y?), 296 (348 blocks away)";
        assert_eq!(
            parse_locate_output(line),
            LocateResult::Found { x: 184, z: 296, y: None }
        );
    }

    #[test]
    fn parse_not_found_markers() {
        for line in [
            // Real BDS 1.26 failure output.
            "No valid structure found within a reasonable distance",
            // Other markers.
            "Unable to locate the requested structure within a square of 201x201 units of the Structure's Spacing.",
            "Could not find that structure",
            "Structure not found.",
            "Cannot locate the requested biome.",
        ] {
            assert_eq!(parse_locate_output(line), LocateResult::NotFound, "line: {line}");
        }
    }

    #[test]
    fn parse_unparseable() {
        let line = "Some unrelated log line";
        assert_eq!(parse_locate_output(line), LocateResult::Unparseable(line.to_string()));
    }

    /// Every real captured line in the fixture must parse to the expected result.
    /// The fixture is the ground truth recorded from the live BDS 1.26.43 server;
    /// if this fails, the parser or the fixture drifted — do not silently fix either.
    #[test]
    fn parse_all_real_captured_lines() {
        let fixture = include_str!("../tests/fixtures/real_locate_output.txt");
        let mut found = 0;
        let mut not_found = 0;
        for raw in fixture.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            match parse_locate_output(line) {
                LocateResult::Found { .. } => found += 1,
                LocateResult::NotFound => not_found += 1,
                LocateResult::Unparseable(_) => {
                    panic!("fixture line did not parse: {line:?}");
                }
            }
        }
        assert_eq!(found, 11, "expected 11 found lines (8 structure + 2 biome + 1 timestamped)");
        assert_eq!(not_found, 1, "expected 1 not-found line");
    }
}
