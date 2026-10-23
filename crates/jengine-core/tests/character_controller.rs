//! `CharacterController2D`: a kinematic body moved by the velocity it asks
//! for, through rapier's character controller, in the frame order every
//! runtime runs (bodies, kinematic sync, characters, step, write-back).
//!
//! There was no way to move a character through a level: a dynamic body
//! tips, bounces and slides off slopes, and a kinematic one passes through
//! walls. Every platformer written on the engine would have had to raycast
//! its own collision.

#![cfg(feature = "physics")]

mod support;

use hecs::{Entity, World};
use jengine_core::ecs::components::{
    BodyType, CharacterController2D, Collider2D, ColliderShape, RigidBody2D, Transform,
};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;

const DT: f32 = 1.0 / 60.0;
const GRAVITY: f32 = -1400.0;

fn at(x: f32, y: f32) -> Transform {
    Transform { position: glam::Vec2::new(x, y), ..Default::default() }
}

fn solid(world: &mut World, x: f32, y: f32, shape: ColliderShape) -> Entity {
    world.spawn((
        at(x, y),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D { shape, ..Default::default() },
    ))
}

fn floor(world: &mut World, x: f32, y: f32, w: f32, h: f32) -> Entity {
    solid(world, x, y, ColliderShape::Box { width: w, height: h })
}

/// A 24 x 40 hero: a capsule, as a platformer's would be.
fn hero(world: &mut World, x: f32, y: f32, cc: CharacterController2D) -> Entity {
    world.spawn((
        at(x, y),
        RigidBody2D { body_type: BodyType::Kinematic, fixed_rotation: true, ..Default::default() },
        Collider2D { shape: ColliderShape::Capsule { half_height: 8.0, radius: 12.0 }, ..Default::default() },
        cc,
    ))
}

fn frame(world: &mut World, physics: &mut PhysicsWorld) {
    ensure_physics_bodies(world, physics);
    physics.sync_kinematic_from_ecs(world);
    physics.move_characters(world, DT);
    physics.step(DT);
    physics.sync_transforms_to_ecs(world);
}

/// Run `frames` frames with gravity and a horizontal speed, the way a
/// platformer's script would drive it.
fn run(world: &mut World, physics: &mut PhysicsWorld, e: Entity, vx: f32, frames: usize) {
    for _ in 0..frames {
        {
            let mut cc = world.get::<&mut CharacterController2D>(e).unwrap();
            let vy = if cc.grounded { -60.0 } else { cc.velocity[1] + GRAVITY * DT };
            cc.velocity = [vx, vy.max(-900.0)];
        }
        frame(world, physics);
    }
}

fn pos(world: &World, e: Entity) -> (f32, f32) {
    let t = world.get::<&Transform>(e).unwrap();
    (t.position.x, t.position.y)
}

fn state(world: &World, e: Entity) -> CharacterController2D {
    (*world.get::<&CharacterController2D>(e).unwrap()).clone()
}

#[test]
fn falls_lands_and_stands_on_the_ground() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let ground = floor(&mut world, 0.0, 0.0, 400.0, 20.0);
    let e = hero(&mut world, 0.0, 120.0, CharacterController2D::default());
    run(&mut world, &mut physics, e, 0.0, 90);
    let s = state(&world, e);
    assert!(s.grounded, "landed");
    assert_eq!(s.ground_entity, Some(ground.id() as u64));
    assert!(s.ground_normal[1] > 0.99, "flat ground: {:?}", s.ground_normal);
    // Floor top at 10, capsule bottom 20 below its centre, plus the skin.
    let y = pos(&world, e).1;
    assert!((y - 30.0).abs() < 1.5, "standing on the floor, not in it or above it: {y}");
    assert!(s.touching.contains(&(ground.id() as u64)));
}

#[test]
fn walls_stop_it_and_are_reported() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    floor(&mut world, 0.0, 0.0, 800.0, 20.0);
    floor(&mut world, 200.0, 60.0, 20.0, 100.0); // left face at x = 190
    let e = hero(&mut world, 0.0, 30.0, CharacterController2D::default());
    run(&mut world, &mut physics, e, 300.0, 120);
    let x = pos(&world, e).0;
    assert!(x < 190.0 - 12.0 + 0.1 && x > 170.0, "stopped at the wall: {x}");
    assert_eq!(state(&world, e).wall, 1, "the wall is on its right");
    assert!(state(&world, e).moved[0].abs() < 1.0, "not moving into it");
}

