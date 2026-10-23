//! Step 2D — unit tests for the broader native-behavior systems.
//!
//! Coverage: Acceleration2D, MaxSpeed, Oscillator, FollowTarget,
//! SeekTarget, Tween (all `TweenTarget` variants + `TweenComplete`
//! modes), PatrolPathRef (Loop / PingPong / Once + missing path).
//!
//! Each test isolates one behavior so regressions surface against the
//! responsible system. Composition with other primitives is covered by
//! the dedicated `tween_runs_alongside_velocity` and
//! `seek_then_max_speed_compose` tests at the bottom.

use hecs::World;
use jengine_core::ecs::components::{
    Acceleration2D, Ease, EntityRef, FollowTarget, MaxSpeed, Oscillator, PatrolMode,
    PatrolPathRef, SeekTarget, SpriteComponent, Transform, Tween, TweenComplete, TweenTarget,
    Velocity2D,
};
use jengine_core::ecs::path::{Path2D, Path2DLibrary};
use jengine_core::ecs::systems;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

// ── Acceleration2D ─────────────────────────────────────────────────

#[test]
fn acceleration_adds_to_velocity() {
    let mut world = World::new();
    let e = world.spawn((
        Velocity2D { x: 0.0, y: 0.0, gravity: 0.0 },
        Acceleration2D { x: 10.0, y: -5.0 },
    ));
    systems::update_acceleration(&mut world, 0.5);
    let v = world.get::<&Velocity2D>(e).unwrap();
    assert!(approx(v.x, 5.0));
    assert!(approx(v.y, -2.5));
}

#[test]
fn acceleration_skips_entities_without_velocity() {
    let mut world = World::new();
    // Acceleration without Velocity2D is a no-op (no panic, no autoadd).
    let _e = world.spawn((Acceleration2D { x: 100.0, y: 0.0 },));
    systems::update_acceleration(&mut world, 1.0);
}

// ── MaxSpeed ───────────────────────────────────────────────────────

#[test]
fn max_speed_clamps_overspeed() {
    let mut world = World::new();
    let e = world.spawn((
        Velocity2D { x: 30.0, y: 40.0, gravity: 0.0 }, // mag = 50
        MaxSpeed { value: 25.0 },
    ));
    systems::update_max_speed(&mut world);
    let v = world.get::<&Velocity2D>(e).unwrap();
    let mag = (v.x * v.x + v.y * v.y).sqrt();
    assert!(approx(mag, 25.0), "mag = {}", mag);
    // Direction preserved.
    assert!(approx(v.x / mag, 0.6));
    assert!(approx(v.y / mag, 0.8));
}

#[test]
fn max_speed_leaves_underspeed_alone() {
    let mut world = World::new();
    let e = world.spawn((
        Velocity2D { x: 3.0, y: 4.0, gravity: 0.0 }, // mag = 5
        MaxSpeed { value: 100.0 },
    ));
    systems::update_max_speed(&mut world);
    let v = world.get::<&Velocity2D>(e).unwrap();
    assert!(approx(v.x, 3.0));
    assert!(approx(v.y, 4.0));
}

#[test]
fn max_speed_handles_zero_velocity() {
    let mut world = World::new();
    let e = world.spawn((
        Velocity2D::default(),
        MaxSpeed { value: 10.0 },
    ));
    systems::update_max_speed(&mut world);
    let v = world.get::<&Velocity2D>(e).unwrap();
    assert!(approx(v.x, 0.0));
    assert!(approx(v.y, 0.0));
}

// ── Oscillator ─────────────────────────────────────────────────────

#[test]
fn oscillator_captures_center_on_first_tick() {
    let mut world = World::new();
    let mut tr = Transform::default();
    tr.position = glam::Vec2::new(50.0, 60.0);
    let e = world.spawn((
        tr,
        Oscillator {
            amplitude: [10.0, 0.0],
            frequency: [1.0, 1.0],
            phase: [0.0, 0.0],
            elapsed: 0.0,
            center: None,
        },
    ));
    systems::update_oscillators(&mut world, 0.0);
    let osc = world.get::<&Oscillator>(e).unwrap();
    assert_eq!(osc.center, Some([50.0, 60.0]));
}

#[test]
fn oscillator_drives_position_with_sin() {
    use std::f32::consts::TAU;
    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        Oscillator {
            amplitude: [10.0, 5.0],
            frequency: [1.0, 2.0],
            phase: [0.0, 0.0],
            elapsed: 0.0,
            center: Some([0.0, 0.0]),
        },
    ));
    // Step exactly one quarter-period for axis x (freq 1Hz → 0.25s = π/2)
    systems::update_oscillators(&mut world, 0.25);
    let tr = world.get::<&Transform>(e).unwrap();
    assert!(approx(tr.position.x, 10.0), "x = {}", tr.position.x); // sin(π/2) = 1
    // y at t=0.25, freq=2 → sin(π) = 0
    assert!((tr.position.y).abs() < 1e-3, "y = {}", tr.position.y);
    // Sanity: TAU is what the system uses.
    let _ = TAU;
}

