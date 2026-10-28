//! **Flow along a path** (Oct 2026): water that runs along a drawn curve -
//! a millrace, a river, a conveyor of lava - drawn as a ribbon whose texture
//! scrolls along the curve, and carrying whatever floats in it.
//!
//! A [`FlowPath`] names an entity carrying a `vector_path` (measured in world
//! space by `path_follow::measured_path`, so a path that moves takes its
//! water with it). Anything with a `Velocity2D` - only things tagged `tag`,
//! when it is set - within `width / 2` of the curve is eased towards the
//! flow's velocity there by `grip` a second ([`carry`]): along the curve, at
//! `speed` in the middle and `bank_speed` of it at the banks. A dynamic body
//! takes it through its `Velocity2D` (a velocity written from outside
//! reaches the body), anything else through `velocity_system`, so the
//! physics needed nothing.
//!
//! The ribbon ([`mesh`]) is cut where the texture repeats, every `tile`
//! world units, because every sampler in the engine clamps; the cuts move
//! with the water, so the texture runs along the curve at the water's
//! speed, round bends and all. Its edges fade over `soft` of the
//! half-width, in the vertex colours. Ticked in `tick_motion`, drawn by
//! `render_world_items` as one `Skinned` item - no host was wired.

use crate::detmath::DetMath;
use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Tags, Transform, Velocity2D, WorldTransform};
use crate::ecs::path_follow::{MeasuredPath, measured_path};

/// Water running along a drawn curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FlowPath {
    /// The entity whose `vector_path` the water runs along, by name.
    pub path: String,
    /// The channel's width, world units.
    pub width: f32,
    /// How fast the middle runs, world units a second (negative runs back).
    pub speed: f32,
    /// How fast the banks run, as a share of `speed`.
    pub bank_speed: f32,
    /// How quickly a floating thing takes the water's velocity, a second
    /// (0: the water carries nothing).
    pub grip: f32,
    /// Only things with this tag are carried (empty: anything with a
    /// `Velocity2D`).
    pub tag: String,
    /// The water's picture, its streaks running down it (v), and the world
    /// length one repeat covers.
    pub texture: String,
    pub tile: f32,
    /// Linear tint, alpha included.
    pub color: [f32; 4],
    /// Share of the half-width over which each edge fades.
    pub soft: f32,
    pub z_order: i32,
    /// How far the texture has run, world units (never saved).
    #[serde(skip)]
    pub scroll: f32,
}

impl Default for FlowPath {
    fn default() -> Self {
        Self {
            path: String::new(),
            width: 80.0,
            speed: 120.0,
            bank_speed: 0.35,
            grip: 3.0,
            tag: String::new(),
            texture: String::new(),
            tile: 128.0,
            color: [0.35, 0.55, 0.75, 0.9],
            soft: 0.25,
            z_order: 0,
            scroll: 0.0,
        }
    }
}

/// The nearest point of `path` to `p`: its distance along the path, the
/// unit direction there, and how far `p` is from it.
pub fn nearest(path: &MeasuredPath, p: Vec2) -> Option<(f32, Vec2, f32)> {
    let mut best: Option<(f32, Vec2, f32)> = None;
    for (i, w) in path.points.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let len2 = ab.length_squared();
        if len2 < 1e-9 {
            continue;
        }
        let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
        let q = a + ab * t;
        let d = p.distance(q);
        if best.is_none_or(|(_, _, bd)| d < bd) {
            best = Some((path.along[i] + t * len2.sqrt(), ab / len2.sqrt(), d));
        }
    }
    best
}

/// The water's velocity at `p`, or `None` outside the channel: along the
/// curve, `speed` in the middle easing to `bank_speed` of it at the banks.
pub fn flow_at(f: &FlowPath, path: &MeasuredPath, p: Vec2) -> Option<Vec2> {
    let half = f.width.max(0.0) * 0.5;
    let (along, _, d) = nearest(path, p)?;
    if d > half || half <= 0.0 {
        return None;
    }
    // The way the water runs: the curve's direction averaged over half the
    // channel's width about the nearest point, so it turns into a bend
    // before the corner - taken at the point itself, a sharp corner pushed
    // whatever reached it straight on, out over the bank.
    let (_, before) = path.sample(along - half * 0.5);
    let (_, after) = path.sample(along + half * 0.5);
    let dir = (before + after).normalize_or(after);
    let across = d / half;
    let profile = f.bank_speed + (1.0 - f.bank_speed) * (1.0 - across * across);
    Some(dir * f.speed * profile)
}

