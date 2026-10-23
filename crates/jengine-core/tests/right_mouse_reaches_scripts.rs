//! Scripts can read the right mouse button.
//!
//! The engine has tracked it since the beginning - `InputState.mouse_right`,
//! which behaviour graphs read - but no script could: only the left button
//! had globals, so a game wanting right-click to cancel (a tower defense
//! putting a tower back, a strategy game clearing an order) had to use a
//! key instead.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

const SRC: &str = r#"
function update(entity, dt)
    if is_mouse_right_just_pressed() then
        set_custom(entity, "Probe", "pressed", (get_custom(entity, "Probe", "pressed") or 0) + 1)
    end
    if is_mouse_right_pressed() then
        set_custom(entity, "Probe", "held", (get_custom(entity, "Probe", "held") or 0) + 1)
    end
    if is_mouse_right_just_released() then
        set_custom(entity, "Probe", "released", (get_custom(entity, "Probe", "released") or 0) + 1)
    end
end
"#;

#[test]
fn the_right_button_has_the_same_three_readings_as_the_left() {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_right_mouse_test",
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
    input.set_mouse_button(false, true);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    for _ in 0..2 {
        input.begin_frame();
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    input.begin_frame();
    input.set_mouse_button(false, false);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    input.begin_frame();
    system.run(&mut world, 1.0 / 60.0, &input, None, None);

    assert_eq!(read(&world, "pressed"), 1.0, "one press edge");
    assert_eq!(read(&world, "held"), 3.0, "held for three frames");
    assert_eq!(read(&world, "released"), 1.0, "one release edge");
}