// ── FollowTarget ───────────────────────────────────────────────────

#[test]
fn follow_target_moves_toward_target() {
    let mut world = World::new();
    let target = world.spawn((Transform {
        position: glam::Vec2::new(100.0, 0.0),
        ..Default::default()
    },));
    let follower = world.spawn((
        Transform::default(),
        FollowTarget {
            target: EntityRef::Runtime(target),
            offset: [0.0, 0.0],
            max_speed: 50.0,
            stop_distance: 0.0,
        },
    ));
    systems::update_follow_targets(&mut world, 1.0); // moves 50 units
    let tr = world.get::<&Transform>(follower).unwrap();
    assert!(approx(tr.position.x, 50.0), "x = {}", tr.position.x);
    assert!(approx(tr.position.y, 0.0));
}

#[test]
fn follow_target_respects_stop_distance() {
    let mut world = World::new();
    let target = world.spawn((Transform {
        position: glam::Vec2::new(20.0, 0.0),
        ..Default::default()
    },));
    let follower = world.spawn((
        Transform {
            position: glam::Vec2::new(15.0, 0.0),
            ..Default::default()
        },
        FollowTarget {
            target: EntityRef::Runtime(target),
            offset: [0.0, 0.0],
            max_speed: 100.0,
            stop_distance: 5.0, // exactly at stop distance
        },
    ));
    systems::update_follow_targets(&mut world, 1.0);
    let tr = world.get::<&Transform>(follower).unwrap();
    // No move (already at stop distance).
    assert!(approx(tr.position.x, 15.0), "x = {}", tr.position.x);
}

#[test]
fn follow_target_skips_unresolvable_target() {
    let mut world = World::new();
    // Target entity intentionally not spawned — use SelfEntity which is None
    // because we don't pass self_entity in the FollowTarget path; it uses
    // the entity itself, which would create a self-loop and be skipped.
    let real_target = world.spawn((Transform::default(),));
    world.despawn(real_target).unwrap(); // dangling Entity reference

    let follower = world.spawn((
        Transform::default(),
        FollowTarget {
            target: EntityRef::Runtime(real_target),
            offset: [0.0, 0.0],
            max_speed: 100.0,
            stop_distance: 0.0,
        },
    ));
    // Should not panic; follower stays put.
    systems::update_follow_targets(&mut world, 1.0);
    let tr = world.get::<&Transform>(follower).unwrap();
    assert!(approx(tr.position.x, 0.0));
}

// ── SeekTarget ─────────────────────────────────────────────────────

#[test]
fn seek_target_accelerates_toward_target() {
    let mut world = World::new();
    let target = world.spawn((Transform {
        position: glam::Vec2::new(100.0, 0.0),
        ..Default::default()
    },));
    let seeker = world.spawn((
        Transform::default(),
        Velocity2D::default(),
        SeekTarget {
            target: EntityRef::Runtime(target),
            accel: 200.0,
            max_speed: 1000.0,
            slowing_radius: 0.0,
        },
    ));
    systems::update_seek_targets(&mut world, 0.1);
    let v = world.get::<&Velocity2D>(seeker).unwrap();
    // dv = unit_dir * accel * dt = (1,0) * 200 * 0.1 = (20, 0)
    assert!(approx(v.x, 20.0), "vx = {}", v.x);
    assert!(approx(v.y, 0.0));
}

#[test]
fn seek_target_clamps_to_own_max_speed() {
    let mut world = World::new();
    let target = world.spawn((Transform {
        position: glam::Vec2::new(1000.0, 0.0),
        ..Default::default()
    },));
    let seeker = world.spawn((
        Transform::default(),
        Velocity2D { x: 50.0, y: 0.0, gravity: 0.0 },
        SeekTarget {
            target: EntityRef::Runtime(target),
            accel: 10000.0, // would blow past max
            max_speed: 60.0,
            slowing_radius: 0.0,
        },
    ));
    systems::update_seek_targets(&mut world, 0.1);
    let v = world.get::<&Velocity2D>(seeker).unwrap();
    let mag = (v.x * v.x + v.y * v.y).sqrt();
    assert!(approx(mag, 60.0), "mag = {}", mag);
}

// ── Tween ──────────────────────────────────────────────────────────

