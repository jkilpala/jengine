//! Things that break (Sep 2026): a `breakable` body hit harder than its
//! `break_speed` comes apart into a grid of pieces - each a body of its own,
//! cut from the same sprite (a sub-rect of its uv), the same collider shape
//! scaled down, the same shadow and material - moving off with the velocity
//! the part had, plus a little scatter. A piece big enough can break again.
//!
//! The rule lives here, once; every host calls [`break_on_impact`] with the
//! contacts its physics step returned, and [`burst_debris`] to throw the
//! dust. Nothing about it is per-runtime. A `grid` breaks only a **box**
//! collider - the pieces of anything else are not a grid of anything.
//!
//! **Voronoi** (Oct 2026): `pattern: "voronoi"` breaks any convex collider
//! (a box, a triangle, a convex polygon) into `pieces` shards - the cells of
//! seeds crowded round where it was struck (`focus`), so glass crazes small
//! at the impact and in long shards away from it. Each is a convex-polygon
//! body drawn as a textured `Mesh2D` cut from the same picture, carries the
//! occluder and material, and remembers its texture map (`uv_map`) so it can
//! break again. A body that cannot move - a pane in its frame - breaks on
//! what hit it instead: the striking body's change in velocity, its impulse
//! over its mass.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{
    BodyType, Collider2D, ColliderShape, LightOccluder, Name, RigidBody2D, SpriteComponent, Transform, Velocity2D,
};
use crate::physics::{ContactEvent, ContactType, PhysicsWorld};

/// A body that breaks when hit hard enough.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Breakable {
    /// How hard a hit breaks it: the **change in its velocity** an impact
    /// gives it, in units / s. Not the contact impulse - a block resting on
    /// the ground reports nearly the impulse of one dropped on it at speed,
    /// its weight dominating the number, so no threshold on impulse tells a
    /// landing from sitting there. A change in velocity is near zero at rest
    /// and the same for a pebble or a boulder hitting at the same speed.
    pub break_speed: f32,
    /// How it comes apart ([`BreakPattern`]).
    pub pattern: BreakPattern,
    /// How many pieces across and down, for a `grid`.
    pub cols: u32,
    pub rows: u32,
    /// How many shards, for `voronoi` (a few fewer when cells are tiny).
    pub pieces: u32,
    /// For `voronoi`: the share of the shards crowded round the point it
    /// was struck - 0 cuts evenly, 1 crazes it all round the impact.
    pub focus: f32,
    /// How many more times a piece may break. 0: the pieces are rubble.
    pub generations: u32,
    /// Speed each piece is thrown out from the middle with, units / s.
    pub scatter: f32,
    /// Seconds before a piece is removed; 0 keeps them.
    pub piece_lifetime: f32,
    /// Dust thrown where it broke: the entity **name** of a (world-space)
    /// particle emitter to burst there - empty for none.
    pub debris: String,
    /// How many particles of it.
    pub debris_count: u32,
    /// Runtime: its velocity last frame, the largest change in it lately,
    /// and how long ago that was.
    #[serde(skip)]
    pub last_velocity: Option<[f32; 2]>,
    #[serde(skip)]
    pub jolt: f32,
    #[serde(skip)]
    pub jolt_age: f32,
    /// Runtime: a shard's place on the picture it was cut from
    /// (`u = a + bx x + by y`, `v = c + dx x + dy y` in its frame).
    #[serde(skip)]
    pub uv_map: Option<[f32; 6]>,
}

/// How long a jolt is remembered: a contact is reported a step after the
/// solver has already changed the velocity it resolved.
const JOLT_WINDOW: f32 = 0.12;

impl Default for Breakable {
    fn default() -> Self {
        Self {
            break_speed: 400.0,
            pattern: BreakPattern::Grid,
            cols: 2,
            rows: 2,
            pieces: 12,
            focus: 0.6,
            generations: 0,
            scatter: 40.0,
            piece_lifetime: 0.0,
            debris: String::new(),
            debris_count: 0,
            last_velocity: None,
            jolt: 0.0,
            jolt_age: 0.0,
            uv_map: None,
        }
    }
}

