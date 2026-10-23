//! Light through a tile grid (Oct 2026): the kind a dug-out world wants -
//! a torch on every ledge, glowing mushrooms, lava - where a point light
//! each would cost a shadow-casting light per torch, and light should spill
//! round corners and die in the rock rather than stop at a line.
//!
//! A [`TileLight`] on a tilemap flood-fills light through its cells, one
//! colour channel at a time: the brightest of every source's light at each
//! cell, falling off by `decay` for each open cell it enters (the 8
//! neighbours, a diagonal by `decay^sqrt 2`, never squeezing between two
//! solid corners) and by `solid_decay` into rock - into rock only: light
//! that has entered a solid cell lights the rock behind it and never comes
//! out of the far side, so a wall's face is lit and a one-cell wall still
//! blocks. A max per channel rather than a sum is what makes the cost one
//! pass over the map whatever the number of sources (a priority queue per
//! channel, seeded with every source at once): a hundred torches cost what
//! one does. Sources are [`TileLightSource`] entities (a torch, a lantern on
//! a walking miner) and the map's own tiles named in `emitters` (lava,
//! mushrooms, crystal).
//!
//! The result is a texture of one texel per cell (`__tile_light/...`,
//! filtered linearly, so the cells blend into a smooth glow), made where the
//! pixels are (`carve::apply_carves`) only when the map, a field or a source
//! (its cell, colour or intensity) changed - at most `max_rate` times a
//! second - and drawn as a lightmap over the map: [`quads`] is read both by
//! `render_lightmaps_wgpu`, into the light buffer, and by `gather_lights`,
//! so `light_at_textured` sees what the screen shows and no host was wired.
//! Orthogonal maps only.

use std::collections::BinaryHeap;

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::asset::{AssetManager, RawTextureData};
use crate::ecs::components::{Transform, WorldTransform};
use crate::renderer::lighting::LightmapQuad;
use crate::tilemap::{TileOrientation, Tilemap};

/// Where every tile light's texture lives.
pub const TEXTURE_PREFIX: &str = "__tile_light/";

/// Light under this is dark: where the flood stops.
const FLOOR: f32 = 1.0 / 512.0;

/// A tile of the map that gives off light wherever it is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TileEmitter {
    /// The tile's global id, in any layer.
    pub tile: u32,
    /// Linear colour.
    #[serde(default = "default_color")]
    pub color: [f32; 3],
    #[serde(default = "one")]
    pub intensity: f32,
}

/// Light flooded through this tilemap's cells.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileLight {
    /// Layers whose filled cells are rock to the light. Empty: every layer
    /// flagged `collision`.
    #[serde(default)]
    pub solid_layers: Vec<String>,
    /// How much light is left for each open cell it enters (0-1): reach is
    /// about `ln(512) / -ln(decay)` cells from a source of 1.
    #[serde(default = "default_decay")]
    pub decay: f32,
    /// The same into rock: how far light lights a wall's face before dying.
    /// 0 stops it at the first solid cell.
    #[serde(default = "default_solid_decay")]
    pub solid_decay: f32,
    /// Tiles that give off light.
    #[serde(default)]
    pub emitters: Vec<TileEmitter>,
    /// Scales the whole map's light as it is drawn (the stored light is
    /// clipped at 1, so this is how it goes past).
    #[serde(default = "one")]
    pub intensity: f32,
    /// Rebuilds a second at most, while things keep changing.
    #[serde(default = "default_rate")]
    pub max_rate: f32,
    /// What the texture was made from (never saved).
    #[serde(skip)]
    pub key: u64,
    /// When it was made, in `profiling::now_ms` (never saved).
    #[serde(skip)]
    pub built_ms: f64,
}

impl Default for TileLight {
    fn default() -> Self {
        Self {
            solid_layers: Vec::new(),
            decay: default_decay(),
            solid_decay: default_solid_decay(),
            emitters: Vec::new(),
            intensity: 1.0,
            max_rate: default_rate(),
            key: 0,
            built_ms: f64::NEG_INFINITY,
        }
    }
}

/// Something that lights the tile light of whatever map it stands in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TileLightSource {
    /// Linear colour.
    #[serde(default = "default_color")]
    pub color: [f32; 3],
    #[serde(default = "one")]
    pub intensity: f32,
    #[serde(default = "yes")]
    pub on: bool,
}

impl Default for TileLightSource {
    fn default() -> Self {
        Self { color: default_color(), intensity: 1.0, on: true }
    }
}

