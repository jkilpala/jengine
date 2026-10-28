//! Maths that give the same bits on every platform, for simulation.
//!
//! Addition, multiplication, division and `sqrt` are IEEE operations and give
//! the same bits everywhere Rust runs. `sin`, `cos`, `atan2`, `exp`, `powf`
//! and the rest are calls into the platform's maths library - MSVC's, Apple's,
//! glibc, the wasm build's - each free to differ in the last bit, and a
//! simulation turns a last bit into a different game within seconds: a car
//! football match from one seed ended 2 - 0 natively on a Mac and 1 - 2 in
//! the browser (docs/determinism-proposal.md).
//!
//! These go through the `libm` crate instead, a pure-Rust port that computes
//! the same bits everywhere - the same functions rapier uses under its
//! `enhanced-determinism` feature, which the workspace turns on. So gameplay
//! code that decides what happens (a tyre's slip angle, a bot's aim, a
//! wall's angle) calls `x.dsin()` / `y.datan2(x)`; drawing, audio and UI
//! may keep the platform's maths, which is a little faster.
//!
//! ```
//! use jengine_core::detmath::DetMath;
//! let (s, c) = 0.5f32.dsin_cos();
//! assert_eq!(s.to_bits(), libm::sinf(0.5).to_bits());
//! # let _ = c;
//! ```

/// The transcendental functions of `f32` and `f64`, computed by `libm`.
///
/// Named with a `d` (deterministic) because an inherent method always wins
/// over a trait's: `x.sin()` would still be the platform's.
pub trait DetMath: Sized {
    fn dsin(self) -> Self;
    fn dcos(self) -> Self;
    fn dsin_cos(self) -> (Self, Self);
    fn dtan(self) -> Self;
    fn dasin(self) -> Self;
    fn dacos(self) -> Self;
    fn datan(self) -> Self;
    fn datan2(self, x: Self) -> Self;
    fn dexp(self) -> Self;
    fn dln(self) -> Self;
    fn dpowf(self, e: Self) -> Self;
    /// `x.powi(n)`, which is not an IEEE operation either: LLVM sends it to
    /// compiler-rt's `__powisf2` (repeated multiplication) on most targets,
    /// but MSVC has no such routine and gets the CRT's `pow` instead, which
    /// rounds once - so `x.powi(3)` came out a bit apart on Windows and a car
    /// football match from one seed ended differently there. This is
    /// `__powisf2`'s loop, so it gives what every other platform gave.
    fn dpowi(self, n: i32) -> Self;
    fn dhypot(self, o: Self) -> Self;
    fn dexp2(self) -> Self;
    fn dlog2(self) -> Self;
    fn dlog10(self) -> Self;
    fn dsinh(self) -> Self;
    fn dcosh(self) -> Self;
    fn dtanh(self) -> Self;
}

impl DetMath for f32 {
    fn dsin(self) -> f32 {
        libm::sinf(self)
    }
    fn dcos(self) -> f32 {
        libm::cosf(self)
    }
    fn dsin_cos(self) -> (f32, f32) {
        libm::sincosf(self)
    }
    fn dtan(self) -> f32 {
        libm::tanf(self)
    }
    fn dasin(self) -> f32 {
        libm::asinf(self)
    }
    fn dacos(self) -> f32 {
        libm::acosf(self)
    }
    fn datan(self) -> f32 {
        libm::atanf(self)
    }
    fn datan2(self, x: f32) -> f32 {
        libm::atan2f(self, x)
    }
    fn dexp(self) -> f32 {
        libm::expf(self)
    }
    fn dln(self) -> f32 {
        libm::logf(self)
    }
    fn dpowf(self, e: f32) -> f32 {
        libm::powf(self, e)
    }
    fn dpowi(self, n: i32) -> f32 {
        let (mut a, mut b, mut r) = (self, n.unsigned_abs(), 1.0);
        loop {
            if b & 1 != 0 {
                r *= a;
            }
            b /= 2;
            if b == 0 {
                break;
            }
            a *= a;
        }
        if n < 0 { 1.0 / r } else { r }
    }
    fn dhypot(self, o: f32) -> f32 {
        libm::hypotf(self, o)
    }
    fn dexp2(self) -> f32 {
        libm::exp2f(self)
    }
    fn dlog2(self) -> f32 {
        libm::log2f(self)
    }
    fn dlog10(self) -> f32 {
        libm::log10f(self)
    }
    fn dsinh(self) -> f32 {
        libm::sinhf(self)
    }
    fn dcosh(self) -> f32 {
        libm::coshf(self)
    }
    fn dtanh(self) -> f32 {
        libm::tanhf(self)
    }
}

