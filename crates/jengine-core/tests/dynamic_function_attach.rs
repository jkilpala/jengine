//! Slice A — package-registered Rust components callable from Luau.
//!
//! End-to-end test that proves the live-handler path works:
//! 1. Build a `ScriptApiRegistry` with one `add_function_with_handler` entry
//!    that emits a `BridgeCommand::SetComponentField` for a typed component.
//! 2. Install it into a `LuauAdapter` via `install_dynamic_function`.
//! 3. Run a Luau script that calls the function.
//! 4. Assert the expected `BridgeCommand` was queued.
//!
//! This exercises the whole chain *except* the global ComponentRegistry
//! applier (covered separately by the runtime). Adapter-level test only.

#![cfg(feature = "native")]

use std::sync::Arc;

use jengine_core::package::script_api::{
    ScriptApiRegistry, ScriptArg, ScriptValueType,
};
use jengine_core::scripting::bridge::{
    BridgeCommand, BridgeContext, BridgeReadSnapshot, Capabilities, LuauAdapter, ScriptAdapter,
};
use jengine_core::scripting::bridge::value::Value;

#[test]
fn dynamic_function_emits_set_component_field() {
    let mut api = ScriptApiRegistry::new();
    api.add_function_with_handler(
        "set_my_field",
        "test-pkg",
        "Test setter",
        vec![
            ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
            ScriptArg { name: "value".into(), arg_type: ScriptValueType::Float },
        ],
        ScriptValueType::Nil,
        |ctx, args| {
            let entity_id = match args.first() {
                Some(Value::EntityId(id)) => *id,
                Some(Value::Number(n)) => *n as u64,
                _ => return Ok(Value::Nil),
            };
            let v = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0);
            ctx.commands.push(BridgeCommand::SetComponentField {
                entity_id,
                component_name: Arc::<str>::from("my_component"),
                field_name: Arc::<str>::from("my_field"),
                value: serde_json::json!(v),
            });
            Ok(Value::Nil)
        },
    );

    let mut adapter = LuauAdapter::new(Capabilities::empty()).expect("luau init");
    for desc in api.iter() {
        adapter.install_dynamic_function(&desc.name, desc);
    }

    let handle = adapter
        .load(
            "test.luau",
            "function update(e, dt) set_my_field(42, 1.5) end",
        )
        .expect("load");

    let read = BridgeReadSnapshot::default();
    let mut commands: Vec<BridgeCommand> = Vec::new();
    let mut ctx = BridgeContext::new(0.0, &read, Capabilities::empty(), &mut commands);

    adapter.frame_begin(&mut ctx);
    adapter.update(handle, 7, &mut ctx).expect("update");
    adapter.frame_end(&mut ctx);

    assert_eq!(commands.len(), 1, "expected exactly one queued command");
    match &commands[0] {
        BridgeCommand::SetComponentField {
            entity_id,
            component_name,
            field_name,
            value,
        } => {
            assert_eq!(*entity_id, 42);
            assert_eq!(component_name.as_ref(), "my_component");
            assert_eq!(field_name.as_ref(), "my_field");
            assert_eq!(value, &serde_json::json!(1.5));
        }
        other => panic!("unexpected command: {:?}", other),
    }
}
