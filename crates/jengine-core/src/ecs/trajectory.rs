//! Seeing a shot before it is taken (Oct 2026): a cue ball's aim line, the
//! arc of a thrown grenade, where a pinball goes off the bumpers - which a
//! game used to draw from a ray and a guess, and got wrong at the first
//! cushion or the first ball it met.
//!
//! A [`Trajectory`] on a dynamic body asks the physics where the body goes
//! if it is set moving at `velocity` now: `PhysicsWorld::predict` steps a
//! copy of the whole world - every body, joint and touching contact - by
//! the same pipeline the real step uses, so the line drawn is the step's
//! own answer and the shot taken at that velocity goes down it. The path is
//! written into the `line` of the entity named by `line` (in that entity's
//! space), the first dynamic body struck and its path into `hit_line`'s,
//! and what a script reads into the custom `Trajectory` fields (`hit` - the
//! struck entity's name, `hit_x` / `hit_y`, `end_x` / `end_y`). It runs at
//! the end of every host's physics frame (`physics_frame_end`), so no host
//! was wired. Area effects (water, wind) are not in the copy.

// Without physics only the component and its setter are compiled in.
#![cfg_attr(not(feature = "physics"), allow(dead_code, unused_imports))]

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{CustomComponents, Name, Transform, WorldTransform};

/// What a prediction found: the body's places, the first dynamic body it
/// touched, where it was then, and that body's places after.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Prediction {
    pub path: Vec<[f32; 2]>,
    /// The struck body's physics id.
    pub hit: Option<u64>,
    pub hit_at: Option<[f32; 2]>,
    pub hit_path: Vec<[f32; 2]>,
    /// Steps run: fewer than asked when everything came to rest.
    pub steps: u32,
}

/// Predict this body's path from `velocity`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trajectory {
    /// The launch velocity, in world units a second.
    #[serde(default)]
    pub velocity: [f32; 2],
    /// Off draws nothing and clears the lines.
    #[serde(default = "yes")]
    pub active: bool,
    /// How far ahead, in seconds of physics (at most 20).
    #[serde(default = "default_seconds")]
    pub seconds: f32,
    /// A point every this many steps. A script's number arrives as a float,
    /// so `3.0` is taken too.
    #[serde(default = "default_every", deserialize_with = "count")]
    pub every: u32,
    /// The entity whose `line` shows the path ("" for none).
    #[serde(default)]
    pub line: String,
    /// The entity whose `line` shows the struck body's path.
    #[serde(default)]
    pub hit_line: String,
}

impl Default for Trajectory {
    fn default() -> Self {
        Self {
            velocity: [0.0, 0.0],
            active: true,
            seconds: default_seconds(),
            every: default_every(),
            line: String::new(),
            hit_line: String::new(),
        }
    }
}

fn yes() -> bool {
    true
}
fn default_seconds() -> f32 {
    3.0
}
fn default_every() -> u32 {
    3
}

fn count<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = f64::deserialize(d)?;
    if v.is_finite() && v >= 0.0 { Ok(v.round() as u32) } else { Err(serde::de::Error::custom("not a count")) }
}

fn named(world: &World, name: &str) -> Option<Entity> {
    if name.is_empty() {
        return None;
    }
    world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e)
}

/// Write world points into `target`'s `line`, in its own space.
fn draw(world: &World, target: Option<Entity>, points: &[[f32; 2]]) {
    let Some(e) = target else { return };
    let t = match (world.get::<&WorldTransform>(e), world.get::<&Transform>(e)) {
        (Ok(w), _) => Transform { position: w.position, rotation: w.rotation, scale: w.scale },
        (_, Ok(t)) => (*t).clone(),
        _ => Transform::default(),
    };
    if let Ok(mut line) = world.get::<&mut crate::renderer::line2d::Line2D>(e) {
        let (sin, cos) = (-t.rotation).sin_cos();
        let s = t.scale.max(glam::Vec2::splat(1e-6));
        line.points = points
            .iter()
            .map(|p| {
                let d = glam::Vec2::new(p[0], p[1]) - t.position;
                [(d.x * cos - d.y * sin) / s.x, (d.x * sin + d.y * cos) / s.y]
            })
            .collect();
    }
}

fn write_custom(world: &mut World, e: Entity, value: serde_json::Value) {
    let missing = match world.get::<&mut CustomComponents>(e) {
        Ok(mut c) => {
            c.0.insert("Trajectory".into(), value);
            None
        }
        Err(_) => Some(value),
    };
    if let Some(value) = missing {
        let mut map = std::collections::HashMap::new();
        map.insert("Trajectory".into(), value);
        let _ = world.insert_one(e, CustomComponents(map));
    }
}

/// Predict every active trajectory and write its lines and fields. Called
/// by `physics_frame_end`.
#[cfg(feature = "physics")]
pub fn predict(world: &mut World, physics: &crate::physics::PhysicsWorld) {
    let asks: Vec<(Entity, Trajectory)> = world.query::<&Trajectory>().iter().map(|(e, t)| (e, t.clone())).collect();
    if asks.is_empty() {
        return;
    }
    let _span = crate::profiling::trace::span("trajectory");
    for (e, t) in asks {
        let moving = t.velocity[0] != 0.0 || t.velocity[1] != 0.0;
        let found = if t.active && moving {
            let steps = (t.seconds.clamp(0.0, 20.0) / physics.fixed_dt()).ceil() as u32;
            physics.predict(e.id() as u64, t.velocity, steps, t.every)
        } else {
            None
        };
        let p = found.unwrap_or_default();
        draw(world, named(world, &t.line), &p.path);
        draw(world, named(world, &t.hit_line), &p.hit_path);
        let hit_name = p
            .hit
            .and_then(|id| physics.owner_of(id))
            .and_then(|h| world.get::<&Name>(h).ok().map(|n| n.0.clone()))
            .unwrap_or_default();
        let end = p.path.last().copied().unwrap_or([0.0, 0.0]);
        let hit_at = p.hit_at.unwrap_or([0.0, 0.0]);
        write_custom(
            world,
            e,
            serde_json::json!({
                "hit": hit_name,
                "hit_x": hit_at[0], "hit_y": hit_at[1],
                "end_x": end[0], "end_y": end[1],
                "steps": p.steps,
            }),
        );
    }
}

/// Set any trajectory field by name (`Trajectory.set`).
pub fn set_trajectory_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut s = world.get::<&mut Trajectory>(entity).map_err(|_| "no trajectory on that entity".to_string())?;
    let mut whole = serde_json::to_value(&*s).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a trajectory field"));
    }
    map.insert(field.to_string(), value.clone());
    *s = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    Ok(())
}