impl DetMath for f64 {
    fn dsin(self) -> f64 {
        libm::sin(self)
    }
    fn dcos(self) -> f64 {
        libm::cos(self)
    }
    fn dsin_cos(self) -> (f64, f64) {
        libm::sincos(self)
    }
    fn dtan(self) -> f64 {
        libm::tan(self)
    }
    fn dasin(self) -> f64 {
        libm::asin(self)
    }
    fn dacos(self) -> f64 {
        libm::acos(self)
    }
    fn datan(self) -> f64 {
        libm::atan(self)
    }
    fn datan2(self, x: f64) -> f64 {
        libm::atan2(self, x)
    }
    fn dexp(self) -> f64 {
        libm::exp(self)
    }
    fn dln(self) -> f64 {
        libm::log(self)
    }
    fn dpowf(self, e: f64) -> f64 {
        libm::pow(self, e)
    }
    fn dpowi(self, n: i32) -> f64 {
        let (mut a, mut b, mut r) = (self, n.unsigned_abs(), 1.0);
        loop {
            if b & 1 != 0 {
                r *= a;
            }
            b /= 2;
            if b == 0 {
                break;
            }
            a *= a;
        }
        if n < 0 { 1.0 / r } else { r }
    }
    fn dhypot(self, o: f64) -> f64 {
        libm::hypot(self, o)
    }
    fn dexp2(self) -> f64 {
        libm::exp2(self)
    }
    fn dlog2(self) -> f64 {
        libm::log2(self)
    }
    fn dlog10(self) -> f64 {
        libm::log10(self)
    }
    fn dsinh(self) -> f64 {
        libm::sinh(self)
    }
    fn dcosh(self) -> f64 {
        libm::cosh(self)
    }
    fn dtanh(self) -> f64 {
        libm::tanh(self)
    }
}

#[cfg(test)]
mod tests {
    use super::DetMath;

    /// Pinned bits. `libm` is pure Rust, so these are the same on every
    /// platform; CI on Windows checks what a Mac wrote. A `libm` upgrade that
    /// changes them moves every deterministic simulation, and must be a
    /// decision, not a dependency bump that slipped by.
    #[test]
    fn results_are_pinned_to_the_bit() {
        let x = 0.8f32;
        let got = [
            x.dsin().to_bits(),
            x.dcos().to_bits(),
            x.dtan().to_bits(),
            x.datan().to_bits(),
            x.datan2(-0.3).to_bits(),
            x.dexp().to_bits(),
            x.dln().to_bits(),
            x.dpowf(0.6).to_bits(),
            x.dacos().to_bits(),
            x.dhypot(1.7).to_bits(),
        ];
        let want = PINNED_F32;
        assert_eq!(got, want, "libm's answers changed: {got:#x?}");
        let (s, c) = x.dsin_cos();
        assert_eq!((s.to_bits(), c.to_bits()), (got[0], got[1]), "sin_cos agrees with sin and cos");
        let y = 0.8f64;
        let got64 = [y.dsin().to_bits(), y.datan2(-0.3).to_bits(), y.dexp().to_bits()];
        assert_eq!(got64, PINNED_F64, "libm's answers changed: {got64:#x?}");
    }

    const PINNED_F32: [u32; 10] = [
        0x3f37a4a6, 0x3f325b5f, 0x3f83cb32, 0x3f2cbbd3, 0x3ff6fc0d, 0x400e6f43, 0xbe647fbd, 0x3f5febaa, 0x3f24bc7d,
        0x3ff07d7c,
    ];
    const PINNED_F64: [u64; 3] = [0x3fe6f494c2bffecd, 0x3ffedf81a4bd64d5, 0x4001cde866fe46e9];

    /// Close to the platform's answers - the same functions, not new ones.
    #[test]
    fn answers_are_the_usual_ones() {
        for i in -40..=40 {
            let x = i as f32 * 0.157;
            assert!((x.dsin() - x.sin()).abs() < 1e-6);
            assert!((x.dcos() - x.cos()).abs() < 1e-6);
            assert!((x.datan2(0.7) - x.atan2(0.7)).abs() < 1e-6);
            assert!((x.dexp() - x.exp()).abs() <= x.exp() * 1e-6);
        }
    }

    /// `powi` as compiler-rt multiplies it, rounding at every step - not a
    /// `pow` rounded once, which is what MSVC's CRT gives `x.powi(n)`.
    #[test]
    fn powi_multiplies_as_compiler_rt_does() {
        let x = 1.1f32;
        assert_eq!(x.dpowi(0), 1.0);
        assert_eq!(x.dpowi(2).to_bits(), (x * x).to_bits());
        assert_eq!(x.dpowi(3).to_bits(), (x * (x * x)).to_bits());
        assert_eq!(x.dpowi(5).to_bits(), (x * ((x * x) * (x * x))).to_bits());
        assert_eq!(x.dpowi(-2).to_bits(), (1.0 / (x * x)).to_bits());
        let mut exact = 1f64;
        for e in 0..=22 {
            assert_eq!(10f64.dpowi(e), exact, "10^{e} is representable, so every step is exact");
            exact *= 10.0;
        }
    }
}
