//! **Afterimages** (Oct 2026; `ecs::afterimage`): a sprite moving right
//! leaves fading copies of itself behind, drawn through the real sprite
//! pass on a headless device - the copies sit where it was, older ones
//! fainter, the sprite itself over its newest copy, and `flat` copies are
//! its shape in their own colour rather than the tinted picture (blue over
//! a red texture, where a tinted copy would be black). Found by the
//! showcase's Dash Echoes. Without a GPU adapter it prints why and passes.
#![cfg(all(feature = "gpu-wgpu", feature = "afterimage"))]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::afterimage::{Afterimage, tick_afterimages};
use jengine_core::ecs::components::{SpriteComponent, Transform};
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 64;
const H: u32 = 32;

/// A 6 x 6 sprite run from x -24 to 0 in steps of 6, leaving blue echoes;
/// the image, rgb a pixel, rows from the top.
fn render(texture: [u8; 3], flat: bool) -> Option<Vec<[u8; 3]>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("afterimages_trail_behind: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    let rgba = [texture[0], texture[1], texture[2], 255].repeat(16);
    assets.insert_texture_bytes("runner.png".into(), RawTextureData { width: 4, height: 4, rgba });
    for id in [WHITE_TEXTURE_ID, "runner.png"] {
        assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
    }
    let (rgba, aw, ah, font) = built_in_font_atlas();
    let mut fa = AssetManager::new();
    fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut world = World::new();
    let runner = world.spawn((
        Transform { position: Vec2::new(-24.0, 0.0), ..Default::default() },
        SpriteComponent { texture_id: "runner.png".into(), size: Some([6.0, 6.0]), ..Default::default() },
        Afterimage {
            interval: 0.04,
            lifetime: 0.3,
            color: [0.0, 0.0, 1.0, 1.0],
            end_color: [0.0, 0.0, 1.0, 0.0],
            flat,
            ..Default::default()
        },
    ));
    for _ in 0..4 {
        tick_afterimages(&mut world, 0.05);
        world.get::<&mut Transform>(runner).unwrap().position.x += 6.0;
    }
    tick_afterimages(&mut world, 0.05);
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
    Some((0..W * H).map(|i| {
        let i = (i * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    }).collect())
}

/// The pixel at world `x` on the runner's row (screen x = world x + 32).
fn at(img: &[[u8; 3]], x: i32) -> [u8; 3] {
    img[((H / 2) * W) as usize + (x + 32) as usize]
}

#[test]
fn echoes_trail_behind_and_fade() {
    let Some(img) = render([255, 255, 255], false) else { return };
    let me = at(&img, 0);
    assert!(me.iter().all(|&c| c > 200), "the runner, white, over its newest echo: {me:?}");
    let (old, new) = (at(&img, -24), at(&img, -6));
    assert!(old[2] > 20 && old[0] < 30, "the oldest echo is blue where it was: {old:?}");
    assert!(new[2] > old[2] + 40, "and fainter than a newer one: {old:?} vs {new:?}");
    assert_eq!(at(&img, 12), [0, 0, 0], "nothing ahead of it");
}

#[test]
fn flat_echoes_are_their_colour() {
    let Some(img) = render([255, 0, 0], true) else { return };
    let e = at(&img, -6);
    assert!(e[2] > 100 && e[0] < 40, "a flat echo of a red sprite is blue: {e:?}");
    let Some(img) = render([255, 0, 0], false) else { return };
    let e = at(&img, -6);
    assert!(e.iter().all(|&c| c < 40), "a tinted one is red times blue, black: {e:?}");
}
