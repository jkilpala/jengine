//! **Area effects** (Oct 2026): a box that does something to the bodies in
//! it - water they float in, wind that pushes them, a pull towards its middle
//! (Unity's effectors, Godot's area gravity). Before it each was a script
//! applying forces every frame, at the script's rate rather than the
//! physics'. Found by the showcase's Harbour at Night.
//!
//! The arithmetic is unit-tested in `ecs::area_effect`; this runs it through
//! the real physics step, the script command and the particles.

#![cfg(feature = "physics")]

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::area_effect::AreaEffect;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;

const DT: f32 = 1.0 / 60.0;

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

fn crate_at(world: &mut World, x: f32, y: f32) -> Entity {
    world.spawn((
        at(x, y),
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 40.0, height: 40.0 }, ..Default::default() },
    ))
}

fn run(world: &mut World, physics: &mut PhysicsWorld, seconds: f32) {
    for _ in 0..(seconds / DT) as usize {
        ensure_physics_bodies(world, physics);
        physics.step(DT);
        physics.sync_transforms_to_ecs(world);
    }
}

fn pos(world: &World, e: Entity) -> Vec2 {
    world.get::<&Transform>(e).unwrap().position
}

#[test]
fn a_crate_floats_half_under_in_water_of_buoyancy_two() {
    let mut world = World::new();
    // Water 800 wide, 400 deep, its surface at y 0. Drag is felt by the
    // share under, so a float half under feels half of it.
    world.spawn((at(0.0, -200.0), AreaEffect { size: [800.0, 400.0], buoyancy: 2.0, drag: 5.0, ..Default::default() }));
    // Kept upright: the submerged share is measured on the body's bounds,
    // and a tilted box's bounds are taller than the box.
    let c = world.spawn((
        at(0.0, 150.0),
        RigidBody2D { body_type: BodyType::Dynamic, fixed_rotation: true, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 40.0, height: 40.0 }, ..Default::default() },
    ));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    run(&mut world, &mut physics, 6.0);
    let y = pos(&world, c).y;
    assert!(y.abs() < 3.0, "its middle rests on the surface, half under: y {y}");
    // And it has stopped: the drag took the bobbing out.
    let before = pos(&world, c);
    run(&mut world, &mut physics, 0.5);
    assert!((pos(&world, c) - before).length() < 1.0, "settled");
}

/// Buoyancy pushes at the middle of the part under water, so a hull set
/// down heeled over turns itself level. Pushed at the centre of mass, as it
/// first was, nothing turned it back and the showcase's boats capsized in
/// the first gust.
#[test]
fn a_heeled_hull_rights_itself() {
    let mut world = World::new();
    world.spawn((at(0.0, -200.0), AreaEffect { size: [1200.0, 400.0], buoyancy: 2.0, drag: 3.0, angular_drag: 5.0, ..Default::default() }));
    let hull = world.spawn((
        Transform { position: Vec2::new(0.0, 20.0), rotation: 0.5, ..Default::default() },
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 160.0, height: 28.0 }, ..Default::default() },
    ));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    run(&mut world, &mut physics, 8.0);
    let t = world.get::<&Transform>(hull).unwrap().clone();
    assert!(t.rotation.abs() < 0.05, "level again: {}", t.rotation);
    assert!(t.position.y.abs() < 3.0, "and floating half under: {}", t.position.y);
}

/// Buoyancy one under water cancels gravity exactly, so a body under holds
/// still. It rose, at first: the push was an impulse, and rapier spreads
/// gravity over a step's solver substeps while an impulse lands whole at
/// the start, so the two never cancelled - and every float sat low.
#[test]
fn buoyancy_one_holds_a_body_under_water_still() {
    let mut world = World::new();
    world.spawn((at(0.0, -200.0), AreaEffect { size: [800.0, 400.0], buoyancy: 1.0, ..Default::default() }));
    let c = crate_at(&mut world, 0.0, -150.0);
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    run(&mut world, &mut physics, 3.0);
    assert!((pos(&world, c).y + 150.0).abs() < 0.01, "held still: {:?}", pos(&world, c));
}

