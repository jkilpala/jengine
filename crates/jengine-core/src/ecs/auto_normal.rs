//! Light for flat art (Oct 2026): a sprite with no normal map is lit as a
//! flat card, which is right for a card and wrong for the hill, the castle
//! and the dragon it shows. An [`AutoNormal`] works out a normal map from
//! the sprite's own silhouette - every opaque texel's distance from the
//! nearest transparent one, rounded into a bevel `bevel` texels wide - so a
//! light moving behind it rounds its edges with no normal map painted.
//!
//! The map is made where the pixels are, beside the other generated
//! textures (`carve::apply_carves`), kept under `__auto_normal/...` (shared by
//! every sprite of one texture and settings, remade when the texture
//! changes - `AssetManager::texture_revision`) and written into the
//! entity's material as its `normal_map_id`, so the normal pass draws it
//! like a painted one and no host was wired. The map follows the convention
//! of the showcase's painted ones: `(-dh/dx, dh/dy, 1)`, x right and y down
//! the image.

use hecs::World;
use serde::{Deserialize, Serialize};

use crate::asset::{AssetManager, RawTextureData};
use crate::ecs::components::{MaterialComponent, SpriteComponent};

/// How the bevel rises from the edge.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BevelShape {
    /// A quarter circle: steep at the edge, flat on top - a rounded thing.
    #[default]
    Round,
    /// A straight slope - a cut, chamfered edge.
    Linear,
    /// Rising all the way to the middle - a cushion.
    Pillow,
}

/// `auto_normal`: light this sprite as if rounded at its edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoNormal {
    /// How wide the bevel is, in texels of the texture.
    pub bevel: f32,
    /// How steep it is.
    pub strength: f32,
    pub shape: BevelShape,
    /// Alpha below which a texel is outside.
    pub cutoff: f32,
    /// What the map was last made from - runtime state, never saved.
    #[serde(skip)]
    pub key: u64,
}

impl Default for AutoNormal {
    fn default() -> Self {
        Self { bevel: 12.0, strength: 2.0, shape: BevelShape::Round, cutoff: 0.5, key: 0 }
    }
}

/// The id a sprite texture's generated normal map is kept under.
pub fn map_id(texture: &str, a: &AutoNormal) -> String {
    format!("__auto_normal/{texture}#{}-{}-{:?}-{}", a.bevel, a.strength, a.shape, a.cutoff)
}

/// Each texel's distance (in texels) from the nearest outside one, capped
/// at `cap`; outside texels are 0. Exact Euclidean (Felzenszwalb and
/// Huttenlocher's separable transform): a chamfer distance steps in
/// octagons, and the slope of a stepped field is streaks across every rim.
pub fn inside_distance(alpha: &[f32], w: usize, h: usize, cutoff: f32, cap: f32) -> Vec<f32> {
    // Padded by one texel of outside all round: the image's own border is
    // outside, so a sprite filling its texture still rounds at the edge.
    let (pw, ph) = (w + 2, h + 2);
    let inf = 1.0e12_f64;
    let mut f = vec![0.0_f64; pw * ph];
    for y in 0..h {
        for x in 0..w {
            if alpha[y * w + x] >= cutoff {
                f[(y + 1) * pw + x + 1] = inf;
            }
        }
    }
    let mut line = vec![0.0_f64; pw.max(ph)];
    let mut out = vec![0.0_f64; pw.max(ph)];
    for x in 0..pw {
        for y in 0..ph {
            line[y] = f[y * pw + x];
        }
        edt_1d(&line[..ph], &mut out[..ph]);
        for y in 0..ph {
            f[y * pw + x] = out[y];
        }
    }
    for y in 0..ph {
        line[..pw].copy_from_slice(&f[y * pw..(y + 1) * pw]);
        edt_1d(&line[..pw], &mut out[..pw]);
        f[y * pw..(y + 1) * pw].copy_from_slice(&out[..pw]);
    }
    let mut d = vec![0.0_f32; w * h];
    for y in 0..h {
        for x in 0..w {
            d[y * w + x] = (f[(y + 1) * pw + x + 1].sqrt() as f32).min(cap);
        }
    }
    d
}

