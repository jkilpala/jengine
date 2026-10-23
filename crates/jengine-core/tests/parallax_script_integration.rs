//! Slice A end-to-end integration test.
//!
//! Drives the FULL chain that production scripts will use:
//!   Lua `set_parallax_auto_scroll(e, 10, 5)`
//!     → handler closure pushes `BridgeCommand::SetComponentField`
//!     → `apply_bridge_commands` → translator → `apply_script_commands`
//!     → global `ComponentRegistry` lookup → JSON-roundtrip patch
//!     → `ParallaxLayer.auto_scroll_x/y` mutated on the live hecs entity.
//!
//! This proves the design-doc Slice A contract: a community package can
//! register a Rust component + a script-facing function, and a script
//! mutating that component sees the change land on the ECS world.

#![cfg(feature = "native")]

use std::sync::Arc;

use jengine_core::ecs::World;
use jengine_core::ecs::systems::apply_bridge_commands;
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::script_api::ScriptApiRegistry;
use jengine_core::package::{set_global_component_registry, JEnginePackage};
use jengine_core::scripting::bridge::{
    BridgeCommand, BridgeContext, BridgeReadSnapshot, Capabilities, LuauAdapter, ScriptAdapter,
};

use jengine_parallax::{ParallaxLayer, ParallaxPackage};

#[test]
fn lua_call_mutates_parallax_layer_via_global_registry() {
    // 1) Register parallax component into a registry, publish process-wide.
    let mut registry = ComponentRegistry::new();
    let pkg = ParallaxPackage;
    pkg.register_components(&mut registry);
    set_global_component_registry(Arc::new(registry));

    // 2) Build a hecs world with one entity carrying a default ParallaxLayer.
    let mut world = World::new();
    let entity = world.spawn((ParallaxLayer::default(),));
    let entity_id = entity.id() as u64;

    // 3) Build script API and install handlers into a Luau adapter.
    let mut api = ScriptApiRegistry::new();
    pkg.register_script_api(&mut api);

    let mut adapter = LuauAdapter::new(Capabilities::empty()).expect("luau adapter init");
    for desc in api.iter() {
        adapter.install_dynamic_function(&desc.name, desc);
    }

    // 4) Load a Luau script that calls our package function during `update`.
    let script = format!(
        "function update(e, dt) set_parallax_auto_scroll({entity_id}, 10.0, 5.0) end"
    );
    let handle = adapter
        .load("integration.luau", &script)
        .expect("script load");

    // 5) Run one frame: collect commands.
    let read = BridgeReadSnapshot::default();
    let mut commands: Vec<BridgeCommand> = Vec::new();
    {
        let mut ctx = BridgeContext::new(0.016, &read, Capabilities::empty(), &mut commands);
        adapter.frame_begin(&mut ctx);
        adapter
            .update(handle, entity_id, &mut ctx)
            .expect("update");
        adapter.frame_end(&mut ctx);
    }

    // Sanity: handler emitted two SetComponentField commands.
    assert_eq!(commands.len(), 2, "expected two field writes, got {commands:#?}");

    // 6) Apply commands through the full applier (translator + global registry).
    apply_bridge_commands(&mut world, commands, None, None);

    // 7) Assert the live component was mutated.
    let layer = world.get::<&ParallaxLayer>(entity).expect("parallax still attached");
    assert_eq!(layer.auto_scroll_x, 10.0, "auto_scroll_x not patched");
    assert_eq!(layer.auto_scroll_y, 5.0, "auto_scroll_y not patched");
    // Untouched fields preserved through the JSON roundtrip (kept default values):
    let default_layer = ParallaxLayer::default();
    assert_eq!(layer.scroll_factor_x, default_layer.scroll_factor_x);
    assert_eq!(layer.scroll_factor_y, default_layer.scroll_factor_y);
}
