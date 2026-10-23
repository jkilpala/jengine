//! **Slicing** (Oct 2026): a body cut in two along a line - fruit under a
//! blade, a rope bridge's plank, a crate split by a laser. `Physics.slice(x0,
//! y0, x1, y1)` cuts every [`Sliceable`] the segment crosses: its convex
//! outline (a box, triangle, convex polygon or circle) is clipped by the
//! line into two halves, each a dynamic body of its own - its collider that
//! half, drawn as a textured `Mesh2D` cut from the same picture (the
//! sprite's, or `inside_texture` laid over it the same way, to show what is
//! inside), moving off with the body's velocity plus `separation` away from
//! the cut. A half keeps its texture map, so it can be cut again for
//! `generations` more cuts. Halves smaller than `min_area` are not cut off.
//!
//! The geometry is the breaking code's (`ecs::fracture`: outlines, the
//! clip, the texture map); a cut needs nothing but the world, so the command
//! is one shared host command and no host was wired - the physics drops the
//! old body (`remove_stale_bodies`) and builds the halves as it builds any
//! new body. The halves are named `<name>/a` and `<name>/b`, so a script
//! knows what it cut by the name it gave.

use glam::{Mat2, Vec2};
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{
    BodyType, Collider2D, ColliderShape, LightOccluder, Mesh2D, MeshVertex, Name, RigidBody2D, SpriteComponent, Transform,
    Velocity2D,
};
use crate::ecs::fracture::{centroid, clip, convex_outline, mesh_uv_map, signed_area, sprite_uv_map, uv_at};

/// A body a blade can cut in two.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sliceable {
    /// What the halves are drawn with (the cut face showing what is inside),
    /// laid over them as the sprite was; empty keeps the sprite's picture.
    pub inside_texture: String,
    /// Speed each half is pushed off the cut with, units / s.
    pub separation: f32,
    /// A half smaller than this (square units) is not cut off.
    pub min_area: f32,
    /// How many more times a half may be cut.
    pub generations: u32,
    /// Seconds before a half is removed; 0 keeps them.
    pub piece_lifetime: f32,
    /// Where the picture lies on the body (`u = a + bx x + by y`, `v = c +
    /// dx x + dy y`), carried by a half so a second cut still matches it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uv_map: Option<[f32; 6]>,
}

impl Default for Sliceable {
    fn default() -> Self {
        Self { inside_texture: String::new(), separation: 160.0, min_area: 60.0, generations: 1, piece_lifetime: 0.0, uv_map: None }
    }
}

/// One cut: what was cut (its id and name), where the blade crossed its
/// middle, and the two halves.
#[derive(Debug, Clone)]
pub struct Cut {
    pub entity_id: u64,
    pub name: String,
    pub point: Vec2,
    pub halves: [Entity; 2],
}

/// The body's convex outline in its own frame, a circle as 24 sides.
pub fn outline(col: &Collider2D) -> Option<Vec<Vec2>> {
    if let ColliderShape::Circle { radius } = col.shape {
        let o = Vec2::from(col.offset);
        return Some((0..24).map(|i| o + Vec2::from_angle(i as f32 / 24.0 * std::f32::consts::TAU) * radius).collect());
    }
    convex_outline(col)
}

