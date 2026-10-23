//! Test-lab scene 42, run headless: both swarms keep out of the rocks, and
//! Separation is the only difference between them.
//!
//! The rocks kept only `Health` bodies out, so this scene's flocks - which
//! have none - walked through them in every runtime (BUGS #155). This steps
//! the scene's own entities through the systems each runtime runs, in the
//! same order: the shared motion step, then separation, then obstacles.

use jengine_core::ecs::behaviors::tick_motion;
use jengine_core::ecs::components::{Name, Obstacle, Separation, Transform};
use jengine_core::ecs::systems::{apply_separation, resolve_obstacle_collisions};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::scene::load_scene_auto;

#[test]
fn scene_42_swarms_stay_out_of_the_rocks_and_only_one_spreads() {
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../projects/engine-test-lab/assets/scenes/42-flocking.scene");
    let mut world = hecs::World::new();
    load_scene_auto(&std::fs::read_to_string(path).unwrap(), &mut world, &registry).unwrap();

    let rocks: Vec<(glam::Vec2, f32)> = world
        .query::<(&Transform, &Obstacle)>()
        .iter()
        .map(|(_, (t, o))| (t.position, o.radius))
        .collect();
    assert_eq!(rocks.len(), 2);

    let dt = 1.0 / 60.0;
    let mut worst_inside = 0.0f32;
    for _ in 0..(12.0 / dt) as usize {
        tick_motion(&mut world, dt, None, false);
        apply_separation(&mut world);
        resolve_obstacle_collisions(&mut world);
        for (_, (t, n)) in world.query::<(&Transform, &Name)>().iter() {
            if !n.0.starts_with("Agent") {
                continue;
            }
            for (c, r) in &rocks {
                worst_inside = worst_inside.max(r - t.position.distance(*c));
            }
        }
    }
    assert!(worst_inside < 0.5, "an agent ended a frame {worst_inside:.1} units inside a rock");

    // Nearest-neighbour spacing per swarm once settled on its target.
    let spacing = |prefix: &str| -> f32 {
        let pts: Vec<glam::Vec2> = world
            .query::<(&Transform, &Name)>()
            .iter()
            .filter(|(_, (_, n))| n.0.starts_with(prefix))
            .map(|(_, (t, _))| t.position)
            .collect();
        let mut min = f32::MAX;
        for (i, a) in pts.iter().enumerate() {
            for b in &pts[i + 1..] {
                min = min.min(a.distance(*b));
            }
        }
        min
    };
    let with = world.query::<(&Name, &Separation)>().iter().filter(|(_, (n, _))| n.0.starts_with("Agent0")).count();
    assert_eq!(with, 10, "the left swarm is the one with Separation");
    let (left, right) = (spacing("Agent0_"), spacing("Agent1_"));
    assert!(left > 15.0, "the Separation swarm stacked anyway: nearest pair {left:.1} apart");
    assert!(right < 5.0, "the swarm without Separation did not pile up: nearest pair {right:.1} apart");
}
