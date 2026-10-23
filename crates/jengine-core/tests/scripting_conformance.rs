//! Phase 7.1 — Scripting bridge conformance harness.
//!
//! Drives a curated catalogue of fixtures through every retained adapter using
//! **only the public bridge API** ([`jengine_core::scripting::bridge`]). For each
//! fixture, every adapter must:
//!
//! 1. Compile/load the source without error.
//! 2. Execute `init` (if defined) followed by `update`.
//! 3. Emit the exact same [`BridgeCommand`] stream as every other adapter.
//! 4. Match the fixture's `expected` stream byte-for-byte.
//!
//! The in-source `parity.rs` tests exercise the full Phase 5 surface; this
//! harness focuses on **contract conformance** — proof that the bridge can be
//! reached entirely through the public API surface, so any future adapter (or
//! out-of-tree adapter) can be conformance-tested without touching internals.
//!
//! Gated behind `luau-adapter + js-adapter-host` so it can run on native CI
//! against both retained adapters in one process.
#![cfg(all(feature = "native", feature = "js-adapter-host"))]

use jengine_core::scripting::bridge::{
    BridgeCommand, BridgeContext, BridgeReadSnapshot, Capabilities, JsAdapter, LuauAdapter,
    ScriptAdapter,
};

/// One conformance case: a Luau source string plus the snapshot, entity id, and
/// expected `BridgeCommand` stream that every adapter must produce.
struct Fixture {
    name: &'static str,
    src: &'static str,
    snapshot: BridgeReadSnapshot,
    entity_id: u64,
    expected: Vec<BridgeCommand>,
}

/// Run a fixture through one adapter and return the emitted command stream.
fn run_through<A: ScriptAdapter + NamedAdapter>(adapter: &mut A, fixture: &Fixture) -> Vec<BridgeCommand> {
    let handle = adapter
        .load(&format!("{}.luau", fixture.name), fixture.src)
        .unwrap_or_else(|e| panic!("[{}] {} load failed: {e:?}", A::backend_name(), fixture.name));
    let mut commands: Vec<BridgeCommand> = Vec::new();
    let mut ctx = BridgeContext::new(
        0.016,
        &fixture.snapshot,
        Capabilities::empty(),
        &mut commands,
    );
    adapter
        .init(handle, fixture.entity_id, &mut ctx)
        .unwrap_or_else(|e| panic!("[{}] {} init failed: {e:?}", A::backend_name(), fixture.name));
    adapter
        .update(handle, fixture.entity_id, &mut ctx)
        .unwrap_or_else(|e| panic!("[{}] {} update failed: {e:?}", A::backend_name(), fixture.name));
    commands
}

/// Trait extension so each adapter can name itself in panic messages.
trait NamedAdapter {
    fn backend_name() -> &'static str;
}
impl NamedAdapter for LuauAdapter {
    fn backend_name() -> &'static str {
        "luau"
    }
}
impl NamedAdapter for JsAdapter {
    fn backend_name() -> &'static str {
        "javascript"
    }
}

/// Run a single fixture through every retained adapter and assert conformance.
fn assert_conforms(fixture: &Fixture) {
    let mut luau = LuauAdapter::new(Capabilities::empty()).expect("luau adapter");
    let mut js = JsAdapter::new();

    let luau_cmds = run_through(&mut luau, fixture);
    let js_cmds = run_through(&mut js, fixture);

    assert_eq!(
        luau_cmds, js_cmds,
        "[{}] adapter divergence:\n  luau = {luau_cmds:#?}\n  js   = {js_cmds:#?}",
        fixture.name
    );
    assert_eq!(
        luau_cmds, fixture.expected,
        "[{}] command stream does not match fixture expectation",
        fixture.name
    );
}

// ---------------------------------------------------------------------------
// Fixture catalogue — covers one representative case per major API area.
// ---------------------------------------------------------------------------

