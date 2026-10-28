//! Noise for scripts and packages (Oct 2026): Perlin (2D and 3D), simplex,
//! cellular (Worley) and fractal sums of any of them - the stuff islands,
//! clouds, wind and wobble are made of. Before this a script that wanted a
//! smooth random field wrote its own value noise in Lua (the showcase's
//! Mapmaker did), slowly and differently in each game.
//!
//! Everything is a pure function of its inputs and a `seed`: the same call
//! gives the same number in every runtime - native through Luau globals, the
//! browsers through one `state.noise` hook (`adapters::js::noise`) - because
//! it is this file in all of them. The lattice is hashed with integers, not
//! a permutation table, so any `u32` is a seed and none costs a table.
//!
//! Ranges: [`perlin`], [`perlin3`] and [`simplex`] are about -1..1 (clamped
//! there); [`cellular`]'s distances are in cells (one feature point per unit
//! cell, so `f1` is rarely past 1) and its `value` is 0..1; a [`Fractal`] is
//! -1..1 as `fbm` and 0..1 as `ridged` or `turbulence`.

use crate::detmath::DetMath;
use serde_json::Value;

/// A 32-bit hash of a lattice point and a seed (a murmur-style finaliser
/// over a mix of the coordinates).
#[inline]
fn hash2(i: i32, j: i32, seed: u32) -> u32 {
    let mut h = (i as u32).wrapping_mul(0x8da6_b343) ^ (j as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

#[inline]
fn hash3(i: i32, j: i32, k: i32, seed: u32) -> u32 {
    hash2(i, j ^ (k.wrapping_mul(0x2c1b_3c6d_u32 as i32)), seed.wrapping_add((k as u32).wrapping_mul(0x9e37_79b9)))
}

/// 0..1 from a hash.
#[inline]
fn unit(h: u32) -> f64 {
    (h >> 8) as f64 / (1u32 << 24) as f64
}

/// Quintic fade, `6t^5 - 15t^4 + 10t^3`: flat at both ends, so the lattice
/// does not show.
#[inline]
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Sixteen unit directions, a lattice point's gradient picked by its hash.
#[inline]
fn grad2(h: u32, x: f64, y: f64) -> f64 {
    let a = (h & 15) as f64 * (std::f64::consts::TAU / 16.0);
    a.dcos() * x + a.dsin() * y
}

/// The twelve edge directions of a cube (Perlin's improved noise).
#[inline]
fn grad3(h: u32, x: f64, y: f64, z: f64) -> f64 {
    match h % 12 {
        0 => x + y,
        1 => -x + y,
        2 => x - y,
        3 => -x - y,
        4 => x + z,
        5 => -x + z,
        6 => x - z,
        7 => -x - z,
        8 => y + z,
        9 => -y + z,
        10 => y - z,
        _ => -y - z,
    }
}

/// Perlin gradient noise in 2D, about -1..1, zero on every integer point.
pub fn perlin(x: f64, y: f64, seed: u32) -> f64 {
    let (i, j) = (x.floor(), y.floor());
    let (fx, fy) = (x - i, y - j);
    let (i, j) = (i as i32, j as i32);
    let n00 = grad2(hash2(i, j, seed), fx, fy);
    let n10 = grad2(hash2(i.wrapping_add(1), j, seed), fx - 1.0, fy);
    let n01 = grad2(hash2(i, j.wrapping_add(1), seed), fx, fy - 1.0);
    let n11 = grad2(hash2(i.wrapping_add(1), j.wrapping_add(1), seed), fx - 1.0, fy - 1.0);
    let (u, v) = (fade(fx), fade(fy));
    // Unit gradients reach about 0.707; scale that to 1.
    (lerp(lerp(n00, n10, u), lerp(n01, n11, u), v) * std::f64::consts::SQRT_2).clamp(-1.0, 1.0)
}

/// Perlin gradient noise in 3D, about -1..1 - the third axis is usually
/// time, so a field can change shape where it stands rather than slide.
pub fn perlin3(x: f64, y: f64, z: f64, seed: u32) -> f64 {
    let (i, j, k) = (x.floor(), y.floor(), z.floor());
    let (fx, fy, fz) = (x - i, y - j, z - k);
    let (i, j, k) = (i as i32, j as i32, k as i32);
    let g = |di: i32, dj: i32, dk: i32| {
        grad3(
            hash3(i.wrapping_add(di), j.wrapping_add(dj), k.wrapping_add(dk), seed),
            fx - di as f64,
            fy - dj as f64,
            fz - dk as f64,
        )
    };
    let (u, v, w) = (fade(fx), fade(fy), fade(fz));
    let x00 = lerp(g(0, 0, 0), g(1, 0, 0), u);
    let x10 = lerp(g(0, 1, 0), g(1, 1, 0), u);
    let x01 = lerp(g(0, 0, 1), g(1, 0, 1), u);
    let x11 = lerp(g(0, 1, 1), g(1, 1, 1), u);
    lerp(lerp(x00, x10, v), lerp(x01, x11, v), w).clamp(-1.0, 1.0)
}

/// Simplex noise in 2D (Perlin's 2001 design, Gustavson's layout), about
/// -1..1: a triangular lattice, so no axis-aligned grain and three corners a
/// point instead of four.
pub fn simplex(x: f64, y: f64, seed: u32) -> f64 {
    const F2: f64 = 0.366_025_403_784_438_6; // (sqrt(3) - 1) / 2
    const G2: f64 = 0.211_324_865_405_187_1; // (3 - sqrt(3)) / 6
    let s = (x + y) * F2;
    let (i, j) = ((x + s).floor(), (y + s).floor());
    let t = (i + j) * G2;
    let (x0, y0) = (x - (i - t), y - (j - t));
    let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
    let (x1, y1) = (x0 - i1 as f64 + G2, y0 - j1 as f64 + G2);
    let (x2, y2) = (x0 - 1.0 + 2.0 * G2, y0 - 1.0 + 2.0 * G2);
    let (i, j) = (i as i32, j as i32);
    let corner = |h: u32, x: f64, y: f64| {
        let t = 0.5 - x * x - y * y;
        if t <= 0.0 { 0.0 } else { t * t * t * t * grad2(h, x, y) }
    };
    let n = corner(hash2(i, j, seed), x0, y0)
        + corner(hash2(i.wrapping_add(i1), j.wrapping_add(j1), seed), x1, y1)
        + corner(hash2(i.wrapping_add(1), j.wrapping_add(1), seed), x2, y2);
    // A unit gradient's corner peaks near 0.0123; 99 brings the sum to ~1.
    (n * 99.0).clamp(-1.0, 1.0)
}

/// What [`cellular`] finds about a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    /// Distance to the nearest feature point, in cells.
    pub f1: f64,
    /// Distance to the second nearest. `f2 - f1` is zero on the borders
    /// between cells, which is what draws cracks and cobbles.
    pub f2: f64,
    /// 0..1, the same everywhere in the nearest point's cell - a colour per
    /// region (biomes, stained glass, plates).
    pub value: f64,
}

/// Cellular (Worley) noise: one feature point jittered inside every unit
/// cell, and the distances to the nearest two.
pub fn cellular(x: f64, y: f64, seed: u32) -> Cell {
    let (ci, cj) = (x.floor() as i32, y.floor() as i32);
    let (mut f1, mut f2, mut value) = (f64::MAX, f64::MAX, 0.0);
    for dj in -1..=1 {
        for di in -1..=1 {
            let (i, j) = (ci.wrapping_add(di), cj.wrapping_add(dj));
            let h = hash2(i, j, seed);
            let px = i as f64 + unit(h);
            let py = j as f64 + unit(hash2(i, j, seed ^ 0x5bd1_e995));
            let d = ((px - x).dpowi(2) + (py - y).dpowi(2)).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
                value = unit(hash2(i, j, seed ^ 0x27d4_eb2f));
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    Cell { f1, f2, value }
}

/// Which noise a [`Fractal`] sums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    Perlin,
    /// Perlin in 3D, reading [`Fractal::z`] - a field that changes in place.
    Perlin3,
    #[default]
    Simplex,
    /// Cellular `f1`, as `f1 * 2 - 1` so it sums like the others.
    Cellular,
}

/// How a [`Fractal`] sums its octaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Plain fractal Brownian motion, -1..1: hills, clouds.
    #[default]
    Fbm,
    /// `1 - |n|`, squared, 0..1: sharp crests - mountain ranges, veins.
    Ridged,
    /// `|n|`, 0..1: billows - smoke, fire, marble.
    Turbulence,
}

/// Octaves of a noise summed at rising frequency and falling amplitude.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fractal {
    pub kind: Kind,
    pub mode: Mode,
    /// 1..=12.
    pub octaves: u32,
    /// Frequency multiplier from one octave to the next.
    pub lacunarity: f64,
    /// Amplitude multiplier from one octave to the next.
    pub gain: f64,
    /// Scales the input before anything else: `0.05` makes features twenty
    /// units across.
    pub frequency: f64,
    pub seed: u32,
    /// The third coordinate for [`Kind::Perlin3`].
    pub z: f64,
}

