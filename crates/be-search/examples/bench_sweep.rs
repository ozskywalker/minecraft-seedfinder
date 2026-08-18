//! Performance harness for the seed-lookup sweep (Phase A), scalar vs SIMD-batched.
//!
//! Run with: `cargo run -p be-search --release --example bench_sweep`
//!
//! Compares `Engine::search_range` (per-seed) against `Engine::search_range_batched`
//! (SIMD-batched) for single-structure origin-anchored queries. The two must return
//! identical result sets (tested), so the difference here is pure speed.
//!
//! `--check` runs a representative release-mode sweep and exits non-zero if the batched
//! path is not materially faster than scalar — the CI regression guard (PLAN §6 / #4).
//! The floor is deliberately generous (real speedup is ~3x) so it is robust to CI noise
//! while still catching a catastrophic regression (e.g. the batched path silently
//! becoming scalar, ~1.0x).

use std::hint::black_box;
use std::time::Instant;

use be_search::{parse, plan, Engine, Version};

/// Minimum speedup for the `--check` guard. Real batched is ~3x; we require only
/// 1.15x (a value real builds clear with huge margin) so a genuine "no longer
/// batched" regression (≈1.0x) fails while CI noise cannot trip it.
const CHECK_FLOOR: f64 = 1.15;

fn bench(dsl: &str, lo: u32, hi: u32) -> (usize, f64, usize, f64) {
    let query = Box::leak(Box::new(parse(dsl).unwrap()));
    let version = Box::leak(Box::new(Version::builtin_1_21_40()));
    let plan = Box::leak(Box::new(plan(query)));
    let engine = Engine {
        query,
        version,
        plan,
    };
    let seeds = (hi - lo) as f64;

    black_box(engine.search_range(lo, lo + (hi - lo) / 100));
    black_box(engine.search_range_batched(lo, lo + (hi - lo) / 100));

    let t0 = Instant::now();
    let hits = engine.search_range(lo, hi);
    let t1 = t0.elapsed().as_secs_f64();
    black_box(&hits);

    let t2 = Instant::now();
    let bhits = engine.search_range_batched(lo, hi);
    let t3 = t2.elapsed().as_secs_f64();
    black_box(&bhits);

    assert_eq!(hits, bhits, "scalar and batched sweeps must agree");
    (hits.len(), seeds / t1 / 1e6, bhits.len(), seeds / t3 / 1e6)
}

fn main() {
    let check = std::env::args().any(|a| a == "--check");
    let mut worst = f64::INFINITY;
    for (label, dsl, lo, hi) in [
        (
            "village <= 800",
            "village v1 @origin <= 800",
            0u32,
            4_000_000u32,
        ),
        (
            "village <= 300 (sparse)",
            "village v1 @origin <= 300",
            0,
            4_000_000,
        ),
        (
            "desert_pyramid <= 1500",
            "desert_pyramid d1 @origin <= 1500",
            0,
            4_000_000,
        ),
        (
            "woodland_mansion <= 3000 (sparse)",
            "woodland_mansion m1 @origin <= 3000",
            0,
            4_000_000,
        ),
    ] {
        let (hits, scalar_mps, _, batched_mps) = bench(dsl, lo, hi);
        let speedup = batched_mps / scalar_mps;
        worst = worst.min(speedup);
        println!(
            "{label}: {hits} hits | scalar {scalar_mps:.2} M/s | batched {batched_mps:.2} M/s | \
             speedup {speedup:.2}x"
        );
    }
    if check {
        if worst < CHECK_FLOOR {
            eprintln!(
                "FAIL: worst speedup {worst:.2}x is below the {CHECK_FLOOR}x guard floor \
                 (batched path appears to have regressed)."
            );
            std::process::exit(1);
        }
        println!("OK: worst speedup {worst:.2}x >= {CHECK_FLOOR}x floor");
    }
}