#[test]
fn climbs_a_gentle_slope_but_not_a_steep_one() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    floor(&mut world, 0.0, 0.0, 2000.0, 20.0);
    // A 30 degree ramp starting at x = 100, and a 70 degree one at x = -100.
    let tan30 = 30f32.to_radians().tan();
    solid(&mut world, 0.0, 0.0, ColliderShape::Triangle { a: [100.0, 10.0], b: [400.0, 10.0], c: [400.0, 10.0 + 300.0 * tan30] });
    let tan70 = 70f32.to_radians().tan();
    solid(&mut world, 0.0, 0.0, ColliderShape::Triangle { a: [-100.0, 10.0], b: [-200.0, 10.0], c: [-200.0, 10.0 + 100.0 * tan70] });
    let e = hero(&mut world, 0.0, 30.0, CharacterController2D::default());
    run(&mut world, &mut physics, e, 200.0, 90);
    let (x, y) = pos(&world, e);
    // Sliding along the ramp keeps the part of the speed along it, so the
    // climb is slower than the walk; a game wanting full speed uphill
    // steers along the slope (a platformer's hero code does).
    assert!(x > 200.0 && y > 30.0 + (x - 100.0 - 12.0) * tan30 - 15.0, "walked up the ramp: {x}, {y}");
    assert!(state(&world, e).grounded, "on the ramp");

    let e2 = hero(&mut world, 0.0, 30.0, CharacterController2D::default());
    run(&mut world, &mut physics, e2, -200.0, 90);
    let (x2, y2) = pos(&world, e2);
    assert!(x2 > -115.0 && y2 < 60.0, "the steep one is a wall: {x2}, {y2}");
}

#[test]
fn steps_up_a_ledge_lower_than_step_height() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    floor(&mut world, 0.0, 0.0, 2000.0, 20.0);
    floor(&mut world, 300.0, 16.0, 200.0, 12.0); // an 12-unit step from x = 200
    let cc = CharacterController2D { step_height: 16.0, ..Default::default() };
    let e = hero(&mut world, 0.0, 30.0, cc);
    run(&mut world, &mut physics, e, 200.0, 90);
    let (x, y) = pos(&world, e);
    assert!(x > 250.0 && y > 38.0, "up the step: {x}, {y}");

    let tall = hero(&mut world, 0.0, 30.0, CharacterController2D::default());
    run(&mut world, &mut physics, tall, 200.0, 90);
    assert!(pos(&world, tall).0 < 200.0, "without step_height the step is a wall");
}

#[test]
fn one_way_platforms_are_jumped_through_landed_on_and_dropped_through() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    floor(&mut world, 0.0, 0.0, 800.0, 20.0);
    let ledge = world.spawn((
        at(0.0, 100.0),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 200.0, height: 10.0 }, one_way: true, ..Default::default() },
    ));
    let e = hero(&mut world, 0.0, 30.0, CharacterController2D::default());
    run(&mut world, &mut physics, e, 0.0, 10);
    // Jump: up through the ledge from below.
    world.get::<&mut CharacterController2D>(e).unwrap().velocity = [0.0, 700.0];
    frame(&mut world, &mut physics);
    for _ in 0..120 {
        {
            let mut cc = world.get::<&mut CharacterController2D>(e).unwrap();
            let vy = if cc.grounded { -60.0 } else { cc.velocity[1] + GRAVITY * DT };
            cc.velocity = [0.0, vy];
        }
        frame(&mut world, &mut physics);
    }
    let s = state(&world, e);
    assert_eq!(s.ground_entity, Some(ledge.id() as u64), "landed on the ledge from above");
    assert!((pos(&world, e).1 - 125.0).abs() < 2.0, "{}", pos(&world, e).1);

    world.get::<&mut CharacterController2D>(e).unwrap().drop_through = true;
    run(&mut world, &mut physics, e, 0.0, 20);
    world.get::<&mut CharacterController2D>(e).unwrap().drop_through = false;
    run(&mut world, &mut physics, e, 0.0, 60);
    assert!(pos(&world, e).1 < 40.0, "dropped through to the floor: {:?}", pos(&world, e));
}

