//! **Navigation sees tilemap walls and moving obstacles** (Oct 2026). A
//! `nav_region` was baked from `collider` components alone, so a tilemap's
//! walls - which exist only inside the physics - were air to it, and an
//! agent walked straight through a warehouse's shelving. And a dynamic body
//! was left out entirely (it would rebake every frame it moved), so a crate
//! pushed into an aisle was walked through too. Now solid tile layers are
//! baked as the physics builds them (cached, keyed by the layers'
//! revisions), and a `nav_obstacle` puts a dynamic body in the bake keyed by
//! its position snapped to `snap` units. Found by the showcase's Warehouse
//! Robots.

#![cfg(all(feature = "physics", feature = "navigation"))]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform};
use jengine_core::navigation::{self, NavObstacle, NavRegion};
use jengine_core::tilemap::{Tilemap, TilemapLayer};

const T: f32 = 40.0;

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE }
}

/// A 10 x 6 map of 40-unit cells hung from (-200, 120), and a region over
/// exactly it: a wall down column 5, rows 0 to 4 - the gap is row 5.
fn warehouse() -> (World, hecs::Entity) {
    let mut world = World::new();
    world.spawn((
        at(0.0, 0.0),
        NavRegion {
            outline: vec![[-200.0, -120.0], [200.0, -120.0], [200.0, 120.0], [-200.0, 120.0]],
            agent_radius: 10.0,
            ..Default::default()
        },
    ));
    let mut tiles = vec![0u32; 60];
    for row in 0..5 {
        tiles[row * 10 + 5] = 1;
    }
    let map = world.spawn((
        at(-200.0, 120.0),
        Tilemap {
            tile_size: [T, T],
            layers: vec![TilemapLayer { name: "walls".into(), tiles, width: 10, height: 6, collision: true, ..Default::default() }],
            ..Default::default()
        },
    ));
    (world, map)
}

fn path(world: &World, from: (f32, f32), to: (f32, f32)) -> Vec<Vec2> {
    let graphs = navigation::graphs(world);
    navigation::find_path_in(&graphs, Vec2::new(from.0, from.1), Vec2::new(to.0, to.1)).expect("a path")
}

#[test]
fn a_path_goes_round_a_tile_wall_through_its_gap() {
    let (world, _) = warehouse();
    let p = path(&world, (-150.0, 100.0), (150.0, 100.0));
    let lowest = p.iter().map(|q| q.y).fold(f32::MAX, f32::min);
    // The wall is x 0..40, y -80..120; the gap below it, y -120..-80.
    assert!(lowest < -80.0, "the path must dip through the gap under the wall: {p:?}");
}

#[test]
fn digging_a_tile_out_opens_the_way() {
    let (world, map) = warehouse();
    let before = navigation::graphs(&world)[0].key();
    world.get::<&mut Tilemap>(map).unwrap().layers[0].set_tile(5, 2, None);
    assert_ne!(navigation::graphs(&world)[0].key(), before, "an edited layer rebakes the region");
    let p = path(&world, (-150.0, 20.0), (150.0, 20.0));
    let lowest = p.iter().map(|q| q.y).fold(f32::MAX, f32::min);
    assert!(lowest > -40.0, "the dug cell (y 0..40) is the way now, not the gap at the bottom: {p:?}");
}

/// A corridor with a crate in the middle of it.
fn corridor(obstacle: bool) -> (World, hecs::Entity) {
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), NavRegion { agent_radius: 10.0, ..Default::default() }));
    let crate_ = world.spawn((
        at(0.0, 0.0),
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 60.0, height: 60.0 }, ..Default::default() },
    ));
    if obstacle {
        world.insert_one(crate_, NavObstacle::default()).unwrap();
    }
    (world, crate_)
}

#[test]
fn a_dynamic_body_counts_only_as_a_nav_obstacle() {
    let (world, _) = corridor(false);
    assert_eq!(path(&world, (-200.0, 0.0), (200.0, 0.0)).len(), 2, "a plain dynamic body is not in the bake");
    let (world, _) = corridor(true);
    let p = path(&world, (-200.0, 0.0), (200.0, 0.0));
    assert!(p.len() > 2 && p.iter().any(|q| q.y.abs() > 30.0), "a nav_obstacle is walked round: {p:?}");
}

#[test]
fn a_moving_obstacle_rebakes_only_when_it_has_really_moved() {
    let (world, crate_) = corridor(true);
    let key = |w: &World| navigation::graphs(w)[0].key();
    let start = key(&world);
    world.get::<&mut Transform>(crate_).unwrap().position.x = 3.0;
    assert_eq!(key(&world), start, "a jitter inside its snap keeps the graph");
    world.get::<&mut Transform>(crate_).unwrap().position = Vec2::new(0.0, 200.0);
    assert_ne!(key(&world), start, "moved out of the way, it rebakes");
    assert_eq!(path(&world, (-200.0, 0.0), (200.0, 0.0)).len(), 2, "and the corridor is clear again");
}

/// A crate pushed onto a robot - or a robot that starts inside one's grown
/// outline - must not strand it: it walks out and on.
#[test]
fn an_agent_inside_an_obstacle_walks_out_of_it() {
    use jengine_core::navigation::NavAgent;
    let (mut world, _) = corridor(true);
    // The crate is 60 across at the origin, grown by 10: (25, 5) is inside.
    let agent = world.spawn((at(25.0, 5.0), NavAgent { target: Some([250.0, 0.0]), radius: 10.0, ..Default::default() }));
    for _ in 0..240 {
        navigation::tick_navigation(&mut world, 1.0 / 60.0);
    }
    let p = world.get::<&Transform>(agent).unwrap().position;
    assert!(p.distance(Vec2::new(250.0, 0.0)) < 10.0, "it walked out and reached its target: {p:?}");
}

/// The same where the obstacle meets the region's edge: the way out is
/// along the edge, not across it.
#[test]
fn an_agent_inside_an_obstacle_at_the_region_edge_walks_out() {
    use jengine_core::navigation::NavAgent;
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), NavRegion {
        outline: vec![[-620.0, -340.0], [620.0, -340.0], [620.0, 340.0], [-620.0, 340.0]],
        agent_radius: 18.0,
        ..Default::default()
    }));
    world.spawn((
        at(-560.0, 100.0),
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 56.0, height: 56.0 }, ..Default::default() },
        NavObstacle::default(),
    ));
    let agent = world.spawn((at(-600.0, 60.0), NavAgent { target: Some([0.0, 100.0]), radius: 18.0, speed: 150.0, ..Default::default() }));
    for _ in 0..600 {
        navigation::tick_navigation(&mut world, 1.0 / 60.0);
    }
    let p = world.get::<&Transform>(agent).unwrap().position;
    assert!(p.distance(Vec2::new(0.0, 100.0)) < 10.0, "it walked out and reached its target: {p:?}");
}