fn fx_log_only() -> Fixture {
    Fixture {
        name: "log_only",
        src: r#"
            function update(entity, dt)
                log("hello " .. entity)
            end
        "#,
        snapshot: BridgeReadSnapshot::default(),
        entity_id: 7,
        expected: vec![BridgeCommand::Log {
            message: "hello 7".into(),
        }],
    }
}

fn fx_transform_write() -> Fixture {
    Fixture {
        name: "transform_write",
        src: r#"
            function update(entity, dt)
                set_position(entity, 1.5, 2.5)
                set_rotation(entity, 0.25)
                set_scale(entity, 2.0, 3.0)
            end
        "#,
        snapshot: BridgeReadSnapshot::default(),
        entity_id: 42,
        expected: vec![
            BridgeCommand::SetPosition {
                entity_id: 42,
                x: 1.5,
                y: 2.5,
            },
            BridgeCommand::SetRotation {
                entity_id: 42,
                radians: 0.25,
            },
            BridgeCommand::SetScale {
                entity_id: 42,
                x: 2.0,
                y: 3.0,
            },
        ],
    }
}

fn fx_read_then_write() -> Fixture {
    let mut snap = BridgeReadSnapshot::default();
    snap.positions.insert(3, (10.0, 20.0));
    Fixture {
        name: "read_then_write",
        src: r#"
            function update(entity, dt)
                local x, y = get_position(entity)
                set_position(entity, x + 1, y + 2)
            end
        "#,
        snapshot: snap,
        entity_id: 3,
        expected: vec![BridgeCommand::SetPosition {
            entity_id: 3,
            x: 11.0,
            y: 22.0,
        }],
    }
}

fn fx_init_then_update_ordering() -> Fixture {
    Fixture {
        name: "init_then_update_ordering",
        src: r#"
            function init(entity)
                set_position(entity, 0.0, 0.0)
                log("init " .. entity)
            end
            function update(entity, dt)
                set_position(entity, 1.0, 1.0)
                log("update " .. entity)
            end
        "#,
        snapshot: BridgeReadSnapshot::default(),
        entity_id: 5,
        expected: vec![
            BridgeCommand::SetPosition {
                entity_id: 5,
                x: 0.0,
                y: 0.0,
            },
            BridgeCommand::Log {
                message: "init 5".into(),
            },
            BridgeCommand::SetPosition {
                entity_id: 5,
                x: 1.0,
                y: 1.0,
            },
            BridgeCommand::Log {
                message: "update 5".into(),
            },
        ],
    }
}

fn fx_lighting_and_postprocess() -> Fixture {
    Fixture {
        name: "lighting_and_postprocess",
        src: r#"
            function update(entity, dt)
                set_light_color(entity, 1.0, 0.5, 0.25)
                set_light_intensity(entity, 2.5)
                set_ambient_light(0.1, 0.2, 0.3, 0.75)
                set_post_process_enabled("bloom", true)
                set_post_process_float("bloom", "threshold", 0.8)
            end
        "#,
        snapshot: BridgeReadSnapshot::default(),
        entity_id: 6,
        expected: vec![
            BridgeCommand::SetLightColor {
                entity_id: 6,
                r: 1.0,
                g: 0.5,
                b: 0.25,
            },
            BridgeCommand::SetLightIntensity {
                entity_id: 6,
                intensity: 2.5,
            },
            BridgeCommand::SetAmbientLight {
                r: 0.1,
                g: 0.2,
                b: 0.3,
                intensity: 0.75,
            },
            BridgeCommand::SetPostProcessEnabled {
                effect_name: "bloom".into(),
                enabled: true,
            },
            BridgeCommand::SetPostProcessFloat {
                effect_name: "bloom".into(),
                param_name: "threshold".into(),
                value: 0.8,
            },
        ],
    }
}

