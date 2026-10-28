//! Fixed-step gameplay (`ecs::fixed_step`, `settings.simulation_hz`;
//! docs/determinism-proposal.md, phase 4): the same steps give the same game
//! whatever the display does.
//!
//! A host used to run its frame once with the frame's own `dt`, so a 165 Hz
//! screen integrated a different sum from a 60 Hz one and two machines in a
//! match that sends inputs alone drifted apart. With fixed steps, each frame
//! runs as many steps of exactly `1 / hz` as real time pays for - here
//! emulated as a host does it, through the shared motion and physics - and
//! after the same number of steps the world is the same to the bit at 30,
//! 60, 144, 165 and 240 frames a second, and with frames that jitter.
//!
//! What a step sees of input is the other half: a press is latched until a
//! step has seen it, and seen by one step only.

#![cfg(feature = "physics")]

use hecs::{Entity, World};
use jengine_core::ecs::behaviors::tick_motion;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform, Velocity2D};
use jengine_core::ecs::fixed_step::FixedStep;
use jengine_core::ecs::systems::{ensure_physics_bodies, physics_fixed_step, physics_frame_begin};
use jengine_core::input::{InputState, KeyCode};
use jengine_core::physics::PhysicsWorld;

struct Game {
    world: World,
    physics: PhysicsWorld,
    ball: Entity,
    drifter: Entity,
    steps: u32,
}

fn game() -> Game {
    let mut world = World::new();
    world.spawn((
        Transform { position: glam::Vec2::new(0.0, -200.0), ..Default::default() },
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 2000.0, height: 40.0 }, ..Default::default() },
    ));
    let ball = world.spawn((
        Transform { position: glam::Vec2::new(-30.0, 120.0), rotation: 0.3, ..Default::default() },
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 12.0 }, ..Default::default() },
    ));
    // Moved by the motion step, not the physics: its position is a sum of
    // `velocity * dt`, which a different `dt` sums differently.
    let drifter = world.spawn((Transform::default(), Velocity2D { x: 37.3, y: -11.9, ..Default::default() }));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    Game { world, physics, ball, drifter, steps: 0 }
}

/// One step of gameplay, as a host's `simulation_step` runs it: motion,
/// the physics' own fixed steps within it, and "the game" - a kick on
/// steps 30 and 75, as inputs a lockstep session hands every machine.
fn step(g: &mut Game, dt: f32) {
    tick_motion(&mut g.world, dt, None, false);
    physics_frame_begin(&mut g.world, &mut g.physics, dt);
    let due = g.physics.begin_frame(dt);
    let fixed_dt = g.physics.fixed_dt();
    for _ in 0..due {
        physics_fixed_step(&mut g.world, &mut g.physics, fixed_dt);
    }
    g.physics.end_frame();
    g.steps += 1;
    if g.steps == 30 || g.steps == 75 {
        g.physics.apply_impulse(g.ball.id() as u64, 40.0, 900.0);
    }
}

/// Everything that decides the game, as bits.
fn state(g: &Game) -> Vec<u32> {
    let mut out = Vec::new();
    for e in [g.ball, g.drifter] {
        let t = g.world.get::<&Transform>(e).unwrap();
        out.extend([t.position.x.to_bits(), t.position.y.to_bits(), t.rotation.to_bits()]);
    }
    let (vx, vy) = g.physics.get_velocity(g.ball.id() as u64).unwrap();
    out.extend([vx.to_bits(), vy.to_bits()]);
    out
}

/// Play `steps` steps of fixed-step gameplay with frames of these lengths
/// (cycled), and what the world came to.
fn play_fixed(frames: &[f32], steps: u32) -> Vec<u32> {
    let mut g = game();
    let mut fixed = FixedStep::new();
    fixed.set_hz(60.0);
    let mut i = 0;
    while g.steps < steps {
        let plan = fixed.plan(frames[i % frames.len()], 1.0);
        i += 1;
        for _ in 0..plan.steps {
            if g.steps < steps {
                fixed.begin_step(&g.world);
                step(&mut g, plan.dt);
            }
        }
        // Drawing blends and puts back; it must not change the game.
        fixed.blend_for_draw(&mut g.world);
        fixed.restore(&mut g.world);
    }
    state(&g)
}

