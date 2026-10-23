//! **Cloth and rope** (Sep 2026): a `cloth` grid and a `rope` chain moved
//! by the engine every frame and written into their entity's mesh, and the
//! bridge a rope is drawn along - planks on revolute joints, weighed down by
//! a character's `push_mass`. Found by the showcase's Cloth & Rope.

#![cfg(feature = "physics")]

mod support;

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::cloth::{set_cloth_field, tick_cloth, Cloth, ClothPin, Rope, RopePin};
use jengine_core::ecs::components::{
    BodyType, CharacterController2D, Collider2D, ColliderShape, Joint2D, JointType, Mesh2D, Name, RigidBody2D,
    Transform,
};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;

static NATIVE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs")));
static VIEWPORT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs")));
static WEB: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs")));
const DT: f32 = 1.0 / 60.0;

#[test]
fn every_runtime_moves_cloth_after_the_hierarchy() {
    for (name, src) in [("native", NATIVE.as_str()), ("viewport", VIEWPORT.as_str()), ("web", WEB.as_str())] {
        let trails = src.find("trails::tick_trails(").unwrap_or_else(|| panic!("{name}: no trail tick"));
        // By characters, not bytes: a cut inside a multi-byte '─' panics.
        let after: String = src[trails..].chars().take(400).collect();
        assert!(
            after.contains("cloth::tick_cloth("),
            "{name} does not move cloth and rope right after the hierarchy resolve, beside the trails"
        );
    }
}

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

#[test]
fn a_banner_writes_its_mesh_and_hangs() {
    let mut world = World::new();
    let e = world.spawn((
        at(100.0, 50.0),
        Cloth { size: [60.0, 120.0], cols: 4, rows: 8, anchor: [0.5, 1.0], pin: ClothPin::Top, ..Default::default() },
    ));
    for _ in 0..120 {
        tick_cloth(&mut world, DT);
    }
    let mesh = world.get::<&Mesh2D>(e).expect("a cloth gets a mesh when it has none");
    assert_eq!(mesh.vertices.len(), 32);
    assert_eq!(mesh.indices.len(), 3 * 7 * 6);
    // In the entity's space: the top row where it is pinned, the bottom
    // hanging below.
    assert!((mesh.vertices[0].position[0] + 30.0).abs() < 1e-3 && mesh.vertices[0].position[1].abs() < 1e-3);
    let bottom = mesh.vertices[31].position[1];
    assert!(bottom < -100.0, "hangs about its length: {bottom}");
}

#[test]
fn a_moving_pole_carries_its_flag() {
    let mut world = World::new();
    let e = world.spawn((at(0.0, 0.0), Cloth { pin: ClothPin::Left, cols: 6, rows: 4, ..Default::default() }));
    tick_cloth(&mut world, DT);
    world.get::<&mut Transform>(e).unwrap().position.x = 300.0;
    for _ in 0..90 {
        tick_cloth(&mut world, DT);
    }
    let c = world.get::<&Cloth>(e).unwrap();
    assert!(c.points()[0].x > 299.0, "the hoist follows the pole: {}", c.points()[0]);
    assert!(c.points().iter().all(|p| p.x > 250.0), "and the rest of the flag comes after it");
}

#[test]
fn a_rope_follows_what_it_is_pinned_to() {
    let mut world = World::new();
    let post = world.spawn((at(200.0, 0.0), Name("Post".into())));
    let mid = world.spawn((at(100.0, -30.0), Name("Mid".into())));
    let e = world.spawn((
        at(0.0, 0.0),
        Rope {
            length: 240.0,
            segments: 12,
            end: "Post".into(),
            pins: vec![RopePin { at: 0.5, entity: "Mid".into(), offset: [0.0, 0.0] }],
            ..Default::default()
        },
    ));
    for _ in 0..60 {
        tick_cloth(&mut world, DT);
    }
    {
        let r = world.get::<&Rope>(e).unwrap();
        assert!(r.points()[12].distance(Vec2::new(200.0, 0.0)) < 1e-3, "the end is on the post");
        assert!(r.points()[6].distance(Vec2::new(100.0, -30.0)) < 1e-3, "the middle on its pin");
    }
    world.get::<&mut Transform>(mid).unwrap().position.y = -80.0;
    world.get::<&mut Transform>(post).unwrap().position.y = 40.0;
    tick_cloth(&mut world, DT);
    let r = world.get::<&Rope>(e).unwrap();
    assert!(r.points()[6].distance(Vec2::new(100.0, -80.0)) < 1e-3, "a pin that moves takes the rope with it");
    assert!(r.points()[12].distance(Vec2::new(200.0, 40.0)) < 1e-3);
}