/// One frame of every flow: the texture run on, and what floats in the
/// water eased towards its velocity.
pub fn tick_flows(world: &mut World, dt: f32) {
    let flows: Vec<(Entity, FlowPath)> = world.query::<&FlowPath>().iter().map(|(e, f)| (e, f.clone())).collect();
    if flows.is_empty() {
        return;
    }
    for (e, f) in &flows {
        if let Ok(mut real) = world.get::<&mut FlowPath>(*e) {
            let tile = f.tile.max(1.0);
            real.scroll = (real.scroll + f.speed * dt).rem_euclid(tile);
        }
    }
    let paths: Vec<(FlowPath, MeasuredPath)> = flows
        .into_iter()
        .filter(|(_, f)| f.grip > 0.0)
        .filter_map(|(_, f)| measured_path(world, &f.path).map(|p| (f, p)))
        .collect();
    if paths.is_empty() {
        return;
    }
    for (e, (v, t, w, tags)) in world.query_mut::<(&mut Velocity2D, &Transform, Option<&WorldTransform>, Option<&Tags>)>() {
        let _ = e;
        let at = w.map_or(t.position, |w| w.position);
        for (f, path) in &paths {
            if !f.tag.is_empty() && !tags.is_some_and(|t| t.0.contains(&f.tag)) {
                continue;
            }
            if let Some(water) = flow_at(f, path, at) {
                let now = carry(Vec2::new(v.x, v.y), water, f.grip, dt);
                v.x = now.x;
                v.y = now.y;
                break;
            }
        }
    }
}

/// A floating thing's velocity `v` after `dt` in water running at `water`:
/// eased towards it by `grip` a second, the same at any frame rate.
pub fn carry(v: Vec2, water: Vec2, grip: f32, dt: f32) -> Vec2 {
    let k = 1.0 - (-grip.max(0.0) * dt).dexp();
    v + (water - v) * k
}

