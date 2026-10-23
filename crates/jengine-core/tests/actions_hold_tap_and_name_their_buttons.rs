//! **An action can be held, tapped and double-tapped, and a prompt names the
//! button the player is holding** (Oct 2026). `Input.is_action_held(a, s)` is
//! true on the frame a press reaches `s`, `is_action_tapped` on a release
//! before it, `is_action_double_tapped` on the second of two quick presses;
//! `Input.get_gamepad_type(pad)` says which family a pad is and
//! `Input.get_prompt(action)` what its binding is called on the device used
//! last - A on an Xbox pad, Cross on a PlayStation one, B on a Switch.
//! Before this a script kept its own timers beside every action, and no
//! runtime knew what a pad was called. Found by the showcase's Button
//! Prompts.
//!
//! `input::interactions` and `input::prompts` test the rules; this drives the
//! script surface natively and checks every host keeps the timing and every
//! backend names its pads.

use jengine_core::input::{InputAction, InputState, KeyCode};

fn read(p: &str) -> String {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(p)).unwrap().replace("\r\n", "\n")
}

#[cfg(feature = "native")]
#[test]
fn a_script_reads_holds_taps_double_taps_and_prompts() {
    use hecs::World;
    use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
    use jengine_core::ecs::systems::ScriptSystem;
    const SRC: &str = r##"
local log = {}
function update(entity, dt)
    local f = #log + 1
    local p = Input.get_prompt("jump")
    log[f] = string.format("%s%s%s%s|%s|%s|%s|%s",
        Input.is_action_held("jump", 0.5) and "H" or "",
        Input.is_action_held("jump", 1.0) and "L" or "",
        Input.is_action_tapped("jump") and "T" or "",
        Input.is_action_double_tapped("jump") and "D" or "",
        Input.get_last_device(), p.label, p.glyph, Input.get_gamepad_type(0))
    Entity.set_custom(entity, "Probe", "log", table.concat(log, " "))
    Entity.set_custom(entity, "Probe", "hold", Input.get_hold_time("jump"))
    Entity.set_custom(entity, "Probe", "switch", Input.get_prompt("jump", "switch").label)
    Entity.set_custom(entity, "Probe", "pad", Input.get_gamepad_name(0) .. "#" .. Input.get_last_gamepad())
end
"##;
    let dir = std::env::temp_dir().join(format!("jengine_prompts_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prompts.lua");
    std::fs::write(&path, SRC).unwrap();
    let mut world = World::new();
    let e = world.spawn((
        Name("Probe".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let mut input = InputState::new();
    input.load_actions(&[InputAction { name: "jump".into(), keys: vec!["Space".into(), "Pad0_A".into()], dead_zone: 0.0 }]);
    input.update_gamepad(0, Default::default(), vec![0.0; 4]);
    input.set_gamepad_info(0, "Wireless Controller", Some(0x054c));
    // A frame at `t`: events, the hosts' timing step, then scripts.
    let mut frame = |t: f64, events: &dyn Fn(&mut InputState)| {
        input.begin_frame();
        events(&mut input);
        input.update_interactions_at(t);
        system.run(&mut world, 0.1, &input, None, None);
    };
    frame(0.0, &|i| i.key_down(KeyCode::Space)); // 1: press
    for k in 1..=10 {
        frame(k as f64 * 0.1, &|_| {}); // 2..11: held to 1.0 s
    }
    frame(1.1, &|i| i.key_up(KeyCode::Space)); // 12: a long press ends - no tap
    frame(2.0, &|i| i.key_down(KeyCode::Space)); // 13
    frame(2.1, &|i| i.key_up(KeyCode::Space)); // 14: a tap
    frame(2.2, &|i| i.key_down(KeyCode::Space)); // 15: a double tap
    frame(2.3, &|i| {
        i.key_up(KeyCode::Space);
        i.update_gamepad(0, [0usize].into_iter().collect(), vec![0.0; 4]); // 16: the pad's south button
    });
    let get = |k: &str| world.get::<&CustomComponents>(e).unwrap().0.get("Probe").and_then(|p| p.get(k)).cloned().unwrap();
    let log = get("log");
    let log: Vec<&str> = log.as_str().unwrap().split(' ').collect();
    let key = "keyboard|Space|keyboard/space|playstation";
    assert_eq!(log[0], format!("|{key}"), "a press is not a hold");
    assert_eq!(log[5], format!("H|{key}"), "0.5 s reached on frame 6");
    assert_eq!(log[10], format!("L|{key}"), "1.0 s reached on frame 11");
    assert_eq!(log[11], format!("|{key}"), "a long press is not a tap when it ends");
    assert_eq!(log[13], format!("T|{key}"));
    assert_eq!(log[14], format!("D|{key}"));
    // The key comes up as the pad's south button goes down: the action stays
    // held (no tap), and the pad, now in use, is what the prompt names.
    assert_eq!(log[15], "|gamepad|Cross|playstation/cross|playstation");
    assert_eq!(get("switch"), serde_json::json!("B"), "a Switch pad's south button says B");
    assert_eq!(get("pad"), serde_json::json!("Wireless Controller#0"));
}

#[test]
fn every_host_keeps_the_timing_and_every_backend_names_its_pads() {
    // The timing step after every device has reported, before scripts.
    let runner = read("crates/jengine-runtime/src/wgpu_scene_runner.rs");
    let web = read("crates/jengine-web/src/lib.rs");
    let vp = read("crates/jengine-editor-viewport/src/lib.rs");
    for (host, src, poll, run) in [
        ("native", &runner, "crate::gamepad_android::poll(&mut state.input);", "state.script_system.run("),
        ("web", &web, "gamepad_web::poll_gamepads(&mut app.input);", "script_host::run_scripts_frame("),
        // The editor publishes input to scripts through the shared mirror
        // (`adapters::js::mirror`, asserted to call `input_mirror::publish`
        // below), so its call of the mirror is what has to come after.
        ("editor", &vp, "gamepad_web::poll_gamepads(&mut app.input);", "sync_world_to_play_namespace(app);"),
    ] {
        let at = src.find(".update_interactions();").unwrap_or_else(|| panic!("{host} never updates the action timing"));
        assert!(src.find(poll).is_some_and(|p| p < at), "{host} updates the timing before its pads are polled");
        assert!(src.rfind(run).is_some_and(|r| r > at), "{host} updates the timing after scripts read it");
    }
    assert!(
        read("crates/jengine-core/src/scripting/bridge/adapters/js/mirror.rs").contains("input_mirror::publish("),
        "the shared mirror no longer publishes the input state"
    );
    // Every backend names its pads.
    assert!(read("crates/jengine-core/src/input/gamepad_web.rs").contains("set_gamepad_info("));
    assert!(read("crates/jengine-runtime/src/gamepad_native.rs").contains("set_gamepad_info(index, pad.name(), pad.vendor_id())"));
    assert!(read("crates/jengine-runtime/src/gamepad_android.rs").contains("input_device_identity("));
    // The script surface: one hook in the browsers, the view natively.
    let preamble = read("crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs");
    let hook = read("crates/jengine-core/src/scripting/bridge/adapters/js/input_query.rs");
    let luau = read("crates/jengine-core/src/scripting/bridge/adapters/luau.rs");
    assert!(read("crates/jengine-core/src/scripting/bridge/adapters/js/input_mirror.rs").contains("input_query::set_view("));
    for (flat, op) in [
        ("get_action_hold_time", "hold_time"),
        ("is_action_held", "held"),
        ("is_action_tapped", "tapped"),
        ("is_action_double_tapped", "double_tapped"),
        ("get_gamepad_name", "pad_name"),
        ("get_gamepad_type", "pad_type"),
        ("get_last_input_device", "last_device"),
        ("get_last_gamepad", "last_pad"),
        ("get_action_prompt", "prompt"),
    ] {
        let at = preamble.find(&format!("function {flat}(")).unwrap_or_else(|| panic!("no preamble {flat}"));
        let body = &preamble[at..at + preamble[at..].find("}}\n").unwrap()];
        assert!(body.contains(&format!("__input_query('{op}'")), "{flat} does not ask the hook for '{op}'");
        assert!(hook.contains(&format!("\"{op}\" =>")), "the hook has no '{op}'");
        assert!(luau.contains(&format!("globals.set(\"{flat}\"")), "{flat} is not bound natively");
    }
}
