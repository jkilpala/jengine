//! Step 6 — unit tests for the new native-behavior vocabulary:
//!
//! - 6a: `InputAxis2D`, `InputActionImpulse` driven by `InputState`
//! - 6b: `TriggerArea2D` → `TriggerOverlaps` enter/staying/exit diff
//! - 6d: `SeekTarget.slowing_radius` (Arrival)

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{
    EntityRef, InputActionImpulse, InputAxis2D, SeekTarget, Tags, Transform, TriggerArea2D,
    TriggerOverlaps, TriggerShape, Velocity2D,
};
use jengine_core::ecs::components::{
    AngularVelocity, ClampBounds, DespawnOutOfBounds, FaceVelocity, Flicker, Shake,
    SpriteComponent, WrapBounds,
};
use jengine_core::ecs::systems;
use jengine_core::input::{InputAction, InputState, KeyCode};

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn input_with_actions() -> InputState {
    let mut input = InputState::new();
    input.load_actions(&[
        InputAction { name: "move_left".into(),  keys: vec!["A".into()], dead_zone: 0.0 },
        InputAction { name: "move_right".into(), keys: vec!["D".into()], dead_zone: 0.0 },
        InputAction { name: "move_up".into(),    keys: vec!["W".into()], dead_zone: 0.0 },
        InputAction { name: "move_down".into(),  keys: vec!["S".into()], dead_zone: 0.0 },
        InputAction { name: "jump".into(),       keys: vec!["Space".into()], dead_zone: 0.0 },
    ]);
    input
}

// ── 6a: InputAxis2D ────────────────────────────────────────────────

#[test]
fn input_axis_zero_when_no_keys_pressed() {
    let mut world = World::new();
    let mut input = input_with_actions();
    let e = world.spawn((InputAxis2D {
        x_neg: "move_left".into(),
        x_pos: "move_right".into(),
        y_neg: "move_down".into(),
        y_pos: "move_up".into(),
        scale: 1.0,
        drive: jengine_core::ecs::components::AxisDrive::None,
        normalize: false,
        current: Vec2::ZERO,
    },));
    input.begin_frame();
    systems::update_input_actions(&mut world, &input);
    let a = world.get::<&InputAxis2D>(e).unwrap();
    assert_eq!(a.current, Vec2::ZERO);
}

#[test]
fn input_axis_reads_held_keys_with_scale() {
    let mut world = World::new();
    let mut input = input_with_actions();
    input.begin_frame();
    input.key_down(KeyCode::D);
    input.key_down(KeyCode::W);

    let e = world.spawn((InputAxis2D {
        x_neg: "move_left".into(),
        x_pos: "move_right".into(),
        y_neg: "move_down".into(),
        y_pos: "move_up".into(),
        scale: 200.0,
        drive: jengine_core::ecs::components::AxisDrive::None,
        normalize: false,
        current: Vec2::ZERO,
    },));
    systems::update_input_actions(&mut world, &input);
    let a = world.get::<&InputAxis2D>(e).unwrap();
    assert!(approx(a.current.x, 200.0));
    assert!(approx(a.current.y, 200.0));
}

#[test]
fn input_axis_normalize_caps_diagonal_at_unit() {
    let mut world = World::new();
    let mut input = input_with_actions();
    input.begin_frame();
    input.key_down(KeyCode::D);
    input.key_down(KeyCode::W);

    let e = world.spawn((InputAxis2D {
        x_neg: String::new(),
        x_pos: "move_right".into(),
        y_neg: String::new(),
        y_pos: "move_up".into(),
        scale: 100.0,
        drive: jengine_core::ecs::components::AxisDrive::None,
        normalize: true,
        current: Vec2::ZERO,
    },));
    systems::update_input_actions(&mut world, &input);
    let a = world.get::<&InputAxis2D>(e).unwrap();
    let mag = a.current.length();
    // Without normalize this would be sqrt(2)*100 ≈ 141.4
    assert!(approx(mag, 100.0), "mag = {}", mag);
}

