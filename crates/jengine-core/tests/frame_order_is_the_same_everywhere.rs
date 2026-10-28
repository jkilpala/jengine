//! The three hosts tick a frame in one order (Oct 2026): the world first -
//! input, behaviour graphs, motion, the physics step, the trigger areas, the
//! graph barrier, combat and animation, UI navigation, lifetimes, visibility,
//! the UI widgets - then the scripts, then the camera. Native used to run its
//! scripts first and read the frame before; the editor put the graph barrier
//! after combat and the web before it; and both browsers polled pads after
//! the input actions that read them. `docs/parity-gaps.md`, "The order of a
//! frame".
//!
//! Read from each host's source, in sequence: every step must appear after
//! the one before it, so a step moved, or added out of place, fails here.


fn code_only(src: &str) -> String {
    // Ending in a newline, so a function that ends the file ends in "\n}\n" too.
    src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n") + "\n"
}

/// The steps, as each host calls them, in order. `None` where a host has no
/// such step in this function (the editor poses its world in its render).
fn steps(scripts: &'static str) -> Vec<&'static str> {
    vec![
        "poll_gamepads(",
        "update_input_actions(",
        "tick_pre_simulation(",
        "tick_motion(",
        "physics_frame_begin(",
        "physics_frame_end(",
        "update_trigger_areas(",
        "flush_commands_and_route_events(",
        "tick_combat_and_animation(",
        "navigate_ui(",
        "update_lifetimes(",
        "tick_visibility(",
        "update_ui_widgets(",
        scripts,
        "update_camera(",
    ]
}

/// `src` with the body of `fn <name>(` put where `call` first calls it, so
/// the steps a host has moved into a function of its own are read in the
/// order the frame runs them.
fn inline_fn(src: &str, name: &str, call: &str) -> String {
    let def = src.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("no `fn {name}(`"));
    let end = src[def..].find("\n}\n").unwrap_or_else(|| panic!("`fn {name}` has no end at column 0"));
    let body = src[def..def + end + 3].to_string();
    let at = src.find(call).unwrap_or_else(|| panic!("nothing calls `{name}` as `{call}`"));
    format!("{}{}\n{}", &src[..at], body, &src[at..])
}

#[test]
fn every_host_ticks_a_frame_in_one_order() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).unwrap();
    for (name, rel, from, scripts, pads) in [
        (
            "native",
            "crates/jengine-runtime/src/wgpu_scene_runner.rs",
            "fn render_frame(",
            "state.script_system.run(",
            "gamepad.poll(",
        ),
        ("web", "crates/jengine-web/src/lib.rs", "fn render_frame(", "run_scripts_frame(", "poll_gamepads("),
        (
            "editor",
            "crates/jengine-editor-viewport/src/lib.rs",
            "fn tick_play_world(",
            "scripts::run_frame(",
            "poll_gamepads(",
        ),
    ] {
        let mut src = code_only(&read(rel));
        // Every host runs its gameplay as `simulation_step` - once a frame, or
        // once per fixed step (`ecs::fixed_step`) - and the web's step calls
        // its `tick_world`: read each where it is called.
        let caller = if name == "native" { "simulation_step(state" } else { "simulation_step(app" };
        src = inline_fn(&src, "simulation_step", caller);
        if name == "web" {
            src = inline_fn(&src, "tick_world", "= tick_world(app");
        }
        let start = src.find(from).unwrap_or_else(|| panic!("{name}: no `{from}`"));
        let mut at = start;
        let mut list = steps(scripts);
        list[0] = pads;
        for step in list {
            match src[at..].find(step) {
                Some(i) => at += i + step.len(),
                None => {
                    let anywhere = src[start..].find(step).is_some();
                    panic!(
                        "{name}: `{step}` {} - every host ticks: {:?}",
                        if anywhere { "comes too early" } else { "is missing" },
                        steps(scripts)
                    );
                }
            }
        }
    }
}