#[test]
fn tween_drives_position_x_linearly() {
    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        Tween {
            target: TweenTarget::PositionX,
            from: 0.0,
            to: 100.0,
            duration: 1.0,
            elapsed: 0.0,
            ease: Ease::Linear,
            on_complete: TweenComplete::Stop,
        },
    ));
    systems::update_tweens(&mut world, 0.5); // halfway
    let tr = world.get::<&Transform>(e).unwrap();
    assert!(approx(tr.position.x, 50.0), "x = {}", tr.position.x);
}

#[test]
fn tween_opacity_writes_sprite_alpha() {
    let mut world = World::new();
    let e = world.spawn((
        SpriteComponent {
            color: [1.0, 1.0, 1.0, 1.0],
            ..Default::default()
        },
        Tween {
            target: TweenTarget::Opacity,
            from: 1.0,
            to: 0.0,
            duration: 1.0,
            elapsed: 0.0,
            ease: Ease::Linear,
            on_complete: TweenComplete::Stop,
        },
    ));
    systems::update_tweens(&mut world, 1.0); // full duration → snap to 0
    let s = world.get::<&SpriteComponent>(e).unwrap();
    assert!(approx(s.color[3], 0.0), "alpha = {}", s.color[3]);
}

#[test]
fn tween_loop_resets_elapsed() {
    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        Tween {
            target: TweenTarget::Rotation,
            from: 0.0,
            to: 1.0,
            duration: 1.0,
            elapsed: 0.0,
            ease: Ease::Linear,
            on_complete: TweenComplete::Loop,
        },
    ));
    systems::update_tweens(&mut world, 1.5); // overshoots → loop resets
    let t = world.get::<&Tween>(e).unwrap();
    assert!(t.elapsed < 1.0, "should have looped, elapsed = {}", t.elapsed);
}

#[test]
fn tween_pingpong_swaps_endpoints() {
    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        Tween {
            target: TweenTarget::ScaleX,
            from: 1.0,
            to: 2.0,
            duration: 1.0,
            elapsed: 0.0,
            ease: Ease::Linear,
            on_complete: TweenComplete::PingPong,
        },
    ));
    systems::update_tweens(&mut world, 1.0); // hits end → ping-pong
    let t = world.get::<&Tween>(e).unwrap();
    assert!(approx(t.from, 2.0), "from = {}", t.from);
    assert!(approx(t.to, 1.0), "to = {}", t.to);
}

#[test]
fn tween_eased_curve_is_not_linear() {
    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        Tween {
            target: TweenTarget::PositionY,
            from: 0.0,
            to: 100.0,
            duration: 1.0,
            elapsed: 0.0,
            ease: Ease::EaseInQuad,
            on_complete: TweenComplete::Stop,
        },
    ));
    systems::update_tweens(&mut world, 0.5);
    let tr = world.get::<&Transform>(e).unwrap();
    // EaseInQuad at t=0.5 → 0.25 → y = 25
    assert!(approx(tr.position.y, 25.0), "y = {}", tr.position.y);
}

// ── PatrolPathRef ──────────────────────────────────────────────────

fn three_point_lib() -> Path2DLibrary {
    let mut lib = Path2DLibrary::new();
    lib.insert(
        "p",
        Path2D::new([
            glam::Vec2::new(0.0, 0.0),
            glam::Vec2::new(100.0, 0.0),
            glam::Vec2::new(100.0, 100.0),
        ]),
    );
    lib
}

#[test]
fn patrol_walks_toward_first_waypoint() {
    let mut world = World::new();
    let lib = three_point_lib();
    let e = world.spawn((
        Transform::default(),
        PatrolPathRef {
            path_id: "p".into(),
            speed: 50.0,
            index: 1,
            mode: PatrolMode::Loop,
            forward: true,
        },
    ));
    systems::update_patrol_paths(&mut world, Some(&lib), 1.0); // 50 units toward (100,0)
    let tr = world.get::<&Transform>(e).unwrap();
    assert!(approx(tr.position.x, 50.0), "x = {}", tr.position.x);
}

#[test]
fn patrol_advances_index_on_loop() {
    let mut world = World::new();
    let lib = three_point_lib();
    let e = world.spawn((
        Transform::default(),
        PatrolPathRef {
            path_id: "p".into(),
            speed: 1000.0, // big enough to overshoot in one tick
            index: 1,
            mode: PatrolMode::Loop,
            forward: true,
        },
    ));
    systems::update_patrol_paths(&mut world, Some(&lib), 1.0);
    let p = world.get::<&PatrolPathRef>(e).unwrap();
    assert_eq!(p.index, 2);
}

