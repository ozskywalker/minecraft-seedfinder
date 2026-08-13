//! Phase B — biome resolution over the high 32 seed bits (§3.1).
//!
//! Phase A (in [`crate::executor`]) produces **structural** candidates: for each low-32
//! seed value it binds every variable's position from the structure RNG alone. But the
//! low 32 bits do not determine the biome; the full 64-bit seed does. Phase B therefore
//! sweeps the **high 32 bits** for each structural candidate, builds the full seed
//! `(high32 << 32) | low32`, and asks cubiomes at each bound position:
//!
//! - **Per-structure biome gate** — a structure only generates if the biome at its
//!   anchor is in its acceptable set (the structure's natural gate from the version
//!   table, intersected with any user `biome=` gate).
//! - **Biome presence probe** — a standalone "biome within radius" node: at least one
//!   sampled point inside the window must be one of the requested biomes.
//!
//! Only full seeds that pass every biome constraint are emitted.
//!
//! ## Completeness honesty
//!
//! Sweeping all 2³² high halves per candidate is infeasible wholesale, so Phase B is
//! **satisficing** by default: it tries `high_lo..high_hi` and stops after
//! `max_per_candidate` matches. Absence of results ≠ no such seed. Callers must report
//! the active mode (§3.1).

use std::collections::HashSet;

use be_biome::{BiomeGate, BiomeIdMap, BiomeQuery, CubiomesQuery};

use crate::executor::{Candidate, Pos};
use crate::ir::{Anchor, Query, VarKind};

/// A resolved acceptable-biome set for one variable: `None` = unconstrained.
type GateIds = Option<HashSet<u16>>;

/// Phase B resolver. Owns a single reused `CubiomesQuery` (re-seeded per full seed)
/// so sweeping many high-32 values does not allocate a generator each time.
pub struct BiomeEngine<'a> {
    query: &'a Query,
    map: &'a BiomeIdMap,
    mc: i32,
    cubiomes: CubiomesQuery,
}

impl<'a> BiomeEngine<'a> {
    pub fn new(query: &'a Query, map: &'a BiomeIdMap, mc: i32) -> Self {
        let cubiomes = CubiomesQuery::new(mc, 0);
        BiomeEngine {
            query,
            map,
            mc,
            cubiomes,
        }
    }

