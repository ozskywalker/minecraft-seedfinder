//! `be-rng` — Bedrock Edition's structure RNG.
//!
//! Bedrock structure placement uses the standard **MT19937** generator (the same
//! algorithm as the canonical `mt19937ar.c`), seeded with the **low 32 bits** of the
//! region seed (`mt[0] = regionSeed & 0xFFFFFFFF`). This is *not* Java Edition's
//! 48-bit LCG.
//!
//! Two facts drive the design:
//!
//! 1. **The twist is the expensive part.** Producing the first `n` tempered outputs
//!    only ever touches `mt[0 ..= n+396]`, so the full 624-word array never needs to
//!    be materialized for the small `n` (2 = linear, 4 = triangular) that structure
//!    placement uses. `first_n` computes exactly those outputs with a working set of
//!    `2(n+1)` words instead of 624.
//!
//! 2. **`mNextInt` is biased.** For non-power-of-two bounds it is plain `next() % n`
//!    with no rejection sampling. The bias must be reproduced exactly or every
//!    downstream position is wrong.

/// MT19937 constants from the canonical `mt19937ar.c`.
const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;

/// Initialization constant (`1812433253`, the "magic" from `init_genrand`).
const INIT_MAGIC: u32 = 1812433253;

/// The largest `n` for which `first_n`'s streaming twist is valid without
/// wrap-around. `N - M == 227`; for `n <= 227` the twist of index `i < n` uses
/// `mt[i+M]` and `mt[i+1]`, neither of which wraps. Structure placement needs `n <= 4`.
pub const MAX_STREAMING_N: usize = N - M;

/// One step of `init_genrand`'s recurrence: `mt[i] = (1812433253 * (mt[i-1] ^
/// (mt[i-1] >> 30)) + i) mod 2^32`.
#[inline]
fn init_step(prev: u32, i: usize) -> u32 {
    INIT_MAGIC
        .wrapping_mul(prev ^ (prev >> 30))
        .wrapping_add(i as u32)
}

/// Temper a raw MT word (the `genrand_int32` finalization).
#[inline]
fn temper(mut y: u32) -> u32 {
    y ^= y >> 11;
    y ^= (y << 7) & 0x9d2c_5680;
    y ^= (y << 15) & 0xefc6_0000;
    y ^= y >> 18;
    y
}

/// A standard full-state MT19937 (`mt19937ar.c`), provided as the authoritative
/// reference for differential testing against the streaming variants.
pub struct MersenneTwister {
    mt: [u32; N],
    index: usize,
}

impl MersenneTwister {
    /// Seed the generator. `seed` is the already-masked 32-bit value
    /// (`mt[0] = seed & 0xFFFFFFFF`).
    pub fn new(seed: u32) -> Self {
        let mut mt = [0u32; N];
        mt[0] = seed;
        // `init_step` uses wrapping (u32) arithmetic, so the canonical C `& 0xffffffff`
        // mask is implicit.
        for i in 1..N {
            mt[i] = init_step(mt[i - 1], i);
        }
        // index == N forces a twist on the first call to next_u32, mirroring how
        // mt19937ar.c treats mti == N after seeding.
        MersenneTwister { mt, index: N }
    }

    /// Advance the state one twist (a full 624-word cycle).
    fn twist(&mut self) {
        for i in 0..N - M {
            let y = (self.mt[i] & UPPER_MASK) | (self.mt[i + 1] & LOWER_MASK);
            self.mt[i] = self.mt[i + M] ^ (y >> 1) ^ if y & 1 == 1 { MATRIX_A } else { 0 };
        }
        for i in N - M..N - 1 {
            let y = (self.mt[i] & UPPER_MASK) | (self.mt[i + 1] & LOWER_MASK);
            self.mt[i] = self.mt[i + M - N] ^ (y >> 1) ^ if y & 1 == 1 { MATRIX_A } else { 0 };
        }
        let y = (self.mt[N - 1] & UPPER_MASK) | (self.mt[0] & LOWER_MASK);
        self.mt[N - 1] = self.mt[M - 1] ^ (y >> 1) ^ if y & 1 == 1 { MATRIX_A } else { 0 };
        self.index = 0;
    }