#[test]
fn rides_a_moving_platform_and_bumps_its_head() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let lift = world.spawn((
        at(0.0, 0.0),
        RigidBody2D { body_type: BodyType::Kinematic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 200.0, height: 20.0 }, ..Default::default() },
    ));
    let e = hero(&mut world, 0.0, 40.0, CharacterController2D::default());
    run(&mut world, &mut physics, e, 0.0, 30);
    let (x0, y0) = pos(&world, e);
    // The platform slides right 2 units a frame for a second.
    for _ in 0..60 {
        world.get::<&mut Transform>(lift).unwrap().position.x += 2.0;
        run(&mut world, &mut physics, e, 0.0, 1);
    }
    let (x1, y1) = pos(&world, e);
    assert!((x1 - x0 - 120.0).abs() < 6.0, "carried along: {x0} -> {x1}");
    assert!((y1 - y0).abs() < 2.0);

    // A ceiling 60 above: a jump bumps it.
    floor(&mut world, x1, y1 + 60.0, 200.0, 20.0);
    run(&mut world, &mut physics, e, 0.0, 2);
    world.get::<&mut CharacterController2D>(e).unwrap().velocity = [0.0, 600.0];
    frame(&mut world, &mut physics);
    let mut bumped = false;
    for _ in 0..10 {
        bumped |= state(&world, e).ceiling;
        {
            let mut cc = world.get::<&mut CharacterController2D>(e).unwrap();
            let vy = cc.velocity[1] + GRAVITY * DT;
            cc.velocity = [0.0, vy];
        }
        frame(&mut world, &mut physics);
    }
    assert!(bumped, "the ceiling was reported");
    assert!(pos(&world, e).1 < y1 + 60.0 - 10.0 - 20.0 + 1.0, "and not passed");
}

#[test]
fn reports_the_sensors_it_touches_and_pushes_crates() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    floor(&mut world, 0.0, 0.0, 2000.0, 20.0);
    let coin = world.spawn((
        at(100.0, 30.0),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 8.0 }, is_sensor: true, ..Default::default() },
    ));
    let crate_ = world.spawn((
        at(300.0, 30.0),
        RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 40.0, height: 40.0 }, friction: 0.1, ..Default::default() },
    ));
    let cc = CharacterController2D { push_mass: 5.0, ..Default::default() };
    let e = hero(&mut world, 0.0, 30.0, cc);
    let mut saw_coin = false;
    for _ in 0..150 {
        run(&mut world, &mut physics, e, 150.0, 1);
        saw_coin |= state(&world, e).touching.contains(&(coin.id() as u64));
    }
    assert!(saw_coin, "walked through the coin and was told");
    assert!(pos(&world, crate_).0 > 320.0, "the crate was pushed: {:?}", pos(&world, crate_));
}


#[test]
fn rides_a_lift_up_and_down() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let lift = world.spawn((
        at(0.0, 0.0),
        RigidBody2D { body_type: BodyType::Kinematic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 200.0, height: 20.0 }, ..Default::default() },
    ));
    let e = hero(&mut world, 0.0, 40.0, CharacterController2D::default());
    run(&mut world, &mut physics, e, 0.0, 30);
    let gap = pos(&world, e).1 - world.get::<&Transform>(lift).unwrap().position.y;
    // Rising: carried exactly, and never off it.
    for _ in 0..60 {
        world.get::<&mut Transform>(lift).unwrap().position.y += 3.0;
        run(&mut world, &mut physics, e, 0.0, 1);
        let ly = world.get::<&Transform>(lift).unwrap().position.y;
        // Within one frame of the platform's own step: it moves, and the
        // rider is carried on the physics frame that follows.
        assert!((pos(&world, e).1 - ly - gap).abs() <= 3.01, "carried up: {:?} on {ly}", pos(&world, e));
        assert!(state(&world, e).grounded, "and still standing on it");
    }
    // Falling, faster than the rider would fall by itself: it stays on,
    // because the snap reaches as far as the ground moved.
    let mut landed = 0;
    for _ in 0..60 {
        world.get::<&mut Transform>(lift).unwrap().position.y -= 3.0;
        run(&mut world, &mut physics, e, 0.0, 1);
        landed += state(&world, e).grounded as u32;
        let ly = world.get::<&Transform>(lift).unwrap().position.y;
        let over = pos(&world, e).1 - ly - gap;
        assert!((-1.0..3.01).contains(&over), "goes down with it: {over}");
    }
    assert!(landed >= 58, "on it the whole way down ({landed} of 60)");
}

