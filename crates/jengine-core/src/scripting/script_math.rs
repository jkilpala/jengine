//! The maths a script's numbers go through: the same bits natively, in the
//! editor and in a web build (docs/determinism-proposal.md, phase 3).
//!
//! Lua's `math.sin`, `^`, `%` and `math.random` were whatever each runtime's
//! VM did: Luau called the platform's C library, Fengari called JavaScript's
//! `Math`, and the two differ in the last bit - as do Windows, macOS and
//! Linux among themselves. A game whose rules live in Lua then played out
//! differently on each, which multiplayer that sends inputs alone cannot
//! survive. So every runtime computes them here:
//!
//! - natively, the vendored Luau calls through a table of function
//!   pointers this fills ([`install_luau_table`], `vendor/luau0-src`), and
//!   `math.random` / `math.randomseed` are replaced by [`random`] / [`seed`];
//! - in the browsers both wasm hosts publish these functions as
//!   `globalThis.__jengine_script_math` (`js/script_math.rs`), which the
//!   editor's Lua runtime puts in Fengari's `math` table and the vendored
//!   Fengari's `^` and `%` call.
//!
//! The functions are `f64` (a Lua number) through the `libm` crate, as
//! [`crate::detmath`] is for the engine's own `f32` simulation. Two
//! operators are defined here rather than borrowed from either VM: [`pow`]
//! (`^`, `math.pow`) and [`modulo`] (`%` on fractional numbers).

use std::cell::Cell;

pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}
pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}
pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}
pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}
pub fn log2(x: f64) -> f64 {
    libm::log2(x)
}
pub fn log10(x: f64) -> f64 {
    libm::log10(x)
}
pub fn sinh(x: f64) -> f64 {
    libm::sinh(x)
}
pub fn cosh(x: f64) -> f64 {
    libm::cosh(x)
}
pub fn tanh(x: f64) -> f64 {
    libm::tanh(x)
}

/// The platform's `pow`, through `libm`, for exponents [`pow`] does not
/// special-case.
pub fn pow_raw(a: f64, b: f64) -> f64 {
    libm::pow(a, b)
}

/// `a ^ b` and `math.pow(a, b)`, as one rule in both VMs: exponents of 2,
/// 0.5 and 3 are `a * a`, `sqrt(a)` and `a * a * a` (Luau's VM always
/// special-cased those three, for speed), anything else `libm`'s `pow`.
/// `luau_jengine_pow` in the vendored Luau and `luai_numpow` in the vendored
/// Fengari are this function, line for line.
pub fn pow(a: f64, b: f64) -> f64 {
    if b == 2.0 {
        a * a
    } else if b == 0.5 {
        a.sqrt()
    } else if b == 3.0 {
        a * a * a
    } else {
        pow_raw(a, b)
    }
}

/// `math.log(x [, base])`, as both Luau and Lua 5.3 define it.
pub fn log(x: f64, base: Option<f64>) -> f64 {
    match base {
        None => ln(x),
        Some(b) if b == 2.0 => log2(x),
        Some(b) if b == 10.0 => log10(x),
        Some(b) => ln(x) / ln(b),
    }
}

/// `a % b` on numbers that are not both whole: Luau's `a - floor(a / b) * b`,
/// computed without a fused multiply-subtract. Lua 5.3 (Fengari) computes it
/// from `fmod`, which differs from this in the last bit for fractional
/// operands; the vendored Fengari uses this rule instead.
pub fn modulo(a: f64, b: f64) -> f64 {
    a - (a / b).floor() * b
}

/// `lerp(a, b, t)` (`Math.lerp`), in `f64` in every runtime.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// `distance(x1, y1, x2, y2)` (`Math.distance`).
pub fn distance(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let (dx, dy) = (x2 - x1, y2 - y1);
    (dx * dx + dy * dy).sqrt()
}

/// `angle_between(x1, y1, x2, y2)` (`Math.angle_between`).
pub fn angle_between(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    atan2(y2 - y1, x2 - x1)
}

// ---- math.random ------------------------------------------------------------

