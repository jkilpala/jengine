//! Following a curve (Oct 2026; Godot's `PathFollow2D`, Unity's spline
//! animate).
//!
//! A [`PathFollow`] names an entity carrying a `vector_path` - the same
//! component that draws a route - and moves its own entity along it at
//! `speed` world units a second: round and round (`loop`), back and forth
//! (`ping_pong`) or to the end and stop (`once`, which calls the script's
//! `on_path_end`). It turns to face along the curve when `rotate` is on, and
//! rides `lateral` units to the left of it - a lane beside the line. The
//! path is read in **world** space through its entity's transform, so a
//! path can move, turn and scale and its riders go with it.
//!
//! `patrol_path` walks the straight segments between waypoints; this rides
//! the drawn curve itself, beziers and all, at an even speed: the path is
//! flattened into short segments and measured, and the follower's
//! `progress` is a distance along it.
//!
//! [`tick_path_follow`] runs inside `behaviors::tick_motion`, so every host
//! has it; `PathFollow.set(e, field, value)` changes any field.

use crate::detmath::DetMath;
use std::collections::HashMap;

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Name, Parent, PathCommand, Transform, VectorPath};

/// How a follower behaves at the ends of an open path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowMode {
    /// Back to the start (seamlessly, on a closed path).
    #[default]
    Loop,
    /// Turn round at each end.
    PingPong,
    /// Stop at the end and call `on_path_end`.
    Once,
}

fn default_speed() -> f32 {
    100.0
}
fn default_true() -> bool {
    true
}

/// `path_follow`: ride along another entity's `vector_path`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathFollow {
    /// The name of the entity whose `vector_path` to follow.
    #[serde(default)]
    pub path: String,
    /// World units a second; negative rides backwards.
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// How far along the path it is, in world units.
    #[serde(default)]
    pub progress: f32,
    #[serde(default)]
    pub mode: FollowMode,
    /// Turn to face along the path.
    #[serde(default = "default_true")]
    pub rotate: bool,
    /// Added to the facing, in radians - for art that points up, `-PI/2`.
    #[serde(default)]
    pub rotation_offset: f32,
    /// World units to the left of the path, facing along it (negative is
    /// right): a lane beside the line.
    #[serde(default)]
    pub lateral: f32,
    /// Whether it moves; `once` turns it off at the end.
    #[serde(default = "default_true")]
    pub playing: bool,
    /// Riding backwards on the way back of a `ping_pong`.
    #[serde(skip)]
    pub returning: bool,
}

impl Default for PathFollow {
    fn default() -> Self {
        Self {
            path: String::new(),
            speed: default_speed(),
            progress: 0.0,
            mode: FollowMode::Loop,
            rotate: true,
            rotation_offset: 0.0,
            lateral: 0.0,
            playing: true,
            returning: false,
        }
    }
}

/// The handler a `once` follower's script gets at the end.
pub const END_HANDLER: &str = "on_path_end";

/// How finely a curve is cut: segments per bezier.
const CURVE_STEPS: usize = 24;

