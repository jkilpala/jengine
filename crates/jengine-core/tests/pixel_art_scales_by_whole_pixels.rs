//! **A pixel-perfect camera can scale by whole pixels** (`integer_scale`,
//! Oct 2026). It drew the art's resolution scaled up by whatever the window
//! gave it, so a 320 x 180 game on a 1366 x 768 laptop drew art pixels four
//! and five screen pixels wide by turns - a nearest upscale by 4.27 - and a
//! sprite walking right rippled. With `integer_scale` the camera shows a
//! little more world instead, so every art pixel is exactly `k` screen
//! pixels (`Camera2D::integer_view`) - Godot's integer scaling with an
//! expanded aspect. No black bars, so nothing that turns a screen point into
//! a world point has to learn about them: the camera itself shows the larger
//! area, in every place a camera's view is fitted.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Camera2DComponent, Transform};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::pixel_perfect;

const ART: Vec2 = Vec2::new(1280.0, 720.0); // 320 x 180 art pixels of 4 units

fn view_on(surface: Vec2, pixel_size: f32, zoom: f32) -> Vec2 {
    let fitted = Camera2D::fit_view(ART, surface, false);
    Camera2D::integer_view(fitted, surface, pixel_size, zoom)
}

/// Screen pixels per art pixel for a view on a surface.
fn scale(view: Vec2, surface: Vec2, pixel_size: f32, zoom: f32) -> Vec2 {
    surface / (view / zoom / pixel_size)
}


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
fn a_laptop_gets_four_whole_pixels_and_a_little_more_world() {
    let surface = Vec2::new(1366.0, 768.0);
    let plain = Camera2D::fit_view(ART, surface, false);
    let s = scale(plain, surface, 4.0, 1.0);
    assert!((s.x - 4.2667).abs() < 1e-3, "fitted, an art pixel is {s} screen pixels");
    let v = view_on(surface, 4.0, 1.0);
    assert_eq!(scale(v, surface, 4.0, 1.0), Vec2::splat(4.0), "whole: four by four");
    assert!(v.x >= ART.x && v.y >= ART.y, "the authored area stays in view: {v}");
}

#[test]
fn an_exact_fit_is_left_alone() {
    let surface = Vec2::new(1920.0, 1080.0);
    assert_eq!(view_on(surface, 4.0, 1.0), ART, "1080p is exactly six");
    assert_eq!(view_on(surface, 4.0, 2.0), ART, "zoomed in, exactly twelve");
    assert_eq!(scale(ART, surface, 4.0, 2.0), Vec2::splat(12.0));
}

#[test]
fn a_window_smaller_than_the_art_draws_it_one_to_one() {
    let surface = Vec2::new(200.0, 100.0);
    let v = view_on(surface, 4.0, 1.0);
    assert_eq!(scale(v, surface, 4.0, 1.0), Vec2::splat(1.0));
}

#[test]
fn without_a_pixel_size_nothing_changes() {
    let surface = Vec2::new(1366.0, 768.0);
    let fitted = Camera2D::fit_view(ART, surface, false);
    assert_eq!(Camera2D::integer_view(fitted, surface, 0.0, 1.0), fitted);
}

fn world(integer_scale: bool, target: &str) -> World {
    let mut w = World::new();
    w.spawn((
        Camera2DComponent { pixel_size: 4.0, integer_scale, target: target.into(), ..Default::default() },
        Transform::default(),
    ));
    w
}

/// The main camera takes the integer view, so screen-to-world (scripts, the
/// mouse) and the projection agree on it.
#[test]
fn the_main_camera_shows_the_whole_pixel_view() {
    let surface = Vec2::new(1366.0, 768.0);
    let mut camera = Camera2D::new(surface.x, surface.y);
    jengine_core::ecs::systems::update_camera(&world(true, ""), &mut camera, 0.016);
    let (w, h) = camera.viewport_size();
    assert_eq!(Vec2::new(w, h), view_on(surface, 4.0, 1.0));
    assert!(camera.integer_scale);
    // Off: the fitted view, as before.
    let mut camera = Camera2D::new(surface.x, surface.y);
    jengine_core::ecs::systems::update_camera(&world(false, ""), &mut camera, 0.016);
    let (w, h) = camera.viewport_size();
    assert_eq!(Vec2::new(w, h), Camera2D::fit_view(ART, surface, false));
}

#[test]
fn a_view_into_a_texture_scales_by_whole_pixels_too() {
    let w = world(true, "views/pixel");
    let mut cam = w.query::<&mut Camera2DComponent>();
    let (_, c) = cam.iter().next().unwrap();
    c.target_size = [1366, 768];
    drop(cam);
    let v = &jengine_core::ecs::views::render_target_cameras(&w, (1920, 1080))[0];
    let (vw, vh) = v.camera.viewport_size();
    assert_eq!(Vec2::new(vw, vh), view_on(Vec2::new(1366.0, 768.0), 4.0, 1.0));
}

/// Through the pixel plan, each texel of the art target covers exactly `k`
/// screen pixels: the source shows `surface / k` texels.
#[test]
fn the_pixel_plan_shows_whole_pixels() {
    let surface = Vec2::new(1366.0, 768.0);
    let mut camera = Camera2D::new(surface.x, surface.y);
    jengine_core::ecs::systems::update_camera(&world(true, ""), &mut camera, 0.016);
    camera.position = Vec2::new(13.3, -7.9);
    let p = pixel_perfect::plan(&camera).unwrap();
    let shown = Vec2::new(p.source[2] * p.size.0 as f32, p.source[3] * p.size.1 as f32);
    let per_texel = surface / shown;
    assert!((per_texel - Vec2::splat(4.0)).abs().max_element() < 1e-3, "{per_texel} screen pixels a texel");
}

/// The editor's tick sizes its camera to the canvas before fitting it, as
/// native and web do - its render leaves the camera at a fitted world size,
/// and the integer view needs the canvas's pixels.
#[test]
fn every_runtime_fits_the_camera_to_its_pixels() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../");
    let viewport = std::fs::read_to_string(format!("{root}jengine-editor-viewport/src/lib.rs")).unwrap();
    assert!(
        viewport.contains("app.camera.resize(cw as f32, ch as f32);\n        sys::update_camera("),
        "the editor fits its camera from a stale world size"
    );
    assert!(viewport.contains("Camera2D::integer_view(view, surface"), "the editor's Game view ignores integer scaling");
    // The browser hosts publish the camera's view for the mouse arithmetic
    // in the preamble: through the same rule, or the cursor picks beside
    // what it is over.
    for host in ["jengine-editor-viewport/src/lib.rs", "jengine-web/src/script_host.rs"] {
        let src = with_mirror(&std::fs::read_to_string(format!("{root}{host}")).unwrap());
        assert!(src.contains("cam.view_on("), "{host} publishes a camera view that ignores integer scaling");
    }
    for host in ["jengine-runtime/src/wgpu_scene_runner.rs", "jengine-web/src/lib.rs"] {
        let src = std::fs::read_to_string(format!("{root}{host}")).unwrap();
        let at = src.find("update_camera(").unwrap_or_else(|| panic!("{host} never fits its camera"));
        assert!(src[..at].rfind("camera.resize(").is_some(), "{host} fits its camera without sizing it first");
    }
}
