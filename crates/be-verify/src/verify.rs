//! `verify` — orchestration tying the harness and `/locate` parser together.
//!
//! This is deliberately thin: it exists so that the corpus generator (Phase 2 / PLAN
//! §4) can obtain a single observed structure position from a live (fake) BDS and
//! hand it to the accuracy layer. The heavy lifting — what "prediction" means and how
//! to compare — lives in `be-corpus`.

use crate::harness::BdsHarness;
use crate::locate::{LocateCommand, LocateResult};

/// Locate a structure and return its parsed result, or `None` if the response could
/// not be parsed.
pub fn locate_structure(
    harness: &mut BdsHarness,
    id: &str,
) -> std::io::Result<Option<LocateResult>> {
    let lines = harness.command(&LocateCommand::structure(id).render(false))?;
    let parsed: Vec<LocateResult> = lines
        .iter()
        .map(|l| crate::locate::parse_locate_output(l))
        .collect();
    Ok(parsed
        .into_iter()
        .find(|r| !matches!(r, LocateResult::Unparseable(_))))
}
