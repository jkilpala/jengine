//! **Fog of war** (Oct 2026): a map dark but for what its scouts see.
//!
//! A [`FogOfWar`] is a grid of cells over an area (`size`, `cell`), each with
//! how much of it is **visible** now and how much has ever been
//! **explored**. Every [`Sight`] - a scout, a tower - reveals the cells
//! within its `radius` (softly, over its last `soft` units), the visible
//! share easing up at `reveal_speed` and back down as it leaves; what was
//! once seen stays explored. Its entity's sprite shows it: a texture of one
//! texel per cell (`__fog/...`, linear, so edges are soft) that covers
//! unexplored land in `unseen_color` and dims explored land not in sight to
//! `explored_alpha` - so give the entity a `sprite` over the area, unlit and
//! above the map. A [`FogHidden`] entity - an enemy, a treasure - is drawn
//! only while a fog says its cell is visible: `in_view_layers`, which every
//! world drawing step asks, leaves it out otherwise.
//!
//! The fog is stepped in `tick_motion` and its texture made in
//! `carve::apply_carves`, the step every host makes with its assets - so no
//! host was wired. A script reads `Entity.get_custom(e, "Fog", "visible")` on
//! a hidden thing and `"FogOfWar", "explored"` (the share of the map seen) on
//! the fog. `FogOfWar.reset(e)` makes the map unseen again (a second run on
//! the same scene) and `FogOfWar.set(e, field, value)` writes any field -
//! [`reset_fog`], [`set_fog_of_war_field`], one shared command each.

use std::cell::Cell;

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::asset::{AssetManager, RawTextureData};
use crate::ecs::components::{CustomComponents, SpriteComponent, Transform, WorldTransform};

/// The texture id prefix of a fog's picture (linear unless a rule says).
pub const TEXTURE_PREFIX: &str = "__fog/";

/// A map's fog: what is seen now, and what has been.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FogOfWar {
    /// The area it covers, world units, centred on its entity.
    pub size: [f32; 2],
    /// World units a cell.
    pub cell: f32,
    /// The colour of land never seen (linear; alpha is how dark).
    pub unseen_color: [f32; 4],
    /// How much explored land out of sight is dimmed (0 none, 1 as unseen).
    pub explored_alpha: f32,
    /// How fast a cell clears as a sight reaches it, and covers as it goes
    /// (a share a second).
    pub reveal_speed: f32,
    /// The most times a second its picture is remade.
    pub max_rate: f32,
    #[serde(skip)]
    pub state: FogState,
}

impl Default for FogOfWar {
    fn default() -> Self {
        Self {
            size: [1280.0, 720.0],
            cell: 16.0,
            unseen_color: [0.01, 0.012, 0.02, 0.97],
            explored_alpha: 0.55,
            reveal_speed: 6.0,
            max_rate: 30.0,
            state: FogState::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FogState {
    pub dims: (usize, usize),
    /// Per cell, 0 to 1, row by row from the bottom.
    pub visible: Vec<f32>,
    pub explored: Vec<f32>,
    /// What the texture was last made from, and when.
    pub key: u64,
    pub built_ms: f64,
}

/// Something that sees: it reveals the fog round it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Sight {
    pub radius: f32,
    /// The rim over which it fades out, world units.
    pub soft: f32,
}

impl Default for Sight {
    fn default() -> Self {
        Self { radius: 180.0, soft: 50.0 }
    }
}

/// Drawn only while a fog sees its cell.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FogHidden {
    /// How visible its cell must be to show it (0 to 1).
    pub threshold: f32,
    /// Out of sight this frame (worked out by `tick_fog`, never saved - so a
    /// world loaded afresh, the editor's after Stop, hides nothing).
    #[serde(skip)]
    pub hidden: bool,
}

thread_local! {
    /// Whether the last fog step hid anything: the drawing passes ask
    /// [`hidden`] per drawable, and this keeps that free in every world
    /// without fog.
    static ANY_HIDDEN: Cell<bool> = const { Cell::new(false) };
}

/// Whether a fog hides `entity` this frame. Free when nothing is hidden.
pub fn hidden(world: &World, entity: Entity) -> bool {
    ANY_HIDDEN.with(Cell::get) && world.get::<&FogHidden>(entity).is_ok_and(|h| h.hidden)
}

fn placed(t: &Transform, w: Option<&WorldTransform>) -> (Vec2, f32) {
    w.map_or((t.position, t.rotation), |w| (w.position, w.rotation))
}

impl FogOfWar {
    fn ensure(&mut self) {
        let cell = self.cell.max(1.0);
        let dims = (((self.size[0] / cell).ceil() as usize).max(1), ((self.size[1] / cell).ceil() as usize).max(1));
        if self.state.dims != dims || self.state.visible.len() != dims.0 * dims.1 {
            self.state.dims = dims;
            self.state.visible = vec![0.0; dims.0 * dims.1];
            self.state.explored = vec![0.0; dims.0 * dims.1];
        }
    }

