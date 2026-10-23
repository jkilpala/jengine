//! Navigation off the grid: a `nav_region` whose walkable area is baked from
//! the colliders inside it, and `nav_agent`s that walk it (Oct 2026; Godot's
//! `NavigationRegion2D` / `NavigationAgent2D`, Unity's NavMesh).
//!
//! A tilemap has always had a path (`Tilemap.find_path`), but only from cell
//! to cell; a level of placed things - a fountain, benches turned at an
//! angle, a cart - had none, so every game that wanted people to walk round
//! them steered by hand and got stuck on the first concave corner.
//!
//! # What is baked
//!
//! Every non-sensor `collider` inside the region (not an agent's own, and not
//! a dynamic body's unless it is a [`NavObstacle`]) and every solid layer of
//! a colliding tilemap is **grown by the agent
//! radius** - its convex outline swept by a disc, the arcs drawn as tangent
//! segments so the grown shape is never smaller than the true one - and the
//! region's outline is shrunk by the same radius. What is left is where the
//! *centre* of an agent can stand, and the shortest way across it bends only
//! at the corners of those grown shapes. So the bake is a **visibility
//! graph** over those corners rather than a triangle mesh: exact shortest
//! paths, overlapping obstacles for free (a corner inside another shape is
//! simply not a corner), and nothing to go wrong in polygon clipping. It is
//! rebaked when anything it was made of moves, and only then
//! ([`NavGraph::key`]).
//!
//! A concave collider (`convex_decomposition`) counts as its hull. A
//! tilemap's walls count as the physics builds them (`tile_area_colliders`:
//! whole cells merged into runs, shaped tiles as their outlines), cached per
//! map and keyed by its layers' revisions, so a still map costs nothing a
//! frame and a dug-out wall rebakes the region. (Until Oct 2026 they did not
//! count at all, and an agent walked straight through a tile wall.)
//!
//! # Moving obstacles
//!
//! A dynamic body is left out - it would rebake every frame it moved -
//! unless it carries a [`NavObstacle`]: then it is baked in, keyed by its
//! position snapped to `snap` units (and its turn to a tenth of a quarter
//! turn), so its region rebakes when it has really moved, not when it
//! jitters, and every agent whose path was planned on the old graph
//! replans. A crate pushed across a corridor closes it; pushed aside, it
//! opens again.
//!
//! # Agents
//!
//! An agent given a target plans a path through its region's graph, walks
//! it at `speed`, and keeps out of other agents' way: it sidesteps one ahead
//! of it (both to their right, so two meeting head on pass), is pushed apart
//! from any it overlaps, and replans when it has been pushed somewhere its
//! next corner cannot be seen from. An agent that reaches a crowd standing
//! at its goal stops at the crowd's edge. All of it is one step,
//! [`tick_navigation`], inside `behaviors::tick_motion`, so no host is wired.

use std::f32::consts::PI;
use std::sync::Arc;

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{BodyType, Collider2D, ColliderShape, FaceVelocity, RigidBody2D, Transform};

/// A point this far inside a grown shape is inside it; on its edge is not.
const EPS: f32 = 0.01;
/// The widest arc one segment of a grown corner may stand for.
const ARC_STEP: f32 = PI / 4.0;

fn default_outline() -> Vec<[f32; 2]> {
    vec![[-640.0, -360.0], [640.0, -360.0], [640.0, 360.0], [-640.0, 360.0]]
}
fn default_agent_radius() -> f32 {
    16.0
}
fn default_mask() -> u16 {
    0xFFFF
}
fn default_speed() -> f32 {
    120.0
}
fn default_arrive() -> f32 {
    4.0
}
fn yes() -> bool {
    true
}
fn default_snap() -> f32 {
    16.0
}

/// `nav_obstacle`: a moving thing the regions bake round - a crate pushed
/// into an aisle, a robot standing still, a door that slides (Oct 2026;
/// Unity's `NavMeshObstacle` with carving). Its collider is baked in like a
/// static one's, keyed by its position snapped to `snap` units, so it
/// rebakes its region when it has moved that far and agents replan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavObstacle {
    #[serde(default = "default_snap")]
    pub snap: f32,
}

impl Default for NavObstacle {
    fn default() -> Self {
        Self { snap: default_snap() }
    }
}

/// `nav_region`: the walkable area inside `outline` (local points, turned and
/// moved with the entity), less every collider in `obstacle_mask` grown by
/// `agent_radius`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavRegion {
    #[serde(default = "default_outline")]
    pub outline: Vec<[f32; 2]>,
    /// How wide the agents that walk it are: obstacles grow by it and the
    /// outline shrinks by it, so a path keeps an agent's whole body clear.
    #[serde(default = "default_agent_radius")]
    pub agent_radius: f32,
    /// Which colliders are obstacles, against their `collision_membership`.
    #[serde(default = "default_mask")]
    pub obstacle_mask: u16,
    /// The graph, baked by [`tick_navigation`] and kept until what it was
    /// made of changes. Never saved.
    #[serde(skip)]
    pub baked: Option<Arc<NavGraph>>,
}

impl Default for NavRegion {
    fn default() -> Self {
        Self { outline: default_outline(), agent_radius: default_agent_radius(), obstacle_mask: default_mask(), baked: None }
    }
}

/// `nav_agent`: walks to `target` through the region it stands in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavAgent {
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// Its body, for keeping out of other agents' way. Obstacles are kept
    /// clear by the region's `agent_radius`.
    #[serde(default = "default_agent_radius")]
    pub radius: f32,
    /// Where it is going; `None` when it has arrived or was stopped.
    #[serde(default)]
    pub target: Option<[f32; 2]>,
    #[serde(default = "default_arrive")]
    pub arrive_distance: f32,
    /// Sidestep and push apart from other agents.
    #[serde(default = "yes")]
    pub avoidance: bool,
    /// The corners still to walk to, the last one the goal. Never saved.
    #[serde(skip)]
    pub path: Vec<[f32; 2]>,
    /// The target and graph the path was planned for; either changing
    /// replans it.
    #[serde(skip)]
    pub planned: Option<([f32; 2], u64)>,
    /// How it moved last frame, in units a second. Never saved.
    #[serde(skip)]
    pub velocity: [f32; 2],
    /// Where its last trip ends (the target, snapped to where it can stand),
    /// kept after it arrives: one arriving at a crowd stops at its edge only
    /// if the crowd was going where it is going. Never saved.
    #[serde(skip)]
    pub goal: Option<[f32; 2]>,
    /// How each corner of `path` is reached: walked
    /// (`ecs::nav_links::WALK`), or across that link. Never saved.
    #[serde(skip)]
    pub ways: Vec<u32>,
    /// A link being crossed (`ecs::nav_links`). Never saved.
    #[serde(skip)]
    pub crossing: Option<crate::ecs::nav_links::Crossing>,
}

