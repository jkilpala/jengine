//! **Smooth terrain** (Oct 2026; `ecs::smooth_terrain`, `Terrain.edit`): a
//! density grid drawn and collided as its marching-squares contour. A ball
//! rests on it; digging under it through the script command's own path
//! (`host_commands`' `stte` arm) swaps the colliders on the same body, so the
//! ball falls; and the browsers' ray snapshot sees the dug terrain exactly as
//! physics does. Found by the showcase's Smooth Caves.

#![cfg(all(feature = "physics", feature = "smooth_terrain"))]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform};
use jengine_core::ecs::smooth_terrain::SmoothTerrain;
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;
use jengine_core::physics::ray::RaySnapshot;

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

/// Solid below y = 0 in the terrain's frame, a ramp across the surface.
fn ground() -> SmoothTerrain {
    let mut t = SmoothTerrain { cols: 40, rows: 20, cell: 20.0, border: false, ..Default::default() };
    let (w, h) = (41usize, 21usize);
    let o = t.origin();
    t.density = (0..h).flat_map(|j| (0..w).map(move |_| j)).map(|j| (0.5 - (o.y + j as f32 * 20.0) / 40.0).clamp(0.0, 1.0)).collect();
    t
}

fn step(world: &mut World, physics: &mut PhysicsWorld) {
    jengine_core::ecs::behaviors::tick_motion(world, 1.0 / 60.0, None, false);
    ensure_physics_bodies(world, physics);
    physics.step(1.0 / 60.0);
    physics.sync_transforms_to_ecs(world);
}

#[test]
fn a_ball_rests_on_the_ground_and_falls_through_a_hole_dug_under_it() {
    let mut world = World::new();
    let land = world.spawn((at(0.0, 0.0), ground()));
    let ball = world.spawn((
        at(0.0, 60.0),
        RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 16.0 }, ..Default::default() },
    ));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    for _ in 0..120 {
        step(&mut world, &mut physics);
    }
    let y = world.get::<&Transform>(ball).unwrap().position.y;
    assert!((y - 16.0).abs() < 3.0, "it rests on the surface: {y}");
    // Dig a pit under it, as a script would.
    jengine_core::ecs::smooth_terrain::edit(&world, land, 0.0, -20.0, 70.0, 2.0).unwrap();
    for _ in 0..90 {
        step(&mut world, &mut physics);
    }
    let y = world.get::<&Transform>(ball).unwrap().position.y;
    assert!(y < -20.0, "it fell into the pit: {y}");
    let t = world.get::<&SmoothTerrain>(land).unwrap();
    assert!(t.density_at(Vec2::new(0.0, -20.0)) < 0.5, "dug");
}

#[test]
fn the_browser_ray_snapshot_sees_dug_terrain_as_physics_does() {
    let mut world = World::new();
    let land = world.spawn((at(30.0, -40.0), ground()));
    jengine_core::ecs::behaviors::tick_motion(&mut world, 1.0 / 60.0, None, false);
    jengine_core::ecs::smooth_terrain::edit(&world, land, 30.0, -60.0, 90.0, 2.0).unwrap();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    physics.step(1.0 / 60.0);
    let mut snap = RaySnapshot::new();
    snap.refresh(&world);
    let mut hits = 0;
    for ox in (-350..350).step_by(23) {
        for (dx, dy) in [(0.0, -1.0), (0.6, -0.8), (-0.7, -0.7)] {
            let (ox, oy) = (ox as f32, 200.0);
            let a = snap.cast(ox, oy, dx, dy, 600.0);
            let b = physics.raycast(ox, oy, dx, dy, 600.0);
            match (a, b) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    hits += 1;
                    assert!((a.distance - b.distance).abs() < 0.05, "same distance from {ox}: {} vs {}", a.distance, b.distance);
                }
                (a, b) => panic!("from {ox} dir ({dx}, {dy}): snapshot {a:?}, physics {b:?}"),
            }
        }
    }
    assert!(hits > 40, "the rays found the ground: {hits}");
}

#[test]
fn the_script_command_digs() {
    use hecs::Entity;
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
    let mut world = World::new();
    let land = world.spawn((at(0.0, 0.0), ground()));
    jengine_core::ecs::behaviors::tick_motion(&mut world, 1.0 / 60.0, None, false);
    let mut host = Host(world);
    let v = serde_json::json!({ "e": land.id(), "x": 0.0, "y": -30.0, "r": 60.0, "a": 2.0 });
    assert!(handle_world_command(&mut host, "stte", &JsonArgs(v)), "the shared arm takes `stte`");
    let t = host.0.get::<&SmoothTerrain>(land).unwrap();
    assert!(t.density_at(Vec2::new(0.0, -30.0)) < 0.5, "Terrain.edit dug");
}
