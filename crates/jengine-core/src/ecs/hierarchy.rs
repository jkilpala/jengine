//! Entity-parenting / transform hierarchy resolver.
//!
//! Foundation slice of `docs/feature-backlog.md` item 3.5 (skeletal
//! animation). The contract:
//!
//! - Only entities in a hierarchy - those with a [`Parent`], and the entities
//!   named as parents - carry a [`WorldTransform`], inserted by
//!   [`resolve_world_transforms`]. Everything else's world transform *is* its
//!   `Transform`; read either through [`world_transform_of`], which falls back.
//!
//! - For entities with a [`Parent`], `WorldTransform` is the product of the
//!   parent's resolved `WorldTransform` and the child's local `Transform`,
//!   as affine maps ([`compose`]). Usually that is plain TRS:
//!     - `position = parent.position + Rotate(parent.rotation, local.position * parent.scale)`
//!     - `rotation = parent.rotation + local.rotation`
//!     - `scale    = parent.scale * local.scale`
//!   but a turned child under a non-uniformly scaled parent comes out
//!   **sheared**, which `WorldTransform::shear` carries.
//!
//! - Cycles and dangling parent references are detected and logged; the
//!   offending child falls back to `WorldTransform == Transform` for the
//!   frame so a broken scene never panics.
//!
//! Performance: the resolver is O(N + E) where N is the entity count and
//! E is the parent-link count. We build a per-frame topological order by
//! repeatedly resolving entities whose parent has already been resolved
//! (worst case `depth` passes). For shallow scene graphs (the common
//! case) it converges in one or two iterations.

use std::collections::{HashMap, HashSet};

use hecs::{Entity, World};

use super::components::{Parent, Transform, WorldTransform};

/// The name a part goes by under its parent: what follows the last `/`. A
/// scene names a prefab's parts `Cart/Wheel` (`scene::prefab::flatten_children`)
/// so two carts never share a wheel's name; a script spawn names them
/// `Wheel`. Either way `Entity.find_child(cart, "Wheel")` finds it.
pub fn part_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// `Entity.find_child`: the part of `parent` called `path` - its children
/// first, then theirs (breadth first, each level in spawn order), matched by
/// [`part_name`]. A path with `/` in it (`"Arm/Bucket"`) is walked a
/// segment at a time, each from the last one found.
pub fn find_child(world: &World, parent: Entity, path: &str) -> Option<Entity> {
    let links: Vec<(Entity, Entity, String)> = world
        .query::<(&Parent, &super::components::Name)>()
        .iter()
        .map(|(e, (p, n))| (e, p.0, part_name(&n.0).to_string()))
        .collect();
    let mut at = parent;
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        let mut level = vec![at];
        let mut found = None;
        while found.is_none() && !level.is_empty() {
            let mut next: Vec<Entity> = links
                .iter()
                .filter(|(_, p, _)| level.contains(p))
                .map(|(e, _, _)| *e)
                .collect();
            next.sort_by_key(|e| e.id());
            found = next
                .iter()
                .copied()
                .find(|e| links.iter().any(|(c, _, n)| c == e && n == segment));
            level = next;
        }
        at = found?;
    }
    (at != parent).then_some(at)
}

