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
//! parity is validated by the agreement layer ([`grid`]) against real BDS `/locate
//! biome` output. **As of 2026-08-12 this gate is GREEN:** cubiomes matches the real
//! Bedrock server (both 1.21.40 and 1.26.43) at **100%** for the surface biomes in the
//! corpus, after fixing a y/z argument-order bug in the cubiomes bridge (see
//! `cubiomes-sys`). See PLAN § "Current status".
//!
//! ## ✅ Empirically: cubiomes matches the live BDS servers
//!
//! Phase 3 validation compares cubiomes' predicted biome at each coordinate where the
//! real server's `/locate biome` reported a biome. With the bridge fixed, agreement is
//! **100%** on both `fixtures/biome-corpus-1.21.40.json` (captured against 1.26.43)
//! and `fixtures/biome-corpus-1.21.40.bds.json` (captured against the 1.21.40
//! validation container). Biome results may be presented as Bedrock-accurate for the
//! surface biomes in this corpus.
//!
//! Earlier "18.2% agreement" / RED-gate claims were an artifact of a bug in
//! `cubiomes-sys/src/bridge.c`: `getBiomeAt`'s `y` and `z` arguments were swapped, so
//! every surface query returned `deep_dark` (the deep-cave biome). This is fixed and
//! regression-tested.

pub mod gate;
pub mod grid;
pub mod map;
pub mod query;

pub use gate::{structure_gate, BiomeGate};
pub use grid::{compute_agreement, AgreementReport, BiomeGrid};
pub use map::{builtin_biome_map, BiomeIdMap};
pub use query::{BiomeQuery, CubiomesQuery};