/// A path flattened into a polyline in local space - its first sub-path,
/// closed when the path says so.
pub fn flatten(path: &VectorPath) -> (Vec<Vec2>, bool) {
    let mut pts: Vec<Vec2> = Vec::new();
    let mut closed = path.closed;
    let mut pen = Vec2::ZERO;
    for cmd in &path.commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                if pts.len() > 1 {
                    break;
                }
                pts.clear();
                pen = Vec2::new(x, y);
                pts.push(pen);
            }
            PathCommand::LineTo { x, y } => {
                if pts.is_empty() {
                    pts.push(pen);
                }
                pen = Vec2::new(x, y);
                pts.push(pen);
            }
            PathCommand::QuadTo { cx, cy, x, y } => {
                if pts.is_empty() {
                    pts.push(pen);
                }
                let (p0, c, p1) = (pen, Vec2::new(cx, cy), Vec2::new(x, y));
                for i in 1..=CURVE_STEPS {
                    let t = i as f32 / CURVE_STEPS as f32;
                    let u = 1.0 - t;
                    pts.push(p0 * u * u + c * 2.0 * u * t + p1 * t * t);
                }
                pen = p1;
            }
            PathCommand::CubicTo { c1x, c1y, c2x, c2y, x, y } => {
                if pts.is_empty() {
                    pts.push(pen);
                }
                let (p0, c1, c2, p1) = (pen, Vec2::new(c1x, c1y), Vec2::new(c2x, c2y), Vec2::new(x, y));
                for i in 1..=CURVE_STEPS {
                    let t = i as f32 / CURVE_STEPS as f32;
                    let u = 1.0 - t;
                    pts.push(p0 * u * u * u + c1 * 3.0 * u * u * t + c2 * 3.0 * u * t * t + p1 * t * t * t);
                }
                pen = p1;
            }
            PathCommand::Close => {
                closed = true;
                break;
            }
        }
    }
    pts.dedup_by(|a, b| a.distance_squared(*b) < 1e-8);
    if closed && pts.len() > 2 && pts[0].distance_squared(*pts.last().unwrap()) > 1e-8 {
        pts.push(pts[0]);
    }
    (pts, closed)
}

/// A path in world space, measured.
#[derive(Debug, Clone)]
pub struct MeasuredPath {
    pub points: Vec<Vec2>,
    /// Distance from the start to each point.
    pub along: Vec<f32>,
    pub closed: bool,
}

impl MeasuredPath {
    pub fn new(points: Vec<Vec2>, closed: bool) -> Self {
        let mut along = Vec::with_capacity(points.len());
        let mut d = 0.0;
        for (i, p) in points.iter().enumerate() {
            if i > 0 {
                d += p.distance(points[i - 1]);
            }
            along.push(d);
        }
        Self { points, along, closed }
    }

    pub fn length(&self) -> f32 {
        self.along.last().copied().unwrap_or(0.0)
    }

    /// The point `d` along, and the unit direction there.
    pub fn sample(&self, d: f32) -> (Vec2, Vec2) {
        let n = self.points.len();
        if n == 0 {
            return (Vec2::ZERO, Vec2::X);
        }
        if n == 1 {
            return (self.points[0], Vec2::X);
        }
        let d = d.clamp(0.0, self.length());
        // The segment holding `d`.
        let i = match self.along.binary_search_by(|a| a.partial_cmp(&d).unwrap_or(std::cmp::Ordering::Less)) {
            Ok(i) => i.min(n - 2),
            Err(i) => i.saturating_sub(1).min(n - 2),
        };
        let (a, b) = (self.points[i], self.points[i + 1]);
        let seg = self.along[i + 1] - self.along[i];
        let t = if seg > 1e-6 { (d - self.along[i]) / seg } else { 0.0 };
        let dir = (b - a).try_normalize().unwrap_or(Vec2::X);
        (a.lerp(b, t), dir)
    }
}

/// The named path in world space, or `None` when no entity of that name
/// carries a `vector_path` with a segment in it.
pub fn measured_path(world: &World, name: &str) -> Option<MeasuredPath> {
    let (e, path) = world
        .query::<(&Name, &VectorPath)>()
        .iter()
        .find(|(_, (n, _))| n.0 == name)
        .map(|(e, (_, p))| (e, p.clone()))?;
    let (local, closed) = flatten(&path);
    if local.len() < 2 {
        return None;
    }
    let points = match crate::ecs::hierarchy::world_transform_of(world, e) {
        Some(w) => local.iter().map(|p| w.apply(*p)).collect(),
        None => match world.get::<&Transform>(e) {
            Ok(t) => {
                let (sin, cos) = t.rotation.dsin_cos();
                local
                    .iter()
                    .map(|p| {
                        let s = *p * t.scale;
                        t.position + Vec2::new(s.x * cos - s.y * sin, s.x * sin + s.y * cos)
                    })
                    .collect()
            }
            Err(_) => local,
        },
    };
    Some(MeasuredPath::new(points, closed))
}

