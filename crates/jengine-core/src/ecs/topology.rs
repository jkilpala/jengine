//! A cheap fingerprint of everything the editor's Scene panel can display.
//!
//! # Why this exists
//!
//! The editor asks the viewport for its entity list on a timer. Building that
//! answer walks the world, gathers four component sets, formats a name per
//! entity and serializes the lot to JSON — and only then compares the result
//! against the previous one to decide whether anything changed. Since the list
//! changes only when something spawns, dies, is renamed or is reparented, the
//! answer is almost always "no", and the whole snapshot was built to discover
//! that.
//!
//! Measured at 16,000 entities that snapshot was the sawtooth in the editor's
//! frame graph (`docs/architecture-debt.md` #7). This is the cheap question
//! asked first: fingerprint the *topology*, and if it is unchanged, skip the
//! snapshot entirely.
//!
//! # What it covers, and what it deliberately does not
//!
//! Covered, because the panel shows it:
//!
//! * which entities exist, by [`hecs::Entity::to_bits`] — so a despawn and a
//!   respawn into the same id still registers, since the generation is part of
//!   the bits;
//! * which component *types* each entity carries — the panel's row icon and
//!   its "internal" marker are derived from those, and hashing the types
//!   catches an add and a remove in the same frame, which a component *count*
//!   would not;
//! * every entity's [`Name`] and [`Parent`].
//!
//! Deliberately not covered: component *values*. A moving entity must not
//! invalidate the list — motion is the common case, and rebuilding the panel
//! every frame because a boid moved is exactly the cost being removed here.
//!
//! # Fingerprint, not equality
//!
//! A 64-bit hash can collide, and a collision here means a stale panel until
//! the next real change. The inputs are entity bits, type ids and authored
//! names — not attacker-controlled — and a collision needs two *different*
//! topologies to hash alike, which for a change as small as one spawn is a
//! 2⁻⁶⁴ event. The alternative, keeping the previous topology around to compare
//! against, costs the allocation this exists to avoid.

use hecs::World;
use std::hash::{Hash, Hasher};

use crate::ecs::components::{Name, Parent};

