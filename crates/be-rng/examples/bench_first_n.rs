//! Performance harness for the streaming MT hot path.
//!
//! Run with: `cargo run -p be-rng --release --example bench_first_n`
//!
//! Measures the per-call cost of the structure-placement RNG primitives:
//! - `first_n_into` (zero-alloc stack-buffer streaming twist) for the n structure
//!   placement needs (2 = linear, 4 = triangular).

use std::hint::black_box;
use std::time::Instant;

use be_rng::{first_n_into, STREAM_BUF};

fn bench_first_n_into(n: usize, iters: u64) -> f64 {
    let mut buf = [0u32; STREAM_BUF];
    let mut sink = 0u32;
    let t0 = Instant::now();
    for i in 0..iters {
        let seed = i as u32;
        let out = first_n_into(seed, n, &mut buf);
        sink = black_box(out[0]).wrapping_add(sink);
    }
    let dt = t0.elapsed().as_nanos() as f64;
    black_box(sink);
    dt / iters as f64
}

fn main() {
    for n in [2usize, 4] {
        let iters = 50_000_000u64;
        let ns = bench_first_n_into(n, iters);
        println!(
            "first_n_into(seed, {n}): {ns:.1} ns/call  ({:.1} M calls/s)",
            1e3 / ns
        );
    }
}
