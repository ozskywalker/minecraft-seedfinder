//! Nested-loop bind-and-backtrack executor (§3.1, PLAN build order item 4).
//!
//! Implements Phase A — the structural sweep over the low 32 seed bits. For each seed:
//!
//! ```text
//! bind variables in planned order (nested-loop join):
//!   window = intersection of constraints on already-bound vars
//!   for regions overlapping window:
//!     streaming MT → offset → block pos
//!     satisfies all edges to bound vars? → bind, recurse
//!   exhausted with no binding → BACKTRACK / reject seed
//! → emit structural candidate (geometry only, all vars bound)
//! ```
//!
//! Per-seed memoisation caches (structure, region) → block position so that the same
//! region queried by multiple constraints is computed once. The executor also performs
//! an **independent invariant re-check** (§5 "Invariant") before emitting: every result
//! is verified against its own edges, separate from the planner that produced it.

use std::collections::HashMap;

use be_struct::placement::structure_block_pos_streaming;
use be_struct::region::floor_div;
use be_struct::Version;

use crate::ir::{Anchor, Query, VarKind};
use crate::planner::Plan;

/// A block position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pos {
    pub x: i64,
    pub z: i64,
}

impl Pos {
    pub fn origin() -> Pos {
        Pos { x: 0, z: 0 }
    }

    /// Euclidean distance to another position (horizontal plane), floored to blocks.
    pub fn dist_to(&self, other: &Pos) -> u32 {
        let dx = (self.x - other.x) as f64;
        let dz = (self.z - other.z) as f64;
        (dx * dx + dz * dz).sqrt() as u32
    }
}

/// A structural search result for one seed: all variables bound to positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The full 64-bit seed whose low 32 bits produced this structural geometry.
    pub seed: u64,
    /// Per-variable bound position (index aligned to `Query.vars`).
    pub positions: Vec<Pos>,
}

/// A per-(structure,region) memo cache, scoped to a single seed's evaluation.
#[derive(Default)]
struct SeedMemo {
    // key: (structure_key, rx, rz) → block pos.
    map: HashMap<(String, i64, i64), Pos>,
}

/// Compute the block position of a structure in a region, with memoisation.
fn region_block_pos(
    version: &Version,
    seed: u64,
    structure: &str,
    rx: i64,
    rz: i64,
    memo: &mut SeedMemo,
) -> Option<Pos> {
    if let Some(p) = memo.map.get(&(structure.to_string(), rx, rz)) {
        return Some(*p);
    }
    let s = version.structures.get(structure)?;
    let dist = s.distribution();
    let (bx, bz) = structure_block_pos_streaming(
        seed,
        rx,
        rz,
        s.salt,
        s.spacing,
        s.chunk_range,
        dist,
    );
    let p = Pos { x: bx, z: bz };
    memo.map.insert((structure.to_string(), rx, rz), p);
    Some(p)
}

/// The engine: holds the query, version and plan, and can search seed ranges.
pub struct Engine<'a> {
    pub query: &'a Query,
    pub version: &'a Version,
    pub plan: &'a Plan,
}

impl<'a> Engine<'a> {
    /// Search a contiguous range of low32 seeds `[start, end)` and return all
    /// structural candidates (every seed that binds every variable), each re-checked
    /// against its own constraints.
    ///
    /// The sweep is over the **low 32 bits** (Phase A): structural geometry depends only
    /// on `seed & 0xFFFF_FFFF`, so we iterate `start..end` as the low word.
    pub fn search_range(&self, start: u32, end: u32) -> Vec<Candidate> {
        let mut out = Vec::new();
        for low in start..end {
            let seed = low as u64;
            if let Some(cand) = self.evaluate_seed(seed) {
                if self.verify(&cand) {
                    out.push(cand);
                }
            }
        }
        out
    }

