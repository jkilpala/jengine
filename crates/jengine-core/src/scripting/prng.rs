//! Slice 5.5c — deterministic per-chunk PRNG seeding for batch scripts.
//!
//! Each batch `run_batch` invocation gets a single 64-bit seed derived
//! from a fixed mixing of `(scene_seed, frame, system_id, chunk_id)`
//! via the `splitmix64` finalizer. The worker thread installs this
//! seed on its embedded RNG before invoking the script, so any
//! `Math.random()` / `math.random()` call from script code is fed by
//! the seeded stream.
//!
//! ### Why this seeding tuple
//!
//! - **scene_seed**: lets the host pick a global stream seed (e.g. via
//!   scene metadata or a CLI flag). Default 0; same value across runs
//!   gives identical script behaviour run-to-run.
//! - **frame**: re-seeds every frame so consecutive frames produce
//!   independent streams (no script accidentally observes "the same
//!   random number every frame" because the worker thread happens to
//!   reuse runtime state).
//! - **system_id**: a stable 32-bit identifier the dispatcher derives
//!   from the script path. Two different scripts within the same
//!   frame get independent streams.
//! - **chunk_id**: the chunk index within a system. Two chunks of the
//!   same script (same `(scene_seed, frame, system_id)`) get
//!   independent streams, so partitioning the same workload across
//!   N=1 vs N=4 vs N=8 workers does NOT affect the per-entity random
//!   draws (chunk 0's stream is fixed regardless of how many other
//!   chunks exist).
//!
//! The four values are mixed with `splitmix64` (one call per input
//! plus one more on the accumulator), which is the standard
//! "thoroughly mix any-bit-pattern inputs into one 64-bit seed"
//! finalizer. The cost is ~6 multiply/xor pairs per seed — negligible
//! compared to the script body.
//!
//! ### Stream
//!
//! `DetRng` is xorshift64*. Tiny (`u64` state, 4 ops per `next_u64`),
//! good enough quality for game randomness, and bit-stable across
//! platforms (no FP, no arch-dependent intrinsics). For `[0,1)` we
//! convert via the canonical "high 53 bits divided by 2^53" trick to
//! match the JS `Math.random` / Lua `math.random()` output range.

#![cfg(all(not(target_arch = "wasm32"), any(feature = "js-adapter-host", feature = "native")))]

/// `splitmix64` finalizer. Standard mixing function for "any 64-bit
/// integer → uniformly distributed 64-bit integer".
#[inline]
pub fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Mix `(scene_seed, frame, system_id, chunk_id)` into a single 64-bit
/// seed. Order-sensitive but otherwise treats all four inputs
/// uniformly. Same inputs → same output (deterministic).
#[inline]
pub fn mix_seed(scene_seed: u64, frame: u64, system_id: u32, chunk_id: u32) -> u64 {
    let mut acc = splitmix64(scene_seed);
    acc = splitmix64(acc ^ frame);
    acc = splitmix64(acc ^ ((system_id as u64) << 32 | chunk_id as u64));
    acc
}

/// xorshift64* PRNG. State must never be 0 — the constructor enforces
/// this by substituting a fixed non-zero seed if 0 is passed.
#[derive(Debug, Clone)]
pub struct DetRng {
    state: u64,
}

impl DetRng {
    #[inline]
    pub fn new(seed: u64) -> Self {
        // xorshift64* requires non-zero state; substitute a fixed
        // sentinel if the caller passed 0.
        Self { state: if seed == 0 { 0xDEAD_BEEF_CAFE_BABE } else { seed } }
    }

    /// Reseed in place. Same non-zero substitution as `new`.
    #[inline]
    pub fn reseed(&mut self, seed: u64) {
        self.state = if seed == 0 { 0xDEAD_BEEF_CAFE_BABE } else { seed };
    }

    /// Standard xorshift64* step.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform `[0, 1)` f64. Uses the high 53 bits of the next
    /// `u64`, matching how stdlib + JS engines convert PRNG output
    /// into `Math.random()`-style floats.
    #[inline]
    pub fn next_f64_unit(&mut self) -> f64 {
        // 53 bits of mantissa precision: shift down so the high bits
        // are the ones used.
        let bits = self.next_u64() >> 11;
        bits as f64 * (1.0_f64 / (1u64 << 53) as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_is_deterministic() {
        assert_eq!(splitmix64(0), splitmix64(0));
        assert_eq!(splitmix64(0xDEAD_BEEF), splitmix64(0xDEAD_BEEF));
        assert_ne!(splitmix64(0), splitmix64(1));
    }

    #[test]
    fn mix_seed_distinguishes_all_four_axes() {
        let base = mix_seed(0, 0, 0, 0);
        assert_ne!(base, mix_seed(1, 0, 0, 0), "scene_seed axis");
        assert_ne!(base, mix_seed(0, 1, 0, 0), "frame axis");
        assert_ne!(base, mix_seed(0, 0, 1, 0), "system_id axis");
        assert_ne!(base, mix_seed(0, 0, 0, 1), "chunk_id axis");
    }

    #[test]
    fn mix_seed_is_deterministic() {
        assert_eq!(mix_seed(7, 99, 3, 2), mix_seed(7, 99, 3, 2));
    }

    #[test]
    fn det_rng_is_deterministic() {
        let mut a = DetRng::new(42);
        let mut b = DetRng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn det_rng_zero_seed_substituted() {
        let a = DetRng::new(0);
        let b = DetRng::new(0);
        assert_eq!(a.state, b.state);
        assert_ne!(a.state, 0);
    }

    #[test]
    fn next_f64_unit_in_range() {
        let mut rng = DetRng::new(1);
        for _ in 0..1000 {
            let x = rng.next_f64_unit();
            assert!((0.0..1.0).contains(&x), "out of range: {x}");
        }
    }
}
