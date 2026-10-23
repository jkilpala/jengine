//! **Sprite tilt** (Oct 2026; `ecs::sprite_tilt`): a sprite turned about its
//! own axes is drawn in perspective - foreshortened, not bent along the
//! quad's diagonal - and turned over it shows its back. Rendered through the
//! real sprite pass on a headless device: a card half red, half green,
//! turned hard about its up axis, keeps the line between its halves exactly
//! where its middle projects (an affine mapping puts it six pixels off), its
//! near side is taller than its far one, and turned right over it is its
//! blue back. Found by the showcase's Card Table. Without a GPU adapter it
//! prints why and passes.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{SpriteComponent, Transform};
use jengine_core::ecs::sprite_tilt::SpriteTilt;
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 64;
const H: u32 = 64;

fn texture(f: impl Fn(u32) -> [u8; 3]) -> RawTextureData {
    let mut rgba = Vec::with_capacity(8 * 8 * 4);
    for _y in 0..8 {
        for x in 0..8 {
            let c = f(x);
            rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    RawTextureData { width: 8, height: 8, rgba }
}

/// A 40 x 40 card at the middle - left half red, right half green, its back
/// blue - turned by `tilt`; the image, rgb a pixel, rows from the top.
fn render(tilt: SpriteTilt) -> Option<Vec<[u8; 3]>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("sprite_tilt_in_perspective: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.insert_texture_bytes("face.png".into(), texture(|x| if x < 4 { [255, 0, 0] } else { [0, 255, 0] }));
    assets.insert_texture_bytes("back.png".into(), texture(|_| [0, 0, 255]));
    for id in [WHITE_TEXTURE_ID, "face.png", "back.png"] {
        assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
    }
    let (rgba, aw, ah, font) = built_in_font_atlas();
    let mut fa = AssetManager::new();
    fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut world = World::new();
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "face.png".into(), size: Some([40.0, 40.0]), ..Default::default() },
        SpriteTilt { back_texture: "back.png".into(), ..tilt },
    ));
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

fn px(img: &[[u8; 3]], x: u32, y: u32) -> [u8; 3] {
    img[(y * W + x) as usize]
}

fn red(p: [u8; 3]) -> bool {
    p[0] > 150 && p[1] < 100
}

fn green(p: [u8; 3]) -> bool {
    p[1] > 150 && p[0] < 100
}

/// How many pixels of column `x` are not black.
fn height(img: &[[u8; 3]], x: u32) -> u32 {
    (0..H).filter(|&y| px(img, x, y).iter().any(|&c| c > 60)).count() as u32
}

#[test]
fn a_turned_card_is_foreshortened_in_perspective() {
    // Turned a radian about its up axis, seen from 40 units: the left edge
    // swings to about x -18.6, the right to 7.6, and the middle stays at 0.
    let Some(img) = render(SpriteTilt { yaw: 1.0, distance: 40.0, ..Default::default() }) else { return };
    let row = H / 2;
    let first_green = (0..W).find(|&x| green(px(&img, x, row))).expect("some green");
    // Screen x = world x + 32: perspective-correct, the halves meet at the
    // middle (32); an affine mapping would put the line near 26.
    assert!((31..=33).contains(&first_green), "the halves meet where the middle projects: {first_green}");
    assert!(red(px(&img, 20, row)), "the near side is the red half: {:?}", px(&img, 20, row));
    let (near, far) = (height(&img, 16), height(&img, 38));
    assert!(near > 45 && far < 35, "the near side is taller than the far: {near} vs {far}");
    assert_eq!(height(&img, 44), 0, "and the card no longer reaches its old right edge");
}

#[test]
fn turned_over_a_card_shows_its_back() {
    let Some(img) = render(SpriteTilt { yaw: std::f32::consts::PI, ..Default::default() }) else { return };
    let p = px(&img, 32, H / 2);
    assert!(p[2] > 150 && p[0] < 60 && p[1] < 60, "its blue back: {p:?}");
    // Untilted, it is its face.
    let Some(img) = render(SpriteTilt::default()) else { return };
    assert!(red(px(&img, 20, H / 2)) && green(px(&img, 44, H / 2)));
}
