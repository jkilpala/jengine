//! **Rich text** (Oct 2026): markup in world text - colours, bold, effects,
//! pauses and inline icons - and a typewriter. Text was one colour per
//! entity with no markup at all, so a line of dialogue that named a place
//! in gold took three entities placed by hand, and went wrong the moment
//! the line changed. Found by the showcase's The Storyteller.
//!
//! The parser and the timing are unit-tested in `renderer::rich_text`; this
//! draws through the real sprite pass: a word in red beside one in the
//! text's own white, a typewriter half way, and an icon between two words.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{TextComponent, TextReveal, Transform};

fn rich(text: &str) -> TextComponent {
    TextComponent { text: text.into(), rich: true, font_scale: 4.0, color: [1.0; 4], ..Default::default() }
}

#[test]
fn the_motion_step_runs_the_typewriter() {
    let mut world = World::new();
    let e = world.spawn((Transform::default(), TextComponent { reveal_speed: 20.0, ..rich("hello there") }));
    jengine_core::ecs::behaviors::tick_motion(&mut world, 0.25, None, false);
    jengine_core::ecs::behaviors::tick_motion(&mut world, 0.25, None, false);
    let t = world.get::<&TextComponent>(e).unwrap();
    assert!((t.reveal.unwrap().elapsed - 0.25).abs() < 1e-6, "started on the first tick, advanced on the second");
}

#[test]
fn an_icon_texture_is_preloaded() {
    let mut world = World::new();
    world.spawn((Transform::default(), rich("a [icon=sprites/key.png] key")));
    world.spawn((Transform::default(), TextComponent { text: "[icon=sprites/not.png]".into(), ..Default::default() }));
    let ids = jengine_core::scene::format::world_texture_ids(&world);
    assert!(ids.contains("sprites/key.png"));
    assert!(!ids.contains("sprites/not.png"), "only rich text reads markup");
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 512;
    const H: u32 = 128;

    /// The text drawn at the left edge, vertically centred; the image.
    fn draw(gpu: &HeadlessGpu, text: TextComponent) -> Vec<u8> {
        let (rgba, aw, ah, font) = built_in_font_atlas();
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
        assets.mark_sdf("__font");
        assets.insert_texture_bytes("icons/green.png".into(), RawTextureData { width: 4, height: 4, rgba: [0u8, 255, 0, 255].repeat(16) });
        for id in ["__font", "icons/green.png"] {
            assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
        }
        let fonts = FrameFonts::builtin_only(&font, "__font");
        let mut world = World::new();
        world.spawn((Transform { position: Vec2::new(-(W as f32) / 2.0 + 8.0, 40.0), ..Default::default() }, text));
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        let vp = Camera2D::new(W as f32, H as f32).view_projection();
        let mut encoder = gpu.encoder();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            batch.begin_frame();
            render_sprites_with_shadows_wgpu_materialized(
                &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                |b, id, kind| {
                    let t = assets.get_wgpu_texture(id).expect("uploaded");
                    if let SpriteShadowFlush::Sprite { .. } = kind {
                        b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                    }
                },
            );
        }
        gpu.submit(encoder);
        gpu.read_rgba().rgba
    }

    /// Lit pixels in columns `x0..x1`: (reddish, whitish, greenish).
    fn census(img: &[u8], x0: u32, x1: u32) -> (usize, usize, usize) {
        let (mut red, mut white, mut green) = (0, 0, 0);
        for y in 0..H {
            for x in x0..x1 {
                let i = ((y * W + x) * 4) as usize;
                let (r, g, b) = (img[i], img[i + 1], img[i + 2]);
                if r > 150 && g < 90 && b < 90 {
                    red += 1;
                } else if r > 150 && g > 150 && b > 150 {
                    white += 1;
                } else if g > 150 && r < 90 && b < 90 {
                    green += 1;
                }
            }
        }
        (red, white, green)
    }

    #[test]
    fn a_coloured_word_draws_in_its_colour() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("rich_text_draws_its_markup: no GPU adapter - skipped");
            return;
        };
        let img = draw(&gpu, rich("[color=#ff0000]HH[/color] HH"));
        let (red_l, white_l, _) = census(&img, 0, 95);
        let (red_r, white_r, _) = census(&img, 125, W);
        assert!(red_l > 200 && white_l < 20, "the marked word is red: {red_l} red, {white_l} white");
        assert!(white_r > 200 && red_r < 20, "the rest is the text's own colour: {red_r} red, {white_r} white");
        // Without `rich` the markup is the text.
        let plain = draw(&gpu, TextComponent { rich: false, ..rich("[color=#ff0000]HHHH[/color]") });
        let (red, white, _) = census(&plain, 0, W);
        assert!(red == 0 && white > 200, "brackets and all, in white");
    }

    #[test]
    fn the_typewriter_hides_what_it_has_not_reached() {
        let Some(gpu) = HeadlessGpu::new(W, H) else { return };
        let half = TextComponent {
            reveal_speed: 10.0,
            reveal: Some(TextReveal { key: 0, elapsed: 0.25 }),
            ..rich("HH HH")
        };
        let img = draw(&gpu, half);
        let (_, left, _) = census(&img, 0, 95);
        let (_, right, _) = census(&img, 125, W);
        assert!(left > 200, "the first word is out: {left}");
        assert_eq!(right, 0, "the second is not yet");
    }

    /// Plain text (not `rich`) types out too: `reveal_speed` was drawn only
    /// for rich text, while the dialogue counted the line as typing - so a
    /// plain line showed whole and the first press finished nothing.
    #[test]
    fn the_typewriter_types_plain_text_too() {
        let Some(gpu) = HeadlessGpu::new(W, H) else { return };
        let half = TextComponent {
            rich: false,
            reveal_speed: 10.0,
            reveal: Some(TextReveal { key: 0, elapsed: 0.25 }),
            ..rich("HH HH")
        };
        let img = draw(&gpu, half);
        let (_, left, _) = census(&img, 0, 95);
        let (_, right, _) = census(&img, 125, W);
        assert!(left > 200, "the first word is out: {left}");
        assert_eq!(right, 0, "the second is not yet");
        // With no reveal running, plain text draws whole as before.
        let whole = draw(&gpu, TextComponent { rich: false, ..rich("HH HH") });
        assert!(census(&whole, 125, W).1 > 200, "plain text with no typewriter draws whole");
    }

    #[test]
    fn an_icon_draws_between_the_words() {
        let Some(gpu) = HeadlessGpu::new(W, H) else { return };
        let img = draw(&gpu, rich("HH [icon=icons/green.png] HH"));
        let (_, white, green) = census(&img, 0, W);
        assert!(green > 400, "the icon's own texture: {green} green pixels");
        assert!(white > 200, "and the words around it");
    }
}