thread_local! {
    /// The generator every script draws from - one stream, as one Luau VM
    /// natively has, so the same draws in the same order give the same
    /// numbers in every runtime. `None` until the first draw or seed.
    static STATE: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Mix a seed into a state (the `splitmix64` finaliser), so neighbouring
/// seeds start far apart.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The next 64 random bits: `splitmix64`, whose whole state is one `u64`
/// and whose arithmetic is integers alone.
fn next_u64() -> u64 {
    STATE.with(|s| {
        let state = s.get().unwrap_or_else(unseeded).wrapping_add(0x9E37_79B9_7F4A_7C15);
        s.set(Some(state));
        mix(state)
    })
}

/// Before any `math.randomseed`, the stream starts somewhere new each run -
/// as `math.random` always has - so a game wanting the same numbers twice
/// seeds it.
fn unseeded() -> u64 {
    mix((crate::profiling::unix_time() * 1000.0) as u64 ^ 0x6A09_E667_F3BC_C909)
}

/// `math.randomseed(x)`: the stream restarts from `x`. Any number seeds - a
/// whole number and its float (`1` and `1.0`) are the same seed, since Lua
/// hands both over as the same `f64`.
pub fn seed(x: f64) {
    // Negative zero is zero.
    let bits = if x == 0.0 { 0 } else { x.to_bits() };
    STATE.with(|s| s.set(Some(mix(bits))));
}

/// `math.random()`: a float in `[0, 1)` with 53 random bits.
pub fn random_unit() -> f64 {
    (next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
}

/// `math.random(m, n)`: a whole number in `[m, n]`, both rounded down to
/// whole numbers first (Luau truncates, Lua 5.3 refuses a fraction; this
/// floors in both). `Err` for an empty interval, which both VMs raise.
pub fn random_between(m: f64, n: f64) -> Result<f64, &'static str> {
    let (m, n) = (m.floor(), n.floor());
    if !(m <= n) {
        return Err("interval is empty");
    }
    let span = n - m + 1.0;
    if !(span <= 9_007_199_254_740_992.0) {
        return Err("interval is too large");
    }
    // Multiply-high: unbiased enough, and integers alone.
    let pick = ((next_u64() as u128 * span as u64 as u128) >> 64) as f64;
    Ok(m + pick)
}

/// `math.random([m [, n]])` with Lua's argument rules: none for a float,
/// one for `[1, m]`, two for `[m, n]`.
pub fn random(m: Option<f64>, n: Option<f64>) -> Result<f64, &'static str> {
    match (m, n) {
        (None, _) => Ok(random_unit()),
        (Some(m), None) => random_between(1.0, m),
        (Some(m), Some(n)) => random_between(m, n),
    }
}

// ---- The vendored Luau's table --------------------------------------------

/// `LuauJEngineMath` in `vendor/luau0-src/luau/Common/include/Luau/JEngineMath.h`,
/// field for field.
#[cfg(feature = "native")]
#[repr(C)]
struct LuauJEngineMath {
    sin: unsafe extern "C" fn(f64) -> f64,
    cos: unsafe extern "C" fn(f64) -> f64,
    tan: unsafe extern "C" fn(f64) -> f64,
    asin: unsafe extern "C" fn(f64) -> f64,
    acos: unsafe extern "C" fn(f64) -> f64,
    atan: unsafe extern "C" fn(f64) -> f64,
    atan2: unsafe extern "C" fn(f64, f64) -> f64,
    exp: unsafe extern "C" fn(f64) -> f64,
    log: unsafe extern "C" fn(f64) -> f64,
    log2: unsafe extern "C" fn(f64) -> f64,
    log10: unsafe extern "C" fn(f64) -> f64,
    pow: unsafe extern "C" fn(f64, f64) -> f64,
    sinh: unsafe extern "C" fn(f64) -> f64,
    cosh: unsafe extern "C" fn(f64) -> f64,
    tanh: unsafe extern "C" fn(f64) -> f64,
}

#[cfg(feature = "native")]
unsafe extern "C" {
    /// Defined in the vendored Luau (`luau/Ast/src/JEngineMath.cpp`).
    static mut luau_jengine_math: LuauJEngineMath;
}

#[cfg(feature = "native")]
mod luau_fns {
    macro_rules! c1 {
        ($($name:ident),*) => { $(pub unsafe extern "C" fn $name(x: f64) -> f64 { super::$name(x) })* };
    }
    c1!(sin, cos, tan, asin, acos, atan, exp, ln, log2, log10, sinh, cosh, tanh);
    pub unsafe extern "C" fn atan2(y: f64, x: f64) -> f64 {
        super::atan2(y, x)
    }
    pub unsafe extern "C" fn pow_raw(a: f64, b: f64) -> f64 {
        super::pow_raw(a, b)
    }
}

