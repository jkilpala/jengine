//! **Area effects** (Oct 2026): a box that does something to the physics
//! bodies inside it - water they float in, wind or a current that pushes
//! them, a pull towards its middle - as Unity's effectors and Godot's area
//! gravity do. Before it, each of those was a script applying forces to
//! every body every frame, at the script's rate rather than the physics'.
//!
//! An [`AreaEffect`] is a box `size` wide and tall about its entity (turned
//! with it). Every fixed physics step, each collider of each dynamic body
//! overlapping it is given, scaled by how much of its outline is inside
//! ([`push`], the outline clipped to the box) and by its mass, so heavy and
//! light things behave alike:
//!
//! - `buoyancy`: an upward acceleration of `buoyancy` times gravity at full
//!   depth - 1 is neutral, 2 floats half under. The box's top is the
//!   surface, so raising the entity raises the tide. It pushes at the middle
//!   of the part under water, so a hull that heels is turned back level.
//! - `force`: a steady acceleration (wind, a current, a conveyor).
//! - `attract`: an acceleration towards the middle (negative pushes away),
//!   strongest there and gone at the box's edge.
//! - `drag`, `angular_drag`: velocity lost per second inside - what makes
//!   water feel like water.
//!
//! Particles feel the `force` too (`tick_particles`), so smoke blows the way
//! the wind pushes the crates. The step is `PhysicsWorld::apply_area_effects`,
//! run inside every fixed step; the boxes are read from the world by
//! `sync_area_effects`, which `ensure_physics_bodies` calls, so no host is
//! wired for it.

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// The `area_effect` component. Every effect is off at 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AreaEffect {
    /// The box, width and height, about the entity.
    pub size: [f32; 2],
    /// Upward acceleration at full depth, in multiples of gravity.
    pub buoyancy: f32,
    /// A steady acceleration inside, world units/s².
    pub force: [f32; 2],
    /// Acceleration towards the middle (negative: away), units/s², falling
    /// to nothing at the box's edge.
    pub attract: f32,
    /// Linear velocity lost per second inside (0..).
    pub drag: f32,
    /// Spin lost per second inside.
    pub angular_drag: f32,
    /// Off, the box does nothing (a fan switched off).
    pub enabled: bool,
}

impl Default for AreaEffect {
    fn default() -> Self {
        Self {
            size: [400.0, 200.0],
            buoyancy: 0.0,
            force: [0.0, 0.0],
            attract: 0.0,
            drag: 0.0,
            angular_drag: 0.0,
            enabled: true,
        }
    }
}

/// `Area.set(e, field, value)`: any field of the entity's [`AreaEffect`] by
/// name - a fan switched off (`enabled`), the tide's strength, the wind.
pub fn set_area_field(world: &hecs::World, entity: hecs::Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut a = world.get::<&mut AreaEffect>(entity).map_err(|_| "the entity has no area_effect".to_string())?;
    let mut whole = serde_json::to_value(&*a).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not an area_effect field"));
    }
    map.insert(field.to_string(), value.clone());
    *a = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    Ok(())
}

/// Every area effect in `world`, placed by its entity's world transform
/// (turned and scaled with it).
pub fn placed_areas(world: &hecs::World) -> Vec<PlacedArea> {
    use super::components::{Transform, WorldTransform};
    #[allow(unused_mut)]
    let mut out = world
        .query::<(&AreaEffect, &Transform, Option<&WorldTransform>)>()
        .iter()
        .map(|(_, (a, t, w))| {
            let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
            PlacedArea {
                centre: wt.position,
                rotation: wt.rotation,
                half: Vec2::from(a.size) * wt.scale.abs() * 0.5,
                effect: a.clone(),
                surface: None,
            }
        })
        .collect::<Vec<_>>();
    crate::feature!("water", { out.extend(super::water::placed(world)) });
    out
}

/// The surface's height above rest along the water, sampled for a step -
/// what the area effect floats things on.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterProfile {
    /// The local x of the first sample (the left edge), and the step.
    pub x0: f32,
    pub dx: f32,
    pub heights: Vec<f32>,
}

