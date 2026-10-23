//! Slice A.JS — package-registered Rust components callable from a
//! `JsAdapter`-hosted script (host QuickJS path).
//!
//! End-to-end test mirroring `dynamic_function_attach.rs`:
//! 1. Build a `ScriptApiRegistry` with one `add_function_with_handler` entry
//!    that emits a `BridgeCommand::SetComponentField`.
//! 2. Install it into a `JsAdapter` via `install_dynamic_function` *before*
//!    `load` (exercises the `pending_dynamic_fns` drain path).
//! 3. Run a Luau-syntax script that calls the function. `JsAdapter::load`
//!    transpiles Luau → JS and forwards to the host QuickJS engine because
//!    `js-adapter-host` is enabled in this test build.
//! 4. Assert the expected `BridgeCommand` was queued.
//!
//! This is the JS-host counterpart to the Lua test and locks in Slice A.JS
//! of `docs/archive/user-rust-components-design.md`. The browser-side handler path
//! is documented as a no-op pending Phase 4.6.

#![cfg(all(feature = "native", feature = "js-adapter-host"))]

use std::sync::Arc;

use jengine_core::package::script_api::{ScriptApiRegistry, ScriptArg, ScriptValueType};
use jengine_core::scripting::bridge::value::Value;
use jengine_core::scripting::bridge::{
    BridgeCommand, BridgeContext, BridgeReadSnapshot, Capabilities, JsAdapter, ScriptAdapter,
};

#[test]
fn js_dynamic_function_emits_set_component_field() {
    let mut api = ScriptApiRegistry::new();
    api.add_function_with_handler(
        "set_my_field",
        "test-pkg",
        "Test setter",
        vec![
            ScriptArg {
                name: "entity_id".into(),
                arg_type: ScriptValueType::EntityId,
            },
            ScriptArg {
                name: "value".into(),
                arg_type: ScriptValueType::Float,
            },
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

    let mut adapter = JsAdapter::new();
    // Install BEFORE load — exercises the pending_dynamic_fns drain path
    // (host QuickJS engine is constructed lazily inside the first `load`).
    for desc in api.iter() {
        adapter.install_dynamic_function(&desc.name, desc);
    }

    let handle = adapter
        .load(
            "test.luau",
            "function update(entity, dt) set_my_field(42, 1.5) end",
        )
        .expect("load");

    let read = BridgeReadSnapshot::default();
    let mut commands: Vec<BridgeCommand> = Vec::new();
    let mut ctx = BridgeContext::new(0.0, &read, Capabilities::empty(), &mut commands);

    adapter.frame_begin(&mut ctx);
    adapter.update(handle, 7, &mut ctx).expect("update");
    adapter.frame_end(&mut ctx);

    assert_eq!(commands.len(), 1, "expected exactly one queued command, got: {commands:?}");
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

#[test]
fn js_dynamic_function_install_after_load_also_works() {
    // Variant: install AFTER load (host engine already exists; direct path).
    let mut adapter = JsAdapter::new();
    let handle = adapter
        .load(
            "test.luau",
            "function update(entity, dt) set_my_field(99, 2.25) end",
        )
        .expect("load");

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
            let entity_id = args
                .first()
                .and_then(|v| match v {
                    Value::EntityId(id) => Some(*id),
                    Value::Number(n) => Some(*n as u64),
                    _ => None,
                })
                .unwrap_or(0);
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
    for desc in api.iter() {
        adapter.install_dynamic_function(&desc.name, desc);
    }

    let read = BridgeReadSnapshot::default();
    let mut commands: Vec<BridgeCommand> = Vec::new();
    let mut ctx = BridgeContext::new(0.0, &read, Capabilities::empty(), &mut commands);

    adapter.frame_begin(&mut ctx);
    adapter.update(handle, 7, &mut ctx).expect("update");
    adapter.frame_end(&mut ctx);

    assert_eq!(commands.len(), 1, "expected exactly one queued command, got: {commands:?}");
    match &commands[0] {
        BridgeCommand::SetComponentField {
            entity_id,
            value,
            ..
        } => {
            assert_eq!(*entity_id, 99);
            assert_eq!(value, &serde_json::json!(2.25));
        }
        other => panic!("unexpected command: {:?}", other),
    }
}
