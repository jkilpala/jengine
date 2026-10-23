//! **Portals** (Oct 2026): a linked pair of lines, each showing the view out
//! of the other and carrying what goes in out of the other - a test chamber
//! where a crate thrown into one wall flies out of another.
//!
//! A [`Portal`] lies along its entity's local x, `width` long, facing its
//! local +y, like a mirror (`ecs::mirror`, whose geometry it shares), and
//! names its partner by entity name in `pair`. Each frame the region
//! `depth` deep in front of the **partner** is drawn by a view of this
//! portal's own ([`portal_views`], appended by `frame_draw` beside the
//! mirrors', so no host was wired) and the sprite pass draws it over the
//! region as deep **behind** this portal, turned half round rather than
//! flipped ([`window`]) - so the wall seems open onto the other place, the
//! partner's left on this portal's right, as walking through would have it.
//!
//! **Going through** ([`tick_portals`], in `tick_motion`): anything that
//! moves - a body, or a sprite moved by `velocity` - whose middle crosses a
//! portal's line from the front, within its width, comes out of the partner
//! at the same place turned half round about the partner's middle, its
//! velocity and rotation turned by the angle between the two, just in front
//! of the partner's line. A written transform teleports a body and a
//! written `velocity` reaches it, so the physics needed nothing new.
//! `carry: false` makes a portal a window only.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use glam::{Mat2, Vec2};
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{BodyType, Name, RigidBody2D, Transform, Velocity2D, WorldTransform};
use crate::ecs::mirror::{MAX_MIRROR_TEXELS, MirrorPlace};
use crate::renderer::camera::Camera2D;

/// `portal`: a line open onto its partner's.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Portal {
    /// The partner's entity name.
    pub pair: String,
    /// The portal's length, along the entity's local x.
    pub width: f32,
    /// How far it shows into the other place (and so how deep behind its
    /// line the view is drawn).
    pub depth: f32,
    /// Texels per world unit of the view's texture.
    pub resolution: f32,
    /// Multiplies the view (linear colour); alpha fades it.
    pub tint: [f32; 4],
    /// Where the view sorts among everything else.
    pub z_order: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    /// Whether things that go in come out of the partner.
    pub carry: bool,
    /// Off, the portal shows nothing and carries nothing.
    pub enabled: bool,
    /// Which side of the line each mover was on last frame (its local y),
    /// by entity bits.
    #[serde(skip)]
    pub seen: HashMap<u64, f32>,
}

impl Default for Portal {
    fn default() -> Self {
        Self {
            pair: String::new(),
            width: 120.0,
            depth: 200.0,
            resolution: 1.0,
            tint: [1.0; 4],
            z_order: 0,
            sort_layer: None,
            carry: true,
            enabled: true,
            seen: HashMap::new(),
        }
    }
}

/// A portal view's key in [`crate::renderer::camera::CameraPass::Mirror`],
/// out of the mirrors' range: its own pass leaves its own window out.
pub fn portal_key(portal: Entity) -> u32 {
    0x4000_0000 + portal.id() + 1
}

/// The texture a portal's view draws into.
pub fn target_id(portal: Entity) -> String {
    format!("__portal/{}", portal.id())
}

/// Where a portal is this frame, by its world transform.
pub fn place_of(world: &World, entity: Entity) -> Option<(Portal, MirrorPlace)> {
    let portal = (*world.get::<&Portal>(entity).ok()?).clone();
    let (p, r) = match (world.get::<&WorldTransform>(entity), world.get::<&Transform>(entity)) {
        (Ok(w), _) => (w.position, w.rotation),
        (_, Ok(t)) => (t.position, t.rotation),
        _ => return None,
    };
    let place = MirrorPlace::at(p, r, portal.width, portal.depth);
    Some((portal, place))
}