impl WaterProfile {
    /// The height above rest and the slope at local `x` (clamped to the
    /// water's ends).
    pub fn at(&self, x: f32) -> (f32, f32) {
        let n = self.heights.len();
        if n == 0 {
            return (0.0, 0.0);
        }
        if n == 1 {
            return (self.heights[0], 0.0);
        }
        let f = ((x - self.x0) / self.dx.max(1e-3)).clamp(0.0, (n - 1) as f32);
        let i = (f.floor() as usize).min(n - 2);
        let t = f - i as f32;
        let (a, b) = (self.heights[i], self.heights[i + 1]);
        (a + (b - a) * t, (b - a) / self.dx.max(1e-3))
    }
}

/// The wind at world point `p`: the `force` of every enabled box holding it.
/// What particles feel (`particles::tick_particles`) - only the force: they
/// do not float or get pulled.
pub fn wind_at(areas: &[PlacedArea], p: Vec2) -> Vec2 {
    areas
        .iter()
        .filter(|a| a.effect.enabled && a.contains(p))
        .map(|a| Vec2::from(a.effect.force))
        .sum()
}

/// An area effect placed in the world for a step: its box's centre, turn and
/// half size, and what it does.
#[derive(Debug, Clone)]
pub struct PlacedArea {
    pub centre: Vec2,
    pub rotation: f32,
    pub half: Vec2,
    pub effect: AreaEffect,
    /// A water's surface (`ecs::water`), heights above the box's top in its
    /// frame: the top follows it rather than lying flat.
    pub surface: Option<std::sync::Arc<WaterProfile>>,
}

impl PlacedArea {
    /// A world point in the box's frame (centre at the origin, unturned).
    pub fn local(&self, p: Vec2) -> Vec2 {
        Vec2::from_angle(-self.rotation).rotate(p - self.centre)
    }

    /// A vector in the box's frame turned back into the world's.
    pub fn world_dir(&self, v: Vec2) -> Vec2 {
        Vec2::from_angle(self.rotation).rotate(v)
    }

    /// Whether a point is inside the box.
    pub fn contains(&self, p: Vec2) -> bool {
        let l = self.local(p);
        l.x.abs() <= self.half.x && l.y.abs() <= self.half.y
    }
}

/// `poly` (world points, either winding) clipped to `area`'s box, in the
/// box's frame: Sutherland-Hodgman against its four sides. The box's top is
/// the water's surface, so for water this is the part under it.
pub fn clip_to_box(area: &PlacedArea, poly: &[Vec2]) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = poly.iter().map(|p| area.local(*p)).collect();
    let h = area.half;
    // A water's top is its surface under the shape: the height and slope
    // at the shape's middle, so a hull rides a crest and heels on a slope.
    let surface = area.surface.as_ref().map(|s| {
        let xc = out.iter().map(|p| p.x).sum::<f32>() / out.len().max(1) as f32;
        let (height, slope) = s.at(xc);
        (xc, h.y + height, slope)
    });
    // Each side as (axis, sign): keep the points with sign * p[axis] <= h[axis].
    for (axis, sign) in [(0usize, 1.0f32), (0, -1.0), (1, 1.0), (1, -1.0)] {
        if out.is_empty() {
            break;
        }
        let limit = h[axis];
        let d = |p: Vec2| match surface {
            Some((xc, top, slope)) if axis == 1 && sign > 0.0 => top + slope * (p.x - xc) - p.y,
            _ => limit - sign * p[axis],
        };
        let src = std::mem::take(&mut out);
        for i in 0..src.len() {
            let (a, b) = (src[i], src[(i + 1) % src.len()]);
            let (da, db) = (d(a), d(b));
            if da >= 0.0 {
                out.push(a);
            }
            if (da >= 0.0) != (db >= 0.0) {
                out.push(a + (b - a) * (da / (da - db)));
            }
        }
    }
    out
}