#[test]
fn input_axis_opposing_keys_cancel() {
    let mut world = World::new();
    let mut input = input_with_actions();
    input.begin_frame();
    input.key_down(KeyCode::A);
    input.key_down(KeyCode::D);

    let e = world.spawn((InputAxis2D {
        x_neg: "move_left".into(),
        x_pos: "move_right".into(),
        y_neg: String::new(),
        y_pos: String::new(),
        scale: 50.0,
        drive: jengine_core::ecs::components::AxisDrive::None,
        normalize: false,
        current: Vec2::ZERO,
    },));
    systems::update_input_actions(&mut world, &input);
    let a = world.get::<&InputAxis2D>(e).unwrap();
    assert!(approx(a.current.x, 0.0));
}

// ── 6a: InputActionImpulse ─────────────────────────────────────────

#[test]
fn impulse_writes_velocity_only_on_just_pressed_edge() {
    let mut world = World::new();
    let mut input = input_with_actions();

    let e = world.spawn((
        Velocity2D { x: 0.0, y: 0.0, gravity: 0.0 },
        InputActionImpulse {
            action: "jump".into(),
            impulse_x: None,
            impulse_y: Some(300.0),
        },
    ));

    // Frame 1: just-pressed → impulse fires.
    input.begin_frame();
    input.key_down(KeyCode::Space);
    systems::update_input_actions(&mut world, &input);
    {
        let v = world.get::<&Velocity2D>(e).unwrap();
        assert!(approx(v.x, 0.0)); // x untouched (None)
        assert!(approx(v.y, 300.0));
    }

    // Frame 2: still held but no longer just-pressed → no overwrite.
    // Drain velocity to verify the system didn't re-write it.
    {
        let mut v = world.get::<&mut Velocity2D>(e).unwrap();
        v.y = 0.0;
    }
    input.begin_frame(); // clears just_pressed
    systems::update_input_actions(&mut world, &input);
    let v = world.get::<&Velocity2D>(e).unwrap();
    assert!(approx(v.y, 0.0), "impulse re-fired while key held: y = {}", v.y);
}

#[test]
fn impulse_skipped_when_velocity_missing() {
    let mut world = World::new();
    let mut input = input_with_actions();
    // No Velocity2D — system must skip silently.
    let _e = world.spawn((InputActionImpulse {
        action: "jump".into(),
        impulse_x: Some(1.0),
        impulse_y: Some(1.0),
    },));
    input.begin_frame();
    input.key_down(KeyCode::Space);
    systems::update_input_actions(&mut world, &input);
    // No panic = pass.
}

// ── 6b: TriggerArea2D ──────────────────────────────────────────────

