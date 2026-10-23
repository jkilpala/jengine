//! The camera framing a *set* of things rather than following one.
//!
//! `CameraFollow` has always followed exactly one entity, and that is the
//! wrong shape for a whole genre. A two-player duel, a co-op screen, a boss
//! and the player running from it - each has a set of things that must all
//! stay visible, and none of them is served by picking one to centre on.
//! Before this, a game wanting it had to compute a centre and a zoom in
//! script every frame and write them back across the bridge.
//!
//! The arcade's ship melee is what needed it: two ships whose whole activity
//! is running away from each other.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Camera2DComponent, CameraFollow, Tags, Transform};
use jengine_core::ecs::systems::{frame_of, update_camera};
use jengine_core::renderer::camera::Camera2D;

const VIEW: (f32, f32) = (1280.0, 720.0);

fn camera() -> Camera2D {
    let mut c = Camera2D::new(VIEW.0, VIEW.1);
    c.zoom = 1.0;
    c
}

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE }
}

/// A world with an active camera framing everything tagged `ship`.
fn world_framing(padding: f32, zoom_range: Option<[f32; 2]>) -> World {
    let mut w = World::new();
    w.spawn((
        at(0.0, 0.0),
        Camera2DComponent {
            zoom: 1.0,
            is_active: true,
            viewport_width: VIEW.0,
            viewport_height: VIEW.1,
            stretch: false,
            pixel_size: 0.0,
            ..Default::default()
        },
        CameraFollow {
            // **1.0 is instant**, not 0.0 - `smoothing` is the fraction of
            // the remaining distance covered per frame, so larger is faster.
            // The component's doc comment said the opposite until this test
            // was written against it and the camera never moved.
            smoothing: 1.0,
            frame_tag: Some("ship".into()),
            frame_padding: padding,
            zoom_range,
            ..Default::default()
        },
    ));
    w
}

fn ship(w: &mut World, x: f32, y: f32) -> hecs::Entity {
    w.spawn((at(x, y), Tags(["ship".to_string()].into())))
}

/// Run enough frames that an eased camera has settled.
fn settle(w: &World, c: &mut Camera2D) {
    for _ in 0..240 {
        update_camera(w, c, 1.0 / 60.0);
    }
}

// ---- The arithmetic, on its own -----------------------------------------

#[test]
fn the_frame_is_the_box_around_the_tagged_things_plus_padding() {
    let mut w = World::new();
    ship(&mut w, -100.0, -50.0);
    ship(&mut w, 300.0, 150.0);
    let (centre, size) = frame_of(&w, "ship", 20.0).expect("two ships are tagged");
    assert_eq!(centre, Vec2::new(100.0, 50.0), "the middle of the two");
    assert_eq!(size, Vec2::new(440.0, 240.0), "400x200 plus 20 on each side");
}

#[test]
fn an_untagged_world_frames_nothing() {
    let mut w = World::new();
    w.spawn((at(0.0, 0.0), Tags(["rock".to_string()].into())));
    assert!(frame_of(&w, "ship", 20.0).is_none());
}

#[test]
fn one_target_still_has_a_size_because_of_the_padding() {
    // A single target's box is zero across, which would ask the camera to
    // zoom in infinitely. The padding is what stops that, and is why it
    // defaults to something rather than nothing.
    let mut w = World::new();
    ship(&mut w, 40.0, 40.0);
    let (centre, size) = frame_of(&w, "ship", 60.0).unwrap();
    assert_eq!(centre, Vec2::new(40.0, 40.0));
    assert_eq!(size, Vec2::new(120.0, 120.0));
}

#[test]
fn the_frame_only_counts_the_tag_it_was_asked_for() {
    let mut w = World::new();
    ship(&mut w, 0.0, 0.0);
    ship(&mut w, 100.0, 0.0);
    // A long way away, and irrelevant.
    w.spawn((at(9000.0, 9000.0), Tags(["debris".to_string()].into())));
    let (centre, _) = frame_of(&w, "ship", 0.0).unwrap();
    assert_eq!(centre, Vec2::new(50.0, 0.0), "the debris should not drag the frame");
}

// ---- Through the camera system -----------------------------------------

#[test]
fn the_camera_centres_between_two_targets() {
    let mut w = world_framing(100.0, None);
    ship(&mut w, -400.0, 0.0);
    ship(&mut w, 400.0, 200.0);
    let mut c = camera();
    settle(&w, &mut c);
    assert!((c.position.x - 0.0).abs() < 1.0, "x should be centred: {}", c.position.x);
    assert!((c.position.y - 100.0).abs() < 1.0, "y should be centred: {}", c.position.y);
}

