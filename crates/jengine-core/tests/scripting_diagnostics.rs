//! Phase 7.2 — Per-adapter diagnostics fixtures.
//!
//! Verifies that every retained adapter classifies failures into the canonical
//! [`ScriptErrorKind`] variants and tags them with the correct
//! [`BackendKind`]. The harness drives each scenario through every adapter so
//! editor tooling can rely on a single shape regardless of which backend the
//! script is running under.
//!
//! ## Coverage matrix
//!
//! | Kind         | Luau | JS (host) | Notes |
//! |--------------|------|-----------|-------|
//! | `Syntax`     | ✅   | ✅        | unparseable source |
//! | `Runtime`    | ✅   | ✅        | `error()` / `throw` from a lifecycle |
//! | `BridgeCall` | ⏳   | ⏳        | reserved variant; no adapter emits it yet — see ignored test |
//! | `Capability` | ⏳   | ⏳        | reserved variant; no adapter emits it yet — see ignored test |
//!
//! `BridgeCall` and `Capability` are part of the contract surface
//! (architecture plan §6) but no current bridge call validates arguments
//! strictly enough to produce them, and no script API checks
//! [`Capabilities`](jengine_core::scripting::bridge::Capabilities) before
//! firing. Tests for those variants are present and `#[ignore]`d so they show
//! up in the suite once an adapter starts emitting them.
#![cfg(all(feature = "native", feature = "js-adapter-host"))]

use jengine_core::scripting::bridge::{
    BackendKind, BridgeContext, BridgeReadSnapshot, Capabilities, JsAdapter, LuauAdapter,
    ScriptAdapter, ScriptError, ScriptErrorKind,
};

/// One diagnostic case: the source that should fail and the expected error
/// classification.
struct Case {
    name: &'static str,
    src: &'static str,
    /// Where the failure should surface — load (compile time) or update (runtime).
    stage: Stage,
    expected_kind: ScriptErrorKind,
}

#[derive(Clone, Copy)]
enum Stage {
    Load,
    Update,
}

/// Drive `case` through `adapter` and return whatever `ScriptError` was
/// produced. Panics if the case unexpectedly succeeds.
fn drive<A: ScriptAdapter>(adapter: &mut A, case: &Case, adapter_label: &str) -> ScriptError {
    match case.stage {
        Stage::Load => adapter
            .load(&format!("{}.luau", case.name), case.src)
            .err()
            .unwrap_or_else(|| {
                panic!(
                    "[{adapter_label}] {} expected load failure but load succeeded",
                    case.name
                )
            }),
        Stage::Update => {
            let handle = adapter
                .load(&format!("{}.luau", case.name), case.src)
                .unwrap_or_else(|e| {
                    panic!("[{adapter_label}] {} unexpected load failure: {e:?}", case.name)
                });
            let snap = BridgeReadSnapshot::default();
            let mut commands = Vec::new();
            let mut ctx = BridgeContext::new(0.016, &snap, Capabilities::empty(), &mut commands);
            // Some adapters surface the failure on init, others on update — try
            // init first then update; whichever returns Err wins.
            if let Err(e) = adapter.init(handle, 1, &mut ctx) {
                return e;
            }
            adapter
                .update(handle, 1, &mut ctx)
                .err()
                .unwrap_or_else(|| {
                    panic!(
                        "[{adapter_label}] {} expected runtime failure but update succeeded",
                        case.name
                    )
                })
        }
    }
}

/// Run a case through every retained adapter and assert each one classifies
/// the failure consistently.
fn assert_diagnoses(case: &Case) {
    // Luau ----------------------------------------------------------------
    {
        let mut luau = LuauAdapter::new(Capabilities::empty()).expect("luau adapter");
        let err = drive(&mut luau, case, "luau");
        assert_eq!(
            err.backend,
            BackendKind::Luau,
            "[luau] {} backend tag mismatch: {err}",
            case.name
        );
        assert_eq!(
            err.kind, case.expected_kind,
            "[luau] {} kind mismatch: {err}",
            case.name
        );
        assert!(
            !err.message.is_empty(),
            "[luau] {} message must not be empty",
            case.name
        );
    }

    // JavaScript (host QuickJS) ------------------------------------------
    {
        let mut js = JsAdapter::new();
        let err = drive(&mut js, case, "js");
        assert_eq!(
            err.backend,
            BackendKind::JavaScript,
            "[js] {} backend tag mismatch: {err}",
            case.name
        );
        assert_eq!(
            err.kind, case.expected_kind,
            "[js] {} kind mismatch: {err}",
            case.name
        );
        assert!(
            !err.message.is_empty(),
            "[js] {} message must not be empty",
            case.name
        );
    }
}

// ---------------------------------------------------------------------------
// Syntax — unparseable source.
// ---------------------------------------------------------------------------

#[test]
fn diagnostic_syntax_unparseable_function() {
    assert_diagnoses(&Case {
        name: "syntax_unparseable_function",
        // Missing `end` — both Luau and the transpiled-to-JS path must reject.
        src: "function update(entity, dt)\n    log(\"hi\")\n",
        stage: Stage::Load,
        expected_kind: ScriptErrorKind::Syntax,
    });
}

// ---------------------------------------------------------------------------
// Runtime — script raises an error from a lifecycle hook.
// ---------------------------------------------------------------------------

#[test]
fn diagnostic_runtime_explicit_error_in_update() {
    assert_diagnoses(&Case {
        name: "runtime_explicit_error",
        src: r#"
            function update(entity, dt)
                error("boom")
            end
        "#,
        stage: Stage::Update,
        expected_kind: ScriptErrorKind::Runtime,
    });
}

#[test]
fn diagnostic_runtime_nil_index_in_update() {
    assert_diagnoses(&Case {
        name: "runtime_nil_index",
        src: r#"
            function update(entity, dt)
                local t = nil
                local _ = t.x
            end
        "#,
        stage: Stage::Update,
        expected_kind: ScriptErrorKind::Runtime,
    });
}

// ---------------------------------------------------------------------------
// BridgeCall + Capability — reserved kinds, no adapter emits them yet.
// ---------------------------------------------------------------------------
//
// These are kept as `#[ignore]`d placeholders so the contract intent stays
// visible in the suite. When an adapter starts validating bridge arguments
// strictly (e.g. `set_position` on a non-existent entity) or when scripts
// gain a host-side capability gate, lift the `#[ignore]` and pick a
// fixture that triggers the new behaviour.

#[test]
#[ignore = "no adapter currently emits ScriptErrorKind::BridgeCall — re-enable once one does"]
fn diagnostic_bridge_call_reserved() {
    // Placeholder: when implemented, drive a case whose source calls a
    // bridge function with an argument shape the adapter rejects, and
    // assert `expected_kind: ScriptErrorKind::BridgeCall`.
}

#[test]
#[ignore = "no adapter currently emits ScriptErrorKind::Capability — re-enable once capability gating ships"]
fn diagnostic_capability_reserved() {
    // Placeholder: when implemented, build a script that calls an API
    // gated by a [`Capability`] flag, run it with that flag absent from
    // the [`Capabilities`] set, and assert `expected_kind:
    // ScriptErrorKind::Capability`.
}
