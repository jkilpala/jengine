//! A colliding tilemap costs what it changed, not what it is.
//!
//! `PhysicsWorld::sync_tilemap_colliders` used to hash **every tile of every
//! colliding layer every frame** to decide whether to rebuild, and then, on
//! any difference at all, remove the body and rebuild every collider on the
//! map. For a world of a million tiles that is a per-frame walk of a million
//! cells to discover that nothing moved, and one pick swing rebuilding the
//! whole level - so a game that digs, farms or builds was not possible on the
//! engine whatever else it had.
//!
//! The fix is two counters on `TilemapLayer` (a total, and one per
//! `CHUNK_TILES`-square chunk) and colliders grouped per chunk. This file is
//! the proof, and it has to check both halves, because they fail differently:
//! a detection bug makes the engine slow, and a rebuild bug makes it *wrong*.
//! Every cost assertion here is a rebuild count, because the resulting
//! colliders are identical either way - nothing else can see the difference.

#![cfg(feature = "physics")]

use hecs::{Entity, World};
use jengine_core::ecs::components::{
    BodyType, CharacterController2D, Collider2D, ColliderShape, RigidBody2D, TilemapCollider, Transform,
};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;
use jengine_core::tilemap::{CHUNK_TILES, Tilemap, TilemapLayer};

const DT: f32 = 1.0 / 60.0;

fn at(x: f32, y: f32) -> Transform {
    Transform { position: glam::Vec2::new(x, y), ..Default::default() }
}

/// A `w` x `h` layer, solid everywhere `filled` says so.
fn layer(name: &str, w: u32, h: u32, filled: impl Fn(u32, u32) -> bool) -> TilemapLayer {
    TilemapLayer {
        name: name.into(),
        tiles: (0..w * h).map(|i| if filled(i % w, i / w) { 1 } else { 0 }).collect(),
        width: w,
        height: h,
        visible: true,
        collision: false,
        ..Default::default()
    }
}

fn map(layers: Vec<TilemapLayer>) -> Tilemap {
    Tilemap { tile_size: [32.0, 32.0], layers, tilesets: vec![], source: None, z_order: 0, sort_layer: None, ..Default::default() }
}

fn spawn_map(world: &mut World, tm: Tilemap, solid: &[&str]) -> Entity {
    let spec = TilemapCollider { solid: solid.iter().map(|s| (*s).to_string()).collect(), one_way: vec![] };
    world.spawn((at(0.0, 0.0), tm, spec))
}

/// One tile of a layer, through the only path that keeps the colliders right.
fn set(world: &World, e: Entity, layer_index: usize, x: u32, y: u32, tile: Option<u32>) {
    world.get::<&mut Tilemap>(e).unwrap().layers[layer_index].set_tile(x, y, tile);
}

// ---------------------------------------------------------------- detection

/// The claim that matters most: standing still is free. Not "cheaper" - the
/// sync reads one integer per colliding layer and touches no tile at all.
#[test]
fn an_idle_frame_rebuilds_nothing_however_large_the_map() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    // 256 x 256 = 65,536 tiles: the old code hashed all of them every frame.
    let e = spawn_map(&mut world, map(vec![layer("rock", 256, 256, |_, y| y > 200)]), &["rock"]);

    ensure_physics_bodies(&world, &mut physics);
    let after_build = physics.tilemap_chunk_rebuilds();
    assert_eq!(after_build, 64, "the first sync builds all 8x8 chunks");
    assert!(physics.tilemap_box_count(e.id() as u64).unwrap() > 0, "and it has colliders");

    for _ in 0..600 {
        ensure_physics_bodies(&world, &mut physics);
    }
    assert_eq!(
        physics.tilemap_chunk_rebuilds(),
        after_build,
        "ten seconds of frames over an unchanged map must rebuild nothing"
    );
}

/// And one edit costs one chunk, not the map. This is the number that decides
/// whether a game can let the player dig.
#[test]
fn one_edited_tile_rebuilds_one_chunk() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let e = spawn_map(&mut world, map(vec![layer("rock", 256, 256, |_, _| true)]), &["rock"]);
    ensure_physics_bodies(&world, &mut physics);
    let built = physics.tilemap_chunk_rebuilds();

    set(&world, e, 0, 100, 100, None);
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_chunk_rebuilds() - built, 1, "one tile, one chunk");

    // A whole pick swing - nine tiles in one chunk - is still one rebuild,
    // because the sync happens once a frame however many tiles changed.
    let before = physics.tilemap_chunk_rebuilds();
    for dx in 0..3 {
        for dy in 0..3 {
            set(&world, e, 0, 40 + dx, 40 + dy, None);
        }
    }
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_chunk_rebuilds() - before, 1, "nine tiles in one chunk is one rebuild");

    // An edit straddling a chunk border costs both, and no more.
    let before = physics.tilemap_chunk_rebuilds();
    set(&world, e, 0, CHUNK_TILES - 1, 8, None);
    set(&world, e, 0, CHUNK_TILES, 8, None);
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_chunk_rebuilds() - before, 2, "two chunks touched, two rebuilt");
}

