//! The scene's colliders as **outlines**, and where a short step first
//! crosses one - asked on the CPU, never through rapier, so it answers the
//! same in every runtime and whether or not physics runs.
//!
//! The shapes are the ones physics builds - the same `ColliderShape`, placed
//! by the entity's transform and offset (no scale, as rapier has none) - but
//! tested as outlines: something that needs to know only where it first
//! crosses an edge this frame should not have to be a body. Two readers:
//! particles with `collision` (rain on a roof, sparks off a wall - a thousand
//! raindrops as bodies would cost more than the storm), and foot IK with
//! `ground`, which asks where the ground is under an animated foot.

use glam::Vec2;
use hecs::World;

use crate::ecs::components::{Collider2D, ColliderShape, Transform};

/// One edge of an outline. A closed shape's edges face out (`normal`), and
/// are crossed only going in, so a particle born inside a box leaves it
/// freely; an open one (a segment, polyline or heightfield) is hit from
/// either side.
#[derive(Debug, Clone, Copy)]
struct Edge {
    a: Vec2,
    b: Vec2,
    /// The outward normal of a closed shape's edge; zero for an open one.
    normal: Vec2,
}

#[derive(Debug, Clone)]
enum Outline {
    Edges(Vec<Edge>),
    Circle { centre: Vec2, radius: f32 },
}

#[derive(Debug, Clone)]
struct Surface {
    /// The entity whose collider it is.
    owner: hecs::Entity,
    outline: Outline,
    min: Vec2,
    max: Vec2,
    /// The collider's `collision_membership`, against an emitter's `mask`.
    membership: u16,
}

/// Where a particle's step first met a surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceHit {
    /// How far along the step, 0..1.
    pub t: f32,
    pub point: Vec2,
    /// The surface's normal, facing the side the particle came from.
    pub normal: Vec2,
}

/// Every collider outline, gathered once a frame by whoever asks.
#[derive(Debug, Clone, Default)]
pub struct ColliderOutlines {
    surfaces: Vec<Surface>,
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// Anticlockwise hull (monotone chain), as rapier's `convex_hull` makes.
fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut p: Vec<Vec2> = points.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let mut hull: Vec<Vec2> = Vec::with_capacity(p.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for &q in iter {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 1] - hull[hull.len() - 2], q - hull[hull.len() - 2]) <= 0.0 {
                hull.pop();
            }
            hull.push(q);
        }
        hull.pop();
    }
    hull
}

/// A closed loop's edges, facing out whichever way it winds.
fn closed(points: &[Vec2]) -> Vec<Edge> {
    let n = points.len();
    if n < 3 {
        return open(points);
    }
    let area: f32 = (0..n).map(|i| cross(points[i], points[(i + 1) % n])).sum();
    let sign = if area >= 0.0 { 1.0 } else { -1.0 };
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            let d = b - a;
            Edge { a, b, normal: (Vec2::new(d.y, -d.x) * sign).normalize_or_zero() }
        })
        .collect()
}

fn open(points: &[Vec2]) -> Vec<Edge> {
    points.windows(2).map(|w| Edge { a: w[0], b: w[1], normal: Vec2::ZERO }).collect()
}