impl Default for Fractal {
    fn default() -> Self {
        Self { kind: Kind::Simplex, mode: Mode::Fbm, octaves: 5, lacunarity: 2.0, gain: 0.5, frequency: 1.0, seed: 0, z: 0.0 }
    }
}

/// A script's seed - any number - as the hash's `u32`: whole numbers map to
/// themselves (negatives wrap), and a fraction is not thrown away.
pub fn seed_from(n: f64) -> u32 {
    if !n.is_finite() {
        return 0;
    }
    if n.fract() == 0.0 {
        return (n as i64) as u32;
    }
    (n.to_bits() ^ (n.to_bits() >> 32)) as u32
}

impl Fractal {
    /// The settings from a script's table - `{ kind = "simplex", mode =
    /// "ridged", octaves = 6, lacunarity = 2, gain = 0.5, frequency = 0.02,
    /// seed = 7, z = t }`, any of them left out. `null` or anything not a
    /// table is the defaults. An unknown `kind` or `mode` is logged and the
    /// default used.
    pub fn from_json(v: &Value) -> Self {
        let mut f = Self::default();
        let Some(o) = v.as_object() else { return f };
        let num = |k: &str| o.get(k).and_then(Value::as_f64);
        if let Some(k) = o.get("kind").and_then(Value::as_str) {
            f.kind = match k {
                "perlin" => Kind::Perlin,
                "perlin3" => Kind::Perlin3,
                "simplex" => Kind::Simplex,
                "cellular" => Kind::Cellular,
                other => {
                    log::warn!("[noise] unknown kind '{other}' - perlin, perlin3, simplex or cellular");
                    f.kind
                }
            };
        }
        if let Some(m) = o.get("mode").and_then(Value::as_str) {
            f.mode = match m {
                "fbm" => Mode::Fbm,
                "ridged" => Mode::Ridged,
                "turbulence" => Mode::Turbulence,
                other => {
                    log::warn!("[noise] unknown mode '{other}' - fbm, ridged or turbulence");
                    f.mode
                }
            };
        }
        if let Some(n) = num("octaves") {
            f.octaves = n.round().clamp(1.0, 12.0) as u32;
        }
        if let Some(n) = num("lacunarity") {
            f.lacunarity = n;
        }
        if let Some(n) = num("gain") {
            f.gain = n;
        }
        if let Some(n) = num("frequency") {
            f.frequency = n;
        }
        if let Some(n) = num("seed") {
            f.seed = seed_from(n);
        }
        if let Some(n) = num("z") {
            f.z = n;
        }
        f
    }