fn default_color() -> [f32; 3] {
    [1.0, 0.75, 0.45]
}
fn one() -> f32 {
    1.0
}
fn yes() -> bool {
    true
}
fn default_decay() -> f32 {
    0.85
}
fn default_solid_decay() -> f32 {
    0.4
}
fn default_rate() -> f32 {
    30.0
}

/// The texture a map entity's light is kept in.
pub fn texture_id(entity: Entity) -> String {
    format!("{TEXTURE_PREFIX}{}", entity.to_bits().get())
}

fn placed(t: &Transform, w: Option<&WorldTransform>) -> Transform {
    match w {
        Some(w) => Transform { position: w.position, rotation: w.rotation, scale: w.scale },
        None => t.clone(),
    }
}

/// Which cells are rock to the light, row-major over `size_in_cells`.
pub fn solid_cells(tm: &Tilemap, tl: &TileLight) -> Vec<bool> {
    let (w, h) = tm.size_in_cells();
    let mut solid = vec![false; (w * h) as usize];
    for layer in &tm.layers {
        let named = if tl.solid_layers.is_empty() { layer.collision } else { tl.solid_layers.contains(&layer.name) };
        if !named {
            continue;
        }
        for y in 0..layer.height.min(h) {
            for x in 0..layer.width.min(w) {
                if layer.get_tile(x, y).is_some() {
                    solid[(y * w + x) as usize] = true;
                }
            }
        }
    }
    solid
}

/// One seed of light: a cell and its linear colour.
pub type Seed = (u32, u32, [f32; 3]);

/// The light at every cell (linear RGB, row-major), flooded from `seeds`.
pub fn flood(w: u32, h: u32, solid: &[bool], seeds: &[Seed], decay: f32, solid_decay: f32) -> Vec<[f32; 3]> {
    let n = (w * h) as usize;
    let mut light = vec![[0.0_f32; 3]; n];
    if n == 0 {
        return light;
    }
    let decay = decay.clamp(0.0, 0.999);
    let solid_decay = solid_decay.clamp(0.0, 0.999);
    let diag = |d: f32| d.powf(std::f32::consts::SQRT_2);
    let (open_d, rock_d) = (diag(decay), diag(solid_decay));
    let (w, h) = (w as i32, h as i32);
    let mut level = vec![0.0_f32; n];
    let mut heap: BinaryHeap<(u32, u32)> = BinaryHeap::new();
    for ch in 0..3 {
        level.iter_mut().for_each(|v| *v = 0.0);
        heap.clear();
        for &(x, y, c) in seeds {
            if x as i32 >= w || y as i32 >= h || c[ch] < FLOOR {
                continue;
            }
            let i = (y as i32 * w + x as i32) as usize;
            if c[ch] > level[i] {
                level[i] = c[ch];
                // A positive float's bits order as the float does.
                heap.push((c[ch].to_bits(), i as u32));
            }
        }
        while let Some((bits, i)) = heap.pop() {
            let v = f32::from_bits(bits);
            let i = i as usize;
            if v < level[i] {
                continue;
            }
            let (x, y) = ((i as i32) % w, (i as i32) / w);
            let from_rock = solid[i];
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let j = (ny * w + nx) as usize;
                let rock = solid[j];
                // Light in the rock stays in the rock.
                if from_rock && !rock {
                    continue;
                }
                let diagonal = dx != 0 && dy != 0;
                if diagonal && solid[(y * w + nx) as usize] && solid[(ny * w + x) as usize] {
                    continue;
                }
                let f = match (rock, diagonal) {
                    (false, false) => decay,
                    (false, true) => open_d,
                    (true, false) => solid_decay,
                    (true, true) => rock_d,
                };
                let nv = v * f;
                if nv >= FLOOR && nv > level[j] {
                    level[j] = nv;
                    heap.push((nv.to_bits(), j as u32));
                }
            }
        }
        for (l, v) in light.iter_mut().zip(&level) {
            l[ch] = *v;
        }
    }
    light
}