/// A body that leaves the water stops being pushed: the force is cleared
/// every step, and the step after the last area goes too.
#[test]
fn a_body_out_of_the_box_feels_nothing_and_neither_does_one_after_the_box_goes() {
    let mut world = World::new();
    let water = world.spawn((at(0.0, 0.0), AreaEffect { size: [400.0, 400.0], force: [0.0, 300.0], ..Default::default() }));
    let c = crate_at(&mut world, 0.0, 0.0);
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    run(&mut world, &mut physics, 0.2);
    world.despawn(water).unwrap();
    physics.set_velocity(c.id() as u64, 0.0, 0.0);
    run(&mut world, &mut physics, 0.1);
    let y = pos(&world, c).y;
    run(&mut world, &mut physics, 0.5);
    assert!((pos(&world, c).y - y).abs() < 0.01, "no push left behind");
}

#[test]
fn wind_pushes_only_what_is_in_its_box_and_a_disabled_box_nothing() {
    let mut world = World::new();
    // Zero gravity: a wind box on the right half.
    let wind = world.spawn((at(300.0, 0.0), AreaEffect { size: [400.0, 400.0], force: [400.0, 0.0], ..Default::default() }));
    let inside = crate_at(&mut world, 200.0, 0.0);
    let outside = crate_at(&mut world, -300.0, 0.0);
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    run(&mut world, &mut physics, 0.5);
    assert!(pos(&world, inside).x > 230.0, "pushed along: {:?}", pos(&world, inside));
    assert!((pos(&world, outside).x + 300.0).abs() < 0.5, "untouched outside");
    // Switched off through the script command, it stops pushing.
    use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
    struct Host(World);
    impl ScriptWorldHost for Host {
        fn world(&mut self) -> &mut World {
            &mut self.0
        }
        fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
            self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
        }
    }
    let mut host = Host(world);
    let args = JsonArgs(serde_json::json!({ "e": wind.id(), "f": "enabled", "v": false }));
    assert!(handle_world_command(&mut host, "saef", &args), "`saef` is a shared world command");
    assert!(!host.0.get::<&AreaEffect>(wind).unwrap().enabled);
    let mut world = host.0;
    // Stop the crate, then watch it not move.
    physics.set_velocity(inside.id() as u64, 0.0, 0.0);
    let x = pos(&world, inside).x;
    run(&mut world, &mut physics, 0.5);
    assert!((pos(&world, inside).x - x).abs() < 1.0, "a disabled box pushes nothing");
}

#[test]
fn a_whirlpool_pulls_towards_its_middle() {
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), AreaEffect { size: [600.0, 600.0], attract: 600.0, drag: 0.5, ..Default::default() }));
    let c = crate_at(&mut world, 200.0, 120.0);
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let start = pos(&world, c).length();
    run(&mut world, &mut physics, 1.5);
    assert!(pos(&world, c).length() < start * 0.6, "drawn in: {start} -> {:?}", pos(&world, c));
}

#[test]
fn wind_blows_particles_too() {
    use jengine_core::particles::{ParticleEmitter, ParticleSystem, tick_particles};
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), AreaEffect { size: [2000.0, 2000.0], force: [300.0, 0.0], ..Default::default() }));
    let emitter = ParticleEmitter {
        emission_rate: 0.0,
        burst_count: 20,
        max_particles: 20,
        speed: [0.0, 0.0],
        lifetime: [5.0, 5.0],
        gravity_modifier: 0.0,
        world_space: true,
        ..Default::default()
    };
    let e = world.spawn((at(0.0, 0.0), emitter.clone()));
    let mut system = ParticleSystem::new();
    for _ in 0..60 {
        tick_particles(&world, &mut system, DT);
    }
    let groups = system.draw_data(&[(e.id() as u64, &emitter, Vec2::ZERO)], &Default::default());
    let xs: Vec<f32> = groups.iter().flat_map(|g| g.particles.iter().map(|p| p.position.x)).collect();
    assert!(!xs.is_empty());
    assert!(xs.iter().all(|x| *x > 100.0), "every particle blown right: {xs:?}");
}

#[test]
fn the_scene_format_keeps_it() {
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), AreaEffect { buoyancy: 1.5, force: [10.0, 0.0], ..Default::default() }));
    let scene = jengine_core::scene::format::save_scene(&world);
    let a = scene.entities[0].components.get::<jengine_core::ecs::area_effect::AreaEffect>("area_effect").expect("saved");
    assert_eq!((a.buoyancy, a.force), (1.5, [10.0, 0.0]));
}