#[test]
fn the_camera_pulls_back_as_the_targets_separate() {
    // The whole point. Two things running away from each other must both
    // stay on screen, and following either one cannot do that.
    let mut close = world_framing(100.0, Some([0.2, 1.0]));
    ship(&mut close, -200.0, 0.0);
    ship(&mut close, 200.0, 0.0);
    let mut c1 = camera();
    settle(&close, &mut c1);

    let mut far = world_framing(100.0, Some([0.2, 1.0]));
    ship(&mut far, -1600.0, 0.0);
    ship(&mut far, 1600.0, 0.0);
    let mut c2 = camera();
    settle(&far, &mut c2);

    assert!(
        c2.zoom < c1.zoom,
        "further apart should be further out: {} apart vs {} close",
        c2.zoom,
        c1.zoom
    );
    // And the far pair should actually fit: the box is 3400 wide including
    // padding, so the zoom has to be about 1280/3400.
    assert!(
        (c2.zoom - 1280.0 / 3400.0).abs() < 0.02,
        "it should fit the box exactly; zoom {}",
        c2.zoom
    );
}

#[test]
fn the_zoom_range_is_respected_at_both_ends() {
    // The maximum matters more than it looks. Without a ceiling, two ships
    // drifting together zoom until the screen is two sprites and a wall of
    // empty space - which hides everything either of them is about to hit.
    let mut together = world_framing(20.0, Some([0.3, 1.4]));
    ship(&mut together, 0.0, 0.0);
    ship(&mut together, 10.0, 0.0);
    let mut c = camera();
    settle(&together, &mut c);
    assert!((c.zoom - 1.4).abs() < 0.01, "it should stop at the max: {}", c.zoom);

    let mut apart = world_framing(20.0, Some([0.3, 1.4]));
    ship(&mut apart, -6000.0, 0.0);
    ship(&mut apart, 6000.0, 0.0);
    let mut c = camera();
    settle(&apart, &mut c);
    assert!((c.zoom - 0.3).abs() < 0.01, "and at the min: {}", c.zoom);
}

#[test]
fn the_fit_uses_whichever_axis_is_tighter() {
    // A viewport is wider than it is tall, so a tall thin box is limited by
    // height. Fitting only on width is the obvious bug and it cuts the tops
    // off both targets.
    let mut w = world_framing(0.0, Some([0.05, 4.0]));
    ship(&mut w, 0.0, -1000.0);
    ship(&mut w, 0.0, 1000.0);
    let mut c = camera();
    settle(&w, &mut c);
    assert!(
        (c.zoom - 720.0 / 2000.0).abs() < 0.02,
        "a 2000-tall box in a 720-tall viewport wants 0.36; got {}",
        c.zoom
    );
}

#[test]
fn a_frame_tag_that_matches_nothing_falls_back_to_following() {
    // A scene must not go blank the moment the things it was framing are
    // destroyed - which in a duel is exactly when one of them loses.
    let mut w = World::new();
    w.spawn((
        at(500.0, 300.0),
        Camera2DComponent {
            zoom: 1.0,
            is_active: true,
            viewport_width: VIEW.0,
            viewport_height: VIEW.1,
            stretch: false,
            pixel_size: 0.0,
            ..Default::default()
        },
        CameraFollow {
            smoothing: 1.0,
            frame_tag: Some("ship".into()),
            ..Default::default()
        },
    ));
    let mut c = camera();
    settle(&w, &mut c);
    assert!(
        (c.position - Vec2::new(500.0, 300.0)).length() < 1.0,
        "it should follow its own entity; it is at {:?}",
        c.position
    );
    assert!((c.zoom - 1.0).abs() < 0.01, "and leave the zoom alone: {}", c.zoom);
}

#[test]
fn framing_follows_the_targets_as_they_move() {
    let mut w = world_framing(100.0, Some([0.2, 1.2]));
    let a = ship(&mut w, -300.0, 0.0);
    ship(&mut w, 300.0, 0.0);
    let mut c = camera();
    settle(&w, &mut c);
    let first = (c.position, c.zoom);

    // One of them runs.
    if let Ok(mut t) = w.get::<&mut Transform>(a) {
        t.position = Vec2::new(-2000.0, 400.0);
    }
    settle(&w, &mut c);
    assert!(c.position.x < first.0.x - 500.0, "the centre should have moved with it");
    assert!(c.zoom < first.1, "and the view pulled back");
}

