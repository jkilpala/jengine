//! **Path morphing** (Oct 2026): one drawn shape turning into another - a
//! logo that becomes a bird, a blob that becomes a star, a map border that
//! shifts.
//!
//! A [`PathMorph`] on an entity with a `vector_path` eases that path into the
//! shape of another entity's `vector_path` (`Morph.to(e, name, seconds)`).
//! Both shapes are cut into `points` points evenly spaced along their length
//! ([`resample`]), the target's turned round its loop to start where it best
//! lines up with the shape now ([`align`] - so a morph does not twist inside
//! out), and each frame the entity's path is written as the points eased
//! between them, with the fill and stroke colours eased too. Shapes are read
//! in each entity's own frame, so the target can be drawn anywhere. Ticked in
//! `tick_motion`; the path draws itself, so no host was wired. `on_morph_end`
//! reaches the entity's script when a morph finishes.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Name, PathCommand, VectorPath};
use crate::ecs::path_follow::flatten;

/// Easing between the two shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MorphEase {
    Linear,
    #[default]
    InOut,
    Out,
    /// Overshoots a little and settles - a springy shape.
    Back,
}

impl MorphEase {
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            MorphEase::Linear => t,
            MorphEase::InOut => t * t * (3.0 - 2.0 * t),
            MorphEase::Out => 1.0 - (1.0 - t) * (1.0 - t),
            MorphEase::Back => {
                let c = 1.70158;
                let u = t - 1.0;
                1.0 + (c + 1.0) * u * u * u + c * u * u
            }
        }
    }
}

/// Eases this entity's `vector_path` into another's shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PathMorph {
    /// The entity whose shape it is turning into (empty: still).
    pub target: String,
    /// Seconds a morph takes.
    pub duration: f32,
    /// How many points a shape is cut into.
    pub points: u32,
    pub ease: MorphEase,
    /// How far through the morph it is, 0 to 1.
    pub progress: f32,
    /// The shape it is turning from, and the target's lined up (never
    /// saved).
    #[serde(skip)]
    pub from: Vec<Vec2>,
    #[serde(skip)]
    pub to: Vec<Vec2>,
    #[serde(skip)]
    pub colors: Option<([Option<[f32; 4]>; 2], [Option<[f32; 4]>; 2])>,
    #[serde(skip)]
    pub started: String,
    /// The last frame of the morph has been written (never saved).
    #[serde(skip)]
    pub settled: bool,
}

impl Default for PathMorph {
    fn default() -> Self {
        Self {
            target: String::new(),
            duration: 1.0,
            points: 96,
            ease: MorphEase::InOut,
            progress: 1.0,
            from: Vec::new(),
            to: Vec::new(),
            colors: None,
            started: String::new(),
            settled: false,
        }
    }
}

/// A closed polyline cut into `n` points evenly spaced along its length,
/// starting at its first point.
pub fn resample(poly: &[Vec2], n: usize) -> Vec<Vec2> {
    let n = n.max(3);
    if poly.len() < 2 {
        return vec![poly.first().copied().unwrap_or(Vec2::ZERO); n];
    }
    let mut ring: Vec<Vec2> = poly.to_vec();
    if ring.first() != ring.last() {
        ring.push(ring[0]);
    }
    let mut along = vec![0.0f32];
    for w in ring.windows(2) {
        along.push(along.last().unwrap() + w[0].distance(w[1]));
    }
    let total = *along.last().unwrap();
    if total <= 0.0 {
        return vec![ring[0]; n];
    }
    let mut out = Vec::with_capacity(n);
    let mut seg = 0;
    for k in 0..n {
        let d = total * k as f32 / n as f32;
        while seg + 1 < along.len() - 1 && along[seg + 1] < d {
            seg += 1;
        }
        let len = along[seg + 1] - along[seg];
        let t = if len > 1e-6 { (d - along[seg]) / len } else { 0.0 };
        out.push(ring[seg].lerp(ring[seg + 1], t));
    }
    out
}

/// Whether a closed loop runs anticlockwise.
fn anticlockwise(p: &[Vec2]) -> bool {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u.x * v.y - v.x * u.y;
    }
    a >= 0.0
}

/// `to` turned round its loop (and reversed, if it runs the other way) to
/// line up with `from`: the rotation with the least summed distance between
/// matching points.
pub fn align(from: &[Vec2], to: &[Vec2]) -> Vec<Vec2> {
    let n = to.len();
    if n == 0 || from.len() != n {
        return to.to_vec();
    }
    let mut to = to.to_vec();
    if anticlockwise(from) != anticlockwise(&to) {
        to.reverse();
    }
    let best = (0..n)
        .min_by(|&a, &b| {
            let cost = |r: usize| (0..n).map(|i| from[i].distance_squared(to[(i + r) % n])).sum::<f32>();
            cost(a).total_cmp(&cost(b))
        })
        .unwrap_or(0);
    (0..n).map(|i| to[(i + best) % n]).collect()
}

fn path_points(path: &VectorPath) -> Vec<Vec2> {
    let (mut pts, _) = flatten(path);
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    pts
}

fn closed(points: &[Vec2]) -> Vec<PathCommand> {
    let mut cmds = Vec::with_capacity(points.len() + 2);
    if let Some(p) = points.first() {
        cmds.push(PathCommand::MoveTo { x: p.x, y: p.y });
    }
    for p in points.iter().skip(1) {
        cmds.push(PathCommand::LineTo { x: p.x, y: p.y });
    }
    cmds.push(PathCommand::Close);
    cmds
}

