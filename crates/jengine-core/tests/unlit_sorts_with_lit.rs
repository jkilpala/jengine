//! An `Unlit` sprite sorts with lit ones by `z_order` and still ignores the
//! light.
//!
//! Until Sep 2026 unlit things were their own pass after the lighting
//! composite, so they sorted among themselves and always drew over every lit
//! sprite whatever its z - the visual showcase's Neon Rain met it as lit
//! windows on a far tower showing through the building in front. Now the
//! world pass draws an unlit sprite as a black cutout in the scene buffer
//! (`WorldLayer::Scene`) and as itself in the emission buffer
//! (`WorldLayer::Emission`), where lit sprites in front cut it out; the
//! composite adds emission after the light.
//!
//! This draws three columns under **zero** ambient, through the real passes
//! in the order every runtime runs them, and reads each back:
//!
//! - unlit red behind a lit white quad: hidden (black - the white is unlit
//!   by a zero light);
//! - unlit red alone: full red, although nothing lights it;
//! - unlit green in front of a lit white quad: full green.
//!
//! Without a GPU adapter it prints why and passes, like `render_golden`.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{AmbientLight, SpriteComponent, Transform, Unlit};
use jengine_core::ecs::wgpu_systems::{
    SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized, render_world_items,
};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuSpriteBatch};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::lighting::gather_lights;
use jengine_core::renderer::text::built_in_font_atlas;

/// Whether a lit frame of `world` draws its emission pass - decided from the
/// items collected, as `draw_frame` decides it.
fn draws_emission(world: &World) -> bool {
    let (_, _, _, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    render_world_items(world, &SortLayers::default(), &fonts, glam::Vec2::splat(64.0), |c| c.draws_emission())
}

/// The emission pass ran only when some entity carried `Unlit`, so every
/// unlit thing the engine draws itself - here a lightning bolt - vanished
/// from a lit scene with nothing else unlit in it. Found by the Test Lab's
/// lightning scene, whose bolt lit the floor and was never seen.
#[test]
fn engine_drawn_unlit_things_need_the_emission_pass_too() {
    use jengine_core::ecs::lightning::{Lightning, strike, tick_lightning};
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.1 },));
    world.spawn(quad(0.0, [1.0; 4], 0));
    assert!(!draws_emission(&world), "nothing unlit: no emission pass");
    let cloud = world.spawn((Transform::default(), Lightning::default()));
    strike(&mut world, cloud, 0.0, -100.0).unwrap();
    tick_lightning(&mut world, 0.0);
    assert!(draws_emission(&world), "a strike draws unlit, with no `Unlit` anywhere");
}

const W: u32 = 96;
const H: u32 = 32;

fn quad(x: f32, color: [f32; 4], z: i32) -> (Transform, SpriteComponent) {
    (
        Transform { position: Vec2::new(x, 0.0), rotation: 0.0, scale: Vec2::ONE },
        SpriteComponent { texture_id: Default::default(), color, size: Some([16.0, 16.0]), z_order: z, ..Default::default() },
    )
}

/// Render `world` as a lit frame does - the scene layer, the emission layer,
/// the lights and the composite - and return the sRGB pixel at each x on the
/// middle row.
fn render(world: &World, xs: &[f32]) -> Option<Vec<[u8; 3]>> {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("unlit_sorts_with_lit: no GPU adapter - skipped");
        return None;
    };
    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).expect("upload");
    let (_, _, _, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");

    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    batch.begin_frame();
    for (layer, clear) in [(WorldLayer::Scene, true), (WorldLayer::Emission, false)] {
        let mut pass = if clear {
            lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK))
        } else {
            lp.emission_pass(&mut encoder)
        };
        render_sprites_with_shadows_wgpu_materialized(
            world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), layer,
            |b, tex, kind| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                match kind {
                    SpriteShadowFlush::Shadow => b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                    SpriteShadowFlush::Sprite { .. } => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                };
            },
        );
    }
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &gather_lights(world));
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);

    let img = gpu.read_rgba();
    Some(
        xs.iter()
            .map(|&x| {
                let (px, py) = ((x + W as f32 / 2.0) as u32, H / 2);
                let i = ((py * W + px) * 4) as usize;
                [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
            })
            .collect(),
    )
}

#[test]
fn an_unlit_sprite_is_hidden_by_a_lit_one_in_front_and_glows_otherwise() {
    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
    const WHITE: [f32; 4] = [1.0; 4];
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.0 },));
    // Left: unlit behind lit.
    let (t, s) = quad(-30.0, RED, 0);
    world.spawn((t, s, Unlit {}));
    world.spawn(quad(-30.0, WHITE, 1));
    // Middle: unlit alone.
    let (t, s) = quad(0.0, RED, 0);
    world.spawn((t, s, Unlit {}));
    // Right: unlit in front of lit.
    world.spawn(quad(30.0, WHITE, 0));
    let (t, s) = quad(30.0, GREEN, 1);
    world.spawn((t, s, Unlit {}));
    assert!(draws_emission(&world));

    let Some(px) = render(&world, &[-30.0, 0.0, 30.0]) else { return };
    let (hidden, alone, front) = (px[0], px[1], px[2]);
    assert!(
        hidden.iter().all(|&c| c < 8),
        "an unlit sprite behind a lit one shows through it: {hidden:?} (it used to draw over every lit sprite)"
    );
    assert!(alone[0] > 245 && alone[1] < 8, "an unlit sprite under zero light should be full red, got {alone:?}");
    assert!(
        front[1] > 245 && front[0] < 8 && front[2] < 8,
        "an unlit sprite in front of a lit one should be full green, got {front:?}"
    );
}

/// A multiply sprite darkens what is behind it even when that is unlit
/// (emission). It used to be skipped in the emission layer, so dust drawn
/// over a glowing nebula left the glow untouched - the showcase's Nebula.
#[test]
fn a_multiply_sprite_darkens_unlit_things_behind_it() {
    use jengine_core::ecs::components::MaterialComponent;
    use jengine_core::renderer::material::BlendMode;
    let half = MaterialComponent { material_id: "default_sprite".into(), blend_mode: Some(BlendMode::Multiply), ..Default::default() };
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.0 },));
    // Left: unlit white, with an unlit multiply at 0.2 in front. Right: the
    // same with a lit multiply. Middle: unlit white alone, for scale.
    for (x, unlit_filter) in [(-30.0, true), (30.0, false)] {
        let (t, s) = quad(x, [1.0; 4], 0);
        world.spawn((t, s, Unlit {}));
        let (t, s) = quad(x, [0.2, 0.2, 0.2, 1.0], 1);
        if unlit_filter {
            world.spawn((t, s, half.clone(), Unlit {}));
        } else {
            world.spawn((t, s, half.clone()));
        }
    }
    let (t, s) = quad(0.0, [1.0; 4], 0);
    world.spawn((t, s, Unlit {}));

    let Some(px) = render(&world, &[-30.0, 0.0, 30.0]) else { return };
    assert!(px[1][0] > 245, "the bare unlit sprite is white: {:?}", px[1]);
    for (i, what) in [(0, "an unlit multiply"), (2, "a lit multiply")] {
        // 0.2 linear is about 124 in sRGB.
        let c = px[i];
        assert!(
            c.iter().all(|&v| (110..=135).contains(&v)),
            "{what} in front of an unlit white sprite should darken it to 0.2, got {c:?}"
        );
    }
}