/// Painting the tile that is already there is not a change. A brush held down
/// sends one `set_tile` a frame, and if each one rebuilt a chunk the cheapest
/// possible edit would cost the same as the most expensive.
#[test]
fn writing_the_same_tile_again_costs_nothing() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let e = spawn_map(&mut world, map(vec![layer("rock", 64, 64, |_, _| true)]), &["rock"]);
    ensure_physics_bodies(&world, &mut physics);
    let built = physics.tilemap_chunk_rebuilds();

    for _ in 0..120 {
        set(&world, e, 0, 10, 10, Some(1));
        ensure_physics_bodies(&world, &mut physics);
    }
    assert_eq!(physics.tilemap_chunk_rebuilds(), built, "painting rock onto rock changed nothing");

    // Clearing an already-empty cell is the same non-event, whichever of the
    // three spellings of "empty" the caller used.
    set(&world, e, 0, 10, 10, None);
    ensure_physics_bodies(&world, &mut physics);
    let after = physics.tilemap_chunk_rebuilds();
    for spelling in [None, Some(0)] {
        set(&world, e, 0, 10, 10, spelling);
        ensure_physics_bodies(&world, &mut physics);
    }
    assert_eq!(physics.tilemap_chunk_rebuilds(), after, "clearing an empty cell is not a change");
}

/// Anything about the map that is *not* tiles is compared whole, because it
/// is a handful of integers - and a difference has to rebuild, or a moved
/// tilemap would collide where it used to be.
#[test]
fn moving_or_reshaping_the_map_rebuilds_all_of_it() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let e = spawn_map(&mut world, map(vec![layer("rock", 64, 64, |_, _| true)]), &["rock"]);
    ensure_physics_bodies(&world, &mut physics);
    let chunks = 2 * 2;
    assert_eq!(physics.tilemap_chunk_rebuilds(), chunks);

    world.get::<&mut Transform>(e).unwrap().position.x = 500.0;
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_chunk_rebuilds(), 2 * chunks, "moved: every chunk is somewhere else now");

    world.get::<&mut Tilemap>(e).unwrap().tile_size = [16.0, 16.0];
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_chunk_rebuilds(), 3 * chunks, "a different tile size resizes every box");

    // A layer that starts colliding brings its own chunks and rebuilds the
    // ones already there, because the record's layer list changed shape.
    world.get::<&mut Tilemap>(e).unwrap().layers.push(layer("ledges", 64, 64, |_, y| y == 3));
    world.get::<&mut TilemapCollider>(e).unwrap().solid.push("ledges".into());
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_chunk_rebuilds(), 3 * chunks + 2 * chunks, "both layers rebuilt");
}

/// A layer filled in bulk has no `set_tile` to bump anything, so the two
/// wholesale paths exist - and the one that silently does not work is
/// documented as not working rather than looking like it does.
#[test]
fn a_bulk_fill_marks_the_layer_and_a_raw_write_does_not() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let e = spawn_map(&mut world, map(vec![layer("rock", 64, 64, |_, _| false)]), &["rock"]);
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_box_count(e.id() as u64), Some(0), "an empty layer has no boxes");
    let built = physics.tilemap_chunk_rebuilds();

    // `tiles` written directly: the colliders stay as they were. This is the
    // one hole in the scheme and the reason `tiles_mut` exists.
    world.get::<&mut Tilemap>(e).unwrap().layers[0].tiles = vec![1; 64 * 64];
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_chunk_rebuilds(), built, "a raw write bumps nothing");
    assert_eq!(physics.tilemap_box_count(e.id() as u64), Some(0), "so the map still collides as empty");

    // Said out loud, it is fixed on the next frame.
    world.get::<&mut Tilemap>(e).unwrap().layers[0].touch_all();
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_chunk_rebuilds() - built, 4, "every chunk rebuilt");
    assert!(physics.tilemap_box_count(e.id() as u64).unwrap() > 0, "and now it is solid");

    // `tiles_mut` says it for you, which is what a generator should use.
    let before = physics.tilemap_chunk_rebuilds();
    world.get::<&mut Tilemap>(e).unwrap().layers[0].tiles_mut().fill(0);
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_chunk_rebuilds() - before, 4);
    assert_eq!(physics.tilemap_box_count(e.id() as u64), Some(0), "emptied");
}