/// One break: where, how hard, and the dust to throw (emitter name, count).
#[derive(Debug, Clone, PartialEq)]
pub struct BreakEvent {
    pub entity_id: u64,
    pub point: Vec2,
    pub impulse: f32,
    pub debris: Option<(String, u32)>,
}

/// Break every `breakable` a contact this step hit hard enough: its
/// velocity changed by more than its `break_speed` in the last moment.
/// Call once a frame after `PhysicsWorld::step`, with what it returned.
pub fn break_on_impact(
    world: &mut World,
    physics: &mut PhysicsWorld,
    contacts: &[ContactEvent],
    dt: f32,
) -> Vec<BreakEvent> {
    // Track each breakable's velocity, and the largest change in it lately.
    for (e, b) in world.query_mut::<&mut Breakable>() {
        let Some((vx, vy)) = physics.get_velocity(e.id() as u64) else { continue };
        let dv = match b.last_velocity {
            Some([lx, ly]) => Vec2::new(vx - lx, vy - ly).length(),
            None => 0.0,
        };
        if dv >= b.jolt || b.jolt_age > JOLT_WINDOW {
            b.jolt = dv;
            b.jolt_age = 0.0;
        } else {
            b.jolt_age += dt;
        }
        b.last_velocity = Some([vx, vy]);
    }
    // Where each breakable was touched, this step, and by what.
    let mut hits: Vec<(u64, f32, Vec2, u64)> = Vec::new();
    for c in contacts {
        if c.event_type != ContactType::Started {
            continue;
        }
        for (id, other) in [(c.entity_a, c.entity_b), (c.entity_b, c.entity_a)] {
            if !hits.iter().any(|h| h.0 == id) {
                hits.push((id, c.impulse, Vec2::new(c.point.0, c.point.1), other));
            }
        }
    }
    let mut out = Vec::new();
    for (id, impulse, point, other) in hits {
        let Some(entity) = find(world, id) else { continue };
        let Ok(b) = world.get::<&Breakable>(entity).map(|b| (*b).clone()) else { continue };
        // A body that cannot move never jolts: judge it by what struck it.
        let jolt = if physics.body_type_of(id).is_none_or(|t| t == BodyType::Dynamic) {
            b.jolt
        } else {
            physics.mass_of(other).filter(|m| *m > 0.0).map_or(0.0, |m| impulse / m)
        };
        let enough = match b.pattern {
            BreakPattern::Grid => b.cols * b.rows >= 2,
            BreakPattern::Voronoi => b.pieces >= 2,
        };
        if jolt < b.break_speed || !enough {
            continue;
        }
        let v = physics.get_velocity(id).map(|(x, y)| Vec2::new(x, y));
        let w = physics.get_angular_velocity(id).unwrap_or(0.0);
        let debris = (!b.debris.is_empty() && b.debris_count > 0).then(|| (b.debris.clone(), b.debris_count));
        if shatter_at(world, entity, &b, v, w, Some(point)) {
            physics.remove_body(id);
            let _ = world.despawn(entity);
            out.push(BreakEvent { entity_id: id, point, impulse, debris });
        }
    }
    out
}

fn find(world: &World, id: u64) -> Option<Entity> {
    world.iter().map(|e| e.entity()).find(|e| e.id() as u64 == id)
}

/// Spawn the pieces of `entity` in its place, struck nowhere in particular.
/// `false` when it cannot break (the wrong shape, no transform).
pub fn shatter(world: &mut World, entity: Entity, b: &Breakable, v: Option<Vec2>, w: f32) -> bool {
    shatter_at(world, entity, b, v, w, None)
}

