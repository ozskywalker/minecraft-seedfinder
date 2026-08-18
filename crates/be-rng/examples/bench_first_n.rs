//! Performance harness for the streaming MT hot path, scalar vs SIMD-batched.
//!
//! Run with: `cargo run -p be-rng --release --example bench_first_n`
//!
//! Compares computing the first `n` tempered outputs for a batch of `BATCH_LANES`
//! consecutive seeds via:
//! - the scalar path (`first_n_into` once per seed), and
//! - the batched lockstep path (`first_n_batched`), which the compiler can SIMD.
//!
//! ns/seed = total time / (batch_count * BATCH_LANES).

use std::hint::black_box;
use std::time::Instant;

use be_rng::{first_n_batched, first_n_into, BATCH_LANES, STREAM_BUF};

fn bench_scalar(n: usize, batches: u64) -> f64 {
    let mut sink = 0u32;
    let mut buf = [0u32; STREAM_BUF];
    let t0 = Instant::now();
    for b in 0..batches {
        let base = (b * BATCH_LANES as u64) as u32;
        for k in 0..BATCH_LANES {
            let out = first_n_into(base.wrapping_add(k as u32), n, &mut buf);
            sink = black_box(out[0]).wrapping_add(sink);
        }
    }
    let dt = t0.elapsed().as_nanos() as f64;
    black_box(sink);
    dt / (batches as f64 * BATCH_LANES as f64)
}

fn bench_batched(n: usize, batches: u64) -> f64 {
    let mut sink = 0u32;
    let mut out = [0u32; BATCH_LANES * STREAM_BUF];
    let t0 = Instant::now();
    for b in 0..batches {
        let base = (b * BATCH_LANES as u64) as u32;
        let mut seeds = [0u32; BATCH_LANES];
        for (k, s) in seeds.iter_mut().enumerate() {
            *s = base.wrapping_add(k as u32);
        }
        first_n_batched(&seeds, n, &mut out);
        sink = black_box(out[0]).wrapping_add(sink);
    }
    let dt = t0.elapsed().as_nanos() as f64;
    black_box(sink);
    dt / (batches as f64 * BATCH_LANES as f64)
}

fn main() {
    let batches = 5_000_000u64;
    for n in [2usize, 4] {
        let s = bench_scalar(n, batches);
        let b = bench_batched(n, batches);
        println!(
            "n={n}: scalar {s:.1} ns/seed | batched {b:.1} ns/seed | speedup {:.2}x",
            s / b
        );
    }
}
