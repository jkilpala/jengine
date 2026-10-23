//! **A collider in the shape of its sprite** (Oct 2026): a `sprite_collider`
//! traces the sprite's opaque pixels (as a shadow from `from_alpha` is
//! traced) and writes the outline into the entity's `collider` as a convex
//! decomposition - so a kettle rests on its base and its spout, not on the
//! box round it. Before it a collider was a box, a circle or a polygon typed
//! in by hand. Found by the showcase's Scrap Yard.
#![cfg(feature = "physics")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::{
    BodyType, Collider2D, ColliderShape, RigidBody2D, SpriteCollider, SpriteComponent, Transform,
};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;

const DT: f32 = 1.0 / 60.0;

/// A 64 x 64 texture whose opaque pixels are where `solid(x, y)` says (y
/// down, as images run).
fn texture(solid: impl Fn(u32, u32) -> bool) -> RawTextureData {
    let mut rgba = vec![0u8; 64 * 64 * 4];
    for y in 0..64 {
        for x in 0..64 {
            if solid(x, y) {
                let i = ((y * 64 + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[200, 200, 200, 255]);
            }
        }
    }
    RawTextureData { width: 64, height: 64, rgba }
}

fn sprite(id: &str) -> SpriteComponent {
    SpriteComponent { texture_id: id.into(), size: Some([64.0, 64.0]), ..Default::default() }
}

/// The shape written into the entity's collider, if any.
fn shape(world: &World, e: hecs::Entity) -> Option<Vec<[f32; 2]>> {
    match &world.get::<&Collider2D>(e).ok()?.shape {
        ColliderShape::ConvexDecomposition { vertices, .. } => Some(vertices.clone()),
        _ => None,
    }
}

fn bounds(v: &[[f32; 2]]) -> (Vec2, Vec2) {
    v.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| {
        (lo.min(Vec2::from_array(*p)), hi.max(Vec2::from_array(*p)))
    })
}

#[test]
fn the_outline_becomes_the_collider_scaled_with_the_entity() {
    let mut assets = AssetManager::new();
    // Two islands: a big block on the left, a speck on the right.
    assets.insert_texture_bytes("t.png".into(), texture(|x, y| (x < 32 && y >= 16) || (x > 56 && y < 4)));
    let mut world = World::new();
    let e = world.spawn((
        Transform { scale: Vec2::new(2.0, 1.0), ..Default::default() },
        sprite("t.png"),
        SpriteCollider::default(),
    ));
    assert!(shape(&world, e).is_none());
    jengine_core::ecs::carve::apply_carves(&mut world, &mut assets);
    let v = shape(&world, e).expect("a collider was added, in the sprite's shape");
    // Two loops: the block and the speck.
    let loops = match &world.get::<&Collider2D>(e).unwrap().shape {
        ColliderShape::ConvexDecomposition { vertices, loops } => {
            jengine_core::ecs::components::decomposition_loops(vertices, loops).into_iter().map(|l| l.to_vec()).collect::<Vec<_>>()
        }
        _ => unreachable!(),
    };
    assert_eq!(loops.len(), 2, "both islands");
    let (lo, hi) = loops.iter().map(|l| bounds(l)).max_by(|a, b| (a.1 - a.0).x.total_cmp(&(b.1 - b.0).x)).unwrap();
    // The block: x -32..0 and y -32..16 in the sprite's frame (centred
    // anchor, y up), x doubled by the scale.
    assert!((lo.x + 64.0).abs() < 2.5 && hi.x.abs() < 2.5, "x {lo:?}..{hi:?}");
    assert!((lo.y + 32.0).abs() < 2.5 && (hi.y - 16.0).abs() < 2.5, "y {lo:?}..{hi:?}");
    // The speck, top right: x 56..64 doubled, y 28..32.
    let (slo, shi) = bounds(&v);
    assert!(shi.x > 50.0 && shi.y > 26.0, "the speck is part of the shape: {slo:?}..{shi:?}");
    let key = world.get::<&SpriteCollider>(e).unwrap().key;
    assert_ne!(key, 0);
    // Nothing changed: not traced again.
    jengine_core::ecs::carve::apply_carves(&mut world, &mut assets);
    assert_eq!(world.get::<&SpriteCollider>(e).unwrap().key, key);
    // A new scale is a new shape.
    world.get::<&mut Transform>(e).unwrap().scale = Vec2::ONE;
    jengine_core::ecs::carve::apply_carves(&mut world, &mut assets);
    assert_ne!(world.get::<&SpriteCollider>(e).unwrap().key, key);
    let (lo, _) = bounds(&shape(&world, e).unwrap());
    assert!((lo.x + 32.0).abs() < 2.5);
}

