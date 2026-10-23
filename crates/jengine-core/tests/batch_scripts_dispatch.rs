//! BUGS #11 — batch scripts actually run.
//!
//! `ecs::behaviors::run_batch_scripts` had no caller in any runtime, so
//! `BatchScriptComponent` — and the whole Slice 5.3 batch vocabulary
//! (`Drag2D`, `Force2D`, `Wind2D`, `Impulse2D`, `Torque2D`) that exists only
//! to serve it — was inert everywhere outside unit tests.
//!
//! Gated on `js-adapter-host`, which is not a default feature: the dispatcher
//! needs a QuickJS runtime. Run with
//! `cargo test -p jengine-core --features js-adapter-host --test batch_scripts_dispatch`.

#![cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host", feature = "native"))]

use hecs::World;
use jengine_core::ecs::behaviors::run_batch_scripts;
use jengine_core::ecs::components::{
    Acceleration2D, BatchScriptComponent, Drag2D, ScriptLang, Transform, Velocity2D,
};
use jengine_core::profiling::FrameCounters;
use jengine_core::scripting::worker_pool::BatchScriptPool;

/// The test-lab scene's own batch script, so this fails if that asset breaks.
const SCRIPT: &str = "scripts/batch/drift_batch.js";

fn lab_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../projects/engine-test-lab/assets")
        .canonicalize()
        .expect("test-lab assets dir")
}

fn spawn_drifter(world: &mut World, x: f32, ax: f32) -> hecs::Entity {
    let mut t = Transform::default();
    t.position.x = x;
    world.spawn((
        t,
        Velocity2D { x: 10.0, y: 0.0, gravity: 0.0 },
        Acceleration2D { x: ax, y: 0.0 },
        Drag2D { x: 0.004, y: 0.004 },
        BatchScriptComponent { path: SCRIPT.to_string(), lang: ScriptLang::Js },
    ))
}

fn pool() -> BatchScriptPool {
    let mut pool = BatchScriptPool::new_direct(None).expect("batch pool builds");
    // The pool only runs scripts it has been given. `run_batch_scripts`
    // silently skips anything not loaded, which is exactly how batch scripts
    // stayed inert even once the dispatcher had a caller — so loading is part
    // of what these tests cover.
    let source = std::fs::read_to_string(lab_root().join(SCRIPT))
        .expect("test-lab batch script exists");
    pool.load_js(SCRIPT, &source).expect("script loads and its schema validates");
    pool
}

#[test]
fn one_call_moves_every_entity_in_the_batch() {
    let mut world = World::new();
    let a = spawn_drifter(&mut world, 0.0, 100.0);
    let b = spawn_drifter(&mut world, 50.0, -100.0);
    let mut pool = pool();
    let mut counters = FrameCounters::default();

    let before_a = world.get::<&Transform>(a).unwrap().position.x;
    let before_b = world.get::<&Transform>(b).unwrap().position.x;

    for _ in 0..10 {
        run_batch_scripts(&mut world, &mut pool, 1.0 / 60.0, &mut counters);
    }

    let after_a = world.get::<&Transform>(a).unwrap().position.x;
    let after_b = world.get::<&Transform>(b).unwrap().position.x;

    // Both entities moved from ONE script per frame — that is the batch.
    assert_ne!(after_a, before_a, "entity a did not move");
    assert_ne!(after_b, before_b, "entity b did not move");
    // Opposite accelerations, so they must not have moved identically —
    // proof the per-entity columns are gathered and scattered individually
    // rather than one value being broadcast.
    assert!(
        (after_a - before_a) > 0.0 && (after_b - before_b) < (after_a - before_a),
        "columns look broadcast, not per-entity: a {before_a}->{after_a}, b {before_b}->{after_b}"
    );
}

#[test]
fn velocity_is_read_modify_write_across_frames() {
    // Velocity2D is a *write* column the dispatcher pre-fills with the current
    // ECS value. If that pre-fill were missing, each frame would start from
    // zero and acceleration could never accumulate.
    let mut world = World::new();
    let e = spawn_drifter(&mut world, 0.0, 200.0);
    let mut pool = pool();
    let mut counters = FrameCounters::default();

    run_batch_scripts(&mut world, &mut pool, 1.0 / 60.0, &mut counters);
    let v1 = world.get::<&Velocity2D>(e).unwrap().x;
    run_batch_scripts(&mut world, &mut pool, 1.0 / 60.0, &mut counters);
    let v2 = world.get::<&Velocity2D>(e).unwrap().x;

    assert!(v2 > v1, "velocity did not accumulate: {v1} then {v2}");
}

#[test]
fn a_world_with_no_batch_components_is_a_no_op() {
    let mut world = World::new();
    world.spawn((Transform::default(), Velocity2D::default()));
    let mut pool = pool();
    let mut counters = FrameCounters::default();
    // Must not panic or touch anything — this is the common case, since
    // almost no scene uses batch scripts.
    run_batch_scripts(&mut world, &mut pool, 1.0 / 60.0, &mut counters);
}
