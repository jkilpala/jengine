//! **`set_body_type` sticks** (Sep 2026). `ensure_physics_bodies` makes each
//! body match its `RigidBody2D` every frame - that is how an inspector edit
//! reaches the simulation - and the script command changed only the body,
//! so the next frame put it back: a kinematic ball switched to dynamic hung
//! where it was. Found by Test Lab scene 138, whose balls never fell. Every
//! runtime now goes through `set_entity_body_type`, which writes both.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, RigidBody2D, Transform};
use jengine_core::ecs::systems::{apply_physics_commands, ensure_physics_bodies, set_entity_body_type};
use jengine_core::physics::PhysicsWorld;
use jengine_core::scripting::api::ScriptCommand;

const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/script_host.rs");

/// A kinematic box at y 300, switched by `switch` on frame 5; its y a second later.
fn fall(switch: impl Fn(&World, &mut PhysicsWorld, hecs::Entity)) -> f32 {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, -9.8, 100.0);
    let col: Collider2D =
        serde_json::from_value(serde_json::json!({ "shape": { "Box": { "width": 40.0, "height": 40.0 } } })).unwrap();
    let rb: RigidBody2D = serde_json::from_value(serde_json::json!({ "body_type": "Kinematic" })).unwrap();
    let e = world.spawn((Transform { position: Vec2::new(0.0, 300.0), rotation: 0.0, scale: Vec2::ONE }, col, rb));
    for i in 0..60 {
        ensure_physics_bodies(&world, &mut physics);
        if i == 5 {
            switch(&world, &mut physics, e);
        }
        physics.sync_kinematic_from_ecs(&world);
        physics.step(1.0 / 60.0);
        physics.sync_transforms_to_ecs(&mut world);
    }
    world.get::<&Transform>(e).unwrap().position.y
}


/// The shared command arms both browser hosts call (`adapters::js::host_dispatch`).
const DISPATCH: &str = include_str!("../src/scripting/bridge/adapters/js/host_dispatch.rs");

/// A host's source as far as its commands go: its own, plus the shared arms -
/// but only if it calls them, so a host that dropped the call fails.
fn with_dispatch(src: &str) -> String {
    if src.contains("host_dispatch::handle_host_command(") {
        format!("{src}
{DISPATCH}")
    } else {
        src.to_string()
    }
}

#[test]
fn a_kinematic_body_made_dynamic_falls() {
    let y = fall(|w, p, e| set_entity_body_type(w, p, e, BodyType::Dynamic));
    assert!(y < -50.0, "made dynamic, it falls: y {y}");
    // Native's script path.
    let y = fall(|w, p, e| {
        let cmd = ScriptCommand::SetBodyType { entity_id: e.id() as u64, body_type: "dynamic".into() };
        apply_physics_commands(&[cmd], p, w);
    });
    assert!(y < -50.0, "through the script command too: y {y}");
    // What it used to be: the body alone, put back by the next frame.
    let y = fall(|_, p, e| p.set_body_type(e.id() as u64, BodyType::Dynamic));
    assert!(y > 250.0, "the body alone is reverted by the component: y {y}");
}

#[test]
fn browser_hosts_set_the_component_too() {
    for (name, src) in [("viewport", with_dispatch(VIEWPORT)), ("web", with_dispatch(WEB))] {
        assert!(src.contains("set_entity_body_type("), "{name} sets the body type without the component");
    }
}
