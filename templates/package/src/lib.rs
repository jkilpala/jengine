//! {{PACKAGE_NAME}} - TODO: one line on what this package is for.
//!
//! A package adds four kinds of thing to the engine, and this file has one of
//! each to copy from:
//!
//! 1. **A component** - plain data on an entity ([`{{COMPONENT_STRUCT}}`]).
//!    Registered, it can be added and edited in the editor's inspector, which
//!    is derived from the struct itself.
//! 2. **A system** - a function the engine calls every frame
//!    ([`drift_movers`]).
//! 3. **A script command** - a function Lua calls, `{{NAMESPACE}}.nudge(e, amount)`.
//!    It reaches every runtime: native, the editor and the web.
//! 4. **Published state** - what a script reads back. A command cannot return
//!    a value, so the system writes what scripts need into the entity's custom
//!    component, where `Entity.get_custom` reads it.
//!
//! Keep the rules themselves in plain functions of numbers ([`drift`]): they
//! are the part worth testing, and a test of one needs no world.
//!
//! From Lua, on an entity carrying a `{{COMPONENT_KEY}}`:
//!
//! ```lua
//! function update(entity, dt)
//!     if Input.is_action_just_pressed("confirm") then
//!         {{NAMESPACE}}.nudge(entity, 50)
//!     end
//!     local travelled = Entity.get_custom(entity, "{{COMPONENT_STRUCT}}", "travelled")
//! end
//! ```
//!
//! After changing this crate, rebuild the editor viewport (the editor offers
//! it) so the editor runs the new code. A scene using the component must be a
//! v2 scene (`"version": 2`).
//!
//! README.md beside this crate is its documentation. See
//! `docs/writing-packages.md` in the engine for the contract a package works
//! under.

use jengine_core::ecs::World;
use jengine_core::ecs::components::{CustomComponents, Transform};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::script_api::ScriptApiRegistry;
use jengine_core::package::script_commands::PackageCommandRegistry;
use jengine_core::package::system_schedule::{SystemPhase, SystemSchedule};
use jengine_core::package::{JEnginePackage, PackageMetadata};
use serde::{Deserialize, Serialize};

// ── 1. Components ──────────────────────────────────────────────────────

/// Moves its entity along x. The scene key is `{{COMPONENT_KEY}}`.
///
/// Every field is saved in the scene and shown in the inspector; `#[serde(skip)]`
/// keeps one out of both (runtime state).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct {{COMPONENT_STRUCT}} {
    /// Units per second the entity drifts along x (the world is about a
    /// centimetre a unit).
    pub speed: f32,
    /// How far it has moved since the scene started - runtime state, which
    /// [`drift_movers`] publishes for scripts.
    #[serde(skip)]
    pub travelled: f32,
}

// ── The rules, as plain functions ──────────────────────────────────────

/// How far something moving at `speed` goes in `dt` seconds.
pub fn drift(speed: f32, dt: f32) -> f32 {
    speed * dt
}

// ── 2. Systems ─────────────────────────────────────────────────────────

/// Move every [`{{COMPONENT_STRUCT}}`] and publish how far it has gone.
///
/// A free function rather than a closure in `register_systems`, so a test can
/// call it on a world of its own.
pub fn drift_movers(world: &mut World, dt: f32) {
    // A component cannot be added to an entity while a query holds the world,
    // so first give every mover the custom component its state is published
    // into, then move them.
    let bare: Vec<_> = world
        .query::<&{{COMPONENT_STRUCT}}>()
        .without::<&CustomComponents>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    for e in bare {
        let _ = world.insert_one(e, CustomComponents::default());
    }

    for (_, (it, transform, custom)) in
        world.query_mut::<(&mut {{COMPONENT_STRUCT}}, &mut Transform, &mut CustomComponents)>()
    {
        let step = drift(it.speed, dt);
        transform.position.x += step;
        it.travelled += step.abs();
        // 4. Published: `Entity.get_custom(e, "{{COMPONENT_STRUCT}}", "travelled")`.
        custom
            .0
            .insert("{{COMPONENT_STRUCT}}".into(), serde_json::json!({ "travelled": it.travelled }));
    }
}

