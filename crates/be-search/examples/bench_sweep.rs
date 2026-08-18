//! Performance harness for the seed-lookup sweep (Phase A structural search).
//!
//! Run with: `cargo run -p be-search --release --example bench_sweep`
//!
//! Measures `Engine::search_range` over a contiguous range of low-32 seed bits for a
//! representative query, i.e. the cost of "looking up" whether each seed satisfies the
//! constraints. Also times `evaluate_seed` in a tight loop to isolate the executor's
//! per-seed binding cost from the result collection/verify overhead.

use std::hint::black_box;
use std::time::Instant;

use be_search::{parse, plan, Engine, Version};

fn bench_sweep(dsl: &str, lo: u32, hi: u32) -> (usize, f64, f64, f64, usize) {
    let query = Box::leak(Box::new(parse(dsl).unwrap()));
    let version = Box::leak(Box::new(Version::builtin_1_21_40()));
    let plan = Box::leak(Box::new(plan(query)));
    let engine = Engine {
        query,
        version,
        plan,
    };
    let seeds = (hi - lo) as f64;

    // Warm up.
    black_box(engine.search_range(lo, lo + (hi - lo) / 100));

    let t0 = Instant::now();
    let hits = engine.search_range(lo, hi);
    let dt = t0.elapsed().as_secs_f64();
    black_box(&hits);

    // Raw evaluate_seed cost (no collection/verify), per seed.
    let t1 = Instant::now();
    let mut nfound = 0usize;
    for low in lo..hi {
        if black_box(engine.evaluate_seed(low as u64)).is_some() {
            nfound += 1;
        }
    }
    let dt2 = t1.elapsed().as_secs_f64();

    (
        hits.len(),
        seeds / dt / 1e6,
        dt / seeds * 1e9,
        dt2 / seeds * 1e9,
        nfound,
    )
}

fn main() {
    for (label, dsl, lo, hi) in [
        (
            "village <= 800 of origin",
            "village v1 @origin <= 800",
            0u32,
            4_000_000u32,
        ),
        (
            "village + desert_pyramid (relative)",
            "village v1 @origin <= 800\ndesert_pyramid t1 @v1 in 600..1200",
            0,
            400_000,
        ),
    ] {
        let (hits, mps, ns_sweep, ns_eval, nfound) = bench_sweep(dsl, lo, hi);
        println!(
            "{label}: sweep {hits} hits, {mps:.2} M seeds/s ({ns_sweep:.0} ns/seed); \
             raw evaluate_seed {ns_eval:.0} ns/seed ({nfound} bound in raw loop)"
        );
    }
}