/// Spawn the pieces of `entity` in its place, struck at `at` (world).
pub fn shatter_at(world: &mut World, entity: Entity, b: &Breakable, v: Option<Vec2>, w: f32, at: Option<Vec2>) -> bool {
    if b.pattern == BreakPattern::Voronoi {
        let v = v.or_else(|| world.get::<&Velocity2D>(entity).ok().map(|v| Vec2::new(v.x, v.y))).unwrap_or(Vec2::ZERO);
        return shatter_voronoi(world, entity, b, v, w, at);
    }
    let Ok(t) = world.get::<&Transform>(entity).map(|t| (*t).clone()) else { return false };
    let Ok(col) = world.get::<&Collider2D>(entity).map(|c| (*c).clone()) else { return false };
    let ColliderShape::Box { width, height } = col.shape else { return false };
    let body = world.get::<&RigidBody2D>(entity).ok().map(|r| (*r).clone());
    let sprite = world.get::<&SpriteComponent>(entity).ok().map(|s| (*s).clone());
    let occluder = world.get::<&LightOccluder>(entity).ok().map(|o| (*o).clone());
    let material = world.get::<&crate::ecs::components::MaterialComponent>(entity).ok().map(|m| (*m).clone());
    let name = world.get::<&Name>(entity).ok().map(|n| n.0.clone()).unwrap_or_default();
    let v = v.or_else(|| world.get::<&Velocity2D>(entity).ok().map(|v| Vec2::new(v.x, v.y))).unwrap_or(Vec2::ZERO);

    let (cols, rows) = (b.cols.max(1), b.rows.max(1));
    let (pw, ph) = (width / cols as f32, height / rows as f32);
    let rot = glam::Mat2::from_angle(t.rotation);
    let uv = sprite.as_ref().and_then(|s| s.uv_rect).unwrap_or([0.0, 0.0, 1.0, 1.0]);
    for j in 0..rows {
        for i in 0..cols {
            // The piece's centre in the body's frame, then the world's.
            let local = Vec2::new((i as f32 + 0.5) * pw - width * 0.5, (j as f32 + 0.5) * ph - height * 0.5)
                + Vec2::from(col.offset);
            let r = rot * (local * t.scale);
            let mut e = hecs::EntityBuilder::new();
            e.add(Transform { position: t.position + r, rotation: t.rotation, scale: t.scale });
            e.add(Name(format!("{name}#{i}.{j}")));
            // What the part was doing: the body's velocity plus its spin at
            // that point, and a little outward.
            let spin = Vec2::new(-r.y, r.x) * w;
            let out = r.normalize_or_zero() * b.scatter;
            let pv = v + spin + out;
            e.add(Velocity2D { x: pv.x, y: pv.y, ..Default::default() });
            e.add(Collider2D { shape: ColliderShape::Box { width: pw, height: ph }, offset: [0.0, 0.0], ..col.clone() });
            // A piece is loose, whatever held the whole in place.
            if let Some(rb) = &body {
                e.add(RigidBody2D { body_type: BodyType::Dynamic, ..rb.clone() });
            }
            if let Some(s) = &sprite {
                // Rows run up in the body and down in the texture.
                let (u0, v0, u1, v1) = (uv[0], uv[1], uv[2], uv[3]);
                let su = (u1 - u0) / cols as f32;
                let sv = (v1 - v0) / rows as f32;
                let row_from_top = rows - 1 - j;
                let size = s.size.unwrap_or([width, height]);
                let mut piece = s.clone();
                piece.size = Some([size[0] / cols as f32, size[1] / rows as f32]);
                piece.uv_rect = Some([
                    u0 + su * i as f32,
                    v0 + sv * row_from_top as f32,
                    u0 + su * (i + 1) as f32,
                    v0 + sv * (row_from_top + 1) as f32,
                ]);
                piece.region = None;
                piece.anchor = [0.5, 0.5];
                e.add(piece);
            }
            if let Some(o) = &occluder {
                let mut o = o.clone();
                o.size = [pw, ph];
                o.polygon.clear();
                o.from_sprite = false;
                e.add(o);
            }
            if let Some(m) = &material {
                e.add(m.clone());
            }
            if b.generations > 0 {
                e.add(Breakable { generations: b.generations - 1, last_velocity: None, jolt: 0.0, jolt_age: 0.0, ..b.clone() });
            }
            if b.piece_lifetime > 0.0 {
                e.add(crate::ecs::components::Lifetime { remaining: b.piece_lifetime, ..Default::default() });
            }
            world.spawn(e.build());
        }
    }
    true
}

/// How a `breakable` comes apart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakPattern {
    /// `cols` x `rows` boxes, each a sub-rect of the sprite.
    #[default]
    Grid,
    /// `pieces` shards, the cells of seeds crowded round where it was hit
    /// (`focus`), each a convex polygon drawn as a textured mesh.
    Voronoi,
}

