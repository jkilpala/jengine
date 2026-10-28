//! Jiggle bones (`animation::jiggle`): a bone's tip lags its pose and springs
//! back. Run through `update_skeletal_players`, which every host calls each
//! frame - the jiggle step rides at its end.
#![cfg(feature = "jiggle")]

mod support;

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::animation::jiggle::Jiggle;
use jengine_core::animation::skeletal::update_skeletal_players;
use jengine_core::ecs::components::{Bone, Parent, SkeletonRoot, Transform};
use jengine_core::ecs::hierarchy::{resolve_world_transforms, world_transform_of};

const DT: f32 = 1.0 / 60.0;

fn step(world: &mut World) {
    update_skeletal_players(world, &HashMap::new(), DT);
    resolve_world_transforms(world);
}

/// A root with a chain of `n` jiggle bones hanging down from it.
fn tail(world: &mut World, n: usize, jiggle: Jiggle) -> (Entity, Vec<Entity>) {
    let root = world.spawn((Transform::default(), SkeletonRoot { name: "rig".into() }));
    let mut bones = Vec::new();
    let mut parent = root;
    for i in 0..n {
        let t = if i == 0 {
            Transform { rotation: -FRAC_PI_2, ..Default::default() }
        } else {
            Transform { position: Vec2::new(30.0, 0.0), ..Default::default() }
        };
        let b = world.spawn((t, Parent(parent), Bone { name: format!("b{i}"), length: 30.0 }, jiggle.clone()));
        bones.push(b);
        parent = b;
    }
    (root, bones)
}

fn world_angle(world: &World, e: Entity) -> f32 {
    world_transform_of(world, e).unwrap().rotation
}

#[test]
fn a_chain_whips_link_by_link() {
    let mut world = World::new();
    let (root, bones) = tail(&mut world, 4, Jiggle::default());
    for _ in 0..30 {
        step(&mut world);
    }
    // Hanging still: every link points straight down.
    for &b in &bones {
        assert!((world_angle(&world, b) + FRAC_PI_2).abs() < 0.05, "{}", world_angle(&world, b));
    }
    // Dash right: the tail streams out behind, the far end most.
    for _ in 0..12 {
        world.get::<&mut Transform>(root).unwrap().position.x += 10.0;
        step(&mut world);
    }
    let lag: Vec<f32> = bones.iter().map(|&b| -FRAC_PI_2 - world_angle(&world, b)).collect();
    assert!(lag.iter().all(|l| *l > 0.05), "every link swings back: {lag:?}");
    assert!(lag[3] > lag[0], "the tip lags most: {lag:?}");
    // Stop: it swings through and settles.
    for _ in 0..600 {
        step(&mut world);
    }
    for &b in &bones {
        assert!((world_angle(&world, b) + FRAC_PI_2).abs() < 0.03, "settled: {}", world_angle(&world, b));
    }
}

#[test]
fn a_posed_bone_springs_to_its_new_pose() {
    // A clip writes the bone's rotation every frame; the jiggle follows the
    // pose it is given, and overshoots when the pose changes.
    let mut world = World::new();
    let (_, bones) = tail(&mut world, 1, Jiggle { gravity: [0.0, 0.0], stiffness: 0.15, damping: 0.9, ..Default::default() });
    let b = bones[0];
    let pose = |w: &mut World, r: f32| w.get::<&mut Transform>(b).unwrap().rotation = r;
    for _ in 0..60 {
        pose(&mut world, 0.5);
        step(&mut world);
    }
    let r = world.get::<&Transform>(b).unwrap().rotation;
    assert!((r - 0.5).abs() < 0.01, "follows its pose: {r}");
    let mut lowest = f32::MAX;
    for _ in 0..90 {
        pose(&mut world, -0.3);
        step(&mut world);
        lowest = lowest.min(world.get::<&Transform>(b).unwrap().rotation);
    }
    assert!(lowest < -0.32, "springs past the new pose: {lowest}");
    let r = world.get::<&Transform>(b).unwrap().rotation;
    assert!((r + 0.3).abs() < 0.02, "and settles on it: {r}");
}

#[test]
fn the_swing_is_limited_and_gravity_leans_it() {
    let mut world = World::new();
    let (root, bones) = tail(&mut world, 1, Jiggle { max_angle: 0.4, stiffness: 0.02, damping: 0.98, ..Default::default() });
    for _ in 0..120 {
        world.get::<&mut Transform>(root).unwrap().position.x += 40.0;
        step(&mut world);
        let off = (world_angle(&world, bones[0]) + FRAC_PI_2).abs();
        assert!(off <= 0.4 + 1e-3, "past its limit: {off}");
    }
    // An upright stalk in a sideways current leans with it.
    let mut world = World::new();
    let root = world.spawn((Transform::default(), SkeletonRoot { name: "weed".into() }));
    let stalk = world.spawn((
        Transform { rotation: FRAC_PI_2, ..Default::default() },
        Parent(root),
        Bone { name: "stalk".into(), length: 60.0 },
        Jiggle { gravity: [400.0, 0.0], ..Default::default() },
    ));
    for _ in 0..240 {
        step(&mut world);
    }
    let a = world_angle(&world, stalk);
    assert!(a < FRAC_PI_2 - 0.01, "leans right, with the current: {a}");
}

#[test]
fn the_scene_format_keeps_it_but_not_its_motion() {
    let mut world = World::new();
    let (_, bones) = tail(&mut world, 1, Jiggle { stiffness: 0.3, ..Default::default() });
    step(&mut world);
    assert!(world.get::<&Jiggle>(bones[0]).unwrap().state.is_some());
    let scene = jengine_core::scene::format::save_scene(&world);
    let j = scene.entities.iter().find_map(|e| e.components.get::<jengine_core::animation::jiggle::Jiggle>("jiggle")).expect("saved");
    assert_eq!(j.stiffness, 0.3);
    assert!(j.state.is_none());
}

#[test]
fn every_host_jiggles_through_the_skeletal_step() {
    let skeletal = include_str!("../src/animation/skeletal.rs");
    let body = &skeletal[skeletal.find("pub fn update_skeletal_players(").unwrap()..];
    assert!(body[..body.find("\n}\n").unwrap()].contains("jiggle::tick_jiggle(world, dt)"));
    for (name, src) in [
        ("native", &support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs"))),
        ("editor", &support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs"))),
        ("web", &support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs"))),
    ] {
        assert!(src.contains("update_skeletal_players("), "{name} never animates skeletons");
    }
}