// ── The package ────────────────────────────────────────────────────────

/// The package. The builder links it as `{{ENTRY_POINT}}`.
pub struct {{PACKAGE_STRUCT}};

impl JEnginePackage for {{PACKAGE_STRUCT}} {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "{{PACKAGE_NAME}}".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "TODO: describe your package".to_string(),
            author: String::new(),
            engine_version: Some("0.1.0".to_string()),
            // Every `jengine-*` package this crate depends on, by name.
            dependencies: vec![],
        }
    }

    fn register_components(&self, registry: &mut ComponentRegistry) {
        // (scene key, name shown in the editor, the inspector's category).
        registry.register_defaulted::<{{COMPONENT_STRUCT}}>("{{COMPONENT_KEY}}", "{{COMPONENT_STRUCT}}", "Custom");
    }

    fn register_systems(&self, schedule: &mut SystemSchedule) {
        // PreUpdate runs before the scripts, so they see this frame's moves.
        // Render and PostRender are not run: a package cannot draw directly.
        schedule.add_system("{{COMPONENT_KEY}}_drift", SystemPhase::PreUpdate, 0, |ctx| {
            let (world, dt, _resources) = ctx.split();
            drift_movers(world, dt);
        });
    }

    // `register_script_api` handlers are installed by the native runtime only;
    // use `register_script_commands`, which every runtime dispatches.
    fn register_script_api(&self, _api: &mut ScriptApiRegistry) {}

    // ── 3. Script commands ─────────────────────────────────────────────
    fn register_script_commands(&self, commands: &mut PackageCommandRegistry) {
        // Scripts call this `{{NAMESPACE}}.nudge(e, amount)`: the namespace comes
        // from the package name, and a prefix every command shares is dropped.
        // The list names the arguments in the order a script passes them; an
        // argument called `e` is an entity. A name starting `get_` / `is_` /
        // `has_` is refused - a command returns nothing (see 4 above).
        let registered = commands.register("{{PACKAGE_NAME}}", "{{COMMAND_PREFIX}}_nudge", &["e", "amount"], |host, args| {
            let amount = args.number("amount").unwrap_or(0.0) as f32;
            let Some(e) = args.number("e").filter(|v| *v >= 0.0).and_then(|v| host.resolve_entity(v as u32)) else {
                return;
            };
            if let Ok(mut transform) = host.world().get::<&mut Transform>(e) {
                transform.position.x += amount;
            }
        });
        if let Err(err) = registered {
            log::error!("[{{PACKAGE_NAME}}] script commands not registered: {err}");
        }
    }
}

// `cargo test` in this folder runs these.
#[cfg(test)]
mod tests {
    use super::*;
    use jengine_core::package::host::PackageHost;

    #[test]
    fn drift_is_speed_times_time() {
        assert_eq!(drift(100.0, 0.5), 50.0);
        assert_eq!(drift(-20.0, 1.0), -20.0);
    }

    #[test]
    fn movers_move_and_publish_how_far() {
        let mut world = World::new();
        let e = world.spawn((Transform::default(), {{COMPONENT_STRUCT}} { speed: 100.0, ..Default::default() }));
        drift_movers(&mut world, 0.5);
        drift_movers(&mut world, 0.5);

        assert_eq!(world.get::<&Transform>(e).unwrap().position.x, 100.0);
        let custom = world.get::<&CustomComponents>(e).unwrap();
        assert_eq!(custom.0["{{COMPONENT_STRUCT}}"]["travelled"], 100.0);
    }

    #[test]
    fn registers_with_a_host() {
        let mut registry = ComponentRegistry::new();
        jengine_core::package::core_registration::register_core_components(&mut registry);
        let mut host = PackageHost::new();
        host.add_package(Box::new({{PACKAGE_STRUCT}}));
        host.initialize(&mut registry);
        assert!(host.systems().iter().any(|(name, _, _)| name.contains("drift")));
        assert!(host.script_commands().names().iter().any(|n| n.ends_with("_nudge")));
    }
}
