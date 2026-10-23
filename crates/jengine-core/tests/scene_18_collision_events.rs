//! Test-lab scene 18, end to end: real scene, real graphs, real physics.
//!
//! The scene promises three reactions to physics contacts - the cyan ball
//! turns red, the yellow one despawns, and the purple pings turn a separate
//! counter box green through `EmitEvent` -> `OnEvent`. **None of them had
//! ever happened in any runtime** (BUGS #92): `PhysicsWorld` reports contacts
//! by bare `Entity::id()`, and `dispatch_contact_events` decoded them with
//! `Entity::from_bits`, which rejects a zero generation. The unit tests built
//! their events with `to_bits()`, so they passed.
//!
//! So this drives the frame the way a runtime does, from the project's own
//! files, rather than hand-building events.

#![cfg(feature = "physics")]

use std::path::PathBuf;

use jengine_core::behavior::BehaviorGraphSystem;
use jengine_core::ecs::components::{BehaviorGraphComponent, Name, SpriteComponent};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::physics::PhysicsWorld;
use jengine_core::scene::load_scene_auto;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../projects/engine-test-lab")
}

fn project_physics() -> (f32, f32, f32) {
    let text = std::fs::read_to_string(project_root().join("project.jengine")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    let p = &doc["settings"]["physics"];
    (
        p["gravity_x"].as_f64().unwrap_or(0.0) as f32,
        p["gravity_y"].as_f64().unwrap_or(-9.8) as f32,
        p["length_unit"].as_f64().unwrap_or(100.0) as f32,
    )
}

fn find(world: &hecs::World, name: &str) -> Option<hecs::Entity> {
    world
        .query::<&Name>()
        .iter()
        .find(|(_, n)| n.0 == name)
        .map(|(e, _)| e)
}

fn color(world: &hecs::World, name: &str) -> [f32; 4] {
    let e = find(world, name).unwrap_or_else(|| panic!("{name} is gone"));
    world.get::<&SpriteComponent>(e).unwrap().color
}

#[test]
fn scene_18_reacts_to_contacts() {
    let (gx, gy, lu) = project_physics();
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let root = project_root().join("assets");
    let text = std::fs::read_to_string(root.join("scenes/18-collision-events.scene")).unwrap();
    let mut world = hecs::World::new();
    load_scene_auto(&text, &mut world, &registry).expect("load scene 18");

    // Attach every graph the way the editor viewport does (JSON in hand).
    let mut behaviors = BehaviorGraphSystem::new();
    let graphs: Vec<(hecs::Entity, String)> = world
        .query::<&BehaviorGraphComponent>()
        .iter()
        .map(|(e, c)| (e, c.graph_path.clone()))
        .collect();
    assert_eq!(graphs.len(), 5, "scene 18 authors five graph-driven entities");
    for (e, path) in &graphs {
        let json = std::fs::read_to_string(root.join(path)).unwrap();
        behaviors.attach_inline(*e, PathBuf::from(path), &json).unwrap();
        behaviors.dispatch_spawn(*e, &mut world);
    }

    let cyan = color(&world, "ColorBall");
    let grey = color(&world, "CounterBox");

    let mut physics = PhysicsWorld::new(gx, gy, lu);
    let dt = 1.0 / 60.0;
    for _ in 0..(4.0 / dt) as usize {
        jengine_core::ecs::systems::ensure_physics_bodies(&world, &mut physics);
        behaviors.tick_pre_simulation(&mut world, dt);
        let contacts = physics.step(dt);
        physics.sync_transforms_to_ecs(&mut world);
        behaviors.dispatch_contact_events(&contacts, &mut world);
        behaviors.flush_commands_and_route_events(&mut world, None);
    }

    assert_ne!(
        color(&world, "ColorBall"),
        cyan,
        "the cyan ball landed but never turned red: OnCollision did not fire"
    );
    assert!(
        find(&world, "DespawnBall").is_none(),
        "the yellow ball landed but was not despawned: OnCollision did not fire"
    );
    assert_ne!(
        color(&world, "CounterBox"),
        grey,
        "the purple balls pinged but the counter box never turned green: \
         EmitEvent was not routed to OnEvent"
    );
}