/// The sources lighting `tm` (at `t`): its emitter tiles and every
/// [`TileLightSource`] standing in one of its cells.
fn seeds(world: &World, tm: &Tilemap, tl: &TileLight, t: &Transform) -> Vec<Seed> {
    let (w, h) = tm.size_in_cells();
    let mut out = Vec::new();
    if !tl.emitters.is_empty() {
        for layer in &tm.layers {
            for y in 0..layer.height.min(h) {
                for x in 0..layer.width.min(w) {
                    if let Some(gid) = layer.get_tile(x, y)
                        && let Some(e) = tl.emitters.iter().find(|e| e.tile == gid)
                    {
                        out.push((x, y, e.color.map(|c| c * e.intensity)));
                    }
                }
            }
        }
    }
    let (sin, cos) = (-t.rotation).sin_cos();
    for (_, (st, sw, src)) in world.query::<(&Transform, Option<&WorldTransform>, &TileLightSource)>().iter() {
        if !src.on || src.intensity <= 0.0 {
            continue;
        }
        let p = placed(st, sw).position - t.position;
        let local = glam::Vec2::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos);
        let local = local / t.scale.max(glam::Vec2::splat(1e-6));
        let (c, r) = tm.local_to_grid(local.x, local.y);
        if c < 0.0 || r < 0.0 || c >= w as f32 || r >= h as f32 {
            continue;
        }
        out.push((c as u32, r as u32, src.color.map(|v| v * src.intensity)));
    }
    out
}

fn key_of(tm: &Tilemap, tl: &TileLight, seeds: &[Seed]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    mix(tm.revision_key());
    mix(tl.decay.to_bits() as u64);
    mix(tl.solid_decay.to_bits() as u64);
    for l in &tl.solid_layers {
        l.bytes().for_each(|b| mix(b as u64));
    }
    for e in &tl.emitters {
        mix(e.tile as u64);
        e.color.iter().for_each(|c| mix(c.to_bits() as u64));
        mix(e.intensity.to_bits() as u64);
    }
    mix(seeds.len() as u64);
    for (x, y, c) in seeds {
        mix(((*x as u64) << 32) | *y as u64);
        c.iter().for_each(|v| mix(v.to_bits() as u64));
    }
    h.max(1)
}

fn linear_to_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// Rebuild each tile light whose map or sources changed. Called by
/// `carve::apply_carves`, every host's step with its assets.
pub fn sync(world: &mut World, assets: &mut AssetManager) {
    let now = crate::profiling::now_ms();
    let mut built: Vec<(Entity, u64, String, RawTextureData)> = Vec::new();
    {
        let mut q = world.query::<(&Transform, Option<&WorldTransform>, &Tilemap, &TileLight)>();
        for (e, (t, w, tm, tl)) in q.iter() {
            if tm.orientation != TileOrientation::Orthogonal {
                continue;
            }
            let id = texture_id(e);
            let t = placed(t, w);
            let seeds = seeds(world, tm, tl, &t);
            let key = key_of(tm, tl, &seeds);
            let have = assets.get_texture_bytes(&id).is_some();
            if have && key == tl.key {
                continue;
            }
            let gap = 1000.0 / tl.max_rate.max(0.1) as f64;
            if have && now - tl.built_ms < gap {
                continue;
            }
            let (cw, ch) = tm.size_in_cells();
            if cw == 0 || ch == 0 {
                continue;
            }
            let solid = solid_cells(tm, tl);
            let light = flood(cw, ch, &solid, &seeds, tl.decay, tl.solid_decay);
            let mut rgba = Vec::with_capacity(light.len() * 4);
            for l in &light {
                rgba.extend_from_slice(&[linear_to_srgb(l[0]), linear_to_srgb(l[1]), linear_to_srgb(l[2]), 255]);
            }
            built.push((e, key, id, RawTextureData { width: cw, height: ch, rgba }));
        }
    }
    for (e, key, id, tex) in built {
        let (w, h) = (tex.width, tex.height);
        match assets.texture_bytes_mut(&id) {
            Some(old) if old.width == w && old.height == h => {
                old.rgba = tex.rgba;
                assets.mark_texture_dirty(&id, 0, 0, w, h);
            }
            _ => assets.insert_texture_bytes(id, tex),
        }
        if let Ok(mut tl) = world.get::<&mut TileLight>(e) {
            tl.key = key;
            tl.built_ms = now;
        }
    }
}

