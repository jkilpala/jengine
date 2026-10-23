//! `fixed_update(entity, dt)` (Oct 2026): called once before each physics
//! step with the step's `dt`, reading the world as the step before left it -
//! so gameplay that steps with the physics comes out the same whatever the
//! frame rate. Driven here the way the native runner drives it.

#![cfg(feature = "physics")]

mod support;

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{
    BodyType, Collider2D, ColliderShape, CustomComponents, Name, RigidBody2D, ScriptComponent, ScriptLanguage,
    Transform,
};
use jengine_core::ecs::systems::{ScriptSystem, apply_physics_commands, ensure_physics_bodies};
use jengine_core::input::InputState;
use jengine_core::physics::PhysicsWorld;

/// A ball pulled toward x = 400 by `fixed_update`, which reads where the
/// ball is each step - so a step taken out of turn would change the path.
fn simulate(frames: &[f32]) -> (Vec2, f64, f64) {
    let mut world = World::new();
    let ball = world.spawn((
        Name("Ball".into()),
        Transform::default(),
        RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 10.0 }, ..Default::default() },
    ));
    let src = format!(
        r#"
script = {{ tick_rate = 60 }}
local steps = 0
function update(entity, dt) end
function fixed_update(entity, dt)
    steps = steps + 1
    local x, y = Transform.get_position({ball})
    Physics.set_velocity({ball}, (400 - x) * 3, 0)
    Entity.set_custom(entity, "Fx", "steps", steps)
    Entity.set_custom(entity, "Fx", "dt", dt)
end
"#,
        ball = ball.id()
    );
    // A file per call: the tests run in parallel, and one writing the file
    // while another loads it handed that one an empty script.
    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("jengine_fixed_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("fixed{n}.lua"));
    std::fs::write(&path, src).unwrap();
    let mut custom = CustomComponents::default();
    custom.0.insert("Fx".into(), serde_json::json!({ "steps": 0.0, "dt": 0.0 }));
    let driver = world.spawn((
        Transform::default(),
        custom,
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));

    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    physics.set_fixed_timestep(1.0 / 64.0);
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for &dt in frames {
        system.run(&mut world, dt, &input, None, None);
        ensure_physics_bodies(&world, &mut physics);
        assert!(system.has_fixed_update());
        let steps = physics.begin_frame(dt);
        for _ in 0..steps {
            system.set_physics_ptr(&physics);
            let commands = system.run_fixed(&mut world, physics.fixed_dt(), &input, None, None);
            system.clear_physics_ptr();
            apply_physics_commands(&commands, &mut physics, &world);
            physics.step_once();
            physics.sync_transforms_to_ecs(&mut world);
        }
        physics.end_frame();
    }
    let at = world.get::<&Transform>(ball).unwrap().position;
    let custom = world.get::<&CustomComponents>(driver).unwrap();
    let num = |f: &str| custom.0.get("Fx").and_then(|c| c.get(f)).and_then(|v| v.as_f64()).unwrap_or(-1.0);
    (at, num("steps"), num("dt"))
}

#[test]
fn fixed_update_steps_with_the_physics_whatever_the_frame_rate() {
    // Two seconds at 64 Hz, and the same two seconds in ragged frames: a
    // quarter step, three quarters, a step and a half, and another.
    let even = vec![1.0 / 64.0; 128];
    let ragged: Vec<f32> = [0.25f32, 0.75, 1.5, 1.5].iter().cycle().take(128).map(|k| k / 64.0).collect();
    let (a, steps_a, dt_a) = simulate(&even);
    let (b, steps_b, dt_b) = simulate(&ragged);
    assert_eq!(steps_a, 128.0, "once per physics step");
    assert_eq!(steps_b, 128.0);
    assert_eq!(dt_a, 1.0 / 64.0, "handed the step's dt, not the frame's");
    assert_eq!(dt_b, 1.0 / 64.0);
    assert!(a.x > 300.0, "the ball was pulled: {a:?}");
    assert_eq!(a, b, "the same steps from the same state give the same path, bit for bit");
}

#[test]
fn a_frame_shorter_than_a_step_runs_no_fixed_update() {
    let (_, steps, _) = simulate(&[0.5 / 64.0]);
    assert_eq!(steps, 0.0);
}

#[test]
fn every_runtime_steps_fixed_update_with_the_physics() {
    // The per-step loop is host code in three places; each must call the
    // scripts between the steps and flush their forces for the step.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (name, rel, call) in [
        ("native", "crates/jengine-runtime/src/wgpu_scene_runner.rs", "run_fixed("),
        ("editor viewport", "crates/jengine-editor-viewport/src/lib.rs", "run_fixed_update(app"),
        ("web", "crates/jengine-web/src/lib.rs", "run_fixed_update(app"),
    ] {
        let src = support::with_frame_draw(&std::fs::read_to_string(root.join(rel)).unwrap());
        for needle in ["begin_frame(dt)", call, "flush_forces(fixed_dt)", "step_once()", "end_frame()"] {
            assert!(src.contains(needle), "{name} ({rel}) does not call `{needle}`");
        }
    }
}