impl Default for NavAgent {
    fn default() -> Self {
        Self {
            speed: default_speed(),
            radius: default_agent_radius(),
            target: None,
            arrive_distance: default_arrive(),
            avoidance: true,
            path: Vec::new(),
            planned: None,
            velocity: [0.0, 0.0],
            goal: None,
            ways: Vec::new(),
            crossing: None,
        }
    }
}

impl NavAgent {
    /// Still walking somewhere, or crossing a link.
    pub fn is_moving(&self) -> bool {
        self.target.is_some() || self.crossing.is_some()
    }
}

/// One collider grown by the agent radius: a convex loop, anticlockwise.
#[derive(Debug, Clone)]
struct Obstacle {
    pts: Vec<Vec2>,
    /// Outward normal of the edge from `pts[i]` to `pts[i + 1]`.
    normals: Vec<Vec2>,
    min: Vec2,
    max: Vec2,
}

impl Obstacle {
    fn new(pts: Vec<Vec2>) -> Option<Self> {
        let n = pts.len();
        if n < 3 {
            return None;
        }
        let normals = (0..n)
            .map(|i| {
                let d = pts[(i + 1) % n] - pts[i];
                Vec2::new(d.y, -d.x).normalize_or_zero()
            })
            .collect();
        let (min, max) = bounds(&pts);
        Some(Self { pts, normals, min, max })
    }

    /// Strictly inside - more than [`EPS`] from every edge.
    fn contains(&self, p: Vec2) -> bool {
        if p.x <= self.min.x || p.y <= self.min.y || p.x >= self.max.x || p.y >= self.max.y {
            return false;
        }
        self.pts.iter().zip(&self.normals).all(|(&a, &n)| n.dot(p - a) < -EPS)
    }

    /// Whether the segment passes through the inside (Cyrus-Beck against
    /// the loop shrunk by [`EPS`], so running along an edge or touching a
    /// corner does not count).
    fn blocks(&self, a: Vec2, b: Vec2) -> bool {
        let (lo, hi) = (a.min(b), a.max(b));
        if hi.x <= self.min.x || hi.y <= self.min.y || lo.x >= self.max.x || lo.y >= self.max.y {
            return false;
        }
        let d = b - a;
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for (&p, &n) in self.pts.iter().zip(&self.normals) {
            let num = n.dot(a - p) + EPS;
            let den = n.dot(d);
            if den.abs() < 1e-9 {
                if num >= 0.0 {
                    return false;
                }
                continue;
            }
            let t = -num / den;
            if den < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 >= t1 {
                return false;
            }
        }
        t1 - t0 > 1e-6
    }
}

fn bounds(pts: &[Vec2]) -> (Vec2, Vec2) {
    pts.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), &p| (lo.min(p), hi.max(p)))
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

fn signed_area(pts: &[Vec2]) -> f32 {
    let n = pts.len();
    (0..n).map(|i| cross(pts[i], pts[(i + 1) % n])).sum::<f32>() * 0.5
}

/// Anticlockwise hull (monotone chain).
fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut p: Vec<Vec2> = points.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup_by(|a, b| a.distance_squared(*b) < 1e-8);
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

/// A convex shape (one, two or more points, anticlockwise) swept by a disc
/// of radius `r`. Each corner's arc is drawn as segments **tangent** to it
/// (their points at `r / cos(half a segment's turn)`), so the result
/// contains the true sweep and an agent following its edges never touches
/// the shape.
fn grow(hull: &[Vec2], r: f32) -> Vec<Vec2> {
    let n = hull.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return disc(hull[0], r);
    }
    let mut out = Vec::with_capacity(n * 3);
    for i in 0..n {
        let (prev, v, next) = (hull[(i + n - 1) % n], hull[i], hull[(i + 1) % n]);
        let d0 = v - prev;
        let d1 = next - v;
        let n0 = Vec2::new(d0.y, -d0.x).normalize_or_zero();
        let n1 = Vec2::new(d1.y, -d1.x).normalize_or_zero();
        let a0 = n0.y.atan2(n0.x);
        let mut turn = n1.y.atan2(n1.x) - a0;
        while turn < 0.0 {
            turn += 2.0 * PI;
        }
        if turn > 2.0 * PI - 1e-4 {
            turn = 0.0;
        }
        let steps = ((turn / ARC_STEP - 1e-3).ceil() as usize).max(1);
        let half = turn / steps as f32 * 0.5;
        let rr = r / half.cos();
        for k in 0..=steps {
            let a = a0 + turn * k as f32 / steps as f32;
            out.push(v + rr * Vec2::new(a.cos(), a.sin()));
        }
    }
    out.dedup_by(|a, b| a.distance_squared(*b) < 1e-6);
    if out.len() > 1 && out[0].distance_squared(out[out.len() - 1]) < 1e-6 {
        out.pop();
    }
    out
}

/// A disc as a tangent polygon of twelve sides.
fn disc(c: Vec2, r: f32) -> Vec<Vec2> {
    const SIDES: usize = 12;
    let rr = r / (PI / SIDES as f32).cos();
    (0..SIDES)
        .map(|k| {
            let a = 2.0 * PI * k as f32 / SIDES as f32;
            c + rr * Vec2::new(a.cos(), a.sin())
        })
        .collect()
}

/// What an obstacle is before it is grown.
#[derive(Debug, Clone)]
enum Shape {
    Hull(Vec<Vec2>),
    Disc(Vec2, f32),
}

impl Shape {
    fn bounds(&self) -> (Vec2, Vec2) {
        match self {
            Shape::Hull(p) => bounds(p),
            Shape::Disc(c, r) => (*c - Vec2::splat(*r), *c + Vec2::splat(*r)),
        }
    }

    fn grown(&self, r: f32) -> Vec<Vec2> {
        match self {
            Shape::Hull(p) => grow(p, r),
            Shape::Disc(c, rad) => disc(*c, rad + r),
        }
    }

    fn hash_into(&self, h: &mut Fnv) {
        match self {
            Shape::Hull(p) => {
                h.write(1);
                for q in p {
                    h.write_vec(*q);
                }
            }
            Shape::Disc(c, r) => {
                h.write(2);
                h.write_vec(*c);
                h.write(r.to_bits() as u64);
            }
        }
    }
}

