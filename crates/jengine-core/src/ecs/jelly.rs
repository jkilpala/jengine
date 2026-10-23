//! **Jelly** (Oct 2026): a sprite drawn as a lattice of points on springs -
//! a block that wobbles, squashes when it lands and quivers when poked.
//!
//! A [`Jelly`] on an entity with a sprite keeps a `cols` x `rows` grid of
//! points over the sprite's quad, each displaced from its place by an
//! offset with a velocity of its own: pulled back by `stiffness`, slowed by
//! `damping`, and pulled toward its neighbours' offsets by `coupling`, so a
//! push at one corner travels across as a ripple. Two things push it:
//!
//! - **The body's own motion.** A change in the entity's velocity in a
//!   frame - a landing, a bounce off a wall, a jump - is felt by every
//!   point, the more the further it lies from the side the push came from:
//!   the side that hit the floor stays put and the far side carries on,
//!   which is a squash, and the sides bulge outward with it. `response`
//!   scales it. The velocity is read from `velocity` (which the physics
//!   writes for a body) or, with none, from how far it moved.
//! - **`Jelly.poke(e, x, y, strength, radius)`**: the points near a world
//!   point pushed in, as a finger would.
//!
//! Offsets are held within `max_stretch` of the sprite's size, so nothing
//! inverts. The collider is the body's and does not deform. Ticked in
//! `tick_motion` (on the entity's own clock, `time_scale`); drawn by
//! `render_world_items` as one `Skinned` grid instead of the sprite's quad,
//! with the sprite's material, so no host was wired.

use glam::{Mat2, Vec2};
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{SpriteComponent, Transform, Velocity2D, WorldTransform};

/// A sprite that wobbles like jelly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Jelly {
    /// Cells across and down (points are one more each way); 2..=24.
    pub cols: u32,
    pub rows: u32,
    /// How hard each point is pulled back to its place, per second squared.
    pub stiffness: f32,
    /// How fast its wobble dies away, per second.
    pub damping: f32,
    /// How strongly neighbours pull each other's offsets together.
    pub coupling: f32,
    /// The share of a change in the body's velocity its points feel.
    pub response: f32,
    /// The furthest a point may move, as a share of the sprite's size.
    pub max_stretch: f32,
    /// Per point, its offset from its place (in the sprite's own frame,
    /// before scale) and its velocity; row by row, bottom first.
    #[serde(skip)]
    pub offsets: Vec<Vec2>,
    #[serde(skip)]
    pub speeds: Vec<Vec2>,
    /// The entity's velocity (world) and position last frame.
    #[serde(skip)]
    pub last: Option<(Vec2, Vec2)>,
}

impl Default for Jelly {
    fn default() -> Self {
        Self {
            cols: 6,
            rows: 6,
            stiffness: 90.0,
            damping: 4.0,
            coupling: 400.0,
            response: 0.5,
            max_stretch: 0.35,
            offsets: Vec::new(),
            speeds: Vec::new(),
            last: None,
        }
    }
}

impl Jelly {
    fn dims(&self) -> (usize, usize) {
        (self.cols.clamp(2, 24) as usize + 1, self.rows.clamp(2, 24) as usize + 1)
    }

    fn ensure(&mut self) {
        let (w, h) = self.dims();
        if self.offsets.len() != w * h {
            self.offsets = vec![Vec2::ZERO; w * h];
            self.speeds = vec![Vec2::ZERO; w * h];
        }
    }

    /// Point `(i, j)`'s place in the sprite's own frame (before scale),
    /// with its anchor at the origin.
    fn rest(&self, sprite: &SpriteComponent, size: Vec2, i: usize, j: usize) -> Vec2 {
        let (w, h) = self.dims();
        let (fx, fy) = (i as f32 / (w - 1) as f32, j as f32 / (h - 1) as f32);
        Vec2::new((fx - sprite.anchor[0]) * size.x, (fy - sprite.anchor[1]) * size.y)
    }
}