#[test]
fn a_force_set_in_fixed_update_pushes_for_its_step() {
    let mut world = World::new();
    let drone = world.spawn((
        Transform::default(),
        RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 20.0, height: 10.0 }, ..Default::default() },
    ));
    let src = format!(
        "script = {{ tick_rate = 60 }}\nfunction update(e, dt) end\nfunction fixed_update(e, dt)\n    Physics.apply_force({}, 0, 600)\nend\n",
        drone.id()
    );
    let dir = std::env::temp_dir().join(format!("jengine_fixed_force_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("force.lua");
    std::fs::write(&path, src).unwrap();
    world.spawn((
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    physics.set_fixed_timestep(1.0 / 64.0);
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..64 {
        system.run(&mut world, 1.0 / 64.0, &input, None, None);
        ensure_physics_bodies(&world, &mut physics);
        let steps = physics.begin_frame(1.0 / 64.0);
        for _ in 0..steps {
            system.set_physics_ptr(&physics);
            let commands = system.run_fixed(&mut world, physics.fixed_dt(), &input, None, None);
            system.clear_physics_ptr();
            apply_physics_commands(&commands, &mut physics, &world);
            physics.flush_forces(physics.fixed_dt());
            physics.step_once();
            physics.sync_transforms_to_ecs(&mut world);
        }
        physics.end_frame();
    }
    // A force of 600 on a unit mass for a second: v = 600, x = 300.
    let (_, vy) = physics.get_velocity(drone.id() as u64).unwrap();
    assert!((vy - 600.0).abs() < 15.0, "a second of the force: {vy}");
}

#[test]
fn a_damped_hover_settles() {
    // A hover that damps with `get_velocity`: the velocity it reads in
    // `fixed_update` has to be rapier's after the last step, or the damping
    // is zero and the drone rings itself apart (it did, at first).
    let mut world = World::new();
    let drone = world.spawn((
        Transform { position: Vec2::new(0.0, -150.0), ..Default::default() },
        RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 80.0, height: 26.0 }, ..Default::default() },
    ));
    let src = format!("script = {{ tick_rate = 60 }}
function update(e, dt) end
function fixed_update(e, dt)
    local _, y = Transform.get_position({d})
    local _, vy = Physics.get_velocity({d})
    Physics.apply_force({d}, 0, 981 + (60 - y) * 40 - vy * 9)
end
", d = drone.id());
    let dir = std::env::temp_dir().join(format!("jengine_probe_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.lua");
    std::fs::write(&path, src).unwrap();
    world.spawn((Transform::default(), ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None }));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..330 {
        system.run(&mut world, 1.0 / 165.0, &input, None, None);
        ensure_physics_bodies(&world, &mut physics);
        let steps = physics.begin_frame(1.0 / 165.0);
        for _ in 0..steps {
            system.set_physics_ptr(&physics);
            let commands = system.run_fixed(&mut world, physics.fixed_dt(), &input, None, None);
            system.clear_physics_ptr();
            apply_physics_commands(&commands, &mut physics, &world);
            physics.flush_forces(physics.fixed_dt());
            physics.step_once();
            physics.sync_transforms_to_ecs(&mut world);
        }
        physics.end_frame();
    }
    let y = world.get::<&Transform>(drone).unwrap().position.y;
    assert!((y - 60.0).abs() < 4.0, "two seconds at 165 Hz should settle it on 60: {y}");
}

/// A burst asked for in `fixed_update` reaches the particle system the host
/// draws. The burst loop lived in `run` alone, reading the command list
/// `run_fixed` swaps out, so a crash judged with the physics went off with no
/// explosion natively (a lander game's crash).
#[test]
fn a_burst_from_fixed_update_reaches_the_particles() {
    let mut world = World::new();
    let emitter = world.spawn((
        Name("Boom".into()),
        Transform::default(),
        jengine_core::particles::ParticleEmitter { emission_rate: 0.0, max_particles: 100, ..Default::default() },
    ));
    let src = format!(
        r#"
script = {{ tick_rate = 60 }}
local done = false
function update(entity, dt) end
function fixed_update(entity, dt)
    if not done then
        Particles.emit_burst({e}, 25)
        done = true
    end
end
"#,
        e = emitter.id()
    );
    let dir = std::env::temp_dir().join(format!("jengine_fixed_burst_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("burst.lua");
    std::fs::write(&path, src).unwrap();
    world.spawn((
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    // `alive_count` is as of the last tick, which the host runs once a frame.
    let tick = |system: &mut ScriptSystem, world: &World| {
        jengine_core::particles::tick_particles(world, &mut system.api.services.particle_system, 0.001);
    };
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    tick(&mut system, &world);
    assert_eq!(system.api.services.particle_system.alive_count(), 0);
    system.run_fixed(&mut world, 1.0 / 60.0, &input, None, None);
    tick(&mut system, &world);
    assert_eq!(system.api.services.particle_system.alive_count(), 25, "the burst from fixed_update was dropped");
}
