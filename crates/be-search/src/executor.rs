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

use be_rng::BATCH_LANES;
use be_struct::placement::{structure_block_pos_batched, structure_block_pos_streaming};
use be_struct::region::floor_div;
use be_struct::{Structure, Version};

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
///
/// Keyed by a borrowed structure name (borrowed from the query/version, which outlive
/// the seed's evaluation) so the hot path performs **no `String` allocation** per
/// lookup — the `String`-keyed alternative cost a heap alloc on every region probe.
#[derive(Default)]
struct SeedMemo<'s> {
    // key: (structure name, rx, rz) → block pos.
    map: HashMap<(&'s str, i64, i64), Pos>,
}

/// Compute the block position of a structure in a region, with memoisation.
///
/// `name` (the version-table key, borrowed from the query/version) and the resolved
/// `structure` params are supplied by the caller — the caller hoists the
/// `version.structures` lookup out of the per-region loop, so this hot path does no
/// map lookup and no allocation on the memo-miss path.
fn region_block_pos<'s>(
    seed: u64,
    name: &'s str,
    s: &Structure,
    rx: i64,
    rz: i64,
    memo: &mut SeedMemo<'s>,
) -> Pos {
    if let Some(p) = memo.map.get(&(name, rx, rz)) {
        return *p;
    }
    let dist = s.distribution();
    let (bx, bz) =
        structure_block_pos_streaming(seed, rx, rz, s.salt, s.spacing, s.chunk_range, dist);
    let p = Pos { x: bx, z: bz };
    memo.map.insert((name, rx, rz), p);
    p
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
        let n = self.query.vars.len();
        // Reuse the per-seed buffers across the whole sweep so no heap allocation
        // happens per seed (the hot path). `evaluate_seed` alone allocates a fresh
        // positions Vec + memo HashMap for every seed, which is pure overhead.
        let mut positions = vec![None; n];
        let mut memo: SeedMemo<'a> = SeedMemo::default();
        for low in start..end {
            let seed = low as u64;
            if let Some(cand) = self.evaluate_seed_buffered(seed, &mut positions, &mut memo) {
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

    /// **SIMD-batched sweep** (PLAN §6) for the common exhaustive shape: a single
    /// structure variable anchored to origin (1 var, structure kind). Processes
    /// `BATCH_LANES` consecutive seeds at once via [`structure_block_pos_batched`],
    /// which is faster than the per-seed path because the batched MT init chains
    /// vectorize.
    ///
    /// Returns the **same result set as [`search_range`]** for any query that matches
    /// this shape (proven by test). For any other query shape it transparently falls
    /// back to the scalar [`search_range`], so it is always a safe drop-in.
    pub fn search_range_batched(&self, start: u32, end: u32) -> Vec<Candidate> {
        // Applicability: exactly one variable of structure kind (⇒ every edge is to
        // origin, since there are no other variables to anchor to).
        if self.query.vars.len() != 1 {
            return self.search_range(start, end);
        }
        let var = &self.query.vars[0];
        let structure = match &var.kind {
            VarKind::Structure(s) => s,
            _ => return self.search_range(start, end),
        };
        let Some(st) = self.version.structures.get(structure) else {
            return self.search_range(start, end);
        };
        let dist = st.distribution();

        // Origin-anchored window (same for every seed in the sweep).
        let none_pos = [None; 1];
        let Some(window) = self.region_window(0, &none_pos) else {
            return Vec::new();
        };

        let mut out = Vec::new();
        let mut low = start;
        while low < end {
            // Lanes 0..valid_lanes are the seeds of this batch that fall in [start,end).
            let valid_lanes = ((end as u64 - low as u64) as usize).min(BATCH_LANES);
            // Collect per-lane so we can emit in ascending seed order (matching the
            // scalar sweep's deterministic order), regardless of region-completion order.
            let mut by_lane: [Option<Candidate>; BATCH_LANES] = std::array::from_fn(|_| None);
            let mut remaining = valid_lanes;
            for &(rx, rz) in &window {
                if remaining == 0 {
                    break;
                }
                let pos_batch = structure_block_pos_batched(
                    low as u64,
                    rx,
                    rz,
                    st.salt,
                    st.spacing,
                    st.chunk_range,
                    dist,
                );
                for (lane, slot) in by_lane.iter_mut().enumerate().take(valid_lanes) {
                    if slot.is_some() {
                        continue;
                    }
                    let p = Pos {
                        x: pos_batch[lane].0,
                        z: pos_batch[lane].1,
                    };
                    // First region whose position satisfies the origin constraint —
                    // exactly when the scalar `bind` would stop for this seed.
                    if !self.satisfies(0, &p, &none_pos) {
                        continue;
                    }
                    let seed = low.wrapping_add(lane as u32) as u64;
                    let cand = Candidate {
                        seed,
                        positions: vec![p],
                    };
                    if self.verify(&cand) {
                        *slot = Some(cand);
                        remaining -= 1;
                    }
                }
            }
            for slot in by_lane.iter_mut().take(valid_lanes) {
                if let Some(c) = slot.take() {
                    out.push(c);
                }
            }
            low = low.wrapping_add(BATCH_LANES as u32);
        }
        out
    }

    /// Streaming sweep over `[start, end)`: invoke `visit` once per verified
    /// structural candidate as it is found, without collecting into a `Vec`. This is
    /// the seam the server uses to stream results over SSE as they are produced
    /// (§3.1 "results go to the UI over SSE as found — never block on a completed
    /// sweep").
    ///
    /// Sequential and deterministic; the callback is never invoked from a parallel
    /// thread, so callers may safely accumulate into a non-threadsafe structure.
    pub fn search_range_visit<F>(&self, start: u32, end: u32, mut visit: F)
    where
        F: FnMut(&Candidate),
    {
        let n = self.query.vars.len();
        let mut positions = vec![None; n];
        let mut memo: SeedMemo<'a> = SeedMemo::default();
        for low in start..end {
            let seed = low as u64;
            if let Some(cand) = self.evaluate_seed_buffered(seed, &mut positions, &mut memo) {
                if self.verify(&cand) {
                    visit(&cand);
                }
            }
        }
    }

    /// Evaluate a single seed's structural binding, in the planned join order.
    /// Returns the bound positions if every variable binds, else `None`.
    pub fn evaluate_seed(&self, seed: u64) -> Option<Candidate> {
        let n = self.query.vars.len();
        let mut positions: Vec<Option<Pos>> = vec![None; n];
        let mut memo: SeedMemo<'a> = SeedMemo::default();
        self.evaluate_seed_buffered(seed, &mut positions, &mut memo)
    }

    /// `evaluate_seed` with caller-supplied, reusable buffers. The buffers are reset in
    /// place (no reallocation), which is what lets a whole sweep avoid per-seed heap
    /// churn. Results are identical to `evaluate_seed`.
    fn evaluate_seed_buffered(
        &self,
        seed: u64,
        positions: &mut Vec<Option<Pos>>,
        memo: &mut SeedMemo<'a>,
    ) -> Option<Candidate> {
        for slot in positions.iter_mut() {
            *slot = None;
        }
        memo.map.clear();
        if self.bind(seed, 0, positions, memo) {
            Some(Candidate {
                seed,
                positions: positions.iter().map(|p| p.expect("bound")).collect(),
            })
        } else {
            None
        }
    }

    /// Recursive nested-loop binding over `plan.order[idx..]`.
    fn bind<'s>(
        &self,
        seed: u64,
        idx: usize,
        positions: &mut Vec<Option<Pos>>,
        memo: &mut SeedMemo<'s>,
    ) -> bool
    where
        'a: 's,
    {
        if idx == self.plan.order.len() {
            return true; // all bound
        }
        let var_idx = self.plan.order[idx];
        let var = &self.query.vars[var_idx];

        // Candidate region window = intersection of constraints on already-bound vars.
        let Some(window) = self.region_window(var_idx, positions) else {
            return false;
        };

        // Resolve the structure's placement params once per variable, not once per
        // region — the structure never changes across the window, and the BTreeMap
        // lookup would otherwise repeat for every region probe.
        let resolved_structure = match &var.kind {
            VarKind::Structure(s) => match self.version.structures.get(s) {
                Some(st) => Some((s.as_str(), st)),
                None => return false, // unknown structure → this var can never bind
            },
            VarKind::BiomePresence { .. } => None,
        };

        for (rx, rz) in window {
            // Biome-presence probes are resolved in Phase B; structurally they
            // bind at any position within the window. Bind at window centre for
            // structural purposes (Phase B re-checks). For now, treat as satisfied
            // by any region — emit centre.
            let Some((name, structure)) = resolved_structure else {
                positions[var_idx] = Some(Pos::origin());
                if self.bind(seed, idx + 1, positions, memo) {
                    return true;
                }
                positions[var_idx] = None;
                return false;
            };

            let p = region_block_pos(seed, name, structure, rx, rz, memo);

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
            let max = self
                .query
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

    /// The SIMD-batched sweep must return the **identical result set** to the scalar
    /// sweep for single-structure origin-anchored queries, across structures, ranges,
    /// and seed ranges (including a range that doesn't start at 0).
    #[test]
    fn batched_equals_scalar_for_single_var_origin() {
        for (dsl, lo, hi) in [
            ("village v1 @origin <= 800", 0u32, 1000u32),
            ("village v1 @origin <= 800", 12345u32, 13000u32),
            ("village v1 @origin in 500..1200", 0u32, 2000u32),
            ("desert_pyramid d1 @origin <= 1500", 0u32, 2000u32),
            ("ocean_monument m1 @origin in 2000..3000", 0u32, 2000u32),
        ] {
            let e = engine(dsl);
            let scalar = e.search_range(lo, hi);
            let batched = e.search_range_batched(lo, hi);
            assert_eq!(
                batched, scalar,
                "batched sweep diverged from scalar for {dsl:?} over [{lo},{hi})"
            );
            // Every emitted candidate independently satisfies its own edges.
            for c in &batched {
                assert!(e.verify(c), "candidate must satisfy its own edges");
            }
        }
    }

    /// For any non-single-var shape the batched sweep must transparently fall back to
    /// the scalar result.
    #[test]
    fn batched_falls_back_for_other_shapes() {
        // Multi-variable query → fallback.
        let e = engine(
            "\
village v1 @origin <= 800
desert_pyramid t1 @v1 in 600..1200
",
        );
        let scalar = e.search_range(0, 400);
        let batched = e.search_range_batched(0, 400);
        assert_eq!(batched, scalar, "multi-var must fall back to scalar");
    }

    #[test]
    fn verify_rejects_fake_result() {
        // Build a candidate that violates its own edge and confirm verify() catches it.
        let e = engine("village v1 @origin <= 800");
        let bad = Candidate {
            seed: 0,
            positions: vec![Pos { x: 100_000, z: 0 }],
        };
        assert!(
            !e.verify(&bad),
            "verify must reject an out-of-range candidate"
        );
    }

    #[test]
    fn distance_matches_euclidean() {
        assert_eq!(Pos { x: 3, z: 4 }.dist_to(&Pos::origin()), 5);
        assert_eq!(Pos { x: 0, z: 0 }.dist_to(&Pos { x: 0, z: 0 }), 0);
    }

    #[test]
    fn memo_caches_region_positions() {
        let version = Version::builtin_1_21_40();
        let s = &version.structures["village"];
        let mut memo = SeedMemo::default();
        let p1 = region_block_pos(42, "village", s, 1, 1, &mut memo);
        let p2 = region_block_pos(42, "village", s, 1, 1, &mut memo);
        assert_eq!(p1, p2);
        assert_eq!(memo.map.len(), 1);
    }
}
