//! **On-screen touch controls reach every runtime, and do what they say.**
//!
//! A phone has no keyboard, so a `touch_controls` component puts sticks and
//! buttons on screen that press keys (`input::touch_controls`). The logic is
//! shared; each host only has to feed it fingers, tick it before anything
//! reads input, and draw it - and a host that skips one of those has controls
//! that do nothing, or that nobody can see, with no error anywhere. The
//! native runtime is what an Android build runs and the web runtime is what a
//! phone's browser runs, so both take real fingers; the editor viewport has
//! none, and works a `show: "always"` layout with the mouse.

mod support;

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{TouchControl, TouchControls, TouchControlsShow};
use jengine_core::input::touch_controls::{TouchControlsState, TouchPhase};
use jengine_core::input::{InputAction, InputState, KeyCode};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::scene::format::load_scene_auto;

static NATIVE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs")));
static VIEWPORT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs")));
static WEB: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs")));

#[test]
fn every_runtime_ticks_and_draws_the_controls() {
    for (name, src) in [("native", NATIVE.as_str()), ("viewport", VIEWPORT.as_str()), ("web", WEB.as_str())] {
        assert!(src.contains("touch.tick("), "{name} never ticks its touch controls");
        assert!(
            src.contains("render_touch_controls_wgpu("),
            "{name} never draws its touch controls"
        );
        assert!(
            src.contains("touch_controls::insert_textures("),
            "{name} does not register the textures touch controls draw with"
        );
    }
    // The two that run on phones queue real fingers - all of them, by id,
    // not the first one only.
    for (name, src) in [("native", NATIVE.as_str()), ("web", WEB.as_str())] {
        assert!(src.contains("touch.push("), "{name} does not queue its fingers");
    }
    // The shared tick is what turns a free finger into the mouse now; a host
    // that still did it itself would press the button twice.
    for (name, src) in [("native", NATIVE.as_str()), ("web", WEB.as_str())] {
        assert!(
            !src.contains("touch_id"),
            "{name} keeps its own finger-as-mouse state beside the shared one"
        );
    }
}

/// A scene authors the component the way the docs show, it survives the
/// loader, and a finger on its button presses the action's key.
#[test]
fn a_layout_from_a_scene_presses_keys() {
    let scene = r#"{
        "version": 1,
        "name": "t",
        "entities": [{
            "name": "controls",
            "components": {
                "touch_controls": {
                    "controls": [
                        { "type": "stick", "anchor": "bottom_left", "offset": [160, 160] },
                        { "type": "button", "anchor": "bottom_right", "offset": [130, 130],
                          "press": "jump", "label": "A" }
                    ]
                }
            }
        }]
    }"#;
    let mut world = World::new();
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    load_scene_auto(scene, &mut world, &registry).expect("scene loads");
    let controls = world
        .query::<&TouchControls>()
        .iter()
        .next()
        .map(|(_, c)| c.clone())
        .expect("the component survives the loader");
    assert_eq!(controls.show, TouchControlsShow::Touch);
    assert!(matches!(controls.controls[0], TouchControl::Stick(_)));

    let mut input = InputState::new();
    input.load_actions(&[InputAction { name: "jump".into(), keys: vec!["Space".into()], dead_zone: 0.0 }]);
    let mut touch = TouchControlsState::new();
    let screen = Vec2::new(1280.0, 720.0);
    let ui = jengine_core::ui::scaling::UiScale::from_reference(screen.x, screen.y);
    touch.push(0, TouchPhase::Started, 1280.0 - 130.0, 720.0 - 130.0);
    touch.tick(Some(&controls), &mut input, &ui, (screen.x, screen.y));
    assert!(input.is_action_pressed("jump"));
    assert!(input.is_pressed(KeyCode::Space));
    // The stick's default bindings are the arrow keys.
    touch.push(1, TouchPhase::Started, 160.0, 560.0);
    touch.push(1, TouchPhase::Moved, 260.0, 560.0);
    touch.tick(Some(&controls), &mut input, &ui, (screen.x, screen.y));
    assert!(input.is_pressed(KeyCode::Right));
}

/// Every binding in every shipped layout presses something.
///
/// A control whose binding names neither a key nor an action with a
/// keyboard key does nothing when pressed, silently - "Shift" for
/// `ShiftLeft`, or an action a project renamed. Layouts are often generated
/// from a game's control hints rather than written by hand, so this reads
/// every shipped one, through the component's own deserializer.
#[test]
fn every_shipped_binding_presses_a_key() {
    let projects = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects");
    let mut checked = 0;
    let mut dead = Vec::new();
    for project in std::fs::read_dir(&projects).unwrap().flatten().map(|e| e.path()) {
        let Ok(text) = std::fs::read_to_string(project.join("project.jengine")) else { continue };
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        let actions: Vec<InputAction> = serde_json::from_value(
            doc["settings"]["input_actions"].clone(),
        )
        .unwrap_or_default();
        let mut input = InputState::new();
        input.load_actions(&actions);
        let presses = |binding: &str| -> bool {
            binding.is_empty()
                || input
                    .get_bindings(binding)
                    .is_some_and(|keys| keys.iter().any(|k| KeyCode::from_name(k).is_some()))
                || KeyCode::from_name(binding).is_some()
        };
        let Ok(scenes) = std::fs::read_dir(project.join("assets/scenes")) else { continue };
        for scene in scenes.flatten().map(|e| e.path()) {
            let Ok(text) = std::fs::read_to_string(&scene) else { continue };
            if !text.contains("touch_controls") {
                continue;
            }
            let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
            for entity in doc["entities"].as_array().into_iter().flatten() {
                let Some(raw) = entity["components"].get("touch_controls") else { continue };
                let controls: TouchControls = serde_json::from_value(raw.clone())
                    .unwrap_or_else(|e| panic!("{}: touch_controls does not load: {e}", scene.display()));
                for control in &controls.controls {
                    let bindings: Vec<&str> = match control {
                        TouchControl::Stick(s) => vec![&s.up, &s.down, &s.left, &s.right],
                        TouchControl::Button(b) => vec![&b.press],
                    };
                    for b in bindings {
                        checked += 1;
                        if !presses(b) {
                            dead.push(format!("{}: '{b}'", scene.display()));
                        }
                    }
                }
            }
        }
    }
    // Test-lab scene 105's stick alone is four bindings; fewer means the
    // walk above stopped reading layouts.
    assert!(checked >= 4, "only {checked} bindings found - did the layouts go?");
    assert!(dead.is_empty(), "bindings that press nothing:\n{}", dead.join("\n"));
}