/// Compose a child's local [`Transform`] under a parent's already-resolved
/// [`WorldTransform`]. Pure; safe to call standalone (used by tests).
///
/// The child's position goes through the parent's whole linear map. Its
/// rotation and scale are plain sums and products - the TRS rule - whenever
/// that is exact: the parent is not sheared, and either the child is not
/// turned or the parent scales both axes by the same amount. Otherwise the
/// product of the two maps is decomposed back into rotation, shear and scale,
/// keeping the scale's x sign where the TRS rule would have put it (so a
/// mirrored parent still reads as mirrored).
#[inline]
pub fn compose(parent: &WorldTransform, local: &Transform) -> WorldTransform {
    let position = parent.apply(local.position);
    let uniform = parent.scale.x.abs() == parent.scale.y.abs();
    if parent.shear == 0.0 && (local.rotation == 0.0 || uniform) {
        return WorldTransform {
            position,
            rotation: parent.rotation + local.rotation,
            scale: parent.scale * local.scale,
            shear: 0.0,
        };
    }
    // The product's two columns: the child's own axes, scaled and turned,
    // then carried through the parent.
    let (sin_c, cos_c) = local.rotation.sin_cos();
    let a = parent.apply_linear(glam::Vec2::new(cos_c, sin_c) * local.scale.x);
    let b = parent.apply_linear(glam::Vec2::new(-sin_c, cos_c) * local.scale.y);
    let len = a.length();
    if len < 1e-9 {
        // A collapsed axis has no direction to decompose; draw it as TRS.
        return WorldTransform {
            position,
            rotation: parent.rotation + local.rotation,
            scale: parent.scale * local.scale,
            shear: 0.0,
        };
    }
    // a = R(rotation) * (scale.x, 0).
    let flip = parent.scale.x * local.scale.x < 0.0;
    let (sx, rotation) = if flip {
        (-len, (-a.y).atan2(-a.x))
    } else {
        (len, a.y.atan2(a.x))
    };
    // R(-rotation) * b = (shear * scale.y, scale.y).
    let (sin_r, cos_r) = rotation.sin_cos();
    let rb = glam::Vec2::new(b.x * cos_r + b.y * sin_r, -b.x * sin_r + b.y * cos_r);
    let sy = rb.y;
    let shear = if sy.abs() > 1e-9 { rb.x / sy } else { 0.0 };
    WorldTransform { position, rotation, scale: glam::Vec2::new(sx, sy), shear }
}

/// Where an entity is in the world, given the two components a query can
/// carry: its resolved [`WorldTransform`] when it is in a hierarchy, else its
/// own [`Transform`].
///
/// This is the renderer's rule, and the one every gameplay system has to
/// share. Until Sep 2026 the camera follow, trigger areas, hitbox knockback,
/// health bars and name tags all read `Transform.position` - so a parented
/// entity was *drawn* where its parent carried it and *tested* at its local
/// offset: a trigger on a child fired around the parent's origin, a camera
/// following a child followed the parent. Query
/// `Option<&WorldTransform>` beside the `Transform` and pass both here.
#[inline]
pub fn world_position(local: &Transform, world: Option<&WorldTransform>) -> glam::Vec2 {
    world.map_or(local.position, |w| w.position)
}

/// An entity's world transform, derived from its local one when it has no
/// `WorldTransform` component.
///
/// Only entities in a hierarchy carry the component (see
/// [`resolve_world_transforms`]), so every consumer needs this fallback. The
/// sprite renderers and the script bridge already had their own; IK and
/// skinning did not — one silently skipped the bone's influence and the other
/// unwrapped, so the same missing component meant a subtly wrong pose in one
/// place and a panic in the other. One accessor, one behaviour.
pub fn world_transform_of(world: &World, entity: Entity) -> Option<WorldTransform> {
    if let Ok(w) = world.get::<&WorldTransform>(entity) {
        return Some((*w).clone());
    }
    world
        .get::<&Transform>(entity)
        .ok()
        .map(|t| WorldTransform::from(&*t))
}

/// A part goes with its parent (Oct 2026): every entity whose [`Parent`] has
/// been despawned is despawned too, and its own parts after it. Nothing that
/// destroys an entity - `Entity.destroy`, a `lifetime`, `despawn_offscreen`,
/// a behaviour graph - walks its children, so before this a destroyed cart
/// left its wheels drawn at their local offsets about the origin, and a
/// warning logged for each of them every frame. Called at the start of
/// [`resolve_world_transforms`], which every host runs each frame. A parent
/// that is alive but has no `Transform` is not an orphan's: that child still
/// falls back to its local transform.
pub fn despawn_orphans(world: &mut World) {
    loop {
        let orphans: Vec<Entity> = world
            .query::<&Parent>()
            .iter()
            .filter(|(_, p)| !world.contains(p.0))
            .map(|(e, _)| e)
            .collect();
        if orphans.is_empty() {
            return;
        }
        for e in orphans {
            let _ = world.despawn(e);
        }
    }
}