/// A world point in the frame of `w` - the inverse of `WorldTransform::apply`.
fn to_local(w: &crate::ecs::components::WorldTransform, p: Vec2) -> Vec2 {
    let (sin, cos) = (-w.rotation).dsin_cos();
    let d = p - w.position;
    let k = Vec2::new(d.x * cos - d.y * sin, d.x * sin + d.y * cos);
    let s = Vec2::new(k.x - w.shear * k.y, k.y);
    let scale = Vec2::new(
        if w.scale.x.abs() > 1e-6 { w.scale.x } else { 1.0 },
        if w.scale.y.abs() > 1e-6 { w.scale.y } else { 1.0 },
    );
    s / scale
}

/// Move every playing follower `dt` along its path and place it there.
pub fn tick_path_follow(world: &mut World, dt: f32) {
    let followers: Vec<(Entity, PathFollow)> =
        world.query::<&PathFollow>().iter().map(|(e, f)| (e, f.clone())).collect();
    if followers.is_empty() {
        return;
    }
    let mut paths: HashMap<String, Option<MeasuredPath>> = HashMap::new();
    for (e, mut f) in followers {
        let Some(path) = paths.entry(f.path.clone()).or_insert_with(|| measured_path(world, &f.path)).clone() else {
            continue;
        };
        let length = path.length();
        if length <= 1e-4 {
            continue;
        }
        let mut ended = false;
        if f.playing {
            let step = f.speed * dt * if f.returning { -1.0 } else { 1.0 };
            f.progress += step;
            match f.mode {
                FollowMode::Loop => f.progress = f.progress.rem_euclid(length),
                FollowMode::PingPong => {
                    // Reflect off each end, as many times as one step needs.
                    let mut guard = 0;
                    while (f.progress < 0.0 || f.progress > length) && guard < 8 {
                        f.progress = if f.progress > length { 2.0 * length - f.progress } else { -f.progress };
                        f.returning = !f.returning;
                        guard += 1;
                    }
                    f.progress = f.progress.clamp(0.0, length);
                }
                FollowMode::Once => {
                    let end = if f.speed >= 0.0 { length } else { 0.0 };
                    if (f.speed >= 0.0 && f.progress >= length) || (f.speed < 0.0 && f.progress <= 0.0) {
                        f.progress = end;
                        f.playing = false;
                        ended = true;
                    }
                }
            }
        }
        let (at, dir) = path.sample(f.progress);
        // Facing the way it is going: back along the path when riding it
        // backwards.
        let heading = if (f.speed < 0.0) != f.returning { -dir } else { dir };
        let left = Vec2::new(-heading.y, heading.x);
        let pos = at + left * f.lateral;
        let rotation = heading.y.datan2(heading.x) + f.rotation_offset;
        let has_parent = world.get::<&Parent>(e).is_ok();
        if let Ok(mut t) = world.get::<&mut Transform>(e) {
            if !has_parent {
                t.position = pos;
                if f.rotate {
                    t.rotation = rotation;
                }
            } else if let Some(pw) = world
                .get::<&Parent>(e)
                .ok()
                .and_then(|p| crate::ecs::hierarchy::world_transform_of(world, p.0))
            {
                // A follower with a parent is placed in the parent's frame.
                t.position = to_local(&pw, pos);
                if f.rotate {
                    t.rotation = rotation - pw.rotation;
                }
            }
        }
        if let Ok(mut c) = world.get::<&mut PathFollow>(e) {
            c.progress = f.progress;
            c.playing = f.playing;
            c.returning = f.returning;
        }
        if ended {
            crate::ecs::callbacks::push(e, END_HANDLER);
        }
    }
}

/// `PathFollow.set(e, field, value)`: write any field by name - `path` to
/// switch tracks, `progress` to jump, `playing` to start or stop.
pub fn set_path_follow_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let Ok(mut c) = world.get::<&mut PathFollow>(entity) else {
        return Err("no path_follow on that entity".into());
    };
    let mut whole = serde_json::to_value(&*c).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a path_follow field"));
    }
    map.insert(field.to_string(), value.clone());
    let mut new: PathFollow = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    new.returning = c.returning;
    *c = new;
    Ok(())
}