/// The partner `portal` names, if it exists and is a portal.
pub fn partner(world: &World, portal: &Portal) -> Option<Entity> {
    if portal.pair.is_empty() {
        return None;
    }
    world
        .query::<(&Name, &Portal)>()
        .iter()
        .find(|(_, (n, _))| n.0 == portal.pair)
        .map(|(e, _)| e)
}

/// The quad behind a portal and its texture coordinates in the partner's
/// view: the line takes the picture's bottom (the partner's line), the far
/// corners its top - and the left end the picture's right, a half turn.
pub fn window(place: &MirrorPlace) -> ([Vec2; 4], [[f32; 3]; 4]) {
    let half = place.along * place.width * 0.5;
    let back = place.front * place.depth;
    (
        [place.centre - half, place.centre + half, place.centre + half - back, place.centre - half - back],
        [[1.0, 1.0, 1.0], [0.0, 1.0, 1.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0]],
    )
}

/// Where a point behind `from`'s line comes out in front of `to`, and the
/// angle everything turns by on the way.
pub fn carry(from: &MirrorPlace, to: &MirrorPlace, p: Vec2) -> (Vec2, f32) {
    let d = p - from.centre;
    let (u, v) = (d.dot(from.along), d.dot(from.front));
    let out = to.centre - to.along * u - to.front * v;
    (out, to.rotation - from.rotation + std::f32::consts::PI)
}

thread_local! {
    /// The portals with a view this frame - the only ones whose texture
    /// exists, so the only ones the sprite pass draws.
    static ACTIVE: RefCell<HashSet<Entity>> = RefCell::new(HashSet::new());
}

/// Whether `portal` has a view this frame.
pub fn is_active(portal: Entity) -> bool {
    ACTIVE.with(|a| a.borrow().contains(&portal))
}

/// A view per enabled, paired portal near `main`'s view: the partner's front
/// region, drawn for this portal. Every host appends these to its views.
pub fn portal_views(world: &World, main: &Camera2D) -> Vec<crate::ecs::views::ViewCamera> {
    let portals: Vec<(Entity, Portal, MirrorPlace)> = world
        .query::<&Portal>()
        .iter()
        .filter(|(_, p)| p.enabled && p.width > 0.0 && p.depth > 0.0)
        .filter_map(|(e, _)| place_of(world, e).map(|(p, place)| (e, p, place)))
        .collect();
    let corners = main.view_corners();
    let reach = portals.iter().fold(0.0f32, |a, (_, _, p)| a.max(p.depth));
    let lo = corners.iter().fold(Vec2::splat(f32::MAX), |a, c| a.min(*c)) - Vec2::splat(reach);
    let hi = corners.iter().fold(Vec2::splat(f32::MIN), |a, c| a.max(*c)) + Vec2::splat(reach);
    let mut active = HashSet::new();
    let mut views = Vec::new();
    for (e, portal, place) in portals {
        // Only a portal whose window could be on screen draws.
        let near = place.centre.x + place.width >= lo.x
            && place.centre.x - place.width <= hi.x
            && place.centre.y + place.width >= lo.y
            && place.centre.y - place.width <= hi.y;
        if !near {
            continue;
        }
        let Some(other) = partner(world, &portal) else { continue };
        let Some((_, there)) = place_of(world, other) else { continue };
        // The partner's front region, as wide and deep as this window.
        let there = MirrorPlace::at(there.centre, there.rotation, portal.width, portal.depth);
        let texels = |units: f32| ((units * portal.resolution.max(0.01)).round() as u32).clamp(1, MAX_MIRROR_TEXELS);
        active.insert(e);
        views.push(crate::ecs::views::ViewCamera {
            target: target_id(e),
            size: (texels(portal.width), texels(portal.depth)),
            camera: there.camera(portal_key(e)),
            transparent: true,
        });
    }
    ACTIVE.with(|a| *a.borrow_mut() = active);
    views.sort_by(|a, b| a.target.cmp(&b.target));
    views
}

/// Forget this frame's portals (a test or a tool drawing without a host).
pub fn clear_active() {
    ACTIVE.with(|a| a.borrow_mut().clear());
}

