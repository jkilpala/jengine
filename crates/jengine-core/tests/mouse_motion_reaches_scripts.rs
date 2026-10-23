//! `Input.get_mouse_delta` - how far the mouse moved this frame (Oct 2026).
//!
//! Before it, a script read motion as the difference of two
//! `get_mouse_position`s, which stops when the pointer reaches the window
//! edge and jumps when it first enters the window (the harness, and a
//! browser before the first move, put it far off). A tube shooter whose
//! mouse is a spinner met both. The delta is the device's raw
//! motion where the host has it (winit's `DeviceEvent::MouseMotion`, a
//! browser's `movementX` / `movementY`), so it keeps counting at the edge
//! and while the pointer is locked (`Cursor.set_locked`).

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

const SRC: &str = r#"
function update(entity, dt)
    local dx, dy = Input.get_mouse_delta()
    set_custom(entity, "Probe", "dx", dx)
    set_custom(entity, "Probe", "dy", dy)
    set_custom(entity, "Probe", "sum", (get_custom(entity, "Probe", "sum") or 0) + dx)
end
"#;

/// The per-frame publish both browser hosts share (`adapters::js::mirror`).
const MIRROR: &str = include_str!("../src/scripting/bridge/adapters/js/mirror.rs");

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(p: &str) -> String {
    std::fs::read_to_string(root().join(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

#[test]
fn a_frames_motion_adds_up_and_starts_again_next_frame() {
    let dir = std::env::temp_dir().join(format!(
        "jengine_mouse_motion_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("motion.lua");
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
    let get = |world: &World, key: &str| -> f64 {
        world
            .get::<&CustomComponents>(e)
            .ok()
            .and_then(|c| c.0.get("Probe").and_then(|p| p.get(key)).and_then(|v| v.as_f64()))
            .unwrap_or(f64::NAN)
    };

    // Two motion events in one frame add up.
    input.begin_frame();
    input.mouse_motion(12.0, -3.0);
    input.mouse_motion(8.0, 1.0);
    // The position does not decide the delta: a pointer held at the edge
    // (or parked far off) moves nothing here.
    input.mouse_position = (-1000.0, -1000.0);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    assert_eq!((get(&world, "dx"), get(&world, "dy")), (20.0, -2.0));

    // The next frame starts from nothing.
    input.begin_frame();
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    assert_eq!((get(&world, "dx"), get(&world, "dy")), (0.0, 0.0));

    // Motion past where an absolute position would have stopped keeps counting.
    for _ in 0..10 {
        input.begin_frame();
        input.mouse_motion(50.0, 0.0);
        input.mouse_position = (1279.0, 300.0);
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    assert_eq!(get(&world, "sum"), 520.0, "20 + 0 + ten frames of 50");
}

/// Moving the mouse is not using it: only a press changes `last_device`.
#[test]
fn motion_does_not_claim_the_last_device() {
    let mut input = InputState::new();
    let before = input.last_device;
    input.mouse_motion(30.0, 30.0);
    assert_eq!(input.last_device, before);
}

/// Every host feeds the delta, and both browser hosts publish it.
#[test]
fn every_host_feeds_and_publishes_the_motion() {
    let native = read("crates/jengine-runtime/src/wgpu_scene_runner.rs");
    assert!(native.contains("DeviceEvent::MouseMotion { delta }"), "native reads the device's raw motion");
    assert!(native.contains("state.input.mouse_motion(delta.0 as f32, delta.1 as f32)"));
    assert!(native.contains("state.window.has_focus()"), "raw motion comes unfocused too; only a focused window takes it");
    assert!(read("crates/jengine-runtime/src/lib.rs").contains("wgpu_scene_runner::handle_device_event(state, event)"),
        "the app hands device events to the runner");
    for host in ["crates/jengine-web/src/lib.rs", "crates/jengine-editor-viewport/src/lib.rs"] {
        let src = read(host);
        assert!(src.contains("movement_x()") && src.contains("mouse_motion("), "{host} feeds movementX / movementY");
    }
    assert!(MIRROR.contains("\"mouse_dx\"") && MIRROR.contains("\"mouse_dy\""), "the shared mirror publishes the delta");
    for host in ["crates/jengine-web/src/script_host.rs", "crates/jengine-editor-viewport/src/lib.rs"] {
        assert!(read(host).contains("mirror::sync_world_to_play_namespace("), "{host} runs the shared mirror");
    }
    let preamble = read("crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs");
    assert!(preamble.contains("function get_mouse_delta()") && preamble.contains("p.mouse_dx"), "the preamble reads it");
}