#[test]
fn trigger_aabb_reports_inside_entity() {
    let mut world = World::new();
    let trigger = world.spawn((
        Transform { position: Vec2::new(0.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
        TriggerArea2D {
            shape: TriggerShape::Aabb { half_extents: [10.0, 10.0] },
            match_tags: Vec::new(),
        },
    ));
    let inside = world.spawn((
        Transform { position: Vec2::new(5.0, 5.0), rotation: 0.0, scale: Vec2::ONE },
    ));
    let _outside = world.spawn((
        Transform { position: Vec2::new(20.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
    ));

    systems::update_trigger_areas(&mut world);

    let overlaps = world.get::<&TriggerOverlaps>(trigger).unwrap();
    assert_eq!(overlaps.staying, vec![inside.to_bits().get()]);
    assert_eq!(overlaps.entered, vec![inside.to_bits().get()]);
    assert!(overlaps.exited.is_empty());
}

#[test]
fn trigger_circle_uses_radius() {
    let mut world = World::new();
    let trigger = world.spawn((
        Transform { position: Vec2::new(0.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
        TriggerArea2D {
            shape: TriggerShape::Circle { radius: 5.0 },
            match_tags: Vec::new(),
        },
    ));
    let inside = world.spawn((
        Transform { position: Vec2::new(3.0, 4.0), rotation: 0.0, scale: Vec2::ONE },
    )); // dist = 5, on boundary
    let _outside = world.spawn((
        Transform { position: Vec2::new(3.0, 5.0), rotation: 0.0, scale: Vec2::ONE },
    ));

    systems::update_trigger_areas(&mut world);
    let overlaps = world.get::<&TriggerOverlaps>(trigger).unwrap();
    assert_eq!(overlaps.staying, vec![inside.to_bits().get()]);
}

#[test]
fn trigger_emits_enter_exit_across_frames() {
    let mut world = World::new();
    let trigger = world.spawn((
        Transform { position: Vec2::new(0.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
        TriggerArea2D {
            shape: TriggerShape::Aabb { half_extents: [5.0, 5.0] },
            match_tags: Vec::new(),
        },
    ));
    let mover = world.spawn((
        Transform { position: Vec2::new(0.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
    ));

    // Frame 1: mover inside → entered.
    systems::update_trigger_areas(&mut world);
    {
        let o = world.get::<&TriggerOverlaps>(trigger).unwrap();
        assert_eq!(o.entered, vec![mover.to_bits().get()]);
        assert!(o.exited.is_empty());
    }

    // Frame 2: still inside → staying, no enter/exit.
    systems::update_trigger_areas(&mut world);
    {
        let o = world.get::<&TriggerOverlaps>(trigger).unwrap();
        assert!(o.entered.is_empty());
        assert!(o.exited.is_empty());
        assert_eq!(o.staying, vec![mover.to_bits().get()]);
    }

    // Frame 3: move out → exited.
    {
        let mut t = world.get::<&mut Transform>(mover).unwrap();
        t.position = Vec2::new(100.0, 0.0);
    }
    systems::update_trigger_areas(&mut world);
    let o = world.get::<&TriggerOverlaps>(trigger).unwrap();
    assert!(o.entered.is_empty());
    assert_eq!(o.exited, vec![mover.to_bits().get()]);
    assert!(o.staying.is_empty());
}

#[test]
fn trigger_match_tags_filters_candidates() {
    let mut world = World::new();
    let trigger = world.spawn((
        Transform { position: Vec2::new(0.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
        TriggerArea2D {
            shape: TriggerShape::Aabb { half_extents: [50.0, 50.0] },
            match_tags: vec!["pickup".into()],
        },
    ));
    // tagged candidate → matches
    let mut tags = std::collections::HashSet::new();
    tags.insert("pickup".to_string());
    let pickup = world.spawn((
        Transform { position: Vec2::new(1.0, 1.0), rotation: 0.0, scale: Vec2::ONE },
        Tags(tags),
    ));
    // untagged candidate inside the area → filtered out
    let _untagged = world.spawn((
        Transform { position: Vec2::new(2.0, 2.0), rotation: 0.0, scale: Vec2::ONE },
    ));

    systems::update_trigger_areas(&mut world);
    let o = world.get::<&TriggerOverlaps>(trigger).unwrap();
    assert_eq!(o.staying, vec![pickup.to_bits().get()]);
}

#[test]
fn trigger_does_not_overlap_itself() {
    let mut world = World::new();
    let trigger = world.spawn((
        Transform { position: Vec2::new(0.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
        TriggerArea2D {
            shape: TriggerShape::Aabb { half_extents: [10.0, 10.0] },
            match_tags: Vec::new(),
        },
    ));
    systems::update_trigger_areas(&mut world);
    let o = world.get::<&TriggerOverlaps>(trigger).unwrap();
    assert!(o.staying.is_empty());
}

// ── 6d: SeekTarget.slowing_radius (Arrival) ────────────────────────

#[test]
fn seek_without_slowing_radius_caps_at_max_speed_inside_range() {
    let mut world = World::new();
    let target = world.spawn((Transform {
        position: Vec2::new(10.0, 0.0), rotation: 0.0, scale: Vec2::ONE,
    },));
    let agent = world.spawn((
        Transform { position: Vec2::new(0.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
        Velocity2D { x: 0.0, y: 0.0, gravity: 0.0 },
        SeekTarget {
            target: EntityRef::Runtime(target),
            accel: 10000.0,
            max_speed: 100.0,
            slowing_radius: 0.0,
        },
    ));
    // After one tick, accel * dt = 10000 * 1.0 = 10000 px/s on x — must clamp to 100.
    systems::update_seek_targets(&mut world, 1.0);
    let v = world.get::<&Velocity2D>(agent).unwrap();
    assert!(approx(v.x, 100.0), "v.x = {}", v.x);
}

#[test]
fn seek_with_slowing_radius_scales_max_inside_radius() {
    let mut world = World::new();
    let target = world.spawn((Transform {
        position: Vec2::new(10.0, 0.0), rotation: 0.0, scale: Vec2::ONE,
    },));
    let agent = world.spawn((
        Transform { position: Vec2::new(0.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
        Velocity2D { x: 0.0, y: 0.0, gravity: 0.0 },
        SeekTarget {
            target: EntityRef::Runtime(target),
            accel: 10000.0,
            max_speed: 100.0,
            // Distance = 10, radius = 20 → t = 0.5 → effective_max = 50.
            slowing_radius: 20.0,
        },
    ));
    systems::update_seek_targets(&mut world, 1.0);
    let v = world.get::<&Velocity2D>(agent).unwrap();
    assert!(approx(v.x, 50.0), "expected arrival cap 50, got {}", v.x);
}

#[test]
fn seek_with_slowing_radius_zero_at_target() {
    let mut world = World::new();
    // Place target at agent position so distance = 0 → effective_max = 0.
    let target = world.spawn((Transform {
        position: Vec2::new(0.001, 0.0), rotation: 0.0, scale: Vec2::ONE,
    },));
    let agent = world.spawn((
        Transform { position: Vec2::new(0.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
        Velocity2D { x: 50.0, y: 0.0, gravity: 0.0 }, // existing momentum
        SeekTarget {
            target: EntityRef::Runtime(target),
            accel: 10.0,
            max_speed: 100.0,
            slowing_radius: 50.0,
        },
    ));
    systems::update_seek_targets(&mut world, 1.0);
    let v = world.get::<&Velocity2D>(agent).unwrap();
    // Distance ≈ 0.001, t ≈ 0 → effective_max ≈ 0 → velocity clamped near 0.
    let speed = (v.x * v.x + v.y * v.y).sqrt();
    assert!(speed < 1.0, "expected clamp near 0, got speed = {}", speed);
}

// ─────────────────────────────────────────────────────────────────────
// Step 6 reopen — broader native vocabulary
// ─────────────────────────────────────────────────────────────────────

fn t_at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::splat(1.0) }
}

#[test]
fn wrap_bounds_wraps_position_across_max() {
    let mut world = World::new();
    let e = world.spawn((
        t_at(105.0, -120.0),
        WrapBounds { min: [-100.0, -100.0], max: [100.0, 100.0] },
    ));
    systems::update_wrap_bounds(&mut world);
    let tr = world.get::<&Transform>(e).unwrap();
    assert!(approx(tr.position.x, -95.0), "x: {}", tr.position.x);
    assert!(approx(tr.position.y, 80.0), "y: {}", tr.position.y);
}

#[test]
fn clamp_bounds_clamps_and_zeroes_velocity() {
    let mut world = World::new();
    let e = world.spawn((
        t_at(150.0, -50.0),
        Velocity2D { x: 30.0, y: 0.0, gravity: 0.0 },
        ClampBounds {
            min: [-100.0, -100.0],
            max: [100.0, 100.0],
            zero_velocity_on_hit: true,
        },
    ));
    systems::update_clamp_bounds(&mut world);
    let tr = world.get::<&Transform>(e).unwrap();
    assert!(approx(tr.position.x, 100.0));
    assert!(approx(tr.position.y, -50.0));
    let v = world.get::<&Velocity2D>(e).unwrap();
    assert!(approx(v.x, 0.0));
}

#[test]
fn clamp_bounds_keeps_velocity_when_flag_off() {
    let mut world = World::new();
    let e = world.spawn((
        t_at(150.0, 0.0),
        Velocity2D { x: 30.0, y: 0.0, gravity: 0.0 },
        ClampBounds {
            min: [-100.0, -100.0],
            max: [100.0, 100.0],
            zero_velocity_on_hit: false,
        },
    ));
    systems::update_clamp_bounds(&mut world);
    let v = world.get::<&Velocity2D>(e).unwrap();
    assert!(approx(v.x, 30.0));
}

#[test]
fn despawn_out_of_bounds_collects_outside_only() {
    let mut world = World::new();
    let inside = world.spawn((
        t_at(0.0, 0.0),
        DespawnOutOfBounds { min: [-50.0, -50.0], max: [50.0, 50.0], margin: 0.0 },
    ));
    let outside = world.spawn((
        t_at(60.0, 0.0),
        DespawnOutOfBounds { min: [-50.0, -50.0], max: [50.0, 50.0], margin: 0.0 },
    ));
    let on_margin = world.spawn((
        t_at(60.0, 0.0),
        DespawnOutOfBounds { min: [-50.0, -50.0], max: [50.0, 50.0], margin: 20.0 },
    ));
    let despawned = systems::update_despawn_out_of_bounds(&mut world);
    assert!(despawned.contains(&outside));
    assert!(!despawned.contains(&inside));
    assert!(!despawned.contains(&on_margin));
}

#[test]
fn face_velocity_rotates_to_velocity_direction() {
    let mut world = World::new();
    let e = world.spawn((
        t_at(0.0, 0.0),
        Velocity2D { x: 0.0, y: 10.0, gravity: 0.0 },
        FaceVelocity { offset_radians: 0.0, min_speed: 1e-3, turn_rate: 0.0 },
    ));
    systems::update_face_velocity(&mut world);
    let tr = world.get::<&Transform>(e).unwrap();
    // atan2(10, 0) = π/2.
    assert!(approx(tr.rotation, std::f32::consts::FRAC_PI_2));
}

#[test]
fn face_velocity_skips_below_min_speed() {
    let mut world = World::new();
    let e = world.spawn((
        Transform { position: Vec2::ZERO, rotation: 1.234, scale: Vec2::splat(1.0) },
        Velocity2D { x: 0.0, y: 0.0, gravity: 0.0 },
        FaceVelocity { offset_radians: 0.0, min_speed: 0.5, turn_rate: 0.0 },
    ));
    systems::update_face_velocity(&mut world);
    let tr = world.get::<&Transform>(e).unwrap();
    assert!(approx(tr.rotation, 1.234));
}

#[test]
fn angular_velocity_accumulates_rotation() {
    let mut world = World::new();
    let e = world.spawn((t_at(0.0, 0.0), AngularVelocity { omega: 2.0 }));
    systems::update_angular_velocity(&mut world, 0.5);
    systems::update_angular_velocity(&mut world, 0.5);
    let tr = world.get::<&Transform>(e).unwrap();
    assert!(approx(tr.rotation, 2.0));
}

#[test]
fn flicker_captures_alpha_toggles_and_expires() {
    let mut world = World::new();
    let mut sprite = SpriteComponent::default();
    sprite.color[3] = 0.8;
    let e = world.spawn((
        sprite,
        Flicker {
            frequency: 10.0,
            duration: 0.2,
            elapsed: 0.0,
            original_alpha: None,
        },
    ));

    // First tick: capture alpha = 0.8, elapsed 0.05, phase 0 → alpha = orig.
    let expired = systems::update_flicker(&mut world, 0.05);
    assert!(expired.is_empty());
    {
        let sp = world.get::<&SpriteComponent>(e).unwrap();
        assert!(approx(sp.color[3], 0.8));
        let fl = world.get::<&Flicker>(e).unwrap();
        assert_eq!(fl.original_alpha, Some(0.8));
    }

    // Tick to elapsed = 0.15 (period = 0.1 → phase 1) → alpha = 0.
    let expired = systems::update_flicker(&mut world, 0.10);
    assert!(expired.is_empty());
    {
        let sp = world.get::<&SpriteComponent>(e).unwrap();
        assert!(approx(sp.color[3], 0.0));
    }

    // Tick to elapsed = 0.25 (≥ duration) → expire + restore.
    let expired = systems::update_flicker(&mut world, 0.10);
    assert_eq!(expired, vec![e]);
    let sp = world.get::<&SpriteComponent>(e).unwrap();
    assert!(approx(sp.color[3], 0.8));
    drop(sp);

    systems::remove_expired_flickers(&mut world, &expired);
    assert!(world.get::<&Flicker>(e).is_err());
}

#[test]
fn shake_jitters_and_leaves_no_offset_on_expire() {
    let mut world = World::new();
    let e = world.spawn((
        t_at(10.0, 20.0),
        Shake {
            intensity: 5.0,
            frequency: 30.0,
            duration: 0.1,
            elapsed: 0.0,
            offset: None,
            seed: None,
        },
    ));

    // Mid-shake: the applied offset is recorded and the seed primed.
    let expired = systems::update_shake(&mut world, 0.05);
    assert!(expired.is_empty());
    {
        let sh = world.get::<&Shake>(e).unwrap();
        assert!(sh.offset.is_some());
        assert!(sh.seed.is_some());
        let tr = world.get::<&Transform>(e).unwrap();
        let [ox, oy] = sh.offset.unwrap();
        assert!(approx(tr.position.x, 10.0 + ox) && approx(tr.position.y, 20.0 + oy));
    }

    // After duration: expired, and the offset is gone.
    let expired = systems::update_shake(&mut world, 0.10);
    assert_eq!(expired, vec![e]);
    let tr = world.get::<&Transform>(e).unwrap();
    assert!(approx(tr.position.x, 10.0));
    assert!(approx(tr.position.y, 20.0));
    drop(tr);

    systems::remove_expired_shakes(&mut world, &expired);
    assert!(world.get::<&Shake>(e).is_err());
}

#[test]
fn a_moving_entity_keeps_moving_while_it_shakes() {
    // The bug: Shake captured the position it started at and wrote
    // `origin + jitter` every tick, so an entity walking or being knocked back
    // froze for the whole shake and then snapped back to where it began.
    let mut world = World::new();
    let e = world.spawn((
        t_at(0.0, 0.0),
        Shake { intensity: 2.0, frequency: 30.0, duration: 0.5, ..Default::default() },
    ));

    // Ten ticks of moving right at 100 units/s, with the shake in between -
    // the order a runtime runs them in.
    for _ in 0..10 {
        world.get::<&mut Transform>(e).unwrap().position.x += 100.0 * 0.02;
        let _ = systems::update_shake(&mut world, 0.02);
    }
    let x = world.get::<&Transform>(e).unwrap().position.x;
    assert!((x - 20.0).abs() <= 2.0 + 1e-3, "20 units of movement, +/- the 2-unit jitter; got {x}");

    // Once the shake ends the entity sits exactly where its movement took it.
    let mut expired = Vec::new();
    for _ in 0..30 {
        world.get::<&mut Transform>(e).unwrap().position.x += 100.0 * 0.02;
        expired = systems::update_shake(&mut world, 0.02);
        if !expired.is_empty() {
            break;
        }
    }
    assert_eq!(expired, vec![e]);
    let moved = world.get::<&Transform>(e).unwrap().position;
    // Every tick moved 2 units; count them from the elapsed time.
    let ticks = (world.get::<&Shake>(e).unwrap().elapsed / 0.02).round();
    assert!(approx(moved.x, ticks * 2.0), "left an offset or lost movement: x={}, ticks={ticks}", moved.x);
    assert!(approx(moved.y, 0.0));
}