/// Whether segment `a..b` reaches into convex `poly` (counter-clockwise).
fn crosses(poly: &[Vec2], a: Vec2, b: Vec2) -> bool {
    let inside = |p: Vec2| (0..poly.len()).all(|i| (poly[(i + 1) % poly.len()] - poly[i]).perp_dot(p - poly[i]) >= 0.0);
    if inside(a) || inside(b) {
        return true;
    }
    let d = b - a;
    (0..poly.len()).any(|i| {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let e = q - p;
        let den = d.perp_dot(e);
        if den.abs() < 1e-9 {
            return false;
        }
        let t = (p - a).perp_dot(e) / den;
        let u = (p - a).perp_dot(d) / den;
        (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
    })
}

/// Cut every sliceable the segment `a..b` (world) crosses. Returns the cuts.
pub fn slice(world: &mut World, a: Vec2, b: Vec2) -> Vec<Cut> {
    if a.distance_squared(b) < 1e-6 {
        return Vec::new();
    }
    let candidates: Vec<(Entity, Sliceable, Transform, Collider2D)> = world
        .query::<(&Sliceable, &Transform, &Collider2D)>()
        .iter()
        .map(|(e, (s, t, c))| (e, s.clone(), t.clone(), c.clone()))
        .collect();
    let mut cuts = Vec::new();
    for (entity, s, t, col) in candidates {
        let Some(local) = outline(&col) else { continue };
        let rot = Mat2::from_angle(t.rotation);
        let nz = |v: f32| if v.abs() < 1e-6 { 1e-6 } else { v };
        let scale = Vec2::new(nz(t.scale.x), nz(t.scale.y));
        let to_world = |p: Vec2| t.position + rot * (p * scale);
        let to_local = |p: Vec2| (rot.transpose() * (p - t.position)) / scale;
        let mut world_poly: Vec<Vec2> = local.iter().map(|p| to_world(*p)).collect();
        if signed_area(&world_poly) < 0.0 {
            world_poly.reverse();
        }
        if !crosses(&world_poly, a, b) {
            continue;
        }
        // The cut line in the body's own frame (an affine map keeps lines).
        let (la, lb) = (to_local(a), to_local(b));
        let n = (lb - la).perp().normalize_or_zero();
        if n == Vec2::ZERO {
            continue;
        }
        let halves = [clip(&local, la, n), clip(&local, la, -n)];
        let area_scale = (scale.x * scale.y).abs();
        if halves.iter().any(|h| h.len() < 3 || signed_area(h).abs() * area_scale < s.min_area) {
            continue;
        }
        let spawned = spawn_halves(world, entity, &s, &t, &col, &halves, n);
        let name = world.get::<&Name>(entity).map(|n| n.0.clone()).unwrap_or_default();
        let point = to_world(centroid(&local));
        let id = entity.id() as u64;
        let _ = world.despawn(entity);
        cuts.push(Cut { entity_id: id, name, point, halves: spawned });
    }
    cuts
}

fn spawn_halves(world: &mut World, entity: Entity, s: &Sliceable, t: &Transform, col: &Collider2D, halves: &[Vec<Vec2>; 2], n: Vec2) -> [Entity; 2] {
    let body = world.get::<&RigidBody2D>(entity).ok().map(|r| (*r).clone());
    let sprite = world.get::<&SpriteComponent>(entity).ok().map(|s| (*s).clone());
    let mesh = world.get::<&Mesh2D>(entity).ok().map(|m| (*m).clone());
    let occluder = world.get::<&LightOccluder>(entity).ok().map(|o| (*o).clone());
    let material = world.get::<&crate::ecs::components::MaterialComponent>(entity).ok().map(|m| (*m).clone());
    let velocity = world.get::<&Velocity2D>(entity).ok().map(|v| Vec2::new(v.x, v.y)).unwrap_or(Vec2::ZERO);
    let name = world.get::<&Name>(entity).ok().map(|n| n.0.clone()).unwrap_or_default();
    let fallback = {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for h in halves {
            for p in h {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        (hi - lo).into()
    };
    let (texture, tint, z, uv_rect, map) = match (&sprite, &mesh) {
        (Some(sp), _) => (sp.texture_id.as_str().to_string(), sp.color, sp.z_order, None, s.uv_map.unwrap_or_else(|| sprite_uv_map(sp, fallback))),
        (None, Some(m)) => (m.texture_id.clone(), m.color, m.z_order, m.uv_rect, s.uv_map.or_else(|| mesh_uv_map(m)).unwrap_or([0.0; 6])),
        _ => (String::new(), [1.0; 4], 0, None, [0.0; 6]),
    };
    // The inside is laid over the whole picture, not a region of a sheet.
    let (texture, uv_rect) = if s.inside_texture.is_empty() { (texture, uv_rect) } else { (s.inside_texture.clone(), None) };
    let rot = Mat2::from_angle(t.rotation);
    let push = (rot * (n * t.scale)).normalize_or_zero() * s.separation;
    let mut out = Vec::with_capacity(2);
    for (k, half) in halves.iter().enumerate() {
        let c = centroid(half);
        let rel: Vec<Vec2> = half.iter().map(|p| *p - c).collect();
        let mut e = hecs::EntityBuilder::new();
        e.add(Transform { position: t.position + rot * (c * t.scale), rotation: t.rotation, scale: t.scale });
        e.add(Name(format!("{name}/{}", if k == 0 { "a" } else { "b" })));
        // The first half lies on the side the normal points away from.
        let v = velocity + if k == 0 { -push } else { push };
        e.add(Velocity2D { x: v.x, y: v.y, ..Default::default() });
        e.add(Collider2D {
            shape: ColliderShape::ConvexPolygon { vertices: rel.iter().map(|p| [p.x, p.y]).collect() },
            offset: [0.0, 0.0],
            ..col.clone()
        });
        let mut rb = body.clone().unwrap_or_default();
        rb.body_type = BodyType::Dynamic;
        e.add(rb);
        if !texture.is_empty() || sprite.is_some() || mesh.is_some() {
            e.add(Mesh2D {
                texture_id: texture.clone(),
                uv_rect,
                vertices: rel.iter().zip(half).map(|(p, q)| MeshVertex { position: [p.x, p.y], uv: uv_at(&map, *q), ..Default::default() }).collect(),
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
        if s.generations > 0 {
            let shifted = [map[0] + map[1] * c.x + map[2] * c.y, map[1], map[2], map[3] + map[4] * c.x + map[5] * c.y, map[4], map[5]];
            // The halves are drawn with the inside now: cut again, they stay so.
            e.add(Sliceable { generations: s.generations - 1, uv_map: Some(shifted), ..s.clone() });
        }
        if s.piece_lifetime > 0.0 {
            e.add(crate::ecs::components::Lifetime { remaining: s.piece_lifetime, ..Default::default() });
        }
        out.push(world.spawn(e.build()));
    }
    [out[0], out[1]]
}

/// `Physics.slice(x0, y0, x1, y1)` from a script: cut what the segment
/// crosses.
pub fn script_slice(world: &mut World, x0: f32, y0: f32, x1: f32, y1: f32) {
    let cuts = slice(world, Vec2::new(x0, y0), Vec2::new(x1, y1));
    for c in &cuts {
        log::debug!("Physics.slice: cut '{}' at {}", c.name, c.point);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fruit(world: &mut World, x: f32, y: f32) -> Entity {
        world.spawn((
            Name("Melon".into()),
            Transform { position: Vec2::new(x, y), ..Default::default() },
            Collider2D { shape: ColliderShape::Circle { radius: 30.0 }, ..Default::default() },
            RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
            Velocity2D { x: 0.0, y: 100.0, ..Default::default() },
            SpriteComponent { texture_id: "melon.png".into(), size: Some([60.0, 60.0]), ..Default::default() },
            Sliceable { inside_texture: "melon_inside.png".into(), ..Default::default() },
        ))
    }

    #[test]
    fn a_swipe_cuts_a_fruit_in_two_halves_that_part() {
        let mut world = World::new();
        let e = fruit(&mut world, 100.0, 50.0);
        // A vertical swipe through its middle.
        let cuts = slice(&mut world, Vec2::new(100.0, 0.0), Vec2::new(100.0, 120.0));
        assert_eq!(cuts.len(), 1);
        assert!(!world.contains(e), "the whole fruit is gone");
        let [a, b] = cuts[0].halves;
        let pa = world.get::<&Transform>(a).unwrap().position;
        let pb = world.get::<&Transform>(b).unwrap().position;
        assert!((pa.x - 100.0).abs() > 8.0 && (pb.x - 100.0).abs() > 8.0 && (pa.x - 100.0).signum() != (pb.x - 100.0).signum(),
            "one half each side of the cut: {pa} / {pb}");
        let (va, vb) = ((*world.get::<&Velocity2D>(a).unwrap()).clone(), (*world.get::<&Velocity2D>(b).unwrap()).clone());
        assert!((va.x - 0.0).signum() == (pa.x - 100.0).signum() && (vb.x).signum() == (pb.x - 100.0).signum(), "pushed apart");
        assert!((va.y - 100.0).abs() < 1e-3, "and still going up as it was");
        assert_eq!(world.get::<&Name>(a).unwrap().0, "Melon/a");
        let m = world.get::<&Mesh2D>(a).unwrap();
        assert_eq!(m.texture_id, "melon_inside.png", "the halves show the inside");
        // A half of the 24-sided circle: its uvs on one side of 0.5.
        let us: Vec<f32> = m.vertices.iter().map(|v| v.uv[0]).collect();
        assert!(us.iter().all(|u| *u <= 0.5 + 1e-4) || us.iter().all(|u| *u >= 0.5 - 1e-4), "{us:?}");
        assert_eq!(world.get::<&Sliceable>(a).unwrap().generations, 0, "cut once more at most");
    }

    #[test]
    fn a_swipe_that_misses_or_grazes_cuts_nothing() {
        let mut world = World::new();
        let e = fruit(&mut world, 100.0, 50.0);
        assert!(slice(&mut world, Vec2::new(200.0, 0.0), Vec2::new(200.0, 120.0)).is_empty(), "a miss");
        // Through the very edge: the sliver is under the minimum.
        assert!(slice(&mut world, Vec2::new(129.5, 0.0), Vec2::new(129.5, 120.0)).is_empty(), "a graze");
        // A swipe that stops short of it.
        assert!(slice(&mut world, Vec2::new(0.0, 50.0), Vec2::new(60.0, 50.0)).is_empty(), "short");
        assert!(world.contains(e));
    }

    #[test]
    fn a_half_cut_again_keeps_its_picture_and_runs_out_of_cuts() {
        let mut world = World::new();
        fruit(&mut world, 0.0, 0.0);
        let cuts = slice(&mut world, Vec2::new(0.0, -50.0), Vec2::new(0.0, 50.0));
        let half = cuts[0].halves[1];
        let before: Vec<[f32; 2]> = world.get::<&Mesh2D>(half).unwrap().vertices.iter().map(|v| v.uv).collect();
        // Across the half on the right: a quarter.
        let again = slice(&mut world, Vec2::new(-50.0, 0.0), Vec2::new(50.0, 0.0));
        assert_eq!(again.len(), 2, "both halves cut");
        let q = again.iter().find(|c| c.name == "Melon/b").expect("the right half");
        let quarter = (*world.get::<&Mesh2D>(q.halves[0]).unwrap()).clone();
        // Every quarter vertex's uv lies inside the half's uv range.
        let (lo, hi) = before.iter().fold((f32::MAX, f32::MIN), |(lo, hi), uv| (lo.min(uv[0]), hi.max(uv[0])));
        assert!(quarter.vertices.iter().all(|v| v.uv[0] >= lo - 1e-3 && v.uv[0] <= hi + 1e-3));
        assert!(world.get::<&Sliceable>(q.halves[0]).is_err(), "no cuts left");
        assert!(slice(&mut world, Vec2::new(10.0, -50.0), Vec2::new(10.0, 50.0)).iter().all(|c| !c.name.starts_with("Melon/b")));
    }
}