fn mix(a: Option<[f32; 4]>, b: Option<[f32; 4]>, t: f32) -> Option<[f32; 4]> {
    match (a, b) {
        (Some(a), Some(b)) => Some(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)),
        (Some(a), None) => (t < 1.0).then_some([a[0], a[1], a[2], a[3] * (1.0 - t)]),
        (None, Some(b)) => Some([b[0], b[1], b[2], b[3] * t]),
        (None, None) => None,
    }
}

/// One frame of every morph.
pub fn tick_morphs(world: &mut World, dt: f32) {
    // Morphs to start: a target named that is not the one under way.
    let starts: Vec<(Entity, String, u32)> = world
        .query::<&PathMorph>()
        .iter()
        .filter(|(_, m)| !m.target.is_empty() && m.target != m.started)
        .map(|(e, m)| (e, m.target.clone(), m.points))
        .collect();
    for (e, target, points) in starts {
        let Some(goal) = world
            .query::<(&Name, &VectorPath)>()
            .iter()
            .find(|(_, (n, _))| n.0 == target)
            .map(|(_, (_, p))| p.clone())
        else {
            continue;
        };
        let Ok(now) = world.get::<&VectorPath>(e).map(|p| (*p).clone()) else { continue };
        let n = points.max(3) as usize;
        let from = resample(&path_points(&now), n);
        let to = align(&from, &resample(&path_points(&goal), n));
        if let Ok(mut m) = world.get::<&mut PathMorph>(e) {
            m.from = from;
            m.to = to;
            m.colors = Some(([now.fill_color, now.stroke_color], [goal.fill_color, goal.stroke_color]));
            m.started = target;
            m.progress = 0.0;
            m.settled = false;
        }
    }
    let mut ended: Vec<Entity> = Vec::new();
    for (e, (m, path)) in world.query_mut::<(&mut PathMorph, &mut VectorPath)>() {
        if m.from.is_empty() || m.settled {
            continue;
        }
        let was = m.progress;
        m.progress = (m.progress + dt / m.duration.max(1e-3)).min(1.0);
        let t = m.ease.apply(m.progress);
        let pts: Vec<Vec2> = m.from.iter().zip(&m.to).map(|(a, b)| *a + (*b - *a) * t).collect();
        path.commands = closed(&pts);
        path.closed = true;
        if let Some(([f0, s0], [f1, s1])) = m.colors {
            path.fill_color = mix(f0, f1, t.clamp(0.0, 1.0));
            path.stroke_color = mix(s0, s1, t.clamp(0.0, 1.0));
        }
        if m.progress >= 1.0 {
            m.settled = true;
            if was < 1.0 {
                ended.push(e);
            }
        }
    }
    for e in ended {
        crate::ecs::callbacks::push(e, "on_morph_end");
    }
}

/// `Morph.to(e, target, seconds)`: turn into that entity's shape, over
/// `seconds` (negative keeps the morph's own duration).
pub fn morph_to(world: &mut World, e: Entity, target: &str, seconds: f32) -> Result<(), String> {
    if !world.contains(e) {
        return Err("no such entity".into());
    }
    let missing = match world.get::<&mut PathMorph>(e) {
        Ok(mut m) => {
            m.target = target.to_string();
            m.started.clear();
            if seconds >= 0.0 {
                m.duration = seconds;
            }
            None
        }
        Err(_) => Some(PathMorph { target: target.to_string(), duration: if seconds >= 0.0 { seconds } else { 1.0 }, ..Default::default() }),
    };
    if let Some(m) = missing {
        world.insert_one(e, m).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// `Morph.set(e, field, value)`: any field by name; a new `target` starts a
/// morph as `Morph.to` does.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut m = world.get::<&mut PathMorph>(e).map_err(|_| "no path_morph on that entity".to_string())?;
    let mut v = serde_json::to_value(&*m).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("path_morph has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    let mut next: PathMorph = serde_json::from_value(v).map_err(|e| format!("path_morph.{field}: {e}"))?;
    next.from = std::mem::take(&mut m.from);
    next.to = std::mem::take(&mut m.to);
    next.colors = m.colors.take();
    next.started = std::mem::take(&mut m.started);
    next.settled = m.settled;
    if field == "target" {
        next.started.clear();
    }
    *m = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(h: f32) -> Vec<Vec2> {
        vec![Vec2::new(-h, -h), Vec2::new(h, -h), Vec2::new(h, h), Vec2::new(-h, h)]
    }

    #[test]
    fn resampling_spaces_points_evenly_round_the_loop() {
        let pts = resample(&square(10.0), 8);
        assert_eq!(pts.len(), 8);
        // Perimeter 80: a point every 10, so corners and edge middles.
        assert!((pts[0] - Vec2::new(-10.0, -10.0)).length() < 1e-4);
        assert!((pts[1] - Vec2::new(0.0, -10.0)).length() < 1e-4);
        assert!((pts[2] - Vec2::new(10.0, -10.0)).length() < 1e-4);
        assert!((pts[7] - Vec2::new(-10.0, 0.0)).length() < 1e-4);
    }

    #[test]
    fn the_target_is_lined_up_so_nothing_twists() {
        let from = resample(&square(10.0), 8);
        // The same square, started elsewhere and run the other way.
        let mut other = from.clone();
        other.rotate_left(3);
        other.reverse();
        let lined = align(&from, &other);
        for (a, b) in from.iter().zip(&lined) {
            assert!(a.distance(*b) < 1e-4, "{a} vs {b}");
        }
    }

    #[test]
    fn eases_end_where_they_should() {
        for e in [MorphEase::Linear, MorphEase::InOut, MorphEase::Out, MorphEase::Back] {
            assert!(e.apply(0.0).abs() < 1e-5 && (e.apply(1.0) - 1.0).abs() < 1e-5, "{e:?}");
        }
        assert!((0..=20).any(|i| MorphEase::Back.apply(i as f32 / 20.0) > 1.0), "back overshoots");
    }
}
