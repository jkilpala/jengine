//! Things that have left the view clean themselves up.
//!
//! `Lifetime` covers "this goes away after a while". The other half - "this
//! goes away once nobody can see it" - had no component at all, so every
//! scrolling game wrote the same loop in Lua: keep your own list of what you
//! spawned, compare each one against the camera every frame, despawn the
//! ones behind. Five games had five copies of it.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::behaviors::despawn_offscreen;
use jengine_core::ecs::components::{DespawnOffscreen, Transform};

fn at(world: &mut World, x: f32, y: f32, rule: DespawnOffscreen) -> hecs::Entity {
    world.spawn((Transform { position: glam::Vec2::new(x, y), ..Default::default() }, rule))
}

/// Something the camera has already reached: only that can be left behind.
fn seen_at(world: &mut World, x: f32, y: f32, rule: DespawnOffscreen) -> hecs::Entity {
    at(world, x, y, DespawnOffscreen { seen: true, ..rule })
}

const VIEW: (glam::Vec2, glam::Vec2) = (glam::Vec2::ZERO, glam::Vec2::new(640.0, 360.0));
const STILL: glam::Vec2 = glam::Vec2::ZERO;

#[test]
fn what_is_on_screen_stays() {
    let mut world = World::new();
    let rule = DespawnOffscreen { margin: 100.0, behind_only: false, ..Default::default() };
    let middle = at(&mut world, 0.0, 0.0, rule);
    let edge = at(&mut world, 600.0, 300.0, rule);
    // Just inside the margin: 640 + 100 = 740.
    let margin = at(&mut world, 735.0, 0.0, rule);
    let gone = despawn_offscreen(&mut world, VIEW, STILL);
    assert!(gone.is_empty(), "nothing has left yet: {gone:?}");
    let _ = (middle, edge, margin);
}

#[test]
fn what_has_left_goes() {
    let mut world = World::new();
    let rule = DespawnOffscreen { margin: 100.0, behind_only: false, ..Default::default() };
    let right = at(&mut world, 760.0, 0.0, rule);
    let below = at(&mut world, 0.0, -500.0, rule);
    let gone = despawn_offscreen(&mut world, VIEW, STILL);
    assert_eq!(gone.len(), 2, "{gone:?}");
    assert!(gone.contains(&right) && gone.contains(&below));
}

#[test]
fn the_margin_is_what_keeps_a_wide_sprite_whole() {
    let mut world = World::new();
    let tight = at(&mut world, 700.0, 0.0, DespawnOffscreen { margin: 20.0, behind_only: false, ..Default::default() });
    let generous = at(&mut world, 700.0, 0.0, DespawnOffscreen { margin: 400.0, behind_only: false, ..Default::default() });
    let gone = despawn_offscreen(&mut world, VIEW, STILL);
    assert_eq!(gone, vec![tight], "only the one with no room to spare");
    let _ = generous;
}

#[test]
fn behind_only_keeps_what_is_still_to_come() {
    let mut world = World::new();
    let rule = DespawnOffscreen { margin: 100.0, behind_only: true, ..Default::default() };
    let ahead = seen_at(&mut world, 900.0, 0.0, rule);
    let behind = seen_at(&mut world, -900.0, 0.0, rule);
    // The camera is travelling right, so the thing on the right is ahead of
    // it - off screen, but its turn has not come yet.
    let gone = despawn_offscreen(&mut world, VIEW, glam::Vec2::new(12.0, 0.0));
    assert_eq!(gone, vec![behind], "only what the camera has passed");
    let _ = ahead;

    // Turn the camera round and the other one is the one behind.
    let gone = despawn_offscreen(&mut world, VIEW, glam::Vec2::new(-12.0, 0.0));
    assert_eq!(gone, vec![ahead]);
}

#[test]
fn a_camera_that_is_not_moving_has_no_behind() {
    // An endless runner spawns its next chunk ahead of the view. This used
    // to despawn everything outside on a still frame - the ready screen, a
    // pause - so an endless runner's hero ran on tiles its package collided with
    // and nobody drew.
    let mut world = World::new();
    let rule = DespawnOffscreen { margin: 100.0, behind_only: true, ..Default::default() };
    seen_at(&mut world, 900.0, 0.0, rule);
    seen_at(&mut world, -900.0, 0.0, rule);
    let gone = despawn_offscreen(&mut world, VIEW, STILL);
    assert!(gone.is_empty(), "with no travel nothing is behind, so both wait: {gone:?}");

    // `behind_only: false` is unaffected: outside is outside.
    let mut world = World::new();
    at(&mut world, 900.0, 0.0, DespawnOffscreen { margin: 100.0, behind_only: false, ..Default::default() });
    assert_eq!(despawn_offscreen(&mut world, VIEW, STILL).len(), 1);
}

