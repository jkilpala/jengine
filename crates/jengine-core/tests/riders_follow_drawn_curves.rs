//! `path_follow` (`ecs::path_follow`): an entity rides another entity's
//! `vector_path` at an even speed - looping, ping-ponging or stopping at the
//! end with `on_path_end` - turned along it, in world space.

use std::f32::consts::FRAC_PI_2;

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::components::{Name, PathCommand, Transform, VectorPath};
use jengine_core::ecs::path_follow::{FollowMode, PathFollow, set_path_follow_field, tick_path_follow};

const DT: f32 = 1.0 / 60.0;

fn route(world: &mut World, name: &str, at: Transform, commands: Vec<PathCommand>, closed: bool) -> Entity {
    world.spawn((
        Name(name.into()),
        at,
        VectorPath {
            commands,
            fill_color: None,
            stroke_color: Some([1.0; 4]),
            stroke_width: 2.0,
            z_order: 0,
            closed,
            sort_layer: None,
        },
    ))
}

/// A straight line 300 long from (0, 0) to (300, 0).
fn line(world: &mut World) -> Entity {
    route(
        world,
        "Line",
        Transform::default(),
        vec![PathCommand::MoveTo { x: 0.0, y: 0.0 }, PathCommand::LineTo { x: 300.0, y: 0.0 }],
        false,
    )
}

fn rider(world: &mut World, f: PathFollow) -> Entity {
    world.spawn((Transform::default(), f))
}

fn pos(world: &World, e: Entity) -> Vec2 {
    world.get::<&Transform>(e).unwrap().position
}

fn rot(world: &World, e: Entity) -> f32 {
    world.get::<&Transform>(e).unwrap().rotation
}

#[test]
fn it_rides_at_its_speed_facing_along_and_beside_the_line() {
    let mut world = World::new();
    line(&mut world);
    let r = rider(&mut world, PathFollow { path: "Line".into(), speed: 60.0, lateral: 10.0, ..Default::default() });
    for _ in 0..60 {
        tick_path_follow(&mut world, DT);
    }
    let p = pos(&world, r);
    assert!((p.x - 60.0).abs() < 0.01, "a second at 60 a second: {p}");
    assert!((p.y - 10.0).abs() < 0.01, "ten to its left, facing +x: {p}");
    assert!(rot(&world, r).abs() < 1e-4);
}

#[test]
fn the_three_modes_at_the_end() {
    let mut world = World::new();
    line(&mut world);
    let looped = rider(&mut world, PathFollow { path: "Line".into(), speed: 100.0, progress: 280.0, ..Default::default() });
    let bounce = rider(&mut world, PathFollow {
        path: "Line".into(),
        speed: 100.0,
        progress: 280.0,
        mode: FollowMode::PingPong,
        ..Default::default()
    });
    let once = rider(&mut world, PathFollow {
        path: "Line".into(),
        speed: 100.0,
        progress: 280.0,
        mode: FollowMode::Once,
        ..Default::default()
    });
    jengine_core::ecs::callbacks::take();
    for _ in 0..30 {
        tick_path_follow(&mut world, DT);
    }
    // Half a second at 100 from 280: 30 past the end.
    assert!((pos(&world, looped).x - 30.0).abs() < 0.1, "back round to 30: {}", pos(&world, looped));
    assert!((pos(&world, bounce).x - 270.0).abs() < 0.1, "turned at 300, back to 270: {}", pos(&world, bounce));
    assert!((rot(&world, bounce).abs() - std::f32::consts::PI).abs() < 1e-3, "facing back: {}", rot(&world, bounce));
    assert!((pos(&world, once).x - 300.0).abs() < 1e-3, "stopped at the end");
    assert!(!world.get::<&PathFollow>(once).unwrap().playing);
    let owed = jengine_core::ecs::callbacks::take();
    assert_eq!(owed.len(), 1, "on_path_end once: {owed:?}");
    assert_eq!((owed[0].entity, owed[0].handler), (once, "on_path_end"));
}

#[test]
fn a_curve_is_ridden_at_an_even_speed() {
    let mut world = World::new();
    // A half-circle-ish arch, as a cubic.
    route(
        &mut world,
        "Arch",
        Transform::default(),
        vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::CubicTo { c1x: 0.0, c1y: 200.0, c2x: 300.0, c2y: 200.0, x: 300.0, y: 0.0 },
        ],
        false,
    );
    let r = rider(&mut world, PathFollow { path: "Arch".into(), speed: 120.0, mode: FollowMode::Once, ..Default::default() });
    let mut last = Vec2::ZERO;
    let mut steps = Vec::new();
    for _ in 0..120 {
        tick_path_follow(&mut world, DT);
        let p = pos(&world, r);
        if world.get::<&PathFollow>(r).unwrap().playing {
            steps.push(p.distance(last));
        }
        last = p;
    }
    // Each frame covers 2 units along the curve; a chord is a hair shorter.
    for s in &steps[1..] {
        assert!((*s - 2.0).abs() < 0.05, "even: {s}");
    }
    // Starting up the arch: facing up at first.
    let mut world2 = World::new();
    route(
        &mut world2,
        "Arch",
        Transform::default(),
        vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::CubicTo { c1x: 0.0, c1y: 200.0, c2x: 300.0, c2y: 200.0, x: 300.0, y: 0.0 },
        ],
        false,
    );
    let r2 = rider(&mut world2, PathFollow { path: "Arch".into(), speed: 1.0, ..Default::default() });
    tick_path_follow(&mut world2, DT);
    assert!((rot(&world2, r2) - FRAC_PI_2).abs() < 0.2, "faces up the arch: {}", rot(&world2, r2));
}