/// Refresh the [`WorldTransform`] of every entity in a hierarchy from its
/// [`Transform`] and [`Parent`] link. Call once per frame, before any
/// rendering system that consumes `WorldTransform` (sprite renderers do).
pub fn resolve_world_transforms(world: &mut World) {
    let _span = crate::profiling::trace::span("hierarchy");
    despawn_orphans(world);
    // 0. Only entities in a hierarchy need a `WorldTransform` at all.
    //
    //    This used to run over the whole world and `insert_one` a
    //    `WorldTransform` onto every entity every frame - including the vast
    //    majority with no `Parent`, where it is a verbatim copy of the
    //    `Transform` and has no consumer: every reader either falls back to
    //    `WorldTransform::from(transform)` (the sprite renderers, the script
    //    bridge's `get_world_position`) or only ever looks at bones, which are
    //    parented by construction. Measured on flat worlds: 0.56 ms/frame at
    //    8,000 entities and 2.39 ms at 16,000, all of it wasted.
    //
    //    Participants are entities with a `Parent` *and* the entities they
    //    name as parents - a parent needs a resolved transform for its
    //    children to be placed against, and `hierarchy_integration` reads it
    //    directly.
    let mut participants: HashSet<Entity> = HashSet::new();
    for (e, p) in world.query::<&Parent>().iter() {
        participants.insert(e);
        participants.insert(p.0);
    }

    // An entity that *stops* being parented would otherwise keep the
    // `WorldTransform` it had when it left, and renderers prefer that
    // component over the fallback - so it would silently freeze at its last
    // hierarchical position. Sweep those off. Cheap: after this change only
    // participants carry one.
    let stale: Vec<Entity> = world
        .query::<&WorldTransform>()
        .iter()
        .map(|(e, _)| e)
        .filter(|e| !participants.contains(e))
        .collect();
    for e in stale {
        let _ = world.remove_one::<WorldTransform>(e);
    }

    if participants.is_empty() {
        return;
    }

    // 1. Pull (entity, local Transform, optional parent Entity) into a
    //    Vec we can iterate freely without holding a query borrow on the
    //    world (we need `world.insert_one` calls below).
    let mut entries: Vec<(Entity, Transform, Option<Entity>)> = world
        .query::<(&Transform, Option<&Parent>)>()
        .iter()
        .filter(|(e, _)| participants.contains(e))
        .map(|(e, (t, p))| (e, t.clone(), p.map(|p| p.0)))
        .collect();

    // 2. Resolve iteratively. `resolved` holds the final WorldTransform
    //    for each entity once available. Passes terminate when no
    //    progress is made; remaining unresolved entries indicate a cycle
    //    or a dangling parent and are forced to local == world.
    let mut resolved: HashMap<Entity, WorldTransform> = HashMap::with_capacity(entries.len());
    let mut pending: Vec<(Entity, Transform, Option<Entity>)> = Vec::with_capacity(entries.len());

    loop {
        let mut progress = false;
        pending.clear();
        for (e, t, parent) in entries.drain(..) {
            match parent {
                None => {
                    resolved.insert(e, WorldTransform::from(&t));
                    progress = true;
                }
                Some(p) => {
                    if let Some(pw) = resolved.get(&p) {
                        let w = compose(pw, &t);
                        resolved.insert(e, w);
                        progress = true;
                    } else {
                        pending.push((e, t, Some(p)));
                    }
                }
            }
        }
        std::mem::swap(&mut entries, &mut pending);
        if entries.is_empty() {
            break;
        }
        if !progress {
            // Either a cycle, or all remaining parents reference entities
            // outside the `Transform` query (dangling). Force-resolve so
            // the frame still renders, log once, and bail.
            for (e, t, parent) in entries.drain(..) {
                log::warn!(
                    "resolve_world_transforms: entity {:?} has unresolvable parent {:?} \
                     (cycle or dangling reference) — falling back to local transform",
                    e,
                    parent
                );
                resolved.insert(e, WorldTransform::from(&t));
            }
            break;
        }
    }

    // 3. Commit the resolved WorldTransforms back into the world. Insert
    //    creates the component if absent, replaces it if present.
    for (e, w) in resolved {
        // `insert_one` ignores entities that no longer exist (shouldn't
        // happen mid-frame, but be defensive against future restructuring).
        let _ = world.insert_one(e, w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::{Parent, Transform};
    use glam::Vec2;

    #[test]
    fn root_world_equals_local() {
        // Contract change: an entity outside any hierarchy no longer *carries*
        // a `WorldTransform`. Writing one per entity per frame cost 2.39 ms at
        // 16,000 entities for a verbatim copy of the `Transform` that every
        // consumer can derive. The value is still available - through
        // `world_transform_of`, which is what consumers use.
        let mut world = World::new();
        let e = world.spawn((Transform {
            position: Vec2::new(3.0, 4.0),
            rotation: 0.5,
            scale: Vec2::new(2.0, 2.0),
        },));
        resolve_world_transforms(&mut world);

        assert!(
            world.get::<&WorldTransform>(e).is_err(),
            "a lone entity should not be given a WorldTransform component"
        );

        let w = world_transform_of(&world, e).expect("derived from the local transform");
        assert_eq!(w.position, Vec2::new(3.0, 4.0));
        assert!((w.rotation - 0.5).abs() < 1e-6);
        assert_eq!(w.scale, Vec2::new(2.0, 2.0));
    }

    #[test]
    fn child_inherits_parent_translation() {
        let mut world = World::new();
        let parent = world.spawn((Transform {
            position: Vec2::new(10.0, 20.0),
            rotation: 0.0,
            scale: Vec2::ONE,
        },));
        let child = world.spawn((
            Transform {
                position: Vec2::new(5.0, 0.0),
                rotation: 0.0,
                scale: Vec2::ONE,
            },
            Parent(parent),
        ));
        resolve_world_transforms(&mut world);
        let w = world.get::<&WorldTransform>(child).unwrap();
        assert_eq!(w.position, Vec2::new(15.0, 20.0));
    }

    #[test]
    fn child_inherits_parent_rotation_and_scale() {
        let mut world = World::new();
        // Parent at (10, 0) rotated 90° (counter-clockwise) with 2x scale.
        let parent = world.spawn((Transform {
            position: Vec2::new(10.0, 0.0),
            rotation: std::f32::consts::FRAC_PI_2,
            scale: Vec2::new(2.0, 2.0),
        },));
        let child = world.spawn((
            Transform {
                position: Vec2::new(3.0, 0.0),
                rotation: 0.0,
                scale: Vec2::ONE,
            },
            Parent(parent),
        ));
        resolve_world_transforms(&mut world);
        let w = world.get::<&WorldTransform>(child).unwrap();
        // (3,0) scaled by 2 -> (6,0); rotated 90° -> (0, 6); plus parent
        // pos (10, 0) -> (10, 6).
        assert!((w.position.x - 10.0).abs() < 1e-5, "got x = {}", w.position.x);
        assert!((w.position.y - 6.0).abs() < 1e-5, "got y = {}", w.position.y);
        assert!((w.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert_eq!(w.scale, Vec2::new(2.0, 2.0));
    }

    #[test]
    fn grandchild_resolves_through_multiple_passes() {
        let mut world = World::new();
        let a = world.spawn((Transform {
            position: Vec2::new(1.0, 0.0),
            rotation: 0.0,
            scale: Vec2::ONE,
        },));
        let b = world.spawn((
            Transform {
                position: Vec2::new(2.0, 0.0),
                rotation: 0.0,
                scale: Vec2::ONE,
            },
            Parent(a),
        ));
        let c = world.spawn((
            Transform {
                position: Vec2::new(4.0, 0.0),
                rotation: 0.0,
                scale: Vec2::ONE,
            },
            Parent(b),
        ));
        resolve_world_transforms(&mut world);
        assert_eq!(world.get::<&WorldTransform>(c).unwrap().position, Vec2::new(7.0, 0.0));
    }

    #[test]
    fn a_part_goes_with_its_parent() {
        let mut world = World::new();
        let ghost = world.spawn((Transform::default(),));
        let child = world.spawn((Transform::default(), Parent(ghost)));
        let grandchild = world.spawn((Transform::default(), Parent(child)));
        let bystander = world.spawn((Transform::default(),));
        world.despawn(ghost).unwrap();
        resolve_world_transforms(&mut world);
        assert!(!world.contains(child) && !world.contains(grandchild), "the parts went with it");
        assert!(world.contains(bystander));
    }

    #[test]
    fn a_parent_without_a_transform_falls_back_to_local() {
        let mut world = World::new();
        let bare = world.spawn((super::super::components::Name("bare".into()),));
        let child = world.spawn((
            Transform {
                position: Vec2::new(42.0, 0.0),
                rotation: 0.0,
                scale: Vec2::ONE,
            },
            Parent(bare),
        ));
        // Should not panic.
        resolve_world_transforms(&mut world);
        let w = world.get::<&WorldTransform>(child).unwrap();
        assert_eq!(w.position, Vec2::new(42.0, 0.0));
    }
}
