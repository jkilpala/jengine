//! IK chains (Oct 2026): any number of bones bent by FABRIK so the tip
//! reaches its target - a tail, a tentacle, a long arm - with a joint limit
//! and a blend over the animated pose.
#![cfg(feature = "ik")]

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::animation::ik::solve_ik;
use jengine_core::ecs::components::{Bone, IkChain, Name, Parent, SkeletonRoot, Transform};
use jengine_core::ecs::hierarchy::{resolve_world_transforms, world_transform_of};

/// A root at the origin and `n` bones of `len` straight along +x; the end
/// bone carries the chain.
fn chain(world: &mut World, n: usize, len: f32, ik: IkChain) -> (Entity, Vec<Entity>) {
    let root = world.spawn((Transform::default(), SkeletonRoot { name: "c".into() }));
    let mut parent = root;
    let mut bones = Vec::new();
    for i in 0..n {
        let x = if i == 0 { 0.0 } else { len };
        let b = world.spawn((
            Transform { position: Vec2::new(x, 0.0), ..Default::default() },
            Bone { name: format!("b{i}"), length: len },
            Parent(parent),
        ));
        bones.push(b);
        parent = b;
    }
    world.insert_one(*bones.last().unwrap(), ik).unwrap();
    resolve_world_transforms(world);
    (root, bones)
}

fn tip(world: &World, end: Entity, len: f32) -> Vec2 {
    let w = world_transform_of(world, end).unwrap();
    w.position + Vec2::from_angle(w.rotation) * len * w.scale.x
}

fn solve(world: &mut World) {
    solve_ik(world);
    resolve_world_transforms(world);
}

#[test]
fn a_chain_reaches_a_target_within_reach() {
    let mut world = World::new();
    let (_, bones) = chain(&mut world, 5, 40.0, IkChain { target: [60.0, 110.0], bones: 5, ..Default::default() });
    solve(&mut world);
    let t = tip(&world, *bones.last().unwrap(), 40.0);
    assert!((t - Vec2::new(60.0, 110.0)).length() < 1.0, "the tip should be on the target: {t:?}");
    // Every bone keeps its length: the joints are where the bones say.
    for pair in bones.windows(2) {
        let a = world_transform_of(&world, pair[0]).unwrap();
        let b = world_transform_of(&world, pair[1]).unwrap();
        assert!(((b.position - a.position).length() - 40.0).abs() < 0.01);
    }
}

#[test]
fn out_of_reach_the_chain_points_straight_at_it() {
    let mut world = World::new();
    let (_, bones) = chain(&mut world, 3, 30.0, IkChain { target: [0.0, 500.0], bones: 3, ..Default::default() });
    solve(&mut world);
    for b in &bones {
        let r = world_transform_of(&world, *b).unwrap().rotation;
        assert!((r - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "straight up: {r}");
    }
}

#[test]
fn a_joint_bends_no_further_than_its_limit() {
    let mut world = World::new();
    // A target behind the root: a free chain folds right back; a limited
    // one cannot.
    let ik = IkChain { target: [-20.0, 10.0], bones: 4, max_bend: 0.5, ..Default::default() };
    let (_, bones) = chain(&mut world, 4, 30.0, ik);
    for _ in 0..3 {
        solve(&mut world);
    }
    for b in &bones[1..] {
        let local = world.get::<&Transform>(*b).unwrap().rotation;
        assert!(local.abs() <= 0.5 + 1e-4, "a joint past its limit: {local}");
    }
}

#[test]
fn weight_blends_and_a_named_target_is_followed() {
    let mut world = World::new();
    let goal = world.spawn((Name("Goal".into()), Transform { position: Vec2::new(0.0, 100.0), ..Default::default() }));
    let ik = IkChain { target_name: "Goal".into(), bones: 3, weight: 0.0, ..Default::default() };
    let (_, bones) = chain(&mut world, 3, 40.0, ik);
    solve(&mut world);
    assert_eq!(world.get::<&Transform>(bones[0]).unwrap().rotation, 0.0, "weight 0 leaves the pose");
    world.get::<&mut IkChain>(bones[2]).unwrap().weight = 1.0;
    for _ in 0..2 {
        solve(&mut world);
    }
    assert!((tip(&world, bones[2], 40.0) - Vec2::new(0.0, 100.0)).length() < 1.0);
    // The goal moves; the chain follows it.
    world.get::<&mut Transform>(goal).unwrap().position = Vec2::new(70.0, 50.0);
    resolve_world_transforms(&mut world);
    solve(&mut world);
    assert!((tip(&world, bones[2], 40.0) - Vec2::new(70.0, 50.0)).length() < 1.0);
}

#[test]
fn the_ik_target_setter_reaches_a_chain() {
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
    let (_, bones) = chain(&mut world, 3, 40.0, IkChain::default());
    let end = bones[2];
    let mut host = Host(world);
    assert!(handle_world_command(&mut host, "ikt", &JsonArgs::new(serde_json::json!({ "e": end.id(), "x": 12.0, "y": 34.0 }))));
    assert!(handle_world_command(&mut host, "ikw", &JsonArgs::new(serde_json::json!({ "e": end.id(), "w": 0.25 }))));
    let ik = host.0.get::<&IkChain>(end).unwrap();
    assert_eq!(ik.target, [12.0, 34.0]);
    assert_eq!(ik.weight, 0.25);
}