impl ColliderOutlines {
    /// Every non-sensor `collider` in `world`, in world space - and every
    /// colliding tilemap's solid layers (a `tilemap_collider`'s `solid`, or a
    /// layer flagged `collision`), as the boxes and shapes the physics builds
    /// for them, owned by the map's entity. Without the tiles a `vision` fan,
    /// a muffled sound or a raindrop went straight through a tile wall.
    pub fn gather(world: &World) -> Self {
        let mut surfaces = Vec::new();
        for (e, (t, col)) in world.query::<(&Transform, &Collider2D)>().iter() {
            if col.is_sensor {
                continue;
            }
            if let Some(mut s) = Self::surface(t, col) {
                s.owner = e;
                surfaces.push(s);
            }
        }
        #[cfg(feature = "physics")]
        for (map, _, hulls) in crate::navigation::tilemap_hulls(world) {
            for hull in hulls.iter() {
                let edges = closed(hull);
                if edges.is_empty() {
                    continue;
                }
                let (min, max) = edges.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), e| {
                    (lo.min(e.a).min(e.b), hi.max(e.a).max(e.b))
                });
                surfaces.push(Surface { owner: map, outline: Outline::Edges(edges), min, max, membership: 0xFFFF });
            }
        }
        Self { surfaces }
    }

    /// How many surfaces there are.
    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    fn surface(t: &Transform, col: &Collider2D) -> Option<Surface> {
        let rot = glam::Mat2::from_angle(t.rotation);
        let offset = Vec2::from(col.offset);
        let place = |p: Vec2| t.position + rot * (p + offset);
        let pts = |v: &[[f32; 2]]| -> Vec<Vec2> { v.iter().map(|p| place(Vec2::from(*p))).collect() };
        let outline = match &col.shape {
            ColliderShape::Box { width, height } => {
                let (hw, hh) = (width / 2.0, height / 2.0);
                Outline::Edges(closed(&[
                    place(Vec2::new(-hw, -hh)),
                    place(Vec2::new(hw, -hh)),
                    place(Vec2::new(hw, hh)),
                    place(Vec2::new(-hw, hh)),
                ]))
            }
            ColliderShape::Circle { radius } => Outline::Circle { centre: place(Vec2::ZERO), radius: *radius },
            ColliderShape::Capsule { half_height, radius } => {
                // `capsule_y`: its rounded ends as eight-sided arcs.
                let mut loop_pts = Vec::with_capacity(18);
                for (cy, from) in [(-*half_height, std::f32::consts::PI), (*half_height, 0.0)] {
                    for k in 0..=8 {
                        let a = from + std::f32::consts::PI * k as f32 / 8.0;
                        loop_pts.push(place(Vec2::new(a.cos() * radius, cy + a.sin() * radius)));
                    }
                }
                Outline::Edges(closed(&loop_pts))
            }
            ColliderShape::Segment { a, b } => Outline::Edges(open(&pts(&[*a, *b]))),
            ColliderShape::Triangle { a, b, c } => Outline::Edges(closed(&pts(&[*a, *b, *c]))),
            ColliderShape::ConvexPolygon { vertices } => Outline::Edges(closed(&convex_hull(&pts(vertices)))),
            ColliderShape::ConvexDecomposition { vertices, loops } => Outline::Edges(
                crate::ecs::components::decomposition_loops(vertices, loops)
                    .into_iter()
                    .flat_map(|l| closed(&pts(l)))
                    .collect(),
            ),
            ColliderShape::Polyline { vertices } => Outline::Edges(open(&pts(vertices))),
            ColliderShape::HeightField { heights, scale } => {
                // rapier spreads the samples evenly across `scale.x`, centred.
                let n = heights.len();
                if n < 2 {
                    return None;
                }
                let line: Vec<Vec2> = heights
                    .iter()
                    .enumerate()
                    .map(|(i, h)| place(Vec2::new((i as f32 / (n - 1) as f32 - 0.5) * scale[0], h * scale[1])))
                    .collect();
                Outline::Edges(open(&line))
            }
        };
        let (min, max) = match &outline {
            Outline::Circle { centre, radius } => (*centre - Vec2::splat(*radius), *centre + Vec2::splat(*radius)),
            Outline::Edges(edges) => {
                if edges.is_empty() {
                    return None;
                }
                edges.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), e| {
                    (lo.min(e.a).min(e.b), hi.max(e.a).max(e.b))
                })
            }
        };
        Some(Surface { owner: hecs::Entity::DANGLING, outline, min, max, membership: col.collision_membership })
    }

    /// How many surfaces the line from `a` to `b` passes through - the walls
    /// between two points (`audio::spatial`'s occlusion). A closed shape
    /// counts when the line goes in and out of it, so one a point is inside
    /// (the listener's own body) does not; an open one (a segment, a
    /// polyline) counts once however often it is crossed. `skip` leaves out
    /// the colliders of the two ends.
    pub fn walls_between(&self, a: Vec2, b: Vec2, skip: &[hecs::Entity]) -> usize {
        let (lo, hi) = (a.min(b), a.max(b));
        let d = b - a;
        let mut walls = 0;
        for s in &self.surfaces {
            if skip.contains(&s.owner) || s.max.x < lo.x || s.min.x > hi.x || s.max.y < lo.y || s.min.y > hi.y {
                continue;
            }
            match &s.outline {
                Outline::Circle { centre, radius } => {
                    let inside = |p: Vec2| (p - *centre).length_squared() < radius * radius;
                    if inside(a) || inside(b) {
                        continue;
                    }
                    let t = ((*centre - a).dot(d) / d.length_squared().max(1e-9)).clamp(0.0, 1.0);
                    if (a + d * t - *centre).length_squared() < radius * radius {
                        walls += 1;
                    }
                }
                Outline::Edges(edges) => {
                    let mut crossings = 0;
                    let mut closed = false;
                    for e in edges {
                        closed |= e.normal != Vec2::ZERO;
                        let r = e.b - e.a;
                        let den = cross(d, r);
                        if den.abs() < 1e-9 {
                            continue;
                        }
                        let t = cross(e.a - a, r) / den;
                        let u = cross(e.a - a, d) / den;
                        if (0.0..=1.0).contains(&t) && (0.0..1.0).contains(&u) {
                            crossings += 1;
                        }
                    }
                    if (closed && crossings >= 2) || (!closed && crossings >= 1) {
                        walls += 1;
                    }
                }
            }
        }
        walls
    }

    /// Every edge of the surfaces within `radius` of `centre` - a circle as a
    /// sixteen-sided loop - but those `skip` owns: what a `vision` looks
    /// past (`ecs::vision`).
    pub fn edges_near(&self, centre: Vec2, radius: f32, skip: &[hecs::Entity]) -> Vec<(Vec2, Vec2)> {
        let mut out = Vec::new();
        for s in &self.surfaces {
            if skip.contains(&s.owner)
                || s.max.x < centre.x - radius
                || s.min.x > centre.x + radius
                || s.max.y < centre.y - radius
                || s.min.y > centre.y + radius
            {
                continue;
            }
            match &s.outline {
                Outline::Edges(edges) => out.extend(edges.iter().map(|e| (e.a, e.b))),
                Outline::Circle { centre: c, radius: r } => {
                    let p = |k: usize| *c + Vec2::from_angle(k as f32 / 16.0 * std::f32::consts::TAU) * *r;
                    out.extend((0..16).map(|k| (p(k), p(k + 1))));
                }
            }
        }
        out
    }

    /// How far a ray from `from` along `dir` goes before it meets a surface,
    /// within `reach` (`None`: nothing in reach). Every surface counts.
    pub fn ray_reach(&self, from: Vec2, dir: Vec2, reach: f32) -> Option<f32> {
        let to = from + dir.normalize_or_zero() * reach;
        self.first_hit(from, to, u16::MAX).map(|h| h.t * reach)
    }

    /// The first surface in `mask` that the step `from` -> `to` crosses.
    pub fn first_hit(&self, from: Vec2, to: Vec2, mask: u16) -> Option<SurfaceHit> {
        let (lo, hi) = (from.min(to), from.max(to));
        let r = to - from;
        let mut best: Option<SurfaceHit> = None;
        for s in &self.surfaces {
            if s.membership & mask == 0 || hi.x < s.min.x || lo.x > s.max.x || hi.y < s.min.y || lo.y > s.max.y {
                continue;
            }
            let hit = match &s.outline {
                Outline::Circle { centre, radius } => circle_hit(from, r, *centre, *radius),
                Outline::Edges(edges) => edges.iter().filter_map(|e| edge_hit(from, r, e)).min_by(|a, b| a.t.total_cmp(&b.t)),
            };
            if let Some(h) = hit
                && best.is_none_or(|b| h.t < b.t)
            {
                best = Some(h);
            }
        }
        best
    }
}