#[test]
fn a_zoomless_frame_never_pushes_past_the_authored_zoom() {
    // With no `zoom_range`, the component's own zoom is the ceiling. The
    // alternative - unbounded - means an authored zoom is silently
    // meaningless the moment framing is switched on.
    let mut w = world_framing(20.0, None);
    ship(&mut w, 0.0, 0.0);
    ship(&mut w, 5.0, 0.0);
    let mut c = camera();
    c.zoom = 1.0;
    settle(&w, &mut c);
    assert!(c.zoom <= 1.01, "it should not zoom past the authored 1.0: {}", c.zoom);
}

#[test]
fn bounds_still_clamp_a_framed_camera() {
    // Framing and bounds are independent: a duel in an arena still should
    // not show the outside of it.
    let mut w = World::new();
    w.spawn((
        at(0.0, 0.0),
        Camera2DComponent {
            zoom: 1.0,
            is_active: true,
            viewport_width: VIEW.0,
            viewport_height: VIEW.1,
            stretch: false,
            pixel_size: 0.0,
            ..Default::default()
        },
        CameraFollow {
            smoothing: 1.0,
            frame_tag: Some("ship".into()),
            frame_padding: 50.0,
            bounds: Some([-100.0, -100.0, 100.0, 100.0]),
            ..Default::default()
        },
    ));
    ship(&mut w, 4000.0, 4000.0);
    ship(&mut w, 4200.0, 4200.0);
    let mut c = camera();
    settle(&w, &mut c);
    assert!(c.position.x <= 100.01 && c.position.y <= 100.01, "clamped: {:?}", c.position);
}

/// With real smoothing the zoom still arrives at the fit.
///
/// Every other test here uses `smoothing: 1.0`, which covers the whole
/// distance in one frame - and so hid that `update_camera` wrote the
/// component's zoom back to the camera at the top of every frame. With the
/// 0.12 test-lab scene 93 uses, each frame restarted the ease from 1.0 and
/// covered 12% of the way: a framing camera hovered just under 1 and never
/// pulled back (BUGS #163).
#[test]
fn an_eased_frame_still_reaches_the_fit() {
    let mut w = world_framing(160.0, Some([0.25, 1.1]));
    for (_, f) in w.query_mut::<&mut CameraFollow>() {
        f.smoothing = 0.12;
    }
    ship(&mut w, -1540.0, 0.0);
    ship(&mut w, 1540.0, 0.0);
    let mut c = camera();
    settle(&w, &mut c);
    // Box 3080 + 2 x 160 wide -> 1280 / 3400.
    let fit = 1280.0 / 3400.0;
    assert!((c.zoom - fit).abs() < 0.02, "an eased frame should settle at {fit}, it sat at {}", c.zoom);

    // And with nothing to frame the authored zoom comes back.
    for (_, t) in w.query_mut::<&mut Tags>() {
        t.0.clear();
    }
    settle(&w, &mut c);
    assert!((c.zoom - 1.0).abs() < 1e-3, "the authored zoom should return, got {}", c.zoom);
}

#[test]
fn an_eased_frame_zooms_back_in_when_the_set_closes() {
    // No `zoom_range`, so the cap is the authored zoom. It was the
    // renderer's own zoom, which once framing stopped resetting it each
    // frame was last frame's eased value - a cap that only ever came down,
    // so a camera that had pulled back for two runners never came in again.
    let mut w = world_framing(160.0, None);
    for (_, f) in w.query_mut::<&mut CameraFollow>() {
        f.smoothing = 0.12;
    }
    let a = ship(&mut w, -1540.0, 0.0);
    let b = ship(&mut w, 1540.0, 0.0);
    let mut c = camera();
    settle(&w, &mut c);
    assert!(c.zoom < 0.4, "two runners 3080 apart need a pulled-back camera, got {}", c.zoom);

    // Close to 400 apart: 720 + 320 wide fits at 1280 / 720 > 1, capped at 1.
    w.get::<&mut Transform>(a).unwrap().position.x = -200.0;
    w.get::<&mut Transform>(b).unwrap().position.x = 200.0;
    settle(&w, &mut c);
    assert!((c.zoom - 1.0).abs() < 1e-3, "the camera should come back in to the authored zoom, got {}", c.zoom);
}
