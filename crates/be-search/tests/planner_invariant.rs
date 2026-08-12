//! Planner invariant (PLAN §5 "Planner"): join order must not change the *result set*,
//! only the speed. For small queries we assert the planned nested-loop search and a
//! naive brute-force reference search return identical seed sets.
//!
//! The brute-force reference binds variables in **declared** order with a simple
//! full-window scan (not the planner's rarest-first order), so any difference in the
//! result set must come from the planner's reordering — which is exactly the invariant
//! under test.
//!
//! This is also the §5 "Invariant" safety net at the integration level — every emitted
//! result is re-checked against its own constraints independent of the planner.

use be_search::{parse, plan, Anchor, Engine, Pos, Query, VarKind, Version};

/// A naive reference evaluator using **declared** order and a plain ±R region scan.
/// Its result set is correct regardless of ordering; comparing it against the planned
/// search catches any planner-induced change to the result set.
fn brute_force(query: &Query, version: &Version, seed: u64, radius_regions: i64) -> bool {
    let n = query.vars.len();
    let mut positions: Vec<Option<Pos>> = vec![None; n];
    let mut memo = std::collections::HashMap::new();
    rec_brute(
        query,
        version,
        seed,
        0,
        &mut positions,
        &mut memo,
        radius_regions,
    )
}

fn rec_brute(
    query: &Query,
    version: &Version,
    seed: u64,
    var_idx: usize,
    positions: &mut Vec<Option<Pos>>,
    memo: &mut std::collections::HashMap<(String, i64, i64), Pos>,
    radius_regions: i64,
) -> bool {
    if var_idx == query.vars.len() {
        return verify_all(query, positions);
    }
    let var = &query.vars[var_idx];
    let VarKind::Structure(structure) = &var.kind else {
        positions[var_idx] = Some(Pos::origin());
        let ok = rec_brute(query, version, seed, var_idx + 1, positions, memo, radius_regions);
        if !ok {
            positions[var_idx] = None;
        }
        return ok;
    };
    let s = &version.structures[structure];

    for rx in -radius_regions..=radius_regions {
        for rz in -radius_regions..=radius_regions {
            let key = (structure.clone(), rx, rz);
            let p = *memo.entry(key).or_insert_with(|| {
                let (bx, bz) = be_struct::placement::structure_block_pos_streaming(
                    seed,
                    rx,
                    rz,
                    s.salt,
                    s.spacing,
                    s.chunk_range,
                    s.distribution(),
                );
                Pos { x: bx, z: bz }
            });
            // Check all edges from this var to already-bound anchors (declared order).
            if !satisfies_edges(query, var_idx, &p, positions) {
                continue;
            }
            positions[var_idx] = Some(p);
            if rec_brute(query, version, seed, var_idx + 1, positions, memo, radius_regions) {
                return true;
            }
        }
    }
    positions[var_idx] = None;
    false
}

fn satisfies_edges(query: &Query, var_idx: usize, p: &Pos, positions: &[Option<Pos>]) -> bool {
    for e in query.edges_incident(var_idx) {
        let other = match query.other_anchor(e, var_idx) {
            Some(Anchor::Origin) => Pos::origin(),
            Some(Anchor::Var(o)) => match positions[o] {
                Some(q) => q,
                None => continue,
            },
            None => continue,
        };
        if !e.contains(p.dist_to(&other)) {
            return false;
        }
    }
    true
}

fn verify_all(query: &Query, positions: &[Option<Pos>]) -> bool {
    for e in &query.edges {
        let pa = match e.a {
            Anchor::Origin => Pos::origin(),
            Anchor::Var(j) => positions[j].expect("bound"),
        };
        let pb = match e.b {
            Anchor::Origin => Pos::origin(),
            Anchor::Var(j) => positions[j].expect("bound"),
        };
        if !e.contains(pa.dist_to(&pb)) {
            return false;
        }
    }
    true
}

fn engine_for(dsl: &str) -> (Query, Version) {
    (parse(dsl).unwrap(), Version::builtin_1_21_40())
}

fn planned_set(dsl: &str, lo: u32, hi: u32) -> std::collections::HashSet<u64> {
    let (query, version) = engine_for(dsl);
    let plan = plan(&query);
    let engine = Engine {
        query: &query,
        version: &version,
        plan: &plan,
    };
    engine
        .search_range(lo, hi)
        .into_iter()
        .map(|c| c.seed)
        .collect()
}

/// For a small seed range, the planner-driven search and the brute-force reference
/// must agree on which seeds satisfy the query.
#[test]
fn planned_search_matches_brute_force() {
    let dsl = "\
village v1 @origin <= 800
desert_pyramid t1 @v1 in 600..1200
";
    let (query, version) = engine_for(dsl);
    let planned = planned_set(dsl, 0, 300);

    let brute: std::collections::HashSet<u64> = (0..300u32)
        .filter(|&low| brute_force(&query, &version, low as u64, 8))
        .map(|low| low as u64)
        .collect();

    assert_eq!(planned, brute, "planned and brute-force result sets must agree");
}

/// Three-variable chain with relative edges only — same invariant.
#[test]
fn three_var_chain_planned_matches_brute_force() {
    let dsl = "\
desert_pyramid a @origin <= 1500
swamp_hut b @a in 400..1000
jungle_pyramid c @b in 400..1000
";
    let (query, version) = engine_for(dsl);
    let planned = planned_set(dsl, 0, 120);

    let brute: std::collections::HashSet<u64> = (0..120u32)
        .filter(|&low| brute_force(&query, &version, low as u64, 8))
        .map(|low| low as u64)
        .collect();

    assert_eq!(planned, brute, "planned and brute-force result sets must agree");
}

/// Every emitted planned result passes the independent verify() re-check.
#[test]
fn emitted_results_pass_invariant_recheck() {
    let (query, version) = engine_for("village v1 @origin <= 800");
    let plan = plan(&query);
    let engine = Engine {
        query: &query,
        version: &version,
        plan: &plan,
    };
    for cand in engine.search_range(0, 300) {
        assert!(engine.verify(&cand), "candidate must satisfy its own edges");
    }
}