fn fx_debug_draws() -> Fixture {
    Fixture {
        name: "debug_draws",
        src: r#"
            function update(entity, dt)
                draw_debug_line(0.0, 0.0, 10.0, 20.0, 1.0, 0.0, 0.0)
                draw_debug_circle(100.0, 50.0, 16.0, 0.0, 0.0, 1.0)
                set_debug_overlay(true)
            end
        "#,
        snapshot: BridgeReadSnapshot::default(),
        entity_id: 1,
        expected: vec![
            BridgeCommand::DrawDebugLine {
                x1: 0.0,
                y1: 0.0,
                x2: 10.0,
                y2: 20.0,
                r: 1.0,
                g: 0.0,
                b: 0.0,
            },
            BridgeCommand::DrawDebugCircle {
                x: 100.0,
                y: 50.0,
                radius: 16.0,
                r: 0.0,
                g: 0.0,
                b: 1.0,
            },
            BridgeCommand::SetDebugOverlay { visible: true },
        ],
    }
}

fn fx_custom_component_round_trip() -> Fixture {
    let mut snap = BridgeReadSnapshot::default();
    let mut comps = std::collections::HashMap::new();
    comps.insert(
        "BunnyData".to_string(),
        serde_json::json!({ "velocity_x": 10.0, "velocity_y": 5.0 }),
    );
    snap.custom_components.insert(7, comps);
    Fixture {
        name: "custom_component_round_trip",
        src: r#"
            function update(entity, dt)
                local vx = get_custom(entity, "BunnyData", "velocity_x")
                set_custom(entity, "BunnyData", "velocity_x", vx + 1.0)
            end
        "#,
        snapshot: snap,
        entity_id: 7,
        expected: vec![BridgeCommand::SetCustomField {
            entity_id: 7,
            component_name: std::sync::Arc::from("BunnyData"),
            field_name: std::sync::Arc::from("velocity_x"),
            value: serde_json::Value::from(11.0_f64),
        }],
    }
}

// ---------------------------------------------------------------------------
// Tests — one per fixture so failures isolate cleanly.
// ---------------------------------------------------------------------------

#[test]
fn conformance_log_only() {
    assert_conforms(&fx_log_only());
}

#[test]
fn conformance_transform_write() {
    assert_conforms(&fx_transform_write());
}

#[test]
fn conformance_read_then_write() {
    assert_conforms(&fx_read_then_write());
}

#[test]
fn conformance_init_then_update_ordering() {
    assert_conforms(&fx_init_then_update_ordering());
}

#[test]
fn conformance_lighting_and_postprocess() {
    assert_conforms(&fx_lighting_and_postprocess());
}

#[test]
fn conformance_debug_draws() {
    assert_conforms(&fx_debug_draws());
}

#[test]
fn conformance_custom_component_round_trip() {
    assert_conforms(&fx_custom_component_round_trip());
}

fn fx_time_scale() -> Fixture {
    Fixture {
        name: "time_scale",
        src: r#"
            function update(entity, dt)
                set_time_scale(0.0)
                set_time_scale(1.0)
            end
        "#,
        snapshot: BridgeReadSnapshot::default(),
        entity_id: 5,
        expected: vec![
            BridgeCommand::SetTimeScale { scale: 0.0 },
            BridgeCommand::SetTimeScale { scale: 1.0 },
        ],
    }
}

/// Smoke test that catalogues every fixture in one shot. Useful for CI summary
/// output and ensures additions to the catalogue stay in sync with the
/// individual `#[test]` entry points above.
/// The pause menu depends on this reaching the host identically from both
/// dialects — a `set_time_scale` that exists on Luau but not in the editor
/// would mean the game pauses natively and does not pause where you develop.
#[test]
fn conformance_time_scale() {
    assert_conforms(&fx_time_scale());
}

#[test]
fn conformance_full_catalogue() {
    let fixtures = [
        fx_log_only(),
        fx_transform_write(),
        fx_read_then_write(),
        fx_init_then_update_ordering(),
        fx_lighting_and_postprocess(),
        fx_debug_draws(),
        fx_custom_component_round_trip(),
        fx_time_scale(),
    ];
    for fixture in &fixtures {
        assert_conforms(fixture);
    }
}