    /// Produce the next tempered 32-bit output.
    pub fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            self.twist();
        }
        let y = self.mt[self.index];
        self.index += 1;
        temper(y)
    }

    /// `mNextInt(bound)` as used by Bedrock structure placement.
    ///
    /// - Power-of-two bound: `next() & (bound - 1)`.
    /// - Otherwise: plain `next() % bound`, **no rejection sampling** (biased).
    pub fn next_int(&mut self, bound: u32) -> u32 {
        next_int(bound, self.next_u32())
    }
}

/// `mNextInt(bound, raw)` applied to an already-produced tempered value.
///
/// - Power-of-two bound: `raw & (bound - 1)`.
/// - Otherwise: plain `raw % bound`, **no rejection sampling** (biased).
///
/// This free-function form is what the streaming placement path uses, since it draws
/// raw tempered values from `first_n` and applies the bias independently.
#[inline]
pub fn next_int(bound: u32, raw: u32) -> u32 {
    debug_assert!(bound > 0);
    if bound.is_power_of_two() {
        raw & (bound - 1)
    } else {
        raw % bound
    }
}

/// Compute the first `n` tempered outputs of MT19937 seeded with `seed`, using the
/// **partial-init / streaming** twist.
///
/// Producing output `i` for `i < n <= 227` needs `mt[i]`, `mt[i+1]` and `mt[i+397]`,
/// i.e. the windows `mt[0..=n]` and `mt[397..=397+n]`. We roll the initialization
/// recurrence forward as scalars and keep only those two windows — never the full
/// 624-word array. Working set is `2(n+1)` words.
///
/// This is the highest-value optimization in the project, and the plan mandates a
/// property test proving it equals the full `MersenneTwister` for the first `n`
/// outputs across many seeds.
///
/// The zero-alloc [`first_n_into`] is the hot-path primitive (it fills a caller-owned
/// stack buffer, so the seed-lookup sweep performs no heap allocation per call); this
/// `Vec`-returning form is kept as a convenience and as the reference the property
/// tests target.
pub fn first_n(seed: u32, n: usize) -> Vec<u32> {
    let mut out = vec![0u32; n];
    first_n_into(seed, n, &mut out);
    out
}

/// The size of the fixed stack buffer [`first_n_into`] requires (one more than the
/// largest supported `n`, so window `mt[0..=n]` always fits).
pub const STREAM_BUF: usize = MAX_STREAMING_N + 1;