/// A small deterministic generator, so a pane struck in the same place
/// breaks the same way in every runtime.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The outline of a convex collider in its body's frame (before scale),
/// anticlockwise: a box, a triangle or a convex polygon, offset included.
pub fn convex_outline(col: &Collider2D) -> Option<Vec<Vec2>> {
    let o = Vec2::from(col.offset);
    let pts: Vec<Vec2> = match &col.shape {
        ColliderShape::Box { width, height } => {
            let (hw, hh) = (width * 0.5, height * 0.5);
            vec![Vec2::new(-hw, -hh), Vec2::new(hw, -hh), Vec2::new(hw, hh), Vec2::new(-hw, hh)]
        }
        ColliderShape::Triangle { a, b, c } => vec![Vec2::from(*a), Vec2::from(*b), Vec2::from(*c)],
        ColliderShape::ConvexPolygon { vertices } if vertices.len() >= 3 => vertices.iter().map(|v| Vec2::from(*v)).collect(),
        _ => return None,
    };
    let mut pts: Vec<Vec2> = pts.into_iter().map(|p| p + o).collect();
    if signed_area(&pts) < 0.0 {
        pts.reverse();
    }
    Some(pts)
}

pub(crate) fn signed_area(p: &[Vec2]) -> f32 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u.x * v.y - v.x * u.y;
    }
    a * 0.5
}

pub(crate) fn centroid(p: &[Vec2]) -> Vec2 {
    let (mut c, mut a) = (Vec2::ZERO, 0.0);
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        let cross = u.x * v.y - v.x * u.y;
        a += cross;
        c += (u + v) * cross;
    }
    if a.abs() < 1e-9 { p.iter().copied().sum::<Vec2>() / p.len().max(1) as f32 } else { c / (3.0 * a) }
}

/// Keep the part of convex `poly` where `(x - at) . n <= 0`.
pub(crate) fn clip(poly: &[Vec2], at: Vec2, n: Vec2) -> Vec<Vec2> {
    let side = |p: Vec2| (p - at).dot(n);
    let mut out = Vec::with_capacity(poly.len() + 1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (sa, sb) = (side(a), side(b));
        if sa <= 0.0 {
            out.push(a);
        }
        if (sa < 0.0 && sb > 0.0) || (sa > 0.0 && sb < 0.0) {
            out.push(a + (b - a) * (sa / (sa - sb)));
        }
    }
    out
}

/// The Voronoi cells of `seeds` inside convex `outline`: each the outline
/// cut by the bisector against every other seed. Cells too small to be a
/// body are left out, so the pieces may be fewer than the seeds.
pub fn voronoi_cells(outline: &[Vec2], seeds: &[Vec2]) -> Vec<Vec<Vec2>> {
    let mut cells = Vec::new();
    for (i, s) in seeds.iter().enumerate() {
        let mut cell = outline.to_vec();
        for (j, o) in seeds.iter().enumerate() {
            if i == j || cell.len() < 3 {
                continue;
            }
            let n = *o - *s;
            if n.length_squared() < 1e-6 {
                continue;
            }
            cell = clip(&cell, (*s + *o) * 0.5, n);
        }
        if cell.len() >= 3 && signed_area(&cell) > 4.0 {
            cells.push(cell);
        }
    }
    cells
}

/// Where the seeds go: `focus` of them scattered round `at` (body frame,
/// before scale) within a third of the body, the rest anywhere inside.
pub fn shard_seeds(outline: &[Vec2], n: u32, focus: f32, at: Option<Vec2>, seed: u64) -> Vec<Vec2> {
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for p in outline {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let size = hi - lo;
    let inside = |p: Vec2| {
        (0..outline.len()).all(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            (b - a).perp_dot(p - a) >= 0.0
        })
    };
    let mut rng = Rng(seed | 1);
    let mut out = Vec::new();
    for k in 0..n {
        let near = at.is_some() && (k as f32) < focus.clamp(0.0, 1.0) * n as f32;
        for _ in 0..24 {
            let p = match (near, at) {
                (true, Some(c)) => {
                    // Closer in for the first seeds: a crack starts small.
                    // Measured on the diagonal: a tall thin pane crowded within its
                    // narrow side packs its seeds into slivers too small to keep.
                    let r = size.length() * (0.04 + 0.22 * (k as f32 / n as f32)) * rng.next().sqrt();
                    let a = rng.next() * std::f32::consts::TAU;
                    c + Vec2::new(a.cos(), a.sin()) * r
                }
                _ => lo + size * Vec2::new(rng.next(), rng.next()),
            };
            if inside(p) {
                out.push(p);
                break;
            }
        }
    }
    out
}

