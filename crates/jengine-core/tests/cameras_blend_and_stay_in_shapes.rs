//! Cameras that hand the view to one another, and a camera whose view stays
//! inside a shape (Oct 2026; Cinemachine's priorities, blends and confiner).
//!
//! Before this the main camera was the first active one the query met, so
//! switching meant turning one off and another on - a cut, every time - and
//! a camera's only bounds were `CameraFollow.bounds`, a rectangle that held
//! the view's *centre*: a gallery shaped like an L showed the street past
//! its walls at every corner.

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::camera_rig::{main_camera, view_fits};
use jengine_core::ecs::components::{Camera2DComponent, CameraFollow, Transform};
use jengine_core::ecs::systems::{set_active_camera_zoom, update_camera};
use jengine_core::renderer::camera::Camera2D;

const DT: f32 = 0.05;

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE }
}

fn cam(priority: i32, zoom: f32, blend: f32) -> Camera2DComponent {
    Camera2DComponent { priority, zoom, blend, viewport_width: 1280.0, viewport_height: 720.0, ..Default::default() }
}

fn set_priority(w: &World, e: Entity, p: i32) {
    w.get::<&mut Camera2DComponent>(e).unwrap().priority = p;
}

fn run(w: &World, c: &mut Camera2D, seconds: f32) {
    for _ in 0..(seconds / DT).round() as usize {
        update_camera(w, c, DT);
    }
}

#[test]
fn a_camera_taking_over_blends_from_where_the_view_was() {
    let mut w = World::new();
    let a = w.spawn((at(0.0, 0.0), cam(1, 1.0, 0.0)));
    let b = w.spawn((at(1000.0, 0.0), cam(0, 2.0, 1.0)));
    let mut c = Camera2D::new(1280.0, 720.0);
    run(&w, &mut c, 0.2);
    assert_eq!(c.position, Vec2::ZERO);
    assert_eq!(main_camera(&w), Some(a));

    set_priority(&w, b, 5);
    update_camera(&w, &mut c, DT);
    assert!(c.position.x < 50.0, "a blend starts where the view was, not at the new camera: {}", c.position);
    run(&w, &mut c, 0.45);
    assert!((c.position.x - 500.0).abs() < 30.0, "halfway through, halfway there: {}", c.position);
    assert!((c.zoom - 2f32.sqrt()).abs() < 0.08, "zoom eases by ratio - 1 to 2 passes 1.41 halfway: {}", c.zoom);
    run(&w, &mut c, 0.6);
    assert_eq!(c.position, Vec2::new(1000.0, 0.0));
    assert_eq!(c.zoom, 2.0);
    assert!(c.blend.is_none());
}

#[test]
fn a_camera_with_no_blend_cuts() {
    let mut w = World::new();
    w.spawn((at(0.0, 0.0), cam(1, 1.0, 0.0)));
    let b = w.spawn((at(1000.0, 0.0), cam(0, 1.0, 0.0)));
    let mut c = Camera2D::new(1280.0, 720.0);
    run(&w, &mut c, 0.1);
    set_priority(&w, b, 5);
    update_camera(&w, &mut c, DT);
    assert_eq!(c.position, Vec2::new(1000.0, 0.0));
}

#[test]
fn handing_back_mid_blend_does_not_jump() {
    let mut w = World::new();
    let a = w.spawn((at(0.0, 0.0), cam(1, 1.0, 0.6)));
    let b = w.spawn((at(1000.0, 0.0), cam(0, 1.0, 1.0)));
    let mut c = Camera2D::new(1280.0, 720.0);
    run(&w, &mut c, 0.1);
    set_priority(&w, b, 5);
    run(&w, &mut c, 0.5);
    let mid = c.position;
    set_priority(&w, a, 9);
    update_camera(&w, &mut c, DT);
    assert!(c.position.distance(mid) < 40.0, "{mid} -> {}", c.position);
    run(&w, &mut c, 0.7);
    assert_eq!(c.position, Vec2::ZERO);
}

#[test]
fn the_new_camera_keeps_following_while_the_view_blends_to_it() {
    // Two cameras following one thief, the second confined to a room on
    // the right; the thief walks on during the blend and the view ends on
    // the second camera's own, live position.
    let mut w = World::new();
    let thief = w.spawn((at(0.0, 0.0), CameraFollow { smoothing: 1.0, ..Default::default() }));
    w.spawn((at(0.0, 0.0), cam(1, 1.0, 0.0)));
    let room = vec![[1000.0, -500.0], [3000.0, -500.0], [3000.0, 500.0], [1000.0, 500.0]];
    let b = w.spawn((at(0.0, 0.0), Camera2DComponent { confine: room, ..cam(0, 1.0, 0.8) }));
    let mut c = Camera2D::new(1280.0, 720.0);
    run(&w, &mut c, 0.1);
    set_priority(&w, b, 5);
    for i in 0..30 {
        w.get::<&mut Transform>(thief).unwrap().position.x = 1500.0 + i as f32 * 20.0;
        update_camera(&w, &mut c, DT);
    }
    // The thief is at 2080; the room keeps the view's right edge at 3000.
    assert!((c.position.x - 2080.0).abs() < 1.0, "{}", c.position);
    assert!(c.blend.is_none());
}

