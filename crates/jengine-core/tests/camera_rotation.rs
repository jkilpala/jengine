//! The camera can be turned.
//!
//! `Camera2D` had a position and a zoom but no rotation, so a game that
//! wanted the view to follow a car round a corner - or to tilt for an
//! effect - had to rotate every entity in the world instead. The active
//! camera entity's own `Transform.rotation` now turns the view, which
//! means `set_rotation` on it is the whole script API.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{Camera2DComponent, Transform};
use jengine_core::ecs::systems::update_camera;
use jengine_core::renderer::camera::Camera2D;

fn cam(rotation: f32) -> Camera2D {
    let mut c = Camera2D::new(800.0, 600.0);
    c.rotation = rotation;
    c
}

/// Where a world point lands on screen, in pixels from the top-left.
fn to_screen(c: &Camera2D, world: glam::Vec2, screen: glam::Vec2) -> glam::Vec2 {
    let clip = c.view_projection() * glam::Vec4::new(world.x, world.y, 0.0, 1.0);
    glam::Vec2::new((clip.x * 0.5 + 0.5) * screen.x, (0.5 - clip.y * 0.5) * screen.y)
}

#[test]
fn a_quarter_turn_swings_the_world_the_other_way() {
    let screen = glam::Vec2::new(800.0, 600.0);
    let straight = cam(0.0);
    let turned = cam(std::f32::consts::FRAC_PI_2);
    let north = glam::Vec2::new(0.0, 100.0);

    let a = to_screen(&straight, north, screen);
    assert!((a.x - 400.0).abs() < 0.01 && a.y < 300.0, "straight on, north is up: {a:?}");
    // Turning the camera anticlockwise turns the world clockwise on screen,
    // which is what makes a camera matching a car's rotation hold the car's
    // nose still.
    let b = to_screen(&turned, north, screen);
    assert!(b.x > 400.0 && (b.y - 300.0).abs() < 0.01, "turned a quarter, north is to the right: {b:?}");
    // The centre of the view does not move, whatever the turn.
    let c = to_screen(&turned, glam::Vec2::ZERO, screen);
    assert!((c - screen * 0.5).length() < 0.01, "{c:?}");
}

/// The point of the whole feature: an entity turned the same way as the
/// camera keeps its heading on screen.
#[test]
fn an_entity_turning_with_the_camera_holds_its_heading_on_screen() {
    let screen = glam::Vec2::new(800.0, 600.0);
    let mut seen = Vec::new();
    for heading in [0.0f32, 0.9, 2.2, -1.7] {
        let mut c = cam(heading);
        c.position = glam::Vec2::new(30.0, -12.0);
        // A nose 50 units ahead of the car, in the direction it faces.
        let nose = c.position + glam::Vec2::new(heading.cos(), heading.sin()) * 50.0;
        seen.push(to_screen(&c, nose, screen));
    }
    for s in &seen[1..] {
        assert!((*s - seen[0]).length() < 0.05, "the nose should not move: {seen:?}");
    }
}

#[test]
fn screen_to_world_undoes_the_turn() {
    let screen = glam::Vec2::new(800.0, 600.0);
    for angle in [0.0, 0.7, std::f32::consts::FRAC_PI_2, 2.9, -1.3] {
        let mut c = cam(angle);
        c.position = glam::Vec2::new(120.0, -40.0);
        c.zoom = 1.5;
        for point in [glam::Vec2::new(0.0, 0.0), glam::Vec2::new(800.0, 600.0), glam::Vec2::new(311.0, 92.0)] {
            let world = c.screen_to_world(point, screen);
            let back = to_screen(&c, world, screen);
            assert!((back - point).length() < 0.05, "angle {angle}: {point:?} -> {world:?} -> {back:?}");
        }
    }
}

#[test]
fn a_turned_camera_sees_a_wider_box() {
    let straight = cam(0.0).half_extents();
    assert_eq!(straight, glam::Vec2::new(400.0, 300.0));
    let turned = cam(std::f32::consts::FRAC_PI_4).half_extents();
    assert!(turned.x > 490.0 && turned.y > 490.0, "at 45 degrees the corners swing out: {turned:?}");
    let quarter = cam(std::f32::consts::FRAC_PI_2).half_extents();
    assert!((quarter.x - 300.0).abs() < 0.01 && (quarter.y - 400.0).abs() < 0.01, "{quarter:?}");
}

#[test]
fn the_active_camera_entitys_rotation_drives_the_view() {
    let mut world = World::new();
    world.spawn((
        Transform { position: glam::Vec2::new(10.0, 20.0), rotation: 0.6, ..Default::default() },
        Camera2DComponent { zoom: 2.0, is_active: true, viewport_width: 800.0, viewport_height: 600.0, stretch: false, pixel_size: 0.0, ..Default::default() },
    ));
    // An inactive camera, turned the other way, must not be the one used.
    world.spawn((
        Transform { position: glam::Vec2::new(-90.0, 0.0), rotation: -1.2, ..Default::default() },
        Camera2DComponent { zoom: 1.0, is_active: false, viewport_width: 800.0, viewport_height: 600.0, stretch: false, pixel_size: 0.0, ..Default::default() },
    ));
    let mut camera = Camera2D::new(800.0, 600.0);
    update_camera(&world, &mut camera, 1.0 / 60.0);
    assert_eq!(camera.rotation, 0.6);
    assert_eq!(camera.position, glam::Vec2::new(10.0, 20.0));
    assert_eq!(camera.zoom, 2.0);
}
