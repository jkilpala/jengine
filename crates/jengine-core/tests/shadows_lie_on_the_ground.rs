//! **Shadows lie on the ground** (Oct 2026; `ecs::cast_shadow`): a post
//! with a `cast_shadow` under a sun travelling west and down lays its
//! silhouette on the floor to the west - and under a sun travelling east, to
//! the east - drawn through the real sprite pass. Without a GPU adapter it
//! prints why and passes. The geometry is tested in the module.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::ecs::cast_shadow::CastShadow;
use jengine_core::ecs::components::{DirectionalLight2D, SpriteComponent, Transform};
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 256;
const H: u32 = 128;

/// The floor under a post at the origin, the sun travelling at `angle`; the
/// red channel at world points (x, 10), half way up the shadow at 45 degrees.
fn render(angle: f32) -> Option<Vec<(f32, u8)>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("shadows_lie_on_the_ground: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("box.png".into(), RawTextureData { width: 4, height: 4, rgba: vec![255; 64] });
    assets.ensure_wgpu_texture("box.png", &gpu.device, &gpu.queue).expect("upload");
    let (rgba, aw, ah, font) = built_in_font_atlas();
    assets.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    let fonts = FrameFonts::builtin_only(&font, "__font");

    let at = |x: f32, y: f32| Transform { position: Vec2::new(x, y), ..Default::default() };
    let mut world = World::new();
    // A white floor seen from the side: it runs from under the foot up the
    // screen, which is the ground receding.
    world.spawn((
        at(0.0, -12.0),
        SpriteComponent { texture_id: "box.png".into(), size: Some([256.0, 104.0]), z_order: 0, ..Default::default() },
    ));
    // A red post standing on it, casting.
    world.spawn((
        at(0.0, 0.0),
        SpriteComponent {
            texture_id: "box.png".into(),
            size: Some([20.0, 60.0]),
            anchor: [0.5, 0.0],
            color: [1.0, 0.0, 0.0, 1.0],
            z_order: 2,
            ..Default::default()
        },
        CastShadow { opacity: 0.8, fade: 1.0, ..Default::default() },
    ));
    world.spawn((Transform::default(), DirectionalLight2D { direction: angle, intensity: 1.0, ..Default::default() }));
    jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        let assets = &assets;
        render_sprites_with_shadows_wgpu_materialized(
            &world,
            &SortLayers::default(),
            &mut batch,
            &fonts,
            Vec2::splat(64.0),
            WorldLayer::All,
            |b, tex, kind| {
                let texture = assets.get_wgpu_texture(tex).expect("texture uploaded");
                match kind {
                    SpriteShadowFlush::Shadow => b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, texture),
                    SpriteShadowFlush::Sprite { .. } => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture),
                };
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let red = |x: f32, y: f32| {
        let (px, py) = ((x + W as f32 / 2.0) as u32, (H as f32 / 2.0 - y) as u32);
        img.rgba[((py * img.width + px) * 4) as usize]
    };
    Some([-35.0, -25.0, 25.0, 35.0].iter().map(|&x| (x, red(x, 10.0))).collect())
}

#[test]
fn a_low_sun_lays_the_shadow_on_the_side_it_is_going() {
    // Travelling west and down at 45 degrees: a shadow 60 long to the west.
    let Some(west) = render(-std::f32::consts::FRAC_PI_4 * 3.0) else { return };
    // Half way up (10 of its 21), the shadow runs from -40 to -20.
    let (w35, w25, e25, e35) = (west[0].1, west[1].1, west[2].1, west[3].1);
    assert!(w35 < 140 && w25 < 140, "the floor to the west is in shadow: {west:?}");
    assert!(e25 > 230 && e35 > 230, "the floor to the east is lit: {west:?}");
    // Travelling east: the other way.
    let Some(east) = render(-std::f32::consts::FRAC_PI_4) else { return };
    assert!(east[0].1 > 230 && east[3].1 < 140, "to the east now: {east:?}");
}