fn edge_hit(p: Vec2, r: Vec2, e: &Edge) -> Option<SurfaceHit> {
    let s = e.b - e.a;
    let denom = cross(r, s);
    if denom.abs() < 1e-9 {
        return None;
    }
    let ap = e.a - p;
    let t = cross(ap, s) / denom;
    let u = cross(ap, r) / denom;
    if !(0.0..=1.0).contains(&t) || !(0.0..=1.0).contains(&u) {
        return None;
    }
    let normal = if e.normal != Vec2::ZERO {
        // A closed shape is entered, never left: a particle inside goes out.
        if r.dot(e.normal) >= 0.0 {
            return None;
        }
        e.normal
    } else {
        let n = Vec2::new(-s.y, s.x).normalize_or_zero();
        if n.dot(p - e.a) < 0.0 { -n } else { n }
    };
    Some(SurfaceHit { t, point: p + r * t, normal })
}

fn circle_hit(p: Vec2, r: Vec2, c: Vec2, radius: f32) -> Option<SurfaceHit> {
    let f = p - c;
    if f.length_squared() <= radius * radius {
        return None; // inside already: let it out
    }
    let a = r.length_squared();
    if a < 1e-12 {
        return None;
    }
    let b = 2.0 * f.dot(r);
    let k = f.length_squared() - radius * radius;
    let disc = b * b - 4.0 * a * k;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / (2.0 * a);
    if !(0.0..=1.0).contains(&t) {
        return None;
    }
    let point = p + r * t;
    Some(SurfaceHit { t, point, normal: (point - c).normalize_or_zero() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collider(shape: ColliderShape) -> Collider2D {
        serde_json::from_value(serde_json::json!({ "shape": serde_json::to_value(shape).unwrap() })).unwrap()
    }

    fn world_with(t: Transform, shape: ColliderShape) -> World {
        let mut w = World::new();
        w.spawn((t, collider(shape)));
        w
    }

    fn at(x: f32, y: f32, rot: f32) -> Transform {
        Transform { position: Vec2::new(x, y), rotation: rot, ..Default::default() }
    }

    /// A colliding tilemap layer is a wall to every outline reader - a
    /// `vision` fan used to pass straight through one (Blackout).
    #[cfg(feature = "physics")]
    #[test]
    fn a_tilemap_wall_is_an_outline() {
        use crate::tilemap::{Tilemap, TilemapLayer};
        // 4 x 3 cells of 32, a wall down column 2; the map hangs from its
        // top-left corner at (0, 0) and rows run down.
        let mut tiles = vec![0u32; 12];
        for r in 0..3 {
            tiles[r * 4 + 2] = 1;
        }
        let layer = TilemapLayer { name: "walls".into(), tiles, width: 4, height: 3, collision: true, ..Default::default() };
        let tm = Tilemap { tile_size: [32.0, 32.0], layers: vec![layer], ..Default::default() };
        let mut w = World::new();
        let map = w.spawn((at(0.0, 0.0, 0.0), tm));
        let s = ColliderOutlines::gather(&w);
        assert!(!s.is_empty(), "the wall layer gave no outline");
        let h = s.first_hit(Vec2::new(16.0, -48.0), Vec2::new(120.0, -48.0), u16::MAX).expect("the ray meets the wall");
        assert!((h.point.x - 64.0).abs() < 1e-3, "{h:?}");
        assert_eq!(s.walls_between(Vec2::new(16.0, -48.0), Vec2::new(120.0, -48.0), &[]), 1);
        assert!(s.edges_near(Vec2::new(80.0, -48.0), 20.0, &[map]).is_empty(), "skipping the map skips its walls");
        assert!(s.first_hit(Vec2::new(16.0, -48.0), Vec2::new(48.0, -48.0), u16::MAX).is_none(), "short of the wall");
    }

    #[test]
    fn a_falling_particle_lands_on_top_of_a_box() {
        let s = ColliderOutlines::gather(&world_with(at(0.0, 0.0, 0.0), ColliderShape::Box { width: 100.0, height: 20.0 }));
        let h = s.first_hit(Vec2::new(10.0, 30.0), Vec2::new(10.0, 0.0), u16::MAX).expect("hits the top");
        assert!((h.point.y - 10.0).abs() < 1e-3 && h.normal.y > 0.99, "{h:?}");
        // Born inside, it leaves freely.
        assert!(s.first_hit(Vec2::new(0.0, 0.0), Vec2::new(0.0, 50.0), u16::MAX).is_none());
    }

    #[test]
    fn a_turned_box_turns_its_faces() {
        let s = ColliderOutlines::gather(&world_with(
            at(0.0, 0.0, std::f32::consts::FRAC_PI_4),
            ColliderShape::Box { width: 40.0, height: 40.0 },
        ));
        let h = s.first_hit(Vec2::new(5.0, 60.0), Vec2::new(5.0, 0.0), u16::MAX).expect("hits a slanted face");
        assert!(h.normal.x > 0.6 && h.normal.y > 0.6, "{h:?}");
    }

    #[test]
    fn open_surfaces_are_hit_from_either_side_and_circles_from_outside() {
        let s = ColliderOutlines::gather(&world_with(
            at(0.0, 0.0, 0.0),
            ColliderShape::Segment { a: [-50.0, 0.0], b: [50.0, 0.0] },
        ));
        assert!(s.first_hit(Vec2::new(0.0, 5.0), Vec2::new(0.0, -5.0), u16::MAX).unwrap().normal.y > 0.99);
        assert!(s.first_hit(Vec2::new(0.0, -5.0), Vec2::new(0.0, 5.0), u16::MAX).unwrap().normal.y < -0.99);
        let c = ColliderOutlines::gather(&world_with(at(100.0, 0.0, 0.0), ColliderShape::Circle { radius: 10.0 }));
        let h = c.first_hit(Vec2::new(80.0, 0.0), Vec2::new(100.0, 0.0), u16::MAX).unwrap();
        assert!((h.point.x - 90.0).abs() < 1e-3 && h.normal.x < -0.99);
        assert!(c.first_hit(Vec2::new(100.0, 0.0), Vec2::new(130.0, 0.0), u16::MAX).is_none());
    }

    #[test]
    fn sensors_and_masked_layers_are_not_surfaces() {
        let mut w = World::new();
        let mut sensor = collider(ColliderShape::Box { width: 100.0, height: 100.0 });
        sensor.is_sensor = true;
        w.spawn((at(0.0, 0.0, 0.0), sensor));
        let mut layered = collider(ColliderShape::Box { width: 100.0, height: 100.0 });
        layered.collision_membership = 0b10;
        w.spawn((at(300.0, 0.0, 0.0), layered));
        let s = ColliderOutlines::gather(&w);
        assert_eq!(s.len(), 1);
        assert!(s.first_hit(Vec2::new(300.0, 80.0), Vec2::new(300.0, 0.0), 0b01).is_none());
        assert!(s.first_hit(Vec2::new(300.0, 80.0), Vec2::new(300.0, 0.0), 0b10).is_some());
    }
}
