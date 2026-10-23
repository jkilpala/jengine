//! **A spawned entity is named by its handle, and every ray sees the land**
//! (Oct 2026). Found by the showcase's Crater Isle, whose bombs went off the
//! moment they were dropped, in three ways at once:
//!
//! - A script holds a `spawn_prefab` handle (a million and up), and a
//!   raycast hit or a contact reported the entity's *real* id - so a bomb
//!   whose ray started inside its own collider could not tell it was looking
//!   at itself. Results now name a spawned entity by its handle, natively
//!   (`spawn_aliases::script_id`) and in the browser hosts.
//! - `get_velocity(handle)` read zero natively (the physics velocities are
//!   keyed by real id) and in the browsers (their rows were too).
//! - The browsers' ray snapshot held `Collider2D`s only, so a ray in the
//!   editor or a web build went straight through tile walls and carved land
//!   that a native ray stopped at. It builds those with the physics sync's
//!   own functions now.
//!
//! The browser preamble's handles also started at the entity count - inside
//! the range real ids live in - so a handle could *be* another entity's id.
//! They start at a million now, as native's do.

#![cfg(feature = "physics")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::carve::{Carvable, apply_carves};
use jengine_core::ecs::components::{
    BodyType, Collider2D, ColliderShape, RigidBody2D, SpriteComponent, TilemapCollider, Transform,
};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;
use jengine_core::physics::ray::RaySnapshot;
use jengine_core::scripting::bridge::BridgeReadSnapshot;
use jengine_core::scripting::spawn_aliases;
use jengine_core::tilemap::{TileOrientation, Tilemap, TilemapLayer};

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

/// Tile walls (square and isometric) and a carved hill, as ECS and as a
/// live physics world.
fn land() -> (World, PhysicsWorld) {
    let mut world = World::new();
    let layer = |tiles: Vec<u32>| TilemapLayer { name: "rock".into(), tiles, width: 6, height: 4, ..Default::default() };
    let walls: Vec<u32> = (0..24).map(|i| u32::from(i % 6 == 0 || i / 6 == 3 || i == 9)).collect();
    let solid = || TilemapCollider { solid: vec!["rock".into()], one_way: vec![] };
    world.spawn((at(-400.0, 300.0), Tilemap { tile_size: [40.0, 40.0], layers: vec![layer(walls.clone())], ..Default::default() }, solid()));
    world.spawn((
        at(200.0, 300.0),
        Tilemap {
            tile_size: [64.0, 32.0],
            orientation: TileOrientation::Isometric,
            layers: vec![layer(walls)],
            ..Default::default()
        },
        solid(),
    ));
    let mut assets = AssetManager::new();
    let (w, h) = (64u32, 32u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            // A hill: solid below a sine.
            let top = 12.0 + 8.0 * (x as f32 * 0.15).sin();
            if y as f32 > top {
                rgba[((y * w + x) * 4 + 3) as usize] = 255;
            }
        }
    }
    assets.insert_texture_bytes("hill".into(), RawTextureData { width: w, height: h, rgba });
    world.spawn((
        Transform { position: Vec2::new(0.0, -250.0), rotation: 0.2, scale: Vec2::splat(3.0) },
        SpriteComponent { texture_id: "hill".into(), size: Some([128.0, 64.0]), ..Default::default() },
        Carvable::default(),
    ));
    apply_carves(&mut world, &mut assets);
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    physics.step(1.0 / 60.0);
    (world, physics)
}


const MIRROR: &str = include_str!("../src/scripting/bridge/adapters/js/mirror.rs");

#[test]
fn the_browser_ray_snapshot_sees_tiles_and_carved_land_as_physics_does() {
    let (world, physics) = land();
    let mut snap = RaySnapshot::new();
    snap.refresh(&world);
    assert!(snap.len() > 3, "tile runs and carved pieces are in the snapshot ({})", snap.len());
    let mut hits = 0;
    for oy in (-500..500).step_by(37) {
        for ox in (-700..700).step_by(53) {
            for (dx, dy) in [(1.0, 0.0), (0.0, -1.0), (-0.6, -0.8), (0.7, 0.7)] {
                let (ox, oy) = (ox as f32, oy as f32);
                let a = snap.cast(ox, oy, dx, dy, 900.0);
                let b = physics.raycast(ox, oy, dx, dy, 900.0);
                match (a, b) {
                    (None, None) => {}
                    (Some(a), Some(b)) => {
                        hits += 1;
                        assert_eq!(a.entity_id, b.entity_id, "the same thing hit from ({ox}, {oy})");
                        assert!((a.distance - b.distance).abs() < 0.05, "at the same distance: {} vs {}", a.distance, b.distance);
                    }
                    (a, b) => panic!("from ({ox}, {oy}) dir ({dx}, {dy}): snapshot {a:?}, physics {b:?}"),
                }
            }
        }
    }
    assert!(hits > 200, "the rays mostly found something ({hits})");
}