#[test]
fn collision_layers_decide_what_stops_it_but_not_what_it_touches() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    floor(&mut world, 0.0, 0.0, 800.0, 20.0);
    // A post on layer 4, which the hero's filter leaves out.
    let post = world.spawn((
        at(200.0, 40.0),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D {
            shape: ColliderShape::Box { width: 20.0, height: 60.0 },
            collision_membership: 4,
            collision_filter: 0xFFFF,
            ..Default::default()
        },
    ));
    let e = world.spawn((
        at(0.0, 30.0),
        RigidBody2D { body_type: BodyType::Kinematic, fixed_rotation: true, ..Default::default() },
        Collider2D {
            shape: ColliderShape::Capsule { half_height: 8.0, radius: 12.0 },
            collision_membership: 1,
            collision_filter: !4,
            ..Default::default()
        },
        CharacterController2D::default(),
    ));
    let mut touched = false;
    for _ in 0..120 {
        run(&mut world, &mut physics, e, 200.0, 1);
        touched |= state(&world, e).touching.contains(&(post.id() as u64));
    }
    assert!(pos(&world, e).0 > 260.0, "walked straight through it: {:?}", pos(&world, e));
    assert!(touched, "and was told it touched it");
}

/// Each runtime runs its own physics frame, so a runtime that forgets
/// `move_characters` leaves every character frozen there and nowhere else.
#[test]
fn every_runtime_moves_characters_between_the_kinematic_sync_and_the_step() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for file in [
        "crates/jengine-runtime/src/wgpu_scene_runner.rs",
        "crates/jengine-editor-viewport/src/lib.rs",
        "crates/jengine-web/src/lib.rs",
        "packages/jengine-physics/src/lib.rs",
    ] {
        let src = support::with_frame_draw(&std::fs::read_to_string(root.join(file)).unwrap());
        let sync = src.find("sync_kinematic_from_ecs(").unwrap_or_else(|| panic!("{file}: no kinematic sync"));
        let moved = src[sync..].find("move_characters(").map(|i| i + sync);
        let step = src[sync..].find(".step(dt)").map(|i| i + sync).unwrap_or(usize::MAX);
        assert!(moved.is_some_and(|m| m < step), "{file} must call move_characters after the kinematic sync, before the step");
    }
}

mod tilemaps {
    use super::*;
    use jengine_core::ecs::components::TilemapCollider;
    use jengine_core::physics::merge_tiles;
    use jengine_core::tilemap::{Tilemap, TilemapLayer};

    fn layer(name: &str, rows: &[&str], collision: bool) -> TilemapLayer {
        let width = rows[0].len() as u32;
        TilemapLayer {
            name: name.into(),
            tiles: rows.iter().flat_map(|r| r.chars().map(|c| if c == '#' { 1 } else { 0 })).collect(),
            width,
            height: rows.len() as u32,
            visible: true,
            collision,
            ..Default::default()
        }
    }

    fn map(layers: Vec<TilemapLayer>) -> Tilemap {
        Tilemap { tile_size: [32.0, 32.0], layers, tilesets: vec![], source: None, z_order: 0, sort_layer: None, ..Default::default() }
    }

    #[test]
    fn tiles_merge_into_few_boxes() {
        let rows = ["....", "##..", "##..", "####"];
        let filled = |x: u32, y: u32| rows[y as usize].as_bytes()[x as usize] == b'#';
        let mut boxes = merge_tiles(4, 4, filled, false);
        boxes.sort();
        assert_eq!(boxes, vec![(0, 1, 2, 2), (0, 3, 4, 1)], "a 2x2 block and the floor under it");
        assert_eq!(merge_tiles(4, 4, filled, true).len(), 3, "row by row");
    }

    #[test]
    fn a_character_walks_on_a_tilemap_and_through_its_one_way_layer() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
        // Origin at the top-left: row 0 spans y 0..-32. The floor is row 5
        // (y -160..-192); a one-way ledge is row 2 (y -64..-96).
        let ground = layer("ground", &["..........", "..........", "..........", "..........", "..........", "##########"], false);
        let ledges = layer("ledges", &["..........", "..........", "..####....", "..........", "..........", ".........."], false);
        let tm = world.spawn((
            at(0.0, 0.0),
            map(vec![ground, ledges]),
            TilemapCollider { solid: vec!["ground".into()], one_way: vec!["ledges".into()] },
        ));
        let e = hero(&mut world, 40.0, -100.0, CharacterController2D::default());
        run(&mut world, &mut physics, e, 0.0, 60);
        assert_eq!(state(&world, e).ground_entity, Some(tm.id() as u64), "stands on the tilemap");
        assert!((pos(&world, e).1 - (-160.0 + 20.5)).abs() < 1.5, "{:?}", pos(&world, e));
        assert_eq!(physics.tilemap_box_count(tm.id() as u64), Some(2), "one floor box, one ledge box");

