//! Water that answers (Oct 2026): a surface of springs that what falls in
//! pushes, and that what floats rides.
//!
//! A [`WaterSurface`] is a body of water seen side on: `width` wide, its
//! surface at the entity's position at rest and `depth` below. The surface
//! is a row of columns a `spacing` apart, each a spring pulled back to rest
//! (`tension`), slowed (`damping`) and pulling on its neighbours (`spread`) -
//! so a push anywhere runs out along it as a pair of waves and dies away.
//! Every dynamic body crossing it pushes the columns under it by its speed
//! (`splash`), a script pokes it with `Water.splash`, and `chop` pokes it at
//! random; under all that a `swell` rolls across, a sine of its own.
//!
//! What is under the surface **floats**: each water is also an area effect
//! (`ecs::area_effect`), whose box's flat top is replaced by the surface
//! itself - the height and slope under the floating body - so a boat rises
//! on a crest, drops into a trough and heels with the slope, with the
//! buoyancy, drag and lift-at-the-wet-centroid the area effect already has.
//! It is drawn as one mesh - a strip from the surface down to the bottom,
//! `color_top` fading to `color_deep`, and a thin `foam` line along the top.
//!
//! The springs step at a fixed 60 Hz, so the water moves the same at any
//! frame rate, and every number is per one of those steps.

use crate::detmath::DetMath;
use std::sync::Arc;

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Collider2D, ColliderShape, Transform, Velocity2D, WorldTransform};

const STEP: f32 = 1.0 / 60.0;

/// `water_surface`: a body of water, side on, whose surface moves.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterSurface {
    /// How wide the water is, centred on the entity.
    pub width: f32,
    /// How deep, below the surface at rest (the entity's position).
    pub depth: f32,
    /// Columns this far apart along the surface.
    pub spacing: f32,
    /// How hard a column is pulled back to rest, a step.
    pub tension: f32,
    /// How much of a column's speed is lost a step.
    pub damping: f32,
    /// How much a column pulls its neighbours, a step (0..0.5).
    pub spread: f32,
    /// How hard a body crossing the surface pushes it, per unit of its
    /// speed through it past `splash_speed`.
    pub splash: f32,
    /// Speeds through the surface under this make no splash - so what
    /// floats and bobs does not shake the water it rides (it would feed
    /// its own waves and be thrown out).
    pub splash_speed: f32,
    /// Random pokes a second along the surface, and how hard each is.
    pub chop: f32,
    pub chop_strength: f32,
    /// A swell rolling across: its height, its length and its speed (world
    /// units a second; negative rolls left).
    pub swell_height: f32,
    pub swell_length: f32,
    pub swell_speed: f32,
    /// Floating: upward acceleration at full depth in multiples of gravity,
    /// and the speed and spin lost a second under it (`area_effect`'s).
    pub buoyancy: f32,
    pub drag: f32,
    pub angular_drag: f32,
    /// The water's colour at the surface and at the bottom (linear).
    pub color_top: [f32; 4],
    pub color_deep: [f32; 4],
    /// The line of foam along the top, and how thick.
    pub foam_color: [f32; 4],
    pub foam_width: f32,
    pub z_order: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    pub blend_mode: crate::renderer::material::BlendMode,
    /// The columns' heights above rest and their speeds - never saved.
    #[serde(skip)]
    pub heights: Vec<f32>,
    #[serde(skip)]
    pub speeds: Vec<f32>,
    /// Time not yet stepped, the swell's clock and the chop's dice.
    #[serde(skip)]
    pub pending: f32,
    #[serde(skip)]
    pub clock: f32,
    #[serde(skip)]
    pub dice: u32,
    /// Where each body near the surface was last step, by entity - how a
    /// body's speed through the surface is known when it has no `velocity`
    /// (physics writes one only where there is one).
    #[serde(skip)]
    pub seen: std::collections::HashMap<u64, Vec2>,
}

