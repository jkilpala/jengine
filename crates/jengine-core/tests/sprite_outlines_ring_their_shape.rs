//! **Sprite outlines** (Oct 2026; `ecs::sprite_outline`): a glowing ring
//! round a sprite's own shape. Rendered through the real sprite pass on a
//! headless device: a red square in the middle of a mostly clear texture
//! comes out ringed in green just outside its pixels - not round its
//! rectangle - solid out to the width, fading through the glow, and a
//! neighbour in the same atlas is never ringed. Found by the showcase's Unit
//! Select. Without a GPU adapter it prints why and passes.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{Masked, Name, SpriteComponent, SpriteMask, Transform};
use jengine_core::ecs::sprite_outline::SpriteOutline;
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
use jengine_core::renderer::mask::MaskShape;
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 96;
const H: u32 = 64;

/// A `w` x 8 texture: `opaque(x, y)` texels white, the rest clear.
fn texture(w: u32, opaque: impl Fn(u32, u32) -> bool) -> RawTextureData {
    let mut rgba = Vec::with_capacity((w * 8 * 4) as usize);
    for y in 0..8 {
        for x in 0..w {
            let a = if opaque(x, y) { 255 } else { 0 };
            rgba.extend_from_slice(&[255, 255, 255, a]);
        }
    }
    RawTextureData { width: w, height: 8, rgba }
}

/// One red 40 x 40 sprite at the middle (`uv` of `tex`), outlined `o`; the
/// row through the middle, rgb.
fn render(tex: RawTextureData, uv: Option<[f32; 4]>, o: SpriteOutline) -> Option<Vec<[u8; 3]>> {
    render_masked(tex, uv, o, false)
}

/// As [`render`], the sprite clipped (when `left_only`) to the left half of
/// the view by a `sprite_mask`.
fn render_masked(tex: RawTextureData, uv: Option<[f32; 4]>, o: SpriteOutline, left_only: bool) -> Option<Vec<[u8; 3]>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("sprite_outlines_ring_their_shape: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.insert_texture_bytes("unit.png".into(), tex);
    for id in [WHITE_TEXTURE_ID, "unit.png"] {
        assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
    }
    let (rgba, aw, ah, font) = built_in_font_atlas();
    let mut fa = AssetManager::new();
    fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut world = World::new();
    let unit = world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "unit.png".into(), color: [1.0, 0.0, 0.0, 1.0], size: Some([40.0, 40.0]), uv_rect: uv, z_order: 5, ..Default::default() },
        o,
    ));
    if left_only {
        world.spawn((
            Transform { position: Vec2::new(-24.0, 0.0), ..Default::default() },
            Name("Cover".into()),
            SpriteMask { shape: MaskShape::Rect { size: [48.0, 64.0] }, feather: 0.0 },
        ));
        world.insert_one(unit, Masked { by: "Cover".into(), outside: false }).unwrap();
    }
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

const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

#[test]
fn the_ring_follows_the_shape_not_the_rectangle() {
    // Opaque in the middle four texels: world -10..10 of the sprite's
    // -20..20. Screen x = world x + 48.
    let middle = texture(8, |x, y| (2..6).contains(&x) && (2..6).contains(&y));
    let o = SpriteOutline { color: GREEN, width: 6.0, glow: 0.0, ..Default::default() };
    let Some(row) = render(middle, None, o) else { return };
    assert!(is(row[48], [255, 0, 0]), "the shape itself: the sprite, {:?}", row[48]);
    assert!(is(row[48 + 13], [0, 255, 0]), "three units out: the ring, {:?}", row[61]);
    assert!(is(row[48 - 13], [0, 255, 0]), "and on the left, {:?}", row[35]);
    assert!(is(row[48 + 19], [0, 0, 0]), "past the width, still inside the rectangle: nothing, {:?}", row[67]);
}

#[test]
fn the_glow_fades_past_the_width() {
    let middle = texture(8, |x, y| (2..6).contains(&x) && (2..6).contains(&y));
    let o = SpriteOutline { color: GREEN, width: 2.0, glow: 16.0, ..Default::default() };
    let Some(row) = render(middle, None, o) else { return };
    // The shape's edge is 10 units out; the ring solid to 12, gone by 28.
    let at = |d: usize| row[48 + 10 + d][1] as i32;
    assert!(at(1) > 200, "on the width: solid, {}", at(1));
    // Half way through the glow, about half (a dithered sixth either way).
    assert!((60..200).contains(&at(10)), "half way: fading, {}", at(10));
    assert!(at(10) < at(4), "fading outward: {} then {}", at(4), at(10));
    assert!(at(20) < 20, "past the glow: nothing, {}", at(20));
}

#[test]
fn a_neighbour_in_the_atlas_is_not_ringed() {
    // A 16 x 8 sheet: the left half the square, the right half solid. The
    // sprite shows the left half; the ring reaches past it into the right
    // half's texture coordinates, which must read clear.
    let sheet = texture(16, |x, y| x >= 8 || ((2..6).contains(&x) && (2..6).contains(&y)));
    let o = SpriteOutline { color: GREEN, width: 6.0, glow: 0.0, ..Default::default() };
    let Some(row) = render(sheet, Some([0.0, 0.0, 0.5, 1.0]), o) else { return };
    assert!(is(row[48 + 13], [0, 255, 0]), "the ring round its own square, {:?}", row[61]);
    // Past the sprite's right edge (world 20) the quad samples the
    // neighbour: without the bounds it would be ringed solid there.
    assert!(is(row[48 + 23], [0, 0, 0]), "the neighbour's texels are not its shape, {:?}", row[71]);
}

#[test]
fn a_disabled_outline_draws_nothing() {
    let middle = texture(8, |x, y| (2..6).contains(&x) && (2..6).contains(&y));
    let o = SpriteOutline { color: GREEN, width: 6.0, glow: 0.0, enabled: false, ..Default::default() };
    let Some(row) = render(middle, None, o) else { return };
    assert!(is(row[48 + 13], [0, 0, 0]), "off: no ring, {:?}", row[61]);
    assert!(is(row[48], [255, 0, 0]), "the sprite as ever, {:?}", row[48]);
}

/// The ring is clipped with its sprite: a man sunk behind cover must not be
/// ringed round the legs the cover hides (Shooter Classics' Crossfire - the
/// outline was its own draw and carried no mask).
#[test]
fn a_masked_sprites_ring_is_clipped_with_it() {
    let middle = texture(8, |x, y| (2..6).contains(&x) && (2..6).contains(&y));
    let o = SpriteOutline { color: GREEN, width: 6.0, glow: 0.0, ..Default::default() };
    let Some(row) = render_masked(middle, None, o, true) else { return };
    assert!(is(row[48 - 13], [0, 255, 0]), "inside the mask: ringed, {:?}", row[35]);
    assert!(is(row[48 + 13], [0, 0, 0]), "outside it: no ring, {:?}", row[61]);
}