        // Jump up through the one-way ledge (x 64..192) and land on it.
        world.get::<&mut Transform>(e).unwrap().position.x = 120.0;
        run(&mut world, &mut physics, e, 0.0, 5);
        world.get::<&mut CharacterController2D>(e).unwrap().velocity = [0.0, 800.0];
        frame(&mut world, &mut physics);
        run(&mut world, &mut physics, e, 0.0, 90);
        assert!((pos(&world, e).1 - (-64.0 + 20.5)).abs() < 1.5, "on the ledge: {:?}", pos(&world, e));

        // Break the ledge tile under it: the body is rebuilt and it falls.
        {
            let mut t = world.get::<&mut Tilemap>(tm).unwrap();
            for x in 2..6 {
                t.layers[1].set_tile(x, 2, None);
            }
        }
        run(&mut world, &mut physics, e, 0.0, 60);
        assert!(pos(&world, e).1 < -130.0, "fell to the floor: {:?}", pos(&world, e));
        assert_eq!(physics.tilemap_box_count(tm.id() as u64), Some(1));
    }

    #[test]
    fn a_collision_layer_collides_without_being_named() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
        let tm = world.spawn((at(0.0, 0.0), map(vec![layer("Collision", &["....", "####"], true)])));
        let e = hero(&mut world, 60.0, 0.0, CharacterController2D::default());
        run(&mut world, &mut physics, e, 0.0, 60);
        assert_eq!(state(&world, e).ground_entity, Some(tm.id() as u64));
        world.despawn(tm).unwrap();
        run(&mut world, &mut physics, e, 0.0, 30);
        assert!(!state(&world, e).grounded, "the body went with its tilemap");
    }
}



/// A collider removed between two steps - a coin collected, an enemy
/// stomped - must not bring the next character move down. The query
/// pipeline only learns of removals at the next `step`, and the refit
/// `move_characters` runs for moved platforms read every leaf it held: the
/// editor panicked with rapier's "No element at index" the frame after
/// a platformer's hero picked something up while a platform was moving.
#[test]
fn a_collider_removed_between_steps_does_not_break_the_next_move() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let lift = world.spawn((
        at(0.0, 0.0),
        RigidBody2D { body_type: BodyType::Kinematic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 200.0, height: 20.0 }, ..Default::default() },
    ));
    let coin = floor(&mut world, 300.0, 100.0, 20.0, 20.0);
    let e = hero(&mut world, 0.0, 40.0, CharacterController2D::default());
    run(&mut world, &mut physics, e, 0.0, 30);
    world.despawn(coin).unwrap();
    for _ in 0..10 {
        world.get::<&mut Transform>(lift).unwrap().position.x += 2.0;
        run(&mut world, &mut physics, e, 0.0, 1);
    }
    assert!(state(&world, e).grounded, "still on the lift");
}

/// A platform that jumps somewhere in one frame - a crumbling platform sent
/// out of the level, a lift reset to its start - has not moved *with* its
/// rider, and must not take the rider along. A platformer's crumbling
/// platforms are parked 100,000 units down, and the hero standing on one
/// was carried there and died below the level.
#[test]
fn a_platform_that_teleports_away_leaves_its_rider_behind() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let lift = world.spawn((
        at(0.0, 0.0),
        RigidBody2D { body_type: BodyType::Kinematic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 200.0, height: 20.0 }, ..Default::default() },
    ));
    let e = hero(&mut world, 0.0, 40.0, CharacterController2D::default());
    run(&mut world, &mut physics, e, 0.0, 30);
    assert!(state(&world, e).grounded);
    let before = pos(&world, e).1;
    world.get::<&mut Transform>(lift).unwrap().position.y -= 100_000.0;
    run(&mut world, &mut physics, e, 0.0, 1);
    let after = pos(&world, e).1;
    assert!(before - after < 50.0, "carried down with a teleport: {before} -> {after}");
    assert!(!state(&world, e).grounded, "and nothing under it now");
}
