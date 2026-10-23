//! **Snow cover** (Oct 2026): snow that settles on whatever a sprite
//! draws - roofs, ledges, sills, the top of a fence post - and melts again.
//!
//! A [`SnowCover`] on a sprite works out, from the sprite's own pixels, a
//! picture of snow lying on every edge of its silhouette that faces up
//! ([`snow_picture`]): each opaque texel with clear sky above it is a
//! surface, and snow piles on it `depth` texels deep - less where the
//! surface is steep (`slope_limit`), thinning off the ends of a ledge, with
//! a lumpy top (`bumps`) and its edges softened. `rate` adds to the depth a
//! second, up to `max_depth`; a negative rate melts it. The picture is made
//! in `carve::apply_carves` (pixels) into `__snow/<entity>` whenever the
//! depth has moved by `step` texels or the sprite's texture changed, and
//! drawn by `render_world_items` as a quad over the sprite, lit as it is -
//! so no host was wired. Snow lies **above** a surface, in the picture's
//! clear texels, so the art needs headroom over its top edges.

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::asset::{AssetManager, RawTextureData};
use crate::ecs::components::SpriteComponent;

/// Snow lying on a sprite's upward edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SnowCover {
    /// How deep the snow lies now, in the sprite's texels.
    pub depth: f32,
    /// The deepest it gets.
    pub max_depth: f32,
    /// Texels a second it deepens (negative: it melts).
    pub rate: f32,
    /// Linear colour; the snow is shaded a little towards its foot.
    pub color: [f32; 4],
    /// How steep a surface still holds snow, in texels of rise a texel
    /// across: at the limit it holds none.
    pub slope_limit: f32,
    /// How lumpy the top is, texels.
    pub bumps: f32,
    /// Where the sprite's alpha counts as solid.
    pub cutoff: f32,
    /// How far the depth moves before the picture is remade, texels.
    pub step: f32,
    /// What the picture was made from, and its texture id - kept here so
    /// the draw can borrow it (never saved).
    #[serde(skip)]
    pub key: u64,
    #[serde(skip)]
    pub picture: String,
}

impl Default for SnowCover {
    fn default() -> Self {
        Self {
            depth: 0.0,
            max_depth: 10.0,
            rate: 0.0,
            color: [0.92, 0.95, 1.0, 1.0],
            slope_limit: 1.5,
            bumps: 1.2,
            cutoff: 0.5,
            step: 0.5,
            key: 0,
            picture: String::new(),
        }
    }
}

/// The texture a snowy entity's picture lives under.
pub fn picture_id(e: Entity) -> String {
    format!("__snow/{}", e.to_bits())
}

/// A small integer hash, for the lumps.
fn hash(x: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B9) ^ seed.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Smooth lumps along x: hashed values every 6 texels, eased between.
fn lumps(x: usize) -> f32 {
    let cell = x / 6;
    let t = (x % 6) as f32 / 6.0;
    let t = t * t * (3.0 - 2.0 * t);
    let a = hash(cell as u32, 7);
    let b = hash(cell as u32 + 1, 7);
    a + (b - a) * t - 0.5
}