/// A box blur of `radius` texels across then down, in place; the border
/// is held (clamped), so a sprite at its texture's edge does not darken in.
fn box_blur(v: &mut [f32], w: usize, h: usize, radius: usize) {
    let span = (2 * radius + 1) as f32;
    let mut tmp = vec![0.0_f32; w.max(h)];
    for y in 0..h {
        let row = &mut v[y * w..(y + 1) * w];
        for (x, t) in tmp[..w].iter_mut().enumerate() {
            let mut sum = 0.0;
            for k in 0..=2 * radius {
                let i = (x + k).saturating_sub(radius).min(w - 1);
                sum += row[i];
            }
            *t = sum / span;
        }
        row.copy_from_slice(&tmp[..w]);
    }
    for x in 0..w {
        for (y, t) in tmp[..h].iter_mut().enumerate() {
            let mut sum = 0.0;
            for k in 0..=2 * radius {
                let j = (y + k).saturating_sub(radius).min(h - 1);
                sum += v[j * w + x];
            }
            *t = sum / span;
        }
        for y in 0..h {
            v[y * w + x] = tmp[y];
        }
    }
}

/// One row of the squared distance transform: `out[q] = min_p (q - p)^2 + f[p]`,
/// by the lower envelope of the parabolas rooted at each `p`.
fn edt_1d(f: &[f64], out: &mut [f64]) {
    let n = f.len();
    let mut v = vec![0usize; n];
    let mut z = vec![0.0_f64; n + 1];
    let mut k = 0usize;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    let sq = |q: usize| (q * q) as f64;
    for q in 1..n {
        loop {
            let p = v[k];
            let s = ((f[q] + sq(q)) - (f[p] + sq(p))) / (2.0 * q as f64 - 2.0 * p as f64);
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }
            if s <= z[k] {
                // k == 0 and the new parabola wins everywhere.
                v[0] = q;
                z[1] = f64::INFINITY;
                break;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f64::INFINITY;
            break;
        }
    }
    k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f64 - p as f64;
        *o = dq * dq + f[p];
    }
}