#[test]
fn a_camera_bobbing_up_does_not_leave_the_road_ahead_behind() {
    // A runner jumps and the camera rises with it faster than it scrolls.
    // One dot product over the whole travel called a tile far ahead and a
    // little below "behind"; per axis, it is ahead in x and on screen in y.
    let mut world = World::new();
    let rule = DespawnOffscreen { margin: 100.0, behind_only: true, ..Default::default() };
    let ahead = seen_at(&mut world, 900.0, -176.0, rule);
    let passed = seen_at(&mut world, -900.0, -176.0, rule);
    let gone = despawn_offscreen(&mut world, VIEW, glam::Vec2::new(2.0, 30.0));
    assert_eq!(gone, vec![passed], "only what the camera has passed");
    let _ = ahead;

    // Outside in y, on the side the camera is leaving: that is behind.
    let mut world = World::new();
    let below = seen_at(&mut world, 0.0, -900.0, rule);
    assert_eq!(despawn_offscreen(&mut world, VIEW, glam::Vec2::new(0.0, 30.0)), vec![below]);
}

#[test]
fn an_entity_without_the_component_is_never_touched() {
    let mut world = World::new();
    world.spawn((Transform { position: glam::Vec2::new(9000.0, 9000.0), ..Default::default() },));
    assert!(despawn_offscreen(&mut world, VIEW, STILL).is_empty());
}

#[test]
fn a_turned_camera_sees_further_and_keeps_more() {
    // `half_extents` grows when the camera is turned, so the same corner is
    // inside the view at 45 degrees and outside it at zero.
    let mut world = World::new();
    let corner = at(&mut world, 640.0, 360.0, DespawnOffscreen { margin: 0.0, behind_only: false, ..Default::default() });
    let mut camera = jengine_core::renderer::camera::Camera2D::new(1280.0, 720.0);
    let straight = (camera.position, camera.half_extents());
    camera.rotation = std::f32::consts::FRAC_PI_4;
    let turned = (camera.position, camera.half_extents());

    assert!(despawn_offscreen(&mut world, straight, STILL).is_empty(), "exactly on the edge is still in");
    assert!(despawn_offscreen(&mut world, turned, STILL).is_empty(), "and a turned view reaches further");
    // Move it out of even the turned box.
    world.get::<&mut Transform>(corner).unwrap().position = glam::Vec2::new(1400.0, 0.0);
    assert_eq!(despawn_offscreen(&mut world, turned, STILL).len(), 1);
}

#[test]
fn a_camera_that_cuts_has_not_travelled() {
    // A runner restarting: the camera jumps thousands of units back to the
    // start in one frame, and the new run's next chunk is spawned just off
    // screen ahead on that same frame. Read as travel, the jump made that
    // chunk "behind" and deleted it - a runner's invisible ground at 25 m.
    let mut world = World::new();
    let rule = DespawnOffscreen { margin: 100.0, behind_only: true, ..Default::default() };
    let next_chunk = seen_at(&mut world, 900.0, 0.0, rule);
    let gone = despawn_offscreen(&mut world, VIEW, glam::Vec2::new(-4800.0, 0.0));
    assert!(gone.is_empty(), "a cut leaves nothing behind: {gone:?}");
    let _ = next_chunk;

    // A fast camera that is still travelling (under a view's width a frame)
    // keeps its behind.
    let mut world = World::new();
    let passed = seen_at(&mut world, -900.0, 0.0, rule);
    assert_eq!(despawn_offscreen(&mut world, VIEW, glam::Vec2::new(600.0, 0.0)), vec![passed]);
}

#[test]
fn what_the_camera_has_not_reached_cannot_be_behind_it() {
    // A runner's first run: the attract run behind the menu has only gone
    // a few hundred units, so starting for real jumps the camera back by
    // less than a view - not a cut, just travel to the left - and the new
    // run's next chunk, spawned just off screen right on that frame, sat on
    // the trailing side. It had never been on screen, so it cannot have
    // been passed.
    let mut world = World::new();
    let rule = DespawnOffscreen { margin: 100.0, behind_only: true, ..Default::default() };
    let next_chunk = at(&mut world, 900.0, 0.0, rule);
    assert!(despawn_offscreen(&mut world, VIEW, glam::Vec2::new(-960.0, 0.0)).is_empty(), "never seen, so kept");

    // Once the camera has reached it, it can be passed like anything else.
    world.get::<&mut Transform>(next_chunk).unwrap().position.x = 0.0;
    assert!(despawn_offscreen(&mut world, VIEW, glam::Vec2::new(12.0, 0.0)).is_empty(), "on screen now");
    assert!(world.get::<&DespawnOffscreen>(next_chunk).unwrap().seen, "and recorded as reached");
    world.get::<&mut Transform>(next_chunk).unwrap().position.x = -900.0;
    assert_eq!(despawn_offscreen(&mut world, VIEW, glam::Vec2::new(12.0, 0.0)), vec![next_chunk]);
}