/// A polygon's area (unsigned) and centroid. A degenerate one has area 0 and
/// the average of its points.
pub fn area_and_centroid(poly: &[Vec2]) -> (f32, Vec2) {
    if poly.is_empty() {
        return (0.0, Vec2::ZERO);
    }
    let mut a2 = 0.0f32;
    let mut c = Vec2::ZERO;
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let cross = p.perp_dot(q);
        a2 += cross;
        c += (p + q) * cross;
    }
    if a2.abs() < 1e-6 {
        let mean = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
        return (0.0, mean);
    }
    (a2.abs() * 0.5, c / (3.0 * a2))
}

/// What an area does to one shape of a body, as accelerations per unit of
/// the shape's mass. `lift` is buoyancy, pushed at `lift_at` - the middle of
/// the part under the surface, which is what turns a heeling boat back
/// level and leaves a square crate floating on a corner, as real ones do.
/// The rest pushes at the body's centre of mass.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Push {
    /// Wind and pull, at the centre of mass.
    pub accel: Vec2,
    /// Buoyancy, at `lift_at`.
    pub lift: Vec2,
    pub lift_at: Vec2,
    /// Velocity and spin lost per second.
    pub drag: f32,
    pub angular_drag: f32,
}

/// The push `area` gives a shape - its outline `shape` in world points, the
/// body's centre of mass `centre` - or `None` when the box is off or the
/// shape is not in it. Everything scales with the share of the shape inside,
/// so a body half in the wind feels half of it.
pub fn push(area: &PlacedArea, shape: &[Vec2], centre: Vec2, gravity: Vec2) -> Option<Push> {
    let e = &area.effect;
    if !e.enabled || shape.len() < 3 {
        return None;
    }
    let (whole, _) = area_and_centroid(shape);
    if whole <= 0.0 {
        return None;
    }
    let (wet_area, wet_centroid) = area_and_centroid(&clip_to_box(area, shape));
    let wet = (wet_area / whole).clamp(0.0, 1.0);
    if wet <= 0.0 {
        return None;
    }
    let mut accel = Vec2::from(e.force) * wet;
    if e.attract != 0.0 {
        let to = area.centre - centre;
        let reach = area.half.length().max(1e-3);
        let k = (1.0 - to.length() / reach).clamp(0.0, 1.0);
        accel += to.normalize_or_zero() * e.attract * k * wet;
    }
    Some(Push {
        accel,
        lift: -gravity * e.buoyancy * wet,
        lift_at: area.centre + area.world_dir(wet_centroid),
        drag: e.drag * wet,
        angular_drag: e.angular_drag * wet,
    })
}