/// The old way: one step a frame, of the frame's own `dt`, for as much
/// real time as `steps` fixed steps cover.
fn play_per_frame(frame: f32, steps: u32) -> Vec<u32> {
    let mut g = game();
    let frames = (steps as f32 / 60.0 / frame).round() as u32;
    for _ in 0..frames {
        step(&mut g, frame);
    }
    state(&g)
}

#[test]
fn the_same_steps_give_the_same_game_at_any_frame_rate() {
    let steps = 180;
    let at60 = play_fixed(&[1.0 / 60.0], steps);
    for (name, frames) in [
        ("30 fps", vec![1.0 / 30.0]),
        ("144 fps", vec![1.0 / 144.0]),
        ("165 fps", vec![1.0 / 165.0]),
        ("240 fps", vec![1.0 / 240.0]),
        // A window on a hybrid-GPU laptop: bursts and stalls, about 165 Hz.
        ("165 fps in bursts", vec![0.010, 0.002, 0.018, 0.002, 0.002, 0.002, 0.0061]),
    ] {
        assert_eq!(play_fixed(&frames, steps), at60, "{name} played a different game");
    }
}

/// So the test above can tell: one step a frame, the old way, gives a
/// different game at a different frame rate.
#[test]
fn one_step_a_frame_does_not() {
    assert_ne!(play_per_frame(1.0 / 60.0, 180), play_per_frame(1.0 / 165.0, 180));
}

/// A press is seen by exactly one step: kept through frames that run none
/// (most frames at 165 Hz over 60 Hz steps), and not seen again by a frame's
/// second step.
#[test]
fn a_press_is_seen_by_one_step() {
    for (name, frames) in [
        ("165 fps", vec![1.0f32 / 165.0]),
        ("30 fps, two steps a frame", vec![1.0 / 30.0]),
        ("bursts", vec![0.010, 0.002, 0.018, 0.002, 0.002, 0.002, 0.0061]),
    ] {
        // Press on every frame number in turn, so presses land in frames
        // that run zero, one and two steps.
        for press_frame in 0..12usize {
            let mut input = InputState::new();
            let mut fixed = FixedStep::new();
            fixed.set_hz(60.0);
            let mut seen = 0;
            for f in 0..40usize {
                // The host's events arrive between frames.
                if f == press_frame {
                    input.key_down(KeyCode::Space);
                } else if f == press_frame + 1 {
                    input.key_up(KeyCode::Space);
                }
                let plan = fixed.plan(frames[f % frames.len()], 1.0);
                for s in 0..plan.steps {
                    if s > 0 {
                        input.clear_edges();
                    }
                    if input.is_just_pressed(KeyCode::Space) {
                        seen += 1;
                    }
                }
                if plan.steps == 0 {
                    input.hold_edges();
                }
                input.begin_frame();
            }
            assert_eq!(seen, 1, "{name}: a press on frame {press_frame} was seen by {seen} steps");
        }
    }
}

