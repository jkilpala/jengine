//! **A script sees every finger.**
//!
//! Before this, a touch screen reached scripts only as the left mouse button
//! (the first free finger), so no game could pinch, play a chord, or take
//! two taps at once. Now `TouchControlsState::tick` keeps every finger on
//! `InputState::touches` and `Input.get_touches()` hands them to scripts.
//!
//! This runs the real path natively: finger events pushed as a host pushes
//! them, the shared `tick`, the script system's snapshot, and a Luau script
//! reading the list back. The browser hosts publish the same list through
//! `input_mirror::publish` (which `input_reaches_every_runtime` checks both
//! call), and every host's `tick` is checked by
//! `touch_controls_reach_every_runtime`.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{CustomComponents, ScriptComponent, ScriptLanguage};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;
use jengine_core::input::touch_controls::{TouchControlsState, TouchPhase};
use jengine_core::ui::scaling::UiScale;

const SRC: &str = r#"
function update(entity, dt)
    local list = Input.get_touches()
    Entity.set_custom(entity, "T", "count", #list)
    local phases, ids = "", ""
    for i, t in ipairs(list) do
        phases = phases .. t.phase .. ","
        ids = ids .. math.floor(t.id) .. ","
        if i == 1 then
            Entity.set_custom(entity, "T", "x", t.x)
            Entity.set_custom(entity, "T", "sx", t.start_x)
            Entity.set_custom(entity, "T", "wx", t.wx)
            Entity.set_custom(entity, "T", "wy", t.wy)
        end
    end
    Entity.set_custom(entity, "T", "phases", phases)
    Entity.set_custom(entity, "T", "ids", ids)
end
"#;

#[test]
fn a_script_reads_every_finger_with_its_phase_and_world_point() {
    let dir = std::env::temp_dir().join(format!("jengine_touches_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fingers.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let e = world.spawn((
        CustomComponents::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut scripts = ScriptSystem::new().expect("script system");
    let mut touch = TouchControlsState::new();
    let ui = UiScale::from_reference(1280.0, 720.0);
    let mut input = InputState::new();
    input.screen_size = (1280.0, 720.0);

    let mut frame = |touch: &mut TouchControlsState, input: &mut InputState, world: &mut World| {
        input.begin_frame();
        touch.tick(None, input, &ui, (1280.0, 720.0));
        scripts.run(world, 1.0 / 60.0, input, None, None);
    };
    let read = |world: &World, k: &str| -> serde_json::Value {
        world.get::<&CustomComponents>(e).unwrap().0.get("T").and_then(|t| t.get(k).cloned()).unwrap_or_default()
    };

    // Two fingers: one at the centre of the screen, one to the right.
    touch.push(4, TouchPhase::Started, 640.0, 360.0);
    touch.push(9, TouchPhase::Started, 900.0, 360.0);
    frame(&mut touch, &mut input, &mut world);
    assert_eq!(read(&world, "count").as_f64(), Some(2.0));
    assert_eq!(read(&world, "phases"), "began,began,");
    assert_eq!(read(&world, "ids"), "4,9,");
    assert_eq!(read(&world, "x").as_f64(), Some(640.0));

    // The first moves; the second is held. The world point follows the
    // finger: with no camera reported, screen centre is the origin.
    touch.push(4, TouchPhase::Moved, 740.0, 260.0);
    frame(&mut touch, &mut input, &mut world);
    assert_eq!(read(&world, "phases"), "moved,held,");
    assert_eq!(read(&world, "sx").as_f64(), Some(640.0), "start_x is where it landed");
    assert_eq!(read(&world, "wx").as_f64(), Some(100.0));
    assert_eq!(read(&world, "wy").as_f64(), Some(100.0), "world y is up");

    // Lifted: seen once as ended, then gone.
    touch.push(4, TouchPhase::Ended, 740.0, 260.0);
    frame(&mut touch, &mut input, &mut world);
    assert_eq!(read(&world, "phases"), "ended,held,");
    frame(&mut touch, &mut input, &mut world);
    assert_eq!(read(&world, "count").as_f64(), Some(1.0));
    assert_eq!(read(&world, "ids"), "9,");
    let _ = std::fs::remove_dir_all(&dir);
}
