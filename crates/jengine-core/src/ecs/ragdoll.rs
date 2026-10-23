//! Ragdolls (Oct 2026; Unity's ragdoll wizard, Godot's `PhysicalBone2D`): a
//! skeleton handed to physics.
//!
//! A [`Ragdoll`] sits on a skeleton's root. While it is `active` every
//! [`Bone`] under that root is a dynamic capsule in rapier - as long as the
//! bone and `thickness` across - and each bone is pinned to its parent bone
//! where it starts by a hinge that bends at most `limit` either way from the
//! angle it had when the skeleton went limp. Physics poses the bones and the
//! world sees them as ordinary bone transforms, so a skin, a sprite on a hand
//! and the hierarchy follow without knowing. Off again, the bodies go and the
//! skeleton is the animator's: it keeps the pose it fell into until a clip
//! moves it.
//!
//! Building and dropping the bodies is
//! [`crate::physics::PhysicsWorld::sync_ragdolls`], called by
//! `ensure_physics_bodies`; writing the pose back rides on
//! `sync_transforms_to_ecs` and `blend_transforms_for_render` - so, like every
//! physics feature, no host is wired. A limp skeleton's players, IK and jiggle
//! are skipped ([`is_limp`]), or the animator would pose the bones back every
//! frame. The parts of one ragdoll do not collide with each other (a shoulder
//! capsule overlaps its chest at rest); they collide with everything else,
//! other ragdolls included.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Bone, Parent, Transform, WorldTransform};
use crate::ecs::hierarchy;

/// A skeleton that can go limp. On the skeleton's root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ragdoll {
    /// Limp now: the bones are bodies and physics poses them.
    pub active: bool,
    /// How thick each bone's capsule is, in world units.
    pub thickness: f32,
    /// How far each hinge bends either way from its angle when the skeleton
    /// went limp, in radians. Past pi a hinge is free.
    pub limit: f32,
    /// Mass per unit of area, as a collider's `density`.
    pub density: f32,
    pub friction: f32,
    /// How fast the bones lose their speed and spin, per second - a little
    /// keeps a heap from twitching for ever.
    pub damping: f32,
}

impl Default for Ragdoll {
    fn default() -> Self {
        Self { active: false, thickness: 12.0, limit: 1.2, density: 1.0, friction: 0.7, damping: 0.4 }
    }
}

/// One bone of a skeleton going limp, parents before children.
#[derive(Debug, Clone)]
pub struct LimpBone {
    pub entity: Entity,
    /// Index of the parent bone in the same list; `None` for a top bone,
    /// which hangs from nothing.
    pub parent: Option<usize>,
    /// Where the bone is in the world now.
    pub world: WorldTransform,
    /// The bone's `length`, before scale.
    pub length: f32,
}

/// Every bone under `root` (and `root` itself, if it is one), parents first,
/// each with its world transform composed from the root down - fresh, rather
/// than last frame's `WorldTransform`.
pub fn bones_of(world: &World, root: Entity) -> Vec<LimpBone> {
    let mut children: std::collections::HashMap<Entity, Vec<Entity>> = std::collections::HashMap::new();
    for (e, (_, p)) in world.query::<(&Bone, &Parent)>().iter() {
        children.entry(p.0).or_default().push(e);
    }
    let Some(root_world) = hierarchy::world_transform_of(world, root) else { return Vec::new() };
    let mut out: Vec<LimpBone> = Vec::new();
    // (entity, its world transform, the bone index it hangs from)
    let mut stack: Vec<(Entity, WorldTransform, Option<usize>)> = Vec::new();
    if let Ok(b) = world.get::<&Bone>(root) {
        out.push(LimpBone { entity: root, parent: None, world: root_world.clone(), length: b.length });
        stack.push((root, root_world, Some(0)));
    } else {
        stack.push((root, root_world, None));
    }
    while let Some((e, at, bone)) = stack.pop() {
        let mut kids = children.get(&e).cloned().unwrap_or_default();
        // Stable order whatever the query gave.
        kids.sort_by_key(|k| k.id());
        for k in kids {
            let Ok(t) = world.get::<&Transform>(k) else { continue };
            let w = hierarchy::compose(&at, &t);
            let length = world.get::<&Bone>(k).map(|b| b.length).unwrap_or(0.0);
            out.push(LimpBone { entity: k, parent: bone, world: w.clone(), length });
            let index = out.len() - 1;
            stack.push((k, w, Some(index)));
        }
        if out.len() > 512 {
            break;
        }
    }
    out
}

