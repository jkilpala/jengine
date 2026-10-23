//! A script that declares `tick_rate` runs less often, and must still see
//! time pass at the wall's rate: each `update` is handed the time since it
//! last ran, not one frame's `dt`.
//!
//! It was handed one frame's. `tick_rate = 30` ticks every second frame
//! (the runner's base is 60), so on a 165 Hz display a script adding up `dt`
//! counted about a second for every two - natively only, since the browser
//! hosts tick every script every frame. Every Test Lab scene that "toggles
//! every two seconds" with `tick_rate = 30` toggled every four natively.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

/// (seconds counted, update calls made).
fn clock_after(src: &str, frames: u32, dt: f32) -> (f32, f32) {
    let dir = std::env::temp_dir().join(format!(
        "jengine_throttled_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("clock.lua");
    std::fs::write(&path, src).unwrap();
    let mut world = World::new();
    let e = world.spawn((
        Name("Clock".into()),
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..frames {
        system.run(&mut world, dt, &input, None, None);
    }
    let p = world.get::<&Transform>(e).unwrap().position;
    (p.x / 100.0, p.y)
}

const CLOCK: &str = r#"
local t, calls = 0.0, 0
function update(entity, dt)
    t = t + dt
    calls = calls + 1
    Transform.set_position(entity, t * 100.0, calls)
end
"#;

#[test]
fn a_throttled_script_counts_wall_time() {
    let dt = 1.0 / 165.0;
    let frames = 330; // two seconds
    let (every_frame, all_calls) = clock_after(CLOCK, frames, dt);
    let (throttled, half_calls) = clock_after(&format!("script = {{ tick_rate = 30 }}\n{CLOCK}"), frames, dt);
    let (slow, _) = clock_after(&format!("script = {{ tick_rate = 10 }}\n{CLOCK}"), frames, dt);
    assert!(half_calls < all_calls * 0.6, "tick_rate 30 really runs less often: {half_calls} of {all_calls} calls");
    assert!((every_frame - 2.0).abs() < 0.02, "every frame: {every_frame}");
    // A throttled clock is behind by at most the time since its last tick.
    assert!((throttled - 2.0).abs() < 2.0 * dt + 0.01, "tick_rate 30 counted {throttled} of 2 seconds");
    assert!((slow - 2.0).abs() < 6.0 * dt + 0.01, "tick_rate 10 counted {slow} of 2 seconds");
}

/// A throttled script sees every press, once, and still counts wall time.
///
/// A key is "just pressed" for exactly one frame, and a script ticking every
/// sixth frame used to see about one press in six: Test Lab scene 02
/// (`tick_rate = 10`) ignored Esc and Page Up natively until one happened to
/// land on its frame. A frame on which input changed now runs it too.
#[test]
fn a_throttled_script_sees_every_press() {
    use jengine_core::input::KeyCode;
    const COUNTER: &str = r#"
script = { tick_rate = 10 }
local t, presses = 0.0, 0
function update(entity, dt)
    t = t + dt
    if Input.is_key_just_pressed("Space") then presses = presses + 1 end
    Transform.set_position(entity, t * 100.0, presses)
end
"#;
    let dir = std::env::temp_dir().join(format!("jengine_throttled_presses_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("counter.lua");
    std::fs::write(&path, COUNTER).unwrap();
    let mut world = World::new();
    let e = world.spawn((
        Name("Counter".into()),
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let mut input = InputState::new();
    let dt = 1.0 / 60.0;
    let frames = 120;
    // Presses on frames of every phase, each released two frames later.
    let pressed_on: Vec<u32> = (0..12).map(|i| 3 + i * 9).collect();
    for frame in 0..frames {
        if pressed_on.contains(&frame) {
            input.key_down(KeyCode::Space);
        }
        if frame >= 2 && pressed_on.contains(&(frame - 2)) {
            input.key_up(KeyCode::Space);
        }
        system.run(&mut world, dt, &input, None, None);
        input.begin_frame();
    }
    let p = world.get::<&Transform>(e).unwrap().position;
    assert_eq!(p.y as usize, pressed_on.len(), "every press seen once");
    // Running early on input frames must not count time twice.
    let counted = p.x / 100.0;
    let wall = frames as f32 * dt;
    assert!(counted <= wall + 0.001 && counted > wall - 6.0 * dt - 0.01, "counted {counted} of {wall} s");
}