/// Step every jelly by `dt`. Called from `tick_motion`.
pub fn tick_jellies(world: &mut World, dt: f32) {
    for (_, (j, s, t, w, v, ts)) in world.query_mut::<(
        &mut Jelly,
        &SpriteComponent,
        &Transform,
        Option<&WorldTransform>,
        Option<&Velocity2D>,
        Option<&crate::ecs::time_scale::TimeScale>,
    )>() {
        let dt = dt.max(0.0) * crate::ecs::time_scale::factor(ts);
        j.ensure();
        let (pos, rot, scale) = match w {
            Some(w) => (w.position, w.rotation, w.scale),
            None => (t.position, t.rotation, t.scale),
        };
        let vel = match (v, j.last) {
            (Some(v), _) => Vec2::new(v.x, v.y),
            (None, Some((_, p))) if dt > 0.0 => (pos - p) / dt,
            _ => Vec2::ZERO,
        };
        let size = s.drawn_size(Vec2::from(SpriteComponent::FALLBACK_SIZE));
        let (gw, gh) = j.dims();
        // The change in the body's velocity, in its own frame (before
        // scale): felt as a push the other way, by the far side most.
        if let Some((last_v, _)) = j.last {
            let dv = vel - last_v;
            let local = Mat2::from_angle(-rot) * dv / Vec2::new(scale.x.abs().max(1e-3), scale.y.abs().max(1e-3));
            let len = local.length();
            if len > 1e-3 {
                let dir = local / len;
                let extent = (size * 0.5 * Vec2::new(dir.x.abs(), dir.y.abs())).length().max(1.0);
                let centre = Vec2::new((0.5 - s.anchor[0]) * size.x, (0.5 - s.anchor[1]) * size.y);
                for jy in 0..gh {
                    for ix in 0..gw {
                        let rel = j.rest(s, size, ix, jy) - centre;
                        // 0 on the side the push came from, 1 on the far side.
                        let far = ((rel.dot(dir) + extent) / (2.0 * extent)).clamp(0.0, 1.0);
                        let across = Vec2::new(-dir.y, dir.x);
                        // The far side carries on, and the middle bulges out.
                        let bulge = across * rel.dot(across) / extent * far * (1.0 - far) * 2.0;
                        let k = jy * gw + ix;
                        j.speeds[k] += (-local * far + bulge * len * 0.6) * j.response;
                    }
                }
            }
        }
        j.last = Some((vel, pos));
        if dt <= 0.0 {
            continue;
        }
        // Springs: home, neighbours, damping - in two half steps, for a
        // stiff jelly at a long frame.
        let steps = if dt > 1.0 / 90.0 { 2 } else { 1 };
        let h = dt / steps as f32;
        let limit = size * j.max_stretch.max(0.0);
        for _ in 0..steps {
            let off = j.offsets.clone();
            for jy in 0..gh {
                for ix in 0..gw {
                    let k = jy * gw + ix;
                    let mut lap = Vec2::ZERO;
                    let mut n = 0.0;
                    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                        let (x2, y2) = (ix as i32 + dx, jy as i32 + dy);
                        if x2 >= 0 && y2 >= 0 && (x2 as usize) < gw && (y2 as usize) < gh {
                            lap += off[y2 as usize * gw + x2 as usize];
                            n += 1.0;
                        }
                    }
                    let lap = if n > 0.0 { lap / n - off[k] } else { Vec2::ZERO };
                    let a = -off[k] * j.stiffness + lap * j.coupling - j.speeds[k] * j.damping;
                    j.speeds[k] += a * h;
                    let o = off[k] + j.speeds[k] * h;
                    j.offsets[k] = o.clamp(-limit, limit);
                }
            }
        }
    }
}

/// `Jelly.poke(e, x, y, strength, radius)`: push the points within
/// `radius` of the world point in, toward the middle, at up to `strength`
/// units a second.
pub fn poke(world: &mut World, e: Entity, at: Vec2, strength: f32, radius: f32) -> Result<(), String> {
    let mut q = world
        .query_one::<(&mut Jelly, &SpriteComponent, &Transform, Option<&WorldTransform>)>(e)
        .map_err(|_| "no jelly on that entity".to_string())?;
    let Some((j, s, t, w)) = q.get() else { return Err("no jelly on that entity".into()) };
    j.ensure();
    let (pos, rot, scale) = match w {
        Some(w) => (w.position, w.rotation, w.scale),
        None => (t.position, t.rotation, t.scale),
    };
    let sc = Vec2::new(scale.x.abs().max(1e-3), scale.y.abs().max(1e-3));
    let local = Mat2::from_angle(-rot) * (at - pos) / sc;
    let size = s.drawn_size(Vec2::from(SpriteComponent::FALLBACK_SIZE));
    let centre = Vec2::new((0.5 - s.anchor[0]) * size.x, (0.5 - s.anchor[1]) * size.y);
    let r = radius.max(1.0) / sc.x.max(sc.y);
    let (gw, gh) = j.dims();
    for jy in 0..gh {
        for ix in 0..gw {
            let p = j.rest(s, size, ix, jy);
            let d = p.distance(local);
            if d < r {
                let k = jy * gw + ix;
                let fall = 1.0 - d / r;
                j.speeds[k] += (centre - p).normalize_or_zero() * strength * fall * fall;
            }
        }
    }
    Ok(())
}

/// The jelly's grid as a `Skinned` item takes it: world positions,
/// `(u, v, q)`, colours and indices - the sprite's quad cut into cells, each
/// point moved by its offset.
#[allow(clippy::type_complexity)]
pub fn mesh(j: &Jelly, s: &SpriteComponent, wt: &WorldTransform, size: Vec2, uv: [f32; 4]) -> (Vec<Vec2>, Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let (gw, gh) = j.dims();
    let mut pos = Vec::with_capacity(gw * gh);
    let mut uvq = Vec::with_capacity(gw * gh);
    for jy in 0..gh {
        for ix in 0..gw {
            let k = jy * gw + ix;
            let off = j.offsets.get(k).copied().unwrap_or(Vec2::ZERO);
            pos.push(wt.apply(j.rest(s, size, ix, jy) + off));
            let (fx, fy) = (ix as f32 / (gw - 1) as f32, jy as f32 / (gh - 1) as f32);
            // v runs down the texture: the bottom row is v1.
            uvq.push([uv[0] + (uv[2] - uv[0]) * fx, uv[3] + (uv[1] - uv[3]) * fy, 1.0]);
        }
    }
    let mut idx = Vec::with_capacity((gw - 1) * (gh - 1) * 6);
    for jy in 0..gh - 1 {
        for ix in 0..gw - 1 {
            let a = (jy * gw + ix) as u32;
            let (b, c, d) = (a + 1, a + 1 + gw as u32, a + gw as u32);
            idx.extend_from_slice(&[a, b, c, a, c, d]);
        }
    }
    (pos, uvq, vec![s.color; gw * gh], idx)
}