/// A texture placed over a body: `u = a + bx x + by y`, `v = c + dx x + dy y`
/// in its frame before scale, so a shard knows where on the picture it was.
pub fn sprite_uv_map(s: &SpriteComponent, fallback: [f32; 2]) -> [f32; 6] {
    let size = s.size.unwrap_or(fallback);
    let uv = s.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let (x0, y0) = (-s.anchor[0] * size[0], -s.anchor[1] * size[1]);
    let bx = (uv[2] - uv[0]) / size[0].max(1e-6);
    let dy = -(uv[3] - uv[1]) / size[1].max(1e-6);
    // v runs down the texture: the top edge (y0 + h) is v0.
    [uv[0] - bx * x0, bx, 0.0, uv[1] - dy * (y0 + size[1]), 0.0, dy]
}

/// The map a mesh's own vertices make: the affine fit through three of
/// them that are not in a line (`None` when every triangle is flat).
pub fn mesh_uv_map(m: &crate::ecs::components::Mesh2D) -> Option<[f32; 6]> {
    let v = &m.vertices;
    for i in 0..v.len() {
        for j in i + 1..v.len() {
            for k in j + 1..v.len() {
                let (p0, p1, p2) = (Vec2::from(v[i].position), Vec2::from(v[j].position), Vec2::from(v[k].position));
                let (e1, e2) = (p1 - p0, p2 - p0);
                let det = e1.perp_dot(e2);
                if det.abs() < 1e-6 {
                    continue;
                }
                // Solve [e1 e2]^T [b d] = [du dv] for the gradients.
                let grad = |a0: f32, a1: f32, a2: f32| {
                    let (d1, d2) = (a1 - a0, a2 - a0);
                    let gx = (d1 * e2.y - d2 * e1.y) / det;
                    let gy = (e1.x * d2 - e2.x * d1) / det;
                    (a0 - gx * p0.x - gy * p0.y, gx, gy)
                };
                let (a, bx, by) = grad(v[i].uv[0], v[j].uv[0], v[k].uv[0]);
                let (c, dx, dy) = grad(v[i].uv[1], v[j].uv[1], v[k].uv[1]);
                return Some([a, bx, by, c, dx, dy]);
            }
        }
    }
    None
}

pub(crate) fn uv_at(m: &[f32; 6], p: Vec2) -> [f32; 2] {
    [m[0] + m[1] * p.x + m[2] * p.y, m[3] + m[4] * p.x + m[5] * p.y]
}