    /// The centre of cell (i, j), in the fog's frame.
    fn cell_centre(&self, i: usize, j: usize) -> Vec2 {
        let c = self.cell.max(1.0);
        Vec2::new((i as f32 + 0.5) * c - self.size[0] * 0.5, (j as f32 + 0.5) * c - self.size[1] * 0.5)
    }

    /// How visible `local` (the fog's frame) is, between the cells round it;
    /// `None` outside the fog.
    pub fn visible_at(&self, local: Vec2) -> Option<f32> {
        if local.x.abs() > self.size[0] * 0.5 || local.y.abs() > self.size[1] * 0.5 {
            return None;
        }
        let (w, h) = self.state.dims;
        if w == 0 || h == 0 {
            return Some(0.0);
        }
        let c = self.cell.max(1.0);
        let i = (((local.x + self.size[0] * 0.5) / c) as usize).min(w - 1);
        let j = (((local.y + self.size[1] * 0.5) / c) as usize).min(h - 1);
        Some(self.state.visible[j * w + i])
    }

    /// The share of the map ever seen.
    pub fn explored_share(&self) -> f32 {
        if self.state.explored.is_empty() {
            return 0.0;
        }
        self.state.explored.iter().sum::<f32>() / self.state.explored.len() as f32
    }

    /// One step: the visible share eased toward what the sights see, and
    /// explored the most it has been. `sights` are in the fog's frame.
    pub fn step(&mut self, sights: &[(Vec2, Sight)], dt: f32) {
        self.ensure();
        let (w, h) = self.state.dims;
        let rate = (self.reveal_speed.max(0.0) * dt).min(1.0);
        for j in 0..h {
            for i in 0..w {
                let p = self.cell_centre(i, j);
                let mut target: f32 = 0.0;
                for (s, sight) in sights {
                    let d = p.distance(*s);
                    let soft = sight.soft.max(1e-3);
                    let v = ((sight.radius - d) / soft).clamp(0.0, 1.0);
                    target = target.max(v * v * (3.0 - 2.0 * v));
                }
                let k = j * w + i;
                let v = &mut self.state.visible[k];
                *v += (target - *v) * rate;
                if (target - *v).abs() < 1e-3 {
                    *v = target;
                }
                let e = &mut self.state.explored[k];
                *e = e.max(*v);
            }
        }
    }

    /// Each cell's fog, as straight RGBA (top row first, as an image is).
    pub fn picture(&self) -> Vec<u8> {
        let (w, h) = self.state.dims;
        let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let rgb = [0, 1, 2].map(|c| srgb8(self.unseen_color[c]));
        let mut out = Vec::with_capacity(w * h * 4);
        for row in (0..h).rev() {
            for i in 0..w {
                let k = row * w + i;
                let (v, e) = (self.state.visible[k], self.state.explored[k]);
                let a = (1.0 - e).max(self.explored_alpha * (1.0 - v)) * self.unseen_color[3];
                out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], to8(a)]);
            }
        }
        out
    }
}

