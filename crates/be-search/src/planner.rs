//! Join planner — selectivity estimation and ordering (§3.1).
//!
//! Rarest-first ordering generalises into join ordering: estimate `P(pass)` per
//! variable (`P(structure in window | spacing) × P(biome gate)`) and bind the most
//! selective variable first. This *is* rarest-first, extended to a graph.
//!
//! The planner also picks the **adaptive mode** (exhaustive vs satisficing) and reports
//! which one is running, so the UI never implies completeness the engine can't deliver
//! (§3.1).

use crate::ir::{Anchor, Query, VarKind};

/// How complete the search is (§3.1). Never let the UI imply completeness the engine
/// cannot deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 1–2 variables, origin-anchored: exhaustive sweep of all 2³² low halves.
    /// Complete over the structural subspace.
    Exhaustive,
    /// 3+ variables / relational edges: satisficing — sample low32 until enough hits.
    /// **No completeness guarantee.**
    Satisficing,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Exhaustive => "exhaustive",
            Mode::Satisficing => "satisficing",
        }
    }
}

/// A single variable's estimated selectivity factor (smaller = rarer = bind earlier).
#[derive(Debug, Clone, Copy)]
struct Selectivity {
    var_idx: usize,
    /// Lower = more selective (rarer).
    score: f64,
}

/// A compiled execution plan.
#[derive(Debug, Clone)]
pub struct Plan {
    /// Join order: variable indices, most selective first.
    pub order: Vec<usize>,
    pub mode: Mode,
}

/// Estimate the selectivity (rarity) of each variable and produce a join order.
///
/// `seed` is not needed for selectivity (rarity is a per-structure constant derived
/// from spacing and biome-gate size), so this is deterministic per query.
pub fn plan(query: &Query) -> Plan {
    let mut sel: Vec<Selectivity> = (0..query.vars.len())
        .map(|i| Selectivity {
            var_idx: i,
            score: variable_selectivity(query, i),
        })
        .collect();

    // Stable sort by score ascending (most selective first). Ties keep declaration order.
    sel.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let order: Vec<usize> = sel.iter().map(|s| s.var_idx).collect();
    let mode = choose_mode(query, &order);
    Plan { order, mode }
}

/// Selectivity score for a variable. Lower = rarer. Uses spacing (rarer = larger
/// spacing) and biome-gate size (fewer acceptable biomes = rarer).
fn variable_selectivity(query: &Query, var_idx: usize) -> f64 {
    let var = &query.vars[var_idx];
    match &var.kind {
        VarKind::BiomePresence { .. } => 0.5, // moderate; biome presence is common
        VarKind::Structure(structure) => {
            let v = be_struct::Version::builtin_1_21_40();
            let s = v.structures.get(structure);
            // Rarity score: smaller = rarer = bind earlier.
            //
            //   score = len_acceptable_biomes / spacing
            //
            // Larger spacing → smaller score (rarer). Fewer acceptable biomes → smaller
            // score (rarer). Default len = 1 when there is no gate (a gated structure is
            // rarer than an ungated one of the same spacing).
            let spacing = s.map(|s| s.spacing.max(1) as f64).unwrap_or(32.0);
            let len = match &var.biome_gate {
                Some(names) if !names.is_empty() => names.len().max(1) as f64,
                _ => 1.0,
            };
            len / spacing
        }
    }
}

/// Decide exhaustive vs satisficing (§3.1 table).
fn choose_mode(query: &Query, order: &[usize]) -> Mode {
    let n = order.len();
    let has_relational = query.edges.iter().any(|e| {
        matches!((e.a, e.b), (Anchor::Var(_), Anchor::Var(_)))
    });
    if n <= 2 && !has_relational {
        Mode::Exhaustive
    } else {
        Mode::Satisficing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parse;

    #[test]
    fn rarest_structure_binds_first() {
        // woodland_mansion (spacing 80) is rarer than village (spacing 34), so it
        // should bind first.
        let q = parse(
            "\
village v1 @origin <= 800
woodland_mansion x1 @origin >= 3000
",
        )
        .unwrap();
        let p = plan(&q);
        assert_eq!(p.order.len(), 2);
        // The mansion (x1) is index 1 and should come first.
        assert_eq!(p.order[0], 1);
        assert_eq!(p.order[1], 0);
    }

    #[test]
    fn small_origin_anchored_query_is_exhaustive() {
        let q = parse(
            "\
village v1 @origin <= 800
",
        )
        .unwrap();
        let p = plan(&q);
        assert_eq!(p.mode, Mode::Exhaustive);
    }

    #[test]
    fn relational_query_is_satisficing() {
        let q = parse(
            "\
village v1 @origin <= 800
desert_pyramid t1 @v1 in 600..1200
",
        )
        .unwrap();
        let p = plan(&q);
        // A relative edge between two vars → satisficing (no completeness guarantee).
        assert_eq!(p.mode, Mode::Satisficing);
    }

    #[test]
    fn three_vars_is_satisficing() {
        let q = parse(
            "\
village v1 @origin <= 800
desert_pyramid t1 @origin <= 2000
swamp_hut s1 @origin <= 3000
",
        )
        .unwrap();
        let p = plan(&q);
        assert_eq!(p.mode, Mode::Satisficing);
    }

    #[test]
    fn mode_names_are_honest() {
        assert_eq!(Mode::Exhaustive.as_str(), "exhaustive");
        assert_eq!(Mode::Satisficing.as_str(), "satisficing");
    }
}
