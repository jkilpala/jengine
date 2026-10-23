//! A package's script global runs the same handler in every runtime.
//!
//! This is the guard on the thing that was broken: `install_script_api`
//! handlers were installed only by `ScriptSystem`, so a package's globals were
//! `nil` in the editor viewport and web builds — the runtimes this engine is
//! actually developed in. The shared world-command path was the mirror image,
//! dispatched only by the browser hosts.
//!
//! The pieces below are the ones a runtime uses, exercised natively:
//!
//! - **Native** turns a call into `BridgeCommand::PackageCall` and applies it
//!   through `dispatch_global`.
//! - **Browser hosts** build the same `(tag, args)` from the preamble JS this
//!   module generates, and dispatch through the same function.
//!
//! What a browser actually does with the generated JS cannot be run here, so
//! the generated source is asserted against the shape the hosts parse.

use hecs::{Entity, World};
use jengine_core::ecs::components::Transform;
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::host::PackageHost;
use jengine_core::package::script_commands::{
    self, PackageCommandRegistry,
};
use jengine_core::package::{JEnginePackage, PackageMetadata};
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A package whose command moves an entity, so the effect is observable in the
/// world rather than only in a counter.
struct MoverPackage {
    calls: Arc<AtomicUsize>,
}

impl JEnginePackage for MoverPackage {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "test-mover".into(),
            version: "0.1.0".into(),
            description: "moves things".into(),
            author: String::new(),
            engine_version: None,
            dependencies: vec![],
        }
    }
    fn register_components(&self, _r: &mut ComponentRegistry) {}
    fn register_systems(&self, _s: &mut jengine_core::package::system_schedule::SystemSchedule) {}
    fn register_script_api(&self, _a: &mut jengine_core::package::script_api::ScriptApiRegistry) {}

    fn register_script_commands(&self, commands: &mut PackageCommandRegistry) {
        let calls = Arc::clone(&self.calls);
        commands
            .register("test-mover", "nudge", &["e", "dx"], move |host, args| {
                calls.fetch_add(1, Ordering::SeqCst);
                let dx = args.number("dx").unwrap_or(0.0) as f32;
                // `e` is the conventional entity field, so the shared
                // `target` helper resolves it the same way core's do.
                let id = args.number("e").unwrap_or(-1.0);
                if id < 0.0 {
                    return;
                }
                if let Some(entity) = host.resolve_entity(id as u32)
                    && let Ok(mut t) = host.world().get::<&mut Transform>(entity)
                {
                    t.position.x += dx;
                }
            })
            .expect("registering a command");
    }
}

/// The minimal host a runtime provides.
struct TestHost {
    world: World,
    entity: Entity,
}

impl ScriptWorldHost for TestHost {
    fn world(&mut self) -> &mut World {
        &mut self.world
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        (id == self.entity.id()).then_some(self.entity)
    }
}

fn hosted(calls: Arc<AtomicUsize>) -> PackageHost {
    let mut registry = ComponentRegistry::new();
    jengine_core::package::core_registration::register_core_components(&mut registry);
    let mut host = PackageHost::new();
    host.add_package(Box::new(MoverPackage { calls }));
    host.initialize(&mut registry);
    host
}

fn world_with_entity() -> TestHost {
    let mut world = World::new();
    let entity = world.spawn((Transform::default(),));
    TestHost { world, entity }
}

#[test]
fn a_package_command_is_registered_through_the_host() {
    let host = hosted(Arc::new(AtomicUsize::new(0)));
    let commands = host.script_commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands.names(), vec!["nudge".to_string()]);
    assert_eq!(commands.commands()[0].tag, "pkg/test-mover/nudge");
}

#[test]
fn the_handler_mutates_the_world_through_the_shared_dispatcher() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut host = hosted(Arc::clone(&calls));
    let registry = host.take_script_commands();

    let mut world_host = world_with_entity();
    let id = world_host.entity.id();

    let handled = script_commands::handle_package_command(
        &mut world_host,
        "pkg/test-mover/nudge",
        &JsonArgs::new(json!({ "e": id, "dx": 5.0 })),
        &registry,
    );

    assert!(handled);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let t = world_host.world.get::<&Transform>(world_host.entity).unwrap();
    assert_eq!(t.position.x, 5.0, "the handler reached the real world");
}

#[test]
fn the_process_wide_registry_serves_the_same_handler() {
    // How the native apply loop and the browser hosts both reach it: neither
    // can borrow a registry owned by the thing they are mutating.
    let calls = Arc::new(AtomicUsize::new(0));
    let mut host = hosted(Arc::clone(&calls));
    script_commands::install_global(host.take_script_commands());

    let mut world_host = world_with_entity();
    let id = world_host.entity.id();
    let handled = script_commands::dispatch_global(
        &mut world_host,
        "pkg/test-mover/nudge",
        &JsonArgs::new(json!({ "e": id, "dx": -2.0 })),
    );

    assert!(handled);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let x = world_host
        .world
        .get::<&Transform>(world_host.entity)
        .unwrap()
        .position
        .x;
    assert_eq!(x, -2.0);
    script_commands::clear_global();
}

#[test]
fn a_core_command_still_falls_through_to_the_host() {
    // The dispatcher must claim only `pkg/` tags, or it would swallow core's
    // commands and every animation call would stop working.
    let mut host = hosted(Arc::new(AtomicUsize::new(0)));
    script_commands::install_global(host.take_script_commands());
    let mut world_host = world_with_entity();

    for tag in ["pa", "sa", "st", "log"] {
        assert!(
            !script_commands::dispatch_global(&mut world_host, tag, &JsonArgs::new(json!({}))),
            "'{tag}' was wrongly claimed"
        );
    }
    script_commands::clear_global();
}

#[test]
fn the_generated_browser_js_matches_what_the_hosts_parse() {
    // The browser half of the bridge. A host reads `{ t: <tag>, ...fields }`
    // off the command queue, so the generated global has to produce exactly
    // that shape - there is no type system spanning this boundary.
    let host = hosted(Arc::new(AtomicUsize::new(0)));
    let js = host.script_commands().browser_globals_js();

    assert!(js.contains("function nudge(a0, a1)"), "{js}");
    assert!(js.contains("window.__jengine_play.commands.push("), "{js}");
    assert!(js.contains("t: 'pkg/test-mover/nudge'"), "{js}");
    // Positional arguments land under the declared names, which is what lets
    // the handler read `args.number("dx")`.
    assert!(js.contains("e: a0"), "{js}");
    assert!(js.contains("dx: a1"), "{js}");
}

#[test]
fn a_second_package_cannot_shadow_an_existing_global() {
    // Bridge globals are a flat namespace; a silent shadow would mean the
    // first package's function stops working with nothing to show why.
    let mut registry = PackageCommandRegistry::new();
    registry.register("a", "nudge", &[], |_, _| {}).unwrap();
    let err = registry.register("b", "nudge", &[], |_, _| {}).unwrap_err();
    assert!(err.contains("already registered"), "{err}");
}

#[test]
fn a_command_that_would_need_a_return_value_is_refused() {
    // The honest limit of this mechanism, enforced where it is cheap to see.
    let mut registry = PackageCommandRegistry::new();
    let err = registry
        .register("p", "get_score", &[], |_, _| {})
        .unwrap_err();
    assert!(err.contains("cannot return a value"), "{err}");
}