/// The normal map (RGBA8) of `src`'s silhouette, by `a`.
pub fn normal_map(src: &RawTextureData, a: &AutoNormal) -> RawTextureData {
    let (w, h) = (src.width as usize, src.height as usize);
    let alpha: Vec<f32> = src.rgba.chunks_exact(4).map(|p| p[3] as f32 / 255.0).collect();
    let bevel = a.bevel.max(0.5);
    let mut dist = inside_distance(&alpha, w, h, a.cutoff, bevel);
    // Even an exact distance is stepped wherever the outline is: a binary
    // silhouette is stairs. Smoothed over a fraction of the bevel, the slope
    // follows the curve the stairs stand for.
    let radius = ((bevel / 8.0).round() as usize).clamp(1, 8);
    for _ in 0..2 {
        box_blur(&mut dist, w, h, radius);
    }
    let height: Vec<f32> = dist
        .iter()
        .map(|&d| {
            let t = (d / bevel).clamp(0.0, 1.0);
            match a.shape {
                BevelShape::Round => (1.0 - (1.0 - t) * (1.0 - t)).sqrt(),
                BevelShape::Linear => t,
                BevelShape::Pillow => t * t * (3.0 - 2.0 * t),
            }
        })
        .collect();
    let hgt = |x: isize, y: isize| -> f32 {
        let x = x.clamp(0, w as isize - 1) as usize;
        let y = y.clamp(0, h as isize - 1) as usize;
        height[y * w + x]
    };
    // Steepness per texel: a bevel of `bevel` texels rising 1 has a slope
    // of 1 / bevel, which `strength` scales back up to a visible tilt.
    let k = a.strength * bevel * 0.5;
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h as isize {
        for x in 0..w as isize {
            let i = (y as usize * w + x as usize) * 4;
            // Sobel: a central difference smoothed across, over 8, so it
            // is the same slope as one but without a texel's jitter.
            let dx = ((hgt(x + 1, y - 1) + 2.0 * hgt(x + 1, y) + hgt(x + 1, y + 1))
                - (hgt(x - 1, y - 1) + 2.0 * hgt(x - 1, y) + hgt(x - 1, y + 1)))
                * 0.25
                * k;
            let dy = ((hgt(x - 1, y + 1) + 2.0 * hgt(x, y + 1) + hgt(x + 1, y + 1))
                - (hgt(x - 1, y - 1) + 2.0 * hgt(x, y - 1) + hgt(x + 1, y - 1)))
                * 0.25
                * k;
            let n = glam::Vec3::new(-dx, dy, 1.0).normalize();
            let enc = |v: f32| ((v * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
            rgba[i] = enc(n.x);
            rgba[i + 1] = enc(n.y);
            rgba[i + 2] = enc(n.z);
            rgba[i + 3] = src.rgba[i + 3];
        }
    }
    RawTextureData { width: src.width, height: src.height, rgba }
}

fn key_of(texture: &str, revision: u64, a: &AutoNormal) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut s = std::collections::hash_map::DefaultHasher::new();
    texture.hash(&mut s);
    revision.hash(&mut s);
    a.bevel.to_bits().hash(&mut s);
    a.strength.to_bits().hash(&mut s);
    a.shape.hash(&mut s);
    a.cutoff.to_bits().hash(&mut s);
    s.finish() | 1
}

/// Make (or reuse) each auto-normal sprite's map and point its material at
/// it. In `carve::apply_carves`, where the pixels are.
pub fn sync(world: &mut World, assets: &mut AssetManager) {
    let mut wanted: Vec<(hecs::Entity, String, AutoNormal)> = Vec::new();
    for (e, (s, a)) in world.query::<(&SpriteComponent, &AutoNormal)>().iter() {
        let tex = crate::asset::resolve_texture_id(&s.texture_id).to_string();
        let key = key_of(&tex, assets.texture_revision(&tex), a);
        if key != a.key || assets.get_texture_bytes(&map_id(&tex, a)).is_none() {
            wanted.push((e, tex, a.clone()));
        }
    }
    for (e, tex, a) in wanted {
        let Some(src) = assets.get_texture_bytes(&tex) else { continue };
        let id = map_id(&tex, &a);
        let key = key_of(&tex, assets.texture_revision(&tex), &a);
        let map = normal_map(src, &a);
        assets.insert_texture_bytes(id.clone(), map);
        if let Ok(mut an) = world.get::<&mut AutoNormal>(e) {
            an.key = key;
        }
        let missing = match world.get::<&mut MaterialComponent>(e) {
            Ok(mut m) => {
                if m.normal_map_id.as_deref().is_none_or(|n| n.is_empty() || n.starts_with("__auto_normal/")) {
                    m.normal_map_id = Some(id.clone());
                }
                None
            }
            Err(_) => Some(MaterialComponent { material_id: "lit_sprite".into(), normal_map_id: Some(id.clone()), ..Default::default() }),
        };
        if let Some(m) = missing {
            let _ = world.insert_one(e, m);
        }
    }
}

/// `AutoNormal.set(e, field, value)`: any field by name.
pub fn set_auto_normal_field(world: &World, entity: hecs::Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut a = world.get::<&mut AutoNormal>(entity).map_err(|_| "no auto_normal on that entity".to_string())?;
    let mut whole = serde_json::to_value(&*a).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not an auto_normal field"));
    }
    map.insert(field.to_string(), value.clone());
    *a = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(w: u32, inset: u32) -> RawTextureData {
        let mut rgba = vec![0u8; (w * w * 4) as usize];
        for y in inset..w - inset {
            for x in inset..w - inset {
                let i = ((y * w + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        RawTextureData { width: w, height: w, rgba }
    }

    fn px(t: &RawTextureData, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * t.width + x) * 4) as usize;
        [t.rgba[i], t.rgba[i + 1], t.rgba[i + 2], t.rgba[i + 3]]
    }

    #[test]
    fn distance_counts_in_from_the_edge() {
        let s = square(20, 4);
        let alpha: Vec<f32> = s.rgba.chunks_exact(4).map(|p| p[3] as f32 / 255.0).collect();
        let d = inside_distance(&alpha, 20, 20, 0.5, 100.0);
        assert_eq!(d[0], 0.0, "outside");
        assert!((d[10 * 20 + 4] - 1.0).abs() < 0.01, "the first texel in: {}", d[10 * 20 + 4]);
        assert!((d[10 * 20 + 9] - 6.0).abs() < 0.01, "six in: {}", d[10 * 20 + 9]);
    }

    #[test]
    fn edges_tilt_outward_and_the_top_is_flat() {
        let map = normal_map(&square(40, 4), &AutoNormal { bevel: 6.0, ..Default::default() });
        let centre = px(&map, 20, 20);
        assert!((centre[0] as i32 - 128).abs() <= 1 && (centre[1] as i32 - 128).abs() <= 1 && centre[2] > 250, "{centre:?}");
        let left = px(&map, 5, 20);
        assert!(left[0] < 100, "the left edge faces left: {left:?}");
        let right = px(&map, 34, 20);
        assert!(right[0] > 156, "the right edge faces right: {right:?}");
        // Image rows run down: the top edge's height rises down the image.
        let top = px(&map, 20, 5);
        assert!(top[1] > 156, "the top edge: {top:?}");
        assert_eq!(px(&map, 1, 1)[3], 0, "the map keeps the silhouette's alpha");
    }

    #[test]
    fn a_sprite_gets_its_map_and_a_material_naming_it() {
        let mut world = World::new();
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes("hill.png".into(), square(32, 2));
        let e = world.spawn((SpriteComponent { texture_id: "hill.png".into(), ..Default::default() }, AutoNormal::default()));
        sync(&mut world, &mut assets);
        let id = world.get::<&MaterialComponent>(e).unwrap().normal_map_id.clone().expect("a normal map");
        assert!(id.starts_with("__auto_normal/hill.png"));
        assert!(assets.get_texture_bytes(&id).is_some(), "and the map itself");
        let key = world.get::<&AutoNormal>(e).unwrap().key;
        sync(&mut world, &mut assets);
        assert_eq!(world.get::<&AutoNormal>(e).unwrap().key, key, "nothing changed: not remade");
    }

    /// A chamfer distance steps in octagons, which tilted a disc's rim
    /// toward eight directions and drew its slope as streaks.
    #[test]
    fn a_discs_rim_points_straight_out_at_every_angle() {
        let (w, r) = (128u32, 50.0_f32);
        let mut rgba = vec![0u8; (w * w * 4) as usize];
        for y in 0..w {
            for x in 0..w {
                let (dx, dy) = (x as f32 + 0.5 - 64.0, y as f32 + 0.5 - 64.0);
                rgba[((y * w + x) * 4 + 3) as usize] = if dx * dx + dy * dy <= r * r { 255 } else { 0 };
            }
        }
        let t = RawTextureData { width: w, height: w, rgba };
        let a = AutoNormal { bevel: 16.0, ..Default::default() };
        let map = normal_map(&t, &a);
        for deg in [0.0_f32, 10.0, 22.5, 30.0, 45.0, 60.0, 77.0] {
            let th = deg.to_radians();
            let (x, y) = ((64.0 + th.cos() * (r - 6.0)) as u32, (64.0 + th.sin() * (r - 6.0)) as u32);
            let p = px(&map, x, y);
            let (nx, ny) = (p[0] as f32 / 127.5 - 1.0, p[1] as f32 / 127.5 - 1.0);
            let got = (-ny).atan2(nx).to_degrees();
            assert!((got - deg).abs() < 5.0, "at {deg} degrees the rim points to {got}");
        }
    }
}