/// Replacing a layer rather than editing it is the hole a plain counter would
/// leave: the new layer has the same name and size and a total of zero, which
/// is what the physics record last saw. Nothing in the repo does this to a
/// *colliding* map today (a roguelike package rebuilds its layers every turn,
/// and they carry no collider), so this is the test that keeps it that way -
/// the failure mode is a level that draws as one map and collides as another.
#[test]
fn a_replaced_layer_does_not_pass_for_the_one_it_replaced() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let e = spawn_map(&mut world, map(vec![layer("rock", 64, 64, |_, _| true)]), &["rock"]);
    ensure_physics_bodies(&world, &mut physics);
    let solid_boxes = physics.tilemap_box_count(e.id() as u64).unwrap();
    assert!(solid_boxes > 0);

    // Same name, same size, never touched by `set_tile` - and empty.
    world.get::<&mut Tilemap>(e).unwrap().layers = vec![layer("rock", 64, 64, |_, _| false)];
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_box_count(e.id() as u64), Some(0), "it collides as the map it now is");

    // And back the other way, so this cannot pass by always rebuilding.
    world.get::<&mut Tilemap>(e).unwrap().layers = vec![layer("rock", 64, 64, |_, _| true)];
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_box_count(e.id() as u64), Some(solid_boxes));
    let built = physics.tilemap_chunk_rebuilds();
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.tilemap_chunk_rebuilds(), built, "and the frame after is idle again");
}

// -------------------------------------------------------------- correctness


/// The cost is only worth having if the colliders are the same ones. A world
/// edited tile by tile must end up with exactly the geometry a world built
/// fresh from the finished map has - per chunk, box for box.
#[test]
fn an_edited_map_ends_up_with_the_colliders_a_fresh_one_has() {
    // A shape with something in it: a floor, a pillar, a cave, a diagonal -
    // so the merge has runs of every length to make, in every chunk.
    let final_state = |x: u32, y: u32| {
        let cave = (40..60).contains(&x) && (30..44).contains(&y);
        let solid = y > 28 || (x.is_multiple_of(17) && y > 4) || x == y;
        solid && !cave
    };
    let (w, h) = (100u32, 70u32);

    // Built from the finished map in one go.
    let mut fresh = World::new();
    let mut fresh_physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let a = spawn_map(&mut fresh, map(vec![layer("rock", w, h, final_state)]), &["rock"]);
    ensure_physics_bodies(&fresh, &mut fresh_physics);

    // Built solid, then carved down to the same thing a tile at a time.
    let mut carved = World::new();
    let mut carved_physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let b = spawn_map(&mut carved, map(vec![layer("rock", w, h, |_, _| true)]), &["rock"]);
    ensure_physics_bodies(&carved, &mut carved_physics);
    for y in 0..h {
        for x in 0..w {
            if !final_state(x, y) {
                set(&carved, b, 0, x, y, None);
                // A frame between edits, as a game would have: this is what
                // makes it an incremental rebuild rather than one big one.
                ensure_physics_bodies(&carved, &mut carved_physics);
            }
        }
    }

    assert_eq!(
        carved_physics.tilemap_box_count(b.id() as u64),
        fresh_physics.tilemap_box_count(a.id() as u64),
        "the same map, however it got there"
    );
    assert!(
        carved_physics.tilemap_chunk_rebuilds() > fresh_physics.tilemap_chunk_rebuilds(),
        "sanity: the carved one really did rebuild repeatedly"
    );
}

/// The trade the chunking makes: merged boxes stop at chunk borders. Worth
/// stating as a number, because it is the one thing that got *worse*.
#[test]
fn merged_boxes_stop_at_chunk_borders() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    // One flat row, four chunks wide.
    let w = CHUNK_TILES * 4;
    let e = spawn_map(&mut world, map(vec![layer("rock", w, 1, |_, _| true)]), &["rock"]);
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(
        physics.tilemap_box_count(e.id() as u64),
        Some(4),
        "one box per chunk, where a whole-layer merge gave one - the cost of a bounded rebuild"
    );
}