    /// Rayon-parallel sweep over `[start, end)`, preserving nothing about order (the
    /// result set must be identical to the sequential sweep — tested).
    pub fn search_range_par(&self, start: u32, end: u32) -> Vec<Candidate> {
        use rayon::prelude::*;
        (start..end)
            .into_par_iter()
            .filter_map(|low| {
                let seed = low as u64;
                let cand = self.evaluate_seed(seed)?;
                if self.verify(&cand) {
                    Some(cand)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Evaluate a single seed's structural binding, in the planned join order.
    /// Returns the bound positions if every variable binds, else `None`.
    pub fn evaluate_seed(&self, seed: u64) -> Option<Candidate> {
        let n = self.query.vars.len();
        let mut positions: Vec<Option<Pos>> = vec![None; n];
        let mut memo = SeedMemo::default();
        if self.bind(seed, 0, &mut positions, &mut memo) {
            Some(Candidate {
                seed,
                positions: positions.into_iter().map(|p| p.expect("bound")).collect(),
            })
        } else {
            None
        }
    }

    /// Recursive nested-loop binding over `plan.order[idx..]`.
    fn bind(
        &self,
        seed: u64,
        idx: usize,
        positions: &mut Vec<Option<Pos>>,
        memo: &mut SeedMemo,
    ) -> bool {
        if idx == self.plan.order.len() {
            return true; // all bound
        }
        let var_idx = self.plan.order[idx];
        let var = &self.query.vars[var_idx];

        // Candidate region window = intersection of constraints on already-bound vars.
        let Some(window) = self.region_window(var_idx, positions) else {
            return false;
        };

        for (rx, rz) in window {
            // Compute block pos (structure) for this region.
            let structure = match &var.kind {
                VarKind::Structure(s) => s,
                // Biome-presence probes are resolved in Phase B; structurally they
                // bind at any position within the window. Bind at window centre for
                // structural purposes (Phase B re-checks). For now, treat as satisfied
                // by any region — emit centre.
                VarKind::BiomePresence { .. } => {
                    positions[var_idx] = Some(Pos::origin());
                    if self.bind(seed, idx + 1, positions, memo) {
                        return true;
                    }
                    positions[var_idx] = None;
                    return false;
                }
            };

            let Some(p) = region_block_pos(self.version, seed, structure, rx, rz, memo) else {
                continue;
            };

            // Check all edges from this var to already-bound anchors.
            if !self.satisfies(var_idx, &p, positions) {
                continue;
            }

            positions[var_idx] = Some(p);
            if self.bind(seed, idx + 1, positions, memo) {
                return true;
            }
            positions[var_idx] = None;
        }

        false
    }

    /// Compute the region-coordinate window for `var_idx`, intersecting the distance
    /// constraints of all its edges to already-bound anchors. Returns `None` if the
    /// window is empty (backtrack).
    fn region_window(&self, var_idx: usize, positions: &[Option<Pos>]) -> Option<Vec<(i64, i64)>> {
        let var = &self.query.vars[var_idx];
        let (VarKind::Structure(s), _) = (&var.kind, ()) else {
            // Biome-presence probe: any region within the window; treat as origin-centred
            // region range from the max incident edge.
            let max = self.query
                .edges_incident(var_idx)
                .map(|e| e.max)
                .max()
                .unwrap_or(0) as i64;
            let spacing_blocks = 512i64;
            let r = (max / spacing_blocks) + 1;
            return Some(region_rect(-r, r, -r, r));
        };

        let spacing = self
            .version
            .structures
            .get(s)
            .map(|s| s.spacing)
            .unwrap_or(32);
        let spacing_blocks = spacing as i64 * 16;

        // Start with an unbounded range and intersect each bound-constraint's range.
        let mut rx_lo = i64::MIN / 2;
        let mut rx_hi = i64::MAX / 2;
        let mut rz_lo = i64::MIN / 2;
        let mut rz_hi = i64::MAX / 2;
        let mut has_bound = false;

        for edge in self.query.edges_incident(var_idx) {
            // Find the other anchor's position.
            let other = match self.query.other_anchor(edge, var_idx) {
                Some(Anchor::Origin) => Pos::origin(),
                Some(Anchor::Var(o)) => match positions[o] {
                    Some(p) => p,
                    None => continue, // not yet bound; skip this constraint for now
                },
                None => continue,
            };
            has_bound = true;

            // The annulus around `other` with radius edge.max (outer) bounds the region
            // search. (Min only shrinks; a superset is fine — the edge check filters.)
            let m = edge.max as i64;
            let (a_lo, a_hi) = region_range(other.x, m, spacing_blocks);
            let (b_lo, b_hi) = region_range(other.z, m, spacing_blocks);
            rx_lo = rx_lo.max(a_lo);
            rx_hi = rx_hi.min(a_hi);
            rz_lo = rz_lo.max(b_lo);
            rz_hi = rz_hi.min(b_hi);
        }

        if !has_bound {
            // No bound constraint (shouldn't happen for a pruned query); treat as 0..1
            return Some(vec![(0, 0)]);
        }
        if rx_lo > rx_hi || rz_lo > rz_hi {
            return None;
        }

        Some(region_rect(rx_lo, rx_hi, rz_lo, rz_hi))
    }

    /// Does position `p` for `var_idx` satisfy all edges to already-bound anchors?
    fn satisfies(&self, var_idx: usize, p: &Pos, positions: &[Option<Pos>]) -> bool {
        for edge in self.query.edges_incident(var_idx) {
            let other = match self.query.other_anchor(edge, var_idx) {
                Some(Anchor::Origin) => Pos::origin(),
                Some(Anchor::Var(o)) => match positions[o] {
                    Some(q) => q,
                    None => continue,
                },
                None => continue,
            };
            let d = p.dist_to(&other);
            if !edge.contains(d) {
                return false;
            }
        }
        true
    }

    /// **Invariant re-check** (§5): independently verify every emitted result against
    /// its own edges, without reference to the planner's join order. This is the safety
    /// net for the whole join-ordering layer.
    pub fn verify(&self, cand: &Candidate) -> bool {
        for (i, edge) in self.query.edges.iter().enumerate() {
            let pa = match edge.a {
                Anchor::Origin => Pos::origin(),
                Anchor::Var(j) => cand.positions[j],
            };
            let pb = match edge.b {
                Anchor::Origin => Pos::origin(),
                Anchor::Var(j) => cand.positions[j],
            };
            let d = pa.dist_to(&pb);
            if !edge.contains(d) {
                return false;
            }
            let _ = i;
        }
        true
    }
}

/// Region-coordinate range [lo, hi] (inclusive) whose region grid cells could contain
/// a block within `radius` of coordinate `center`, for a given region block-size.
fn region_range(center: i64, radius: i64, spacing_blocks: i64) -> (i64, i64) {
    // The farthest candidate block coordinate is center±radius; the region containing
    // a block at coordinate b is floorDiv(b, spacing_blocks).
    let lo = floor_div(center - radius, spacing_blocks);
    let hi = floor_div(center + radius, spacing_blocks);
    (lo, hi)
}

fn region_rect(rx_lo: i64, rx_hi: i64, rz_lo: i64, rz_hi: i64) -> Vec<(i64, i64)> {
    // Guard against absurdly large windows (pathological max distances).
    let span_x = rx_hi.saturating_sub(rx_lo).min(1_000_000);
    let span_z = rz_hi.saturating_sub(rz_lo).min(1_000_000);
    let mut out = Vec::with_capacity(((span_x + 1) * (span_z + 1)) as usize);
    for rx in rx_lo..=rx_lo + span_x {
        for rz in rz_lo..=rz_lo + span_z {
            out.push((rx, rz));
        }
    }
    out
}

/// Convenience: `dist_to` helper exported for tests.
pub fn distance(a: Pos, b: Pos) -> u32 {
    a.dist_to(&b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parse;
    use crate::planner::plan;

    fn engine(dsl: &str) -> Engine<'static> {
        // NOTE: for the tests we use a leaked Box to get 'static refs. Real callers own
        // the query/version/plan; this is a test-only convenience.
        let query = Box::leak(Box::new(parse(dsl).unwrap()));
        let version = Box::leak(Box::new(Version::builtin_1_21_40()));
        let plan = Box::leak(Box::new(plan(query)));
        Engine {
            query,
            version,
            plan,
        }
    }

    #[test]
    fn single_structure_within_radius() {
        // village within 800 of origin. Find seeds that bind.
        let e = engine("village v1 @origin <= 800");
        // Sweep a small range and verify every result independently.
        let hits = e.search_range(0, 200);
        for c in &hits {
            assert!(e.verify(c), "candidate must satisfy its own edges");
            let d = Pos::origin().dist_to(&c.positions[0]);
            assert!(d <= 800, "village at distance {d} exceeds 800");
        }
    }

    #[test]
    fn sequential_and_parallel_agree() {
        let e = engine(
            "\
village v1 @origin <= 800
desert_pyramid t1 @v1 in 600..1200
",
        );
        let seq = e.search_range(0, 400);
        let par = e.search_range_par(0, 400);
        // Parallel returns results unordered; sort both by seed for comparison.
        let mut seq = seq;
        let mut par = par;
        seq.sort_by_key(|c| c.seed);
        par.sort_by_key(|c| c.seed);
        assert_eq!(seq, par, "parallel and sequential sweeps must agree");
    }

    #[test]
    fn verify_rejects_fake_result() {
        // Build a candidate that violates its own edge and confirm verify() catches it.
        let e = engine("village v1 @origin <= 800");
        let bad = Candidate {
            seed: 0,
            positions: vec![Pos { x: 100_000, z: 0 }],
        };
        assert!(!e.verify(&bad), "verify must reject an out-of-range candidate");
    }

    #[test]
    fn distance_matches_euclidean() {
        assert_eq!(Pos { x: 3, z: 4 }.dist_to(&Pos::origin()), 5);
        assert_eq!(Pos { x: 0, z: 0 }.dist_to(&Pos { x: 0, z: 0 }), 0);
    }

    #[test]
    fn memo_caches_region_positions() {
        let version = Version::builtin_1_21_40();
        let mut memo = SeedMemo::default();
        let p1 = region_block_pos(&version, 42, "village", 1, 1, &mut memo).unwrap();
        let p2 = region_block_pos(&version, 42, "village", 1, 1, &mut memo).unwrap();
        assert_eq!(p1, p2);
        assert_eq!(memo.map.len(), 1);
    }
}
