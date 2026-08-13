//! `be-struct` — Bedrock structure placement.
//!
//! Pure Rust reimplementation of the region-seed formula and structure placement math
//! from §2.4 of PLAN.md, plus version tables as data (§2.5 / §3). This crate must be
//! exact about the two silent-corruption traps the plan calls out:
//!
//! - **Negative floor-division** for `regX = floorDiv(chunkX, spacing)` (Rust's `/`
//!   truncates; the game floors).
//! - **Exact draw order** for the RNG (linear = 2 draws, triangular = 4 draws in a
//!   specific order).

#[cfg(test)]
mod golden;
pub mod placement;
pub mod region;
pub mod version;

pub use placement::{structure_block_pos, Distribution};
pub use region::{floor_div, region_of_block, region_seed};
pub use version::{Structure, Version};
