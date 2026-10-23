//! A sprite's, an unlit sprite's and a particle emitter's blend mode reach
//! the pixels.
//!
//! Until Sep 2026 none did. `MaterialComponent.blend_mode` had a field, an
//! inspector, a doc comment and a test-lab scene labelled "Alpha | Additive |
//! Multiply", and no renderer read it: the flush for a material without a
//! shader graph was always the alpha pipeline. Particles had no blend field
//! at all, and the runner said so in a comment. So every "additive" glow in
//! every scene drew alpha-blended, which is most of why glows looked flat.
//!
//! Each test draws a coloured quad over a grey one through the real render
//! pass, and reads back the overlap: additive keeps the grey underneath and
//! adds to it; alpha replaces it. Without a GPU adapter they print why and
//! pass, like `render_golden`.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{MaterialComponent, SpriteComponent, Transform, Unlit};
use jengine_core::ecs::wgpu_systems::{
    SpriteShadowFlush, WorldLayer, render_particles_wgpu, render_sprites_with_shadows_wgpu_materialized,
};
use jengine_core::particles::{ParticleEmitter, ParticleSystem};
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

fn assets(gpu: &HeadlessGpu) -> AssetManager {
    let mut a = AssetManager::new();
    a.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    a.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    a
}

fn quad(color: [f32; 4], z: i32) -> SpriteComponent {
    SpriteComponent { texture_id: Default::default(), color, size: Some([32.0, 32.0]), z_order: z, ..Default::default() }
}

fn with_blend(blend: BlendMode) -> MaterialComponent {
    MaterialComponent { material_id: "default_sprite".into(), blend_mode: Some(blend), ..Default::default() }
}

/// The centre pixel, linear RGB.
fn centre(gpu: &HeadlessGpu) -> [f64; 3] {
    let img = gpu.read_rgba();
    let i = (((H / 2) * W + W / 2) * 4) as usize;
    [srgb_to_linear(img.rgba[i]), srgb_to_linear(img.rgba[i + 1]), srgb_to_linear(img.rgba[i + 2])]
}

/// A grey quad (0.25) under a red one (0.5, 0, 0), drawn by the world pass.
/// `red_material` decides how the red one blends.
fn world_overlap(red_material: Option<MaterialComponent>) -> Option<[f64; 3]> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("blend_modes_reach_the_screen: no GPU adapter - skipped");
        None
    })?;
    let assets = assets(&gpu);
    let (rgba, aw, ah, font) = built_in_font_atlas();
    let mut fa = AssetManager::new();
    fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    fa.mark_sdf("__font");
    let fonts = FrameFonts::builtin_only(&font, "__font");

    let mut world = World::new();
    world.spawn((Transform::default(), quad([0.25, 0.25, 0.25, 1.0], 0)));
    match red_material {
        Some(m) => world.spawn((Transform::default(), quad([0.5, 0.0, 0.0, 1.0], 1), m)),
        None => world.spawn((Transform::default(), quad([0.5, 0.0, 0.0, 1.0], 1))),
    };

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0),
            jengine_core::ecs::wgpu_systems::WorldLayer::All,
            |b, tex, kind| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                match kind {
                    SpriteShadowFlush::Shadow => b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                    SpriteShadowFlush::Sprite { .. } => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                };
            },
        );
    }
    gpu.submit(encoder);
    Some(centre(&gpu))
}

#[test]
fn an_additive_sprite_adds_to_what_is_under_it() {
    let Some(alpha) = world_overlap(None) else { return };
    assert!((alpha[0] - 0.5).abs() < 0.01 && alpha[1] < 0.01, "alpha replaces: {alpha:?}");

    let Some(add) = world_overlap(Some(with_blend(BlendMode::Additive))) else { return };
    assert!((add[0] - 0.75).abs() < 0.02, "0.25 + 0.5 red, got {add:?}");
    assert!((add[1] - 0.25).abs() < 0.01, "the grey underneath survives in green, got {add:?}");

    // A built-in material named for its blend means the same.
    let named = MaterialComponent { material_id: "additive".into(), ..Default::default() };
    let Some(by_name) = world_overlap(Some(named)) else { return };
    assert!((by_name[1] - 0.25).abs() < 0.01, "material \"additive\" adds, got {by_name:?}");
}

