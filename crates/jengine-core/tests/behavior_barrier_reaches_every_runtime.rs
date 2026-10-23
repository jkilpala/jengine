//! Every runtime must drive behavior graphs through the same core calls.
//!
//! Each of these was, at some point, a graph feature that compiled, passed its
//! unit tests, and did nothing in a real runtime - because the core function
//! existed and no runtime called it, or called it and dropped what it
//! returned:
//!
//! * `EmitEvent` -> `OnEvent`: routed by native only; the editor and web
//!   called the bare `flush_commands`, which drains the events and drops them
//!   (BUGS #92).
//! * `OnKey` / `OnMouseClick`: `dispatch_input_keys` and `dispatch_mouse_click`
//!   had no caller anywhere (BUGS #93).
//! * `SpawnEntity`: spawned a bare `Name + Transform`, because no runtime lent
//!   the barrier its prefabs (BUGS #93).
//! * `PlaySound` / `ChangeScene`: returned "for the host to forward", and no
//!   host read them (BUGS #93).
//! * `OnDespawn`: the browser hosts bound graphs but never pruned them, so a
//!   despawned entity kept its instance forever (BUGS #93).
//!
//! Test-lab scene 68 and `scene_68_graph_commands` check the behavior; this
//! checks the call sites, which is the part that kept going missing.

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");

fn compact(src: &str) -> String {
    src.split_whitespace().collect()
}

fn runtimes() -> [(&'static str, String); 3] {
    [("native", compact(NATIVE)), ("viewport", compact(VIEWPORT)), ("web", compact(WEB))]
}

#[test]
fn every_runtime_routes_emitted_behavior_events() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("behavior_system.flush_commands_and_route_events("),
            "{name} does not end its behavior frame with \
             `flush_commands_and_route_events`, so `EmitEvent` never reaches `OnEvent` there"
        );
        assert!(
            !src.contains("behavior_system.flush_commands("),
            "{name} calls the bare `flush_commands`, which drains `EmitEvent` requests \
             and drops them. Use `flush_commands_and_route_events`."
        );
    }
}

#[test]
fn every_runtime_lends_the_barrier_its_prefabs() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("Some(&jengine_core::behavior::PrefabSource{"),
            "{name} does not pass its prefabs to the behavior barrier, so a graph's \
             `SpawnEntity` makes a bare entity instead of the prefab"
        );
    }
}

#[test]
fn every_runtime_forwards_graph_sounds_and_scene_changes() {
    for (name, src) in runtimes() {
        for field in ["play_sound_requests", "change_scene_requests"] {
            assert!(
                src.contains(&format!("routed.{field}")),
                "{name} never reads `{field}` from the behavior barrier, so a graph's \
                 request is dropped there"
            );
        }
    }
}

#[test]
fn every_runtime_feeds_graphs_their_input() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("behavior_system.dispatch_input(&"),
            "{name} never calls `dispatch_input`, so `OnKey` and `OnMouseClick` never fire there"
        );
    }
}

#[test]
fn every_runtime_prunes_instances_of_despawned_entities() {
    for (name, src, call) in [
        ("native", compact(NATIVE), "behavior_system.sync_instances(&mut"),
        ("viewport", compact(VIEWPORT), "behavior_system.sync_instances_inline(&mut"),
        ("web", compact(WEB), "behavior_system.sync_instances_inline(&mut"),
    ] {
        assert!(
            src.contains(call),
            "{name} does not sync its graph instances each frame, so a despawned entity \
             keeps its graph and `OnDespawn` never fires there"
        );
    }
}
