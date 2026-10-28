//! **Meshes in the normal pass, and squashed lights** (Oct 2026). A `mesh`
//! whose material has a `normal_map_id` writes its normals, in perspective,
//! as a sprite does - before, the normal pass drew sprites only and every
//! mesh lit flat whatever its map, which a 3D stage's walls (all meshes)
//! could not live with. And a point or spot light's `squash` makes its pool
//! an ellipse, the shape of a lamp's pool on a floor seen in perspective,
//! with `light_at` answering the same. Found by Visual Showcase 2.

use glam::Vec2;
use jengine_core::ecs::components::PointLight2D;
use jengine_core::renderer::lighting::{Occluders, point_light_at, squashed_distance};

#[test]
fn a_squashed_pool_is_an_ellipse_on_the_cpu() {
    let light = PointLight2D { radius: 100.0, falloff: 1.0, squash: 0.5, casts_shadows: false, ..Default::default() };
    let at = |x: f32, y: f32| point_light_at(Vec2::new(x, y), Vec2::ZERO, &light, Occluders::default()).x;
    assert!((at(60.0, 0.0) - 0.4).abs() < 1e-4, "as wide as its radius: {}", at(60.0, 0.0));
    assert_eq!(at(0.0, 60.0), 0.0, "and half as tall");
    assert!((at(0.0, 30.0) - at(60.0, 0.0)).abs() < 1e-4, "30 up is as far as 60 across");
    assert_eq!(squashed_distance(Vec2::new(0.0, 10.0), 0.0), 10.0, "zero is no squash");
    let round = PointLight2D { squash: 1.0, ..light.clone() };
    assert_eq!(point_light_at(Vec2::new(0.0, 60.0), Vec2::ZERO, &round, Occluders::default()).x, at(60.0, 0.0));
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use glam::Vec2;
    use hecs::World;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::ecs::components::{AmbientLight, MaterialComponent, Mesh2D, PointLight2D, SpriteComponent, Transform};
    use jengine_core::ecs::wgpu_systems::{
        SpriteShadowFlush, WorldLayer, lit_sprite_textures, needs_normal_pass, render_normals_wgpu,
        render_sprites_with_shadows_wgpu_materialized,
    };
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuLitSpritePipeline, WgpuSpriteBatch};
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::lighting::gather_lights;
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 128;
    const H: u32 = 128;

    fn solid(rgba: [u8; 4]) -> RawTextureData {
        RawTextureData { width: 4, height: 4, rgba: rgba.repeat(16) }
    }

    /// A grey mesh filling the frame - or a sprite, to compare - whose normal
    /// map leans every normal toward `lean` (+x or -x), lit by a light at
    /// `light_x` squashed by `squash`; the red channel of the frame.
    fn render(gpu: &HeadlessGpu, mesh: bool, lean: Option<f32>, light_x: f32, squash: f32) -> Vec<u8> {
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes("grey.png".into(), solid([200, 200, 200, 255]));
        assets.ensure_wgpu_texture("grey.png", &gpu.device, &gpu.queue).unwrap();
        if let Some(l) = lean {
            // A normal tipped 45 degrees toward +x (or -x): (+-0.7, 0, 0.7).
            let x = (128.0 + 127.0 * 0.7071 * l) as u8;
            assets.insert_texture_bytes("lean.png".into(), solid([x, 128, (128.0 + 127.0 * 0.7071) as u8, 255]));
        }
        let material = MaterialComponent { normal_map_id: lean.map(|_| "lean.png".to_string()), ..Default::default() };
        let mut world = World::new();
        world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.05 },));
        world.spawn((
            Transform { position: Vec2::new(light_x, 0.0), ..Default::default() },
            PointLight2D { intensity: 0.6, radius: 200.0, falloff: 1.0, height: 0.3, squash, ..Default::default() },
        ));
        if mesh {
            world.spawn((
                Transform::default(),
                Mesh2D::quad("grey.png", [[-64.0, 64.0], [64.0, 64.0], [64.0, -64.0], [-64.0, -64.0]]),
                material,
            ));
        } else {
            world.spawn((
                Transform::default(),
                SpriteComponent { texture_id: "grey.png".into(), size: Some([128.0, 128.0]), ..Default::default() },
                material,
            ));
        }
        let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).unwrap();
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        let mut lit = WgpuLitSpritePipeline::new(&gpu.device, gpu.color_format).unwrap();
        let flat = WgpuLitSpritePipeline::flat_normal_texture(&gpu.device, &gpu.queue);
        let camera = Camera2D::new(W as f32, H as f32);
        let vp = camera.view_projection();
        let atlas = built_in_font_atlas();
        let fonts = FrameFonts::builtin_only(&atlas.3, "__font");
        let mut encoder = gpu.encoder();
        {
            let mut pass = lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
            batch.begin_frame();
            let assets = &assets;
            render_sprites_with_shadows_wgpu_materialized(
                &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                |b, tex, kind| {
                    if let SpriteShadowFlush::Sprite { .. } = kind {
                        b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(tex).unwrap());
                    }
                },
            );
        }
        if needs_normal_pass(&world) {
            let mut np = lp.normal_pass(&mut encoder);
            lit.begin_frame();
            let assets = &mut assets;
            render_normals_wgpu(&world, &SortLayers::default(), &mut lit, Vec2::splat(64.0), |pipe, d, n| {
                let (diffuse, normal, missing) = lit_sprite_textures(assets, &gpu.device, &gpu.queue, d, n, &flat).unwrap();
                assert!(missing.is_none(), "the normal map is loaded");
                pipe.flush_normals(&gpu.device, &gpu.queue, &mut np, &vp, diffuse, normal);
            });
        }
        let lights = gather_lights(&world);
        lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
        lp.composite(&mut encoder, gpu.view());
        gpu.submit(encoder);
        gpu.read_rgba().rgba.chunks(4).map(|p| p[0]).collect()
    }

    fn at(img: &[u8], x: i32, y: i32) -> i32 {
        img[((H as i32 / 2 - y) as u32 * W + (W as i32 / 2 + x) as u32) as usize] as i32
    }

    #[test]
    fn a_normal_mapped_mesh_is_lit_by_its_bumps() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("meshes_take_normal_maps: no GPU adapter - skipped");
            return;
        };
        // A light off to the left: a face turned toward it is brighter than
        // flat, one turned away darker - for a mesh as for a sprite.
        let flat = render(&gpu, true, None, -60.0, 1.0);
        let toward = render(&gpu, true, Some(-1.0), -60.0, 1.0);
        let away = render(&gpu, true, Some(1.0), -60.0, 1.0);
        let (f, t, a) = (at(&flat, 0, 0), at(&toward, 0, 0), at(&away, 0, 0));
        assert!(t > f + 15 && a + 15 < f, "a mesh's normal map reaches the light: toward {t}, flat {f}, away {a}");
        // And it matches what a sprite with the same map gets.
        let sprite = render(&gpu, false, Some(-1.0), -60.0, 1.0);
        assert!((at(&sprite, 0, 0) - t).abs() <= 3, "sprite {} mesh {t}", at(&sprite, 0, 0));
    }

    #[test]
    fn a_squashed_light_lights_an_ellipse() {
        let Some(gpu) = HeadlessGpu::new(W, H) else { return };
        let round = render(&gpu, true, None, 0.0, 1.0);
        let squashed = render(&gpu, true, None, 0.0, 0.25);
        // 40 across is lit the same either way; 40 up is dark when squashed
        // (40 / 0.25 = 160, near the edge of a 200 radius).
        assert!((at(&round, 40, 0) - at(&squashed, 40, 0)).abs() <= 2, "{} {}", at(&round, 40, 0), at(&squashed, 40, 0));
        assert!(at(&squashed, 0, 40) + 30 < at(&round, 0, 40), "squashed {} round {}", at(&squashed, 0, 40), at(&round, 0, 40));
        // The CPU agrees on the ratio across and up.
        let light = PointLight2D { intensity: 0.6, radius: 200.0, falloff: 1.0, squash: 0.25, casts_shadows: false, ..Default::default() };
        let cpu = |x: f32, y: f32| {
            jengine_core::renderer::lighting::point_light_at(Vec2::new(x, y), Vec2::ZERO, &light, Default::default()).x
        };
        assert!(cpu(0.0, 40.0) < cpu(40.0, 0.0) * 0.5);
    }
}