/// Carry what crossed a portal's line out of its partner. Called from
/// `tick_motion`.
pub fn tick_portals(world: &mut World) {
    let portals: Vec<(Entity, Portal, MirrorPlace)> = world
        .query::<&Portal>()
        .iter()
        .filter(|(_, p)| p.enabled && p.carry && p.width > 0.0)
        .filter_map(|(e, _)| place_of(world, e).map(|(p, place)| (e, p, place)))
        .collect();
    if portals.is_empty() {
        return;
    }
    // Everything that moves: a body that is not fixed, or anything with a
    // velocity.
    let movers: Vec<(Entity, Vec2)> = world
        .query::<(&Transform, Option<&RigidBody2D>, Option<&Velocity2D>, Option<&Portal>)>()
        .iter()
        .filter(|(_, (_, rb, v, p))| p.is_none() && (rb.is_some_and(|r| r.body_type != BodyType::Static) || v.is_some()))
        .map(|(e, (t, ..))| (e, t.position))
        .collect();
    let mut moves: Vec<(Entity, Vec2, f32)> = Vec::new();
    let mut fresh: Vec<(Entity, HashMap<u64, f32>)> = Vec::new();
    for (pe, portal, place) in &portals {
        let there = partner(world, portal).and_then(|o| place_of(world, o).map(|(_, p)| p));
        let mut seen = HashMap::new();
        for (m, p) in &movers {
            if moves.iter().any(|(e, ..)| e == m) {
                continue;
            }
            let d = *p - place.centre;
            let (u, v) = (d.dot(place.along), d.dot(place.front));
            if u.abs() > place.width * 0.5 + 1.0 || v.abs() > place.depth.max(place.width) {
                continue;
            }
            let key = m.to_bits().get();
            let was = portal.seen.get(&key).copied();
            if let (Some(was), Some(there)) = (was, there.as_ref())
                && was > 0.0
                && v <= 0.0
                && u.abs() <= place.width * 0.5
            {
                let (out, turn) = carry(place, there, *p);
                // Just in front of the partner's line, so it is not caught
                // going back.
                let out = out + there.front * 0.5;
                moves.push((*m, out, turn));
                continue;
            }
            seen.insert(key, v);
        }
        fresh.push((*pe, seen));
    }
    for (pe, seen) in fresh {
        if let Ok(mut p) = world.get::<&mut Portal>(pe) {
            p.seen = seen;
        }
    }
    for (m, out, turn) in moves {
        if let Ok(mut t) = world.get::<&mut Transform>(m) {
            t.position = out;
            t.rotation += turn;
        }
        if let Ok(mut v) = world.get::<&mut Velocity2D>(m) {
            let r = Mat2::from_angle(turn) * Vec2::new(v.x, v.y);
            v.x = r.x;
            v.y = r.y;
        }
    }
}

