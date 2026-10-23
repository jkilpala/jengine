//! Fog you can see light in (Oct 2026): air thick enough to catch the light
//! that passes through it - a lamp's halo spreading past its own reach, a
//! window's light lying in the mist with the shadows of the bars cut
//! through it, and everything farther off going grey into it.
//!
//! A [`Fog`] on any entity (one a world - the first found) is applied in the
//! lighting **composite** (`light_composite.wgsl`), where each pixel already
//! has its light: the fog's density there - drifting noise, thicker toward
//! its `ground`, inside its box or everywhere - veils what is behind it and
//! adds the light **scattered** by the air, which is the light buffer
//! gathered over `scatter` pixels round the point, so a lamp glows in the
//! fog beyond its own edge and a shadow in the light is a shadow in the
//! fog. Being the light buffer, it is lit exactly where the scene is lit:
//! occluders, cookies, layers and all. `light_at` leaves it out, as it does
//! specular.
//!
//! Its clock is kept by `tick_fog` in `tick_motion`, and `gather_lights`
//! hands it to the GPU in `LightData::fog`, so no host was wired.

use glam::Vec2;
use hecs::World;
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Transform, WorldTransform};

/// `fog`: air that catches light.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Fog {
    /// What colour the air turns the light it scatters (linear).
    pub color: [f32; 3],
    /// How thick it is at its thickest, 0..1: the share of what is behind
    /// it the fog hides, and of the light it gathers it gives back.
    pub density: f32,
    /// How far the light spreads in it, in pixels: a lamp's halo past its
    /// own reach.
    pub scatter: f32,
    /// The size of its billows, in world units (0: even).
    pub noise_scale: f32,
    /// How much it billows, 0 (even) to 1 (gaps to double).
    pub noise_amount: f32,
    /// How it drifts, world units a second.
    pub wind: [f32; 2],
    /// The height it is thickest at and below (world y).
    pub ground: f32,
    /// How fast it thins above `ground`: it falls to a third every this
    /// many world units (0: as thick at any height).
    pub height: f32,
    /// A box about the entity it fills, fading at its edges; `[0, 0]` fills
    /// everywhere.
    pub size: [f32; 2],
    /// How long it has drifted - runtime state, never saved.
    #[serde(skip)]
    pub time: f32,
}

impl Default for Fog {
    fn default() -> Self {
        Self {
            color: [0.8, 0.85, 0.95],
            density: 0.5,
            scatter: 24.0,
            noise_scale: 300.0,
            noise_amount: 0.6,
            wind: [20.0, 0.0],
            ground: 0.0,
            height: 0.0,
            size: [0.0, 0.0],
            time: 0.0,
        }
    }
}

/// A fog where it is: what the composite needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedFog {
    pub fog_color: [f32; 4],
    /// noise scale, noise amount, ground, height.
    pub shape: [f32; 4],
    /// The drift so far (x, y), scatter, unused.
    pub motion: [f32; 4],
    /// The box's centre and half size (half size 0: everywhere).
    pub area: [f32; 4],
}

/// Drift every fog. Inside `tick_motion`.
pub fn tick_fog(world: &mut World, dt: f32) {
    for (_, f) in world.query_mut::<&mut Fog>() {
        f.time += dt.max(0.0);
    }
}

/// The world's fog, placed - the first `fog` found.
pub fn placed(world: &World) -> Option<PlacedFog> {
    let mut q = world.query::<(&Fog, &Transform, Option<&WorldTransform>)>();
    let (_, (f, t, w)) = q.iter().next()?;
    if f.density <= 0.0 {
        return None;
    }
    let (centre, scale) = match w {
        Some(w) => (w.position, w.scale),
        None => (t.position, t.scale),
    };
    let half = Vec2::from(f.size) * scale.abs() * 0.5;
    let drift = Vec2::from(f.wind) * f.time;
    Some(PlacedFog {
        fog_color: [f.color[0], f.color[1], f.color[2], f.density.clamp(0.0, 1.0)],
        shape: [f.noise_scale.max(0.0), f.noise_amount.clamp(0.0, 1.0), f.ground, f.height.max(0.0)],
        motion: [drift.x, drift.y, f.scatter.max(0.0), 0.0],
        area: [centre.x, centre.y, half.x.max(0.0), half.y.max(0.0)],
    })
}

