//! Script timers fire, and named GUI callbacks resolve, in every runtime.
//!
//! In the editor viewport and web builds, `set_timer` / `set_interval` are
//! preamble functions that push onto `__jengine_play.timers` - and until
//! Sep 2026 no host ever read that list back. Every timer and interval there
//! was dead: a snake game and a falling-block puzzle never reached their
//! game-over screens, and test-lab scene 48 (timers) could never have passed. Native
//! ticks its own list in `ScriptServices`, so only the browser runtimes
//! broke, and nothing said so - the call returned an id like it worked.
//!
//! Found alongside it: both browser hosts resolved a *named* GUI callback
//! (`gui_on_click(e, gui, node, "on_resume")`) by reading a property off the
//! script's `lookup`, which is a function - so only `on_gui_click` ever ran.
//!
//! The rule itself (`scripting::timers::advance`) is unit-tested in core;
//! this checks both hosts actually call it and dispatch what fires.

const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/script_host.rs");

#[test]
fn both_browser_hosts_fire_script_timers() {
    // Both hosts' script phase is `adapters::js::scripts::run_frame` since
    // Oct 2026: the timers are advanced there, and a host must call it.
    let shared = include_str!("../src/scripting/bridge/adapters/js/scripts.rs");
    let at = shared.find("timers::advance_play_timers(").expect("run_frame advances the timers");
    assert!(shared[at..at + 300].contains("call_handler("), "run_frame advances the timers but does not call what fired");
    for (name, src) in [("viewport", VIEWPORT), ("web", WEB)] {
        if src.contains("scripts::run_frame(") {
            continue;
        }
        assert!(
            src.contains("timers::advance_play_timers("),
            "{name} never advances `__jengine_play.timers`, so set_timer / set_interval never fire there"
        );
        let at = src.find("timers::advance_play_timers(").unwrap();
        let after = &src[at..at + 300.min(src.len() - at)];
        assert!(
            after.contains("call_lifecycle_handler("),
            "{name} advances the timers but does not call what fired"
        );
    }
}

#[test]
fn both_browser_hosts_resolve_named_gui_callbacks_through_lookup() {
    // One implementation since Oct 2026, `scripts::call_gui_click`: a name
    // other than `on_gui_click` goes through `handler`, which calls `lookup`.
    let shared = include_str!("../src/scripting/bridge/adapters/js/scripts.rs");
    let at = shared.find("pub fn call_gui_click(").expect("scripts::call_gui_click");
    let body = &shared[at..at + shared[at..].find("
}
").unwrap()];
    assert!(body.contains("handler(inst, cb)"), "call_gui_click resolves a named callback without `lookup(name)`");
    let lookup = &shared[shared.find("pub fn handler(").unwrap()..];
    assert!(lookup[..600].contains(".call1(&JsValue::NULL, &JsValue::from_str(name))"), "handler does not call lookup(name)");
    for (name, src) in [("viewport", VIEWPORT), ("web", WEB)] {
        if src.contains("scripts::call_gui_click(") {
            continue;
        }
        let start = src
            .find("fn dispatch_gui_click")
            .unwrap_or_else(|| panic!("{name}: no GUI click dispatch found"));
        let body = &src[start..start + 2500.min(src.len() - start)];
        assert!(
            body.contains("inst.fns.lookup.is_function()") && body.contains(".call1(&JsValue::NULL, &JsValue::from_str(&cb_name))"),
            "{name} resolves a named GUI callback without calling `lookup(name)`, so only `on_gui_click` can fire"
        );
    }
}
