//! **Colour look-up tables** (Oct 2026; Unity's Color Lookup, Godot's
//! `Environment.adjustment_color_correction`): a grade painted in an image
//! editor - grade a screenshot with the neutral strip ([`identity`]) on it,
//! save the strip - and applied to the whole frame in the output pass, after
//! the tonemap, where the colours are the ones it was painted against.
//!
//! A LUT is a strip of `n` slices, each `n` x `n`: red runs right within a
//! slice, green runs down, blue steps slice by slice - `n * n` wide and `n`
//! high, 16 or 32 usually. A scene names one with a `color_lut` component;
//! [`sync`] reads its pixels in the step every host already makes with its
//! assets (`carve::apply_carves`) and leaves them for the post stack, which
//! uploads them when they change - so no host carries a LUT itself.

use std::cell::RefCell;
use std::sync::Arc;

use crate::asset::RawTextureData;

/// The frame's LUT, as the post stack takes it.
#[derive(Debug, Clone)]
pub struct ActiveLut {
    /// Changes when the texture does: its id, its size, and a sample of its
    /// pixels - an image re-saved under the same name (the editor reloads it)
    /// is a new grade.
    pub key: u64,
    pub width: u32,
    pub height: u32,
    /// sRGB-encoded RGBA, as the image stores it.
    pub rgba: Arc<Vec<u8>>,
    /// 0 is no grade, 1 the LUT's.
    pub strength: f32,
}

thread_local! {
    static ACTIVE: RefCell<Option<ActiveLut>> = const { RefCell::new(None) };
}

/// The LUT the frame is graded through, if any.
pub fn active() -> Option<ActiveLut> {
    ACTIVE.with(|a| a.borrow().clone())
}

/// How many slices a strip `width` x `height` has - `None` when it is not a
/// LUT strip (`n * n` by `n`).
pub fn slices(width: u32, height: u32) -> Option<u32> {
    (height >= 2 && width == height * height).then_some(height)
}

/// Take the world's `color_lut` (the first one) and its texture's pixels,
/// if they are loaded, as the frame's LUT. Cheap when nothing changed: the
/// pixels are copied only when the texture does.
pub fn sync(world: &hecs::World, assets: &crate::asset::AssetManager) {
    let found = world
        .query::<&crate::ecs::components::ColorLut>()
        .iter()
        .find(|(_, l)| !l.texture_id.is_empty())
        .map(|(_, l)| (l.texture_id.clone(), l.strength));
    let next = found.and_then(|(id, strength)| {
        let tex = assets.get_texture_bytes(crate::asset::resolve_texture_id(&id))?;
        slices(tex.width, tex.height)?;
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (id.as_str(), tex.width, tex.height, tex.rgba.len()).hash(&mut h);
            // Every 16th byte: a few thousand a frame, and any real regrade
            // touches far more of the strip than that misses.
            for b in tex.rgba.iter().step_by(16) {
                b.hash(&mut h);
            }
            h.finish()
        };
        Some((key, tex, strength.clamp(0.0, 1.0)))
    });
    ACTIVE.with(|a| {
        let mut a = a.borrow_mut();
        *a = match next {
            None => None,
            Some((key, _, strength)) if a.as_ref().is_some_and(|l| l.key == key) => {
                a.take().map(|l| ActiveLut { strength, ..l })
            }
            Some((key, tex, strength)) => Some(ActiveLut {
                key,
                width: tex.width,
                height: tex.height,
                rgba: Arc::new(tex.rgba.clone()),
                strength,
            }),
        };
    });
}

/// The neutral strip of `n` slices: every colour maps to itself. Grade a
/// screenshot with this pasted on it and the strip, cut out again, is the
/// grade.
pub fn identity(n: u32) -> RawTextureData {
    let n = n.max(2);
    let step = |v: u32| ((v as f32 / (n - 1) as f32) * 255.0).round() as u8;
    let mut rgba = Vec::with_capacity((n * n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n * n {
            let (slice, r) = (x / n, x % n);
            rgba.extend_from_slice(&[step(r), step(y), step(slice), 255]);
        }
    }
    RawTextureData { width: n * n, height: n, rgba }
}

/// Where `output.wgsl` samples a strip of `n` slices for a display-encoded
/// colour: the two neighbouring slices' coordinates (0..1) and how far
/// between them blue lies - line for line with its `lut_lookup`.
pub fn strip_uv(c: [f32; 3], n: u32) -> ([f32; 2], [f32; 2], f32) {
    let n = n as f32;
    let [r, g, b] = c.map(|v| v.clamp(0.0, 1.0));
    let blue = b * (n - 1.0);
    let b0 = blue.floor();
    let b1 = (b0 + 1.0).min(n - 1.0);
    let x = r * (n - 1.0) + 0.5;
    let y = (g * (n - 1.0) + 0.5) / n;
    ([(b0 * n + x) / (n * n), y], [(b1 * n + x) / (n * n), y], blue - b0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The strip's texel nearest a uv, as the sampler would fetch it at its
    /// centre.
    fn texel(t: &RawTextureData, uv: [f32; 2]) -> [u8; 3] {
        let x = ((uv[0] * t.width as f32).floor() as u32).min(t.width - 1);
        let y = ((uv[1] * t.height as f32).floor() as u32).min(t.height - 1);
        let i = ((y * t.width + x) * 4) as usize;
        [t.rgba[i], t.rgba[i + 1], t.rgba[i + 2]]
    }

    #[test]
    fn the_identity_strip_gives_every_grid_colour_back() {
        let n = 16;
        let t = identity(n);
        assert_eq!((t.width, t.height), (256, 16));
        assert_eq!(slices(t.width, t.height), Some(16));
        for (r, g, b) in [(0, 0, 0), (15, 15, 15), (3, 9, 12), (15, 0, 7)] {
            let c = [r as f32 / 15.0, g as f32 / 15.0, b as f32 / 15.0];
            let (uv0, _, f) = strip_uv(c, n);
            assert!(f.abs() < 1e-5, "a grid colour lands on a slice");
            let want = [r, g, b].map(|v| ((v as f32 / 15.0) * 255.0).round() as u8);
            assert_eq!(texel(&t, uv0), want, "{c:?}");
        }
    }

    #[test]
    fn blue_between_slices_mixes_the_two() {
        let (uv0, uv1, f) = strip_uv([0.0, 0.0, 0.5], 16);
        assert!((f - 0.5).abs() < 1e-5);
        assert!(uv1[0] > uv0[0], "the next slice along");
        assert_eq!(slices(256, 32), None, "not a strip");
        assert_eq!(slices(1024, 32), Some(32));
    }
}
