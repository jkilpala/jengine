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
    src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
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

/// The web's source with `tick_world`'s body put where `render_frame` calls it.
fn inline_tick_world(src: &str) -> String {
    let def = src.find("fn tick_world(").expect("web's tick_world");
    let body = &src[def..def + src[def..].find("\n}\n").expect("its end") + 3];
    let call = src.find("= tick_world(app").expect("render_frame calls tick_world");
    format!("{}{}\n{}", &src[..call], body, &src[call..])
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
        // The web's frame calls its `tick_world` (defined above it) part way
        // through: read the tick where it is called.
        if name == "web" {
            src = inline_tick_world(&src);
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
