//! Integration tests for Slice 3.5.F — IK constraints round-trip and
//! end-to-end solve through the runtime pipeline.
#![cfg(feature = "native")]

use glam::Vec2;
use jengine_core::animation::ik::solve_ik;
use jengine_core::ecs::components::{Bone, IkLookAt, IkTwoBone, Parent, SkeletonRoot, Transform, WorldTransform};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::scene::format::{
    load_scene_with_mapping, save_scene, ComponentBundle, EntityData, SceneData,
};

#[test]
fn scene_round_trip_preserves_ik_components() {
    let scene = SceneData {
        name: "ik".into(),
        entities: vec![
            EntityData {
                children: Vec::new(),
                name: "RootBone".into(),
                parent: None,
                components: ComponentBundle {
                    transform: Some(Transform::default()),
                    bone: Some(Bone { name: "root".into(), length: 2.0 }),
                    skeleton_root: Some(SkeletonRoot { name: "rig".into() }),
                    ..Default::default()
                },
            },
            EntityData {
                children: Vec::new(),
                name: "MidBone".into(),
                parent: Some("RootBone".into()),
                components: ComponentBundle {
                    transform: Some(Transform { position: Vec2::new(2.0, 0.0), rotation: 0.0, scale: Vec2::ONE }),
                    bone: Some(Bone { name: "mid".into(), length: 2.0 }),
                    ..Default::default()
                },
            },
            EntityData {
                children: Vec::new(),
                name: "EndBone".into(),
                parent: Some("MidBone".into()),
                components: ComponentBundle {
                    transform: Some(Transform { position: Vec2::new(2.0, 0.0), rotation: 0.0, scale: Vec2::ONE }),
                    bone: Some(Bone { name: "end".into(), length: 0.0 }),
                    ik_two_bone: Some(IkTwoBone { target: [1.5, 2.5], flip_bend: true, ..Default::default() }),
                    ik_look_at: Some(IkLookAt { target: [0.0, 10.0], rotation_offset: 0.25 }),
                    ..Default::default()
                },
            },
        ],
        ..Default::default()
    };

    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);
    let end_entity = mapping[2];

    let two = world.get::<&IkTwoBone>(end_entity).unwrap();
    assert_eq!(two.target, [1.5, 2.5]);
    assert!(two.flip_bend);
    let look = world.get::<&IkLookAt>(end_entity).unwrap();
    assert_eq!(look.target, [0.0, 10.0]);
    assert!((look.rotation_offset - 0.25).abs() < 1.0e-6);
    drop(two);
    drop(look);

    let saved = save_scene(&world);
    let saved_end = saved.entities.iter().find(|e| e.name == "EndBone").unwrap();
    let s_two = saved_end.components.ik_two_bone.as_ref().unwrap();
    assert_eq!(s_two.target, [1.5, 2.5]);
    assert!(s_two.flip_bend);
    let s_look = saved_end.components.ik_look_at.as_ref().unwrap();
    assert_eq!(s_look.target, [0.0, 10.0]);
}

