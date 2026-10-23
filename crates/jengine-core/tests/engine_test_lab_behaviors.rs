//! Scene 13's behaviour graphs, driven for real.
//!
//! The scene looked broken in the editor — the player would not move, the
//! bouncers flew off screen keeping their initial velocity, the faders never
//! expired. Every one of those is what a graph-driven entity does when its
//! graph never attaches, and the cause was an editor-side loading gap rather
//! than anything wrong with the scene: `.behavior` files were uploaded to the
//! viewport only by the Toolbar's Play handler, so a scene entered through a
//! script's `load_scene` never got them.
//!
//! That gap is invisible from here (it is TypeScript), so this asserts the
//! other half: given the graphs, the scene behaves as its on-screen hints
//! claim. If these pass and the scene still misbehaves in a runtime, the fault
//! is in that runtime's asset loading, not in the scene or the graphs.

use std::path::PathBuf;

use glam::Vec2;
use hecs::World;

use jengine_core::behavior::BehaviorGraphSystem;
use jengine_core::ecs::components::{
    BehaviorGraphComponent, InputAxis2D, Lifetime, Name, Velocity2D,
};
use jengine_core::scene::format::{load_scene, SceneData};

fn assets_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("projects")
        .join("engine-test-lab")
        .join("assets")
}

fn load_scene13() -> (World, PathBuf) {
    let root = assets_root();
    let path = root.join("scenes").join("13-behavior.scene");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let scene: SceneData = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    let mut world = World::new();
    load_scene(&scene, &mut world);
    (world, root)
}

fn find_named(world: &World, name: &str) -> hecs::Entity {
    for (e, n) in world.query::<&Name>().iter() {
        if n.0 == name {
            return e;
        }
    }
    panic!("entity {name:?} not found in scene 13");
}

#[test]
fn every_graph_in_the_scene_attaches() {
    let (mut world, root) = load_scene13();

    let declared = world
        .query::<&BehaviorGraphComponent>()
        .iter()
        .count();
    assert!(declared > 0, "scene 13 should carry behavior graphs");

    let mut system = BehaviorGraphSystem::new();
    let errors = system.sync_instances(&mut world, &root);
    assert!(errors.is_empty(), "sync_instances reported: {errors:#?}");
    assert_eq!(
        system.instances.instances.len(),
        declared,
        "every entity declaring a graph should end up with an instance"
    );
}

#[test]
fn the_player_graph_turns_input_into_velocity() {
    // The on-screen hint promises "WASD / arrows = move yellow Player".
    let (mut world, root) = load_scene13();
    let mut system = BehaviorGraphSystem::new();
    let _ = system.sync_instances(&mut world, &root);

    let player = find_named(&world, "Player");
    {
        let mut axis = world
            .get::<&mut InputAxis2D>(player)
            .expect("Player needs an InputAxis2D for the graph to read");
        axis.current = Vec2::new(1.0, 0.0);
    }
    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let vx = world
        .get::<&Velocity2D>(player)
        .expect("Player has Velocity2D")
        .x;
    assert!(
        vx > 0.0,
        "a positive input axis should drive the player right, got vx = {vx}"
    );

    // ...and releasing the input stops it, rather than leaving it drifting.
    {
        let mut axis = world.get::<&mut InputAxis2D>(player).unwrap();
        axis.current = Vec2::ZERO;
    }
    system.tick_pre_simulation(&mut world, 1.0 / 60.0);
    let vx = world.get::<&Velocity2D>(player).unwrap().x;
    assert!(vx.abs() < 1e-3, "zero input should zero the velocity, got vx = {vx}");
}

#[test]
fn the_bouncer_graph_keeps_dots_on_screen() {
    // The hint promises "Coloured dots: bouncer.behavior (OnTick ->
    // BounceOnBounds)". Without the graph they keep their initial velocity and
    // leave the screen, which is exactly what it looked like in the editor.
    let (mut world, root) = load_scene13();
    let mut system = BehaviorGraphSystem::new();
    let _ = system.sync_instances(&mut world, &root);

    // Any entity with a graph and a velocity that is not the player.
    let player = find_named(&world, "Player");
    let dot = world
        .query::<(&BehaviorGraphComponent, &Velocity2D)>()
        .iter()
        .map(|(e, _)| e)
        .find(|e| *e != player)
        .expect("scene 13 should have a graph-driven bouncer");

    // BounceOnBounds inserts the bounds component the mover needs; the point
    // here is that the graph ran and touched the entity at all.
    let before = {
        let v = world.get::<&Velocity2D>(dot).unwrap();
        (v.x, v.y)
    };
    system.tick_pre_simulation(&mut world, 1.0 / 60.0);
    let after = {
        let v = world.get::<&Velocity2D>(dot).unwrap();
        (v.x, v.y)
    };
    // Either the graph set up bounds handling or it adjusted velocity; both
    // mean it ran. A silent no-op is the failure this guards.
    let bounced = world
        .get::<&jengine_core::ecs::components::BounceArea>(dot)
        .is_ok()
        || before != after;
    assert!(
        bounced,
        "the bouncer graph did nothing on tick — before {before:?}, after {after:?}"
    );
}

#[test]
fn the_fader_graphs_insert_their_lifetimes() {
    // The hint promises "fader-3s/6s/9s (OnTick -> DespawnAfter) - watch them
    // vanish". DespawnAfter inserts a Lifetime, which the runtime then counts
    // down; if the graph never attaches they sit there forever.
    let (mut world, root) = load_scene13();
    let mut system = BehaviorGraphSystem::new();
    let _ = system.sync_instances(&mut world, &root);

    let faders: Vec<hecs::Entity> = world
        .query::<(&Name, &BehaviorGraphComponent)>()
        .iter()
        .filter(|(_, (_, bg))| bg.graph_path.contains("fader"))
        .map(|(e, _)| e)
        .collect();
    assert!(!faders.is_empty(), "scene 13 should have fader entities");

    for e in &faders {
        assert!(
            world.get::<&Lifetime>(*e).is_err(),
            "Lifetime should come from the graph, not the scene"
        );
    }
    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let mut remaining: Vec<f32> = faders
        .iter()
        .map(|e| {
            world
                .get::<&Lifetime>(*e)
                .expect("DespawnAfter should insert a Lifetime on first tick")
                .remaining
        })
        .collect();
    remaining.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert!(
        remaining.windows(2).all(|w| w[1] > w[0]),
        "the three faders should have distinct, increasing lifetimes: {remaining:?}"
    );
}