/// `Jelly.set(e, field, value)`: any field by name, keeping its wobble.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut j = world.get::<&mut Jelly>(e).map_err(|_| "no jelly on that entity".to_string())?;
    let mut v = serde_json::to_value(&*j).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("jelly has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    let mut next: Jelly = serde_json::from_value(v).map_err(|e| format!("jelly.{field}: {e}"))?;
    next.offsets = std::mem::take(&mut j.offsets);
    next.speeds = std::mem::take(&mut j.speeds);
    next.last = j.last;
    *j = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(world: &mut World) -> Entity {
        world.spawn((
            Transform::default(),
            SpriteComponent { size: Some([100.0, 100.0]), ..Default::default() },
            Velocity2D { x: 0.0, y: -500.0, ..Default::default() },
            Jelly::default(),
        ))
    }

    fn point(world: &World, e: Entity, i: usize, j: usize) -> Vec2 {
        let jl = world.get::<&Jelly>(e).unwrap();
        jl.offsets[j * (jl.cols as usize + 1) + i]
    }

    #[test]
    fn at_rest_it_is_the_sprite() {
        let mut world = World::new();
        let e = block(&mut world);
        for _ in 0..30 {
            tick_jellies(&mut world, 1.0 / 60.0);
        }
        assert!(world.get::<&Jelly>(e).unwrap().offsets.iter().all(|o| o.length() < 1e-4), "a steady fall moves nothing");
        let j = (*world.get::<&Jelly>(e).unwrap()).clone();
        let s = SpriteComponent { size: Some([100.0, 100.0]), ..Default::default() };
        let (pos, uvq, _, idx) = mesh(&j, &s, &WorldTransform { scale: Vec2::ONE, ..Default::default() }, Vec2::splat(100.0), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!((pos.len(), idx.len()), (49, 6 * 36));
        assert!(pos[0].distance(Vec2::new(-50.0, -50.0)) < 1e-4 && pos[48].distance(Vec2::new(50.0, 50.0)) < 1e-4);
        assert_eq!((uvq[0], uvq[48]), ([0.0, 1.0, 1.0], [1.0, 0.0, 1.0]), "bottom left is the texture's bottom left");
    }

    #[test]
    fn landing_squashes_the_top_down_and_bulges_the_sides() {
        let mut world = World::new();
        let e = block(&mut world);
        tick_jellies(&mut world, 1.0 / 60.0);
        // It lands: the fall stops in a frame.
        world.get::<&mut Velocity2D>(e).unwrap().y = 0.0;
        for _ in 0..4 {
            tick_jellies(&mut world, 1.0 / 60.0);
        }
        let (top, bottom) = (point(&world, e, 3, 6), point(&world, e, 3, 0));
        assert!(top.y < -5.0, "the top carries on down: {top}");
        assert!(bottom.y.abs() < top.y.abs() * 0.3, "the bottom, which hit, barely moves: {bottom}");
        let (left, right) = (point(&world, e, 0, 3), point(&world, e, 6, 3));
        assert!(left.x < -1.0 && right.x > 1.0, "the sides bulge out: {left} / {right}");
        // And it settles back.
        for _ in 0..300 {
            tick_jellies(&mut world, 1.0 / 60.0);
        }
        assert!(world.get::<&Jelly>(e).unwrap().offsets.iter().all(|o| o.length() < 0.5));
    }

    #[test]
    fn a_poke_dents_near_it_and_the_quiver_spreads() {
        let mut world = World::new();
        let e = block(&mut world);
        tick_jellies(&mut world, 1.0 / 60.0);
        // Poke the right edge's middle.
        poke(&mut world, e, Vec2::new(50.0, 0.0), 400.0, 30.0).unwrap();
        tick_jellies(&mut world, 1.0 / 60.0);
        let near = point(&world, e, 6, 3);
        let far = point(&world, e, 0, 3);
        assert!(near.x < -2.0, "dented in: {near}");
        assert!(far.length() < 0.1, "the far side, not yet: {far}");
        for _ in 0..10 {
            tick_jellies(&mut world, 1.0 / 60.0);
        }
        assert!(point(&world, e, 4, 3).length() > 0.5, "the ripple reached the middle");
        let plain = world.spawn((Transform::default(),));
        assert!(poke(&mut world, plain, Vec2::ZERO, 1.0, 1.0).is_err());
    }
}
