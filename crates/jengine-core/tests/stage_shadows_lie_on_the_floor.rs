//! **Shadows on a 3D stage** (Oct 2026; `ecs::stage`): a billboard with a
//! `shadow` throws its sprite's frame on the floor from each stage light
//! near it, projected from the light's 3D position - drawn through the real
//! sprite pass on a headless device: the floor is dark where the projection
//! says the shadow lies, and lit (here: white) on the lamp's side. Found by
//! Visual Showcase 2. Without a GPU adapter it prints why and passes.
#![cfg(all(feature = "gpu-wgpu", feature = "stage"))]

use glam::{Vec2, Vec3};
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{Mesh2D, PointLight2D, SpriteComponent, Transform};
use jengine_core::ecs::stage::{Stage, StageBillboard, StageCaster, StageQuad, StageShadow};
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 160;
const H: u32 = 120;

/// A white floor, a lamp three west of the middle and three up, and what
/// `stand` adds; the red of the frame and the stage it was seen from.
fn render(stand: impl FnOnce(&mut World)) -> Option<(Vec<u8>, Stage)> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("stage_shadows_lie_on_the_floor: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.insert_texture_bytes("figure.png".into(), RawTextureData { width: 2, height: 2, rgba: vec![255; 16] });
    for id in [WHITE_TEXTURE_ID, "figure.png"] {
        assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
    }
    let (rgba, aw, ah, font) = built_in_font_atlas();
    let mut fa = AssetManager::new();
    fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    let fonts = FrameFonts::builtin_only(&font, "__font");

    let mut world = World::new();
    let stage = Stage { target: [0.0, 0.0, 0.0], pitch: 0.7, distance: 40.0, zoom: 8.0, ..Default::default() };
    world.spawn((Transform::default(), stage.clone()));
    world.spawn((
        Transform::default(),
        Mesh2D { texture_id: WHITE_TEXTURE_ID.into(), ..Default::default() },
        StageQuad { origin: [-12.0, -8.0, 0.0], u: [24.0, 0.0, 0.0], v: [0.0, 16.0, 0.0], ground: true, ..Default::default() },
        jengine_core::ecs::components::MaterialComponent { material_id: "lit_sprite".into(), ..Default::default() },
    ));
    // A lamp three units west of the figure, three up.
    world.spawn((
        Transform::default(),
        // Strength (1 - 3/12)^2 x 2 at the figure's feet: over 1, so the
        // shadow is as dark as its opacity.
        PointLight2D { intensity: 2.0, ..Default::default() },
        StageBillboard { position: [-3.0, 0.0, 3.0], light_radius: 12.0, ..Default::default() },
    ));
    stand(&mut world);
    jengine_core::ecs::behaviors::tick_motion(&mut world, 1.0 / 60.0, None, false);

    let cam = Camera2D::new(W as f32, H as f32);
    set_frame_view(&cam);
    let vp = cam.view_projection();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, kind| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                if let SpriteShadowFlush::Sprite { .. } = kind {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                }
            },
        );
    }
    gpu.submit(encoder);
    clear_frame_view();
    let img = gpu.read_rgba();
    Some((img.rgba.chunks(4).map(|p| p[0]).collect(), stage))
}

fn at(img: &[u8], stage: &Stage, p: Vec3) -> u8 {
    let (s, _) = stage.camera(Vec2::ZERO).project(p).unwrap();
    let (x, y) = ((W as f32 / 2.0 + s.x) as u32, (H as f32 / 2.0 - s.y) as u32);
    img[(y * W + x) as usize]
}

#[test]
fn a_lamp_throws_the_figure_across_the_floor() {
    let Some((img, stage)) = render(|world| {
        world.spawn((
            Transform::default(),
            SpriteComponent {
                texture_id: "figure.png".into(),
                size: Some([8.0, 12.8]),
                color: [1.0, 1.0, 1.0, 1.0],
                anchor: [0.5, 0.0],
                ..Default::default()
            },
            StageBillboard { shadow: Some(StageShadow { opacity: 1.0, fade: 1.0, ..Default::default() }), ..Default::default() },
        ));
    }) else {
        return;
    };
    let at = |p: Vec3| at(&img, &stage, p);
    // The figure (1.6 tall) throws its top 3 * (3 / 1.4 - 1) = 3.4 east.
    let (shadow, lit) = (at(Vec3::new(2.0, 0.0, 0.0)), at(Vec3::new(-2.0, 0.0, 0.0)));
    assert!(lit > 200, "the floor on the lamp's side is white: {lit}");
    assert!(shadow < 60, "the floor east of the figure is in its shadow: {shadow}");
    assert!(at(Vec3::new(5.0, 0.0, 0.0)) > 200, "and past the shadow's tip it is white again");
}


/// A solid - a box two tall on a two-by-two footprint east of the lamp -
/// darkens the floor east of it, and only there.
#[test]
fn a_lamp_throws_a_house_across_the_floor() {
    let Some((img, stage)) = render(|world| {
        world.spawn((StageCaster {
            footprint: vec![[1.0, -1.0], [3.0, -1.0], [3.0, 1.0], [1.0, 1.0]],
            height: 2.0,
            opacity: 1.0,
            ..Default::default()
        },));
    }) else {
        return;
    };
    let at = |p: Vec3| at(&img, &stage, p);
    assert!(at(Vec3::new(-1.0, 0.0, 0.0)) > 200, "the lamp's side is white");
    // As dark as the lamp is strong there: (1 - d / 12)^2 x 2 - 0.4 at 3.6
    // east of it, 0.28 at 4.5. A point as far from the lamp but out of the
    // box's way stays white.
    let (near, far) = (at(Vec3::new(3.6, 0.0, 0.0)), at(Vec3::new(4.5, 0.0, 0.0)));
    assert!(near < 190 && near < far, "east of the box, in its shadow, darker nearer the lamp: {near} {far}");
    assert!(far < 215, "and still shadowed further on: {far}");
    assert!(at(Vec3::new(-3.0, -6.6, 0.0)) > 245, "as far from the lamp, out of the shadow, white");
    assert!(at(Vec3::new(2.0, -4.0, 0.0)) > 245, "and nothing to its south");
}