/// A collider as convex shapes in world space - the same shapes physics
/// builds, placed by the entity's transform and offset (no scale, as rapier
/// has none). An open shape is one stadium per segment.
fn collider_shapes(t: &Transform, col: &Collider2D) -> Vec<Shape> {
    let rot = glam::Mat2::from_angle(t.rotation);
    let offset = Vec2::from(col.offset);
    let place = |p: Vec2| t.position + rot * (p + offset);
    let pts = |v: &[[f32; 2]]| -> Vec<Vec2> { v.iter().map(|p| place(Vec2::from(*p))).collect() };
    let segments = |line: Vec<Vec2>| -> Vec<Shape> { line.windows(2).map(|w| Shape::Hull(vec![w[0], w[1]])).collect() };
    match &col.shape {
        ColliderShape::Box { width, height } => {
            let (hw, hh) = (width / 2.0, height / 2.0);
            vec![Shape::Hull(convex_hull(&[
                place(Vec2::new(-hw, -hh)),
                place(Vec2::new(hw, -hh)),
                place(Vec2::new(hw, hh)),
                place(Vec2::new(-hw, hh)),
            ]))]
        }
        ColliderShape::Circle { radius } => vec![Shape::Disc(place(Vec2::ZERO), *radius)],
        // A capsule is its spine swept by its radius (`capsule_y`).
        ColliderShape::Capsule { half_height, radius } => vec![Shape::Hull(grow(
            &[place(Vec2::new(0.0, -*half_height)), place(Vec2::new(0.0, *half_height))],
            *radius,
        ))],
        ColliderShape::Segment { a, b } => segments(pts(&[*a, *b])),
        ColliderShape::Triangle { a, b, c } => vec![Shape::Hull(convex_hull(&pts(&[*a, *b, *c])))],
        ColliderShape::ConvexPolygon { vertices } => vec![Shape::Hull(convex_hull(&pts(vertices)))],
        // Each loop as its hull.
        ColliderShape::ConvexDecomposition { vertices, loops } => crate::ecs::components::decomposition_loops(vertices, loops)
            .into_iter()
            .map(|l| Shape::Hull(convex_hull(&pts(l))))
            .collect(),
        ColliderShape::Polyline { vertices } => segments(pts(vertices)),
        ColliderShape::HeightField { heights, scale } => {
            let n = heights.len();
            if n < 2 {
                return Vec::new();
            }
            segments(
                heights
                    .iter()
                    .enumerate()
                    .map(|(i, h)| place(Vec2::new((i as f32 / (n - 1) as f32 - 0.5) * scale[0], h * scale[1])))
                    .collect(),
            )
        }
    }
}

/// FNV-1a over 64-bit words: the bake's key.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn write(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn write_vec(&mut self, v: Vec2) {
        self.write(v.x.to_bits() as u64 | ((v.y.to_bits() as u64) << 32));
    }
}

/// A region's baked walkable area: the shrunk outline, the grown obstacles,
/// and the visibility graph over their corners.
#[derive(Debug, Clone, Default)]
pub struct NavGraph {
    key: u64,
    region: Vec<Vec2>,
    rmin: Vec2,
    rmax: Vec2,
    obstacles: Vec<Obstacle>,
    nodes: Vec<Vec2>,
    links: Vec<Vec<(u32, f32)>>,
}

/// One region's inputs, gathered from the world.
struct RegionInputs {
    outline: Vec<Vec2>,
    radius: f32,
    shapes: Vec<Shape>,
    key: u64,
}

impl NavGraph {
    /// What the graph was baked from; a different key means it is stale.
    pub fn key(&self) -> u64 {
        self.key
    }

    /// The corners paths may bend at.
    pub fn nodes(&self) -> &[Vec2] {
        &self.nodes
    }

    /// Each node's visible neighbours and the distance to them.
    pub fn links(&self) -> &[Vec<(u32, f32)>] {
        &self.links
    }

    /// The walkable outline (the region shrunk by the agent radius).
    pub fn outline(&self) -> &[Vec2] {
        &self.region
    }

    /// Every grown obstacle's loop.
    pub fn obstacle_loops(&self) -> impl Iterator<Item = &[Vec2]> {
        self.obstacles.iter().map(|o| o.pts.as_slice())
    }

    /// Bake from an outline (anticlockwise or not) and obstacles, all in
    /// world space.
    fn bake(inputs: &RegionInputs) -> Self {
        let mut outline = inputs.outline.clone();
        if signed_area(&outline) < 0.0 {
            outline.reverse();
        }
        let r = inputs.radius.max(0.0);
        let n = outline.len();
        // Shrink the outline: each corner moved in along its mitre, which is
        // where the two edges' offset lines meet.
        let mut region = Vec::with_capacity(n);
        let mut reflex = Vec::new();
        for i in 0..n {
            let (prev, v, next) = (outline[(i + n - 1) % n], outline[i], outline[(i + 1) % n]);
            let d0 = (v - prev).normalize_or_zero();
            let d1 = (next - v).normalize_or_zero();
            let n0 = Vec2::new(-d0.y, d0.x);
            let n1 = Vec2::new(-d1.y, d1.x);
            let m = n0 + n1;
            let denom = (1.0 + n0.dot(n1)).max(0.1);
            region.push(v + m * (r / denom));
            if cross(d0, d1) < -1e-6 {
                reflex.push(i);
            }
        }
        let (rmin, rmax) = if region.is_empty() { (Vec2::ZERO, Vec2::ZERO) } else { bounds(&region) };
        let obstacles: Vec<Obstacle> = inputs
            .shapes
            .iter()
            .filter_map(|s| Obstacle::new(convex_hull(&s.grown(r))))
            .collect();
        let mut graph = Self { key: inputs.key, region, rmin, rmax, obstacles, nodes: Vec::new(), links: Vec::new() };
        let mut nodes: Vec<Vec2> = reflex.iter().map(|&i| graph.region[i]).collect();
        for o in &graph.obstacles {
            nodes.extend(o.pts.iter().copied());
        }
        nodes.retain(|&p| graph.is_walkable(p));
        let count = nodes.len();
        if count > 2000 {
            log::warn!("nav_region: {count} corners to bake - a visibility graph grows with their square");
        }
        let mut links = vec![Vec::new(); count];
        for i in 0..count {
            for j in i + 1..count {
                if graph.clear(nodes[i], nodes[j]) {
                    let d = nodes[i].distance(nodes[j]);
                    links[i].push((j as u32, d));
                    links[j].push((i as u32, d));
                }
            }
        }
        graph.nodes = nodes;
        graph.links = links;
        graph
    }

