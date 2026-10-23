//! What an entity can see (Oct 2026; the visibility polygon of every
//! stealth game, Red Blob Games' 2D visibility): the area round a point that
//! no wall hides, cut to a cone, drawn as a fan and asked who is in it.
//!
//! A [`Vision`] looks from its entity, `radius` far, across `fov` radians
//! about its facing (the entity's turn plus `direction`; a full turn or more
//! is all round). Each frame it casts a ray at every corner of every
//! non-sensor collider near it (and just either side, to slip past it) and at
//! even steps across its cone, takes where each ray first meets a wall, and
//! joins them in order: the polygon of what it sees. With `draw` it writes
//! that polygon into the entity's own `mesh` as a fan - `color` at the eye,
//! fading to `edge_color` at the reach - so a guard's view is drawn and
//! sorts with the sprites like any mesh. Whatever carries the `targets` tag
//! and stands inside it is **seen**: the entity's script is told
//! `on_seen(entity)` when the first is seen and `on_lost(entity)` when the
//! last is lost (`ecs::callbacks`), and its custom `Vision` component holds
//! `seen` (how many) for a script to read with `Entity.get_custom`.
//!
//! The walls are the colliders as `ecs::outlines` reads them - the same the
//! particles and foot IK meet - its own collider and the targets' left out.
//! Ticked in `tick_motion`, so no host was wired.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{CustomComponents, Mesh2D, MeshVertex, Tags, Transform, WorldTransform};

/// `vision`: what an entity can see.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Vision {
    /// How far it sees, in world units.
    pub radius: f32,
    /// How wide, in radians: a full turn (6.2832) or more is all round.
    pub fov: f32,
    /// Where it looks, added to the entity's turn (radians; 0 is +x).
    pub direction: f32,
    /// Write what it sees into the entity's `mesh` as a fan.
    pub draw: bool,
    /// The fan's colour at the eye and at its reach (linear).
    pub color: [f32; 4],
    pub edge_color: [f32; 4],
    pub z_order: i32,
    /// The tag of what it looks for (empty: nothing).
    pub targets: String,
    /// Rays across the cone besides the ones aimed at corners.
    pub rays: u32,
    /// Runtime state - never saved.
    #[serde(skip)]
    pub state: VisionState,
}

impl Default for Vision {
    fn default() -> Self {
        Self {
            radius: 400.0,
            fov: 1.2,
            direction: 0.0,
            draw: true,
            color: [1.0, 0.9, 0.5, 0.35],
            edge_color: [1.0, 0.9, 0.5, 0.0],
            z_order: 5,
            targets: String::new(),
            rays: 48,
            state: VisionState::default(),
        }
    }
}

/// What it saw last frame.
#[derive(Debug, Clone, Default)]
pub struct VisionState {
    /// The polygon's rim in world space, in order of angle (the eye is its
    /// centre).
    pub rim: Vec<Vec2>,
    pub eye: Vec2,
    pub seen: Vec<Entity>,
}