#[test]
fn cloth_set_changes_the_wind_and_keeps_the_cloth() {
    let mut world = World::new();
    let e = world.spawn((at(0.0, 0.0), Cloth { pin: ClothPin::Left, flutter: 0.0, gust: 0.0, ..Default::default() }));
    for _ in 0..30 {
        tick_cloth(&mut world, DT);
    }
    let before = world.get::<&Cloth>(e).unwrap().points().to_vec();
    set_cloth_field(&world, e, "wind", &serde_json::json!([1800.0, 0.0])).unwrap();
    set_cloth_field(&world, e, "iterations", &serde_json::json!(8.0)).unwrap();
    assert!(set_cloth_field(&world, e, "windd", &serde_json::json!(1.0)).is_err(), "a misspelt field is an error");
    {
        let c = world.get::<&Cloth>(e).unwrap();
        assert_eq!(c.wind, [1800.0, 0.0]);
        assert_eq!(c.iterations, 8);
        assert_eq!(c.points(), &before[..], "the simulation carries on from where it was");
    }
    for _ in 0..240 {
        tick_cloth(&mut world, DT);
    }
    let c = world.get::<&Cloth>(e).unwrap();
    let fly = c.points()[c.cols as usize - 1];
    assert!(fly.x > 120.0, "and the wind holds the flag out: {fly}");
}

/// A bridge of `n` planks between two posts, and a traveller standing on
/// its middle (or not); the middle plank's height after two seconds.
fn bridge(walker: bool) -> f32 {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, -9.8, 100.0);
    let (n, pitch, gap) = (15usize, 40.0f32, 290.0f32);
    let fixed = |world: &mut World, name: &str, x: f32| {
        world.spawn((at(x, 0.0), Name(name.into()), RigidBody2D { body_type: BodyType::Static, ..Default::default() }))
    };
    fixed(&mut world, "PostL", -gap);
    let post_r = fixed(&mut world, "PostR", gap);
    let mut planks: Vec<Entity> = Vec::new();
    for i in 0..n {
        let x = -gap + (i as f32 + 0.5) * (2.0 * gap / n as f32);
        let target = if i == 0 { "PostL".to_string() } else { format!("Plank{}", i - 1) };
        let anchor_b = if i == 0 { [0.0, 0.0] } else { [pitch / 2.0, 0.0] };
        let e = world.spawn((
            at(x, -20.0),
            Name(format!("Plank{i}")),
            RigidBody2D { body_type: BodyType::Dynamic, mass: 3.0, linear_damping: 0.4, angular_damping: 0.8, ..Default::default() },
            Collider2D { shape: ColliderShape::Box { width: 36.0, height: 9.0 }, ..Default::default() },
            Joint2D {
                target_entity: target,
                joint_type: JointType::Revolute {
                    anchor: [-pitch / 2.0, 0.0],
                    anchor_b,
                    limits: None,
                    motor_speed: 0.0,
                    motor_max_torque: 0.0,
                },
                collide_connected: false,
                ..Default::default()
            },
        ));
        planks.push(e);
    }
    world
        .insert_one(
            post_r,
            Joint2D {
                target_entity: format!("Plank{}", n - 1),
                joint_type: JointType::Revolute {
                    anchor: [0.0, 0.0],
                    anchor_b: [pitch / 2.0, 0.0],
                    limits: None,
                    motor_speed: 0.0,
                    motor_max_torque: 0.0,
                },
                collide_connected: false,
                ..Default::default()
            },
        )
        .unwrap();
    let hero = walker.then(|| {
        world.spawn((
            at(0.0, 60.0),
            RigidBody2D { body_type: BodyType::Kinematic, fixed_rotation: true, ..Default::default() },
            Collider2D { shape: ColliderShape::Capsule { half_height: 14.0, radius: 10.0 }, ..Default::default() },
            CharacterController2D { push_mass: 40.0, snap_to_ground: 12.0, ..Default::default() },
        ))
    });
    for _ in 0..120 {
        if let Some(h) = hero {
            let mut cc = world.get::<&mut CharacterController2D>(h).unwrap();
            let vy = if cc.grounded { -40.0 } else { cc.velocity[1] - 1200.0 * DT };
            cc.velocity = [0.0, vy];
        }
        ensure_physics_bodies(&world, &mut physics);
        physics.sync_kinematic_from_ecs(&world);
        physics.move_characters(&mut world, DT);
        physics.step(DT);
        physics.sync_transforms_to_ecs(&mut world);
    }
    world.get::<&Transform>(planks[n / 2]).unwrap().position.y
}

#[test]
fn a_bridge_sags_under_a_traveller() {
    let empty = bridge(false);
    let loaded = bridge(true);
    assert!(empty > -120.0, "the bridge hangs between its posts: {empty}");
    assert!(loaded < empty - 12.0, "and dips under someone standing on it: {loaded} against {empty}");
}
