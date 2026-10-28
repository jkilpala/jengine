//! Scripts can steer a trail: `Trail.set_emitting` (a sword's trail only
//! during the swing), `Trail.clear` (a teleport that leaves no streak) and
//! `Trail.set` (a pooled ribbon's colour, Paint Party's team blobs) -
//! natively, and in the browser hosts through the shared arm.

#![cfg(all(feature = "native", feature = "trails"))]

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::components::{Name, ScriptComponent, ScriptLanguage, Trail, TrailPoint, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
use serde_json::json;

const SRC: &str = r#"
local frames = 0
function update(entity, dt)
    frames = frames + 1
    if frames == 1 then
        Trail.set_emitting(Entity.find_by_name("Blade"), false)
        Trail.clear(Entity.find_by_name("Comet"))
        Trail.set(Entity.find_by_name("Blade"), "color", { 0.1, 0.9, 0.6, 1.0 })
        Trail.set(Entity.find_by_name("Blade"), "width", 9)
    end
end
"#;

fn with_points() -> Trail {
    Trail {
        points: vec![TrailPoint { position: Vec2::ZERO, age: 0.1 }, TrailPoint { position: Vec2::X * 10.0, age: 0.0 }],
        ..Trail::default()
    }
}

#[test]
fn scripts_stop_and_clear_trails_natively() {
    let dir = std::env::temp_dir().join(format!(
        "jengine_trail_script_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("trail.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let blade = world.spawn((Name("Blade".into()), Transform::default(), with_points()));
    let comet = world.spawn((Name("Comet".into()), Transform::default(), with_points()));
    world.spawn((
        Name("Driver".into()),
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..3 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    let b = world.get::<&Trail>(blade).unwrap();
    assert!(!b.emitting, "set_emitting(false) stopped the blade's trail");
    assert_eq!(b.points.len(), 2, "stopping keeps the points there to age out");
    assert_eq!(b.color, [0.1, 0.9, 0.6, 1.0], "Trail.set recoloured the blade's trail");
    assert_eq!(b.width, 9.0);
    let c = world.get::<&Trail>(comet).unwrap();
    assert!(c.points.is_empty(), "clear forgot the comet's points");
    assert!(c.emitting);
}

const TELEPORT: &str = r#"
local frames = 0
function update(entity, dt)
    frames = frames + 1
    local comet = Entity.find_by_name("Comet")
    if frames == 2 then
        Transform.set_position(comet, 500, 0)
        Trail.clear(comet)
    end
end
"#;

/// A teleport and a clear in one frame leave one point, where it arrived:
/// the frame's trail tick (after the scripts' commands, as every host poses
/// its world) must not see the old place. Crossfire reported a streak from a
/// parked knife to its throw and parked its trails two frames to dodge it.
#[test]
fn a_teleport_and_a_clear_in_one_frame_leave_no_streak() {
    let dir = std::env::temp_dir().join(format!(
        "jengine_trail_teleport_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("teleport.lua");
    std::fs::write(&path, TELEPORT).unwrap();
    let mut world = World::new();
    let comet = world.spawn((Name("Comet".into()), Transform::default(), with_points()));
    world.spawn((
        Name("Driver".into()),
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..3 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
        jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
        jengine_core::ecs::trails::tick_trails(&mut world, 1.0 / 60.0);
    }
    let c = world.get::<&Trail>(comet).unwrap();
    let xs: Vec<f32> = c.points.iter().map(|p| p.position.x).collect();
    assert_eq!(xs, vec![500.0], "the trail after a teleport and a clear: {xs:?}");
}

const THROW: &str = r#"
local frames = 0
function update(entity, dt)
    frames = frames + 1
    local world = Entity.find_by_name("World")
    local knife = Entity.find_by_name("Knife")
    -- The world parent moves every frame, as Crossfire's does (its recoil).
    Transform.set_position(world, frames * 3, 0)
    if frames == 3 then
        -- A throw from a parked slot: placed, cleared and lit in one frame.
        Transform.set_position(knife, 100, 50)
        Trail.clear(knife)
        Trail.set_emitting(knife, true)
    elseif frames > 3 and frames < 8 then
        Transform.set_position(knife, 100 + (frames - 3) * 20, 50)
    elseif frames == 8 then
        -- Gone: stopped and parked in one frame.
        Trail.set_emitting(knife, false)
        Transform.set_position(knife, -9000, 0)
    end
end
"#;

/// Crossfire's knives: pooled children of a moving `World`, parked far off,
/// thrown by placing, clearing and lighting the trail in one frame, and
/// parked again when they land. Neither the throw nor the park may leave a
/// point at the parking spot - it reported a streak from a knife to its park
/// point and dodged it by waiting two frames.
#[test]
fn a_pooled_child_thrown_and_parked_leaves_no_streak() {
    use jengine_core::ecs::components::Parent;
    let dir = std::env::temp_dir().join(format!(
        "jengine_trail_throw_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("throw.lua");
    std::fs::write(&path, THROW).unwrap();
    let mut world = World::new();
    let parent = world.spawn((Name("World".into()), Transform::default()));
    let knife = world.spawn((
        Name("Knife".into()),
        Transform { position: Vec2::new(-9000.0, 0.0), ..Default::default() },
        Parent(parent),
        Trail { emitting: false, min_distance: 0.0, ..Trail::default() },
    ));
    world.spawn((
        Name("Driver".into()),
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    let mut assets = jengine_core::asset::AssetManager::new();
    let mut seen = Vec::new();
    for _ in 0..10 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
        jengine_core::ecs::systems::pose_world_for_draw(&mut world, &mut assets, None, 1.0 / 60.0);
        seen.extend(world.get::<&Trail>(knife).unwrap().points.iter().map(|p| p.position.x));
    }
    assert!(!seen.is_empty(), "the throw recorded nothing");
    assert!(seen.iter().all(|x| x.abs() < 1000.0), "a point at the parking spot: {seen:?}");
    // What was wrong was the *drawing*: the ribbon ran from the recorded
    // points to the entity's position now, which after the park frame is
    // the parking spot. Drawn as the renderer draws it, from the parked
    // knife, it stays where the knife flew.
    let t = world.get::<&Trail>(knife).unwrap();
    assert!(!t.emitting && !t.points.is_empty(), "stopped, with points still fading");
    let head = world.get::<&jengine_core::ecs::components::WorldTransform>(knife).unwrap().position;
    assert!(head.x < -8000.0, "the knife is parked");
    let r = jengine_core::ecs::trails::ribbon(&t, head).expect("the fading streak is drawn");
    assert!(r.positions.iter().all(|p| p.x > -1000.0), "the ribbon reaches the parking spot");
}

struct Host(World);

impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

#[test]
fn the_browser_hosts_steer_trails_through_the_shared_arm() {
    let mut host = Host(World::new());
    let e = host.0.spawn((Transform::default(), with_points()));
    assert!(handle_world_command(&mut host, "tre", &JsonArgs::new(json!({ "e": e.id(), "on": false }))));
    assert!(!host.0.get::<&Trail>(e).unwrap().emitting);
    assert!(handle_world_command(&mut host, "tre", &JsonArgs::new(json!({ "e": e.id(), "on": true }))));
    assert!(host.0.get::<&Trail>(e).unwrap().emitting);
    assert!(handle_world_command(&mut host, "strl", &JsonArgs::new(json!({ "e": e.id(), "f": "color", "v": [1.0, 0.4, 0.7, 1.0] }))));
    assert_eq!(host.0.get::<&Trail>(e).unwrap().color, [1.0, 0.4, 0.7, 1.0]);
    assert_eq!(host.0.get::<&Trail>(e).unwrap().points.len(), 2, "a recolour keeps the ribbon");
    assert!(handle_world_command(&mut host, "trc", &JsonArgs::new(json!({ "e": e.id() }))));
    assert!(host.0.get::<&Trail>(e).unwrap().points.is_empty());
}
