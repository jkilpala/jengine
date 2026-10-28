//! **A camera shows more of the world on a wider screen; it does not stretch.**
//!
//! A camera authors a viewport - 1280 x 720, usually - and a phone is about
//! 20:9. Until Sep 2026 the native and web runtimes drew that 1280 x 720 of
//! world stretched over whatever shape the screen was, so on a phone a square
//! was drawn a quarter wider than tall (test-lab scene 105's box, on the
//! Android emulator). The editor's Game view had widened the view to the
//! canvas's shape all along, so a game looked right in the editor and wrong
//! once shipped - and even there `get_mouse_world_position` still used the
//! authored size, so a click on a non-16:9 editor panel picked slightly off.
//!
//! `Camera2D::fit_view` is the one rule now: the authored area always visible,
//! extra world along the longer axis. Everything that turns a screen point
//! into a world point has to use it too, or the cursor and the picture
//! disagree - which is what the source checks below are for.

use glam::Vec2;
use hecs::World;
use jengine_core::behavior::system::mouse_world_position;
use jengine_core::ecs::components::{Camera2DComponent, Transform};
use jengine_core::ecs::systems::update_camera;
use jengine_core::renderer::camera::Camera2D;

const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB_HOST: &str = include_str!("../../jengine-web/src/script_host.rs");

/// A 20:9 phone.
const PHONE: Vec2 = Vec2::new(2400.0, 1080.0);


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
fn the_authored_area_stays_visible_and_the_long_axis_grows() {
    let authored = Vec2::new(1280.0, 720.0);
    assert_eq!(Camera2D::fit_view(authored, PHONE, false), Vec2::new(1600.0, 720.0));
    // A tall phone grows the height instead.
    assert_eq!(Camera2D::fit_view(authored, Vec2::new(1080.0, 2400.0), false), Vec2::new(1280.0, 1280.0 * 2400.0 / 1080.0));
    // A 16:9 window of any size changes nothing.
    assert_eq!(Camera2D::fit_view(authored, Vec2::new(1920.0, 1080.0), false), authored);
    // Stretch keeps the authored size, whatever the screen.
    assert_eq!(Camera2D::fit_view(authored, PHONE, true), authored);
    // Fitting a fitted size again is a no-op: only the surface's shape matters.
    let once = Camera2D::fit_view(authored, PHONE, false);
    assert_eq!(Camera2D::fit_view(authored, once, false), once);
}

fn world_with_camera(stretch: bool) -> World {
    let mut world = World::new();
    world.spawn((
        Transform::default(),
        Camera2DComponent { zoom: 1.0, is_active: true, viewport_width: 1280.0, viewport_height: 720.0, stretch, pixel_size: 0.0, ..Default::default() },
    ));
    world
}

/// What every host does each frame: size the camera to the surface, then let
/// the scene's camera component have its say.
#[test]
fn the_frame_camera_widens_on_a_wide_screen() {
    let world = world_with_camera(false);
    let mut camera = Camera2D::new(PHONE.x, PHONE.y);
    camera.resize(PHONE.x, PHONE.y);
    update_camera(&world, &mut camera, 0.016);
    assert_eq!(camera.viewport_size(), (1600.0, 720.0));
    // And a world unit is square: pixels per unit match on both axes.
    let (w, h) = camera.viewport_size();
    assert!((PHONE.x / w - PHONE.y / h).abs() < 1e-4);

    let world = world_with_camera(true);
    let mut camera = Camera2D::new(PHONE.x, PHONE.y);
    update_camera(&world, &mut camera, 0.016);
    assert_eq!(camera.viewport_size(), (1280.0, 720.0), "stretch asked for the old way");
}

/// Behaviour graphs turn the mouse into world space with numbers alone; they
/// must land where the renderer's camera says the cursor is.
#[test]
fn the_mouse_picks_what_the_screen_shows() {
    let world = world_with_camera(false);
    let mut camera = Camera2D::new(PHONE.x, PHONE.y);
    update_camera(&world, &mut camera, 0.016);
    for point in [Vec2::new(0.0, 0.0), Vec2::new(2400.0, 540.0), Vec2::new(600.0, 900.0)] {
        let from_camera = camera.screen_to_world(point, PHONE);
        let from_graph = mouse_world_position(&world, (point.x, point.y), (PHONE.x, PHONE.y));
        assert!(
            (from_camera - from_graph).length() < 1e-3,
            "{point}: camera {from_camera}, graph {from_graph}"
        );
    }
    // The right edge of a phone is 800 world units out, not 640.
    let right = mouse_world_position(&world, (2400.0, 540.0), (PHONE.x, PHONE.y));
    assert!((right.x - 800.0).abs() < 1e-3, "{right}");
}

/// Both browser hosts publish the camera size `get_mouse_world_position`
/// divides by; it has to be the fitted one.
#[test]
fn the_browser_hosts_publish_the_fitted_view() {
    for (name, src) in [("viewport", with_mirror(VIEWPORT)), ("web", with_mirror(WEB_HOST))] {
        let src = src.as_str();
        let publish = src.find("\"camera_vw\"").unwrap_or_else(|| panic!("{name} publishes no camera_vw"));
        let before = &src[publish.saturating_sub(1500)..publish];
        assert!(
            before.contains("view_on(") || before.contains("fit_view("),
            "{name} publishes the authored camera size for `get_mouse_world_position` \
             while drawing the fitted one - the cursor picks beside what it is over"
        );
    }
}

/// The native runner hands scripts the view `get_mouse_world_position`
/// divides by, and it has to be the fitted one too. It handed over
/// `state.camera`'s size - which, before the scripts, is the surface's own
/// pixels (the frame resizes the camera to the surface and `update_camera`
/// fits it only after them) - so on any screen not exactly the authored size
/// every tap and click converted to the wrong world point. On a 2622 x 1206
/// iPhone simulator the showcase hub's rows answered no tap at all.
#[test]
fn the_native_runner_hands_scripts_the_fitted_view() {
    const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
    let call = NATIVE.find("set_camera_view(").expect("the native runner reports no camera view");
    let before = &NATIVE[call.saturating_sub(1500)..call];
    assert!(
        before.contains("view_on(") || before.contains("fit_view("),
        "the native runner hands scripts the camera's surface-sized view for \
         `get_mouse_world_position` while drawing the fitted one"
    );
}