/// Zero-alloc streaming twist: write the first `n` tempered outputs of MT19937 seeded
/// with `seed` into `out[0..n]` and return `&out[..n]`.
///
/// `n` must be `<= MAX_STREAMING_N` and `out.len() >= n`. The caller supplies a fixed
/// stack buffer (e.g. `let mut buf = [0u32; STREAM_BUF]`), so no heap allocation
/// happens on the seed-lookup hot path — this is the difference that makes the Phase A
/// sweep fast. Results are bit-identical to [`first_n`] / the full `MersenneTwister`.
#[inline]
pub fn first_n_into(seed: u32, n: usize, out: &mut [u32]) -> &[u32] {
    assert!(
        n <= MAX_STREAMING_N,
        "first_n_into supports n <= {} (got {n})",
        MAX_STREAMING_N
    );
    assert!(out.len() >= n, "out buffer too small: len {} < n {n}", out.len());

    // Window A: mt[0..=n]; window B: mt[397..=397+n]. Both fit in fixed stack arrays;
    // only the touched entries are written, but declaring them const-sized lets the
    // compiler keep them in registers/stack with no heap traffic.
    let mut a = [0u32; MAX_STREAMING_N + 1];
    let mut b = [0u32; MAX_STREAMING_N + 1];

    a[0] = seed;
    let mut cur = seed;
    for (i, slot) in a[1..=n].iter_mut().enumerate() {
        cur = init_step(cur, i + 1);
        *slot = cur;
    }
    // `cur` is now mt[n]. Continue rolling forward to mt[397+n], capturing window B:
    // mt[397..=397+n] (b[i] == mt[397 + i]).
    for i in (n + 1)..=(n + M) {
        cur = init_step(cur, i);
        if i >= M {
            b[i - M] = cur;
        }
    }

    // Twist window A against window B to produce the first n outputs.
    for i in 0..n {
        let y = (a[i] & UPPER_MASK) | (a[i + 1] & LOWER_MASK);
        let mag = if y & 1 == 1 { MATRIX_A } else { 0 };
        let raw = b[i] ^ (y >> 1) ^ mag;
        out[i] = temper(raw);
    }
    &out[..n]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canonical `mt19937ar.c` reference vectors: the first 10 `genrand_int32()`
    /// outputs for seed 5489. Values independently re-derived from a faithful Python
    /// port of `mt19937ar.c` (the first four match the widely published canonical
    /// values, confirming the port); the remainder correct earlier memorized-but-wrong
    /// values.
    const SEED_5489: [u32; 10] = [
        0xd091_bb5c,
        0x22ae_9ef6,
        0xe7e1_faee,
        0xd5c3_1f79,
        0x2082_352c,
        0xf807_b7df,
        0xe9d3_0005,
        0x3895_afe1,
        0xa1e2_4bba,
        0x4ee4_092b,
    ];

    /// A second seed's first outputs, cross-checked against the same independent
    /// reference implementation, to guard against only matching the 5489 vector.
    const SEED_0XC0FFEE: [u32; 10] = [
        0x92aa_1674,
        0x8ca4_5cd9,
        0xd0b0_9f67,
        0x2468_5469,
        0xed04_8e20,
        0xa65f_a9fb,
        0x17f5_7cc4,
        0x9f5a_d868,
        0x4464_afc6,
        0x08bb_4527,
    ];

    /// A third seed, also cross-checked against the independent implementation.
    const SEED_0X12345678: [u32; 10] = [
        0xc697_9343,
        0x0962_d2fa,
        0xa73a_24a4,
        0xe118_a180,
        0xb547_5abb,
        0x6461_3c7c,
        0x6f32_f4db,
        0xf27b_f199,
        0x464d_d8dc,
        0x95c1_fed6,
    ];

    #[test]
    fn full_mt_matches_canonical_seed_5489() {
        let mut rng = MersenneTwister::new(5489);
        for (i, &expected) in SEED_5489.iter().enumerate() {
            assert_eq!(rng.next_u32(), expected, "output {i}");
        }
    }

    #[test]
    fn full_mt_matches_second_reference_vector() {
        for (seed, vec) in [
            (0xC0FFEEu32, &SEED_0XC0FFEE[..]),
            (0x12345678, &SEED_0X12345678[..]),
        ] {
            let mut rng = MersenneTwister::new(seed);
            for (i, &expected) in vec.iter().enumerate() {
                assert_eq!(rng.next_u32(), expected, "seed {seed:#x}, output {i}");
            }
        }
    }

    #[test]
    fn first_n_matches_canonical_seed_5489() {
        let got = first_n(5489, 10);
        assert_eq!(got, SEED_5489.to_vec());
    }

    /// The streaming variant must equal the full generator for the first `n` outputs,
    /// for every `n` in 1..=8 and a spread of seeds. This is the single highest-value
    /// test in the project: the streaming twist is the optimization most likely to be
    /// subtly wrong, and it fails silently.
    #[test]
    fn streaming_matches_full_for_many_seeds_and_n() {
        let seeds = [
            0u32,
            1,
            5489,
            0xC0FFEE,
            0xFFFF_FFFF,
            0x8000_0000,
            0x1234_5678,
            0xDEAD_BEEF,
        ];
        for &seed in &seeds {
            for n in 1..=8usize {
                let full: Vec<u32> = {
                    let mut rng = MersenneTwister::new(seed);
                    (0..n).map(|_| rng.next_u32()).collect()
                };
                let streamed = first_n(seed, n);
                assert_eq!(
                    streamed, full,
                    "seed {seed:#010x}, n {n}: streaming diverged from full MT"
                );
            }
        }
    }

    /// Streaming matches full for n up to the streaming limit (227), on a few seeds —
    /// the edge of the no-wrap bound.
    #[test]
    fn streaming_matches_full_at_n_227() {
        for &seed in &[0u32, 1, 5489, 0xDEAD_BEEF] {
            let full: Vec<u32> = {
                let mut rng = MersenneTwister::new(seed);
                (0..MAX_STREAMING_N).map(|_| rng.next_u32()).collect()
            };
            assert_eq!(first_n(seed, MAX_STREAMING_N), full, "seed {seed:#010x}");
        }
    }

    /// The zero-alloc `first_n_into` must produce bit-identical output to the
    /// `Vec`-returning `first_n` for every n and seed (they share one implementation,
    /// but this pins that the stack-buffer path equals the reference).
    #[test]
    fn first_n_into_matches_first_n() {
        let seeds = [0u32, 1, 5489, 0xC0FFEE, 0xFFFF_FFFF, 0x8000_0000];
        for &seed in &seeds {
            for n in 1..=8usize {
                let expected = first_n(seed, n);
                let mut buf = [0u32; STREAM_BUF];
                let got = first_n_into(seed, n, &mut buf);
                assert_eq!(
                    got, expected.as_slice(),
                    "seed {seed:#010x} n {n}: first_n_into diverged"
                );
            }
        }
    }

    /// `first_n_into` at the streaming limit matches the full generator.
    #[test]
    fn first_n_into_matches_full_at_n_227() {
        let mut buf = [0u32; STREAM_BUF];
        for &seed in &[0u32, 5489, 0xDEAD_BEEF] {
            let full: Vec<u32> = {
                let mut rng = MersenneTwister::new(seed);
                (0..MAX_STREAMING_N).map(|_| rng.next_u32()).collect()
            };
            let got = first_n_into(seed, MAX_STREAMING_N, &mut buf);
            assert_eq!(got, full.as_slice(), "seed {seed:#010x}");
        }
    }

    #[test]
    #[should_panic(expected = "n <= 227")]
    fn first_n_rejects_too_large_n() {
        let _ = first_n(0, 228);
    }

    #[test]
    #[should_panic(expected = "n <= 227")]
    fn first_n_into_rejects_too_large_n() {
        let mut buf = [0u32; STREAM_BUF];
        let _ = first_n_into(0, 228, &mut buf);
    }

    /// `mNextInt` power-of-two path uses a mask (`next & (bound-1)`).
    #[test]
    fn next_int_power_of_two_masks() {
        let mut rng = MersenneTwister::new(1234);
        let bound = 32u32;
        for _ in 0..1000 {
            let v = rng.next_int(bound);
            assert!(v < bound, "masked value out of range");
        }
    }

    /// `mNextInt` non-power-of-two path is plain modulo (biased, no rejection).
    #[test]
    fn next_int_non_power_of_two_is_modulo() {
        let mut rng = MersenneTwister::new(99);
        let bound = 26u32;
        assert!(!bound.is_power_of_two());
        for _ in 0..1000 {
            let v = rng.next_int(bound);
            assert!(v < bound);
        }
    }

    /// Directly demonstrate the biased path equals `next() % bound` (not rejection
    /// sampled) by driving a known seed's raw outputs through both formulas.
    #[test]
    fn next_int_equals_raw_modulo() {
        let mut rng = MersenneTwister::new(5489);
        let mut raw_rng = MersenneTwister::new(5489);
        let bound = 26u32;
        for _ in 0..100 {
            let expected = raw_rng.next_u32() % bound;
            assert_eq!(rng.next_int(bound), expected);
        }
    }
}
