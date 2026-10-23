//! `on_spawn` / `on_despawn` end-to-end on the native (Luau) path.
//!
//! Both were declared in the docs and indexed by the bridge long before
//! anything dispatched them: `on_spawn` was raised inside `run_frame`, but
//! `notify_despawn` — the only thing that raises `EngineEvent::Despawn` — had
//! no callers at all, in any runtime. Subscribing looked like it worked and
//! silently never fired, which meant a script had no teardown hook: anything
//! it owned that outlived the world (looping audio being the obvious case,
//! since the audio backend is not part of the ECS) stranded with nothing left
//! to stop it.
//!
//! These tests drive the real `ScriptSystem` so a regression in the dispatch
//! wiring fails here rather than in someone's game.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

/// A script that records every lifecycle callback it receives by appending to
/// a tag on its own entity — the simplest channel that survives into the world
/// where the test can read it.
const LIFECYCLE_SRC: &str = r#"
script = { events = { "on_spawn", "on_despawn" } }

function init(entity)
    add_tag(entity, "init")
end

function update(entity, dt)
end

function on_spawn(entity)
    add_tag(entity, "spawned")
end

function on_despawn(entity)
    -- Deliberately touches a command that must still be honoured at teardown.
    add_tag(entity, "despawned")
    log("on_despawn fired")
end

function on_ping(entity)
    add_tag(entity, "pinged")
end

function update_broadcast(entity)
end
"#;

/// Write the script to a real file — `ScriptSystem` resolves `source_path`
/// off disk (`assets/<path>`, then the path as given).
///
/// `name` must be unique per test: these run in parallel, and sharing one path
/// means one test can read the file while another is mid-truncate, which shows
/// up as a script that loaded but did nothing.
fn script_on_disk(name: &str, src: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_lifecycle_test",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.lua"));
    std::fs::write(&path, src).unwrap();
    path
}

fn spawn_scripted(world: &mut World, name: &str, path: &str) -> hecs::Entity {
    world.spawn((
        Name(name.to_string()),
        Transform::default(),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_string(),
            cull_distance: None,
        },
    ))
}

/// Run one script frame.
fn tick(system: &mut ScriptSystem, world: &mut World) {
    let input = InputState::new();
    system.run(world, 1.0 / 60.0, &input, None, None);
}

#[test]
fn on_spawn_fires_once_after_init() {
    let path = script_on_disk("spawn_once", LIFECYCLE_SRC);
    let mut world = World::new();
    let e = spawn_scripted(&mut world, "Subject", path.to_str().unwrap());
    let mut system = ScriptSystem::new().expect("script system");

    // init runs on the first frame; on_spawn on the same or next, but exactly
    // once either way.
    for _ in 0..4 {
        tick(&mut system, &mut world);
    }

    let tags = world
        .get::<&jengine_core::ecs::components::Tags>(e)
        .expect("entity should carry Tags after init");
    let spawned = tags.0.iter().filter(|t| *t == "spawned").count();
    assert!(tags.0.iter().any(|t| t == "init"), "init did not run");
    assert_eq!(spawned, 1, "on_spawn must fire exactly once, got {spawned}");
}

#[test]
fn on_despawn_fires_when_the_entity_leaves_the_world() {
    let path = script_on_disk("despawn_leaves", LIFECYCLE_SRC);
    let mut world = World::new();
    let e = spawn_scripted(&mut world, "Subject", path.to_str().unwrap());
    let mut system = ScriptSystem::new().expect("script system");
    tick(&mut system, &mut world);
    tick(&mut system, &mut world);

    // Remove it the way anything else would — Lifetime, bounds, despawn_entity
    // all land here — and tick again.
    world.despawn(e).unwrap();
    tick(&mut system, &mut world);

    // The entity is gone, so the observable effect is that the bridge no
    // longer holds it: `notify_despawn` cleans up after dispatching.
    assert!(
        !system.lua_bridge.loaded_entity_ids().contains(&(e.id() as u64)),
        "despawned entity must be dropped from the bridge"
    );
}

#[test]
fn scene_teardown_notifies_every_scripted_entity() {
    let path = script_on_disk("teardown_all", LIFECYCLE_SRC);
    let mut world = World::new();
    let a = spawn_scripted(&mut world, "A", path.to_str().unwrap());
    let b = spawn_scripted(&mut world, "B", path.to_str().unwrap());
    let mut system = ScriptSystem::new().expect("script system");
    tick(&mut system, &mut world);
    tick(&mut system, &mut world);

    assert_eq!(system.lua_bridge.loaded_entity_ids().len(), 2);

    // What a scene swap does before replacing the world.
    system.notify_scene_teardown(&mut world, None, None, 0.0);

    assert!(
        system.lua_bridge.loaded_entity_ids().is_empty(),
        "teardown must release every scripted entity"
    );
    // Both handlers ran while the world was still intact, so both tags landed.
    for (entity, who) in [(a, "A"), (b, "B")] {
        let tags = world
            .get::<&jengine_core::ecs::components::Tags>(entity)
            .unwrap_or_else(|_| panic!("{who} lost its Tags"));
        assert!(
            tags.0.iter().any(|t| t == "despawned"),
            "{who} did not receive on_despawn"
        );
    }
}

/// A script that subscribes to a broadcast event and sends one to itself.
const BROADCAST_SRC: &str = r#"
local sent = false

function init(entity)
    on_event("ping", entity)
end

function update(entity, dt)
    if not sent then
        sent = true
        send_event("ping", entity, "hello")
    end
end

function on_ping(entity, data)
    add_tag(entity, "pinged")
end
"#;

/// Broadcast `send_event` shared the same broken dispatch path as the
/// lifecycle hooks: `dispatch_event` called `ScriptAdapter::event`, whose
/// default did nothing and which no adapter overrode. `send_event_to` worked
/// (it goes through `event_with_data`), so the failure looked like "only
/// targeted events work" rather than one unimplemented method.
#[test]
fn broadcast_send_event_reaches_its_handler() {
    let path = script_on_disk("broadcast", BROADCAST_SRC);

    let mut world = World::new();
    let e = spawn_scripted(&mut world, "Listener", path.to_str().unwrap());
    let mut system = ScriptSystem::new().expect("script system");
    for _ in 0..4 {
        tick(&mut system, &mut world);
    }

    let tags = world
        .get::<&jengine_core::ecs::components::Tags>(e)
        .expect("Tags — on_ping should have added one");
    assert!(
        tags.0.iter().any(|t| t == "pinged"),
        "on_ping never fired — broadcast send_event is not reaching handlers"
    );
}