    fn in_region(&self, p: Vec2) -> bool {
        let n = self.region.len();
        if n < 3 || p.x < self.rmin.x - EPS || p.y < self.rmin.y - EPS || p.x > self.rmax.x + EPS || p.y > self.rmax.y + EPS {
            return false;
        }
        let mut inside = false;
        let mut near = f32::MAX;
        for i in 0..n {
            let (a, b) = (self.region[i], self.region[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
            near = near.min(point_segment(p, a, b).distance_squared(p));
        }
        inside || near < 0.05 * 0.05
    }

    /// Whether an agent's centre may stand at `p`.
    pub fn is_walkable(&self, p: Vec2) -> bool {
        self.in_region(p) && !self.obstacles.iter().any(|o| o.contains(p))
    }

    /// Whether an agent can walk straight from `a` to `b`.
    pub fn clear(&self, a: Vec2, b: Vec2) -> bool {
        if self.obstacles.iter().any(|o| o.blocks(a, b)) {
            return false;
        }
        let n = self.region.len();
        let len = (b - a).length().max(1e-6);
        let tol = 1e-3 * len;
        for i in 0..n {
            let (c, d) = (self.region[i], self.region[(i + 1) % n]);
            let o1 = cross(b - a, c - a);
            let o2 = cross(b - a, d - a);
            let el = (d - c).length().max(1e-6);
            let o3 = cross(d - c, a - c);
            let o4 = cross(d - c, b - c);
            if ((o1 > tol && o2 < -tol) || (o1 < -tol && o2 > tol)) && ((o3 > 1e-3 * el && o4 < -1e-3 * el) || (o3 < -1e-3 * el && o4 > 1e-3 * el)) {
                return false;
            }
        }
        self.in_region((a + b) * 0.5)
    }

    /// The walkable point nearest `p`: `p` itself if it is walkable, else
    /// the nearest point just outside the obstacle (or just inside the
    /// outline) it fell in. `None` only for a region with nowhere to stand.
    pub fn closest_walkable(&self, p: Vec2) -> Option<Vec2> {
        if self.is_walkable(p) {
            return Some(p);
        }
        let mut best: Option<(f32, Vec2)> = None;
        let consider = |q: Vec2, best: &mut Option<(f32, Vec2)>| {
            let d = q.distance_squared(p);
            if best.is_none_or(|(bd, _)| d < bd) && self.is_walkable(q) {
                *best = Some((d, q));
            }
        };
        for o in &self.obstacles {
            let n = o.pts.len();
            for i in 0..n {
                let q = point_segment(p, o.pts[i], o.pts[(i + 1) % n]) + o.normals[i] * (EPS * 4.0);
                consider(q, &mut best);
            }
        }
        let n = self.region.len();
        for i in 0..n {
            let (a, b) = (self.region[i], self.region[(i + 1) % n]);
            let d = (b - a).normalize_or_zero();
            let q = point_segment(p, a, b) + Vec2::new(-d.y, d.x) * (EPS * 4.0);
            consider(q, &mut best);
        }
        for &q in &self.nodes {
            consider(q, &mut best);
        }
        best.map(|(_, q)| q)
    }

    /// The shortest walkable way from `from` to `to`, both ends included -
    /// each snapped to the nearest walkable point first, so a click on a
    /// fountain walks to its rim. `None` when the two are not connected.
    pub fn find_path(&self, from: Vec2, to: Vec2) -> Option<Vec<Vec2>> {
        let s = self.closest_walkable(from)?;
        let g = self.closest_walkable(to)?;
        if self.clear(s, g) {
            return Some(vec![s, g]);
        }
        let n = self.nodes.len();
        let goal_link: Vec<Option<f32>> =
            self.nodes.iter().map(|&q| self.clear(q, g).then(|| q.distance(g))).collect();
        // A* with the start and goal as nodes `n` and `n + 1`.
        let goal = n + 1;
        let mut cost = vec![f32::MAX; n + 2];
        let mut came = vec![usize::MAX; n + 2];
        let mut open = std::collections::BinaryHeap::new();
        cost[n] = 0.0;
        for (j, &q) in self.nodes.iter().enumerate() {
            if self.clear(s, q) {
                let c = s.distance(q);
                cost[j] = c;
                came[j] = n;
                open.push(Open(c + q.distance(g), j));
            }
        }
        while let Some(Open(f, i)) = open.pop() {
            if i == goal {
                break;
            }
            let here = self.nodes[i];
            if f > cost[i] + here.distance(g) + 1e-3 {
                continue;
            }
            let mut relax = |j: usize, step: f32, h: f32| {
                let c = cost[i] + step;
                if c < cost[j] {
                    cost[j] = c;
                    came[j] = i;
                    open.push(Open(c + h, j));
                }
            };
            if let Some(d) = goal_link[i] {
                relax(goal, d, 0.0);
            }
            for &(j, d) in &self.links[i] {
                let j = j as usize;
                relax(j, d, self.nodes[j].distance(g));
            }
        }
        if came[goal] == usize::MAX {
            return None;
        }
        let mut path = vec![g];
        let mut i = came[goal];
        while i != n {
            path.push(self.nodes[i]);
            i = came[i];
        }
        path.push(s);
        path.reverse();
        Some(path)
    }
}

/// An open A* entry, smallest `f` first.
struct Open(f32, usize);

impl PartialEq for Open {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
impl Eq for Open {}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

fn point_segment(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let d = b - a;
    let l = d.length_squared();
    if l < 1e-12 {
        return a;
    }
    a + d * ((p - a).dot(d) / l).clamp(0.0, 1.0)
}

/// An obstacle's shape, the collision membership a region's mask is tested
/// against, and - for a moving obstacle or a tilemap - the key it adds to the
/// bake's in place of its exact shape.
type Obstacles = Vec<(Shape, u16, Option<u64>)>;

/// Every collider that can be an obstacle: not a sensor, not an agent's,
/// not a dynamic body's unless it is a [`NavObstacle`] - and every solid
/// tilemap layer.
fn obstacle_shapes(world: &World) -> Obstacles {
    let mut out = Vec::new();
    for (e, (t, col, body, agent, moving)) in world
        .query::<(&Transform, &Collider2D, Option<&RigidBody2D>, Option<&NavAgent>, Option<&NavObstacle>)>()
        .iter()
    {
        if col.is_sensor || agent.is_some() {
            continue;
        }
        if moving.is_none() && body.is_some_and(|b| b.body_type == BodyType::Dynamic) {
            continue;
        }
        // A moving obstacle is keyed where it stands to the nearest `snap`.
        let key = moving.map(|m| {
            let snap = m.snap.max(0.5);
            let mut h = Fnv::new();
            h.write(e.to_bits().get());
            h.write((t.position.x / snap).round() as i64 as u64);
            h.write((t.position.y / snap).round() as i64 as u64);
            h.write((t.rotation / (PI / 20.0)).round() as i64 as u64);
            h.0
        });
        for s in collider_shapes(t, col) {
            out.push((s, col.collision_membership, key));
        }
    }
    #[cfg(feature = "physics")]
    tilemap_shapes(world, &mut out);
    out
}

/// A colliding tilemap's solid layers as obstacles - see [`tilemap_hulls`].
#[cfg(feature = "physics")]
fn tilemap_shapes(world: &World, out: &mut Obstacles) {
    for (_, key, hulls) in tilemap_hulls(world) {
        for h in hulls.iter() {
            out.push((Shape::Hull(h.clone()), 0xFFFF, Some(key)));
        }
    }
}

/// Every colliding tilemap's solid layers as convex outlines in world space:
/// the colliders the physics builds (`physics::tile_area_colliders`), placed
/// by the map's position (the physics body has no turn, so neither do
/// these), as `(map, key, hulls)`. Cached per map, keyed by its layers'
/// revisions and its grid, so a map nothing edits is converted once.
/// Navigation bakes them, and [`crate::ecs::outlines`] tests them - which is
/// what lets a `vision` fan stop at a tile wall (Blackout's could not).
#[cfg(feature = "physics")]
pub(crate) fn tilemap_hulls(world: &World) -> Vec<(hecs::Entity, u64, Arc<Vec<Vec<Vec2>>>)> {
    use crate::ecs::components::TilemapCollider;
    use crate::tilemap::Tilemap;
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static CACHE: RefCell<HashMap<u64, (u64, Arc<Vec<Vec<Vec2>>>)>> = RefCell::new(HashMap::new());
    }
    let mut result = Vec::new();
    for (e, (tm, t)) in world.query::<(&Tilemap, &Transform)>().iter() {
        let spec = world.get::<&TilemapCollider>(e).ok();
        let layers: Vec<usize> = tm
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.collision || spec.as_ref().is_some_and(|s| s.solid.contains(&l.name)))
            .map(|(i, _)| i)
            .collect();
        if layers.is_empty() || tm.tile_size[0] <= 0.0 || tm.tile_size[1] <= 0.0 {
            continue;
        }
        // Where it is in the world: a parented map moves with its parent.
        let origin = crate::tilemap::world_origin(world, e, t);
        let mut h = Fnv::new();
        h.write(e.to_bits().get());
        h.write_vec(origin);
        h.write_vec(Vec2::from(tm.tile_size));
        h.write(tm.orientation as u64);
        h.write(tm.hex_side.to_bits() as u64);
        h.write(tm.shapes_key());
        for &i in &layers {
            let l = &tm.layers[i];
            h.write(i as u64);
            h.write(l.rev.id());
            h.write(l.rev.total());
            h.write(l.width as u64 | ((l.height as u64) << 32));
        }
        let key = h.0;
        let id = e.to_bits().get();
        let shapes = CACHE.with(|c| {
            let mut c = c.borrow_mut();
            if let Some((k, s)) = c.get(&id)
                && *k == key
            {
                return Arc::clone(s);
            }
            let mut shapes = Vec::new();
            for &i in &layers {
                let l = &tm.layers[i];
                for b in crate::physics::tile_area_colliders(tm, l, (0, 0, l.width, l.height), false) {
                    let iso = b.position;
                    let local: Vec<Vec2> = if let Some(c) = b.shape.as_cuboid() {
                        let he = c.half_extents;
                        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                            .iter()
                            .map(|&(sx, sy)| {
                                let p = iso * rapier2d::na::Point2::new(sx * he.x, sy * he.y);
                                Vec2::new(p.x, p.y)
                            })
                            .collect()
                    } else if let Some(poly) = b.shape.as_convex_polygon() {
                        poly.points()
                            .iter()
                            .map(|q| {
                                let p = iso * q;
                                Vec2::new(p.x, p.y)
                            })
                            .collect()
                    } else {
                        continue;
                    };
                    let world_pts: Vec<Vec2> = local.into_iter().map(|p| origin + p).collect();
                    shapes.push(convex_hull(&world_pts));
                }
            }
            let shapes = Arc::new(shapes);
            c.insert(id, (key, Arc::clone(&shapes)));
            shapes
        });
        result.push((e, key, shapes));
    }
    result
}

fn region_inputs(region: &NavRegion, t: &Transform, obstacles: &[(Shape, u16, Option<u64>)]) -> RegionInputs {
    let rot = glam::Mat2::from_angle(t.rotation);
    let outline: Vec<Vec2> = region.outline.iter().map(|p| t.position + rot * Vec2::from(*p)).collect();
    let (lo, hi) = if outline.is_empty() { (Vec2::ZERO, Vec2::ZERO) } else { bounds(&outline) };
    let reach = region.agent_radius.max(0.0) + 1.0;
    let mut h = Fnv::new();
    h.write(region.agent_radius.to_bits() as u64);
    for p in &outline {
        h.write_vec(*p);
    }
    let mut shapes = Vec::new();
    for (s, membership, key) in obstacles {
        if membership & region.obstacle_mask == 0 {
            continue;
        }
        let (smin, smax) = s.bounds();
        if smax.x < lo.x - reach || smax.y < lo.y - reach || smin.x > hi.x + reach || smin.y > hi.y + reach {
            continue;
        }
        // A moving obstacle or a tilemap stands for its shapes with a key of
        // its own: where it is to the nearest snap, or its revisions.
        match key {
            Some(k) => h.write(*k),
            None => s.hash_into(&mut h),
        }
        shapes.push(s.clone());
    }
    RegionInputs { outline, radius: region.agent_radius, shapes, key: h.0 }
}

/// The region's graph as the world stands: its baked one if that is still
/// current, else a fresh bake (which is not kept - [`tick_navigation`] keeps
/// it). For a script asking before the first frame has ticked.
pub fn graph_for(world: &World, entity: Entity) -> Option<Arc<NavGraph>> {
    let obstacles = obstacle_shapes(world);
    let region = world.get::<&NavRegion>(entity).ok()?;
    let t = world.get::<&Transform>(entity).ok()?;
    let inputs = region_inputs(&region, &t, &obstacles);
    match &region.baked {
        Some(g) if g.key == inputs.key => Some(Arc::clone(g)),
        _ => Some(Arc::new(NavGraph::bake(&inputs))),
    }
}

/// Every region's graph, current as of now (see [`graph_for`]).
pub fn graphs(world: &World) -> Vec<Arc<NavGraph>> {
    let mut q = world.query::<&NavRegion>();
    let entities: Vec<Entity> = q.iter().map(|(e, _)| e).collect();
    drop(q);
    if entities.is_empty() {
        return Vec::new();
    }
    let obstacles = obstacle_shapes(world);
    entities
        .into_iter()
        .filter_map(|e| {
            let region = world.get::<&NavRegion>(e).ok()?;
            let t = world.get::<&Transform>(e).ok()?;
            let inputs = region_inputs(&region, &t, &obstacles);
            Some(match &region.baked {
                Some(g) if g.key == inputs.key => Arc::clone(g),
                _ => Arc::new(NavGraph::bake(&inputs)),
            })
        })
        .collect()
}

/// The graph whose walkable area holds `p` - or, failing that, whose
/// outline is nearest it.
pub fn graph_at(graphs: &[Arc<NavGraph>], p: Vec2) -> Option<&Arc<NavGraph>> {
    graphs.iter().find(|g| g.in_region(p)).or_else(|| {
        graphs.iter().min_by(|a, b| {
            let d = |g: &NavGraph| p.clamp(g.rmin, g.rmax).distance_squared(p);
            d(a).total_cmp(&d(b))
        })
    })
}

/// `Nav.find_path(x0, y0, x1, y1)` over a set of graphs.
pub fn find_path_in(graphs: &[Arc<NavGraph>], from: Vec2, to: Vec2) -> Option<Vec<Vec2>> {
    graph_at(graphs, from)?.find_path(from, to)
}

/// `Nav.closest_point(x, y)` over a set of graphs.
pub fn closest_point_in(graphs: &[Arc<NavGraph>], p: Vec2) -> Option<Vec2> {
    graph_at(graphs, p)?.closest_walkable(p)
}

/// `Nav.set_target(e, x, y)` / `Nav.stop(e)`: where the agent is going, or
/// `None` to stop it where it stands.
pub fn set_nav_target(world: &World, entity: Entity, target: Option<[f32; 2]>) -> Result<(), String> {
    let mut a = world.get::<&mut NavAgent>(entity).map_err(|_| "the entity has no nav_agent".to_string())?;
    a.target = target;
    if target.is_none() {
        a.path.clear();
        a.planned = None;
        a.velocity = [0.0, 0.0];
        a.goal = None;
    }
    Ok(())
}

/// `Nav.set(e, field, value)`: any field of the entity's `nav_agent` - or,
/// failing that, its `nav_region` - by name.
pub fn set_nav_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    fn patch<T: Serialize + for<'de> Deserialize<'de>>(c: &mut T, field: &str, value: &serde_json::Value) -> Result<bool, String> {
        let mut whole = serde_json::to_value(&*c).map_err(|e| e.to_string())?;
        let map = whole.as_object_mut().ok_or("not an object")?;
        if !map.contains_key(field) {
            return Ok(false);
        }
        map.insert(field.to_string(), value.clone());
        *c = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
        Ok(true)
    }
    if let Ok(mut a) = world.get::<&mut NavAgent>(entity) {
        let keep = (a.path.clone(), a.planned);
        if patch(&mut *a, field, value)? {
            if field != "target" {
                (a.path, a.planned) = keep;
            }
            return Ok(());
        }
    }
    if let Ok(mut r) = world.get::<&mut NavRegion>(entity) {
        let baked = r.baked.take();
        let done = patch(&mut *r, field, value);
        r.baked = baked;
        if done? {
            return Ok(());
        }
    }
    Err(format!("'{field}' is not a nav_agent or nav_region field"))
}

