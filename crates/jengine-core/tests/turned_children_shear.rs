//! A turned child under a squashed parent turns *within* the squash.
//!
//! The hierarchy composed transforms as translate / rotate / scale and threw
//! the rest away, so a child turned 45 degrees under a parent squashed to half
//! height was drawn as its own rectangle, squashed and *then* turned - a
//! rotated ellipse where the parent's squash should have held. That is how a
//! rune circle spinning on a floor seen at an angle has to be built (the
//! showcase's Spellcraft), and Godot's `Transform2D` and Unity's matrices
//! both get it right. `WorldTransform` carries a shear now, `compose` is the
//! true product of the two maps, and the sprite pass shears the quad.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Parent, Transform, WorldTransform};
use jengine_core::ecs::hierarchy::{compose, resolve_world_transforms};

fn local(t: &Transform, p: Vec2) -> Vec2 {
    WorldTransform::from(t).apply(p)
}

/// The composed transform maps every point exactly where applying the child
/// and then the parent does - for turned, mirrored, squashed and already
/// sheared parents alike.
#[test]
fn compose_is_the_product_of_the_two_maps() {
    let parents = [
        WorldTransform { position: Vec2::new(10.0, -4.0), rotation: 0.3, scale: Vec2::new(1.0, 0.4), shear: 0.0 },
        WorldTransform { position: Vec2::ZERO, rotation: -1.2, scale: Vec2::new(-2.0, 0.5), shear: 0.0 },
        WorldTransform { position: Vec2::new(3.0, 3.0), rotation: 2.0, scale: Vec2::new(0.7, 1.9), shear: 0.35 },
        WorldTransform { position: Vec2::ZERO, rotation: 0.0, scale: Vec2::new(2.0, 2.0), shear: 0.0 },
    ];
    let children = [
        Transform { position: Vec2::new(5.0, 2.0), rotation: 0.785, scale: Vec2::new(1.0, 1.0) },
        Transform { position: Vec2::new(-1.0, 7.0), rotation: 2.6, scale: Vec2::new(1.5, -0.5) },
        Transform { position: Vec2::ZERO, rotation: 0.0, scale: Vec2::new(3.0, 1.0) },
    ];
    let points = [Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(-3.0, 2.5), Vec2::new(12.0, -7.0)];
    for parent in &parents {
        for child in &children {
            let w = compose(parent, child);
            for &p in &points {
                let want = parent.apply(local(child, p));
                let got = w.apply(p);
                assert!(
                    (want - got).length() < 1e-3,
                    "parent {parent:?}, child {child:?}, point {p}: composed {got}, expected {want}"
                );
            }
        }
    }
}

/// Everywhere TRS was already exact, the result is TRS - so no existing
/// scene moves: a uniform parent adds rotations and multiplies scales.
#[test]
fn a_uniform_parent_composes_as_before() {
    let parent = WorldTransform { position: Vec2::new(1.0, 2.0), rotation: 0.5, scale: Vec2::new(2.0, 2.0), shear: 0.0 };
    let child = Transform { position: Vec2::new(3.0, 0.0), rotation: 0.25, scale: Vec2::new(1.0, 0.5) };
    let w = compose(&parent, &child);
    assert_eq!(w.shear, 0.0);
    assert!((w.rotation - 0.75).abs() < 1e-6);
    assert_eq!(w.scale, Vec2::new(2.0, 1.0));
}

#[test]
fn a_turned_child_under_a_squashed_parent_is_sheared() {
    let mut world = World::new();
    let floor = world.spawn((Transform { position: Vec2::ZERO, rotation: 0.0, scale: Vec2::new(1.0, 0.5) },));
    let ring = world.spawn((
        Transform { position: Vec2::ZERO, rotation: std::f32::consts::FRAC_PI_4, scale: Vec2::ONE },
        Parent(floor),
    ));
    resolve_world_transforms(&mut world);
    let w = world.get::<&WorldTransform>(ring).unwrap().clone();
    assert!(w.shear.abs() > 0.1, "a 45 degree turn under a half-height squash shears: {w:?}");
    // The child's own x axis tip lands on the squashed diagonal.
    let tip = w.apply(Vec2::new(10.0, 0.0));
    assert!((tip - Vec2::new(7.071, 3.536)).length() < 1e-2, "{tip}");
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::ecs::components::SpriteComponent;
    use jengine_core::ecs::wgpu_systems::render_sprites_wgpu_multi_materialized;
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;

    const W: u32 = 256;
    const H: u32 = 128;

    /// A 64 x 64 white square turned 45 degrees on a floor squashed to half
    /// height is a flat diamond, (+-45, 0) and (0, +-23). Drawn the old way -
    /// a 64 x 32 rectangle turned 45 degrees - it covers (20, 20), which the
    /// diamond does not, and misses (40, 0), which the diamond covers.
    #[test]
    fn the_sprite_pass_draws_the_shear() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("turned_children_shear: no GPU adapter - skipped");
            return;
        };
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes("white".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
        assets.ensure_wgpu_texture("white", &gpu.device, &gpu.queue).expect("upload");

        let mut world = World::new();
        let floor = world.spawn((Transform { position: Vec2::ZERO, rotation: 0.0, scale: Vec2::new(1.0, 0.5) },));
        world.spawn((
            Transform { position: Vec2::ZERO, rotation: std::f32::consts::FRAC_PI_4, scale: Vec2::ONE },
            SpriteComponent { texture_id: "white".into(), size: Some([64.0, 64.0]), ..Default::default() },
            Parent(floor),
        ));
        resolve_world_transforms(&mut world);

        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
        let vp = Camera2D::new(W as f32, H as f32).view_projection();
        let mut encoder = gpu.encoder();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            batch.begin_frame();
            render_sprites_wgpu_multi_materialized(&world, &SortLayers::default(), &mut batch, Vec2::splat(64.0), |b, tex, _| {
                let texture = assets.get_wgpu_texture(tex).expect("uploaded");
                b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
            });
        }
        gpu.submit(encoder);
        let img = gpu.read_rgba();
        let lit = |x: f32, y: f32| {
            let (px, py) = ((W as f32 / 2.0 + x) as u32, (H as f32 / 2.0 - y) as u32);
            img.rgba[((py * W + px) * 4) as usize] > 128
        };
        assert!(lit(0.0, 0.0), "the middle is drawn");
        assert!(lit(40.0, 0.0), "the diamond reaches (40, 0) - drawn as a turned rectangle, it does not");
        assert!(!lit(20.0, 20.0), "(20, 20) is outside the diamond - a turned rectangle would cover it");
        assert!(!lit(0.0, 30.0), "the diamond is squashed to half height");
    }
}