/// Each tile light as the lightmap quad that draws it: over its map, one
/// texel a cell. Read by `render_lightmaps_wgpu` and `gather_lights`.
pub fn quads(world: &World) -> Vec<LightmapQuad> {
    let mut out = Vec::new();
    for (e, (t, w, tm, tl)) in world.query::<(&Transform, Option<&WorldTransform>, &Tilemap, &TileLight)>().iter() {
        if tm.orientation != TileOrientation::Orthogonal || tl.key == 0 {
            continue;
        }
        let (cw, ch) = tm.size_in_cells();
        let t = placed(t, w);
        let size = glam::Vec2::new(cw as f32 * tm.tile_size[0], ch as f32 * tm.tile_size[1]) * t.scale;
        let half = glam::Vec2::new(size.x * 0.5, -size.y * 0.5);
        let center = t.position + glam::Vec2::from_angle(t.rotation).rotate(half);
        out.push(LightmapQuad {
            center,
            rotation: t.rotation,
            size,
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            intensity: tl.intensity,
            texture_id: texture_id(e),
        });
    }
    out
}

/// Set any field of the entity's [`TileLight`] or [`TileLightSource`] by
/// name (`TileLight.set`).
pub fn set_tile_light_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    fn set<T: Serialize + serde::de::DeserializeOwned>(c: &mut T, field: &str, value: &serde_json::Value, what: &str) -> Result<(), String> {
        let mut whole = serde_json::to_value(&*c).map_err(|e| e.to_string())?;
        let map = whole.as_object_mut().ok_or("not an object")?;
        if !map.contains_key(field) {
            return Err(format!("'{field}' is not a {what} field"));
        }
        map.insert(field.to_string(), value.clone());
        *c = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
        Ok(())
    }
    if let Ok(mut tl) = world.get::<&mut TileLight>(entity) {
        let built = tl.built_ms;
        set(&mut *tl, field, value, "tile_light")?;
        tl.built_ms = built;
        return Ok(());
    }
    if let Ok(mut s) = world.get::<&mut TileLightSource>(entity) {
        return set(&mut *s, field, value, "tile_light_source");
    }
    Err("no tile_light or tile_light_source on that entity".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(l: &[[f32; 3]], w: u32, x: u32, y: u32) -> [f32; 3] {
        l[(y * w + x) as usize]
    }

    #[test]
    fn light_falls_off_by_cell() {
        let solid = vec![false; 9];
        let l = flood(9, 1, &solid, &[(0, 0, [1.0, 1.0, 1.0])], 0.5, 0.0);
        assert_eq!(at(&l, 9, 0, 0)[0], 1.0);
        assert!((at(&l, 9, 1, 0)[0] - 0.5).abs() < 1e-6);
        assert!((at(&l, 9, 3, 0)[0] - 0.125).abs() < 1e-6);
    }

    #[test]
    fn a_thin_wall_blocks_and_its_face_is_lit() {
        // A 7x1 corridor with a solid cell at 3.
        let mut solid = vec![false; 7];
        solid[3] = true;
        let l = flood(7, 1, &solid, &[(0, 0, [1.0, 0.0, 0.0])], 0.9, 0.5);
        assert!(at(&l, 7, 3, 0)[0] > 0.3, "the wall's face is lit");
        assert_eq!(at(&l, 7, 4, 0)[0], 0.0, "and nothing comes out behind it");
        assert_eq!(at(&l, 7, 0, 0)[1], 0.0, "red stays red");
    }

    #[test]
    fn light_turns_corners_and_squeezes_through_no_diagonal_crack() {
        // . . #
        // # . #     the source at (0,0), round the bend to (1,2)
        // # . .
        let s = |x: u32, y: u32| matches!((x, y), (2, 0) | (0, 1) | (2, 1) | (0, 2));
        let solid: Vec<bool> = (0..9).map(|i| s(i % 3, i / 3)).collect();
        let l = flood(3, 3, &solid, &[(0, 0, [1.0; 3])], 0.8, 0.0);
        assert!(at(&l, 3, 2, 2)[0] > 0.3, "round the bend");
        // Two cells touching only at a corner, both sides rock: no way
        // through.
        let solid = vec![false, true, true, false];
        let l = flood(2, 2, &solid, &[(0, 0, [1.0; 3])], 0.8, 0.0);
        assert_eq!(at(&l, 2, 1, 1)[0], 0.0);
    }

    #[test]
    fn many_sources_are_the_brightest_not_the_sum() {
        let solid = vec![false; 3];
        let one = flood(3, 1, &solid, &[(1, 0, [0.5; 3])], 0.5, 0.0);
        let two = flood(3, 1, &solid, &[(1, 0, [0.5; 3]), (1, 0, [0.5; 3])], 0.5, 0.0);
        assert_eq!(one, two);
    }
}
