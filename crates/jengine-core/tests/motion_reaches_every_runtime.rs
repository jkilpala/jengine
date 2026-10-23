//! **One motion step, in every runtime, and every motion component in it.**
//!
//! Motion was three hand-copied sequences - native's `tick_world`, the editor
//! viewport's and the web runtime's. They agreed, which is luck rather than
//! design: a system added to one would have been missing from the others with
//! no error anywhere. And five of the components the editor offers as motion -
//! `wind2d`, `force2d`, `impulse2d`, `drag2d`, `torque2d` - were integrated by
//! none of them: batch-script vocabulary only, so a `drag2d` on a sprite did
//! nothing. Found by the arcade's physics toy, which is built without scripts
//! and so found every component that needed one.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::behaviors::{tick_motion, update_input_actions};
use jengine_core::ecs::components::{
    AngularVelocity, AxisDrive, BatchScriptComponent, Drag2D, Impulse2D, InputAxis2D, Torque2D,
    Transform, Velocity2D, Wind2D,
};
use jengine_core::input::InputState;

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");

/// Every host calls the shared step, and none keeps a piece of its own copy.
#[test]
fn every_runtime_runs_the_one_motion_step() {
    for (name, src) in [("native", NATIVE), ("viewport", VIEWPORT), ("web", WEB)] {
        assert!(src.contains("behaviors::tick_motion("), "{name} does not call `tick_motion`");
        for piece in [
            "velocity_system(",
            "update_gravity(",
            "update_acceleration(",
            "update_forces(",
            "update_seek_targets(",
            "update_max_speed(",
            "update_follow_targets(",
            "update_patrol_paths(",
            "update_oscillators(",
            "update_wrap_bounds(",
            "update_clamp_bounds(",
            "update_angular_velocity(",
        ] {
            assert!(
                !src.contains(&format!("behaviors::{piece}")) && !src.contains(&format!("sys::{piece}")),
                "{name} still calls `{piece}` itself - a hand copy of the motion step, which is \
                 how one runtime ends up with a system the others lack"
            );
        }
    }
    // Only the host that runs batch scripts hands them their entities.
    assert!(NATIVE.contains("tick_motion(&mut state.world, dt, state.path_library.as_ref(), true)"));
    for (name, src) in [("viewport", VIEWPORT), ("web", WEB)] {
        assert!(src.contains("app.path_library.as_ref(), false)"), "{name}: batch scripts do not run here");
    }
}

fn body(world: &mut World, v: Vec2) -> hecs::Entity {
    world.spawn((Transform::default(), Velocity2D { x: v.x, y: v.y, ..Default::default() }))
}

fn vel(world: &World, e: hecs::Entity) -> Vec2 {
    let v = world.get::<&Velocity2D>(e).unwrap();
    Vec2::new(v.x, v.y)
}

#[test]
fn wind_pushes_and_drag_slows() {
    let mut w = World::new();
    let blown = body(&mut w, Vec2::ZERO);
    w.insert_one(blown, Wind2D { x: 100.0, y: 0.0 }).unwrap();
    let dragged = body(&mut w, Vec2::new(200.0, 0.0));
    w.insert_one(dragged, Drag2D { x: 2.0, y: 2.0 }).unwrap();
    for _ in 0..30 {
        tick_motion(&mut w, 1.0 / 60.0, None, false);
    }
    assert!((vel(&w, blown).x - 50.0).abs() < 0.5, "half a second of 100/s² wind: {}", vel(&w, blown));
    let slowed = vel(&w, dragged).x;
    assert!(slowed > 60.0 && slowed < 80.0, "drag 2 for half a second takes ~63% off: {slowed}");
    assert!(w.get::<&Transform>(blown).unwrap().position.x > 5.0, "and it moved");
}

#[test]
fn a_big_drag_stops_a_body_rather_than_reversing_it() {
    let mut w = World::new();
    let e = body(&mut w, Vec2::new(100.0, -40.0));
    w.insert_one(e, Drag2D { x: 500.0, y: 500.0 }).unwrap();
    tick_motion(&mut w, 1.0 / 60.0, None, false);
    assert_eq!(vel(&w, e), Vec2::ZERO);
}

#[test]
fn an_impulse_is_spent_in_one_frame() {
    let mut w = World::new();
    let e = body(&mut w, Vec2::ZERO);
    w.insert_one(e, Impulse2D { x: 0.0, y: 300.0 }).unwrap();
    tick_motion(&mut w, 1.0 / 60.0, None, false);
    assert_eq!(vel(&w, e).y, 300.0);
    tick_motion(&mut w, 1.0 / 60.0, None, false);
    assert_eq!(vel(&w, e).y, 300.0, "not added twice");
    let i = w.get::<&Impulse2D>(e).unwrap();
    assert_eq!((i.x, i.y), (0.0, 0.0));
}

#[test]
fn torque_spins_up() {
    let mut w = World::new();
    let e = w.spawn((Transform::default(), AngularVelocity { omega: 0.0 }, Torque2D { x: 6.0, y: 0.0 }));
    for _ in 0..60 {
        tick_motion(&mut w, 1.0 / 60.0, None, false);
    }
    let omega = w.get::<&AngularVelocity>(e).unwrap().omega;
    assert!((omega - 6.0).abs() < 0.01, "{omega}");
    assert!(w.get::<&Transform>(e).unwrap().rotation > 2.5, "and it turned");
}

/// An arrow-key axis moves its entity with no script.
#[test]
fn an_input_axis_can_drive_its_own_velocity() {
    let mut w = World::new();
    let e = body(&mut w, Vec2::ZERO);
    w.insert_one(
        e,
        InputAxis2D {
            x_pos: "move_right".into(),
            scale: 240.0,
            drive: AxisDrive::Velocity,
            ..Default::default()
        },
    )
    .unwrap();
    let mut input = InputState::new();
    input.set_bindings("move_right", vec!["D".into()]);
    input.key_down(jengine_core::input::KeyCode::D);
    update_input_actions(&mut w, &input);
    assert_eq!(vel(&w, e).x, 240.0);
    tick_motion(&mut w, 0.5, None, false);
    assert!((w.get::<&Transform>(e).unwrap().position.x - 120.0).abs() < 0.01);
}

/// Where batch scripts run, their entities are theirs: the native integrators
/// used to move them a second time after the batch had.
#[test]
fn a_batch_scripts_entities_are_left_to_it() {
    let mut w = World::new();
    let e = body(&mut w, Vec2::new(100.0, 0.0));
    w.insert_one(e, Wind2D { x: 50.0, y: 0.0 }).unwrap();
    w.insert_one(e, BatchScriptComponent::default()).unwrap();
    tick_motion(&mut w, 0.5, None, true);
    assert_eq!(w.get::<&Transform>(e).unwrap().position, Vec2::ZERO, "the batch moves it, not the step");
    assert_eq!(vel(&w, e).x, 100.0);
    // Where they do not run, the components move it.
    tick_motion(&mut w, 0.5, None, false);
    assert!(w.get::<&Transform>(e).unwrap().position.x > 40.0);
}
