//! **An occluder can shadow the wall behind and not its neighbours**
//! (`LightOccluder.shadows_casters`, Sep 2026). Lighting here is flat land:
//! every occluder is infinitely tall, so in a side view each block of a
//! tower shadowed the blocks beside it and the tower drew black, only its
//! edges lit. With `shadows_casters` off, a shadow skips pixels that are
//! themselves casters - the normal pass marks them - and still falls on
//! everything else. Found by the showcase's Collapse.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::ecs::components::{AmbientLight, LightOccluder, PointLight2D, SpriteComponent, Transform};
use jengine_core::ecs::wgpu_systems::{lit_sprite_textures, render_normals_wgpu};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuLitSpritePipeline};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::lighting::gather_lights;

const W: u32 = 256;
const H: u32 = 128;

/// Light reaching block B, and the wall behind both blocks, in 0..255.
fn render(shadows_casters: bool) -> Option<(u8, u8)> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("casters_can_spare_each_other: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("__white".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.0 },));
    world.spawn((
        Transform { position: Vec2::new(-100.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
        PointLight2D {
            color: [1.0, 1.0, 1.0],
            intensity: 1.0,
            radius: 400.0,
            falloff: 0.5,
            shadow_softness: 0.0,
            height: jengine_core::ecs::components::DEFAULT_LIGHT_HEIGHT,
            casts_shadows: true,
            ..Default::default()
        },
    ));
    // Block A, and block B right behind it (from the light), both casters.
    for x in [0.0, 40.0] {
        world.spawn((
            Transform { position: Vec2::new(x, 0.0), rotation: 0.0, scale: Vec2::ONE },
            SpriteComponent { texture_id: "__white".into(), size: Some([30.0, 30.0]), ..Default::default() },
            LightOccluder { size: [30.0, 30.0], shadows_casters, ..Default::default() },
        ));
    }
    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let mut lit = WgpuLitSpritePipeline::new(&gpu.device, gpu.color_format).expect("lit pipeline");
    let flat = WgpuLitSpritePipeline::flat_normal_texture(&gpu.device, &gpu.queue);
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
    {
        let mut np = lp.normal_pass(&mut encoder);
        lit.begin_frame();
        let assets = &mut assets;
        render_normals_wgpu(&world, &SortLayers::default(), &mut lit, Vec2::splat(64.0), |pipe, d, n| {
            let (diffuse, normal, _) = lit_sprite_textures(assets, &gpu.device, &gpu.queue, d, n, &flat).unwrap();
            pipe.flush_normals(&gpu.device, &gpu.queue, &mut np, &vp, diffuse, normal);
        });
    }
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &gather_lights(&world));
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let px = |p: Vec2| {
        let (x, y) = ((p.x + W as f32 / 2.0) as u32, (H as f32 / 2.0 - p.y) as u32);
        img.rgba[((y * W + x) * 4) as usize]
    };
    Some((px(Vec2::new(40.0, 0.0)), px(Vec2::new(100.0, 0.0))))
}

#[test]
fn a_caster_spares_its_neighbour_and_still_shadows_the_wall() {
    let Some((b_on, wall_on)) = render(true) else { return };
    assert!(b_on < 20, "by default block A's shadow falls on block B: {b_on}");
    assert!(wall_on < 20, "and on the wall behind: {wall_on}");
    let Some((b_off, wall_off)) = render(false) else { return };
    assert!(b_off > 120, "with shadows_casters off, block B is lit: {b_off}");
    assert!(wall_off < 20, "and the wall behind both is still in their shadow: {wall_off}");
}
