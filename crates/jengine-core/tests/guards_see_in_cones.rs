//! **Guards see in cones** (Oct 2026; `ecs::vision`): a `vision` as a scene
//! authors it sees a tagged target in its cone, loses it behind a wall and
//! finds it again through a doorway, turns with its entity, and draws what
//! it sees as a fan in the entity's mesh. Found by the showcase's Eyes in the
//! Dark. The polygon itself is tested in the module.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Collider2D, ColliderShape, CustomComponents, Mesh2D, Tags, Transform};
use jengine_core::ecs::vision::{Vision, set_vision_field, tick_vision};

fn at(x: f32, y: f32, rot: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: rot, ..Default::default() }
}

fn seen(world: &World, guard: hecs::Entity) -> i64 {
    world.get::<&CustomComponents>(guard).unwrap().0["Vision"]["seen"].as_i64().unwrap()
}

#[test]
fn a_guard_sees_through_the_doorway_not_the_wall_and_turns_with_its_body() {
    let mut world = World::new();
    let vision: Vision = serde_json::from_value(serde_json::json!({
        "radius": 500.0, "fov": 1.0, "targets": "thief", "color": [1.0, 0.9, 0.5, 0.3]
    }))
    .expect("a vision deserializes");
    let guard = world.spawn((at(0.0, 0.0, 0.0), vision));
    // A wall at x = 200 with a doorway from y = -30 to 30.
    for (y, h) in [(130.0, 200.0), (-130.0, 200.0)] {
        world.spawn((at(200.0, y, 0.0), Collider2D { shape: ColliderShape::Box { width: 20.0, height: h }, ..Default::default() }));
    }
    let mut tags = Tags::default();
    tags.0.insert("thief".into());
    let thief = world.spawn((at(350.0, 0.0, 0.0), tags));

    tick_vision(&mut world);
    assert_eq!(seen(&world, guard), 1, "through the doorway");
    world.get::<&mut Transform>(thief).unwrap().position = Vec2::new(350.0, 120.0);
    tick_vision(&mut world);
    assert_eq!(seen(&world, guard), 0, "behind the wall");
    // On this side of the wall, but off to the guard's left: out of the cone
    // until the guard turns to it.
    world.get::<&mut Transform>(thief).unwrap().position = Vec2::new(0.0, 150.0);
    tick_vision(&mut world);
    assert_eq!(seen(&world, guard), 0, "out of the cone");
    world.get::<&mut Transform>(guard).unwrap().rotation = std::f32::consts::FRAC_PI_2;
    tick_vision(&mut world);
    assert_eq!(seen(&world, guard), 1, "turned to face it");
    // The fan is in the guard's own frame: turned with it, it still points
    // along local +x.
    let mesh = world.get::<&Mesh2D>(guard).unwrap();
    let far = mesh.vertices.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
    assert!(far > 400.0, "the fan reaches along the guard's facing: {far}");
    drop(mesh);
    set_vision_field(&world, guard, "radius", &serde_json::json!(100.0)).unwrap();
    tick_vision(&mut world);
    assert_eq!(seen(&world, guard), 0, "a shorter reach no longer gets there");
    assert!(set_vision_field(&world, guard, "custard", &serde_json::json!(1)).is_err());
}
