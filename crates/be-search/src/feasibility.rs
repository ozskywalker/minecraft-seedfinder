//! Feasibility pre-check — static analysis of the constraint graph (§3.1).
//!
//! Run before any search, in milliseconds, to reject queries that are *provably*
//! impossible regardless of seed — and report *why*, not just "no results". This is a
//! headline feature: with a graph query language users write impossible queries for
//! non-obvious reasons, and proving it instantly beats failing after a ten-minute
//! search.
//!
//! Checks implemented:
//!
//! - **Shared-slot conflicts** (§2.5) — two structures that share one placement slot
//!   (e.g. desert pyramid & jungle temple) can never be closer than one region apart.
//! - **Triangle inequality** over distance edges — `d(A,B) ≤ 500 ∧ d(B,C) ≤ 500 ∧
//!   d(A,C) ≥ 2000` is unsatisfiable regardless of seed.
//! - **Spacing-vs-radius bounds** — a structure with spacing S (in blocks) cannot
//!   appear twice within less than its spacing.

use crate::ir::{Anchor, Edge, Query, VarKind};

/// The outcome of a feasibility check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feasibility {
    /// The query is satisfiable in principle; no static contradiction found.
    Ok,
    /// The query is provably unsatisfiable; `reasons` explains why.
    Infeasible(Vec<String>),
}

/// Run the full feasibility pre-check over a query.
pub fn check(query: &Query) -> Feasibility {
    let mut reasons = Vec::new();
    check_shared_slot(query, &mut reasons);
    check_triangle_inequality(query, &mut reasons);
    check_spacing_bounds(query, &mut reasons);

    if reasons.is_empty() {
        Feasibility::Ok
    } else {
        Feasibility::Infeasible(reasons)
    }
}

/// For each pair of variables, if they're both structure vars that share a placement
/// slot AND an edge constrains them closer than one region apart, it's impossible.
fn check_shared_slot(query: &Query, reasons: &mut Vec<String>) {
    let n = query.vars.len();
    for i in 0..n {
        for j in (i + 1)..n {
            let (VarKind::Structure(si), VarKind::Structure(sj)) =
                (&query.vars[i].kind, &query.vars[j].kind)
            else {
                continue;
            };
            if si == sj {
                continue; // same structure type — handled by spacing check
            }
            if !share_slot(si, sj) {
                continue;
            }
            // They share a slot. Find any edge between them; if its max < one region
            // apart (spacing * 16 blocks), it's impossible. Use the shared spacing.
            for e in query.edges.iter() {
                if edge_is_between(e, i, j) {
                    let spacing_blocks = shared_spacing_blocks(si, sj);
                    if e.max < spacing_blocks {
                        reasons.push(format!(
                            "'{}' and '{}' share one placement slot (§2.5) and cannot be closer than {} blocks (one region apart), but the query requires distance ≤ {}",
                            query.vars[i].name,
                            query.vars[j].name,
                            spacing_blocks,
                            e.max
                        ));
                    }
                }
            }
        }
    }
}

/// Triangle inequality over the distance graph: for any three anchors A,B,C, if the
/// max of A–B plus max of B–C is strictly less than the min of A–C, impossible.
///
/// We only check over variable anchors (origin has no meaningful "min" constraint
/// beyond the others). This is a sound over-approximation — it catches the classic
/// impossible-chain shape without needing seed data.
#[allow(clippy::needless_range_loop)]
fn check_triangle_inequality(query: &Query, reasons: &mut Vec<String>) {
    let n = query.vars.len();
    // Build a max/min matrix.
    // max_dist[i][j] = smallest known upper bound (min over edges of max)
    // min_dist[i][j] = largest known lower bound (max over edges of min)
    let inf = u32::MAX;
    let mut maxd = vec![vec![inf; n]; n];
    let mut mind = vec![vec![0u32; n]; n];
    for e in query.edges.iter() {
        match (e.a, e.b) {
            (Anchor::Var(i), Anchor::Var(j)) if i != j => {
                maxd[i][j] = maxd[i][j].min(e.max);
                maxd[j][i] = maxd[i][j];
                mind[i][j] = mind[i][j].max(e.min);
                mind[j][i] = mind[i][j];
            }
            _ => {}
        }
    }
    for a in 0..n {
        for k in 0..n {
            for c in 0..n {
                if a == k || k == c || a == c {
                    continue;
                }
                if maxd[a][k] != inf && maxd[k][c] != inf && mind[a][c] > 0 {
                    // Can we prove d(a,c) must exceed mind[a][c] while d(a,k)+d(k,c)
                    // is already too small? By triangle inequality:
                    // d(a,c) <= d(a,k) + d(k,c) <= maxd[a][k] + maxd[k][c]
                    // But we require d(a,c) >= mind[a][c]. If mind[a][c] >
                    // maxd[a][k] + maxd[k][c], then the requirement contradicts the
                    // upper bound implied by the two edges.
                    let upper = maxd[a][k].saturating_add(maxd[k][c]);
                    if mind[a][c] > upper {
                        reasons.push(format!(
                            "triangle-inequality violation: by way of '{}' the distance between '{}' and '{}' is at most {upper}, but the query requires at least {}",
                            query.vars[k].name,
                            query.vars[a].name,
                            query.vars[c].name,
                            mind[a][c]
                        ));
                    }
                }
            }
        }
    }
}

