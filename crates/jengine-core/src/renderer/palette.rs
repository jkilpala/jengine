//! **Palettes** (Oct 2026): the frame quantised to a handful of colours, with
//! ordered dithering and palette cycling - the `palette` post effect.
//!
//! It runs in the post stack's **output pass**, after the tonemap, the grade
//! and the grain, so what reaches the display is exactly the palette's
//! colours (the 8-bit dither is left off under it). Each pixel is taken to
//! sRGB, nudged by a 4x4 ordered (Bayer) threshold in cells of `cell` screen
//! pixels times `dither`, and replaced by the nearest palette colour; with a
//! cycle (`cycle_start` .. `cycle_end`, `cycle_speed` steps a second) the
//! colours at those indices rotate, the old way water and fire moved without
//! redrawing a pixel.
//!
//! The palette is a preset by number (`preset`: [`PRESETS`]) or, with
//! `count` > 0, the colours `c0` .. `c15` (sRGB, 0..1). [`quantize`] is the
//! shader on the CPU, line for line.

use std::collections::HashMap;

use crate::renderer::material::UniformValue;

/// Most colours a palette holds (the output uniform's array).
pub const MAX_COLORS: usize = 16;

/// The built-in palettes, sRGB bytes.
pub const PRESETS: &[(&str, &[[u8; 3]])] = &[
    ("gameboy", &[[15, 56, 15], [48, 98, 48], [139, 172, 15], [155, 188, 15]]),
    (
        "pico8",
        &[
            [0, 0, 0], [29, 43, 83], [126, 37, 83], [0, 135, 81], [171, 82, 54], [95, 87, 79], [194, 195, 199], [255, 241, 232],
            [255, 0, 77], [255, 163, 0], [255, 236, 39], [0, 228, 54], [41, 173, 255], [131, 118, 156], [255, 119, 168], [255, 204, 170],
        ],
    ),
    ("cga", &[[0, 0, 0], [85, 255, 255], [255, 85, 255], [255, 255, 255]]),
    (
        "sweetie16",
        &[
            [26, 28, 44], [93, 39, 93], [177, 62, 83], [239, 125, 87], [255, 205, 117], [167, 240, 112], [56, 183, 100], [37, 113, 121],
            [41, 54, 111], [59, 93, 201], [65, 166, 246], [115, 239, 247], [244, 244, 244], [148, 176, 194], [86, 108, 134], [51, 60, 87],
        ],
    ),
    (
        "ega",
        &[
            [0, 0, 0], [0, 0, 170], [0, 170, 0], [0, 170, 170], [170, 0, 0], [170, 0, 170], [170, 85, 0], [170, 170, 170],
            [85, 85, 85], [85, 85, 255], [85, 255, 85], [85, 255, 255], [255, 85, 85], [255, 85, 255], [255, 255, 85], [255, 255, 255],
        ],
    ),
];

/// A palette effect as the shader takes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// sRGB 0..1, `count` of them used.
    pub colors: [[f32; 4]; MAX_COLORS],
    pub count: usize,
    pub dither: f32,
    pub cell: f32,
    /// Indices `start .. end` rotate by `shift`.
    pub cycle: (usize, usize),
    pub speed: f32,
}

fn num(p: &HashMap<String, UniformValue>, key: &str) -> Option<f32> {
    match p.get(key)? {
        UniformValue::Float(v) => Some(*v),
        UniformValue::Int(v) => Some(*v as f32),
        _ => None,
    }
}

fn colour(p: &HashMap<String, UniformValue>, key: &str) -> Option<[f32; 4]> {
    match p.get(key)? {
        UniformValue::Vec3(v) => Some([v[0], v[1], v[2], 1.0]),
        UniformValue::Vec4(v) => Some(*v),
        _ => None,
    }
}

impl Palette {
    /// The effect's parameters made into a palette.
    pub fn from_params(p: &HashMap<String, UniformValue>) -> Self {
        let mut colors = [[0.0; 4]; MAX_COLORS];
        let custom = num(p, "count").unwrap_or(0.0).max(0.0) as usize;
        let count = if custom > 0 {
            let n = custom.min(MAX_COLORS);
            for (i, c) in colors.iter_mut().enumerate().take(n) {
                *c = colour(p, &format!("c{i}")).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            }
            n
        } else {
            let k = (num(p, "preset").unwrap_or(1.0).max(0.0) as usize).min(PRESETS.len() - 1);
            let list = PRESETS[k].1;
            for (c, b) in colors.iter_mut().zip(list.iter()) {
                *c = [b[0] as f32 / 255.0, b[1] as f32 / 255.0, b[2] as f32 / 255.0, 1.0];
            }
            list.len().min(MAX_COLORS)
        };
        let start = num(p, "cycle_start").unwrap_or(0.0).max(0.0) as usize;
        let end = (num(p, "cycle_end").unwrap_or(0.0).max(0.0) as usize).min(count);
        Self {
            colors,
            count,
            dither: num(p, "dither").unwrap_or(0.6).clamp(0.0, 2.0),
            cell: num(p, "cell").unwrap_or(2.0).max(1.0),
            cycle: (start, end),
            speed: num(p, "cycle_speed").unwrap_or(0.0),
        }
    }

