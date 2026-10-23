//! **The world is saved and loaded** (Oct 2026; `ecs::save_state`,
//! `State.save` / `State.load`). A save state is the whole running world -
//! entities, components and every body's pose, velocity and spin - and a load
//! puts it back under the same handles, so a script's handles still work.
//! This saves a world in motion, lets it run on and change, loads it, and
//! holds the bodies to what was saved and the run after the load to the run
//! after the save. Found by the showcase's Quicksave Workshop.

#![cfg(feature = "physics")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, CustomComponents, Name, RigidBody2D, Transform};
use jengine_core::ecs::save_state;
use jengine_core::physics::PhysicsWorld;

fn frame(world: &mut World, physics: &mut PhysicsWorld) {
    jengine_core::ecs::systems::ensure_physics_bodies(world, physics);
    physics.step_once();
    physics.sync_transforms_to_ecs(world);
    save_state::sync(world, physics);
}

fn bodies(world: &World, physics: &PhysicsWorld) -> Vec<(String, save_state::BodyState)> {
    let mut v: Vec<_> = world
        .query::<&Name>()
        .iter()
        .filter_map(|(e, n)| physics.body_state(e.id() as u64).map(|b| (n.0.clone(), b)))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn workshop() -> (World, PhysicsWorld, hecs::Entity) {
    let mut world = World::new();
    world.spawn((
        Name("Floor".into()),
        Transform { position: Vec2::new(0.0, -100.0), ..Default::default() },
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 2000.0, height: 20.0 }, ..Default::default() },
    ));
    let mut custom = std::collections::HashMap::new();
    custom.insert("Score".to_string(), serde_json::json!({ "points": 3 }));
    let marble = world.spawn((
        Name("Marble".into()),
        Transform { position: Vec2::new(-200.0, -78.0), ..Default::default() },
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 12.0 }, friction: 0.8, ..Default::default() },
        CustomComponents(custom),
    ));
    world.spawn((
        Name("Domino".into()),
        Transform { position: Vec2::new(100.0, -50.0), rotation: 0.2, ..Default::default() },
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 10.0, height: 80.0 }, ..Default::default() },
    ));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    jengine_core::ecs::systems::ensure_physics_bodies(&world, &mut physics);
    physics.set_velocity(marble.id() as u64, 300.0, 0.0);
    (world, physics, marble)
}

#[test]
fn a_world_in_motion_is_put_back_exactly() {
    save_state::clear();
    let (mut world, mut physics, marble) = workshop();
    for _ in 0..20 {
        frame(&mut world, &mut physics);
    }
    save_state::request_save("quick");
    assert!(save_state::has("quick"), "a save asked for counts at once");
    frame(&mut world, &mut physics);
    let saved = bodies(&world, &physics);
    assert!(saved.iter().any(|(n, b)| n == "Marble" && b.velocity[0].abs() > 10.0 && b.spin.abs() > 0.1),
            "saved while rolling: {saved:?}");
    // Run on, and change things: the marble's custom field, a new crate,
    // and the domino destroyed.
    let mut after_save = Vec::new();
    for _ in 0..40 {
        frame(&mut world, &mut physics);
        after_save.push(bodies(&world, &physics));
    }
    world.get::<&mut CustomComponents>(marble).unwrap().0.insert("Score".into(), serde_json::json!({ "points": 9 }));
    let domino = world.query::<&Name>().iter().find(|(_, n)| n.0 == "Domino").map(|(e, _)| e).unwrap();
    world.despawn(domino).unwrap();
    world.spawn((Name("Crate".into()), Transform::default()));

    save_state::request_load("quick");
    frame(&mut world, &mut physics);
    // The frame ran its step before the load, so what it shows is the save.
    let loaded = bodies(&world, &physics);
    assert_eq!(loaded, saved, "every body where and as fast as it was");
    assert!(world.contains(marble), "the marble keeps its handle");
    assert!(world.contains(domino), "the domino is back, under its own handle");
    assert!(world.query::<&Name>().iter().all(|(_, n)| n.0 != "Crate"), "the crate never was");
    assert_eq!(world.get::<&CustomComponents>(marble).unwrap().0["Score"]["points"], 3);
    let t = world.get::<&Transform>(marble).unwrap().position;
    let m = saved.iter().find(|(n, _)| n == "Marble").unwrap().1.position;
    assert!((t.x - m[0]).abs() < 1e-3 && (t.y - m[1]).abs() < 1e-3, "and its transform says so");

    // And it plays on as it did the first time.
    for (k, expected) in after_save.iter().enumerate().take(30) {
        frame(&mut world, &mut physics);
        let now = bodies(&world, &physics);
        for ((n, a), (_, b)) in now.iter().zip(expected) {
            let d = (Vec2::from(a.position) - Vec2::from(b.position)).length();
            assert!(d < 0.5, "frame {k}: {n} is {d} from where it went the first time");
        }
    }
    save_state::request_forget("quick");
    frame(&mut world, &mut physics);
    assert!(!save_state::has("quick"));
}