impl Default for WaterSurface {
    fn default() -> Self {
        Self {
            width: 800.0,
            depth: 300.0,
            spacing: 8.0,
            tension: 0.02,
            damping: 0.025,
            spread: 0.24,
            splash: 0.012,
            splash_speed: 150.0,
            chop: 0.0,
            chop_strength: 2.0,
            swell_height: 0.0,
            swell_length: 400.0,
            swell_speed: 60.0,
            buoyancy: 0.0,
            // Water damps what floats in it hard: a thin hull on light drag is a
            // stiff spring hardly slowed, and rings until it leaps out.
            drag: 2.5,
            angular_drag: 3.0,
            color_top: [0.12, 0.3, 0.42, 0.85],
            color_deep: [0.01, 0.04, 0.08, 0.95],
            foam_color: [0.8, 0.9, 1.0, 0.8],
            foam_width: 3.0,
            z_order: 0,
            sort_layer: None,
            blend_mode: crate::renderer::material::BlendMode::Alpha,
            heights: Vec::new(),
            speeds: Vec::new(),
            pending: 0.0,
            clock: 0.0,
            dice: 0x9e37_79b9,
            seen: std::collections::HashMap::new(),
        }
    }
}

pub use super::area_effect::WaterProfile;

impl WaterSurface {
    fn columns(&self) -> usize {
        ((self.width / self.spacing.max(1.0)).ceil() as usize + 1).max(2)
    }

    /// The local x of column `i` (0 at the entity).
    pub fn column_x(&self, i: usize) -> f32 {
        let n = self.columns();
        -self.width * 0.5 + self.width * i as f32 / (n - 1) as f32
    }

    fn ensure(&mut self) {
        let n = self.columns();
        if self.heights.len() != n {
            self.heights = vec![0.0; n];
            self.speeds = vec![0.0; n];
        }
    }

    /// The swell's height at local `x` now.
    pub fn swell_at(&self, x: f32) -> f32 {
        if self.swell_height == 0.0 {
            return 0.0;
        }
        let k = std::f32::consts::TAU / self.swell_length.max(1.0);
        self.swell_height * (k * (x - self.swell_speed * self.clock)).dsin()
    }

    /// The surface's height above rest at local `x` - springs and swell.
    pub fn height_at(&self, x: f32) -> f32 {
        self.profile().at(x).0
    }

    /// The surface as samples, springs and swell together.
    pub fn profile(&self) -> WaterProfile {
        let n = self.columns();
        let heights = (0..n)
            .map(|i| self.heights.get(i).copied().unwrap_or(0.0) + self.swell_at(self.column_x(i)))
            .collect();
        WaterProfile { x0: -self.width * 0.5, dx: self.width / (n - 1) as f32, heights }
    }

    /// Push the surface at local `x`: `strength` is the speed given to the
    /// column there (down is negative), its neighbours a little less.
    pub fn splash(&mut self, x: f32, strength: f32, radius: f32) {
        self.ensure();
        let r = radius.max(self.spacing);
        for i in 0..self.heights.len() {
            let d = (self.column_x(i) - x).abs();
            if d < r {
                let k = 0.5 + 0.5 * (std::f32::consts::PI * d / r).dcos();
                self.speeds[i] += strength * k;
            }
        }
    }

    /// One fixed step of the springs.
    fn step(&mut self) {
        let n = self.heights.len();
        for i in 0..n {
            self.speeds[i] += -self.tension * self.heights[i] - self.damping * self.speeds[i];
            self.heights[i] += self.speeds[i];
        }
        // The pull between neighbours, a few passes so a wave runs.
        let spread = self.spread.clamp(0.0, 0.5);
        let mut left = vec![0.0f32; n];
        let mut right = vec![0.0f32; n];
        for _ in 0..4 {
            for i in 0..n {
                if i > 0 {
                    left[i] = spread * (self.heights[i] - self.heights[i - 1]);
                    self.speeds[i - 1] += left[i];
                }
                if i + 1 < n {
                    right[i] = spread * (self.heights[i] - self.heights[i + 1]);
                    self.speeds[i + 1] += right[i];
                }
            }
            for i in 0..n {
                if i > 0 {
                    self.heights[i - 1] += left[i];
                }
                if i + 1 < n {
                    self.heights[i + 1] += right[i];
                }
            }
        }
        for h in &mut self.heights {
            *h = h.clamp(-self.depth, self.depth);
        }
    }

    fn roll(&mut self) -> f32 {
        // xorshift32: the chop is the same every run.
        let mut x = self.dice.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.dice = x;
        x as f32 / u32::MAX as f32
    }
}

fn position_of(world: &World, e: Entity) -> Option<Vec2> {
    if let Ok(w) = world.get::<&WorldTransform>(e) {
        return Some(w.position);
    }
    world.get::<&Transform>(e).ok().map(|t| t.position)
}