/// `FogOfWar.reset(e)`: the map unseen again - every cell's visible and
/// explored share back to none, and the picture remade on the next sync. A
/// second run of a game on the same scene starts dark, not where the first
/// one left off.
pub fn reset_fog(world: &mut World, entity: Entity) -> Result<(), String> {
    {
        let Ok(mut f) = world.get::<&mut FogOfWar>(entity) else {
            return Err("no fog_of_war on that entity".into());
        };
        f.state.visible.iter_mut().for_each(|v| *v = 0.0);
        f.state.explored.iter_mut().for_each(|v| *v = 0.0);
        // A key no picture hashes to and a build long ago: the next sync
        // remakes the picture at once, whatever `max_rate` says.
        f.state.key = 0;
        f.state.built_ms = f64::NEG_INFINITY;
    }
    write_custom(world, entity, "FogOfWar", serde_json::json!({ "explored": 0.0 }));
    Ok(())
}

/// `FogOfWar.set(e, field, value)`: write any `fog_of_war` field by name
/// (`size`, `cell`, `unseen_color`, `explored_alpha`, `reveal_speed`,
/// `max_rate`). What has been seen is kept while the grid keeps its shape; a
/// new `size` or `cell` makes a new grid, unseen.
pub fn set_fog_of_war_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let Ok(mut f) = world.get::<&mut FogOfWar>(entity) else {
        return Err("no fog_of_war on that entity".into());
    };
    let mut whole = serde_json::to_value(&*f).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a fog_of_war field"));
    }
    map.insert(field.to_string(), value.clone());
    let state = std::mem::take(&mut f.state);
    *f = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    f.state = state;
    // Remade on the next sync (a colour change moves no alpha, so the
    // picture's key would not notice it).
    f.state.key = 0;
    f.state.built_ms = f64::NEG_INFINITY;
    f.ensure();
    Ok(())
}

