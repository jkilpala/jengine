//! **A shot is seen before it is taken** (Oct 2026; `ecs::trajectory`,
//! `PhysicsWorld::predict`). A prediction steps a copy of the world by the
//! real pipeline, so it is not "roughly where the ball goes": it is where
//! the ball goes. This takes the shot after predicting it and holds the
//! two to each other, cushion, struck ball and all - then runs the
//! component through the ECS: the lines it draws, the fields a script
//! reads, and `Trajectory.set`. Found by the showcase's Billiard Hall.

#![cfg(feature = "physics")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, CustomComponents, Name, RigidBody2D, Transform};
use jengine_core::ecs::trajectory::{self, Trajectory};
use jengine_core::physics::PhysicsWorld;
use jengine_core::renderer::line2d::Line2D;

fn ball() -> RigidBody2D {
    RigidBody2D { body_type: BodyType::Dynamic, gravity_scale: 0.0, linear_damping: 0.6, ccd: true, ..Default::default() }
}

fn round() -> Collider2D {
    Collider2D { shape: ColliderShape::Circle { radius: 12.0 }, restitution: 0.9, ..Default::default() }
}

/// A cue ball at the left, a red ball ahead of it and a little up, and a
/// cushion along the top.
fn table() -> PhysicsWorld {
    let mut w = PhysicsWorld::new(0.0, 0.0, 100.0);
    w.add_body(1, &RigidBody2D { body_type: BodyType::Static, ..Default::default() }, 0.0, 160.0, 0.0);
    w.add_collider(1, &Collider2D { shape: ColliderShape::Box { width: 1200.0, height: 20.0 }, restitution: 0.8, ..Default::default() });
    w.add_body(2, &ball(), -300.0, 0.0, 0.0);
    w.add_collider(2, &round());
    w.add_body(3, &ball(), 0.0, 10.0, 0.0);
    w.add_collider(3, &round());
    w
}

#[test]
fn the_shot_goes_where_it_was_seen_to_go() {
    let mut w = table();
    let v = [900.0, 40.0];
    let steps = 150;
    let seen = w.predict(2, v, steps, 1).expect("a dynamic body");
    assert_eq!(seen.hit, Some(3), "it strikes the red ball");
    assert!(seen.hit_path.len() > 10);
    // Predicting changed nothing.
    assert_eq!(w.get_position(2), Some((-300.0, 0.0)));

    w.set_velocity(2, v[0], v[1]);
    for _ in 0..seen.steps {
        w.step_once();
    }
    let (cx, cy) = w.get_position(2).unwrap();
    let (rx, ry) = w.get_position(3).unwrap();
    let c = seen.path.last().unwrap();
    let r = seen.hit_path.last().unwrap();
    assert!((cx - c[0]).abs() < 1e-2 && (cy - c[1]).abs() < 1e-2, "cue ball at ({cx}, {cy}), seen at {c:?}");
    assert!((rx - r[0]).abs() < 1e-2 && (ry - r[1]).abs() < 1e-2, "red ball at ({rx}, {ry}), seen at {r:?}");
    // And it really went somewhere: the red ball was knocked well along.
    assert!(rx > 100.0, "the red ball moved: {rx}");
}

#[test]
fn a_cushion_turns_the_line() {
    let w = table();
    // Straight up at the cushion, missing the red ball.
    let seen = w.predict(2, [0.0, 800.0], 120, 2).unwrap();
    assert_eq!(seen.hit, None, "a cushion is not a ball");
    let top = seen.path.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
    let last = seen.path.last().unwrap()[1];
    assert!(top < 150.0 && top > 120.0, "it reached the cushion and no further: {top}");
    assert!(last < top - 30.0, "and came back off it: {last}");
    // A static body cannot be launched.
    assert!(w.predict(1, [10.0, 0.0], 10, 1).is_none());
}

#[test]
fn the_component_draws_its_lines_and_tells_the_script() {
    let mut world = World::new();
    let spawn_ball = |world: &mut World, name: &str, x: f32, y: f32| {
        world.spawn((Name(name.into()), Transform { position: Vec2::new(x, y), ..Default::default() }, ball(), round()))
    };
    let cue = spawn_ball(&mut world, "Cue", -300.0, 0.0);
    spawn_ball(&mut world, "Red", 0.0, 10.0);
    let aim = world.spawn((Name("Aim".into()), Transform { position: Vec2::new(100.0, 0.0), ..Default::default() }, Line2D::default()));
    let ghost = world.spawn((Name("Ghost".into()), Transform::default(), Line2D::default()));
    world
        .insert_one(cue, Trajectory { velocity: [900.0, 40.0], line: "Aim".into(), hit_line: "Ghost".into(), ..Default::default() })
        .unwrap();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    jengine_core::ecs::systems::ensure_physics_bodies(&world, &mut physics);
    trajectory::predict(&mut world, &physics);

    let path = world.get::<&Line2D>(aim).unwrap().points.clone();
    assert!(path.len() > 5);
    assert_eq!(path[0], [-400.0, 0.0], "drawn in the line entity's own space");
    assert!(!world.get::<&Line2D>(ghost).unwrap().points.is_empty(), "the struck ball's path");
    let fields = world.get::<&CustomComponents>(cue).unwrap().0["Trajectory"].clone();
    assert_eq!(fields["hit"], "Red");
    assert!(fields["hit_x"].as_f64().unwrap() < -10.0, "struck just short of the red ball");

    // Through the script setter: a count as a float, then off.
    trajectory::set_trajectory_field(&world, cue, "every", &serde_json::json!(4.0)).unwrap();
    assert_eq!(world.get::<&Trajectory>(cue).unwrap().every, 4);
    trajectory::set_trajectory_field(&world, cue, "active", &serde_json::json!(false)).unwrap();
    trajectory::predict(&mut world, &physics);
    assert!(world.get::<&Line2D>(aim).unwrap().points.is_empty(), "off clears the line");
    assert_eq!(world.get::<&CustomComponents>(cue).unwrap().0["Trajectory"]["hit"], "");
    assert!(trajectory::set_trajectory_field(&world, cue, "nonsense", &serde_json::json!(1)).is_err());
}