#[test]
fn patrol_loop_wraps_at_end() {
    let mut world = World::new();
    let lib = three_point_lib();
    let e = world.spawn((
        Transform {
            position: glam::Vec2::new(100.0, 100.0),
            ..Default::default()
        },
        PatrolPathRef {
            path_id: "p".into(),
            speed: 1.0,
            index: 2,
            mode: PatrolMode::Loop,
            forward: true,
        },
    ));
    // Already at waypoint 2 → snaps + advances → wraps to 0.
    systems::update_patrol_paths(&mut world, Some(&lib), 0.001);
    let p = world.get::<&PatrolPathRef>(e).unwrap();
    assert_eq!(p.index, 0);
}

#[test]
fn patrol_pingpong_reverses_at_end() {
    let mut world = World::new();
    let lib = three_point_lib();
    let e = world.spawn((
        Transform {
            position: glam::Vec2::new(100.0, 100.0),
            ..Default::default()
        },
        PatrolPathRef {
            path_id: "p".into(),
            speed: 1.0,
            index: 2,
            mode: PatrolMode::PingPong,
            forward: true,
        },
    ));
    systems::update_patrol_paths(&mut world, Some(&lib), 0.001);
    let p = world.get::<&PatrolPathRef>(e).unwrap();
    assert!(!p.forward, "should have flipped to reverse");
    assert_eq!(p.index, 1);
}

#[test]
fn patrol_once_clamps_at_end() {
    let mut world = World::new();
    let lib = three_point_lib();
    let e = world.spawn((
        Transform {
            position: glam::Vec2::new(100.0, 100.0),
            ..Default::default()
        },
        PatrolPathRef {
            path_id: "p".into(),
            speed: 1.0,
            index: 2,
            mode: PatrolMode::Once,
            forward: true,
        },
    ));
    systems::update_patrol_paths(&mut world, Some(&lib), 0.001);
    let p = world.get::<&PatrolPathRef>(e).unwrap();
    assert_eq!(p.index, 2, "Once mode should clamp at last waypoint");
}

#[test]
fn patrol_with_no_library_is_noop() {
    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        PatrolPathRef {
            path_id: "missing".into(),
            speed: 100.0,
            index: 0,
            mode: PatrolMode::Loop,
            forward: true,
        },
    ));
    systems::update_patrol_paths(&mut world, None, 1.0);
    let tr = world.get::<&Transform>(e).unwrap();
    assert!(approx(tr.position.x, 0.0));
}

#[test]
fn patrol_with_missing_path_id_is_noop() {
    let mut world = World::new();
    let lib = three_point_lib();
    let e = world.spawn((
        Transform::default(),
        PatrolPathRef {
            path_id: "does-not-exist".into(),
            speed: 100.0,
            index: 0,
            mode: PatrolMode::Loop,
            forward: true,
        },
    ));
    systems::update_patrol_paths(&mut world, Some(&lib), 1.0);
    let tr = world.get::<&Transform>(e).unwrap();
    assert!(approx(tr.position.x, 0.0));
}

// ── Composition ────────────────────────────────────────────────────

#[test]
fn seek_then_max_speed_compose() {
    // SeekTarget pushes velocity past its own cap; an external MaxSpeed
    // component clamps it further. Run order matters: seek → max_speed.
    let mut world = World::new();
    let target = world.spawn((Transform {
        position: glam::Vec2::new(1000.0, 0.0),
        ..Default::default()
    },));
    let seeker = world.spawn((
        Transform::default(),
        Velocity2D::default(),
        SeekTarget {
            target: EntityRef::Runtime(target),
            accel: 1000.0,
            max_speed: 200.0,
            slowing_radius: 0.0,
        },
        MaxSpeed { value: 50.0 }, // tighter cap than seek's own
    ));
    systems::update_seek_targets(&mut world, 0.1);
    systems::update_max_speed(&mut world);
    let v = world.get::<&Velocity2D>(seeker).unwrap();
    let mag = (v.x * v.x + v.y * v.y).sqrt();
    assert!(approx(mag, 50.0), "expected 50, got {}", mag);
}

#[test]
fn tween_runs_alongside_velocity() {
    // Tween writes Transform.rotation while velocity_system writes
    // Transform.position. They should not interfere.
    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        Velocity2D { x: 100.0, y: 0.0, gravity: 0.0 },
        Tween {
            target: TweenTarget::Rotation,
            from: 0.0,
            to: 1.0,
            duration: 1.0,
            elapsed: 0.0,
            ease: Ease::Linear,
            on_complete: TweenComplete::Stop,
        },
    ));
    systems::velocity_system(&mut world, 0.5);
    systems::update_tweens(&mut world, 0.5);
    let tr = world.get::<&Transform>(e).unwrap();
    assert!(approx(tr.position.x, 50.0), "x = {}", tr.position.x);
    assert!(approx(tr.rotation, 0.5), "rot = {}", tr.rotation);
}