#[test]
fn a_named_snapshot_reports_a_spawned_entity_by_its_handle() {
    let mut world = World::new();
    let wall = world.spawn((at(300.0, 0.0), Collider2D { shape: ColliderShape::Box { width: 40.0, height: 400.0 }, ..Default::default() }));
    let mut snap = RaySnapshot::new();
    let names = std::collections::HashMap::from([(wall.id() as u64, 1_000_007u64)]);
    snap.refresh_named(&world, &names);
    assert_eq!(snap.cast(0.0, 0.0, 1.0, 0.0, 1000.0).unwrap().entity_id, 1_000_007);
    snap.refresh(&world);
    assert_eq!(snap.cast(0.0, 0.0, 1.0, 0.0, 1000.0).unwrap().entity_id, wall.id() as u64, "unnamed, its own id");
}

#[test]
fn native_reads_and_results_follow_a_spawn_handle() {
    spawn_aliases::clear();
    let mut world = World::new();
    let ball = world.spawn((
        at(0.0, 0.0),
        RigidBody2D { body_type: BodyType::Dynamic, gravity_scale: 0.0, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 20.0 }, ..Default::default() },
    ));
    let handle = spawn_aliases::FIRST_SPAWN_ID + 3;
    spawn_aliases::record(handle, ball);
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    physics.set_velocity(ball.id() as u64, 120.0, -30.0);
    physics.step(1.0 / 60.0);

    let mut snap = jengine_core::scripting::bridge::snapshot::snapshot_world_borrow(&world);
    jengine_core::scripting::bridge::snapshot::snapshot_physics(&mut snap, &world, &physics);
    snap.physics_ptr = Some(&physics as *const _);
    let (vx, vy) = snap.get_velocity(handle).expect("a velocity for the handle");
    assert!((vx - 120.0).abs() < 1.0 && (vy + 30.0).abs() < 1.0, "the handle reads the body's velocity: ({vx}, {vy})");
    let hit = snap.raycast(-200.0, 0.0, 1.0, 0.0, 400.0).expect("the ray meets the ball");
    assert_eq!(hit.entity_id, handle, "and a ray names it as the script knows it");
    snap.physics_ptr = None;
    let _: &BridgeReadSnapshot = &snap;
    spawn_aliases::clear();
}

#[test]
fn browser_spawn_handles_start_where_natives_do() {
    let preamble = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
    // UI elements share the counter, as native's do.
    for f in ["function spawn_entity(", "function spawn_prefab(", "function create_ui_text(", "function create_ui_panel("] {
        let at = preamble.find(f).unwrap_or_else(|| panic!("{f} is in the preamble"));
        let body: String = preamble[at..].chars().take(1200).collect();
        assert!(body.contains("next_entity_id || 1000000"), "{f} hands out ids below a million again");
    }
    assert_eq!(spawn_aliases::FIRST_SPAWN_ID, 1_000_000);
    // A host that seeds the counter overrides the preamble's start: both
    // seeded it at the largest entity id plus 1,024 until Oct 2026, so the
    // handles were back inside the range real ids grow into.
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../");
    for host in ["jengine-web/src/script_host.rs", "jengine-editor-viewport/src/lib.rs"] {
        let src = std::fs::read_to_string(format!("{root}{host}")).unwrap();
        assert!(!src.contains("\"next_entity_id\""), "{host} seeds the spawn handles' counter");
    }
}

#[test]
fn both_browser_hosts_name_physics_rows_by_handle() {
    // One implementation since Oct 2026 (`adapters::js::mirror`): its rows
    // are named through the handles a host passes in, so each host must.
    let at = MIRROR.find("fn publish_physics_state(").unwrap();
    let body: String = MIRROR[at..].chars().take(6000).collect();
    assert!(body.contains("sid(*a as u64)") && body.contains("sid(*id)"), "the shared mirror publishes physics rows by real id");
    // Both hosts publish through `publish_physics_for_scripts` since Oct 2026,
    // which hands the shared spawn names over.
    let at = MIRROR.find("pub fn publish_physics_for_scripts(").expect("the shared publish");
    let shared: String = MIRROR[at..].chars().take(1500).collect();
    assert!(shared.contains("script_names()"), "the shared publish names physics rows without the spawn handles");
    for (host, src) in [
        ("viewport", include_str!("../../jengine-editor-viewport/src/lib.rs").to_string()),
        ("web", format!("{}{}", include_str!("../../jengine-web/src/script_host.rs"), include_str!("../../jengine-web/src/lib.rs"))),
    ] {
        assert!(src.contains("mirror::publish_physics_for_scripts("), "{host} publishes no physics state for its scripts");
    }
    let web = include_str!("../../jengine-web/src/lib.rs");
    let viewport = include_str!("../../jengine-editor-viewport/src/lib.rs");
    for (host, src) in [("viewport", viewport), ("web", web)] {
        assert!(!src.contains("ray_snapshot.borrow_mut().refresh(&"), "{host} refreshes its rays without the spawn names");
    }
}
