//! Contacts are reported correctly when the display runs faster than physics.
//!
//! Physics steps at a fixed 60 Hz; at 165 Hz most frames run **no** step.
//! `PhysicsWorld::step` diffed each frame's contacts against the last, and a
//! frame with no step saw none - so every resting contact was reported as
//! ended, and on the next step as starting again with the resting load as its
//! "impact". Test-lab scene 84's three crates, dropped from three heights,
//! all read ~20 in the editor (BUGS #159); the engine's own impulse tests
//! passed because they stepped at exactly 60 Hz.

#![cfg(feature = "physics")]

use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::{ContactType, PhysicsWorld};

fn boxed(w: f32, h: f32) -> Collider2D {
    Collider2D { shape: ColliderShape::Box { width: w, height: h }, ..Default::default() }
}

/// Drop three crates from three heights at `hz` frames a second; return each
/// one's first landing impulse and how many times any crate "started"
/// touching the floor in total.
fn drop_three(hz: f32) -> ([f32; 3], usize) {
    let mut world = hecs::World::new();
    let floor = world.spawn((
        Transform { position: glam::Vec2::new(0.0, -240.0), ..Default::default() },
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        boxed(1200.0, 32.0),
    ));
    let crates: Vec<hecs::Entity> = [(-300.0, -40.0), (0.0, 120.0), (300.0, 300.0)]
        .iter()
        .map(|&(x, y)| {
            world.spawn((
                Transform { position: glam::Vec2::new(x, y), ..Default::default() },
                RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() },
                boxed(40.0, 40.0),
            ))
        })
        .collect();
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    let (mut first, mut starts) = ([0.0f32; 3], 0usize);
    let dt = 1.0 / hz;
    for _ in 0..(3.0 * hz) as usize {
        ensure_physics_bodies(&world, &mut physics);
        physics.sync_kinematic_from_ecs(&world);
        for ev in physics.step(dt) {
            if ev.event_type != ContactType::Started {
                continue;
            }
            let other = if ev.entity_a == floor.id() as u64 { ev.entity_b } else { ev.entity_a };
            if let Some(i) = crates.iter().position(|c| c.id() as u64 == other) {
                starts += 1;
                if first[i] == 0.0 {
                    first[i] = ev.impulse;
                }
            }
        }
        physics.sync_transforms_to_ecs(&mut world);
    }
    (first, starts)
}

#[test]
fn landings_order_by_height_at_any_display_rate() {
    for hz in [60.0, 144.0, 165.0, 240.0] {
        let (first, starts) = drop_three(hz);
        assert!(
            first[0] < first[1] && first[1] < first[2],
            "at {hz} Hz the landings from -40, 120 and 300 reported {first:?} - not ordered by height"
        );
        assert!(
            starts <= 6,
            "at {hz} Hz the three crates 'started' touching the floor {starts} times in three \
             seconds - resting contacts are being reported as ending and restarting"
        );
    }
}