/// The local transform that puts a child at world `(position, rotation)`
/// under `parent` - [`hierarchy::compose`]'s plain rule turned round, which
/// is exact for a parent that is not sheared and scales both axes by the
/// same amount (the skeletons this is for).
pub fn local_under(parent: &WorldTransform, position: Vec2, rotation: f32) -> (Vec2, f32) {
    let d = position - parent.position;
    let (sin, cos) = parent.rotation.sin_cos();
    let r = Vec2::new(d.x * cos + d.y * sin, -d.x * sin + d.y * cos);
    let s = Vec2::new(
        if parent.scale.x.abs() > 1e-9 { parent.scale.x } else { 1.0 },
        if parent.scale.y.abs() > 1e-9 { parent.scale.y } else { 1.0 },
    );
    (r / s, rotation - parent.rotation)
}

/// Whether some ragdoll in the world is limp - the cheap test that keeps
/// [`is_limp`] free in a world without one.
pub fn any_limp(world: &World) -> bool {
    world.query::<&Ragdoll>().iter().any(|(_, r)| r.active)
}

/// Whether `entity` is, or hangs under, a limp ragdoll's root - so its pose is
/// physics's, and an animator must leave it alone.
pub fn is_limp(world: &World, mut entity: Entity) -> bool {
    for _ in 0..64 {
        if world.get::<&Ragdoll>(entity).is_ok_and(|r| r.active) {
            return true;
        }
        match world.get::<&Parent>(entity) {
            Ok(p) => entity = p.0,
            Err(_) => return false,
        }
    }
    false
}

/// `Ragdoll.set(e, field, value)`: write any field by name - `"active"` to go
/// limp or stand up, `"limit"`, `"damping"`. A change other than `active`
/// takes effect the next time the skeleton goes limp.
pub fn set_ragdoll_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let Ok(mut c) = world.get::<&mut Ragdoll>(entity) else {
        return Err("no ragdoll on that entity".into());
    };
    let mut whole = serde_json::to_value(&*c).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a ragdoll field"));
    }
    // Scripts may say 1 / 0 for a flag.
    let v = match (map.get(field), value) {
        (Some(serde_json::Value::Bool(_)), serde_json::Value::Number(n)) => serde_json::Value::Bool(n.as_f64().unwrap_or(0.0) != 0.0),
        _ => value.clone(),
    };
    map.insert(field.to_string(), v);
    *c = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_under_undoes_compose() {
        let parent = WorldTransform { position: Vec2::new(30.0, -12.0), rotation: 0.7, scale: Vec2::new(2.0, 2.0), shear: 0.0 };
        let local = Transform { position: Vec2::new(14.0, 3.0), rotation: -0.4, scale: Vec2::ONE, ..Default::default() };
        let w = hierarchy::compose(&parent, &local);
        let (p, r) = local_under(&parent, w.position, w.rotation);
        assert!((p - local.position).length() < 1e-4, "{p:?}");
        assert!((r - local.rotation).abs() < 1e-5);
    }

    #[test]
    fn bones_come_parents_first() {
        let mut world = World::new();
        let root = world.spawn((Transform::default(), Ragdoll::default()));
        let a = world.spawn((Transform::default(), Bone { name: "a".into(), length: 10.0 }, Parent(root)));
        let b = world.spawn((
            Transform { position: Vec2::new(10.0, 0.0), ..Default::default() },
            Bone { name: "b".into(), length: 10.0 },
            Parent(a),
        ));
        let bones = bones_of(&world, root);
        assert_eq!(bones.len(), 2);
        assert_eq!(bones[0].entity, a);
        assert_eq!(bones[0].parent, None);
        assert_eq!(bones[1].entity, b);
        assert_eq!(bones[1].parent, Some(0));
        assert!((bones[1].world.position.x - 10.0).abs() < 1e-5);
        assert!(!is_limp(&world, b));
        world.get::<&mut Ragdoll>(root).unwrap().active = true;
        assert!(is_limp(&world, b));
    }
}