/// Spacing-vs-radius bounds: a structure with spacing S blocks cannot appear twice in
/// the query within less than S blocks of each other. Two vars of the same structure
/// type with an edge max < spacing_blocks is impossible (each region hosts at most one).
fn check_spacing_bounds(query: &Query, reasons: &mut Vec<String>) {
    let n = query.vars.len();
    for i in 0..n {
        for j in (i + 1)..n {
            let (VarKind::Structure(si), VarKind::Structure(sj)) =
                (&query.vars[i].kind, &query.vars[j].kind)
            else {
                continue;
            };
            if si != sj {
                continue;
            }
            for e in query.edges.iter() {
                if edge_is_between(e, i, j) {
                    // Spacing is in chunks; convert to blocks (x16).
                    let spacing_blocks = spacing_blocks(si);
                    if e.max < spacing_blocks {
                        reasons.push(format!(
                            "'{}' and '{}' are the same structure type with spacing {} blocks; two of them cannot be closer than one region apart, but the query requires distance ≤ {}",
                            query.vars[i].name, query.vars[j].name, spacing_blocks, e.max
                        ));
                    }
                }
            }
        }
    }
}

fn edge_is_between(e: &Edge, i: usize, j: usize) -> bool {
    (e.a == Anchor::Var(i) && e.b == Anchor::Var(j))
        || (e.a == Anchor::Var(j) && e.b == Anchor::Var(i))
}

/// Whether two structure keys share a placement slot. Both directions: A's
/// shares_slot_with lists B, or B's lists A. (The version table lists them mutually,
/// but be defensive.)
fn share_slot(a: &str, b: &str) -> bool {
    let v = be_struct::Version::builtin_1_21_40();
    match (v.structures.get(a), v.structures.get(b)) {
        (Some(sa), Some(sb)) => {
            sa.shares_slot_with.iter().any(|x| x == b) || sb.shares_slot_with.iter().any(|x| x == a)
        }
        _ => false,
    }
}

/// Blocks in one region for the shared slot (spacing * 16). All scattered structures
/// share spacing 32 → 512 blocks.
fn shared_spacing_blocks(a: &str, b: &str) -> u32 {
    let v = be_struct::Version::builtin_1_21_40();
    let spacing = v
        .structures
        .get(a)
        .or_else(|| v.structures.get(b))
        .map(|s| s.spacing)
        .unwrap_or(32);
    spacing * 16
}

fn spacing_blocks(s: &str) -> u32 {
    let v = be_struct::Version::builtin_1_21_40();
    v.structures
        .get(s)
        .map(|s| s.spacing * 16)
        .unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parse;

    #[test]
    fn feasible_query_is_ok() {
        let q = parse(
            "\
village v1 @origin <= 800
desert_pyramid t1 @v1 in 600..1200, biome=desert
",
        )
        .unwrap();
        assert_eq!(check(&q), Feasibility::Ok);
    }

    #[test]
    fn shared_slot_conflict_is_detected() {
        // Desert pyramid and jungle temple share one slot every 512 blocks; requiring
        // them within 400 blocks is provably impossible (PLAN §2.5 example).
        let q = parse(
            "\
desert_pyramid a @origin <= 2000
jungle_pyramid b @a <= 400
",
        )
        .unwrap();
        match check(&q) {
            Feasibility::Infeasible(reasons) => {
                assert!(
                    reasons
                        .iter()
                        .any(|r| r.contains("share one placement slot")),
                    "reasons: {reasons:?}"
                );
            }
            Feasibility::Ok => panic!("shared-slot conflict not detected"),
        }
    }

    #[test]
    fn shared_slot_far_apart_is_ok() {
        // Same two structures, but required far apart (> 512) — feasible.
        let q = parse(
            "\
desert_pyramid a @origin <= 5000
jungle_pyramid b @a in 2000..3000
",
        )
        .unwrap();
        assert_eq!(check(&q), Feasibility::Ok);
    }

    #[test]
    fn triangle_inequality_violation_is_detected() {
        // d(A,B) <= 500, d(B,C) <= 500, d(A,C) >= 2000  → impossible (PLAN §3.1).
        let q = parse(
            "\
village a @origin <= 1000
desert_pyramid b @a <= 500
swamp_hut c @b <= 500
",
        )
        .unwrap();
        // Add the A–C >= 2000 constraint by hand (DSL only supports per-var edges; we
        // build it directly).
        let mut q = q;
        q.edges.push(crate::ir::Edge {
            a: Anchor::Var(0),
            b: Anchor::Var(2),
            min: 2000,
            max: u32::MAX,
        });
        match check(&q) {
            Feasibility::Infeasible(reasons) => {
                assert!(
                    reasons.iter().any(|r| r.contains("triangle-inequality")),
                    "reasons: {reasons:?}"
                );
            }
            Feasibility::Ok => panic!("triangle inequality not detected"),
        }
    }

    #[test]
    fn same_structure_too_close_is_detected() {
        // Two villages closer than one region (spacing 34 → 544 blocks) apart: impossible.
        let q = parse(
            "\
village a @origin <= 1000
village b @a <= 300
",
        )
        .unwrap();
        match check(&q) {
            Feasibility::Infeasible(reasons) => {
                assert!(
                    reasons.iter().any(|r| r.contains("same structure type")),
                    "reasons: {reasons:?}"
                );
            }
            Feasibility::Ok => panic!("spacing bound not detected"),
        }
    }

    #[test]
    fn same_structure_far_enough_is_ok() {
        let q = parse(
            "\
village a @origin <= 5000
village b @a in 3000..4000
",
        )
        .unwrap();
        assert_eq!(check(&q), Feasibility::Ok);
    }
}
