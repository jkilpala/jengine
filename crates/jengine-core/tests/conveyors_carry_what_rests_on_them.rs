//! **Conveyor surfaces** (Oct 2026): a `conveyor` on a static collider
//! carries what rests on it along its +x at its speed - by friction, as a
//! belt does - while the collider itself never moves. Before it a belt had
//! to be a script pushing every box on it. Found by the showcase's Baggage
//! Claim.
#![cfg(feature = "physics")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, Conveyor, RigidBody2D, Transform};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;

const DT: f32 = 1.0 / 60.0;

/// A belt turned by `turn`, a crate dropped on it; the crate's velocity
/// after `seconds`, and the belt's position then.
fn ride(speed: f32, turn: f32, seconds: f32, change: Option<(f32, f32)>) -> (Vec2, Vec2) {
    ride_ordered(speed, turn, seconds, change, false)
}

/// [`ride`], with the crate spawned (and so given its collider) first when
/// `crate_first` - which of the two rapier calls the first collider of the
/// pair is what the slide's sign depends on.
fn ride_ordered(speed: f32, turn: f32, seconds: f32, change: Option<(f32, f32)>, crate_first: bool) -> (Vec2, Vec2) {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    let early = crate_first.then(|| {
        world.spawn((
            Transform { position: Vec2::new(0.0, 40.0), ..Default::default() },
            RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
            Collider2D { shape: ColliderShape::Box { width: 40.0, height: 40.0 }, friction: 0.8, ..Default::default() },
        ))
    });
    let belt = world.spawn((
        Transform { position: Vec2::ZERO, rotation: turn, ..Default::default() },
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 2000.0, height: 20.0 }, friction: 0.8, ..Default::default() },
        Conveyor { speed },
    ));
    let crate_ = early.unwrap_or_else(|| {
        world.spawn((
            Transform { position: Vec2::new(0.0, if turn.abs() > 1.0 { -40.0 } else { 40.0 }), ..Default::default() },
            RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
            Collider2D { shape: ColliderShape::Box { width: 40.0, height: 40.0 }, friction: 0.8, ..Default::default() },
        ))
    });
    if turn.abs() > 1.0 {
        // Upside down the belt's top is its underside: hang gravity upward
        // so the crate presses on it.
        physics.set_gravity(0.0, 9.81);
    }
    let steps = (seconds / DT) as usize;
    for i in 0..steps {
        if let Some((at, new_speed)) = change
            && i == (at / DT) as usize
        {
            world.get::<&mut Conveyor>(belt).unwrap().speed = new_speed;
        }
        ensure_physics_bodies(&world, &mut physics);
        physics.step(DT);
    }
    let v = physics.get_velocity(crate_.id() as u64).unwrap();
    let p = physics.get_position(belt.id() as u64).unwrap();
    (Vec2::new(v.0, v.1), Vec2::new(p.0, p.1))
}

#[test]
fn a_belt_carries_what_rests_on_it_at_its_speed() {
    let (v, belt) = ride(150.0, 0.0, 2.0, None);
    assert!((v.x - 150.0).abs() < 10.0 && v.y.abs() < 5.0, "carried at the belt's speed: {v:?}");
    assert_eq!(belt, Vec2::ZERO, "the belt itself does not move");
    let (back, _) = ride(-150.0, 0.0, 2.0, None);
    assert!((back.x + 150.0).abs() < 10.0, "a negative speed runs the other way: {back:?}");
    let (still, _) = ride(0.0, 0.0, 2.0, None);
    assert!(still.length() < 1.0, "a stopped belt holds: {still:?}");
}

#[test]
fn either_order_of_the_pair_slides_the_same_way() {
    let (v, _) = ride_ordered(150.0, 0.0, 2.0, None, true);
    assert!((v.x - 150.0).abs() < 10.0, "crate first: {v:?}");
}

#[test]
fn a_belt_runs_along_its_own_x() {
    // Turned half a turn, its +x points left.
    let (v, _) = ride(150.0, std::f32::consts::PI, 2.0, None);
    assert!((v.x + 150.0).abs() < 10.0, "{v:?}");
}

#[test]
fn a_changed_speed_is_picked_up() {
    let (v, _) = ride(150.0, 0.0, 3.0, Some((1.5, -60.0)));
    assert!((v.x + 60.0).abs() < 8.0, "the new speed: {v:?}");
}

#[test]
fn set_conveyor_speed_reaches_a_belt_from_every_runtime() {
    use jengine_core::ecs::systems::apply_script_commands;
    use jengine_core::scripting::api::ScriptCommand;
    use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
    let mut world = World::new();
    let e = world.spawn((Conveyor { speed: 10.0 },));
    let id = e.id() as u64;
    apply_script_commands(&mut world, &[ScriptCommand::SetConveyorSpeed { entity_id: id, speed: 42.0 }], None, None);
    assert_eq!(world.get::<&Conveyor>(e).unwrap().speed, 42.0);
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
    assert!(handle_world_command(&mut host, "scvs", &JsonArgs::new(serde_json::json!({ "e": id, "s": -7.5 }))));
    assert_eq!(host.0.get::<&Conveyor>(e).unwrap().speed, -7.5);
    let c: Conveyor = serde_json::from_str(r#"{ "speed": 90 }"#).unwrap();
    assert_eq!(c.speed, 90.0);
}
