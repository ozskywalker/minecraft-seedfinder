//! `be-biome` — safe biome API (PLAN §3 Phase 3).
//!
//! - [`map`] — the Bedrock & Java numeric biome ID maps as data
//!   (`versions/biomes.json`), plus alias resolution for the legacy names used by
//!   structure gates.
//! - [`query`] — a safe `BiomeQuery` trait and a `CubiomesQuery` backend over the
//!   vendored cubiomes FFI.
//! - [`gate`] — structure biome-gate evaluation (PLAN §2.6).
//! - [`grid`] — 2D biome grids and the predicted-vs-observed agreement computation
//!   that underpins LevelDB `Data3D` validation (PLAN §4).
//!
//! ## ⚠️ Honesty constraint
//!
//! [`cubiomes_query::CubiomesQuery`] returns **Java** biome IDs. Bedrock↔Java biome
//! parity is **empirically observed but not proven** (PLAN §8), and the agreement
//! layer ([`grid`]) is the mechanism that would validate it against real LevelDB
//! `Data3D` grids — data we do not have in this environment. Do not present biome
//! results as Bedrock-accurate until that gate is cleared.
//!
//! ## ⚠️ Empirically: cubiomes does NOT match the live BDS 1.26.43 server
//!
//! Phase 3 validation attempted to compare cubiomes' predicted nearest-biome position
//! against the real server's `/locate biome` output (fixture
//! `fixtures/biome-corpus-1.21.40.json`, 11 samples, seeds 0..2). Agreement was
//! **18%** (2/11). cubiomes reports `deep_dark` where the server reports `desert` and
//! `ocean`, and finds *no* desert within 3000 blocks for seed 0 where the server finds
//! one at (608, -1632).
//!
//! This is expected and unavoidable with the available server: cubiomes caps at
//! **1.21** (`MC_1_21`), but the live server is **1.26.43** — a five-version gap in
//! which Bedrock biome generation changed. cubiomes therefore **cannot validate
//! Bedrock biome output for this server version**. This gate stays RED; biome results
//! must not be presented as Bedrock-accurate until either (a) a server on a
//! cubiomes-supported version is available, or (b) cubiomes is updated. See the §8
//! open risk.

pub mod gate;
pub mod grid;
pub mod map;
pub mod query;

pub use gate::{BiomeGate, structure_gate};
pub use grid::{AgreementReport, BiomeGrid, compute_agreement};
pub use map::{BiomeIdMap, builtin_biome_map};
pub use query::{BiomeQuery, CubiomesQuery};