/// Break `entity` into Voronoi shards round `at` (world). Every shard is a
/// dynamic body - glass that breaks does not stay in its frame.
fn shatter_voronoi(world: &mut World, entity: Entity, b: &Breakable, v: Vec2, w: f32, at: Option<Vec2>) -> bool {
    let Ok(t) = world.get::<&Transform>(entity).map(|t| (*t).clone()) else { return false };
    let Ok(col) = world.get::<&Collider2D>(entity).map(|c| (*c).clone()) else { return false };
    let Some(outline) = convex_outline(&col) else { return false };
    let body = world.get::<&RigidBody2D>(entity).ok().map(|r| (*r).clone());
    let sprite = world.get::<&SpriteComponent>(entity).ok().map(|s| (*s).clone());
    let mesh = world.get::<&crate::ecs::components::Mesh2D>(entity).ok().map(|m| (*m).clone());
    let occluder = world.get::<&LightOccluder>(entity).ok().map(|o| (*o).clone());
    let material = world.get::<&crate::ecs::components::MaterialComponent>(entity).ok().map(|m| (*m).clone());
    let name = world.get::<&Name>(entity).ok().map(|n| n.0.clone()).unwrap_or_default();

    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for p in &outline {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    // What the shards are drawn with: the sprite, or the mesh a shard
    // already is, with the map it was cut with.
    let (texture, tint, z, uv_rect, map) = match (&sprite, &mesh) {
        (Some(s), _) => (s.texture_id.as_str().to_string(), s.color, s.z_order, None, b.uv_map.unwrap_or_else(|| sprite_uv_map(s, (hi - lo).into()))),
        (None, Some(m)) => (m.texture_id.clone(), m.color, m.z_order, m.uv_rect, b.uv_map.or_else(|| mesh_uv_map(m)).unwrap_or([0.0; 6])),
        _ => (String::new(), [1.0; 4], 0, None, [0.0; 6]),
    };
    let rot = glam::Mat2::from_angle(t.rotation);
    let nz = |v: f32| if v.abs() < 1e-6 { 1e-6 } else { v };
    let scale = Vec2::new(nz(t.scale.x), nz(t.scale.y));
    let local_at = at.map(|p| (rot.transpose() * (p - t.position)) / scale);
    let seed = entity.to_bits().get() ^ ((b.generations as u64) << 40) ^ 0x9E37_79B9_7F4A_7C15;
    let seeds = shard_seeds(&outline, b.pieces.max(2), b.focus, local_at, seed);
    let cells = voronoi_cells(&outline, &seeds);
    if cells.len() < 2 {
        return false;
    }
    for (k, cell) in cells.iter().enumerate() {
        let c = centroid(cell);
        let rel: Vec<Vec2> = cell.iter().map(|p| *p - c).collect();
        let r = rot * (c * t.scale);
        let mut e = hecs::EntityBuilder::new();
        e.add(Transform { position: t.position + r, rotation: t.rotation, scale: t.scale });
        e.add(Name(format!("{name}#{k}")));
        let spin = Vec2::new(-r.y, r.x) * w;
        // Thrown out from where it was struck, not from the middle.
        let from = local_at.map_or(c, |a| c - a);
        let out = (rot * (from * t.scale)).normalize_or_zero() * b.scatter;
        let pv = v + spin + out;
        e.add(Velocity2D { x: pv.x, y: pv.y, ..Default::default() });
        e.add(Collider2D {
            shape: ColliderShape::ConvexPolygon { vertices: rel.iter().map(|p| [p.x, p.y]).collect() },
            offset: [0.0, 0.0],
            ..col.clone()
        });
        let mut rb = body.clone().unwrap_or_default();
        rb.body_type = crate::ecs::components::BodyType::Dynamic;
        e.add(rb);
        if !texture.is_empty() || sprite.is_some() || mesh.is_some() {
            e.add(crate::ecs::components::Mesh2D {
                texture_id: texture.clone(),
                uv_rect,
                vertices: rel
                    .iter()
                    .zip(cell)
                    .map(|(p, q)| crate::ecs::components::MeshVertex {
                        position: [p.x, p.y],
                        uv: uv_at(&map, *q),
                        ..Default::default()
                    })
                    .collect(),
                color: tint,
                z_order: z,
                ..Default::default()
            });
        }
        if let Some(o) = &occluder {
            let mut o = o.clone();
            o.polygon = rel.iter().map(|p| [p.x, p.y]).collect();
            o.from_sprite = false;
            o.from_alpha = false;
            e.add(o);
        }
        if let Some(m) = &material {
            e.add(m.clone());
        }
        if b.generations > 0 {
            // The map moved to the shard's own frame.
            let m = map;
            let shifted = [m[0] + m[1] * c.x + m[2] * c.y, m[1], m[2], m[3] + m[4] * c.x + m[5] * c.y, m[4], m[5]];
            e.add(Breakable {
                generations: b.generations - 1,
                last_velocity: None,
                jolt: 0.0,
                jolt_age: 0.0,
                uv_map: Some(shifted),
                ..b.clone()
            });
        }
        if b.piece_lifetime > 0.0 {
            e.add(crate::ecs::components::Lifetime { remaining: b.piece_lifetime, ..Default::default() });
        }
        world.spawn(e.build());
    }
    true
}

/// Throw each break's dust: burst its named emitter at the point it broke.
pub fn burst_debris(world: &World, particles: &mut crate::particles::ParticleSystem, breaks: &[BreakEvent]) {
    for b in breaks {
        let Some((emitter, count)) = &b.debris else { continue };
        let found = world
            .query::<(&Name, &crate::particles::ParticleEmitter)>()
            .iter()
            .find(|(_, (n, _))| &n.0 == emitter)
            .map(|(e, (_, em))| (e.id() as u64, em.clone()));
        if let Some((id, em)) = found {
            particles.burst(id, &em, b.point, *count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_shatters_into_its_grid_of_sprite_pieces() {
        let mut world = World::new();
        let col: Collider2D =
            serde_json::from_value(serde_json::json!({ "shape": { "Box": { "width": 40.0, "height": 20.0 } } })).unwrap();
        let b = Breakable { cols: 2, rows: 2, generations: 1, scatter: 0.0, ..Default::default() };
        let e = world.spawn((
            Transform { position: Vec2::new(100.0, 50.0), rotation: 0.0, scale: Vec2::ONE },
            col,
            SpriteComponent { size: Some([40.0, 20.0]), uv_rect: Some([0.5, 0.0, 1.0, 0.5]), ..Default::default() },
            Name("Stone".into()),
            b.clone(),
        ));
        assert!(shatter(&mut world, e, &b, Some(Vec2::new(10.0, 0.0)), 0.0));
        world.despawn(e).unwrap();
        let mut pieces: Vec<(Vec2, [f32; 4], [f32; 2], f32)> = world
            .query::<(&Transform, &SpriteComponent, &Velocity2D)>()
            .iter()
            .map(|(_, (t, s, v))| (t.position, s.uv_rect.unwrap(), s.size.unwrap(), v.x))
            .collect();
        pieces.sort_by(|a, b| (a.0.y, a.0.x).partial_cmp(&(b.0.y, b.0.x)).unwrap());
        assert_eq!(pieces.len(), 4);
        // Bottom-left piece: centred a quarter in, the bottom-left of the uv.
        assert_eq!(pieces[0].0, Vec2::new(90.0, 45.0));
        assert_eq!(pieces[0].2, [20.0, 10.0]);
        assert_eq!(pieces[0].1, [0.5, 0.25, 0.75, 0.5], "rows run up in the body and down in the texture");
        assert_eq!(pieces[3].1, [0.75, 0.0, 1.0, 0.25]);
        assert!(pieces.iter().all(|p| p.3 == 10.0), "each keeps the body's velocity");
        // One more generation, then rubble.
        assert_eq!(world.query::<&Breakable>().iter().filter(|(_, b)| b.generations == 0).count(), 4);
    }

    fn pane() -> Vec<Vec2> {
        convex_outline(&Collider2D { shape: ColliderShape::Box { width: 200.0, height: 100.0 }, ..Default::default() }).unwrap()
    }

    #[test]
    fn voronoi_cells_tile_the_outline() {
        let outline = pane();
        let seeds = shard_seeds(&outline, 14, 0.5, Some(Vec2::new(-60.0, 10.0)), 7);
        assert_eq!(seeds.len(), 14);
        let cells = voronoi_cells(&outline, &seeds);
        assert!(cells.len() >= 12, "{} cells", cells.len());
        let total: f32 = cells.iter().map(|c| signed_area(c)).sum();
        assert!((total - 200.0 * 100.0).abs() < 1.0, "the shards cover the pane exactly: {total}");
        for c in &cells {
            assert!(signed_area(c) > 0.0, "every cell anticlockwise");
            assert!(c.iter().all(|p| p.x.abs() <= 100.01 && p.y.abs() <= 50.01), "and inside the pane");
        }
    }

    #[test]
    fn shards_are_small_where_it_was_struck() {
        let outline = pane();
        let at = Vec2::new(-70.0, 0.0);
        let cells = voronoi_cells(&outline, &shard_seeds(&outline, 16, 0.7, Some(at), 3));
        let (mut near, mut far) = (Vec::new(), Vec::new());
        for c in &cells {
            let a = signed_area(c);
            if (centroid(c) - at).length() < 45.0 { near.push(a) } else { far.push(a) }
        }
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
        assert!(!near.is_empty() && !far.is_empty());
        assert!(mean(&near) * 3.0 < mean(&far), "near {:?} far {:?}", near, far);
        // The same blow breaks the same way.
        assert_eq!(cells, voronoi_cells(&outline, &shard_seeds(&outline, 16, 0.7, Some(at), 3)));
    }

    #[test]
    fn shards_are_cut_from_the_picture_and_can_break_again() {
        let mut world = World::new();
        let b = Breakable { pattern: BreakPattern::Voronoi, pieces: 8, generations: 1, scatter: 0.0, ..Default::default() };
        let e = world.spawn((
            Transform { position: Vec2::new(100.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
            Collider2D { shape: ColliderShape::Box { width: 200.0, height: 100.0 }, ..Default::default() },
            RigidBody2D { body_type: BodyType::Static, ..Default::default() },
            SpriteComponent { texture_id: "glass.png".into(), size: Some([200.0, 100.0]), ..Default::default() },
            Name("Pane".into()),
            b.clone(),
        ));
        assert!(shatter_at(&mut world, e, &b, None, 0.0, Some(Vec2::new(50.0, 20.0))));
        world.despawn(e).unwrap();
        let shards: Vec<(Transform, crate::ecs::components::Mesh2D, RigidBody2D, Breakable)> = world
            .query::<(&Transform, &crate::ecs::components::Mesh2D, &RigidBody2D, &Breakable)>()
            .iter()
            .map(|(_, (t, m, r, b))| (t.clone(), m.clone(), r.clone(), b.clone()))
            .collect();
        assert!(shards.len() >= 6);
        for (t, m, r, sb) in &shards {
            assert_eq!(r.body_type, BodyType::Dynamic, "a shard is loose");
            assert_eq!(m.texture_id, "glass.png");
            for v in &m.vertices {
                // Where the vertex is on the pane gives its place on the
                // picture: u across from the left edge, v down from the top.
                let x = t.position.x + v.position[0] - 100.0;
                let y = t.position.y + v.position[1];
                assert!((v.uv[0] - (x + 100.0) / 200.0).abs() < 1e-4, "u at {x}");
                assert!((v.uv[1] - (50.0 - y) / 100.0).abs() < 1e-4, "v at {y}");
            }
            assert_eq!(sb.generations, 0);
            assert!(sb.uv_map.is_some(), "a shard keeps its map to break again");
        }
        // Break a shard again: its pieces sit on the same picture.
        let (shard, (t, sb)) = world
            .query::<(&Transform, &Breakable)>()
            .iter()
            .map(|(e, (t, b))| (e, (t.clone(), b.clone())))
            .next()
            .unwrap();
        let again = Breakable { pieces: 4, ..sb };
        assert!(shatter_at(&mut world, shard, &again, None, 0.0, Some(t.position)));
        world.despawn(shard).unwrap();
        let small = world.query::<(&Transform, &crate::ecs::components::Mesh2D)>().iter().count();
        assert!(small > shards.len(), "more pieces than before");
        for (_, (t, m)) in world.query::<(&Transform, &crate::ecs::components::Mesh2D)>().iter() {
            for v in &m.vertices {
                let x = t.position.x + v.position[0] - 100.0;
                assert!((v.uv[0] - (x + 100.0) / 200.0).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn a_mesh_pane_gives_its_shards_its_own_map() {
        use crate::ecs::components::{Mesh2D, MeshVertex};
        // A trapezoid of glass, uv from its bounding box (v down).
        let pts = [[-50.0, -40.0], [50.0, -40.0], [50.0, 10.0], [-50.0, 40.0]];
        let mesh = Mesh2D {
            texture_id: "glass.png".into(),
            vertices: pts
                .iter()
                .map(|p| MeshVertex { position: *p, uv: [(p[0] + 50.0) / 100.0, (40.0 - p[1]) / 80.0], ..Default::default() })
                .collect(),
            ..Default::default()
        };
        let m = mesh_uv_map(&mesh).unwrap();
        let at = uv_at(&m, Vec2::new(10.0, 0.0));
        assert!((at[0] - 0.6).abs() < 1e-5 && (at[1] - 0.5).abs() < 1e-5, "{at:?}");
    }

    #[test]
    fn only_a_box_breaks() {
        let mut world = World::new();
        let col: Collider2D =
            serde_json::from_value(serde_json::json!({ "shape": { "Circle": { "radius": 10.0 } } })).unwrap();
        let b = Breakable::default();
        let e = world.spawn((Transform::default(), col, b.clone()));
        assert!(!shatter(&mut world, e, &b, None, 0.0));
    }
}
