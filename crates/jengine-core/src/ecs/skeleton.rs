//! Skeleton traversal helpers (slice 3.5.B).
//!
//! Skeletons are entity-parented trees of [`crate::ecs::components::Bone`]
//! components rooted at an entity carrying
//! [`crate::ecs::components::SkeletonRoot`]. This module provides
//! lookup-by-name helpers used by scripts, animation clips (slice C),
//! and IK constraints (slice F).

use hecs::{Entity, World};

use crate::ecs::components::{Bone, Parent, SkeletonRoot};

/// Returns true if `entity` is a descendant of `root` via the
/// [`Parent`] chain. Includes `entity == root`.
fn is_descendant_of(world: &World, entity: Entity, root: Entity) -> bool {
    let mut current = entity;
    // Hard cap to defend against malformed parent cycles. Real skeletons
    // are shallow; 1024 is generous and bounds the iteration cost.
    for _ in 0..1024 {
        if current == root {
            return true;
        }
        match world.get::<&Parent>(current) {
            Ok(p) => current = p.0,
            Err(_) => return false,
        }
    }
    log::warn!(
        "is_descendant_of: walk exceeded 1024 ancestors from {:?} — assuming cycle",
        entity
    );
    false
}

/// Find the first bone named `bone_name` that is a descendant of the
/// `root` skeleton entity. Returns `None` if no such bone exists.
///
/// `root` does not need to itself carry [`SkeletonRoot`] — callers
/// already passing a known root entity get the lookup directly. To
/// validate that an entity is a skeleton root, query
/// `world.get::<&SkeletonRoot>(root)` separately.
///
/// Note: this is an O(N · D) scan where N is the entity count carrying
/// `Bone` and D is the average ancestor depth from those bones. Skeletons
/// are typically tiny (~50 bones, depth 5) so this is fine; if a hot
/// callsite needs faster lookup, cache `(bone_name → Entity)` in user
/// code or wait for slice C's `SkeletonInstance` cache.
pub fn find_bone(world: &World, root: Entity, bone_name: &str) -> Option<Entity> {
    for (entity, bone) in world.query::<&Bone>().iter() {
        if bone.name == bone_name && is_descendant_of(world, entity, root) {
            return Some(entity);
        }
    }
    None
}

/// Collect every bone (entity + name) that is a descendant of the
/// given skeleton root. Useful for editor inspectors and tests.
pub fn collect_bones(world: &World, root: Entity) -> Vec<(Entity, String)> {
    let mut out = Vec::new();
    for (entity, bone) in world.query::<&Bone>().iter() {
        if is_descendant_of(world, entity, root) {
            out.push((entity, bone.name.clone()));
        }
    }
    out
}

/// Locate the first skeleton root in the world whose
/// [`SkeletonRoot::name`] equals `skeleton_name`.
pub fn find_skeleton(world: &World, skeleton_name: &str) -> Option<Entity> {
    world
        .query::<&SkeletonRoot>()
        .iter()
        .find(|(_, s)| s.name == skeleton_name)
        .map(|(e, _)| e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::{Bone, Parent, SkeletonRoot, Transform};

    fn make_rig() -> (World, Entity, Entity, Entity, Entity) {
        // root -> pelvis -> spine -> head
        //                  \-> arm
        let mut w = World::new();
        let root = w.spawn((Transform::default(), SkeletonRoot { name: "hero".into() }));
        let pelvis = w.spawn((
            Transform::default(),
            Parent(root),
            Bone {
                name: "pelvis".into(),
                length: 1.0,
            },
        ));
        let spine = w.spawn((
            Transform::default(),
            Parent(pelvis),
            Bone {
                name: "spine".into(),
                length: 1.5,
            },
        ));
        let head = w.spawn((
            Transform::default(),
            Parent(spine),
            Bone {
                name: "head".into(),
                length: 0.5,
            },
        ));
        let arm = w.spawn((
            Transform::default(),
            Parent(spine),
            Bone {
                name: "arm".into(),
                length: 2.0,
            },
        ));
        (w, root, pelvis, head, arm)
    }

    #[test]
    fn find_bone_returns_descendant_by_name() {
        let (w, root, _pelvis, head, _arm) = make_rig();
        assert_eq!(find_bone(&w, root, "head"), Some(head));
    }

    #[test]
    fn find_bone_returns_none_for_unknown_name() {
        let (w, root, _, _, _) = make_rig();
        assert_eq!(find_bone(&w, root, "tail"), None);
    }

    #[test]
    fn find_bone_excludes_other_skeletons() {
        let (mut w, root_a, _, _, _) = make_rig();
        let root_b = w.spawn((Transform::default(), SkeletonRoot { name: "villain".into() }));
        let stranger = w.spawn((
            Transform::default(),
            Parent(root_b),
            Bone {
                name: "head".into(),
                length: 0.5,
            },
        ));
        // Searching root_a must not return root_b's head.
        let found = find_bone(&w, root_a, "head").unwrap();
        assert_ne!(found, stranger);
    }

    #[test]
    fn collect_bones_lists_all_descendants() {
        let (w, root, _, _, _) = make_rig();
        let mut names: Vec<String> = collect_bones(&w, root)
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        names.sort();
        assert_eq!(names, vec!["arm", "head", "pelvis", "spine"]);
    }

    #[test]
    fn find_skeleton_returns_matching_root() {
        let (w, root, _, _, _) = make_rig();
        assert_eq!(find_skeleton(&w, "hero"), Some(root));
        assert_eq!(find_skeleton(&w, "nope"), None);
    }
}