    /// The cycle's shift at `seconds`.
    pub fn shift(&self, seconds: f64) -> f32 {
        let (s, e) = self.cycle;
        if e <= s + 1 || self.speed == 0.0 {
            return 0.0;
        }
        ((seconds * self.speed as f64).floor().rem_euclid((e - s) as f64)) as f32
    }
}

/// The 4x4 ordered-dither threshold at a cell, 0..1.
pub fn bayer(x: i32, y: i32) -> f32 {
    const B: [f32; 16] = [0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0];
    B[(y.rem_euclid(4) * 4 + x.rem_euclid(4)) as usize] / 16.0 + 1.0 / 32.0
}

/// The shader on the CPU: a linear colour at pixel `pos` to the linear
/// colour of its palette entry (after the cycle's `shift`).
pub fn quantize(pal: &Palette, linear: [f32; 3], pos: [f32; 2], shift: f32) -> [f32; 3] {
    if pal.count == 0 {
        return linear;
    }
    let cell = [(pos[0] / pal.cell).floor() as i32, (pos[1] / pal.cell).floor() as i32];
    let t = bayer(cell[0], cell[1]) - 0.5;
    let s: Vec<f32> = linear.iter().map(|v| v.max(0.0).powf(1.0 / 2.2) + t * pal.dither * 0.25).collect();
    let mut best = (f32::MAX, 0usize);
    for (i, c) in pal.colors.iter().enumerate().take(pal.count) {
        let d = [s[0] - c[0], s[1] - c[1], s[2] - c[2]];
        let e = d[0] * d[0] * 0.30 + d[1] * d[1] * 0.59 + d[2] * d[2] * 0.11;
        if e < best.0 {
            best = (e, i);
        }
    }
    let mut i = best.1;
    let (a, b) = pal.cycle;
    if b > a + 1 && i >= a && i < b {
        i = a + (i - a + shift as usize) % (b - a);
    }
    let c = pal.colors[i];
    [c[0].powf(2.2), c[1].powf(2.2), c[2].powf(2.2)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(list: &[(&str, UniformValue)]) -> HashMap<String, UniformValue> {
        list.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn every_pixel_lands_on_the_palette() {
        let pal = Palette::from_params(&params(&[("preset", UniformValue::Int(0))]));
        assert_eq!(pal.count, 4, "the Game Boy's four greens");
        let lin: Vec<[f32; 3]> = pal.colors[..4].iter().map(|c| [c[0].powf(2.2), c[1].powf(2.2), c[2].powf(2.2)]).collect();
        for k in 0..64 {
            let v = k as f32 / 63.0;
            let out = quantize(&pal, [v * 0.3, v, v * 0.6], [k as f32, (k * 7) as f32], 0.0);
            assert!(lin.iter().any(|c| (0..3).all(|j| (c[j] - out[j]).abs() < 1e-5)), "{out:?} is not a palette colour");
        }
    }

    #[test]
    fn dithering_mixes_neighbours_for_a_colour_between_them() {
        let mut p = params(&[("count", UniformValue::Int(2)), ("dither", UniformValue::Float(2.0)), ("cell", UniformValue::Float(1.0))]);
        p.insert("c0".into(), UniformValue::Vec3([0.0, 0.0, 0.0]));
        p.insert("c1".into(), UniformValue::Vec3([1.0, 1.0, 1.0]));
        let pal = Palette::from_params(&p);
        // Mid grey in sRGB: a 4x4 tile of the threshold makes about half of
        // each, not all one.
        let grey = 0.5f32.powf(2.2);
        let whites = (0..16).filter(|i| quantize(&pal, [grey; 3], [(i % 4) as f32, (i / 4) as f32], 0.0)[0] > 0.5).count();
        assert!((6..=10).contains(&whites), "{whites} of 16 white");
        // Without dithering it is all one.
        let flat = Palette { dither: 0.0, ..pal };
        let whites = (0..16).filter(|i| quantize(&flat, [grey * 1.05; 3], [(i % 4) as f32, (i / 4) as f32], 0.0)[0] > 0.5).count();
        assert!(whites == 0 || whites == 16);
    }

    #[test]
    fn a_cycle_rotates_its_indices() {
        let pal = Palette::from_params(&params(&[
            ("preset", UniformValue::Int(1)),
            ("cycle_start", UniformValue::Int(8)),
            ("cycle_end", UniformValue::Int(12)),
            ("cycle_speed", UniformValue::Float(2.0)),
            ("dither", UniformValue::Float(0.0)),
        ]));
        assert_eq!(pal.shift(0.0), 0.0);
        assert_eq!(pal.shift(1.6), 3.0, "two steps a second, wrapped in four");
        assert_eq!(pal.shift(2.1), 0.0);
        // PICO-8's red (8) becomes its orange (9) one step on.
        let red = [1.0f32, 0.0, 77.0 / 255.0].map(|v: f32| v.powf(2.2));
        let out = quantize(&pal, red, [0.0, 0.0], 1.0);
        let orange = [1.0f32, 163.0 / 255.0, 0.0].map(|v: f32| v.powf(2.2));
        assert!((0..3).all(|j| (out[j] - orange[j]).abs() < 1e-4), "{out:?}");
        // Colours outside the cycle stay put.
        let black = quantize(&pal, [0.0; 3], [0.0, 0.0], 1.0);
        assert_eq!(black, [0.0; 3]);
    }
}