impl VisionState {
    /// Whether `p` is inside what it saw.
    pub fn sees(&self, p: Vec2, full_turn: bool) -> bool {
        let n = self.rim.len();
        if n < 2 {
            return false;
        }
        let tris = if full_turn { n } else { n - 1 };
        (0..tris).any(|i| in_triangle(p, self.eye, self.rim[i], self.rim[(i + 1) % n]))
    }
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

fn in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let (d1, d2, d3) = (cross(b - a, p - a), cross(c - b, p - b), cross(a - c, p - c));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Where a ray from `o` along `dir` first meets one of `walls`, within
/// `reach`.
fn cast(o: Vec2, dir: Vec2, reach: f32, walls: &[(Vec2, Vec2)]) -> f32 {
    let mut best = reach;
    for &(a, b) in walls {
        let r = b - a;
        let den = cross(dir, r);
        if den.abs() < 1e-9 {
            continue;
        }
        let t = cross(a - o, r) / den;
        let u = cross(a - o, dir) / den;
        if t > 1e-4 && t < best && (0.0..=1.0).contains(&u) {
            best = t;
        }
    }
    best
}

/// The angle `a` brought within half a turn of `about`.
fn wrap_near(a: f32, about: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut d = (a - about) % tau;
    if d > std::f32::consts::PI {
        d -= tau;
    } else if d < -std::f32::consts::PI {
        d += tau;
    }
    about + d
}

/// The rim of what is seen from `eye`: rays at every wall corner in reach
/// (and either side of it) and evenly across the cone, sorted by angle.
pub fn visibility(eye: Vec2, facing: f32, fov: f32, radius: f32, rays: u32, walls: &[(Vec2, Vec2)]) -> Vec<Vec2> {
    let full = fov >= std::f32::consts::TAU - 1e-3;
    let (lo, hi) = if full { (facing - std::f32::consts::PI, facing + std::f32::consts::PI) } else { (facing - fov / 2.0, facing + fov / 2.0) };
    let mut angles: Vec<f32> = Vec::new();
    let steps = rays.max(4);
    for k in 0..=steps {
        angles.push(lo + (hi - lo) * k as f32 / steps as f32);
    }
    for &(a, b) in walls {
        for p in [a, b] {
            if (p - eye).length_squared() > radius * radius * 1.0001 {
                continue;
            }
            let base = wrap_near((p.y - eye.y).atan2(p.x - eye.x), facing);
            for off in [-1e-4, 0.0, 1e-4] {
                let ang = base + off;
                if full || (ang > lo && ang < hi) {
                    angles.push(ang);
                }
            }
        }
    }
    angles.sort_by(|a, b| a.total_cmp(b));
    angles.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    if full {
        // The seam is closed by the fan itself.
        angles.pop();
    }
    angles
        .into_iter()
        .map(|a| {
            let dir = Vec2::new(a.cos(), a.sin());
            eye + dir * cast(eye, dir, radius, walls)
        })
        .collect()
}

/// Look, draw and tell. Inside `tick_motion`.
pub fn tick_vision(world: &mut World) {
    let lookers: Vec<Entity> = world.query::<&Vision>().iter().map(|(e, _)| e).collect();
    if lookers.is_empty() {
        return;
    }
    let outlines = crate::ecs::outlines::ColliderOutlines::gather(world);
    for e in lookers {
        let (eye, turn) = match (world.get::<&WorldTransform>(e), world.get::<&Transform>(e)) {
            (Ok(w), _) => (w.position, w.rotation),
            (_, Ok(t)) => (t.position, t.rotation),
            _ => continue,
        };
        let Some(v) = world.get::<&Vision>(e).ok().map(|v| (*v).clone()) else { continue };
        // The targets, where they are.
        let targets: Vec<(Entity, Vec2)> = if v.targets.is_empty() {
            Vec::new()
        } else {
            world
                .query::<(&Tags, &Transform, Option<&WorldTransform>)>()
                .iter()
                .filter(|(_, (tags, _, _))| tags.0.contains(&v.targets))
                .map(|(t, (_, tr, w))| (t, w.map_or(tr.position, |w| w.position)))
                .collect()
        };
        let skip: Vec<Entity> = std::iter::once(e).chain(targets.iter().map(|(t, _)| *t)).collect();
        let walls = outlines.edges_near(eye, v.radius, &skip);
        let facing = turn + v.direction;
        let rim = visibility(eye, facing, v.fov, v.radius, v.rays, &walls);
        let full = v.fov >= std::f32::consts::TAU - 1e-3;
        let mut state = VisionState { rim, eye, seen: Vec::new() };
        for (t, p) in &targets {
            if (*p - eye).length_squared() <= v.radius * v.radius && state.sees(*p, full) {
                state.seen.push(*t);
            }
        }
        let before = world.get::<&Vision>(e).map(|v| v.state.seen.len()).unwrap_or(0);
        let now = state.seen.len();
        if before == 0 && now > 0 {
            crate::ecs::callbacks::push(e, "on_seen");
        } else if before > 0 && now == 0 {
            crate::ecs::callbacks::push(e, "on_lost");
        }
        // What a script reads.
        let custom = serde_json::json!({ "seen": now });
        let custom = match world.get::<&mut CustomComponents>(e) {
            Ok(mut c) => {
                c.0.insert("Vision".into(), custom);
                None
            }
            Err(_) => Some(custom),
        };
        if let Some(custom) = custom {
            let mut map = std::collections::HashMap::new();
            map.insert("Vision".into(), custom);
            let _ = world.insert_one(e, CustomComponents(map));
        }
        if v.draw {
            let mesh = fan(&state, turn, full, &v);
            let missing = match world.get::<&mut Mesh2D>(e) {
                Ok(mut m) => {
                    m.vertices = mesh.vertices;
                    m.indices = mesh.indices;
                    m.z_order = v.z_order;
                    None
                }
                Err(_) => Some(mesh),
            };
            if let Some(mesh) = missing {
                let _ = world.insert_one(e, mesh);
            }
        }
        if let Ok(mut v) = world.get::<&mut Vision>(e) {
            v.state = state;
        }
    }
}

/// The fan of what is seen, in the entity's own frame.
fn fan(state: &VisionState, turn: f32, full: bool, v: &Vision) -> Mesh2D {
    let un = |p: Vec2| Vec2::from_angle(-turn).rotate(p - state.eye);
    let mut vertices = vec![MeshVertex { position: [0.0, 0.0], uv: [0.5, 0.5], color: v.color, q: 1.0 }];
    let radius = v.radius.max(1e-3);
    for p in &state.rim {
        let l = un(*p);
        let f = (l.length() / radius).clamp(0.0, 1.0);
        let color = std::array::from_fn(|i| v.color[i] + (v.edge_color[i] - v.color[i]) * f);
        vertices.push(MeshVertex { position: [l.x, l.y], uv: [0.5, 0.5], color, q: 1.0 });
    }
    let n = state.rim.len() as u32;
    let mut indices = Vec::new();
    let tris = if full { n } else { n.saturating_sub(1) };
    for i in 0..tris {
        indices.extend_from_slice(&[0, 1 + i, 1 + (i + 1) % n]);
    }
    Mesh2D { texture_id: String::new(), vertices, indices, z_order: v.z_order, ..Default::default() }
}

/// `Vision.set(e, field, value)`: any field by name - a guard's cone turned
/// red when it raises the alarm, a lamp's reach shortened as it gutters.
pub fn set_vision_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut v = world.get::<&mut Vision>(entity).map_err(|_| "no vision on that entity".to_string())?;
    let mut whole = serde_json::to_value(&*v).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a vision field"));
    }
    // Scripts have only floats: a whole number over a count stays whole.
    let value = match (map.get(field), value) {
        (Some(serde_json::Value::Number(o)), serde_json::Value::Number(n)) if !o.is_f64() => n
            .as_f64()
            .filter(|f| f.fract() == 0.0)
            .map_or(value.clone(), |f| (f as i64).into()),
        _ => value.clone(),
    };
    map.insert(field.to_string(), value);
    let mut new: Vision = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    new.state = std::mem::take(&mut v.state);
    *v = new;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(x: f32, y0: f32, y1: f32) -> (Vec2, Vec2) {
        (Vec2::new(x, y0), Vec2::new(x, y1))
    }

    #[test]
    fn a_wall_hides_what_is_behind_it() {
        let walls = [wall(100.0, -50.0, 50.0)];
        let rim = visibility(Vec2::ZERO, 0.0, 1.2, 400.0, 48, &walls);
        let s = VisionState { rim, eye: Vec2::ZERO, seen: vec![] };
        assert!(s.sees(Vec2::new(80.0, 0.0), false), "in front of the wall");
        assert!(!s.sees(Vec2::new(200.0, 0.0), false), "behind it");
        assert!(s.sees(Vec2::new(200.0, 110.0), false), "past its end");
        assert!(!s.sees(Vec2::new(-50.0, 0.0), false), "behind the eye, outside the cone");
    }

    #[test]
    fn all_round_sees_every_way_and_the_rim_slips_past_corners() {
        let walls = [wall(100.0, -50.0, 50.0)];
        let rim = visibility(Vec2::ZERO, 0.0, std::f32::consts::TAU, 300.0, 64, &walls);
        let s = VisionState { rim: rim.clone(), eye: Vec2::ZERO, seen: vec![] };
        for p in [Vec2::new(-200.0, 0.0), Vec2::new(0.0, 250.0), Vec2::new(0.0, -250.0)] {
            assert!(s.sees(p, true), "all round: {p}");
        }
        assert!(!s.sees(Vec2::new(250.0, 0.0), true));
        // A ray just past the wall's end reaches far: the corner was aimed at.
        assert!(rim.iter().any(|p| p.x > 250.0 && p.y > 50.0 && p.y < 160.0), "{rim:?}");
    }

    #[test]
    fn a_tagged_target_in_view_is_seen_and_the_script_is_told() {
        let mut world = World::new();
        let guard = world.spawn((
            Transform::default(),
            Vision { targets: "thief".into(), radius: 400.0, fov: 1.0, ..Default::default() },
        ));
        let mut tags = Tags::default();
        tags.0.insert("thief".into());
        let thief = world.spawn((Transform { position: Vec2::new(150.0, 0.0), ..Default::default() }, tags));
        crate::ecs::callbacks::take();
        tick_vision(&mut world);
        assert_eq!(world.get::<&Vision>(guard).unwrap().state.seen, vec![thief]);
        assert_eq!(world.get::<&CustomComponents>(guard).unwrap().0["Vision"]["seen"], 1);
        assert!(world.get::<&Mesh2D>(guard).unwrap().indices.len() >= 3, "the fan is drawn");
        // A wall between them: lost.
        world.spawn((
            Transform { position: Vec2::new(80.0, 0.0), ..Default::default() },
            crate::ecs::components::Collider2D {
                shape: crate::ecs::components::ColliderShape::Box { width: 10.0, height: 200.0 },
                ..Default::default()
            },
        ));
        tick_vision(&mut world);
        assert!(world.get::<&Vision>(guard).unwrap().state.seen.is_empty());
        let calls: Vec<&str> = crate::ecs::callbacks::take().iter().map(|c| c.handler).collect();
        assert_eq!(calls, vec!["on_seen", "on_lost"]);
    }
}
