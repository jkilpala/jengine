//! Debug shapes must be drawn by every runtime, and F4 must toggle the
//! physics outlines in every runtime.
//!
//! `draw_debug_line` / `_rect` / `_circle` bind in all four script hosts, and
//! native collected every call into `ScriptServices::debug_draws` - then
//! cleared the list at the start of the next frame without ever drawing it.
//! Scene 52 (debug draw) could not pass natively, and nothing said so: the
//! function existed, the command applied, the list filled. Only the render
//! call was missing, so the render call is what this checks.

mod support;

static NATIVE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs")));
static VIEWPORT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs")));
static WEB: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs")));

#[test]
fn native_and_web_draw_script_debug_shapes() {
    for (name, src) in [("native", NATIVE.as_str()), ("web", WEB.as_str())] {
        assert!(
            src.contains("debug_draw::render_debug_commands("),
            "{name} never renders debug shapes, so `draw_debug_*` does nothing there"
        );
    }
    assert!(
        NATIVE.as_str().contains("services.debug_draws"),
        "native renders debug commands but not the ones scripts queued"
    );
}

#[test]
fn every_runtime_has_the_f4_physics_overlay() {
    for (name, src) in [("native", NATIVE.as_str()), ("viewport", VIEWPORT.as_str()), ("web", WEB.as_str())] {
        assert!(
            src.contains("KeyCode::F4"),
            "{name} does not bind F4, so the collider and joint outlines cannot be shown there"
        );
        assert!(
            src.contains("debug_draw::physics_debug_commands("),
            "{name} never draws the physics outlines"
        );
    }
}
