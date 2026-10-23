//! Round-trip scene serialization for Step 2D behavior components.
//!
//! Verifies the 7 native-behavior components survive a full save→load cycle
//! through both the V1 `ComponentBundle` path and the V2 registry path,
//! and that the bundled `behaviors-demo.scene` example loads cleanly into
//! a hecs world with all expected components present.

use hecs::World;
use jengine_core::ecs::components::{
    Acceleration2D, Ease, EntityRef, FollowTarget, MaxSpeed, Name, Oscillator, PatrolMode,
    PatrolPathRef, SeekTarget, SpriteComponent, Transform, Tween, TweenComplete, TweenTarget,
    Velocity2D,
};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::scene::format::{
    load_scene, load_scene_auto, load_scene_v2, save_scene, save_scene_v2,
};

fn populate_world(world: &mut World) {
    world.spawn((
        Name("OscEntity".into()),
        Transform::default(),
        SpriteComponent::default(),
        Oscillator {
            amplitude: [40.0, 12.0],
            frequency: [0.5, 1.0],
            phase: [0.25, 0.75],
            elapsed: 0.0,
            center: None,
        },
    ));
    world.spawn((
        Name("Mover".into()),
        Transform::default(),
        Velocity2D { x: 10.0, y: 20.0, gravity: 0.0 },
        Acceleration2D { x: 5.0, y: -2.5 },
        MaxSpeed { value: 250.0 },
    ));
    world.spawn((
        Name("Tweener".into()),
        Transform::default(),
        Tween {
            target: TweenTarget::Opacity,
            from: 0.1,
            to: 0.9,
            duration: 2.5,
            elapsed: 0.0,
            ease: Ease::EaseInOutQuad,
            on_complete: TweenComplete::PingPong,
        },
    ));
    world.spawn((
        Name("Follower".into()),
        Transform::default(),
        FollowTarget {
            target: EntityRef::Named("Hero".into()),
            offset: [12.0, -3.0],
            max_speed: 150.0,
            stop_distance: 6.0,
        },
    ));
    world.spawn((
        Name("Seeker".into()),
        Transform::default(),
        Velocity2D::default(),
        SeekTarget {
            target: EntityRef::Named("Hero".into()),
            accel: 333.0,
            max_speed: 222.0,
            slowing_radius: 0.0,
        },
    ));
    world.spawn((
        Name("Patroller".into()),
        Transform::default(),
        PatrolPathRef {
            path_id: "guard_loop".into(),
            speed: 75.0,
            index: 2,
            mode: PatrolMode::PingPong,
            forward: false,
        },
    ));
}

#[test]
fn v1_bundle_roundtrip_preserves_behaviors() {
    let mut world = World::new();
    populate_world(&mut world);

    let scene = save_scene(&world);

    let mut world2 = World::new();
    load_scene(&scene, &mut world2);

    let mut osc_count = 0;
    for (_, osc) in world2.query::<&Oscillator>().iter() {
        osc_count += 1;
        assert!((osc.amplitude[0] - 40.0).abs() < 1e-4);
        assert!((osc.frequency[1] - 1.0).abs() < 1e-4);
        assert!((osc.phase[0] - 0.25).abs() < 1e-4);
    }
    assert_eq!(osc_count, 1);

    let mut acc_count = 0;
    for (_, (acc, ms)) in world2.query::<(&Acceleration2D, &MaxSpeed)>().iter() {
        acc_count += 1;
        assert!((acc.x - 5.0).abs() < 1e-4);
        assert!((acc.y - -2.5).abs() < 1e-4);
        assert!((ms.value - 250.0).abs() < 1e-4);
    }
    assert_eq!(acc_count, 1);

    let mut tween_count = 0;
    for (_, t) in world2.query::<&Tween>().iter() {
        tween_count += 1;
        assert_eq!(t.target, TweenTarget::Opacity);
        assert_eq!(t.ease, Ease::EaseInOutQuad);
        assert_eq!(t.on_complete, TweenComplete::PingPong);
        assert!((t.duration - 2.5).abs() < 1e-4);
    }
    assert_eq!(tween_count, 1);

    let mut follow_count = 0;
    for (_, f) in world2.query::<&FollowTarget>().iter() {
        follow_count += 1;
        match &f.target {
            EntityRef::Named(n) => assert_eq!(n, "Hero"),
            other => panic!("unexpected follow target {other:?}"),
        }
        assert!((f.max_speed - 150.0).abs() < 1e-4);
    }
    assert_eq!(follow_count, 1);

    let mut seek_count = 0;
    for (_, s) in world2.query::<&SeekTarget>().iter() {
        seek_count += 1;
        assert!((s.accel - 333.0).abs() < 1e-4);
        assert!((s.max_speed - 222.0).abs() < 1e-4);
    }
    assert_eq!(seek_count, 1);

    let mut patrol_count = 0;
    for (_, p) in world2.query::<&PatrolPathRef>().iter() {
        patrol_count += 1;
        assert_eq!(p.path_id, "guard_loop");
        assert_eq!(p.mode, PatrolMode::PingPong);
        assert_eq!(p.index, 2);
        assert!(!p.forward);
    }
    assert_eq!(patrol_count, 1);
}