/// ...and the reason that trade is acceptable: a character does not catch on
/// the seams. Abutting cuboids already existed wherever runs did not line up
/// (`merge_tiles` only stacks rows with identical spans), so this was never a
/// seam-free world - but a floor is exactly where it would be noticed.
#[test]
fn a_character_walks_across_chunk_seams() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let w = CHUNK_TILES * 4;
    spawn_map(&mut world, map(vec![layer("rock", w, 8, |_, y| y == 6)]), &["rock"]);
    let e = world.spawn((
        at(16.0, -100.0),
        RigidBody2D { body_type: BodyType::Kinematic, fixed_rotation: true, ..Default::default() },
        Collider2D { shape: ColliderShape::Capsule { half_height: 8.0, radius: 12.0 }, ..Default::default() },
        CharacterController2D::default(),
    ));

    // Walk the whole width at a steady speed, with gravity holding it down.
    let mut last_x = 16.0f32;
    let mut stalls = 0;
    for _ in 0..600 {
        world.get::<&mut CharacterController2D>(e).unwrap().velocity = [400.0, -1400.0 * DT];
        ensure_physics_bodies(&world, &mut physics);
        physics.sync_kinematic_from_ecs(&world);
        physics.move_characters(&mut world, DT);
        physics.step(DT);
        physics.sync_transforms_to_ecs(&mut world);
        let x = world.get::<&Transform>(e).unwrap().position.x;
        // Moving at 400 u/s it covers ~6.7 units a frame; anything under a
        // unit is a catch, not a slow frame.
        if x - last_x < 1.0 && x < (w as f32 * 32.0) - 64.0 {
            stalls += 1;
        }
        last_x = x;
    }
    assert_eq!(stalls, 0, "caught on a chunk seam {stalls} times, ending at {last_x}");
    assert!(last_x > (w as f32 * 32.0) - 100.0, "crossed all four chunks: {last_x}");
}

/// A box asleep on a tile that is dug out has to fall. Rapier does not wake a
/// sleeping body when a collider it was resting on is removed unless the
/// removal asks for it, so this is about one boolean - and without it a
/// digging game leaves furniture hanging in the air.
#[test]
fn a_body_resting_on_a_dug_out_tile_wakes_and_falls() {
    let mut world = World::new();
    // `new` multiplies gravity by the length unit, so -9.81 here is the 981
    // units/s² a centimetre-scaled world actually falls at. Ask for -1400 and
    // the crate crosses the whole 32-unit floor in a step and tunnels.
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    let e = spawn_map(&mut world, map(vec![layer("rock", 64, 64, |_, y| y == 10)]), &["rock"]);
    // Row 10 spans y -320..-352; drop a crate onto it and let it settle.
    let crate_e = world.spawn((
        at(100.0, -260.0),
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 24.0, height: 24.0 }, ..Default::default() },
    ));
    for _ in 0..240 {
        ensure_physics_bodies(&world, &mut physics);
        physics.step(DT);
        physics.sync_transforms_to_ecs(&mut world);
    }
    let resting = world.get::<&Transform>(crate_e).unwrap().position.y;
    assert!(resting > -330.0, "it should be sitting on row 10: {resting}");

    // Dig out the three tiles under it. x 100 is tile 3.
    for x in 2..5 {
        set(&world, e, 0, x, 10, None);
    }
    for _ in 0..120 {
        ensure_physics_bodies(&world, &mut physics);
        physics.step(DT);
        physics.sync_transforms_to_ecs(&mut world);
    }
    let after = world.get::<&Transform>(crate_e).unwrap().position.y;
    assert!(after < resting - 200.0, "it was asleep on ground that is gone: {resting} -> {after}");
}

/// What it costs on the scale the fix was for. Ignored because a timing is not
/// a pass/fail, but run it (`--release --ignored`) before changing any of this:
/// the numbers are the whole argument.
#[test]
#[ignore = "a measurement, not an assertion"]
fn the_cost_of_a_million_tiles() {
    let n = 1000u32;
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let e = spawn_map(&mut world, map(vec![layer("rock", n, n, |x, y| (x / 7 + y / 5) % 3 != 0)]), &["rock"]);

    let t = std::time::Instant::now();
    ensure_physics_bodies(&world, &mut physics);
    let build = t.elapsed();
    let boxes = physics.tilemap_box_count(e.id() as u64).unwrap();

    let t = std::time::Instant::now();
    for _ in 0..600 {
        ensure_physics_bodies(&world, &mut physics);
    }
    let idle = t.elapsed() / 600;

    let t = std::time::Instant::now();
    for i in 0..600 {
        set(&world, e, 0, 300 + i % 3, 300, None);
        ensure_physics_bodies(&world, &mut physics);
    }
    let edit = t.elapsed() / 600;

    println!(
        "{n}x{n} = {} tiles, {boxes} boxes\n  first build {build:?}\n  idle frame  {idle:?}\n  edit frame  {edit:?}",
        n * n
    );
    // The old code's idle frame was a walk of every tile; this one reads one
    // integer per layer. If that ever stops being true this is where it shows.
    assert!(idle < build / 100, "an idle frame must not be within 1% of a full build: {idle:?} vs {build:?}");
}