/// Point the vendored Luau's maths at this module. Once per process, before
/// the first script is compiled - its constant folding computes through the
/// same table. Every Luau VM in the process (scripts, batch scripts) shares it.
#[cfg(feature = "native")]
pub fn install_luau_table() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        use luau_fns as f;
        let table = LuauJEngineMath {
            sin: f::sin,
            cos: f::cos,
            tan: f::tan,
            asin: f::asin,
            acos: f::acos,
            atan: f::atan,
            atan2: f::atan2,
            exp: f::exp,
            log: f::ln,
            log2: f::log2,
            log10: f::log10,
            pow: f::pow_raw,
            sinh: f::sinh,
            cosh: f::cosh,
            tanh: f::tanh,
        };
        // SAFETY: written once, before any Luau code reads it (the `Once`
        // runs before the first VM is made); the C side only reads it.
        unsafe { std::ptr::write(&raw mut luau_jengine_math, table) };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The functions' bits, pinned: a `libm` upgrade that moved one would
    /// move every script's numbers, and this says so before a game does.
    #[test]
    fn script_maths_are_pinned() {
        let got = [
            sin(1.234),
            cos(1.234),
            tan(0.7),
            asin(0.3),
            acos(0.3),
            atan(2.5),
            atan2(-1.5, 0.25),
            exp(1.7),
            ln(7.5),
            log10(1234.5),
            pow(1.7, 2.3),
            sinh(0.8),
            cosh(0.8),
            tanh(0.8),
            modulo(5.3, 0.1),
        ];
        let bits: Vec<u64> = got.iter().map(|v| v.to_bits()).collect();
        assert_eq!(bits, PINNED.to_vec(), "{:#018x?}", bits);
    }

    const PINNED: [u64; 15] = [
        0x3fee33c23ed44442,
        0x3fd526571fe8c7a5,
        0x3feaf406c2fc78ae,
        0x3fd380159e14f6ff,
        0x3ff441f5ecbeef59,
        0x3ff30b6d796a4da8,
        0xbff67d8863bc99bd,
        0x4015e552770df8a6,
        0x40001e85798eb9a3,
        0x4008bb5faece0c80,
        0x400b1c0c46dcb4ea,
        0x3fec6b5d3c9c16ef,
        0x3ff566222fae825a,
        0x3fe53fca0a748a40,
        0x3fb9999999999980,
    ];

    #[test]
    fn pow_has_one_rule() {
        assert_eq!(pow(1.1, 2.0).to_bits(), (1.1f64 * 1.1).to_bits());
        assert_eq!(pow(2.0, 0.5).to_bits(), 2f64.sqrt().to_bits());
        assert_eq!(pow(1.1, 3.0).to_bits(), (1.1f64 * 1.1 * 1.1).to_bits());
        assert_eq!(pow(2.0, 10.0), 1024.0);
    }

    #[test]
    fn modulo_is_luaus() {
        assert_eq!(modulo(7.0, 3.0), 1.0);
        assert_eq!(modulo(-7.0, 3.0), 2.0);
        assert_eq!(modulo(7.0, -3.0), -2.0);
    }

    #[test]
    fn a_seed_gives_the_same_draws() {
        seed(42.0);
        let a: Vec<f64> = (0..5).map(|_| random_unit()).collect();
        let b: Vec<f64> = (0..5).map(|_| random_between(1.0, 6.0).unwrap()).collect();
        seed(42.0);
        assert_eq!(a, (0..5).map(|_| random_unit()).collect::<Vec<_>>());
        assert_eq!(b, (0..5).map(|_| random_between(1.0, 6.0).unwrap()).collect::<Vec<_>>());
        // Pinned, so the stream is the same tomorrow and in the browser.
        seed(42.0);
        let first: Vec<u64> = (0..3).map(|_| random_unit().to_bits()).collect();
        assert_eq!(first, PINNED_DRAWS.to_vec(), "{:#018x?}", first);
        assert!(a.iter().all(|v| (0.0..1.0).contains(v)));
        assert!(b.iter().all(|v| (1.0..=6.0).contains(v) && v.fract() == 0.0));
        assert!(random_between(3.0, 2.0).is_err());
        // `1` and `1.0` and `-0.0` and `0` are one seed each.
        seed(0.0);
        let z = random_unit();
        seed(-0.0);
        assert_eq!(z, random_unit());
    }

    const PINNED_DRAWS: [u64; 3] = [0x3fd77484a3b16836, 0x3fd5b409ad07bd00, 0x3fe59dd91d31af20];
}
