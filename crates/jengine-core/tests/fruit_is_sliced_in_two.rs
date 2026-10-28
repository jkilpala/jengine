//! **Slicing** (Oct 2026; `ecs::slice`, `Physics.slice`): a fruit thrown up
//! is cut in two by a swipe across it, and the halves are real bodies - the
//! whole fruit's body leaves the physics, each half's is built from its
//! collider, and they fly apart with the fruit's own motion plus the push
//! off the cut, then fall. This runs the real physics step as a host does,
//! and the cut through the shared host command a browser script reaches.
//! Found by the showcase's Fruit Slicer.

#![cfg(all(feature = "physics", feature = "slice"))]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, Name, RigidBody2D, SpriteComponent, Transform, Velocity2D};
use jengine_core::ecs::slice::Sliceable;
use jengine_core::physics::PhysicsWorld;

const DT: f32 = 1.0 / 60.0;

fn step(world: &mut World, physics: &mut PhysicsWorld, frames: usize) {
    for _ in 0..frames {
        jengine_core::ecs::behaviors::tick_motion(world, DT, None, false);
        jengine_core::ecs::systems::ensure_physics_bodies(world, physics);
        physics.step(DT);
        physics.sync_transforms_to_ecs(world);
    }
}

fn find(world: &World, name: &str) -> Option<hecs::Entity> {
    world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e)
}

#[test]
fn a_thrown_fruit_is_cut_into_two_falling_halves() {
    let mut world = World::new();
    world.spawn((
        Name("Melon".into()),
        Transform { position: Vec2::new(0.0, -200.0), ..Default::default() },
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 40.0 }, ..Default::default() },
        SpriteComponent { texture_id: "melon.png".into(), size: Some([80.0, 80.0]), ..Default::default() },
        Velocity2D { x: 0.0, y: 0.0, ..Default::default() },
        Sliceable { separation: 200.0, ..Default::default() },
    ));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    jengine_core::ecs::systems::ensure_physics_bodies(&world, &mut physics);
    let id = find(&world, "Melon").unwrap().id() as u64;
    physics.set_velocity(id, 0.0, 900.0);
    step(&mut world, &mut physics, 20);
    let at = world.get::<&Transform>(find(&world, "Melon").unwrap()).unwrap().position;
    assert!(at.y > -100.0, "thrown up: {at}");

    // A swipe across it, through the shared command a script reaches.
    use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
    struct Host(World);
    impl ScriptWorldHost for Host {
        fn world(&mut self) -> &mut World {
            &mut self.0
        }
        fn resolve_entity(&mut self, id: u32) -> Option<hecs::Entity> {
            self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
        }
    }
    let mut host = Host(world);
    let args = JsonArgs(serde_json::json!({ "x0": at.x - 100.0, "y0": at.y + 5.0, "x1": at.x + 100.0, "y1": at.y - 5.0 }));
    assert!(handle_world_command(&mut host, "pslc", &args), "a shared command");
    let mut world = host.0;
    assert!(find(&world, "Melon").is_none(), "the whole fruit is gone");
    let (a, b) = (find(&world, "Melon/a").expect("a half"), find(&world, "Melon/b").expect("the other"));
    let ya0 = world.get::<&Transform>(a).unwrap().position.y;
    let yb0 = world.get::<&Transform>(b).unwrap().position.y;
    assert!((ya0 - yb0).abs() > 20.0, "cut across: one half above the other ({ya0} / {yb0})");

    step(&mut world, &mut physics, 30);
    let pa = world.get::<&Transform>(a).unwrap().position;
    let pb = world.get::<&Transform>(b).unwrap().position;
    assert!((pa.y - pb.y).abs() > (ya0 - yb0).abs() + 40.0, "pushed apart off the cut: {pa} / {pb}");
    assert!(physics.get_velocity(id).is_none(), "the whole fruit's body left the physics");
    let va = physics.get_velocity(a.id() as u64).expect("a body for the half");
    let vb = physics.get_velocity(b.id() as u64).expect("and the other");
    assert!(va.1 < 600.0 && vb.1 < 600.0, "carrying the fruit's slowing climb: {va:?} {vb:?}");
}
