//! What `resolve_world_transforms` costs for entities that have no hierarchy.
//!
//! It runs once per frame over the whole world: collect every
//! `(Entity, Transform, Option<Parent>)` into a Vec, resolve, then
//! `world.insert_one` a `WorldTransform` onto **every** entity. For an entity
//! with no `Parent` that component is just a copy of its `Transform`, and the
//! sprite renderers already fall back to `WorldTransform::from(t)` when it is
//! absent — so the work has no consumer.
//!
//! `#[ignore]`d: timing. Run with
//! `cargo test -p jengine-core --release --test hierarchy_cost -- --ignored --nocapture`

use std::time::Instant;

use hecs::World;
use jengine_core::ecs::components::Transform;
use jengine_core::ecs::hierarchy::resolve_world_transforms;

fn flat_world(n: usize) -> World {
    let mut world = World::new();
    for _ in 0..n {
        world.spawn((Transform::default(),));
    }
    world
}

#[test]
#[ignore = "timing-sensitive; run with --release"]
fn report_flat_hierarchy_cost() {
    for n in [1_000usize, 8_000, 16_000] {
        let mut world = flat_world(n);

        // First call is the expensive one: every entity gains a component it
        // did not have, which in an archetypal ECS moves all of its data.
        let t0 = Instant::now();
        resolve_world_transforms(&mut world);
        let first_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // Steady state: the component now exists, so this is the per-frame
        // cost of maintaining it.
        let t1 = Instant::now();
        for _ in 0..20 {
            resolve_world_transforms(&mut world);
        }
        let steady_ms = t1.elapsed().as_secs_f64() * 1000.0 / 20.0;

        println!(
            "{n:>6} flat entities: first call {first_ms:>7.3} ms, steady {steady_ms:>7.3} ms/frame"
        );
    }
    println!();
    println!("None of these entities has a Parent, so every WorldTransform written");
    println!("here is a copy of a Transform the renderer would have derived itself.");
}

// ── correctness of the restriction ─────────────────────────────────────────

use jengine_core::ecs::components::{Parent, WorldTransform};

#[test]
fn an_unparented_entity_does_not_keep_a_stale_world_transform() {
    // The hazard the restriction introduces. Renderers prefer
    // `WorldTransform` over deriving from `Transform`, so an entity that
    // leaves a hierarchy and keeps its last one would silently freeze at the
    // position it had while parented.
    let mut world = World::new();
    let mut parent_tf = Transform::default();
    parent_tf.position.x = 100.0;
    let parent = world.spawn((parent_tf,));
    let child = world.spawn((Transform::default(), Parent(parent)));

    resolve_world_transforms(&mut world);
    assert_eq!(
        world.get::<&WorldTransform>(child).unwrap().position.x,
        100.0,
        "child was not placed against its parent"
    );

    // Unparent it, then move it far away.
    world.remove_one::<Parent>(child).unwrap();
    let mut moved = Transform::default();
    moved.position.x = -50.0;
    world.insert_one(child, moved).unwrap();
    resolve_world_transforms(&mut world);

    assert!(
        world.get::<&WorldTransform>(child).is_err(),
        "stale WorldTransform survived unparenting; the renderer would draw          the entity at its old hierarchical position forever"
    );
}

#[test]
fn entities_outside_a_hierarchy_never_gain_the_component() {
    // The whole point: no per-entity insert, so no archetype churn and no
    // per-frame write for something with no consumer.
    let mut world = flat_world(100);
    resolve_world_transforms(&mut world);
    assert_eq!(
        world.query::<&WorldTransform>().iter().count(),
        0,
        "flat entities were given a WorldTransform they have no use for"
    );
}

#[test]
fn a_parent_still_gets_one_even_without_a_parent_of_its_own() {
    // A root with children is a participant: its children resolve against it,
    // and `hierarchy_integration` reads it directly.
    let mut world = World::new();
    let root = world.spawn((Transform::default(),));
    world.spawn((Transform::default(), Parent(root)));
    resolve_world_transforms(&mut world);
    assert!(
        world.get::<&WorldTransform>(root).is_ok(),
        "a root with children must still resolve"
    );
}