/// The snow lying on `src`'s upward edges at `depth`, as a picture the size
/// of `src`: snow white (in `color`) where it lies, clear elsewhere.
pub fn snow_picture(src: &RawTextureData, s: &SnowCover, depth: f32) -> RawTextureData {
    let (w, h) = (src.width as usize, src.height as usize);
    let mut out = vec![0u8; w * h * 4];
    if depth <= 0.0 || w == 0 || h == 0 {
        return RawTextureData { width: src.width, height: src.height, rgba: out };
    }
    let cut = (s.cutoff.clamp(0.0, 1.0) * 255.0) as u8;
    let solid = |x: usize, y: usize| src.rgba[(y * w + x) * 4 + 3] >= cut.max(1);
    // Every surface: a solid texel with sky (or the picture's top) above.
    let surface = |x: usize, y: usize| solid(x, y) && (y == 0 || !solid(x, y - 1));
    // The surface in column `x` nearest row `y`, within a few rows.
    let near = |x: isize, y: usize| -> Option<f32> {
        if x < 0 || x >= w as isize {
            return None;
        }
        let x = x as usize;
        (0..=6usize).find_map(|d| {
            if y >= d && surface(x, y - d) {
                Some((y - d) as f32)
            } else if y + d < h && surface(x, y + d) {
                Some((y + d) as f32)
            } else {
                None
            }
        })
    };
    // How much snow each texel above a surface holds (0..1).
    let mut amount = vec![0.0f32; w * h];
    for x in 0..w {
        for y in 0..h {
            if !surface(x, y) {
                continue;
            }
            // The slope from the surfaces two columns either side; a side
            // with none is a ledge's end, where snow rounds off.
            let (l, r) = (near(x as isize - 2, y), near(x as isize + 2, y));
            let slope = match (l, r) {
                (Some(a), Some(b)) => (b - a).abs() / 4.0,
                (Some(a), None) | (None, Some(a)) => (a - y as f32).abs() / 2.0,
                (None, None) => 0.0,
            };
            let ends = if l.is_none() || r.is_none() { 0.55 } else { 1.0 };
            let hold = (1.0 - slope / s.slope_limit.max(1e-3)).clamp(0.0, 1.0) * ends;
            let t = (depth * hold + s.bumps * lumps(x) * hold).max(0.0);
            if t <= 0.0 {
                continue;
            }
            // Over the surface, filling the clear texels above it, the top
            // texel partly; and one texel into it, over the seam.
            let whole = t.floor() as usize;
            for k in 0..=whole.min(y) {
                let yy = y - k;
                if k > 0 && solid(x, yy) {
                    break;
                }
                let a = if k == whole { t - whole as f32 } else { 1.0 };
                let a = if k == 0 { 1.0f32.min(t) } else { a };
                let i = yy * w + x;
                amount[i] = amount[i].max(a);
            }
        }
    }
    // Soften across columns, so caps are rounded rather than stepped.
    let mut soft = amount.clone();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if amount[i] <= 0.0 {
                continue;
            }
            let l = if x > 0 { amount[i - 1] } else { 0.0 };
            let r = if x + 1 < w { amount[i + 1] } else { 0.0 };
            soft[i] = amount[i] * 0.6 + (l + r) * 0.2;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let a = soft[y * w + x];
            if a <= 0.0 {
                continue;
            }
            // A little bluer and darker towards its foot, against what it
            // lies on.
            let foot = y + 1 < h && solid(x, y + 1);
            let shade = if foot { 0.82 } else { 1.0 };
            let c = s.color;
            let i = (y * w + x) * 4;
            out[i] = (c[0] * shade * 255.0).round().clamp(0.0, 255.0) as u8;
            out[i + 1] = (c[1] * shade * 255.0).round().clamp(0.0, 255.0) as u8;
            out[i + 2] = (c[2] * 255.0).round().clamp(0.0, 255.0) as u8;
            out[i + 3] = (a * c[3] * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    RawTextureData { width: src.width, height: src.height, rgba: out }
}

/// The depth a picture is made at: the depth snapped to `step`.
pub fn shown_depth(s: &SnowCover) -> f32 {
    let step = s.step.max(0.05);
    (s.depth / step).round() * step
}

fn key_of(texture: &str, revision: u64, s: &SnowCover) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    texture.hash(&mut h);
    revision.hash(&mut h);
    shown_depth(s).to_bits().hash(&mut h);
    for v in [s.slope_limit, s.bumps, s.cutoff, s.color[0], s.color[1], s.color[2], s.color[3]] {
        v.to_bits().hash(&mut h);
    }
    h.finish() | 1
}

/// One frame of snowfall and thaw.
pub fn tick_snow(world: &mut World, dt: f32) {
    for (_, s) in world.query_mut::<&mut SnowCover>() {
        if s.rate != 0.0 {
            s.depth = (s.depth + s.rate * dt).clamp(0.0, s.max_depth.max(0.0));
        }
    }
}

/// Remake each snowy sprite's picture whose depth or texture has moved. In
/// `carve::apply_carves`, where the pixels are.
pub fn sync(world: &mut World, assets: &mut AssetManager) {
    let mut wanted: Vec<(Entity, String, SnowCover, u64)> = Vec::new();
    for (e, (sp, s)) in world.query::<(&SpriteComponent, &SnowCover)>().iter() {
        let tex = crate::asset::resolve_texture_id(&sp.texture_id).to_string();
        let key = key_of(&tex, assets.texture_revision(&tex), s);
        if key != s.key || assets.get_texture_bytes(&picture_id(e)).is_none() {
            wanted.push((e, tex, s.clone(), key));
        }
    }
    for (e, tex, s, key) in wanted {
        let Some(src) = assets.get_texture_bytes(&tex) else { continue };
        let picture = snow_picture(src, &s, shown_depth(&s));
        let (w, h) = (picture.width, picture.height);
        let id = picture_id(e);
        assets.insert_texture_bytes(id.clone(), picture);
        // Inserting replaces the pixels, not a copy already on the GPU, so
        // the whole picture is marked to be written again.
        assets.mark_texture_dirty(&id, 0, 0, w, h);
        if let Ok(mut real) = world.get::<&mut SnowCover>(e) {
            real.key = key;
            real.picture = id;
        }
    }
}