/// `Portal.set(e, field, value)`: write any field by name.
pub fn set_field(world: &mut World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut p = world.get::<&mut Portal>(entity).map_err(|_| "no portal on that entity".to_string())?;
    let mut whole = serde_json::to_value(&*p).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) && field != "sort_layer" {
        return Err(format!("'{field}' is not a portal field"));
    }
    let value = match (field, value) {
        ("z_order", serde_json::Value::Number(n)) => serde_json::Value::from(n.as_f64().unwrap_or(0.0) as i64),
        _ => value.clone(),
    };
    map.insert(field.to_string(), value);
    let seen = std::mem::take(&mut p.seen);
    *p = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    p.seen = seen;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(world: &mut World) -> (Entity, Entity) {
        // A: a floor-level portal at (-200, 0) facing up; B: a wall portal
        // at (300, 100) facing left (turned a quarter).
        let a = world.spawn((
            Name("A".into()),
            Transform { position: Vec2::new(-200.0, 0.0), ..Default::default() },
            Portal { pair: "B".into(), width: 100.0, ..Default::default() },
        ));
        let b = world.spawn((
            Name("B".into()),
            Transform { position: Vec2::new(300.0, 100.0), rotation: std::f32::consts::FRAC_PI_2, ..Default::default() },
            Portal { pair: "A".into(), width: 100.0, ..Default::default() },
        ));
        (a, b)
    }

    #[test]
    fn a_point_through_one_comes_out_of_the_other_turned_half_round() {
        let a = MirrorPlace::at(Vec2::ZERO, 0.0, 100.0, 50.0);
        let b = MirrorPlace::at(Vec2::new(500.0, 0.0), 0.0, 100.0, 50.0);
        // Just behind A, 20 to its right: out in front of B, 20 to its left.
        let (out, turn) = carry(&a, &b, Vec2::new(20.0, -5.0));
        assert!(out.distance(Vec2::new(480.0, 5.0)) < 1e-4, "{out}");
        assert!((turn - std::f32::consts::PI).abs() < 1e-6, "facing the same way, it turns right round");
        // The window shows the partner's left on this portal's right.
        let (q, uv) = window(&a);
        assert_eq!((q[0], uv[0]), (Vec2::new(-50.0, 0.0), [1.0, 1.0, 1.0]));
    }

    #[test]
    fn a_crate_falling_into_a_floor_portal_flies_out_of_a_wall() {
        let mut world = World::new();
        pair(&mut world);
        let c = world.spawn((
            Transform { position: Vec2::new(-190.0, 30.0), ..Default::default() },
            RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
            Velocity2D { x: 0.0, y: -400.0, ..Default::default() },
        ));
        tick_portals(&mut world);
        // Fall through the floor portal.
        world.get::<&mut Transform>(c).unwrap().position.y = -6.0;
        tick_portals(&mut world);
        let t = (*world.get::<&Transform>(c).unwrap()).clone();
        let v = (*world.get::<&Velocity2D>(c).unwrap()).clone();
        // B faces -x: out to its left, moving left as fast as it fell.
        assert!(t.position.x < 300.0 && (t.position.y - 100.0).abs() < 20.0, "out of the wall portal: {}", t.position);
        assert!((v.x + 400.0).abs() < 1e-2 && v.y.abs() < 1e-2, "falling becomes flying out: {v:?}");
        // And it is not carried straight back.
        tick_portals(&mut world);
        assert!(world.get::<&Transform>(c).unwrap().position.x < 300.0);
    }

    #[test]
    fn beside_or_from_behind_it_carries_nothing() {
        let mut world = World::new();
        pair(&mut world);
        // Falling past A's end, outside its width.
        let c = world.spawn((Transform { position: Vec2::new(-130.0, 10.0), ..Default::default() }, Velocity2D::default()));
        tick_portals(&mut world);
        world.get::<&mut Transform>(c).unwrap().position.y = -10.0;
        tick_portals(&mut world);
        assert_eq!(world.get::<&Transform>(c).unwrap().position.x, -130.0);
        // Coming up from behind A.
        let d = world.spawn((Transform { position: Vec2::new(-200.0, -10.0), ..Default::default() }, Velocity2D::default()));
        tick_portals(&mut world);
        world.get::<&mut Transform>(d).unwrap().position.y = 10.0;
        tick_portals(&mut world);
        assert_eq!(world.get::<&Transform>(d).unwrap().position.x, -200.0);
    }

    #[test]
    fn views_look_out_of_the_partner() {
        let mut world = World::new();
        let (a, _) = pair(&mut world);
        let main = Camera2D::new(1280.0, 720.0);
        let views = portal_views(&world, &main);
        assert_eq!(views.len(), 2);
        let v = views.iter().find(|v| v.target == target_id(a)).unwrap();
        // A's view is centred in front of B (B faces -x): left of it.
        assert!(v.camera.position.x < 300.0 && (v.camera.position.y - 100.0).abs() < 1e-3, "{}", v.camera.position);
        assert!(is_active(a));
        clear_active();
    }
}
