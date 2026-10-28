//! **A field things move through** (Oct 2026; `ecs::displacement`): a
//! `displacer` walking through a `displacement_field` presses its cells
//! outward, and every `field_bend` sprite there leans away from the push and
//! springs back once it has gone - grass parting round a walker. Ticked in
//! `tick_motion`, the step every host makes. Found by the showcase's Meadow
//! Trails.
#![cfg(feature = "displacement")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::Transform;
use jengine_core::ecs::displacement::{DisplacementField, Displacer, FieldBend};

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

#[test]
fn grass_parts_round_a_walker_and_springs_back() {
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), DisplacementField { size: [800.0, 200.0], cell: 10.0, recover: 0.4, ..Default::default() }));
    // A row of blades along y = 0, every 20 units.
    let blades: Vec<(f32, hecs::Entity)> =
        (-15..=15).map(|k| k as f32 * 20.0).map(|x| (x, world.spawn((at(x, 0.0), FieldBend::default())))).collect();
    // The walker crosses the row at x = 0, going up.
    let walker = world.spawn((at(0.0, -150.0), Displacer { radius: 50.0, strength: 1.0, ..Default::default() }));
    let rot = |world: &World, e: hecs::Entity| world.get::<&Transform>(e).unwrap().rotation;
    let mut bent_right = 0.0f32;
    let mut bent_left = 0.0f32;
    for frame in 0..120 {
        world.get::<&mut Transform>(walker).unwrap().position.y = -150.0 + frame as f32 * 2.5;
        jengine_core::ecs::behaviors::tick_motion(&mut world, 1.0 / 60.0, None, false);
        for &(x, e) in &blades {
            if (x - 20.0).abs() < 1.0 {
                bent_right = bent_right.min(rot(&world, e));
            }
            if (x + 20.0).abs() < 1.0 {
                bent_left = bent_left.max(rot(&world, e));
            }
        }
    }
    // A blade to the right of the path leans right (clockwise), one to the
    // left leans left; one far off never moves.
    assert!(bent_right < -0.3, "right of the path leans away: {bent_right}");
    assert!(bent_left > 0.3, "left of the path leans away: {bent_left}");
    let far = blades.iter().find(|(x, _)| *x == 300.0).unwrap().1;
    assert!(rot(&world, far).abs() < 1e-4, "far off, untouched: {}", rot(&world, far));
    // Gone on, out of reach: the grass stands again.
    world.get::<&mut Transform>(walker).unwrap().position = Vec2::new(0.0, 900.0);
    for _ in 0..240 {
        jengine_core::ecs::behaviors::tick_motion(&mut world, 1.0 / 60.0, None, false);
    }
    for &(x, e) in &blades {
        assert!(rot(&world, e).abs() < 0.03, "the blade at {x} stands again: {}", rot(&world, e));
    }
}

#[test]
fn a_bent_blade_is_squashed_and_keeps_its_own_scale() {
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), DisplacementField { size: [400.0, 400.0], cell: 8.0, ..Default::default() }));
    let blade = world.spawn((
        Transform { position: Vec2::new(10.0, 0.0), scale: Vec2::new(2.0, 3.0), ..Default::default() },
        FieldBend { squash: 0.5, ..Default::default() },
    ));
    world.spawn((at(0.0, 0.0), Displacer { radius: 60.0, ..Default::default() }));
    for _ in 0..30 {
        jengine_core::ecs::behaviors::tick_motion(&mut world, 1.0 / 60.0, None, false);
    }
    let t = world.get::<&Transform>(blade).unwrap().clone();
    assert_eq!(t.scale.x, 2.0, "its width is its own");
    assert!(t.scale.y < 3.0 && t.scale.y > 1.5, "shorter under the push: {}", t.scale.y);
}
