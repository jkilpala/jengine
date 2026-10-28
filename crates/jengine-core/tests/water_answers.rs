//! **Water that answers** (Oct 2026; `ecs::water`): a `water_surface` is a
//! row of springs that a push runs out along as waves, that what falls in
//! splashes, and that what floats rides - its surface is the top of the
//! area effect that floats things, so a boat rises on a crest and drops in a
//! trough. Found by the showcase's Harbour Storm, whose water had been a
//! flat box of buoyancy under a painted sea.
#![cfg(feature = "water")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::area_effect::{placed_areas, push};
use jengine_core::ecs::components::{Collider2D, ColliderShape, Transform, Velocity2D};
use jengine_core::ecs::water::{WaterSurface, set_water_field, splash_world, tick_water};

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

fn water(world: &mut World) -> hecs::Entity {
    world.spawn((at(0.0, 0.0), WaterSurface { width: 400.0, depth: 200.0, spacing: 8.0, ..Default::default() }))
}

fn heights(world: &World, e: hecs::Entity) -> Vec<f32> {
    world.get::<&WaterSurface>(e).unwrap().heights.clone()
}

#[test]
fn a_push_runs_out_as_waves_and_dies_away() {
    let mut world = World::new();
    let w = water(&mut world);
    tick_water(&mut world, 1.0 / 60.0);
    splash_world(&world, w, 0.0, -12.0).unwrap();
    for _ in 0..18 {
        tick_water(&mut world, 1.0 / 60.0);
    }
    let h = heights(&world, w);
    let mid = h.len() / 2;
    let away = h[mid + 10].abs().max(h[mid - 10].abs());
    assert!(away > 0.3, "a wave has run 80 units out from the push in 0.3 s: {away}");
    assert!((h[mid + 10] - h[mid - 10]).abs() < 0.05, "and both ways alike: {} vs {}", h[mid + 10], h[mid - 10]);
    for _ in 0..1800 {
        tick_water(&mut world, 1.0 / 60.0);
    }
    let left = heights(&world, w).iter().fold(0.0f32, |m, h| m.max(h.abs()));
    assert!(left < 0.2, "thirty seconds on, it has settled: {left}");
}

#[test]
fn the_springs_step_the_same_at_any_frame_rate() {
    let mut fast = World::new();
    let mut slow = World::new();
    let (a, b) = (water(&mut fast), water(&mut slow));
    tick_water(&mut fast, 1.0 / 60.0);
    tick_water(&mut slow, 1.0 / 60.0);
    splash_world(&fast, a, 40.0, -10.0).unwrap();
    splash_world(&slow, b, 40.0, -10.0).unwrap();
    for _ in 0..60 {
        tick_water(&mut fast, 1.0 / 60.0);
    }
    for _ in 0..20 {
        tick_water(&mut slow, 1.0 / 20.0);
    }
    let (ha, hb) = (heights(&fast, a), heights(&slow, b));
    let worst = ha.iter().zip(&hb).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
    assert!(worst < 1e-3, "a second at 60 fps and at 20 fps leaves the same surface: {worst}");
}

#[test]
fn a_body_falling_in_splashes() {
    let mut world = World::new();
    let w = water(&mut world);
    tick_water(&mut world, 1.0 / 60.0);
    world.spawn((
        at(-100.0, 2.0),
        Velocity2D { x: 0.0, y: -500.0, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 30.0, height: 30.0 }, ..Default::default() },
    ));
    for _ in 0..6 {
        tick_water(&mut world, 1.0 / 60.0);
    }
    let water = world.get::<&WaterSurface>(w).unwrap();
    let under = water.height_at(-100.0);
    let far = water.height_at(150.0);
    assert!(under < -0.5, "the surface under it is pushed down: {under}");
    assert!(far.abs() < 0.05, "and not yet far away: {far}");
}

#[test]
fn what_floats_rides_the_surface() {
    // A swell 20 high and 400 long: a crest at x = 100, a trough at -100.
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), WaterSurface {
        width: 800.0,
        depth: 200.0,
        swell_height: 20.0,
        swell_length: 400.0,
        swell_speed: 0.0,
        buoyancy: 1.5,
        ..Default::default()
    }));
    let areas = placed_areas(&world);
    assert_eq!(areas.len(), 1, "the water floats things as an area effect");
    let gravity = Vec2::new(0.0, -981.0);
    let hull = |x: f32| [Vec2::new(x - 20.0, -15.0), Vec2::new(x + 20.0, -15.0), Vec2::new(x + 20.0, 5.0), Vec2::new(x - 20.0, 5.0)];
    let crest = push(&areas[0], &hull(100.0), Vec2::new(100.0, -5.0), gravity).unwrap();
    let trough = push(&areas[0], &hull(-100.0), Vec2::new(-100.0, -5.0), gravity);
    assert!(crest.lift.y > 0.0, "on a crest it is wholly under and lifted");
    assert!(
        trough.is_none_or(|t| t.lift.y < crest.lift.y * 0.25),
        "in a trough it is barely wet: {:?} vs {:?}", trough, crest
    );
    // On the slope between, the wet part leans downhill.
    let slope = push(&areas[0], &hull(0.0), Vec2::new(0.0, -5.0), gravity).unwrap();
    assert!(slope.lift_at.x > 0.0, "the deeper side (towards the crest) lifts more: {}", slope.lift_at.x);
}

#[test]
fn a_script_changes_the_sea_without_stilling_it() {
    let mut world = World::new();
    let w = water(&mut world);
    tick_water(&mut world, 1.0 / 60.0);
    splash_world(&world, w, 0.0, -8.0).unwrap();
    tick_water(&mut world, 0.1);
    let before = heights(&world, w);
    set_water_field(&world, w, "chop", &serde_json::json!(3.0)).unwrap();
    set_water_field(&world, w, "z_order", &serde_json::json!(5.0)).unwrap();
    let water = world.get::<&WaterSurface>(w).unwrap();
    assert_eq!(water.chop, 3.0);
    assert_eq!(water.z_order, 5);
    assert_eq!(water.heights, before, "the surface keeps moving through a change");
    drop(water);
    assert!(set_water_field(&world, w, "tide", &serde_json::json!(1)).is_err());
}

#[test]
fn a_body_with_no_velocity_splashes_by_how_it_moves() {
    // Physics writes a `velocity` only where there is one, so a falling
    // body often has none: the water reads its speed from where it was.
    let mut world = World::new();
    let w = water(&mut world);
    let block = world.spawn((
        at(60.0, 20.0),
        Collider2D { shape: ColliderShape::Box { width: 30.0, height: 30.0 }, ..Default::default() },
    ));
    tick_water(&mut world, 1.0 / 60.0);
    for i in 1..=5 {
        world.get::<&mut Transform>(block).unwrap().position.y = 20.0 - 9.0 * i as f32;
        tick_water(&mut world, 1.0 / 60.0);
    }
    let under = world.get::<&WaterSurface>(w).unwrap().height_at(60.0);
    assert!(under < -0.5, "falling in at 540 units a second, it pushed the surface down: {under}");
}
