//! **Every runtime joins the script and graph event buses, both ways.**
//!
//! `graph_and_script_events_meet` proves the native path end to end. The
//! editor viewport and the web host do the same thing from command arms and
//! frame code that only run in a browser, so no native test can call them -
//! and "wired in one runtime, forgotten in another" is the most common bug in
//! this repo, and a silent one: the event simply never arrives.
//!
//! So this reads the source. For each runtime it requires:
//!
//! * **scripts -> graphs** - the script's broadcast `send_event`, and an
//!   animation frame's event (native's bus carries both), handed to
//!   `deliver_script_event`;
//! * **graphs -> scripts** - the barrier's `emit_event_requests` handed to the
//!   scripts' listeners, as `script_payload`.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn count(hay: &str, needle: &str) -> usize {
    hay.matches(needle).count()
}

/// A browser host's source plus the command arms it shares
/// (`adapters::js::host_dispatch`, where the `evt` arm lives since Oct 2026) -
/// only if it calls them, so a host that dropped the call fails.
fn with_dispatch(src: String) -> String {
    if src.contains("handle_host_command(") {
        src + "
" + &read("crates/jengine-core/src/scripting/bridge/adapters/js/host_dispatch.rs")
    } else {
        src
    }
}

#[test]
fn native_runs_both_directions_around_the_barrier() {
    let runner = read("crates/jengine-runtime/src/wgpu_scene_runner.rs");
    assert!(
        runner.contains("take_events_for_graphs()") && runner.contains("deliver_script_event("),
        "the native runner no longer hands scripts' events to graphs"
    );
    assert!(
        runner.contains("queue_graph_events(&routed.emit_event_requests)"),
        "the native runner no longer hands graphs' events to scripts"
    );
    // Before the barrier, or a handler's commands wait a frame for nothing.
    let deliver = runner.find("deliver_script_event(").unwrap();
    let barrier = runner.find("flush_commands_and_route_events(").unwrap();
    assert!(deliver < barrier, "scripts' events should reach graphs before the barrier");
}

#[test]
fn the_editor_viewport_runs_both_directions() {
    let vp = with_dispatch(read("crates/jengine-editor-viewport/src/lib.rs"));
    assert!(
        count(&vp, "deliver_script_event(") >= 2,
        "the viewport should hand graphs both a script's `send_event` (the `evt` arm) and an \
         animation frame's event - native's bus carries both"
    );
    assert!(
        vp.contains("routed.emit_event_requests") && vp.contains("script_payload()"),
        "the viewport no longer hands graphs' events to its scripts"
    );
}

#[test]
fn the_web_host_runs_both_directions() {
    let host = with_dispatch(read("crates/jengine-web/src/script_host.rs"));
    let lib = read("crates/jengine-web/src/lib.rs");
    assert!(
        count(&host, "deliver_script_event(") >= 2,
        "the web host should hand graphs both a script's `send_event` and an animation \
         frame's event"
    );
    assert!(
        host.contains("script_payload()"),
        "the web host's graph-event dispatch no longer builds the shared payload"
    );
    assert!(
        lib.contains("dispatch_graph_events(app, &routed.emit_event_requests)"),
        "the web frame no longer hands graphs' events to its scripts"
    );
}

/// The one rule that must not be copied: what a graph hears for a script's
/// data. Each runtime calls the core helper; none converts it itself.
#[test]
fn no_runtime_converts_script_data_for_graphs_itself() {
    for rel in [
        "crates/jengine-runtime/src/wgpu_scene_runner.rs",
        "crates/jengine-editor-viewport/src/lib.rs",
        "crates/jengine-web/src/script_host.rs",
        "crates/jengine-web/src/lib.rs",
    ] {
        let src = read(rel);
        assert!(
            !calls_graph_dispatch(&src),
            "{rel} calls `BehaviorGraphSystem::dispatch_event` directly - go through \
             `deliver_script_event`, which is where a script's data becomes a graph's number"
        );
    }
}

/// Whether `src` calls `BehaviorGraphSystem::dispatch_event(name, value,
/// world)` - told apart by its three arguments from the browser's own
/// one-argument `EventTarget::dispatch_event(&event)`, which the web host
/// uses to announce its first frame to the page.
fn calls_graph_dispatch(src: &str) -> bool {
    src.match_indices(".dispatch_event(").any(|(at, needle)| {
        let mut depth = 0usize;
        let mut commas = 0usize;
        for c in src[at + needle.len()..].chars() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' if depth == 0 => break,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => commas += 1,
                _ => {}
            }
        }
        commas >= 2
    })
}

#[test]
fn the_dispatch_rule_tells_graphs_from_the_dom() {
    assert!(calls_graph_dispatch("graphs.dispatch_event(&name, v, &mut world);"));
    assert!(!calls_graph_dispatch("let _ = w.dispatch_event(&event);"));
}