/// Hash of everything the Scene panel derives from the world's shape.
///
/// `O(entities + components)` with no allocation and no formatting, against the
/// snapshot's hash sets, per-entity name clone and serde pass.
///
/// Equal fingerprints mean the panel's contents would be unchanged; unequal
/// means rebuild. See the module docs for the collision argument.
pub fn topology_fingerprint(world: &World) -> u64 {
    // Fixed-seed hasher: values are only ever compared against another value
    // from the same process, but determinism also lets tests assert on it.
    let mut h = std::collections::hash_map::DefaultHasher::new();

    // The world iterates in archetype order, which itself shifts when a
    // component is added or removed even if nothing else changed. That is a
    // change the panel cares about anyway, so the order sensitivity is free.
    let mut count = 0u64;
    for e in world.iter() {
        e.entity().to_bits().hash(&mut h);
        for t in e.component_types() {
            t.hash(&mut h);
        }
        count += 1;
    }
    // Implied by the loop, but hashing it explicitly means a prefix of a longer
    // world can never hash to the same value as the whole of a shorter one.
    count.hash(&mut h);

    for (e, name) in world.query::<&Name>().iter() {
        e.to_bits().hash(&mut h);
        name.0.hash(&mut h);
    }
    for (e, parent) in world.query::<&Parent>().iter() {
        e.to_bits().hash(&mut h);
        parent.0.to_bits().hash(&mut h);
    }

    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::{SpriteComponent, Transform};
    use glam::Vec2;

    fn world_with(n: usize) -> World {
        let mut w = World::new();
        for i in 0..n {
            w.spawn((Transform::default(), Name(format!("E{i}"))));
        }
        w
    }

    #[test]
    fn an_unchanged_world_fingerprints_the_same() {
        let w = world_with(50);
        assert_eq!(topology_fingerprint(&w), topology_fingerprint(&w));
    }

    /// The whole point: a moving world is the common case, and it must not look
    /// like a changed one or the snapshot gets rebuilt every frame regardless.
    #[test]
    fn moving_entities_does_not_change_the_fingerprint() {
        let w = world_with(50);
        let before = topology_fingerprint(&w);
        for (_, t) in w.query::<&mut Transform>().iter() {
            t.position += Vec2::new(1.0, -2.0);
        }
        assert_eq!(topology_fingerprint(&w), before, "motion invalidated the list");
    }

    #[test]
    fn spawning_changes_the_fingerprint() {
        let mut w = world_with(50);
        let before = topology_fingerprint(&w);
        w.spawn((Transform::default(), Name("new".into())));
        assert_ne!(topology_fingerprint(&w), before);
    }

    #[test]
    fn despawning_changes_the_fingerprint() {
        let mut w = world_with(50);
        let victim = w.iter().map(|e| e.entity()).next().expect("entity");
        let before = topology_fingerprint(&w);
        w.despawn(victim).expect("despawn");
        assert_ne!(topology_fingerprint(&w), before);
    }

    /// hecs reuses ids, so a despawn followed by a spawn can land on the same
    /// id with a bumped generation. `to_bits` carries the generation; hashing
    /// `id()` alone would have called that world unchanged.
    #[test]
    fn a_respawn_into_a_reused_id_still_registers() {
        let mut w = World::new();
        let a = w.spawn((Transform::default(), Name("a".into())));
        let before = topology_fingerprint(&w);
        w.despawn(a).expect("despawn");
        let b = w.spawn((Transform::default(), Name("a".into())));
        assert_eq!(a.id(), b.id(), "test assumes hecs reuses the id");
        assert_ne!(topology_fingerprint(&w), before, "generation was not hashed");
    }

    #[test]
    fn renaming_changes_the_fingerprint() {
        let w = world_with(10);
        let e = w.iter().map(|e| e.entity()).next().expect("entity");
        let before = topology_fingerprint(&w);
        w.get::<&mut Name>(e).expect("name").0 = "renamed".into();
        assert_ne!(topology_fingerprint(&w), before);
    }

    #[test]
    fn reparenting_changes_the_fingerprint() {
        let mut w = World::new();
        let a = w.spawn((Transform::default(),));
        let b = w.spawn((Transform::default(),));
        let c = w.spawn((Transform::default(), Parent(a)));
        let before = topology_fingerprint(&w);
        w.get::<&mut Parent>(c).expect("parent").0 = b;
        assert_ne!(topology_fingerprint(&w), before);
    }

    /// The panel's row icon comes from which components an entity carries, so
    /// gaining one has to invalidate even though nothing else moved.
    #[test]
    fn adding_a_component_changes_the_fingerprint() {
        let mut w = world_with(10);
        let e = w.iter().map(|e| e.entity()).next().expect("entity");
        let before = topology_fingerprint(&w);
        w.insert_one(e, SpriteComponent::default()).expect("insert");
        assert_ne!(topology_fingerprint(&w), before);
    }

    /// Hashing a component *count* would miss this; hashing the type ids does
    /// not.
    #[test]
    fn swapping_one_component_for_another_changes_the_fingerprint() {
        let mut w = World::new();
        let e = w.spawn((Transform::default(), SpriteComponent::default()));
        let before = topology_fingerprint(&w);
        w.remove_one::<SpriteComponent>(e).expect("remove");
        w.insert_one(e, Name("t".into())).expect("insert");
        assert_ne!(
            topology_fingerprint(&w),
            before,
            "count-only hashing would have missed this"
        );
    }

    #[test]
    fn two_worlds_differing_only_by_a_name_do_not_fingerprint_alike() {
        let mut a = World::new();
        a.spawn((Transform::default(), Name("alpha".into())));
        let mut b = World::new();
        b.spawn((Transform::default(), Name("beta".into())));
        assert_ne!(topology_fingerprint(&a), topology_fingerprint(&b));
    }

    #[test]
    fn an_empty_world_is_fine() {
        let w = World::new();
        assert_eq!(topology_fingerprint(&w), topology_fingerprint(&World::new()));
    }
}
