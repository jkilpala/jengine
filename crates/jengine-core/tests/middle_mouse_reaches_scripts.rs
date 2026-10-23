//! Scripts can read the middle mouse button (Oct 2026).
//!
//! No runtime tracked it at all: native's winit handler matched Left and
//! Right, and both browser hosts read buttons 0 and 2, so the wheel pressed
//! - a map's pan, an editor-like "place another" - did nothing anywhere.
//! The browsers also start their autoscroll on it unless it is stopped.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

const SRC: &str = r#"
function update(entity, dt)
    if is_mouse_middle_just_pressed() then
        set_custom(entity, "Probe", "pressed", (get_custom(entity, "Probe", "pressed") or 0) + 1)
    end
    if is_mouse_middle_pressed() then
        set_custom(entity, "Probe", "held", (get_custom(entity, "Probe", "held") or 0) + 1)
    end
    if is_mouse_middle_just_released() then
        set_custom(entity, "Probe", "released", (get_custom(entity, "Probe", "released") or 0) + 1)
    end
end
"#;


/// The per-frame publish both browser hosts share (`adapters::js::mirror`).
const MIRROR: &str = include_str!("../src/scripting/bridge/adapters/js/mirror.rs");

/// A host's source as far as what it publishes goes: its own, plus the shared
/// mirror - but only if it calls it, so a host that dropped the call fails.
fn with_mirror(src: &str) -> String {
    if src.contains("mirror::sync_world_to_play_namespace(") {
        format!("{src}
{MIRROR}")
    } else {
        src.to_string()
    }
}

#[test]
fn the_middle_button_has_the_same_three_readings_as_the_left() {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_middle_mouse_test",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mouse.lua");
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
    let read = |world: &World, key: &str| -> f64 {
        world
            .get::<&CustomComponents>(e)
            .ok()
            .and_then(|c| c.0.get("Probe").and_then(|p| p.get(key)).and_then(|v| v.as_f64()))
            .unwrap_or(0.0)
    };

    // Held down for three frames, then let go.
    input.begin_frame();
    input.set_middle_mouse_button(true);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    for _ in 0..2 {
        input.begin_frame();
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    input.begin_frame();
    input.set_middle_mouse_button(false);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    input.begin_frame();
    system.run(&mut world, 1.0 / 60.0, &input, None, None);

    assert_eq!(read(&world, "pressed"), 1.0, "one press edge");
    assert_eq!(read(&world, "held"), 3.0, "held for three frames");
    assert_eq!(read(&world, "released"), 1.0, "one release edge");
}

/// Every host turns the button into the state: native's winit `Middle`, and
/// button 1 in both browser hosts - with the browser's autoscroll stopped.
#[test]
fn every_host_reports_the_middle_button() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |p: &str| std::fs::read_to_string(root.join(p)).unwrap();
    assert!(read("crates/jengine-runtime/src/wgpu_scene_runner.rs").contains("MouseButton::Middle => state.input.set_middle_mouse_button(pressed)"));
    for host in ["crates/jengine-web/src/lib.rs", "crates/jengine-editor-viewport/src/lib.rs"] {
        let src = read(host);
        assert!(src.contains("set_middle_mouse_button(true)") && src.contains("set_middle_mouse_button(false)"), "{host}");
        assert!(src.contains("prevent_default"), "{host} stops the browser's autoscroll");
    }
    for host in ["crates/jengine-web/src/script_host.rs", "crates/jengine-editor-viewport/src/lib.rs"] {
        assert!(with_mirror(&read(host)).contains("\"mouse_middle_down\""), "{host} publishes the middle button");
    }
}
