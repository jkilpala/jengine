//! What a scene snapshot costs on a large world.
//!
//! `save_scene_v2` serializes each entity through `ComponentRegistry::
//! serialize_entity`, which loops over **every registered descriptor** (~70
//! core components plus whatever packages added) asking the world "does this
//! entity have you?". That is ~70 random component lookups per entity
//! regardless of how few components it actually carries.
//!
//! It runs on `push_scene` / `pop_scene`, so the cost is a visible hitch in
//! gameplay rather than something only a save dialog pays.
//!
//! `#[ignore]`d: timing. Run with
//! `cargo test -p jengine-core --release --test scene_snapshot_cost -- --ignored --nocapture`

use std::time::Instant;

use hecs::World;
use jengine_core::ecs::components::{SpriteComponent, Transform};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::scene::save_scene_v2;

#[test]
#[ignore = "timing-sensitive; run with --release"]
fn report_snapshot_cost_by_entity_count() {
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    println!("registered component types: {}", registry.len());
    println!();
    println!("{:>8}  {:>12}  {:>14}", "entities", "snapshot ms", "us/entity");
    println!("{:->8}  {:->12}  {:->14}", "", "", "");

    for n in [1_000usize, 8_000, 16_000] {
        let mut world = World::new();
        for _ in 0..n {
            // Three components, like a boid — the point is that the cost
            // should follow what an entity *has*, not what exists.
            world.spawn((Transform::default(), SpriteComponent::default()));
        }
        let t0 = Instant::now();
        let scene = save_scene_v2(&world, &registry);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(scene.entities.len(), n);
        println!("{n:>8}  {ms:>12.2}  {:>14.2}", ms * 1000.0 / n as f64);
    }
}

// ── correctness of the enumeration ─────────────────────────────────────────

#[test]
fn every_component_an_entity_carries_survives_serialization() {
    // The risk in enumerating rather than probing: if `component_types()`
    // ever fails to report a component, it silently vanishes from every save
    // and snapshot — data loss with no error attached. Spawn a deliberately
    // mixed entity and require each piece back.
    use jengine_core::ecs::components::{Health, Name, Tags, Velocity2D};

    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);

    let mut world = World::new();
    world.spawn((
        Transform::default(),
        SpriteComponent::default(),
        Velocity2D::default(),
        Health::default(),
        Name("Hero".to_string()),
        Tags(["player".to_string()].into_iter().collect()),
    ));

    let scene = jengine_core::scene::save_scene_v2(&world, &registry);
    assert_eq!(scene.entities.len(), 1);
    let got = &scene.entities[0].components;

    for key in ["transform", "sprite", "velocity", "health", "tags"] {
        assert!(
            got.contains_key(key),
            "`{key}` was dropped from the snapshot; keys present: {:?}",
            got.keys().collect::<Vec<_>>()
        );
    }
    // `Name` is lifted to the entity-level field rather than a component.
    assert_eq!(scene.entities[0].name, "Hero");
}

#[test]
fn an_entity_with_no_registered_components_serializes_to_nothing() {
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let mut world = World::new();
    // `u32` is not a registered component type.
    world.spawn((7u32,));
    let scene = jengine_core::scene::save_scene_v2(&world, &registry);
    assert_eq!(scene.entities.len(), 1);
    assert!(scene.entities[0].components.is_empty());
}


/// A snapshot skips bulk-spawned entities; a save does not.
///
/// This is the semantic half of the optimisation below, and the more important
/// half: dropping entities from a *save* would be data loss.
#[test]
fn a_snapshot_omits_bulk_entities_and_a_save_keeps_them() {
    use jengine_core::ecs::components::{BulkEntity, Name};
    use jengine_core::scene::snapshot_scene_v2;

    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);

    let mut world = World::new();
    world.spawn((Transform::default(), Name("Player".into())));
    for _ in 0..50 {
        world.spawn((Transform::default(), SpriteComponent::default(), BulkEntity));
    }

    let saved = save_scene_v2(&world, &registry);
    assert_eq!(saved.entities.len(), 51, "a save must keep every entity");

    let snapped = snapshot_scene_v2(&world, &registry);
    assert_eq!(
        snapped.entities.len(),
        1,
        "the snapshot kept bulk entities; a scene push should not carry a flock"
    );
    assert_eq!(snapped.entities[0].name, "Player");
}

#[test]
#[ignore = "timing-sensitive; run with --release"]
fn report_snapshot_cost_with_and_without_bulk_entities() {
    use jengine_core::ecs::components::BulkEntity;
    use jengine_core::scene::snapshot_scene_v2;

    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    println!();
    println!("{:>8}  {:>12}  {:>14}", "entities", "save ms", "snapshot ms");
    println!("{:->8}  {:->12}  {:->14}", "", "", "");

    for n in [1_000usize, 8_000, 16_000] {
        let mut world = World::new();
        // A handful of authored entities plus a flock, which is the shape a
        // scene push actually meets.
        for _ in 0..20 {
            world.spawn((Transform::default(), SpriteComponent::default()));
        }
        for _ in 0..n {
            world.spawn((Transform::default(), SpriteComponent::default(), BulkEntity));
        }

        let t0 = Instant::now();
        let full = save_scene_v2(&world, &registry);
        let save_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let t1 = Instant::now();
        let snap = snapshot_scene_v2(&world, &registry);
        let snap_ms = t1.elapsed().as_secs_f64() * 1000.0;

        assert_eq!(full.entities.len(), n + 20);
        assert_eq!(snap.entities.len(), 20);
        println!("{n:>8}  {save_ms:>12.2}  {snap_ms:>14.2}");
    }
    println!();
    println!("`push_scene` / `pop_scene` pay the right-hand column mid-game.");
}
