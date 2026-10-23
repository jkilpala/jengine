//! **Silhouettes** (Oct 2026; `ecs::silhouette`): a thing a wall hides is
//! drawn again over the wall - filled and outlined in its own colour - only
//! where the wall's own pixels cover it. Rendered through the real sprite
//! pass on a headless device: a red thief half behind a wall whose left half
//! is opaque comes out green (the silhouette) where the wall covers it, red
//! where the wall is clear, and the wall stays blue past the thief. Found by
//! the showcase's Behind the Walls. Without a GPU adapter it prints why and
//! passes.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{SpriteComponent, Transform};
use jengine_core::ecs::silhouette::{Silhouette, SilhouetteOccluder};
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 64;
const H: u32 = 64;

/// An 8x8 texture: `opaque(x, y)` texels in `rgb`, the rest clear.
fn texture(rgb: [u8; 3], opaque: impl Fn(u32, u32) -> bool) -> RawTextureData {
    let mut rgba = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8 {
        for x in 0..8 {
            let a = if opaque(x, y) { 255 } else { 0 };
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
        }
    }
    RawTextureData { width: 8, height: 8, rgba }
}

/// The thief (`thief` texture, red, 40 x 40 at the middle, z 5) behind a
/// wall (40 x 40, its left half opaque blue, z 10), with `style`; the row
/// through the middle, rgb.
fn render(thief: RawTextureData, style: Silhouette) -> Option<Vec<[u8; 3]>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("silhouettes_show_through_walls: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.insert_texture_bytes("thief.png".into(), thief);
    assets.insert_texture_bytes("wall.png".into(), texture([0, 0, 255], |x, _| x < 4));
    for id in [WHITE_TEXTURE_ID, "thief.png", "wall.png"] {
        assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
    }
    let (rgba, aw, ah, font) = built_in_font_atlas();
    let mut fa = AssetManager::new();
    fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut world = World::new();
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "thief.png".into(), color: [1.0, 0.0, 0.0, 1.0], size: Some([40.0, 40.0]), z_order: 5, ..Default::default() },
        style,
    ));
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "wall.png".into(), size: Some([40.0, 40.0]), z_order: 10, ..Default::default() },
        SilhouetteOccluder::default(),
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
    Some((0..W).map(|x| {
        let i = ((H / 2 * W + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    }).collect())
}

fn is(p: [u8; 3], want: [u8; 3]) -> bool {
    p.iter().zip(want).all(|(a, b)| (*a as i32 - b as i32).abs() < 40)
}

#[test]
fn a_hidden_thief_shows_through_the_wall_only_where_it_is_hidden() {
    // A solid thief, filled green, no outline.
    let solid = texture([255, 255, 255], |_, _| true);
    let Some(row) = render(solid, Silhouette { color: [0.0, 1.0, 0.0, 1.0], outline: 0.0, fill: 1.0 }) else { return };
    // Screen x = world x + 32; the thief spans -20..20, the wall's opaque
    // half -20..0.
    assert!(is(row[22], [0, 255, 0]), "behind the wall: the silhouette, {:?}", row[22]);
    assert!(is(row[42], [255, 0, 0]), "where the wall is clear: the thief, {:?}", row[42]);
    assert!(is(row[4], [0, 0, 0]), "past both: nothing, {:?}", row[4]);
}

#[test]
fn the_outline_rings_the_hidden_shape() {
    // A thief opaque in its middle four texels (world -10..10); outlined one
    // texel (5 world units) wide, not filled.
    let middle = texture([255, 255, 255], |x, y| (2..6).contains(&x) && (2..6).contains(&y));
    let Some(row) = render(middle, Silhouette { color: [0.0, 1.0, 0.0, 1.0], outline: 1.0, fill: 0.0 }) else { return };
    assert!(is(row[19], [0, 255, 0]), "a texel outside the shape, behind the wall: the outline, {:?}", row[19]);
    assert!(is(row[27], [0, 0, 255]), "inside the shape, unfilled: the wall, {:?}", row[27]);
    assert!(is(row[13], [0, 0, 255]), "two texels out: the wall, {:?}", row[13]);
}
