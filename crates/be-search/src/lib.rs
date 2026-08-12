//! `be-search` — the query engine (§3).
//!
//! Turns a constraint-graph [`Query`] (from either the route builder or the text DSL)
//! into a search over Bedrock world seeds. Pipeline (§3.1):
//!
//! 1. **Feasibility pre-check** ([`feasibility`]) — static analysis proving impossible
//!    queries instantly, with the *reason*.
//! 2. **Join planner** ([`planner`]) — rarest-first ordering + honest adaptive mode.
//! 3. **Executor** ([`executor`]) — nested-loop bind-and-backtrack structural sweep
//!    over the low 32 seed bits, with per-seed memoisation and a rayon-parallel sweep.
//!
//! Phase B (biome resolution over the high 32 bits) and Phase C (BDS verification) are
//! wired on top of this crate by the caller; this crate owns the structural Phase A.

pub mod dsl;
pub mod executor;
pub mod feasibility;
pub mod ir;
pub mod planner;

pub use dsl::{parse, serialize, DslError};
pub use executor::{Candidate, Engine, Pos};
pub use feasibility::{check, Feasibility};
pub use ir::{Anchor, Edge, Query, Var, VarKind};
pub use planner::{plan, Mode, Plan};

/// Re-export of the version table type used to build an engine.
pub use be_struct::Version;
