//! An event listener's handler can be named either way, in every runtime.
//!
//! There were two conventions, and each runtime knew exactly one of them.
//!
//! - **`on_<event_name>`** is what `docs/scripting-api.md` documents and what
//!   native has always called: `send_event("score_up")` invokes
//!   `on_score_up(entity, data)`.
//! - **`on_event(self, name, sender, data)`** is what the editor viewport and
//!   web have always called - a single handler that switches on the name.
//!
//! Neither knew about the other, so a handler written from the docs fired on
//! native and **silently did nothing** in the editor - test-lab scene 48, the
//! scene whose whole subject is timers and events, had been dead there for its
//! entire life - while three games written the other way did nothing on
//! native.
//!
//! It is a textbook grade-2 gap: `on_event` the *registration* binds in all
//! four runtimes and works, so every name-level ratchet counted it as present.
//! What differed was the name of the function it eventually called, which
//! nothing checks and nothing logs - a handler that is not there is simply not
//! called. Found by running test-lab scene 96 in the editor, where a scene
//! that hears exactly one ping per copy on native heard **zero**. BUGS #150.
//!
//! Both spellings now work everywhere: the specific name is tried first, so
//! nothing that worked before changes, and the general one is the fallback.

#![cfg(feature = "native")]

use jengine_core::scripting::bridge::{
    BridgeReadSnapshot, Capabilities, LuauAdapter, ScriptBridgeRunner,
};

/// A runner with one Luau script loaded on entity 1, through the same
/// `run_frame` path the engine uses.
fn runner_with(source: &str) -> (ScriptBridgeRunner, u64) {
    let adapter = LuauAdapter::new(Capabilities::empty()).expect("a Luau adapter");
    let mut runner = ScriptBridgeRunner::new(Box::new(adapter), Capabilities::empty());
    let snap = BridgeReadSnapshot::default();
    let entity = 1u64;
    let _ = runner.run_frame(&[(entity, "test.luau".to_string(), source.to_string())], &snap, 0.016, &[]);
    (runner, entity)
}

/// The probe both tests use: the handler records that it ran by calling
/// `set_custom`, which reaches us as a bridge command we can inspect. A `log`
/// would not do - logging is not deferred, so it proves the handler *started*
/// rather than that the engine believed it.
const MARK: &str = r#"set_custom(entity, "Probe", "heard", 1)"#;

#[test]
fn the_documented_on_name_spelling_is_called() {
    let (mut runner, e) = runner_with(&format!(
        r#"
        function on_score_up(entity, data)
            {MARK}
        end
        "#
    ));
    let snap = BridgeReadSnapshot::default();
    let (cmds, err) = runner.dispatch_event(e, "score_up", 0, &snap, 0.016);
    assert!(err.is_none(), "{err:?}");
    assert!(!cmds.is_empty(), "`on_score_up` should have run and emitted its command");
}

/// The other spelling, which is what the browser hosts have always called and
/// what three projects in this repo are written against.
#[test]
fn the_general_on_event_spelling_is_called_too() {
    let (mut runner, e) = runner_with(&format!(
        r#"
        function on_event(entity, name, sender, data)
            if name == "score_up" then
                {MARK}
            end
        end
        "#
    ));
    let snap = BridgeReadSnapshot::default();
    let (cmds, err) = runner.dispatch_event(e, "score_up", 0, &snap, 0.016);
    assert!(err.is_none(), "{err:?}");
    assert!(
        !cmds.is_empty(),
        "`on_event` should have run: it is what the editor and web have always called, and \
         three projects here are written that way"
    );
}

/// The specific name wins, so nothing that works today changes behaviour. A
/// script with both must not have its `on_<name>` handler shadowed by a
/// catch-all it also happens to define.
#[test]
fn a_script_with_both_gets_the_specific_one() {
    let (mut runner, e) = runner_with(
        r#"
        function on_score_up(entity, data)
            set_custom(entity, "Probe", "which", "specific")
        end
        function on_event(entity, name, sender, data)
            set_custom(entity, "Probe", "which", "general")
        end
        "#,
    );
    let snap = BridgeReadSnapshot::default();
    let (cmds, err) = runner.dispatch_event(e, "score_up", 0, &snap, 0.016);
    assert!(err.is_none(), "{err:?}");
    let got = format!("{cmds:?}");
    assert!(got.contains("specific"), "the specific handler should win: {got}");
    assert!(!got.contains("general"), "and the catch-all must not also run: {got}");
}

/// The sender reaches the general handler, because that is its second
/// argument in every browser host and a run of nils would be a quiet
/// difference rather than a loud one.
#[test]
fn the_general_handler_is_given_the_sender() {
    let (mut runner, e) = runner_with(
        r#"
        function on_event(entity, name, sender, data)
            set_custom(entity, "Probe", "sender", sender)
        end
        "#,
    );
    let snap = BridgeReadSnapshot::default();
    let (cmds, err) = runner.dispatch_event(e, "ping", 42, &snap, 0.016);
    assert!(err.is_none(), "{err:?}");
    let got = format!("{cmds:?}");
    assert!(got.contains("42"), "the sender should arrive as 42: {got}");
}

/// A script with neither handler is not an error - it simply is not listening,
/// which is the ordinary case for most entities.
#[test]
fn a_script_with_no_handler_at_all_is_silent_rather_than_an_error() {
    let (mut runner, e) = runner_with("function update(entity, dt) end");
    let snap = BridgeReadSnapshot::default();
    let (cmds, err) = runner.dispatch_event(e, "score_up", 0, &snap, 0.016);
    assert!(err.is_none(), "a missing handler is not a failure: {err:?}");
    assert!(cmds.is_empty());
}