/// How thick the fog is at a world point - the composite's `fog_density`,
/// line for line (its noise included), for tests and for anything that
/// wants to know how hidden a point is.
pub fn density_at(f: &PlacedFog, p: Vec2) -> f32 {
    let mut d = f.fog_color[3];
    if f.area[2] > 0.0 && f.area[3] > 0.0 {
        let q = (p - Vec2::new(f.area[0], f.area[1])).abs() - Vec2::new(f.area[2], f.area[3]);
        let soft = f.area[2].min(f.area[3]) * 0.25;
        let outside = q.x.max(q.y);
        d *= 1.0 - smoothstep(-soft, 0.0, outside);
    }
    if f.shape[3] > 0.0 {
        let above = (p.y - f.shape[2]).max(0.0);
        d *= (-above / f.shape[3]).exp();
    }
    if f.shape[0] > 0.0 {
        let q = (p - Vec2::new(f.motion[0], f.motion[1])) / f.shape[0];
        let n = 0.65 * value_noise(q) + 0.35 * value_noise(q * 2.03 + Vec2::splat(17.0));
        d *= 1.0 + (n * 2.0 - 1.0) * f.shape[1];
    }
    d.clamp(0.0, 1.0)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The composite's `fog_hash`: a lattice cell to 0..1, with integers.
fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393).wrapping_add((y as u32).wrapping_mul(668_265_263));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h ^ (h >> 16)) as f32 / 4_294_967_295.0
}

/// The composite's `fog_noise`: smooth value noise, 0..1.
fn value_noise(p: Vec2) -> f32 {
    let c = p.floor();
    let f = p - c;
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let (x, y) = (c.x as i32, c.y as i32);
    let a = hash(x, y);
    let b = hash(x + 1, y);
    let c2 = hash(x, y + 1);
    let d = hash(x + 1, y + 1);
    let top = a + (b - a) * u.x;
    let bottom = c2 + (d - c2) * u.x;
    top + (bottom - top) * u.y
}

/// `Fog.set(e, field, value)`: any field by name; the drift goes on.
pub fn set_fog_field(world: &World, entity: hecs::Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut f = world.get::<&mut Fog>(entity).map_err(|_| "no fog on that entity".to_string())?;
    let mut whole = serde_json::to_value(&*f).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a fog field"));
    }
    map.insert(field.to_string(), value.clone());
    let mut new: Fog = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    new.time = f.time;
    *f = new;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fog(f: Fog) -> PlacedFog {
        let mut w = World::new();
        w.spawn((f, Transform::default()));
        placed(&w).unwrap()
    }

    #[test]
    fn even_fog_is_its_density_everywhere() {
        let f = fog(Fog { density: 0.4, noise_scale: 0.0, ..Default::default() });
        assert_eq!(density_at(&f, Vec2::new(123.0, -456.0)), 0.4);
    }

    #[test]
    fn it_thins_with_height_and_stays_in_its_box() {
        let f = fog(Fog { density: 0.6, noise_scale: 0.0, ground: -100.0, height: 50.0, ..Default::default() });
        assert!((density_at(&f, Vec2::new(0.0, -200.0)) - 0.6).abs() < 1e-6, "below the ground: whole");
        let up = density_at(&f, Vec2::new(0.0, -50.0));
        assert!((up - 0.6 * (-1.0f32).exp()).abs() < 1e-5, "one height up: a third: {up}");
        let b = fog(Fog { density: 0.5, noise_scale: 0.0, size: [200.0, 100.0], ..Default::default() });
        assert_eq!(density_at(&b, Vec2::ZERO), 0.5, "the middle of its box");
        assert_eq!(density_at(&b, Vec2::new(150.0, 0.0)), 0.0, "outside it");
    }

    #[test]
    fn it_billows_and_drifts() {
        let mut w = World::new();
        let e = w.spawn((Fog { density: 0.5, noise_scale: 100.0, noise_amount: 1.0, ..Default::default() }, Transform::default()));
        let before = placed(&w).unwrap();
        let samples: Vec<f32> = (0..40).map(|k| density_at(&before, Vec2::new(k as f32 * 37.0, 11.0))).collect();
        let (lo, hi) = samples.iter().fold((1.0f32, 0.0f32), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(hi - lo > 0.3, "billows: {lo}..{hi}");
        tick_fog(&mut w, 2.0);
        let after = placed(&w).unwrap();
        assert_eq!(after.motion[0], 40.0, "drifted two seconds at 20 a second");
        set_fog_field(&w, e, "density", &serde_json::json!(0.2)).unwrap();
        assert_eq!(w.get::<&Fog>(e).unwrap().time, 2.0, "setting a field keeps the drift");
        assert!(set_fog_field(&w, e, "custard", &serde_json::json!(1)).is_err());
    }
}