/// A linear colour channel as an sRGB byte (the texture is uploaded sRGB).
fn srgb8(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// One frame of every fog, then who it hides. Called from `tick_motion`.
pub fn tick_fog(world: &mut World, dt: f32) {
    let fogs: Vec<Entity> = world.query::<&FogOfWar>().iter().map(|(e, _)| e).collect();
    if fogs.is_empty() {
        ANY_HIDDEN.with(|h| h.set(false));
        return;
    }
    let sights: Vec<(Vec2, Sight)> = world
        .query::<(&Sight, &Transform, Option<&WorldTransform>)>()
        .iter()
        .map(|(_, (s, t, w))| (placed(t, w).0, s.clone()))
        .collect();
    let mut placed_fogs: Vec<(Entity, Vec2, f32)> = Vec::new();
    for e in fogs {
        let (origin, rot) = {
            let t = world.get::<&Transform>(e).map(|t| (*t).clone()).unwrap_or_default();
            let w = world.get::<&WorldTransform>(e).ok().map(|w| (*w).clone());
            placed(&t, w.as_ref())
        };
        let (sn, cs) = (-rot).sin_cos();
        let local: Vec<(Vec2, Sight)> = sights
            .iter()
            .map(|(p, s)| {
                let d = *p - origin;
                (Vec2::new(d.x * cs - d.y * sn, d.x * sn + d.y * cs), s.clone())
            })
            .collect();
        let explored = if let Ok(mut f) = world.get::<&mut FogOfWar>(e) {
            f.step(&local, dt);
            f.explored_share()
        } else {
            0.0
        };
        write_custom(world, e, "FogOfWar", serde_json::json!({ "explored": explored }));
        placed_fogs.push((e, origin, rot));
    }
    // Who is hidden: a `FogHidden` whose cell no fog sees enough.
    let hiders: Vec<(Entity, Vec2, f32)> = world
        .query::<(&FogHidden, &Transform, Option<&WorldTransform>)>()
        .iter()
        .map(|(e, (h, t, w))| (e, placed(t, w).0, h.threshold))
        .collect();
    let mut any = false;
    for (e, p, threshold) in hiders {
        let mut seen = 0.0f32;
        for (fe, origin, rot) in &placed_fogs {
            let Ok(f) = world.get::<&FogOfWar>(*fe) else { continue };
            let d = p - *origin;
            let (sn, cs) = (-rot).sin_cos();
            if let Some(v) = f.visible_at(Vec2::new(d.x * cs - d.y * sn, d.x * sn + d.y * cs)) {
                seen = seen.max(v);
            }
        }
        let visible = seen > threshold.clamp(0.0, 0.99);
        any |= !visible;
        if let Ok(mut h) = world.get::<&mut FogHidden>(e) {
            h.hidden = !visible;
        }
        write_custom(world, e, "Fog", serde_json::json!({ "visible": visible }));
    }
    ANY_HIDDEN.with(|h| h.set(any));
}

fn write_custom(world: &mut World, e: Entity, key: &str, v: serde_json::Value) {
    let missing = match world.get::<&mut CustomComponents>(e) {
        Ok(mut c) => {
            c.0.insert(key.into(), v);
            None
        }
        Err(_) => Some(v),
    };
    if let Some(v) = missing {
        let mut map = std::collections::HashMap::new();
        map.insert(key.into(), v);
        let _ = world.insert_one(e, CustomComponents(map));
    }
}

/// Make each fog's picture when it changed (at most `max_rate` a second) and
/// point its entity's sprite at it. Called from `carve::apply_carves`.
pub fn sync(world: &mut World, assets: &mut AssetManager) {
    let now = crate::profiling::now_ms();
    let mut built: Vec<(Entity, u64, String, RawTextureData, [f32; 2])> = Vec::new();
    for (e, f) in world.query::<&FogOfWar>().iter() {
        let (w, h) = f.state.dims;
        if w == 0 || h == 0 {
            continue;
        }
        let id = format!("{TEXTURE_PREFIX}{}", e.to_bits().get());
        let rgba = f.picture();
        let mut key: u64 = 0xcbf2_9ce4_8422_2325;
        for b in rgba.chunks(4) {
            key = (key ^ b[3] as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
        let have = assets.get_texture_bytes(&id).is_some();
        if have && key == f.state.key {
            continue;
        }
        if have && now - f.state.built_ms < 1000.0 / f.max_rate.max(0.1) as f64 {
            continue;
        }
        built.push((e, key, id, RawTextureData { width: w as u32, height: h as u32, rgba }, f.size));
    }
    for (e, key, id, tex, size) in built {
        let (w, h) = (tex.width, tex.height);
        match assets.texture_bytes_mut(&id) {
            Some(old) if old.width == w && old.height == h => {
                old.rgba = tex.rgba;
                assets.mark_texture_dirty(&id, 0, 0, w, h);
            }
            _ => assets.insert_texture_bytes(id.clone(), tex),
        }
        if let Ok(mut f) = world.get::<&mut FogOfWar>(e) {
            f.state.key = key;
            f.state.built_ms = now;
        }
        if let Ok(mut s) = world.get::<&mut SpriteComponent>(e)
            && s.texture_id.as_str() != id
        {
            s.texture_id = id.as_str().into();
            s.size = Some(size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sight_reveals_and_what_it_leaves_stays_explored() {
        let mut f = FogOfWar { size: [400.0, 400.0], cell: 20.0, reveal_speed: 1000.0, ..Default::default() };
        let scout = Sight { radius: 80.0, soft: 20.0 };
        f.step(&[(Vec2::new(-100.0, 0.0), scout.clone())], 1.0 / 60.0);
        assert!(f.visible_at(Vec2::new(-100.0, 0.0)).unwrap() > 0.99, "seen where the scout is");
        assert_eq!(f.visible_at(Vec2::new(100.0, 0.0)).unwrap(), 0.0, "not far off");
        f.step(&[(Vec2::new(100.0, 0.0), scout)], 1.0 / 60.0);
        assert_eq!(f.visible_at(Vec2::new(-100.0, 0.0)).unwrap(), 0.0, "out of sight once it leaves");
        let (w, _) = f.state.dims;
        let k = 10 * w + 5; // the cell by (-100, 0)
        assert!(f.state.explored[k] > 0.99, "but explored");
        assert!(f.explored_share() > 0.0 && f.explored_share() < 0.5);
    }

    #[test]
    fn the_picture_is_dark_unseen_dim_explored_and_clear_in_sight() {
        let mut f = FogOfWar { size: [60.0, 20.0], cell: 20.0, explored_alpha: 0.5, unseen_color: [0.0, 0.0, 0.0, 1.0], ..Default::default() };
        f.ensure();
        f.state.visible = vec![0.0, 0.0, 1.0];
        f.state.explored = vec![0.0, 1.0, 1.0];
        let p = f.picture();
        assert_eq!([p[3], p[7], p[11]], [255, 128, 0], "unseen, explored, in sight");
    }
}