#[test]
fn v2_registry_roundtrip_preserves_behaviors() {
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);

    let mut world = World::new();
    populate_world(&mut world);

    let scene = save_scene_v2(&world, &registry);

    // Sanity: every behavior component name must have been emitted by the
    // registry path, otherwise a missing `register::<T>` call elsewhere
    // would silently drop the component instead of failing here.
    let names: Vec<&str> = scene
        .entities
        .iter()
        .flat_map(|e| e.components.keys().map(String::as_str))
        .collect();
    for expected in [
        "oscillator",
        "acceleration",
        "max_speed",
        "tween",
        "follow_target",
        "seek_target",
        "patrol_path",
    ] {
        assert!(
            names.contains(&expected),
            "v2 scene did not serialize {expected}"
        );
    }

    let mut world2 = World::new();
    load_scene_v2(&scene, &mut world2, &registry);

    let osc_n = world2.query::<&Oscillator>().iter().count();
    let acc_n = world2.query::<&Acceleration2D>().iter().count();
    let max_n = world2.query::<&MaxSpeed>().iter().count();
    let tween_n = world2.query::<&Tween>().iter().count();
    let follow_n = world2.query::<&FollowTarget>().iter().count();
    let seek_n = world2.query::<&SeekTarget>().iter().count();
    let patrol_n = world2.query::<&PatrolPathRef>().iter().count();
    assert_eq!(osc_n, 1);
    assert_eq!(acc_n, 1);
    assert_eq!(max_n, 1);
    assert_eq!(tween_n, 1);
    assert_eq!(follow_n, 1);
    assert_eq!(seek_n, 1);
    assert_eq!(patrol_n, 1);
}

#[test]
fn behaviors_demo_scene_loads() {
    // An authored example in the BunnyMark benchmark project.
    // CARGO_MANIFEST_DIR points at crates/jengine-core, so walk up two levels.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(manifest_dir)
        .join("..")
        .join("..")
        .join("projects")
        .join("bunnymark")
        .join("assets")
        .join("scenes")
        .join("behaviors-demo.scene");
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);

    let mut world = World::new();
    load_scene_auto(&json, &mut world, &registry).expect("load_scene_auto failed");

    // Hero exists.
    let mut found_hero = false;
    for (_, name) in world.query::<&Name>().iter() {
        if name.0 == "Hero" {
            found_hero = true;
        }
    }
    assert!(found_hero, "demo scene missing Hero entity");

    // Each authored behavior is present.
    assert_eq!(world.query::<&Oscillator>().iter().count(), 1);
    assert_eq!(world.query::<&Tween>().iter().count(), 1);
    assert_eq!(world.query::<&FollowTarget>().iter().count(), 1);
    assert_eq!(world.query::<&Acceleration2D>().iter().count(), 1);
    assert!(world.query::<&MaxSpeed>().iter().count() >= 1);
    assert!(world.query::<&Velocity2D>().iter().count() >= 1);
}
