//! A `trail` is recorded by the motion step every runtime runs and drawn by
//! the sprite pass every runtime draws with - read back as pixels.
//!
//! There was no trail at all before Sep 2026: a sword swing or a comet's
//! tail was a row of sprites a script spawned and faded itself. Without a
//! GPU adapter these print why and pass, like `render_golden`.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::trails::tick_trails;
use jengine_core::ecs::components::{SpriteComponent, Trail, Transform};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::ecs::wgpu_systems::{WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::material::BlendMode;
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 64;
const H: u32 = 64;

fn srgb_to_linear(b: u8) -> f64 {
    let v = b as f64 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// Draw `world` and read back the middle row, linear RGB per column - world
/// x `-32..32` left to right.
fn middle_row(world: &World) -> Option<Vec<[f64; 3]>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("trails_reach_the_screen: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    let (_, _, _, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, _| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    Some(
        (0..W)
            .map(|x| {
                let i = (((H / 2) * W + x) * 4) as usize;
                [srgb_to_linear(img.rgba[i]), srgb_to_linear(img.rgba[i + 1]), srgb_to_linear(img.rgba[i + 2])]
            })
            .collect(),
    )
}

fn red_trail() -> Trail {
    Trail {
        lifetime: 0.4,
        width: 12.0,
        width_end: 12.0,
        color: [1.0, 0.0, 0.0, 1.0],
        color_end: [1.0, 0.0, 0.0, 1.0],
        min_distance: 2.0,
        ..Trail::default()
    }
}

/// Move the entity right, 4 units a frame at 20 fps, from x -40 to x 20,
/// through the steps a host runs: the hierarchy resolve, then the trails
/// recording what it resolved.
fn run_right(world: &mut World, e: hecs::Entity) {
    for i in 0..=15 {
        world.get::<&mut Transform>(e).unwrap().position.x = -40.0 + 4.0 * i as f32;
        resolve_world_transforms(world);
        tick_trails(world, 0.05);
    }
}

#[test]
fn a_moving_entity_leaves_a_trail_behind_it() {
    let mut world = World::new();
    let e = world.spawn((Transform::default(), red_trail()));
    run_right(&mut world, e);
    let Some(row) = middle_row(&world) else { return };
    let at = |x: i32| row[(x + 32) as usize];
    // Just behind the head, and half a lifetime back: red.
    assert!(at(16)[0] > 0.9, "behind the head: {:?}", at(16));
    assert!(at(4)[0] > 0.9, "a few frames back: {:?}", at(4));
    // Ahead of the entity nothing has been drawn, and far enough behind the
    // points have aged out (0.4 s is 8 frames, 32 units).
    assert!(at(26)[0] < 0.01, "ahead of the head: {:?}", at(26));
    assert!(at(-24)[0] < 0.01, "points past their lifetime: {:?}", at(-24));
}

#[test]
fn a_trail_blends_and_sorts_like_a_sprite() {
    // An additive trail over a grey quad keeps the grey; a sprite with a
    // higher z covers the trail.
    let mut world = World::new();
    world.spawn((
        Transform { position: Vec2::new(8.0, 0.0), ..Default::default() },
        SpriteComponent {
            texture_id: Default::default(),
            color: [0.25, 0.25, 0.25, 1.0],
            size: Some([16.0, 16.0]),
            z_order: 0,
            ..Default::default()
        },
    ));
    world.spawn((
        Transform { position: Vec2::new(-8.0, 0.0), ..Default::default() },
        SpriteComponent {
            texture_id: Default::default(),
            color: [0.0, 0.0, 1.0, 1.0],
            size: Some([8.0, 8.0]),
            z_order: 5,
            ..Default::default()
        },
    ));
    let trail = Trail { color: [0.5, 0.0, 0.0, 1.0], color_end: [0.5, 0.0, 0.0, 1.0], z_order: 1, blend_mode: BlendMode::Additive, ..red_trail() };
    let e = world.spawn((Transform::default(), trail));
    run_right(&mut world, e);
    let Some(row) = middle_row(&world) else { return };
    let at = |x: i32| row[(x + 32) as usize];
    let over_grey = at(8);
    assert!((over_grey[0] - 0.75).abs() < 0.02 && (over_grey[1] - 0.25).abs() < 0.02, "additive over grey: {over_grey:?}");
    let under_blue = at(-8);
    assert!(under_blue[0] < 0.01 && under_blue[2] > 0.9, "the z 5 sprite covers the trail: {under_blue:?}");
}