/// Bake what changed, then move every agent. Called by
/// `behaviors::tick_motion`, so every runtime runs it; free when the world
/// has no `nav_region`.
pub fn tick_navigation(world: &mut World, dt: f32) {
    let regions: Vec<Entity> = world.query::<&NavRegion>().iter().map(|(e, _)| e).collect();
    if regions.is_empty() {
        return;
    }
    // 1. Rebake any region whose inputs moved.
    let obstacles = obstacle_shapes(world);
    let mut graphs: Vec<Arc<NavGraph>> = Vec::with_capacity(regions.len());
    for e in regions {
        let Ok(t) = world.get::<&Transform>(e).map(|t| (*t).clone()) else { continue };
        let Ok(mut region) = world.get::<&mut NavRegion>(e) else { continue };
        let inputs = region_inputs(&region, &t, &obstacles);
        let graph = match &region.baked {
            Some(g) if g.key == inputs.key => Arc::clone(g),
            _ => {
                let g = Arc::new(NavGraph::bake(&inputs));
                region.baked = Some(Arc::clone(&g));
                g
            }
        };
        graphs.push(graph);
    }
    if dt <= 0.0 {
        return;
    }
    // 2. Agents. Links between and within regions (`ecs::nav_links`) take
    // part in planning only when there are any.
    let links = crate::ecs::nav_links::links(world);
    let links_key = crate::ecs::nav_links::key(&links);
    let mut set_off: Vec<Entity> = Vec::new();
    let mut crossing_now: Vec<(Entity, Option<(String, f32)>)> = Vec::new();
    struct Walker {
        e: Entity,
        pos: Vec2,
        radius: f32,
        moving: bool,
        goal: Option<Vec2>,
        avoid: bool,
        heading: Vec2,
        crossing: bool,
    }
    let mut walkers: Vec<Walker> = world
        .query::<(&NavAgent, &Transform)>()
        .iter()
        .map(|(e, (a, t))| Walker {
            e,
            pos: t.position,
            radius: a.radius.max(0.0),
            moving: a.is_moving(),
            goal: a.goal.map(Vec2::from),
            avoid: a.avoidance,
            heading: Vec2::from(a.velocity),
            crossing: a.crossing.is_some(),
        })
        .collect();
    if walkers.is_empty() {
        return;
    }
    let snapshot: Vec<(Vec2, f32, bool, Option<Vec2>, bool)> =
        walkers.iter().map(|w| (w.pos, w.radius, w.moving, w.goal, w.avoid)).collect();
    let headings: Vec<Vec2> = walkers.iter().map(|w| w.heading).collect();
    for (wi, w) in walkers.iter_mut().enumerate() {
        if !w.moving {
            continue;
        }
        let Ok(mut agent) = world.get::<&mut NavAgent>(w.e) else { continue };
        // Crossing a link: along it, whatever the ground, and nothing else.
        if let Some(mut c) = agent.crossing.take() {
            let (a, b) = (Vec2::from(c.from), Vec2::from(c.to));
            let dur = if c.duration > 0.0 { c.duration } else { a.distance(b) / agent.speed.max(1e-3) };
            c.t = (c.t + dt / dur.max(1e-3)).min(1.0);
            let from = w.pos;
            w.pos = a.lerp(b, c.t);
            agent.velocity = ((w.pos - from) / dt).to_array();
            crossing_now.push((w.e, (c.t < 1.0).then(|| (c.kind.clone(), c.t))));
            if c.t < 1.0 {
                agent.crossing = Some(c);
            } else {
                // Landed: plan on from here, in whatever region this is.
                agent.planned = None;
            }
            let mv = w.pos - from;
            drop(agent);
            if let Ok(face) = world.get::<&FaceVelocity>(w.e).map(|f| (*f).clone())
                && let Ok(mut t) = world.get::<&mut Transform>(w.e)
                && mv.length_squared() > 1e-8
            {
                t.rotation = face.turn(t.rotation, mv.y.atan2(mv.x), dt);
            }
            continue;
        }
        let Some(target) = agent.target else { continue };
        let Some(graph) = graph_at(&graphs, w.pos) else { continue };
        let walk_next = agent.ways.first().is_none_or(|&k| k == crate::ecs::nav_links::WALK);
        let need_plan = agent.planned != Some((target, graph.key ^ links_key))
            || agent.path.is_empty()
            || (walk_next && !graph.clear(graph.closest_walkable(w.pos).unwrap_or(w.pos), Vec2::from(agent.path[0])));
        if need_plan {
            let planned = if links.is_empty() {
                graph.find_path(w.pos, Vec2::from(target)).map(|path| {
                    let corners: Vec<Vec2> = path.into_iter().skip(1).collect();
                    let ways = vec![crate::ecs::nav_links::WALK; corners.len()];
                    (corners, ways)
                })
            } else {
                crate::ecs::nav_links::plan(&graphs, &links, w.pos, Vec2::from(target))
            };
            match planned {
                Some((corners, ways)) => {
                    agent.goal = corners.last().map(|p| p.to_array());
                    agent.path = corners.into_iter().map(|p| p.to_array()).collect();
                    agent.ways = ways;
                    agent.planned = Some((target, graph.key ^ links_key));
                }
                None => {
                    // Nowhere to go: stop rather than walk into a wall.
                    agent.target = None;
                    agent.path.clear();
                    agent.ways.clear();
                    agent.planned = None;
                    w.moving = false;
                    continue;
                }
            }
        }
        // At a link's start: set off across it.
        if agent.ways.first().is_some_and(|&k| k != crate::ecs::nav_links::WALK) && !agent.path.is_empty() {
            let k = agent.ways.remove(0) as usize;
            let to = agent.path.remove(0);
            let (duration, kind) = links.get(k).map(|l| (l.duration, l.kind.clone())).unwrap_or((0.0, "link".into()));
            agent.crossing = Some(crate::ecs::nav_links::Crossing { from: w.pos.to_array(), to, t: 0.0, duration, kind });
            set_off.push(w.e);
            continue;
        }
        let goal = agent.path.last().map(|p| Vec2::from(*p)).unwrap_or(w.pos);
        let step = agent.speed.max(0.0) * dt;
        // Arrived - or at the edge of a crowd already standing at the goal.
        // How far round its goal a crowd going there spreads: the circle
        // that many bodies fill, packed loosely. Further off than that, the
        // one in front was only queueing.
        let sharing = snapshot
            .iter()
            .filter(|&&(_, r, _, g, _)| g.is_some_and(|g| g.distance(goal) < (r + w.radius) * 0.75))
            .count()
            .max(1);
        let spread = w.radius * ((sharing as f32 / 0.7).sqrt() * 1.3 + 1.0);
        let crowded = w.avoid
            && w.pos.distance(goal) < spread
            && snapshot.iter().enumerate().any(|(j, &(p, r, moving, other_goal, _))| {
                j != wi
                    && !moving
                    && p.distance(w.pos) < (r + w.radius) * 1.1
                    && other_goal.is_some_and(|g| g.distance(goal) < (r + w.radius) * 0.75)
            });
        if w.pos.distance(goal) <= agent.arrive_distance.max(step * 0.5) || crowded {
            agent.velocity = [0.0, 0.0];
            agent.target = None;
            agent.path.clear();
            agent.ways.clear();
            agent.planned = None;
            w.moving = false;
            continue;
        }
        // Pull the path taut: a corner it has been pushed past, or can see
        // beyond, is no longer one it needs to walk to.
        // Never past a link: its far end is not a corner to see.
        while agent.path.len() > 1
            && agent.ways.iter().take(2).all(|&k| k == crate::ecs::nav_links::WALK)
            && graph.clear(w.pos, Vec2::from(agent.path[1]))
        {
            agent.path.remove(0);
            agent.ways.remove(0);
        }
        let next = Vec2::from(agent.path[0]);
        let to_next = next - w.pos;
        let dist = to_next.length();
        let dir = if dist > 1e-5 {
            to_next / dist
        } else {
            agent.path.get(1).map(|p| (Vec2::from(*p) - w.pos).normalize_or_zero()).unwrap_or(Vec2::ZERO)
        };
        // Sidestep whoever is ahead, to the side away from them (to the
        // right when dead ahead, so two meeting head on pass).
        let mut side = Vec2::ZERO;
        if w.avoid && dir != Vec2::ZERO {
            let look = w.radius * 2.0 + agent.speed * 0.6;
            for (j, &(p, r, _, _, avoids)) in snapshot.iter().enumerate() {
                if j == wi || !avoids {
                    continue;
                }
                let rel = p - w.pos;
                let ahead = rel.dot(dir);
                let reach = r + w.radius;
                if ahead <= 0.0 || ahead > look + r || rel.length() > look + r {
                    continue;
                }
                let lateral = cross(dir, rel);
                if lateral.abs() >= reach {
                    continue;
                }
                // Following someone the same way is a queue, not a meeting:
                // stepping round them only jams the corner they are taking.
                let theirs = headings[j];
                if theirs.length_squared() > 1.0 && theirs.normalize().dot(dir) > 0.5 {
                    continue;
                }
                // `lateral > 0`: they are to the left, so go right.
                let right = Vec2::new(dir.y, -dir.x);
                let away = if lateral < -1e-3 { -right } else { right };
                let weight = (1.0 - ahead / (look + r)) * (1.0 - lateral.abs() / reach).max(0.3);
                side += away * weight * 1.5;
            }
        }
        let from = w.pos;
        if side == Vec2::ZERO {
            // Walk the path itself: to the corner, through it, and on with
            // what is left of the step. Its legs are clear, so this never
            // needs checking - and never cuts a corner, which a step aimed
            // past a corner would (into the obstacle it bends round).
            let mut left = step;
            loop {
                let next = Vec2::from(agent.path[0]);
                let d = w.pos.distance(next);
                if d <= left {
                    w.pos = next;
                    left -= d;
                    if agent.path.len() > 1 {
                        agent.path.remove(0);
                        agent.ways.remove(0);
                        // Stop at a link's start: it is crossed, not walked.
                        if agent.ways.first().is_some_and(|&k| k != crate::ecs::nav_links::WALK) {
                            break;
                        }
                        continue;
                    }
                } else {
                    w.pos += (next - w.pos) / d * left;
                }
                break;
            }
        } else {
            let mut desired = (dir + side).normalize_or(dir);
            if desired.dot(dir) < 0.2 {
                // Never turn back on the path to make room.
                desired = (dir + desired).normalize_or(dir);
            }
            let mv = desired * step.min(dist.max(step * 0.5));
            let to = w.pos + mv;
            if graph.is_walkable(to) {
                w.pos = to;
            } else {
                // No room to the side: keep to the path and let the push
                // apart sort it out.
                let next = Vec2::from(agent.path[0]);
                let d = w.pos.distance(next);
                w.pos = if d <= step { next } else { w.pos + (next - w.pos) / d * step };
            }
        }
        let mv = w.pos - from;
        agent.velocity = (mv / dt).to_array();
        if mv.length_squared() < 1e-8 {
            // Boxed in: plan again next frame from wherever it now stands.
            agent.planned = None;
        }
        drop(agent);
        if let Ok(face) = world.get::<&FaceVelocity>(w.e).map(|f| (*f).clone())
            && let Ok(mut t) = world.get::<&mut Transform>(w.e)
            && mv.length_squared() > 1e-8
        {
            t.rotation = face.turn(t.rotation, mv.y.atan2(mv.x), dt);
        }
    }
    // 3. Push apart whoever overlaps: half each, or all of it onto the one
    // standing still when only one of them is walking.
    let n = walkers.len();
    if n > 1 {
        let mut shift = vec![Vec2::ZERO; n];
        for i in 0..n {
            if !walkers[i].avoid || walkers[i].crossing {
                continue;
            }
            for j in i + 1..n {
                if !walkers[j].avoid || walkers[j].crossing {
                    continue;
                }
                let d = walkers[i].pos - walkers[j].pos;
                let reach = walkers[i].radius + walkers[j].radius;
                let len = d.length();
                if len >= reach {
                    continue;
                }
                let push = if len > 1e-4 { d / len } else { Vec2::new(((i * 7 + j) % 5) as f32 - 2.0, 1.0).normalize() };
                let overlap = (reach - len) * 0.5;
                let (wi, wj) = match (walkers[i].moving, walkers[j].moving) {
                    (true, false) => (0.25, 0.75),
                    (false, true) => (0.75, 0.25),
                    _ => (0.5, 0.5),
                };
                shift[i] += push * overlap * 2.0 * wi;
                shift[j] -= push * overlap * 2.0 * wj;
            }
        }
        for (w, s) in walkers.iter_mut().zip(shift) {
            if s.length_squared() < 1e-8 {
                continue;
            }
            let Some(graph) = graph_at(&graphs, w.pos) else { continue };
            let to = w.pos + s;
            if graph.is_walkable(to) {
                w.pos = to;
            } else if let Some(q) = [w.pos + Vec2::new(s.x, 0.0), w.pos + Vec2::new(0.0, s.y)].into_iter().find(|&q| graph.is_walkable(q)) {
                w.pos = q;
            }
        }
    }
    for w in walkers {
        if let Ok(mut t) = world.get::<&mut Transform>(w.e) {
            t.position = w.pos;
        }
    }
    // What a script reads of a crossing, and the call as one sets off.
    for e in set_off {
        crate::ecs::callbacks::push(e, "on_nav_link");
    }
    for (e, now) in crossing_now {
        crate::ecs::nav_links::publish(world, e, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region_with(boxes: &[(Vec2, Vec2)], radius: f32) -> NavGraph {
        let shapes = boxes
            .iter()
            .map(|&(c, h)| Shape::Hull(convex_hull(&[c - h, Vec2::new(c.x + h.x, c.y - h.y), c + h, Vec2::new(c.x - h.x, c.y + h.y)])))
            .collect();
        NavGraph::bake(&RegionInputs {
            outline: vec![Vec2::new(-500.0, -300.0), Vec2::new(500.0, -300.0), Vec2::new(500.0, 300.0), Vec2::new(-500.0, 300.0)],
            radius,
            shapes,
            key: 7,
        })
    }

    #[test]
    fn a_grown_box_keeps_its_distance() {
        let g = grow(&convex_hull(&[Vec2::new(-10.0, -10.0), Vec2::new(10.0, -10.0), Vec2::new(10.0, 10.0), Vec2::new(-10.0, 10.0)]), 5.0);
        // Every point of the true sweep is inside the grown loop.
        let o = Obstacle::new(g).unwrap();
        for k in 0..64 {
            let a = k as f32 / 64.0 * 2.0 * PI;
            let corner = Vec2::new(10.0 * a.cos().signum(), 10.0 * a.sin().signum());
            let p = corner + Vec2::new(a.cos(), a.sin()) * 4.9;
            assert!(o.contains(p), "{p} is in the sweep and outside the grown box");
        }
    }

    #[test]
    fn a_path_goes_round_a_wall() {
        let g = region_with(&[(Vec2::ZERO, Vec2::new(20.0, 200.0))], 10.0);
        let path = g.find_path(Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)).expect("a path");
        assert!(path.len() >= 4, "it bends round both corners: {path:?}");
        for w in path.windows(2) {
            assert!(g.clear(w[0], w[1]));
        }
        let len: f32 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
        // Round the end of a wall reaching y = 200, grown by 10.
        let direct = 2.0 * Vec2::new(200.0, 210.0).length();
        assert!(len >= direct - 1.0 && len < direct + 20.0, "{len} vs {direct}");
    }

    #[test]
    fn a_closed_gap_has_no_path() {
        // Two boxes that leave a gap narrower than an agent.
        let g = region_with(&[(Vec2::new(0.0, 155.0), Vec2::new(20.0, 150.0)), (Vec2::new(0.0, -155.0), Vec2::new(20.0, 150.0))], 10.0);
        assert!(g.find_path(Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)).is_none());
        let open = region_with(&[(Vec2::new(0.0, 165.0), Vec2::new(20.0, 140.0)), (Vec2::new(0.0, -165.0), Vec2::new(20.0, 140.0))], 10.0);
        assert!(open.find_path(Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)).is_some());
    }

    #[test]
    fn a_click_inside_an_obstacle_walks_to_its_edge() {
        let g = region_with(&[(Vec2::ZERO, Vec2::new(50.0, 50.0))], 10.0);
        let q = g.closest_walkable(Vec2::new(0.0, 45.0)).unwrap();
        assert!(g.is_walkable(q));
        assert!((q.y - 60.0).abs() < 2.0 && q.x.abs() < 1.0, "{q}");
    }
}