/// A collider's half size, roughly - how wide a span of surface a body
/// crossing it pushes.
fn half_extent(shape: &ColliderShape) -> Vec2 {
    match shape {
        ColliderShape::Box { width, height } => Vec2::new(*width, *height) * 0.5,
        ColliderShape::Circle { radius } => Vec2::splat(*radius),
        ColliderShape::Capsule { half_height, radius } => Vec2::new(*radius, half_height + radius),
        ColliderShape::ConvexPolygon { vertices }
        | ColliderShape::Polyline { vertices }
        | ColliderShape::ConvexDecomposition { vertices, .. } => {
            vertices.iter().fold(Vec2::ZERO, |m, v| m.max(Vec2::new(v[0].abs(), v[1].abs())))
        }
        _ => Vec2::splat(16.0),
    }
}

/// Step every water's surface by `dt`: the bodies crossing it push it, the
/// chop pokes it, the springs run at 60 Hz and the swell rolls on. Inside
/// `tick_motion`, so every host runs it.
pub fn tick_water(world: &mut World, dt: f32) {
    let _span = crate::profiling::trace::span("water");
    let waters: Vec<(Entity, Vec2)> = world
        .query::<&WaterSurface>()
        .iter()
        .filter_map(|(e, _)| Some((e, position_of(world, e)?)))
        .map(|(e, p)| (e, p))
        .collect();
    if waters.is_empty() {
        return;
    }
    // What crosses a surface: anything solid with a collider. Its speed is
    // its `velocity` where it has one, else how far it moved since the
    // last frame.
    let bodies: Vec<(u64, Vec2, Option<Vec2>, Vec2)> = world
        .query::<(&Collider2D, Option<&Velocity2D>)>()
        .iter()
        .filter(|(_, (c, _))| !c.is_sensor)
        .filter_map(|(e, (c, v))| {
            let speed = v.map(|v| Vec2::new(v.x, v.y)).filter(|v| *v != Vec2::ZERO);
            Some((e.to_bits().get(), position_of(world, e)?, speed, half_extent(&c.shape)))
        })
        .collect();
    for (e, at) in waters {
        let Ok(mut w) = world.get::<&mut WaterSurface>(e) else { continue };
        w.ensure();
        let steps = {
            w.pending += dt.clamp(0.0, 0.25);
            // A hair over, so three steps' worth of time is three steps.
            let n = (w.pending / STEP + 1e-3).floor();
            w.pending = (w.pending - n * STEP).max(0.0);
            n as usize
        };
        let mut seen = std::collections::HashMap::new();
        for (id, p, speed, half) in &bodies {
            let local = *p - at;
            let last = w.seen.get(id).copied();
            seen.insert(*id, *p);
            let v = match (speed, last) {
                (Some(v), _) => *v,
                (None, Some(last)) if dt > 0.0 => (*p - last) / dt,
                _ => continue,
            };
            if local.x + half.x < -w.width * 0.5 || local.x - half.x > w.width * 0.5 {
                continue;
            }
            let surface = w.height_at(local.x);
            if (local.y - surface).abs() > half.y.max(4.0) {
                continue;
            }
            // Pushed by the body's speed through the surface, for the time
            // it spends in it - so one landing makes one splash.
            let through = v.y.signum() * (v.y.abs() - w.splash_speed).max(0.0);
            if through == 0.0 {
                continue;
            }
            let push = through * w.splash * (dt * 60.0).min(4.0);
            let r = half.x.max(w.spacing);
            let strength = push.clamp(-30.0, 30.0);
            let spread_out = (r / w.spacing.max(1.0)).sqrt().max(1.0);
            w.splash(local.x, strength / spread_out, r);
        }
        w.seen = seen;
        for _ in 0..steps {
            if w.chop > 0.0 && w.roll() < w.chop * STEP {
                let x = -w.width * 0.5 + w.roll() * w.width;
                let s = (w.roll() - 0.5) * 2.0 * w.chop_strength;
                let r = w.spacing * 3.0;
                w.splash(x, s, r);
            }
            w.step();
            w.clock += STEP;
        }
    }
}