/// A finger is latched as a key is: its `began`, its `ended` and the tap it
/// made each reach exactly one step, though at 165 Hz most frames run none -
/// through the touch tick and the gesture recogniser every host runs.
#[test]
fn a_tap_is_seen_by_one_step() {
    use jengine_core::input::TouchStage;
    use jengine_core::input::gestures::GestureKind;
    use jengine_core::input::touch_controls::{TouchControlsState, TouchPhase};
    use jengine_core::ui::scaling::UiScale;
    let ui = UiScale::from_reference(1280.0, 720.0);
    for (name, frames) in [
        ("165 fps", vec![1.0f32 / 165.0]),
        ("30 fps, two steps a frame", vec![1.0 / 30.0]),
        ("bursts", vec![0.010, 0.002, 0.018, 0.002, 0.002, 0.002, 0.0061]),
    ] {
        // Land on each frame in turn and lift on the next.
        for land in 0..10usize {
            let mut input = InputState::new();
            let mut touch = TouchControlsState::new();
            let mut fixed = FixedStep::new();
            fixed.set_hz(60.0);
            let (mut began, mut ended, mut taps) = (0, 0, 0);
            let mut now = 0.0f64;
            for f in 0..40usize {
                let frame = frames[f % frames.len()];
                now += frame as f64;
                if f == land {
                    touch.push(7, TouchPhase::Started, 400.0, 300.0);
                } else if f == land + 1 {
                    touch.push(7, TouchPhase::Ended, 401.0, 300.0);
                }
                touch.tick(None, &mut input, &ui, (1280.0, 720.0));
                input.update_interactions_at(now);
                let plan = fixed.plan(frame, 1.0);
                for s in 0..plan.steps {
                    if s > 0 {
                        input.clear_edges();
                    }
                    for t in input.touches.iter().filter(|t| t.id == 7) {
                        match t.stage {
                            TouchStage::Began => began += 1,
                            TouchStage::Ended => ended += 1,
                            _ => {}
                        }
                    }
                    taps += input.gestures.iter().filter(|g| g.kind == GestureKind::Tap).count();
                }
                if plan.steps == 0 {
                    input.hold_edges();
                }
                input.begin_frame();
            }
            // A finger that lands and lifts before any step reads as one
            // `ended` - as a tap shorter than a frame always has.
            assert!(began <= 1, "{name}: landing on frame {land}, `began` seen by {began} steps");
            assert_eq!(ended, 1, "{name}: landing on frame {land}, `ended` seen by {ended} steps");
            assert_eq!(taps, 1, "{name}: landing on frame {land}, {taps} taps");
        }
    }
}

/// Off - no `settings.simulation_hz` - a host's frame is what it always was:
/// one step of the frame's `dt`, and nothing blended or held.
#[test]
fn off_changes_nothing() {
    let mut fixed = FixedStep::from_settings(Some(&serde_json::json!({ "name": "x" })));
    assert!(!fixed.is_on());
    let plan = fixed.plan(1.0 / 165.0, 0.5);
    assert_eq!((plan.steps, plan.dt), (1, 0.5 / 165.0));
    let on = FixedStep::from_settings(Some(&serde_json::json!({ "simulation_hz": 60 })));
    assert_eq!(on.hz(), 60.0);
}

/// Every host runs its gameplay through a `FixedStep`: reads the setting,
/// plans each frame's steps, latches input, interpolates what it draws and
/// starts afresh with a new world. A host left out would play a different
/// game from the others on the same machine.
#[test]
fn every_host_runs_fixed_steps() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (name, rel, setting) in [
        ("native", "crates/jengine-runtime/src/wgpu_scene_runner.rs", "FixedStep::from_settings("),
        ("web", "crates/jengine-web/src/lib.rs", "simulation_hz"),
        ("editor", "crates/jengine-editor-viewport/src/lib.rs", "FixedStep::from_settings("),
    ] {
        let src = std::fs::read_to_string(root.join(rel)).unwrap();
        for needle in [
            setting,
            "fixed_step.plan(real_dt",
            "fixed_step.begin_step(",
            "input.clear_edges()",
            "input.hold_edges()",
            "fixed_step.restore(",
            "fixed_step.enter_scene(",
            "scene_simulation_hz(",
            "simulation_step(",
            "fixed_step.blend_for_draw(",
            "fixed_step.is_on()",
            // Packages' once-a-frame phase, before any step: what reads the
            // network a lockstep gate waits on (`lockstep_session`).
            "SystemPhase::Frame",
        ] {
            assert!(src.contains(needle), "{name} ({rel}) has no `{needle}`");
        }
    }
    let editor = std::fs::read_to_string(root.join("editor/src/lib/Viewport.svelte")).unwrap();
    assert!(editor.contains("viewport_set_simulation("), "the editor never hands the viewport `simulation_hz`");
}
