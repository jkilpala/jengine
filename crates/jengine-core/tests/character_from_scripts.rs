//! A script drives a character controller: `move_character` asks for a
//! velocity, the physics step moves the body, and `get_character` reads back
//! what happened - natively through the Luau globals, and in the browser
//! hosts through the shared `mvc` command arm.

#![cfg(all(feature = "native", feature = "physics"))]

use hecs::{Entity, World};
use jengine_core::ecs::components::{
    BodyType, CharacterController2D, Collider2D, ColliderShape, CustomComponents, Name, RigidBody2D, ScriptComponent,
    ScriptLanguage, Transform,
};
use jengine_core::ecs::systems::{ScriptSystem, ensure_physics_bodies};
use jengine_core::input::InputState;
use jengine_core::physics::PhysicsWorld;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
use serde_json::json;

/// Walks right with gravity, jumps once when it lands, and records what it
/// saw.
const SRC: &str = r#"
local vy = 0
local jumped = false
function update(entity, dt)
    local c = get_character(entity)
    if c ~= nil then
        if c.grounded then
            set_custom(entity, "Probe", "landed", 1)
            vy = -50
            if not jumped then
                vy = 500
                jumped = true
            end
        else
            vy = vy - 1400 * dt
        end
        if c.wall == 1 then
            set_custom(entity, "Probe", "wall", 1)
        end
        set_custom(entity, "Probe", "vx", c.vx)
    end
    move_character(entity, 240, vy)
end
"#;

fn at(x: f32, y: f32) -> Transform {
    Transform { position: glam::Vec2::new(x, y), ..Default::default() }
}

#[test]
fn a_luau_script_walks_jumps_and_meets_a_wall() {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_character_script_test",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hero.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    world.spawn((
        at(0.0, 0.0),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 1000.0, height: 20.0 }, ..Default::default() },
    ));
    world.spawn((
        at(300.0, 60.0),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 20.0, height: 100.0 }, ..Default::default() },
    ));
    let hero = world.spawn((
        Name("Hero".into()),
        at(0.0, 60.0),
        RigidBody2D { body_type: BodyType::Kinematic, fixed_rotation: true, ..Default::default() },
        Collider2D { shape: ColliderShape::Capsule { half_height: 8.0, radius: 12.0 }, ..Default::default() },
        CharacterController2D::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    let dt = 1.0 / 60.0;
    let mut peak: f32 = 0.0;
    for _ in 0..180 {
        system.run(&mut world, dt, &input, None, None);
        ensure_physics_bodies(&mut world, &mut physics);
        physics.sync_kinematic_from_ecs(&world);
        physics.move_characters(&mut world, dt);
        physics.step(dt);
        peak = peak.max(world.get::<&Transform>(hero).unwrap().position.y);
    }
    let probe = world.get::<&CustomComponents>(hero).unwrap().0.get("Probe").cloned().expect("the script ran");
    assert_eq!(probe["landed"], 1.0, "get_character reported the ground");
    assert_eq!(probe["wall"], 1.0, "and the wall on its right");
    assert!(peak > 60.0, "it jumped ({peak})");
    let x = world.get::<&Transform>(hero).unwrap().position.x;
    assert!(x < 290.0 - 12.0 + 0.1 && x > 260.0, "stopped at the wall: {x}");
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
fn the_browser_hosts_move_a_character_through_the_shared_arm() {
    let mut host = Host(World::new());
    let e = host.0.spawn((at(0.0, 0.0), CharacterController2D::default()));
    assert!(handle_world_command(&mut host, "mvc", &JsonArgs::new(json!({ "e": e.id(), "vx": 120.0, "vy": -5.0, "drop": true }))));
    let cc = (*host.0.get::<&CharacterController2D>(e).unwrap()).clone();
    assert_eq!(cc.velocity, [120.0, -5.0]);
    assert!(cc.drop_through);
    assert!(handle_world_command(&mut host, "mvc", &JsonArgs::new(json!({ "e": e.id(), "vx": 0.0, "vy": 0.0 }))));
    assert!(host.0.get::<&CharacterController2D>(e).unwrap().drop_through, "drop left as it was when not given");
}