#[test]
fn an_unlit_sprite_keeps_its_blend() {
    let Some(gpu) = HeadlessGpu::new(W, H) else { return };
    let assets = assets(&gpu);
    let mut world = World::new();
    world.spawn((Transform::default(), quad([0.25, 0.25, 0.25, 1.0], 0), Unlit {}));
    world.spawn((Transform::default(), quad([0.5, 0.0, 0.0, 1.0], 1), Unlit {}, with_blend(BlendMode::Additive)));

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        // The emission layer is where a lit frame draws its unlit things.
        let (rgba, aw, ah, font) = built_in_font_atlas();
        let _ = (rgba, aw, ah);
        let fonts = FrameFonts::builtin_only(&font, "__font");
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::Emission,
            |b, tex, _| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
            },
        );
    }
    gpu.submit(encoder);
    let c = centre(&gpu);
    assert!((c[0] - 0.75).abs() < 0.02 && (c[1] - 0.25).abs() < 0.01, "unlit and additive, got {c:?}");
    assert_eq!(batch.blend(), BlendMode::Alpha, "the pass leaves the batch as it found it");
}

#[test]
fn additive_particles_add_up() {
    let Some(gpu) = HeadlessGpu::new(W, H) else { return };
    let assets = assets(&gpu);
    // Two particles on one spot, each 0.3 grey: additive makes 0.6, alpha
    // would leave the front one's 0.3.
    let emitter = ParticleEmitter {
        active: false,
        speed: [0.0, 0.0],
        lifetime: [10.0, 10.0],
        size_start: [20.0, 20.0],
        size_end: [20.0, 20.0],
        color_start: [0.3, 0.3, 0.3, 1.0],
        color_end: [0.3, 0.3, 0.3, 1.0],
        blend_mode: BlendMode::Additive,
        ..Default::default()
    };
    let mut world = World::new();
    world.spawn((Transform::default(), emitter.clone()));
    let id = world.iter().next().unwrap().entity().id() as u64;
    let mut system = ParticleSystem::new();
    system.burst(id, &emitter, Vec2::ZERO, 2);

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_particles_wgpu(&world, &SortLayers::default(), &system, &mut batch, |b, tex| {
            let t = assets.get_wgpu_texture(tex.unwrap_or(WHITE_TEXTURE_ID)).expect("uploaded");
            b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
        });
    }
    gpu.submit(encoder);
    let c = centre(&gpu);
    assert!((c[0] - 0.6).abs() < 0.02, "two additive particles at 0.3 make 0.6, got {c:?}");
    assert_eq!(batch.blend(), BlendMode::Alpha, "the pass leaves the batch as it found it");
}

/// Grey 0.25 under a multiply quad of `color`: the centre, linear.
fn multiply_over_grey(color: [f32; 4]) -> Option<[f64; 3]> {
    let gpu = HeadlessGpu::new(W, H)?;
    let assets = assets(&gpu);
    let (_, _, _, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut world = World::new();
    world.spawn((Transform::default(), quad([0.25, 0.25, 0.25, 1.0], 0)));
    world.spawn((Transform::default(), quad(color, 1), with_blend(BlendMode::Multiply)));
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, _| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
            },
        );
    }
    gpu.submit(encoder);
    Some(centre(&gpu))
}

/// A multiply sprite darkens by its colour where it is opaque and leaves what
/// is under it alone where it is not - never brightens. The blend computes
/// `src.rgb * dst + (1 - src.a) * dst`, which is that only for a colour
/// premultiplied by its alpha, and the sprite shader's is straight: a half
/// transparent white multiply came out `1.5 * dst`, and a fully transparent
/// texel of a white texture `2 * dst` - so a soft scorch decal lit its whole
/// quad as a pale rectangle (the showcase's Spellcraft). The multiply
/// pipelines draw through `fs_premultiplied` now.
#[test]
fn a_multiply_sprite_never_brightens() {
    let Some(white_half) = multiply_over_grey([1.0, 1.0, 1.0, 0.5]) else { return };
    assert!((white_half[0] - 0.25).abs() < 0.01, "white at half alpha multiplies by 1: {white_half:?}");
    let Some(clear) = multiply_over_grey([0.3, 0.3, 0.3, 0.0]) else { return };
    assert!((clear[0] - 0.25).abs() < 0.01, "a transparent multiply changes nothing: {clear:?}");
    let Some(black_half) = multiply_over_grey([0.0, 0.0, 0.0, 0.5]) else { return };
    assert!((black_half[0] - 0.125).abs() < 0.01, "black at half alpha halves: {black_half:?}");
    let Some(red) = multiply_over_grey([0.5, 0.0, 0.0, 1.0]) else { return };
    assert!((red[0] - 0.125).abs() < 0.01 && red[1] < 0.01, "an opaque multiply is a multiply: {red:?}");
}
