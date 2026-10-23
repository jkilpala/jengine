//! **Input replays** (Oct 2026; `input::replay`): a run recorded and played
//! back exactly. Two balls bounce round a walled box under gravity, kicked
//! by keys and pushed by the mouse, on frame times that jitter as a real
//! display's do - chaotic enough that any difference grows. Recorded as a
//! host runs a frame (the frame time through `replay::frame_dt`, the input
//! through `InputState::update_interactions`), then played back on a freshly
//! built world while the live input and the clock do something else
//! entirely: the trajectory is the same, bit for bit, frame for frame, and
//! the same again on a second playing - and without the replay the same
//! jittery clock alone sends it elsewhere.
//! Found by the showcase's Instant Replay.
#![cfg(feature = "physics")]

use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D};
use jengine_core::input::replay::{self, Mode};
use jengine_core::input::{InputState, KeyCode};
use jengine_core::physics::PhysicsWorld;

/// A small deterministic number source for the test's live input and clock.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The box and two balls, as a scene load builds them.
fn world() -> PhysicsWorld {
    let mut w = PhysicsWorld::new(0.0, -9.81, 100.0);
    let wall = RigidBody2D { body_type: BodyType::Static, ..Default::default() };
    for (id, x, y, bw, bh) in [(1u64, 0.0, -300.0, 900.0, 20.0), (2, 0.0, 300.0, 900.0, 20.0), (3, -450.0, 0.0, 20.0, 600.0), (4, 450.0, 0.0, 20.0, 600.0)] {
        w.add_body(id, &wall, x, y, 0.0);
        w.add_collider(id, &Collider2D { shape: ColliderShape::Box { width: bw, height: bh }, restitution: 0.8, ..Default::default() });
    }
    for (id, x) in [(10u64, -100.0f32), (11, 120.0)] {
        w.add_body(id, &RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() }, x, 0.0, 0.0);
        w.add_collider(id, &Collider2D { shape: ColliderShape::Circle { radius: 24.0 }, restitution: 0.8, ..Default::default() });
    }
    w
}

/// Run `frames` frames as a host does, the live input and clock drawn from
/// `seed`; the balls' positions each frame.
fn run(frames: usize, seed: u64) -> Vec<[f32; 4]> {
    let mut w = world();
    let mut input = InputState::new();
    let mut rng = Lcg(seed);
    let mut trace = Vec::with_capacity(frames);
    for _ in 0..frames {
        // The device's input this frame.
        input.begin_frame();
        let r = rng.next();
        if r < 0.05 {
            input.key_down(KeyCode::Space);
        } else if r < 0.10 {
            input.key_up(KeyCode::Space);
        }
        if rng.next() < 0.1 {
            input.key_down(KeyCode::Left);
        } else {
            input.key_up(KeyCode::Left);
        }
        input.mouse_position = (rng.next() * 1280.0, rng.next() * 720.0);
        input.update_interactions();
        // The display's frame time, jittering round 60 Hz.
        let raw = 1.0 / 60.0 * (0.6 + 0.8 * rng.next());
        let dt = replay::frame_dt(raw);
        // The game: Space kicks both balls up, Left pushes them left, the
        // mouse's x leans on the second.
        if input.is_just_pressed(KeyCode::Space) {
            w.apply_impulse(10, 0.0, 400.0);
            w.apply_impulse(11, 0.0, 400.0);
        }
        if input.is_pressed(KeyCode::Left) {
            w.apply_impulse(10, -40.0, 0.0);
        }
        w.apply_impulse(11, (input.mouse_position.0 - 640.0) * 0.02, 0.0);
        w.step(dt);
        let (a, b) = (w.get_position(10).unwrap(), w.get_position(11).unwrap());
        trace.push([a.0, a.1, b.0, b.1]);
    }
    trace
}

#[test]
fn a_run_plays_back_exactly_from_its_inputs_and_frame_times() {
    replay::clear();
    // Record: armed, then the scene loads.
    replay::record();
    jengine_core::scene::streaming::scene_replaced();
    assert_eq!(replay::mode(), Mode::Recording);
    let recorded = run(600, 7);
    assert_eq!(replay::frames(), 600);
    // Play it back on a fresh world, with a different live input and clock.
    assert!(replay::play());
    jengine_core::scene::streaming::scene_replaced();
    assert_eq!(replay::mode(), Mode::Playing);
    let played = run(600, 99);
    for (k, (a, b)) in recorded.iter().zip(&played).enumerate() {
        assert_eq!(a, b, "frame {k}: the replay went its own way");
    }
    assert!((replay::progress() - 1.0).abs() < 1e-6, "played to its end");
    // And again from its start: a replay can be watched twice.
    assert!(replay::play());
    jengine_core::scene::streaming::scene_replaced();
    let again = run(600, 1234);
    assert_eq!(again.last(), recorded.last(), "the second playing starts from the first frame too");
    // Without the replay, that other input and clock alone send it elsewhere.
    replay::clear();
    let live = run(600, 99);
    assert_ne!(live.last(), recorded.last(), "a different run is different - else this test proves nothing");
    replay::clear();
}

#[test]
fn every_host_hands_its_frame_time_to_the_replay() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for host in [
        "crates/jengine-runtime/src/wgpu_scene_runner.rs",
        "crates/jengine-web/src/lib.rs",
        "crates/jengine-editor-viewport/src/lib.rs",
    ] {
        let src = std::fs::read_to_string(root.join(host)).unwrap();
        assert!(src.contains("input::replay::frame_dt("), "{host} hands its frame time to the replay");
    }
    let interactions = std::fs::read_to_string(root.join("crates/jengine-core/src/input/interactions.rs")).unwrap();
    assert!(interactions.contains("super::replay::apply(self)"), "the input is recorded where every host's frame passes");
}