/// Whether a snowy entity has anything to draw.
pub fn shows(s: &SnowCover) -> bool {
    shown_depth(s) > 0.0 && !s.picture.is_empty()
}

/// `Snow.set(e, field, value)`: any field by name.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut s = world.get::<&mut SnowCover>(e).map_err(|_| "no snow_cover on that entity".to_string())?;
    let mut v = serde_json::to_value(&*s).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("snow_cover has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    let mut next: SnowCover = serde_json::from_value(v).map_err(|e| format!("snow_cover.{field}: {e}"))?;
    next.key = s.key;
    next.picture = std::mem::take(&mut s.picture);
    *s = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w` x `h` picture solid where `f` says.
    fn pic(w: u32, h: u32, f: impl Fn(u32, u32) -> bool) -> RawTextureData {
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[100, 80, 60, if f(x, y) { 255 } else { 0 }]);
            }
        }
        RawTextureData { width: w, height: h, rgba }
    }

    fn alpha(t: &RawTextureData, x: u32, y: u32) -> u8 {
        t.rgba[((y * t.width + x) * 4 + 3) as usize]
    }

    /// How many texels of snow lie over column `x`.
    fn column(t: &RawTextureData, x: u32) -> u32 {
        (0..t.height).filter(|&y| alpha(t, x, y) > 128).count() as u32
    }

    #[test]
    fn snow_lies_on_a_flat_top_and_not_beside_it() {
        // A block, solid from row 20 down, columns 8..40.
        let block = pic(48, 32, |x, y| y >= 20 && (8..40).contains(&x));
        let s = SnowCover { bumps: 0.0, ..Default::default() };
        let snow = snow_picture(&block, &s, 6.0);
        assert!(column(&snow, 24) >= 6, "about six deep over the middle: {}", column(&snow, 24));
        assert!(alpha(&snow, 24, 15) > 200 && alpha(&snow, 24, 12) == 0, "above the top, not higher");
        assert_eq!(column(&snow, 4), 0, "none beside the block");
        assert!(column(&snow, 9) < column(&snow, 24), "thinner off the ledge's end");
        assert_eq!(alpha(&snow, 24, 25), 0, "none inside the block");
    }

    #[test]
    fn a_steep_roof_holds_less_than_a_flat_one() {
        // A shallow slope (one rise in four) and a steep one (two in one).
        let shallow = pic(64, 64, |x, y| y as f32 >= 30.0 + x as f32 * 0.25);
        let steep = pic(64, 64, |x, y| y as f32 >= 2.0 + x as f32 * 0.9);
        let s = SnowCover { bumps: 0.0, ..Default::default() };
        let a = column(&snow_picture(&shallow, &s, 8.0), 32);
        let b = column(&snow_picture(&steep, &s, 8.0), 32);
        assert!(a > b + 2, "the shallow roof holds more: {a} vs {b}");
        let cliff = pic(64, 64, |x, _| x >= 32);
        assert_eq!(column(&snow_picture(&cliff, &s, 8.0), 31), 0, "a wall holds none");
    }

    #[test]
    fn it_falls_settles_and_melts() {
        let mut w = World::new();
        let e = w.spawn((SnowCover { rate: 2.0, max_depth: 5.0, ..Default::default() },));
        for _ in 0..60 {
            tick_snow(&mut w, 0.1);
        }
        assert_eq!(w.get::<&SnowCover>(e).unwrap().depth, 5.0, "settled at its deepest");
        w.get::<&mut SnowCover>(e).unwrap().rate = -1.0;
        for _ in 0..100 {
            tick_snow(&mut w, 0.1);
        }
        assert_eq!(w.get::<&SnowCover>(e).unwrap().depth, 0.0, "melted away");
    }

    #[test]
    fn the_picture_is_remade_only_when_the_depth_moves_a_step() {
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes("hut.png".into(), pic(16, 16, |_, y| y >= 8));
        let mut w = World::new();
        let e = w.spawn((
            SpriteComponent { texture_id: "hut.png".into(), ..Default::default() },
            SnowCover { depth: 3.0, step: 1.0, ..Default::default() },
        ));
        sync(&mut w, &mut assets);
        let key = w.get::<&SnowCover>(e).unwrap().key;
        assert!(key != 0 && assets.get_texture_bytes(&picture_id(e)).is_some());
        w.get::<&mut SnowCover>(e).unwrap().depth = 3.2;
        sync(&mut w, &mut assets);
        assert_eq!(w.get::<&SnowCover>(e).unwrap().key, key, "under a step: kept");
        w.get::<&mut SnowCover>(e).unwrap().depth = 4.0;
        sync(&mut w, &mut assets);
        assert_ne!(w.get::<&SnowCover>(e).unwrap().key, key, "a step on: remade");
    }
}