#[test]
fn end_to_end_two_bone_solve_via_scene_load() {
    // Build the same root→mid→end chain via the scene loader (proves
    // Parent name-references resolve correctly for IK chains), then
    // call solve_ik + resolve and confirm the end joint lands on the
    // target.
    let scene = SceneData {
        name: "ik".into(),
        entities: vec![
            EntityData {
                children: Vec::new(),
                name: "RootBone".into(),
                parent: None,
                components: ComponentBundle {
                    transform: Some(Transform::default()),
                    bone: Some(Bone { name: "root".into(), length: 2.0 }),
                    skeleton_root: Some(SkeletonRoot { name: "rig".into() }),
                    ..Default::default()
                },
            },
            EntityData {
                children: Vec::new(),
                name: "MidBone".into(),
                parent: Some("RootBone".into()),
                components: ComponentBundle {
                    transform: Some(Transform { position: Vec2::new(2.0, 0.0), rotation: 0.0, scale: Vec2::ONE }),
                    bone: Some(Bone { name: "mid".into(), length: 2.0 }),
                    ..Default::default()
                },
            },
            EntityData {
                children: Vec::new(),
                name: "EndBone".into(),
                parent: Some("MidBone".into()),
                components: ComponentBundle {
                    transform: Some(Transform { position: Vec2::new(2.0, 0.0), rotation: 0.0, scale: Vec2::ONE }),
                    bone: Some(Bone { name: "end".into(), length: 0.0 }),
                    ik_two_bone: Some(IkTwoBone { target: [0.0, 3.0], flip_bend: false, ..Default::default() }),
                    ..Default::default()
                },
            },
        ],
        ..Default::default()
    };

    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);
    let end_entity = mapping[2];
    // Verify Parent links resolved.
    assert!(world.get::<&Parent>(mapping[1]).is_ok());
    assert!(world.get::<&Parent>(mapping[2]).is_ok());

    solve_ik(&mut world);
    resolve_world_transforms(&mut world);

    let end_world = world.get::<&WorldTransform>(end_entity).unwrap();
    let dx = end_world.position.x - 0.0;
    let dy = end_world.position.y - 3.0;
    assert!(
        (dx * dx + dy * dy).sqrt() < 1.0e-3,
        "end at {:?}, expected (0,3)",
        end_world.position
    );
}

/// A rig under a scaled or mirrored parent still reaches its target.
///
/// `bone.length` is in the bone's own units, and the hierarchy scales them:
/// a creature authored at scale 2 has legs twice as long on screen as its
/// bone lengths say. A solver that used the raw lengths put the knee in the
/// wrong place and the foot short of the target (or past it) - and a rig
/// mirrored to face the other way (`scale.x = -1`, the usual way a 2D
/// character turns round) bent its knee the wrong way and missed.
#[test]
fn a_scaled_or_mirrored_rig_still_reaches_its_target() {
    use jengine_core::ecs::components::{Name, SkeletonRoot};
    for (scale, target, flip) in [
        (Vec2::new(2.0, 2.0), [3.0, -5.0], false),
        (Vec2::new(0.5, 0.5), [0.8, -1.2], true),
        (Vec2::new(-1.0, 1.0), [-2.0, -2.5], false),
        (Vec2::new(-1.5, 1.5), [-3.0, -3.0], true),
    ] {
        let mut world = hecs::World::new();
        let pivot = world.spawn((
            Name("Pivot".into()),
            Transform { position: Vec2::new(0.0, 0.0), rotation: 0.0, scale },
            SkeletonRoot { name: "rig".into() },
        ));
        let root = world.spawn((Transform::default(), Parent(pivot), Bone { name: "root".into(), length: 2.0 }));
        let mid = world.spawn((
            Transform { position: Vec2::new(2.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
            Parent(root),
            Bone { name: "mid".into(), length: 2.0 },
        ));
        let end = world.spawn((
            Transform { position: Vec2::new(2.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
            Parent(mid),
            Bone { name: "end".into(), length: 0.0 },
            IkTwoBone { target, flip_bend: flip, ..Default::default() },
        ));
        solve_ik(&mut world);
        resolve_world_transforms(&mut world);
        let at = world.get::<&WorldTransform>(end).unwrap().position;
        let miss = at.distance(Vec2::from(target));
        assert!(miss < 1.0e-2, "scale {scale:?} flip {flip}: the tip is at {at:?}, {miss:.3} from {target:?}");
        // Unmirrored, `flip_bend = false` puts the knee clockwise of the
        // hip-to-target line. Mirrored, the whole character is reflected, so
        // the knee goes to the other side - as a turned-round character's
        // knee should.
        let knee = world.get::<&WorldTransform>(mid).unwrap().position;
        let side = Vec2::from(target).perp_dot(knee).signum();
        let mirrored = scale.x * scale.y < 0.0;
        let want = if flip ^ mirrored { 1.0 } else { -1.0 };
        assert_eq!(side, want, "scale {scale:?} flip {flip}: the knee bent to the wrong side");
    }
}
