//! **Particles start on a shape the entity already has** (Oct 2026): an
//! emitter's `emit_shape` of `path` starts them along the entity's own
//! `vector_path`, evenly by length, and `sprite` over its sprite's opaque
//! pixels (or only its outline with `edge`) - a fuse, a neon sign throwing
//! sparks, a silhouette burning away. Before it a shape was a point, a
//! circle, a box or a line, so anything else took a row of emitters.
//!
//! Both follow the entity's turn and scale. The pixels are read in the step
//! every host makes with its assets (`carve::apply_carves`).

use glam::Vec2;
use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::carve::apply_carves;
use jengine_core::ecs::components::{PathCommand, SpriteComponent, Transform, VectorPath};
use jengine_core::particles::{EmitShape, ParticleEmitter, ParticleSystem, tick_particles};

/// An emitter that bursts `n` particles that do not move and never die.
fn still(shape: EmitShape, n: u32) -> ParticleEmitter {
    ParticleEmitter {
        emission_rate: 0.0,
        burst_count: n,
        max_particles: n,
        lifetime: [100.0, 100.0],
        speed: [0.0, 0.0],
        emit_shape: shape,
        ..Default::default()
    }
}

/// Where every particle of the world's emitters is, after one tick.
fn positions(world: &World) -> Vec<Vec2> {
    let mut system = ParticleSystem::new();
    tick_particles(world, &mut system, 0.01);
    let mut q = world.query::<(&Transform, &ParticleEmitter)>();
    let emitters: Vec<_> = q.iter().map(|(e, (t, em))| (e.id() as u64, em, t.position)).collect();
    system
        .draw_data_masked(&emitters, &Default::default(), &Default::default())
        .into_iter()
        .flat_map(|g| g.particles.into_iter().map(|p| p.position))
        .collect()
}

#[test]
fn a_path_shape_starts_particles_along_the_path() {
    let mut world = World::new();
    let line = VectorPath {
        commands: vec![PathCommand::MoveTo { x: 0.0, y: 0.0 }, PathCommand::LineTo { x: 200.0, y: 0.0 }],
        ..Default::default()
    };
    world.spawn((
        Transform { position: Vec2::new(100.0, 50.0), rotation: std::f32::consts::FRAC_PI_2, ..Default::default() },
        line,
        still(EmitShape::Path, 400),
    ));
    let ps = positions(&world);
    assert_eq!(ps.len(), 400);
    // Turned a quarter: the path runs up from (100, 50) to (100, 250).
    assert!(ps.iter().all(|p| (p.x - 100.0).abs() < 0.01 && p.y >= 49.9 && p.y <= 250.1), "{:?}", &ps[..5]);
    let mean = ps.iter().map(|p| p.y).sum::<f32>() / ps.len() as f32;
    assert!((mean - 150.0).abs() < 12.0, "evenly along it: mean {mean}");
}

/// A 16 x 16 texture opaque on its left half only.
fn half_texture() -> RawTextureData {
    let mut rgba = Vec::new();
    for _y in 0..16 {
        for x in 0..16 {
            rgba.extend_from_slice(&[255, 255, 255, if x < 8 { 255 } else { 0 }]);
        }
    }
    RawTextureData { width: 16, height: 16, rgba }
}

fn sprite_world(shape: EmitShape, rotation: f32) -> World {
    let mut world = World::new();
    world.spawn((
        Transform { rotation, ..Default::default() },
        SpriteComponent { texture_id: "half.png".into(), size: Some([64.0, 64.0]), ..Default::default() },
        still(shape, 300),
    ));
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("half.png".into(), half_texture());
    apply_carves(&mut world, &mut assets);
    world
}

#[test]
fn a_sprite_shape_starts_particles_on_its_opaque_pixels() {
    let ps = positions(&sprite_world(EmitShape::Sprite { edge: false, cutoff: 0.5 }, 0.0));
    assert_eq!(ps.len(), 300);
    assert!(ps.iter().all(|p| p.x <= 0.5 && p.x >= -32.5 && p.y.abs() <= 32.5), "only the left half: {:?}", &ps[..5]);
    // Turned a quarter, the left half is the bottom half.
    let turned = positions(&sprite_world(EmitShape::Sprite { edge: false, cutoff: 0.5 }, std::f32::consts::FRAC_PI_2));
    assert!(turned.iter().all(|p| p.y <= 0.5), "it turns with the sprite: {:?}", &turned[..5]);
}

#[test]
fn an_edge_sprite_shape_keeps_to_the_outline() {
    let ps = positions(&sprite_world(EmitShape::Sprite { edge: true, cutoff: 0.5 }, 0.0));
    // The half's outline: its left, top and bottom rows, and the column at x 0.
    let near_edge = |p: &Vec2| p.x < -26.0 || p.x > -6.0 || p.y.abs() > 26.0;
    assert!(ps.iter().all(near_edge), "none deep inside: {:?}", ps.iter().find(|p| !near_edge(p)));
}

#[test]
fn the_scene_spells_them_by_kind() {
    let em: ParticleEmitter =
        serde_json::from_str(r#"{ "emit_shape": { "kind": "sprite", "edge": true } }"#).expect("parses");
    assert_eq!(em.emit_shape, EmitShape::Sprite { edge: true, cutoff: 0.5 });
    let em: ParticleEmitter = serde_json::from_str(r#"{ "emit_shape": { "kind": "path" } }"#).expect("parses");
    assert_eq!(em.emit_shape, EmitShape::Path);
}