/// Every water that floats things, as an area effect whose top is its
/// surface - read with the other area effects (`area_effect::placed_areas`).
pub fn placed(world: &World) -> Vec<crate::ecs::area_effect::PlacedArea> {
    world
        .query::<&WaterSurface>()
        .iter()
        .filter(|(_, w)| w.buoyancy > 0.0 || w.drag > 0.0)
        .filter_map(|(e, w)| {
            let at = position_of(world, e)?;
            Some(crate::ecs::area_effect::PlacedArea {
                centre: at - Vec2::new(0.0, w.depth * 0.5),
                rotation: 0.0,
                half: Vec2::new(w.width * 0.5, w.depth * 0.5),
                effect: crate::ecs::area_effect::AreaEffect {
                    size: [w.width, w.depth],
                    buoyancy: w.buoyancy,
                    drag: w.drag,
                    angular_drag: w.angular_drag,
                    ..Default::default()
                },
                surface: Some(Arc::new(w.profile())),
            })
        })
        .collect()
}

/// The water's mesh, in world space: the body from the surface to the
/// bottom, top colour fading to deep, then the foam along the top.
pub fn mesh(w: &WaterSurface, at: Vec2) -> (Vec<Vec2>, Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let profile = w.profile();
    let n = profile.heights.len();
    let (mut positions, mut uvq, mut colors, mut indices) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let bottom = at.y - w.depth;
    for (i, h) in profile.heights.iter().enumerate() {
        let x = at.x + profile.x0 + profile.dx * i as f32;
        positions.push(Vec2::new(x, at.y + h));
        positions.push(Vec2::new(x, bottom));
        uvq.push([0.5, 0.5, 1.0]);
        uvq.push([0.5, 0.5, 1.0]);
        colors.push(w.color_top);
        colors.push(w.color_deep);
    }
    for i in 0..n.saturating_sub(1) as u32 {
        let (a, b, c, d) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        indices.extend_from_slice(&[a, b, d, a, d, c]);
    }
    if w.foam_width > 0.0 && w.foam_color[3] > 0.0 {
        let base = positions.len() as u32;
        let clear = [w.foam_color[0], w.foam_color[1], w.foam_color[2], 0.0];
        for (i, h) in profile.heights.iter().enumerate() {
            let x = at.x + profile.x0 + profile.dx * i as f32;
            // Foam thickens where the surface moves fastest.
            let lively = w.speeds.get(i).map_or(0.0, |s| s.abs()).min(4.0);
            let top = at.y + h + w.foam_width * 0.5 * (1.0 + lively * 0.4);
            positions.push(Vec2::new(x, top));
            positions.push(Vec2::new(x, at.y + h - w.foam_width * 0.5));
            uvq.push([0.5, 0.5, 1.0]);
            uvq.push([0.5, 0.5, 1.0]);
            colors.push(w.foam_color);
            colors.push(clear);
        }
        for i in 0..n.saturating_sub(1) as u32 {
            let (a, b, c, d) = (base + 2 * i, base + 2 * i + 1, base + 2 * i + 2, base + 2 * i + 3);
            indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }
    (positions, uvq, colors, indices)
}

/// `Water.set(e, field, value)`: write any field by name - `chop` for a
/// storm, `swell_height` for a sea getting up.
pub fn set_water_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let Ok(mut w) = world.get::<&mut WaterSurface>(entity) else {
        return Err("no water_surface on that entity".into());
    };
    let mut whole = serde_json::to_value(&*w).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) && field != "sort_layer" {
        return Err(format!("'{field}' is not a water_surface field"));
    }
    let value = match (field, value) {
        ("z_order", serde_json::Value::Number(n)) => serde_json::Value::from(n.as_f64().unwrap_or(0.0) as i64),
        _ => value.clone(),
    };
    map.insert(field.to_string(), value);
    let (heights, speeds, pending, clock, dice) =
        (std::mem::take(&mut w.heights), std::mem::take(&mut w.speeds), w.pending, w.clock, w.dice);
    *w = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    // The surface keeps moving.
    (w.heights, w.speeds, w.pending, w.clock, w.dice) = (heights, speeds, pending, clock, dice);
    Ok(())
}

/// `Water.splash(e, x, strength)`: push the surface at world `x`.
pub fn splash_world(world: &World, entity: Entity, x: f32, strength: f32) -> Result<(), String> {
    let at = position_of(world, entity).ok_or("no transform")?;
    let Ok(mut w) = world.get::<&mut WaterSurface>(entity) else {
        return Err("no water_surface on that entity".into());
    };
    let r = w.spacing * 3.0;
    w.splash(x - at.x, strength, r);
    Ok(())
}
