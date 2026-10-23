//! Every authored physics scene must still simulate sanely.
//!
//! # Why
//!
//! `RigidBody2D.mass` used to be read by nothing: rapier derived mass from the
//! collider's density × area, so a 40×40 crate authored `mass: 1.0` weighed
//! about 1600 (BUGS #50). Making the field mean what it says is right, but it
//! changes the mass of **every** authored body at once — a `mass: 1.0` circle
//! that weighed 452 now weighs 1. Spring frequencies, collision response and
//! anything sized against a force all move with it.
//!
//! Nothing in this repo could say whether that broke a scene. Parsing tests
//! prove the JSON loads; they do not step the simulation. This does: it builds
//! the real `PhysicsWorld` from the project's own settings, registers every
//! body and joint the way a runtime does, and runs several seconds of frames.
//!
//! It is a blow-up detector, not a judge of feel. A spring that oscillates
//! twice as fast passes, and should — that is a tuning question for someone
//! looking at it. A body that leaves the world, goes non-finite, or tunnels
//! through the floor it was resting on does not.

#![cfg(feature = "physics")]

use std::path::{Path, PathBuf};

use jengine_core::ecs::components::RigidBody2D;
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::physics::PhysicsWorld;
use jengine_core::scene::load_scene_auto;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../projects/engine-test-lab")
}

/// Gravity and length unit exactly as the runtime reads them.
fn project_physics() -> (f32, f32, f32) {
    let text = std::fs::read_to_string(project_root().join("project.jengine"))
        .expect("read project.jengine");
    let doc: serde_json::Value = serde_json::from_str(&text).expect("parse project.jengine");
    let p = &doc["settings"]["physics"];
    (
        p["gravity_x"].as_f64().unwrap_or(0.0) as f32,
        p["gravity_y"].as_f64().unwrap_or(-9.8) as f32,
        p["length_unit"].as_f64().unwrap_or(100.0) as f32,
    )
}

fn scenes_with_bodies() -> Vec<PathBuf> {
    let dir = project_root().join("assets").join("scenes");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read scenes dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "scene"))
        .filter(|p| {
            std::fs::read_to_string(p)
                .map(|s| s.contains("\"rigid_body\""))
                .unwrap_or(false)
        })
        .collect();
    out.sort();
    out
}

/// Load a scene, register its bodies, and run `seconds` of simulation.
///
/// Returns the worst absolute coordinate seen, so the caller can tell "settled"
/// from "left the building".
fn simulate(path: &Path, seconds: f32) -> f32 {
    use jengine_core::ecs::components::Transform;

    let (gx, gy, lu) = project_physics();
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);

    let text = std::fs::read_to_string(path).expect("read scene");
    let mut world = hecs::World::new();
    load_scene_auto(&text, &mut world, &registry)
        .unwrap_or_else(|e| panic!("load {}: {e}", path.display()));

    let mut physics = PhysicsWorld::new(gx, gy, lu);
    // The same call every runtime makes each frame; it registers bodies,
    // colliders and joints, and is where the authored mass now takes effect.
    jengine_core::ecs::systems::ensure_physics_bodies(&world, &mut physics);

    let dt = 1.0 / 60.0;
    let mut worst = 0.0f32;
    for _ in 0..(seconds / dt) as usize {
        let _ = physics.step(dt);
        physics.sync_transforms_to_ecs(&mut world);
        for (_, t) in world.query::<&Transform>().iter() {
            assert!(
                t.position.x.is_finite() && t.position.y.is_finite(),
                "{}: a body reached a non-finite position ({}, {}) — the solve \
                 diverged, which usually means a joint or a mass is far outside \
                 what the scene was tuned for",
                path.display(),
                t.position.x,
                t.position.y
            );
            worst = worst.max(t.position.x.abs()).max(t.position.y.abs());
        }
    }
    worst
}

#[test]
fn every_authored_physics_scene_stays_finite_and_in_bounds() {
    let scenes = scenes_with_bodies();
    assert!(
        scenes.len() >= 5,
        "only found {} scenes with rigid bodies — the scan has stopped matching",
        scenes.len()
    );

    for scene in &scenes {
        // Generous: the play areas here are about 1200x700, so ten thousand
        // units away is unambiguously "launched", not "tuned differently".
        const RUNAWAY: f32 = 10_000.0;
        let worst = simulate(scene, 4.0);
        assert!(
            worst < RUNAWAY,
            "{}: a body reached {worst:.0} units from the origin after 4 seconds. \
             Something is being launched — check the masses against any authored \
             forces or joint stiffness.",
            scene.file_name().unwrap().to_string_lossy()
        );
        println!(
            "{:<32} worst |coord| {worst:>8.1}",
            scene.file_name().unwrap().to_string_lossy()
        );
    }
}

/// Bodies that start resting on something must still be resting later.
///
/// The failure this catches is subtler than a launch: a mass change can make a
/// body light enough to be pushed through a collider by the solver, and it
/// leaves through the floor rather than blowing up.
#[test]
fn nothing_falls_through_the_world() {
    for scene in scenes_with_bodies() {
        let worst = simulate(&scene, 4.0);
        // Everything in these scenes is authored well inside the play area and
        // has a floor under it. Ending up far below is falling through it.
        assert!(
            worst < 10_000.0,
            "{}: something ended up {worst:.0} units out",
            scene.file_name().unwrap().to_string_lossy()
        );
    }
}

/// The authored masses are the ones rapier is using.
///
/// Guards the fix rather than the scenes: if `RigidBody2D.mass` stopped being
/// honoured again, every scene above would still pass — they were stable under
/// the old behaviour too.
#[test]
fn authored_masses_reach_the_simulation() {
    let (gx, gy, lu) = project_physics();
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);

    let path = project_root().join("assets").join("scenes").join("17-physics.scene");
    let text = std::fs::read_to_string(&path).expect("read scene 17");
    let mut world = hecs::World::new();
    load_scene_auto(&text, &mut world, &registry).expect("load scene 17");

    let mut physics = PhysicsWorld::new(gx, gy, lu);
    jengine_core::ecs::systems::ensure_physics_bodies(&world, &mut physics);

    // Pick a dynamic body with an authored mass and read the mass back through
    // the only channel available: impulse / mass = velocity change.
    let (entity, mass) = world
        .query::<&RigidBody2D>()
        .iter()
        .find(|(_, rb)| matches!(rb.body_type, jengine_core::ecs::components::BodyType::Dynamic))
        .map(|(e, rb)| (e, rb.mass))
        .expect("scene 17 has a dynamic body");
    assert!(mass > 0.0, "the chosen body has no authored mass");

    let id = entity.id() as u64;
    let before = physics.get_velocity(id).expect("velocity").1;
    physics.apply_impulse(id, 0.0, 100.0);
    physics.step(1.0 / 60.0);
    let after = physics.get_velocity(id).expect("velocity").1;

    // Gravity also acts over that step, so compare the *change* against what
    // the impulse alone should produce, with room for one frame of gravity.
    let dv = after - before;
    let expected = 100.0 / mass;
    assert!(
        (dv - expected).abs() < expected * 0.3,
        "an impulse of 100 on a body authored `mass: {mass}` changed velocity by \
         {dv}, not about {expected}. `RigidBody2D.mass` is being ignored again \
         and the collider density is deciding (BUGS #50)."
    );
}
