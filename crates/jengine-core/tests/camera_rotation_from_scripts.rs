//! `get_mouse_world_position()` follows a turned camera.
//!
//! The camera can be rotated since Sep 2026 (its entity's
//! `Transform.rotation` drives the view), which means every place that
//! converts screen pixels to world units has to turn the offset too.
//! `Camera2D::screen_to_world` is the reference; this checks the native
//! script path agrees with it. The two browser hosts publish
//! `camera_rot` for the preamble to do the same arithmetic, and
//! `play_state_reaches_browser_hosts` guards that half.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;
use jengine_core::renderer::camera::Camera2D;

const SRC: &str = r#"
function update(entity, dt)
    local x, y = get_mouse_world_position()
    set_custom(entity, "Probe", "x", x)
    set_custom(entity, "Probe", "y", y)
end
"#;

#[test]
fn the_mouse_lands_where_the_turned_view_puts_it() {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_camera_rotation_test",
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
        Name("Pointer".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");

    let screen = (800.0f32, 600.0f32);
    let mut input = InputState::new();
    input.screen_size = screen;
    input.mouse_position = (640.0, 150.0);

    let mut camera = Camera2D::new(800.0, 600.0);
    camera.position = glam::Vec2::new(-70.0, 25.0);
    camera.zoom = 1.25;

    for angle in [0.0f32, 0.8, std::f32::consts::FRAC_PI_2, -2.1] {
        camera.rotation = angle;
        system.set_camera_view((camera.position.x, camera.position.y, camera.width(), camera.height(), camera.zoom));
        system.set_camera_rotation(camera.rotation);
        system.run(&mut world, 1.0 / 60.0, &input, None, None);

        let custom = world.get::<&CustomComponents>(e).unwrap();
        let probe = custom.0.get("Probe").expect("the script wrote a probe");
        let got = glam::Vec2::new(
            probe.get("x").and_then(|v| v.as_f64()).unwrap() as f32,
            probe.get("y").and_then(|v| v.as_f64()).unwrap() as f32,
        );
        drop(custom);
        let want = camera.screen_to_world(glam::Vec2::new(input.mouse_position.0, input.mouse_position.1), glam::Vec2::new(screen.0, screen.1));
        assert!((got - want).length() < 0.05, "at {angle} rad the script said {got:?}, the camera says {want:?}");
    }
}
