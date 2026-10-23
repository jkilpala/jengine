//! **Glass breaks where it is struck** (Oct 2026; `ecs::fracture`,
//! `pattern: "voronoi"`). A pane fixed in its frame cannot jolt - it never
//! moves - so it breaks on what hit it: the stone's change in velocity, its
//! impulse over its mass. It comes apart into Voronoi shards crowded round
//! the point of impact, each a loose convex body that falls. This runs the
//! real physics step and `break_on_impact` as a host does. Found by the
//! showcase's Glass House.

#![cfg(feature = "physics")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, Mesh2D, Name, RigidBody2D, SpriteComponent, Transform};
use jengine_core::ecs::fracture::{self, BreakPattern, Breakable};
use jengine_core::physics::PhysicsWorld;

const DT: f32 = 1.0 / 60.0;

/// A tall pane at the origin and a stone thrown at it from the left at
/// `speed`, at the height `y`.
fn throw(speed: f32, y: f32) -> (World, PhysicsWorld) {
    let mut world = World::new();
    world.spawn((
        Name("Pane".into()),
        Transform::default(),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 16.0, height: 240.0 }, ..Default::default() },
        SpriteComponent { texture_id: "glass.png".into(), size: Some([16.0, 240.0]), ..Default::default() },
        Breakable { pattern: BreakPattern::Voronoi, pieces: 14, focus: 0.7, break_speed: 300.0, scatter: 30.0, ..Default::default() },
    ));
    world.spawn((
        Name("Stone".into()),
        Transform { position: Vec2::new(-200.0, y), ..Default::default() },
        RigidBody2D { body_type: BodyType::Dynamic, gravity_scale: 0.0, ccd: true, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 8.0 }, ..Default::default() },
        jengine_core::ecs::components::Velocity2D { x: speed, y: 0.0, ..Default::default() },
    ));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    jengine_core::ecs::systems::ensure_physics_bodies(&world, &mut physics);
    let stone = world.query::<&Name>().iter().find(|(_, n)| n.0 == "Stone").map(|(e, _)| e.id() as u64).unwrap();
    physics.set_velocity(stone, speed, 0.0);
    (world, physics)
}

fn run(world: &mut World, physics: &mut PhysicsWorld, frames: usize) -> usize {
    let mut broke = 0;
    for _ in 0..frames {
        jengine_core::ecs::systems::ensure_physics_bodies(world, physics);
        let contacts = physics.step(DT);
        physics.sync_transforms_to_ecs(world);
        broke += fracture::break_on_impact(world, physics, &contacts, DT).len();
    }
    broke
}

fn shards(world: &World) -> Vec<(Vec2, f32)> {
    world
        .query::<(&Transform, &Collider2D, &Mesh2D)>()
        .iter()
        .filter_map(|(_, (t, c, _))| match &c.shape {
            ColliderShape::ConvexPolygon { vertices } => {
                let mut a = 0.0;
                for i in 0..vertices.len() {
                    let (p, q) = (vertices[i], vertices[(i + 1) % vertices.len()]);
                    a += p[0] * q[1] - q[0] * p[1];
                }
                Some((t.position, a * 0.5))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_stone_shatters_the_pane_round_where_it_struck() {
    let (mut world, mut physics) = throw(1500.0, 60.0);
    assert_eq!(run(&mut world, &mut physics, 30), 1, "the pane broke once");
    assert!(world.query::<&Name>().iter().all(|(_, n)| n.0 != "Pane"), "and is gone");
    let pieces = shards(&world);
    assert!(pieces.len() >= 10, "into shards: {}", pieces.len());
    let total: f32 = pieces.iter().map(|p| p.1).sum();
    assert!((total - 16.0 * 240.0).abs() < 5.0, "which add up to the pane: {total}");
    // Small where the stone struck (y = 60), long away from it.
    let near: Vec<f32> = pieces.iter().filter(|p| (p.0.y - 60.0).abs() < 40.0).map(|p| p.1).collect();
    let far: Vec<f32> = pieces.iter().filter(|p| (p.0.y - 60.0).abs() > 80.0).map(|p| p.1).collect();
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
    assert!(!near.is_empty() && !far.is_empty());
    assert!(mean(&near) < mean(&far), "near {near:?} far {far:?}");

    // Loose: they fall.
    let before: f32 = pieces.iter().map(|p| p.0.y).sum::<f32>() / pieces.len() as f32;
    run(&mut world, &mut physics, 40);
    let after = shards(&world);
    let after: f32 = after.iter().map(|p| p.0.y).sum::<f32>() / after.len() as f32;
    assert!(after < before - 20.0, "the shards fall: {before} -> {after}");
}

#[test]
fn a_tap_does_not() {
    let (mut world, mut physics) = throw(120.0, 0.0);
    assert_eq!(run(&mut world, &mut physics, 150), 0);
    assert!(world.query::<&Name>().iter().any(|(_, n)| n.0 == "Pane"));
}