/// Drop a sprite whose opaque pixels are only its upper half onto a floor;
/// where its transform rests.
fn rest_height(traced: bool) -> f32 {
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("half.png".into(), texture(|_, y| y < 32));
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    world.spawn((
        Transform { position: Vec2::new(0.0, -20.0), ..Default::default() },
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 400.0, height: 40.0 }, ..Default::default() },
    ));
    let falling = world.spawn((
        Transform { position: Vec2::new(0.0, 120.0), ..Default::default() },
        sprite("half.png"),
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 64.0, height: 64.0 }, ..Default::default() },
    ));
    if traced {
        world.insert_one(falling, SpriteCollider::default()).unwrap();
    }
    for frame in 0..240 {
        jengine_core::ecs::carve::apply_carves(&mut world, &mut assets);
        ensure_physics_bodies(&world, &mut physics);
        if frame == 0 && traced {
            assert!(physics.has_body(falling.id() as u64), "traced in the same frame, so built at once");
        }
        physics.step(DT);
    }
    physics.get_position(falling.id() as u64).unwrap().1
}

#[test]
fn a_body_rests_on_its_pixels_not_its_box() {
    let boxed = rest_height(false);
    let traced = rest_height(true);
    // The floor's top is y = 0. The box rests its centre 32 up; the traced
    // shape is the upper half (image rows run down), so the sprite's centre
    // - the bottom of its pixels - rests on the floor.
    assert!((boxed - 32.0).abs() < 3.0, "the box: {boxed}");
    assert!(traced.abs() < 3.0, "the pixels: {traced}");
}

#[test]
fn a_body_waits_for_its_shape_and_is_rebuilt_when_it_changes() {
    let mut assets = AssetManager::new();
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let e = world.spawn((
        Transform::default(),
        sprite("late.png"),
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        SpriteCollider::default(),
    ));
    let id = e.id() as u64;
    // Its pixels are not loaded yet: no body, rather than a placeholder.
    jengine_core::ecs::carve::apply_carves(&mut world, &mut assets);
    ensure_physics_bodies(&world, &mut physics);
    assert!(!physics.has_body(id));
    assets.insert_texture_bytes("late.png".into(), texture(|x, _| x < 32));
    jengine_core::ecs::carve::apply_carves(&mut world, &mut assets);
    ensure_physics_bodies(&world, &mut physics);
    assert!(physics.has_body(id) && physics.has_collider(id));
    let first = physics.sprite_shape_key(id).expect("built from the traced shape");
    world.get::<&mut Transform>(e).unwrap().scale = Vec2::splat(2.0);
    jengine_core::ecs::carve::apply_carves(&mut world, &mut assets);
    ensure_physics_bodies(&world, &mut physics);
    let second = physics.sprite_shape_key(id).expect("rebuilt");
    assert_ne!(first, second);
    assert!(physics.has_collider(id));
}

/// Two separate pixels' worth of island, dropped on a floor: neither falls
/// through it.
#[test]
fn every_island_collides() {
    let mut assets = AssetManager::new();
    // Two blocks with a gap between them.
    assets.insert_texture_bytes("two.png".into(), texture(|x, y| y >= 32 && (x < 20 || x >= 44)));
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    world.spawn((
        Transform { position: Vec2::new(0.0, -20.0), ..Default::default() },
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 400.0, height: 40.0 }, ..Default::default() },
    ));
    let e = world.spawn((
        Transform { position: Vec2::new(0.0, 120.0), ..Default::default() },
        sprite("two.png"),
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        SpriteCollider::default(),
    ));
    for _ in 0..240 {
        jengine_core::ecs::carve::apply_carves(&mut world, &mut assets);
        ensure_physics_bodies(&world, &mut physics);
        physics.step(DT);
    }
    physics.sync_transforms_to_ecs(&mut world);
    let t = world.get::<&Transform>(e).unwrap().clone();
    let (y, angle) = (t.position.y, t.rotation);
    assert!((y - 32.0).abs() < 3.0 && angle.abs() < 0.05, "standing level on both feet: y {y}, angle {angle}");
}

#[test]
fn decomposition_loops_split_where_they_say() {
    use jengine_core::ecs::components::decomposition_loops;
    let v = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [5.0, 5.0], [6.0, 5.0], [6.0, 6.0]];
    assert_eq!(decomposition_loops(&v, &[]).len(), 1);
    let two = decomposition_loops(&v, &[3]);
    assert_eq!((two[0].len(), two[1].len()), (3, 3));
    assert_eq!(decomposition_loops(&v, &[0, 9]).len(), 1, "nonsense starts are ignored");
    // An authored shape without `loops` still loads, and saves without it.
    let s: ColliderShape = serde_json::from_str(r#"{ "ConvexDecomposition": { "vertices": [[0,0],[1,0],[0,1]] } }"#).unwrap();
    assert!(!serde_json::to_string(&s).unwrap().contains("loops"));
}

#[test]
fn a_sprite_collider_loads_from_a_scene() {
    let c: SpriteCollider = serde_json::from_str(r#"{ "alpha_cutoff": 0.3 }"#).unwrap();
    assert_eq!((c.alpha_cutoff, c.alpha_tolerance, c.key), (0.3, 1.5, 0));
    assert!(serde_json::to_value(&c).unwrap().get("key").is_none());
}