#[test]
fn a_closed_path_loops_seamlessly_and_the_path_carries_its_riders() {
    let mut world = World::new();
    let square = route(
        &mut world,
        "Square",
        Transform { position: Vec2::new(1000.0, 0.0), ..Default::default() },
        vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::LineTo { x: 100.0, y: 0.0 },
            PathCommand::LineTo { x: 100.0, y: 100.0 },
            PathCommand::LineTo { x: 0.0, y: 100.0 },
            PathCommand::Close,
        ],
        false,
    );
    let r = rider(&mut world, PathFollow { path: "Square".into(), speed: 0.0, progress: 350.0, ..Default::default() });
    tick_path_follow(&mut world, DT);
    // 350 of a 400 loop: half-way up the closing side, in world space.
    let p = pos(&world, r);
    assert!((p - Vec2::new(1000.0, 50.0)).length() < 0.01, "{p}");
    // Turn the path a quarter: the rider goes with it.
    world.get::<&mut Transform>(square).unwrap().rotation = FRAC_PI_2;
    tick_path_follow(&mut world, DT);
    let p = pos(&world, r);
    assert!((p - Vec2::new(950.0, 0.0)).length() < 0.01, "{p}");
}

#[test]
fn a_script_switches_tracks_and_jumps() {
    let mut world = World::new();
    line(&mut world);
    route(
        &mut world,
        "Up",
        Transform::default(),
        vec![PathCommand::MoveTo { x: 0.0, y: 0.0 }, PathCommand::LineTo { x: 0.0, y: 500.0 }],
        false,
    );
    let r = rider(&mut world, PathFollow { path: "Line".into(), speed: 0.0, ..Default::default() });
    set_path_follow_field(&world, r, "path", &serde_json::json!("Up")).unwrap();
    set_path_follow_field(&world, r, "progress", &serde_json::json!(200.0)).unwrap();
    set_path_follow_field(&world, r, "mode", &serde_json::json!("ping_pong")).unwrap();
    assert!(set_path_follow_field(&world, r, "colour", &serde_json::json!(1)).is_err());
    tick_path_follow(&mut world, DT);
    assert!((pos(&world, r) - Vec2::new(0.0, 200.0)).length() < 0.01);
    assert_eq!(world.get::<&PathFollow>(r).unwrap().mode, FollowMode::PingPong);
}

#[test]
fn every_host_has_it_through_the_motion_step_and_the_scene_keeps_it() {
    let mut world = World::new();
    line(&mut world);
    let r = rider(&mut world, PathFollow { path: "Line".into(), speed: 60.0, ..Default::default() });
    jengine_core::ecs::behaviors::tick_motion(&mut world, 0.5, None, false);
    assert!((pos(&world, r).x - 30.0).abs() < 0.01, "tick_motion rides it");
    let scene = jengine_core::scene::format::save_scene(&world);
    let f = scene.entities.iter().find_map(|e| e.components.path_follow.as_ref()).expect("saved");
    assert_eq!(f.path, "Line");
    assert!((f.progress - 30.0).abs() < 0.01);
}

#[cfg(feature = "native")]
#[test]
fn the_native_script_hears_the_end() {
    use jengine_core::ecs::components::{CustomComponents, ScriptComponent, ScriptLanguage};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::input::InputState;

    let dir = std::env::temp_dir().join(format!("jengine_path_end_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("train.lua");
    std::fs::write(&path, r#"
local ends = 0
function update(entity, dt)
end

function on_path_end(entity)
    ends = ends + 1
    set_custom(entity, "Probe", "ends", ends)
end
"#).unwrap();
    let mut world = World::new();
    line(&mut world);
    let r = world.spawn((
        Transform::default(),
        CustomComponents(Default::default()),
        PathFollow { path: "Line".into(), speed: 600.0, mode: FollowMode::Once, ..Default::default() },
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    jengine_core::ecs::callbacks::take();
    for _ in 0..60 {
        system.run(&mut world, DT, &input, None, None);
        tick_path_follow(&mut world, DT);
        system.queue_entity_callbacks(&jengine_core::ecs::callbacks::take());
    }
    system.run(&mut world, DT, &input, None, None);
    let ends = world
        .get::<&CustomComponents>(r)
        .unwrap()
        .0
        .get("Probe")
        .and_then(|p| p.get("ends"))
        .and_then(|v| v.as_f64());
    assert_eq!(ends, Some(1.0));
    let _ = std::fs::remove_dir_all(&dir);
}