/// The ribbon: four points across (edge, inner, inner, edge - the edges
/// clear), at every point of the path and at every place the texture
/// repeats, each span its own quads so a repeat's texture coordinates run
/// 0 to 1 within it. Positions, texture coordinates, colours, indices.
pub fn mesh(f: &FlowPath, path: &MeasuredPath) -> (Vec<Vec2>, Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let (mut pos, mut uvq, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let length = path.length();
    if length <= 0.0 || f.width <= 0.0 {
        return (pos, uvq, col, idx);
    }
    let tile = f.tile.max(1.0);
    // Stations: the path's own points and every repeat's start, which sit
    // at `scroll` plus whole tiles - so they run downstream with the water.
    let mut at: Vec<f32> = path.along.clone();
    let mut d = f.scroll.rem_euclid(tile);
    while d < length {
        if d > 0.0 {
            at.push(d);
        }
        d += tile;
    }
    at.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    at.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    let half = f.width * 0.5;
    let soft = f.soft.clamp(0.0, 0.49);
    let across = [0.0, soft, 1.0 - soft, 1.0];
    let lit = f.color;
    let clear = [f.color[0], f.color[1], f.color[2], 0.0];
    // The direction at a station: the mean of the segments either side.
    let side_at = |d: f32| {
        let (_, a) = path.sample((d - 0.5).max(0.0));
        let (_, b) = path.sample((d + 0.5).min(length));
        let dir = (a + b).normalize_or(a);
        Vec2::new(-dir.y, dir.x)
    };
    for w in at.windows(2) {
        let (d0, d1) = (w[0], w[1]);
        if d1 - d0 < 1e-4 {
            continue;
        }
        // The texture coordinate down the water: whole repeats fall on
        // stations, so within a span it runs from its fraction up.
        let v0 = ((d0 - f.scroll) / tile).rem_euclid(1.0);
        let v0 = if v0 > 1.0 - 1e-4 { 0.0 } else { v0 };
        let v1 = v0 + (d1 - d0) / tile;
        let base = pos.len() as u32;
        for &(d, v) in &[(d0, v0), (d1, v1)] {
            let (c, _) = path.sample(d);
            let side = side_at(d);
            for (k, &u) in across.iter().enumerate() {
                pos.push(c + side * half * (1.0 - 2.0 * u));
                uvq.push([u, v.min(1.0), 1.0]);
                col.push(if k == 0 || k == 3 { clear } else { lit });
            }
        }
        for k in 0..3u32 {
            let (a, b, c, e) = (base + k, base + k + 1, base + 4 + k + 1, base + 4 + k);
            idx.extend_from_slice(&[a, b, c, a, c, e]);
        }
    }
    (pos, uvq, col, idx)
}

/// `Flow.set(e, field, value)`: any field by name, keeping the scroll.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut f = world.get::<&mut FlowPath>(e).map_err(|_| "no flow_path on that entity".to_string())?;
    let mut v = serde_json::to_value(&*f).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("flow_path has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    let mut next: FlowPath = serde_json::from_value(v).map_err(|e| format!("flow_path.{field}: {e}"))?;
    next.scroll = f.scroll;
    *f = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bend() -> MeasuredPath {
        // Right along y = 0 for 200, then up for 200.
        MeasuredPath::new(vec![Vec2::ZERO, Vec2::new(200.0, 0.0), Vec2::new(200.0, 200.0)], false)
    }

    #[test]
    fn the_water_runs_along_the_curve_fastest_in_the_middle() {
        let f = FlowPath { width: 40.0, speed: 100.0, bank_speed: 0.4, ..Default::default() };
        let p = bend();
        let mid = flow_at(&f, &p, Vec2::new(100.0, 0.0)).unwrap();
        assert!((mid - Vec2::new(100.0, 0.0)).length() < 1e-3, "{mid}");
        let bank = flow_at(&f, &p, Vec2::new(100.0, 19.9)).unwrap();
        assert!((bank.x - 40.0).abs() < 1.0 && bank.y.abs() < 1e-3, "slower at the bank: {bank}");
        let up = flow_at(&f, &p, Vec2::new(200.0, 120.0)).unwrap();
        assert!(up.y > 99.0 && up.x.abs() < 1e-3, "round the bend: {up}");
        assert!(flow_at(&f, &p, Vec2::new(100.0, 25.0)).is_none(), "outside the channel");
    }

    #[test]
    fn carrying_reaches_the_water_at_any_frame_rate() {
        let water = Vec2::new(100.0, 0.0);
        let mut fast = Vec2::ZERO;
        for _ in 0..120 {
            fast = carry(fast, water, 3.0, 1.0 / 120.0);
        }
        let mut slow = Vec2::ZERO;
        for _ in 0..30 {
            slow = carry(slow, water, 3.0, 1.0 / 30.0);
        }
        assert!((fast - slow).length() < 1e-3, "{fast} vs {slow}");
        assert!((fast.x - 100.0 * (1.0 - (-3.0f32).dexp())).abs() < 1e-2, "{fast}");
    }

    #[test]
    fn the_ribbon_is_cut_where_the_texture_repeats_and_runs_with_the_water() {
        let p = MeasuredPath::new(vec![Vec2::ZERO, Vec2::new(300.0, 0.0)], false);
        let mut f = FlowPath { width: 40.0, tile: 100.0, ..Default::default() };
        let (pos, uv, col, idx) = mesh(&f, &p);
        // Three repeats: three spans, eight points and eighteen indices each.
        assert_eq!(pos.len(), 3 * 8);
        assert_eq!(idx.len(), 3 * 18);
        for span in uv.chunks(8) {
            let (v0, v1) = (span[0][1], span[4][1]);
            assert!(v0.abs() < 1e-4 && (v1 - 1.0).abs() < 1e-4, "each span one whole repeat: {v0} {v1}");
        }
        assert_eq!(col[0][3], 0.0, "a clear edge");
        assert!(col[1][3] > 0.0);
        assert!((pos[0].y - 20.0).abs() < 1e-4 && (pos[3].y + 20.0).abs() < 1e-4, "its width across");
        // Run on by a quarter repeat: every cut has moved a quarter
        // downstream, so a new span opens at the start.
        f.scroll = 25.0;
        let (pos, uv, _, _) = mesh(&f, &p);
        assert_eq!(pos.len(), 4 * 8, "four spans once the cuts have moved");
        assert!((pos[4].x - 25.0).abs() < 1e-3, "the first cut has run downstream: {}", pos[4].x);
        assert!((uv[0][1] - 0.75).abs() < 1e-4, "starting a quarter into a repeat: {}", uv[0][1]);
    }

    #[test]
    fn it_carries_what_floats_and_only_what_it_should() {
        let mut w = World::new();
        w.spawn((Transform::default(), crate::ecs::components::Name("Race".into()),
            crate::ecs::components::VectorPath {
                commands: vec![
                    crate::ecs::components::PathCommand::MoveTo { x: 0.0, y: 0.0 },
                    crate::ecs::components::PathCommand::LineTo { x: 400.0, y: 0.0 },
                ],
                ..Default::default()
            }));
        w.spawn((FlowPath { path: "Race".into(), width: 60.0, speed: 100.0, grip: 5.0, tag: "floats".into(), ..Default::default() },));
        let mut tags = std::collections::HashSet::new();
        tags.insert("floats".to_string());
        let leaf = w.spawn((Transform { position: Vec2::new(50.0, 5.0), ..Default::default() }, Velocity2D::default(), Tags(tags)));
        let stone = w.spawn((Transform { position: Vec2::new(60.0, 0.0), ..Default::default() }, Velocity2D::default()));
        let ashore = w.spawn((Transform { position: Vec2::new(60.0, 90.0), ..Default::default() }, Velocity2D::default(),
            Tags(["floats".to_string()].into_iter().collect())));
        for _ in 0..60 {
            tick_flows(&mut w, 1.0 / 60.0);
        }
        let v = |e| { let v = w.get::<&Velocity2D>(e).unwrap(); Vec2::new(v.x, v.y) };
        assert!(v(leaf).x > 90.0 && v(leaf).y.abs() < 1e-3, "the leaf is carried: {}", v(leaf));
        assert_eq!(v(stone), Vec2::ZERO, "untagged, the stone stays");
        assert_eq!(v(ashore), Vec2::ZERO, "out of the water, nothing moves it");
        assert!(w.query::<&FlowPath>().iter().next().unwrap().1.scroll > 0.0, "the texture runs");
    }
}