    /// Whether a full 64-bit seed passes every biome constraint given the bound
    /// positions. Independent of any planner/executor ordering.
    pub fn passes(&mut self, seed: u64, positions: &[Pos]) -> bool {
        self.cubiomes.set_seed(seed);
        for (i, var) in self.query.vars.iter().enumerate() {
            match &var.kind {
                VarKind::Structure(_) => {
                    if let Some(ids) = self.gate_ids(i) {
                        let p = positions[i];
                        let Some(id) = self.cubiomes.biome_id_at(p.x as i32, p.z as i32) else {
                            return false;
                        };
                        if !ids.contains(&id) {
                            return false;
                        }
                    }
                }
                VarKind::BiomePresence { biomes } => {
                    if !self.presence_passes(i, biomes, positions) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// **Invariant re-check** (§5): independently verify a candidate's biome
    /// constraints, with no reference to the planner/executor that produced it. Pairs
    /// with [`crate::Engine::verify`] (structural edges) to form the full safety net.
    pub fn verify(&mut self, cand: &Candidate) -> bool {
        self.passes(cand.seed, &cand.positions)
    }

    /// Resolve the acceptable biome ids for a structure variable:
    /// `natural_gate ∩ user_gate`. `None` when the structure is unconstrained (e.g.
    /// village has an empty natural gate and no user gate).
    fn gate_ids(&self, var_idx: usize) -> GateIds {
        let var = &self.query.vars[var_idx];
        let VarKind::Structure(structure) = &var.kind else {
            return None;
        };
        let version = crate::Version::builtin_1_21_40();
        let natural = be_biome::structure_gate(self.map, &version, structure);
        let user = var
            .biome_gate
            .as_ref()
            .map(|names| BiomeGate::from_names(self.map, names));

        match (natural, user) {
            (Some(n), Some(u)) => {
                let inter: HashSet<u16> = n.ids().intersection(u.ids()).copied().collect();
                if inter.is_empty() {
                    None
                } else {
                    Some(inter)
                }
            }
            (Some(n), None) if !n.is_unconstrained() => Some(n.ids().clone()),
            (None, Some(u)) if !u.is_unconstrained() => Some(u.ids().clone()),
            _ => None, // unconstrained
        }
    }

    /// A biome-presence probe passes if any sampled point within the window is one of
    /// the requested biomes.
    fn presence_passes(&mut self, var_idx: usize, biomes: &[String], positions: &[Pos]) -> bool {
        // Anchor = the other endpoint of the probe's defining edge; radius = that
        // edge's max. Use the largest-radius incident edge.
        let mut anchor = Pos::origin();
        let mut radius = 0u32;
        let mut found_edge = false;
        for e in self.query.edges_incident(var_idx) {
            if let Some(other) = self.query.other_anchor(e, var_idx) {
                anchor = match other {
                    Anchor::Origin => Pos::origin(),
                    Anchor::Var(j) => positions[j],
                };
                radius = radius.max(e.max);
                found_edge = true;
            }
        }
        if !found_edge || radius == 0 {
            return false;
        }

        let wanted: HashSet<u16> = biomes
            .iter()
            .flat_map(|b| self.map.resolve_alias(b).iter().copied())
            .collect();
        if wanted.is_empty() {
            return false;
        }

        // Sample the disk at quart (1:4) resolution, coarse enough to stay cheap.
        let r = radius as i64;
        let step = 4i64;
        let mut dz = -r;
        while dz <= r {
            let mut dx = -r;
            while dx <= r {
                if dx * dx + dz * dz <= r * r {
                    let id = self
                        .cubiomes
                        .biome_id_at((anchor.x + dx) as i32, (anchor.z + dz) as i32);
                    if let Some(id) = id {
                        if wanted.contains(&id) {
                            return true;
                        }
                    }
                }
                dx += step;
            }
            dz += step;
        }
        false
    }

    /// Phase B over a slice of structural candidates. For each, sweep the high 32 bits
    /// in `high_lo..high_hi`, emit every full seed that passes, up to
    /// `max_per_candidate` (satisficing). `max_per_candidate == 0` means no cap.
    pub fn resolve_biomes(
        &mut self,
        candidates: &[Candidate],
        high_lo: u32,
        high_hi: u32,
        max_per_candidate: usize,
    ) -> Vec<Candidate> {
        let mut out = Vec::new();
        for cand in candidates {
            let low = cand.seed & 0xFFFF_FFFF;
            let mut emitted = 0usize;
            for high in high_lo..high_hi {
                let full = ((high as u64) << 32) | low;
                if self.passes(full, &cand.positions) {
                    out.push(Candidate {
                        seed: full,
                        positions: cand.positions.clone(),
                    });
                    emitted += 1;
                    if max_per_candidate != 0 && emitted >= max_per_candidate {
                        break;
                    }
                }
            }
        }
        out
    }

    /// The cubiomes MC version this engine uses.
    pub fn mc(&self) -> i32 {
        self.mc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parse;
    use crate::planner::plan;
    use crate::Engine;

    // Structural Phase A engine builder that shares the builtin version/map.
    fn structural(dsl: &str) -> (Query, crate::Version) {
        (parse(dsl).unwrap(), crate::Version::builtin_1_21_40())
    }

    /// A gate that definitely accepts: query a structure whose bound position we force,
    /// then check gate_ids directly.
    #[test]
    fn gate_ids_resolve_natural_gate() {
        let (query, _) = structural("desert_pyramid t1 @origin <= 2000, biome=desert");
        let map = be_biome::builtin_biome_map();
        let eng = BiomeEngine::new(&query, &map, cubiomes_sys::mc_latest());
        let ids = eng.gate_ids(0).expect("desert gate resolves");
        // desert = 2
        assert!(ids.contains(&2));
        assert!(!ids.contains(&1)); // plains
    }

    #[test]
    fn village_has_no_gate() {
        let (query, _) = structural("village v1 @origin <= 800");
        let map = be_biome::builtin_biome_map();
        let eng = BiomeEngine::new(&query, &map, cubiomes_sys::mc_latest());
        assert!(eng.gate_ids(0).is_none(), "village is unconstrained");
    }

    /// End-to-end: a `biome=desert` gate must only emit seeds whose desert pyramid
    /// actually lands on desert. We verify each emitted candidate independently via a
    /// fresh biome engine (the invariant re-check).
    #[test]
    fn biome_gate_filters_candidates() {
        let dsl = "desert_pyramid t1 @origin <= 4000, biome=desert";
        let (query, version) = structural(dsl);
        let plan = plan(&query);
        let engine = Engine {
            query: &query,
            version: &version,
            plan: &plan,
        };
        // Phase A over a modest low-32 range.
        let structural_cands = engine.search_range(0, 2000);
        let map = be_biome::builtin_biome_map();
        let mc = cubiomes_sys::mc_latest();
        let mut biome_engine = BiomeEngine::new(&query, &map, mc);

        // Without a gate filter (high32 sweep that only checks edges), we should see
        // structural candidates. Then the gate filter should be a subset and every
        // emitted full seed must actually be in desert.
        let filtered = biome_engine.resolve_biomes(&structural_cands, 0, 20, 0);
        let mut verify = BiomeEngine::new(&query, &map, mc);
        for cand in &filtered {
            assert!(
                verify.passes(cand.seed, &cand.positions),
                "emitted candidate must pass its own biome gate"
            );
            // The bound desert pyramid position must report desert (2).
            let p = cand.positions[0];
            let id = verify.cubiomes.biome_id_at(p.x as i32, p.z as i32).unwrap();
            assert_eq!(
                id, 2,
                "desert pyramid at ({},{}) must be on desert",
                p.x, p.z
            );
        }
    }

    /// Biome-presence probe: "swamp within 400 of origin". Emitted seeds must have
    /// swamp (6) somewhere in the window.
    #[test]
    fn biome_presence_probe_works() {
        let dsl = "biome swamp1 @origin <= 400";
        let (query, version) = structural(dsl);
        let plan = plan(&query);
        let engine = Engine {
            query: &query,
            version: &version,
            plan: &plan,
        };
        let structural_cands = engine.search_range(0, 500);
        let map = be_biome::builtin_biome_map();
        let mc = cubiomes_sys::mc_latest();
        let mut biome_engine = BiomeEngine::new(&query, &map, mc);
        let filtered = biome_engine.resolve_biomes(&structural_cands, 0, 10, 0);

        let mut verify = BiomeEngine::new(&query, &map, mc);
        for cand in &filtered {
            assert!(
                verify.passes(cand.seed, &cand.positions),
                "emitted presence candidate must pass"
            );
        }
    }

    /// Full seed reconstruction: `(high<<32)|low` and the low-32 structure positions
    /// are unchanged by high bits. Verify via the structural engine itself: evaluating
    /// the full seed must bind the same positions as evaluating the low word.
    #[test]
    fn high32_does_not_change_structure_positions() {
        let (query, version) = structural("village v1 @origin <= 1000");
        let plan = plan(&query);
        let engine = Engine {
            query: &query,
            version: &version,
            plan: &plan,
        };
        let cands = engine.search_range(0, 50);
        assert!(
            !cands.is_empty(),
            "village within 1000 of origin for some seed"
        );
        for cand in &cands {
            let low = cand.seed & 0xFFFF_FFFF;
            let high = 0xABCD_0000u32;
            let full = ((high as u64) << 32) | low;
            let full_cand = engine
                .evaluate_seed(full)
                .expect("full seed binds same structure");
            assert_eq!(full_cand.positions, cand.positions);
        }
    }
}