/// A circle as a polygon of `n` sides - how a ball collider is clipped.
pub fn circle_outline(centre: Vec2, radius: f32, n: usize) -> Vec<Vec2> {
    (0..n)
        .map(|i| centre + Vec2::from_angle(i as f32 / n as f32 * std::f32::consts::TAU) * radius)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn water() -> PlacedArea {
        PlacedArea {
            centre: Vec2::ZERO,
            rotation: 0.0,
            half: Vec2::new(200.0, 100.0),
            effect: AreaEffect { buoyancy: 2.0, drag: 1.0, ..Default::default() },
            surface: None,
        }
    }

    fn square(c: Vec2, half: f32) -> Vec<Vec2> {
        vec![c + Vec2::new(-half, -half), c + Vec2::new(half, -half), c + Vec2::new(half, half), c + Vec2::new(-half, half)]
    }

    #[test]
    fn a_body_half_under_the_surface_is_half_submerged() {
        let w = water();
        let g = Vec2::new(0.0, -981.0);
        // A 20 x 20 box straddling the surface at y 100.
        let p = push(&w, &square(Vec2::new(0.0, 100.0), 10.0), Vec2::new(0.0, 100.0), g).unwrap();
        assert!((p.lift + g).length() < 1e-2, "buoyancy 2, half under, cancels gravity: {p:?}");
        assert!((p.lift_at - Vec2::new(0.0, 95.0)).length() < 1e-3, "pushed at the wet half's middle");
        assert!((p.drag - 0.5).abs() < 1e-4, "drag on the wet half");
        // Deep down, all of it; above the water, nothing.
        let deep = push(&w, &square(Vec2::new(0.0, -40.0), 10.0), Vec2::new(0.0, -40.0), g).unwrap();
        assert!((deep.lift.y - 2.0 * 981.0).abs() < 1e-1);
        assert!(push(&w, &square(Vec2::new(0.0, 160.0), 10.0), Vec2::new(0.0, 160.0), g).is_none());
    }

    #[test]
    fn a_heeling_hull_is_pushed_back_upright() {
        // A wide flat hull turned 15 degrees, half under: the wet part's
        // middle moves to the low side, so lift there turns it back.
        let w = water();
        let c = Vec2::new(0.0, 100.0);
        let turn = Vec2::from_angle(0.26);
        let hull: Vec<Vec2> = [(-40.0, -7.0), (40.0, -7.0), (40.0, 7.0), (-40.0, 7.0)]
            .iter()
            .map(|(x, y)| c + turn.rotate(Vec2::new(*x, *y)))
            .collect();
        let p = push(&w, &hull, c, Vec2::new(0.0, -981.0)).unwrap();
        // Turned anticlockwise, its right end is up: lift on the left side
        // is a clockwise (negative) torque, back towards level.
        let torque = (p.lift_at - c).perp_dot(p.lift);
        assert!(p.lift_at.x < c.x && torque < 0.0, "rights itself: {p:?}");
    }

    #[test]
    fn clipping_keeps_what_is_inside_the_box() {
        let w = water();
        // Half of this square sticks out of the right side (x 200).
        let (a, c) = area_and_centroid(&clip_to_box(&w, &square(Vec2::new(200.0, 0.0), 10.0)));
        assert!((a - 200.0).abs() < 1e-3 && (c - Vec2::new(195.0, 0.0)).length() < 1e-3, "{a} {c:?}");
        assert!(clip_to_box(&w, &square(Vec2::new(500.0, 0.0), 10.0)).is_empty());
        // Either winding.
        let mut cw = square(Vec2::ZERO, 10.0);
        cw.reverse();
        assert!((area_and_centroid(&cw).0 - 400.0).abs() < 1e-3);
    }

    #[test]
    fn wind_pushes_in_proportion_and_attraction_fades_to_the_edge() {
        let wind = PlacedArea {
            centre: Vec2::ZERO,
            rotation: 0.0,
            half: Vec2::new(100.0, 100.0),
            effect: AreaEffect { force: [300.0, 0.0], ..Default::default() },
            surface: None,
        };
        let p = push(&wind, &square(Vec2::new(100.0, 0.0), 10.0), Vec2::new(100.0, 0.0), Vec2::ZERO).unwrap();
        assert!((p.accel.x - 150.0).abs() < 1e-2, "half in, half the push: {p:?}");
        let pull = PlacedArea { effect: AreaEffect { attract: 500.0, ..Default::default() }, ..wind.clone() };
        let near = push(&pull, &square(Vec2::new(15.0, 0.0), 5.0), Vec2::new(15.0, 0.0), Vec2::ZERO).unwrap().accel;
        let far = push(&pull, &square(Vec2::new(85.0, 0.0), 5.0), Vec2::new(85.0, 0.0), Vec2::ZERO).unwrap().accel;
        assert!(near.x < 0.0 && far.x < 0.0 && near.x.abs() > far.x.abs(), "towards the middle, weaker further out");
        let off = PlacedArea { effect: AreaEffect { enabled: false, ..wind.effect.clone() }, ..wind };
        assert!(push(&off, &square(Vec2::ZERO, 5.0), Vec2::ZERO, Vec2::ZERO).is_none());
    }

    #[test]
    fn a_turned_box_turns_what_is_inside() {
        // `force` is in world space; turning the box only turns what is inside.
        let mut w = water();
        w.rotation = std::f32::consts::FRAC_PI_2;
        // The box is now 200 wide (along y) and 400 tall (along x): a point
        // at (150, 0) is inside, (0, 150) is not.
        assert!(w.contains(Vec2::new(90.0, 0.0)) && w.contains(Vec2::new(0.0, 150.0)));
        assert!(!w.contains(Vec2::new(150.0, 0.0)));
    }
}