#[test]
fn a_followed_thief_never_shows_the_view_past_the_gallery_walls() {
    // An L-shaped gallery: a wide hall along the bottom and a tall one up
    // its left. The thief walks everywhere, outside it too.
    let gallery = [[0.0, 0.0], [3000.0, 0.0], [3000.0, 900.0], [1400.0, 900.0], [1400.0, 2600.0], [0.0, 2600.0]];
    let mut w = World::new();
    let thief = w.spawn((at(100.0, 100.0), CameraFollow { smoothing: 0.3, ..Default::default() }));
    w.spawn((at(0.0, 0.0), Camera2DComponent { confine: gallery.to_vec(), ..cam(0, 1.0, 0.0) }));
    let mut c = Camera2D::new(1280.0, 720.0);
    let pts: Vec<Vec2> = gallery.iter().map(|p| Vec2::from(*p)).collect();
    let route = [(100.0, 100.0), (2900.0, 450.0), (2900.0, 1500.0), (700.0, 450.0), (700.0, 2500.0), (-400.0, 3000.0), (2000.0, 2000.0), (100.0, 100.0)];
    for leg in route.windows(2) {
        let (from, to) = (Vec2::from(leg[0]), Vec2::from(leg[1]));
        for i in 0..=60 {
            let p = from.lerp(to, i as f32 / 60.0);
            w.get::<&mut Transform>(thief).unwrap().position = p;
            update_camera(&w, &mut c, 1.0 / 60.0);
            assert!(view_fits(&pts, c.position, c.half_extents()), "the view at {} shows past a wall (thief at {p})", c.position);
        }
    }
}

#[test]
fn the_zoom_setter_reaches_the_main_camera() {
    let mut w = World::new();
    let low = w.spawn((at(0.0, 0.0), cam(0, 1.0, 0.0)));
    let high = w.spawn((at(0.0, 0.0), cam(3, 1.0, 0.0)));
    assert!(set_active_camera_zoom(&mut w, 2.0));
    assert_eq!(w.get::<&Camera2DComponent>(high).unwrap().zoom, 2.0);
    assert_eq!(w.get::<&Camera2DComponent>(low).unwrap().zoom, 1.0, "only the camera drawing the screen zooms");
}

#[test]
fn the_view_follows_the_thief_into_the_next_room() {
    // The showcase's hall and gallery: the hall camera at zoom 1, the
    // gallery's at 0.9 with a blend, handed the screen as the thief crosses.
    let hall = vec![[0.0, 0.0], [1800.0, 0.0], [1800.0, 1200.0], [0.0, 1200.0]];
    let gallery = vec![[1800.0, 100.0], [4400.0, 100.0], [4400.0, 2600.0], [2900.0, 2600.0], [2900.0, 1100.0], [1800.0, 1100.0]];
    let mut w = World::new();
    let thief = w.spawn((at(300.0, 600.0), CameraFollow { smoothing: 0.12, ..Default::default() }));
    let a = w.spawn((at(900.0, 600.0), Camera2DComponent { confine: hall, ..cam(1, 1.0, 0.9) }));
    let b = w.spawn((at(3000.0, 1300.0), Camera2DComponent { confine: gallery.clone(), ..cam(0, 0.9, 0.9) }));
    let mut c = Camera2D::new(1280.0, 720.0);
    let pts: Vec<Vec2> = gallery.iter().map(|p| Vec2::from(*p)).collect();
    for i in 0..600 {
        let x = 300.0 + i as f32 * 320.0 / 60.0;
        w.get::<&mut Transform>(thief).unwrap().position = Vec2::new(x, 600.0);
        if x > 1800.0 && main_camera(&w) == Some(a) {
            set_priority(&w, a, 0);
            set_priority(&w, b, 10);
        }
        update_camera(&w, &mut c, 1.0 / 60.0);
        if x > 2600.0 {
            assert!(view_fits(&pts, c.position, c.half_extents()), "{x}: {}", c.position);
            assert!((c.position.x - x).abs() < 120.0, "the view lost the thief at {x}: {}", c.position);
        }
    }
}