    /// One octave's noise, -1..1.
    fn base(&self, x: f64, y: f64, z: f64, seed: u32) -> f64 {
        match self.kind {
            Kind::Perlin => perlin(x, y, seed),
            Kind::Perlin3 => perlin3(x, y, z, seed),
            Kind::Simplex => simplex(x, y, seed),
            Kind::Cellular => (cellular(x, y, seed).f1 * 2.0 - 1.0).clamp(-1.0, 1.0),
        }
    }

    /// The sum at a point.
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        let octaves = self.octaves.clamp(1, 12);
        let (mut freq, mut amp) = (self.frequency, 1.0);
        let (mut sum, mut norm) = (0.0, 0.0);
        for o in 0..octaves {
            // Each octave its own seed, or their lattices line up at the
            // origin and every octave has a zero there.
            let seed = self.seed.wrapping_add(o.wrapping_mul(0x9e37_79b9));
            let n = self.base(x * freq, y * freq, self.z * freq, seed);
            sum += amp
                * match self.mode {
                    Mode::Fbm => n,
                    Mode::Ridged => (1.0 - n.abs()).dpowi(2),
                    Mode::Turbulence => n.abs(),
                };
            norm += amp;
            freq *= self.lacunarity;
            amp *= self.gain;
        }
        if norm > 0.0 { sum / norm } else { 0.0 }
    }

    /// `w` x `h` samples, row by row: sample `(c, r)` is at
    /// `(x0 + c * step, y0 + r * step)`. One call for a whole map, so a
    /// script does not cross into the engine once per cell.
    pub fn grid(&self, x0: f64, y0: f64, w: usize, h: usize, step: f64) -> Vec<f64> {
        let mut out = Vec::with_capacity(w * h);
        for r in 0..h {
            for c in 0..w {
                out.push(self.sample(x0 + c as f64 * step, y0 + r as f64 * step));
            }
        }
        out
    }
}

/// The most cells [`Fractal::grid`] fills for a script in one call (a
/// 1024 x 1024 map): past it the call answers an empty list and logs, rather
/// than hanging a frame.
pub const MAX_GRID_CELLS: usize = 1 << 20;

/// A script's grid request: an empty list (and a log line) when it is past
/// [`MAX_GRID_CELLS`].
pub fn script_grid(x0: f64, y0: f64, w: f64, h: f64, step: f64, opts: &Value) -> Vec<f64> {
    let (w, h) = (w.max(0.0) as usize, h.max(0.0) as usize);
    if w.saturating_mul(h) > MAX_GRID_CELLS {
        log::warn!("[noise] Noise.grid of {w} x {h} is past {MAX_GRID_CELLS} cells - nothing drawn");
        return Vec::new();
    }
    Fractal::from_json(opts).grid(x0, y0, w, h, step)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perlin_is_zero_on_the_lattice() {
        for (x, y) in [(0.0, 0.0), (3.0, -2.0), (-7.0, 11.0)] {
            assert!(perlin(x, y, 9).abs() < 1e-12);
        }
    }

    #[test]
    fn a_seed_from_a_script_keeps_whole_numbers() {
        assert_eq!(seed_from(7.0), 7);
        assert_eq!(seed_from(-1.0), u32::MAX);
        assert_ne!(seed_from(0.5), seed_from(0.25));
    }
}
