//! A sprite marked `y_sort` draws in front of one further up the screen.
//!
//! Until Sep 2026 the only draw order a sprite had was `z_order` (plus its
//! sort layer's base), fixed when the scene was authored. That is enough for
//! a backdrop or a HUD and useless for the case where y *is* depth: a
//! beat-em-up's floor, an isometric room, anything where a character walks
//! behind a tree by standing above it. Two actors on the same floor share a
//! z_order, so the pair drew in whatever order the ECS query happened to
//! return them in — which is to say the one that shipped was the one that
//! spawned last.
//!
//! The ordering is the whole feature and it is invisible in code, so the
//! render half is checked through the real pipeline and read back pixel by
//! pixel. Needs a GPU; without one it prints why and passes. The key
//! arithmetic underneath is checked separately and needs nothing.

use jengine_core::ecs::components::SpriteComponent;

fn sprite(texture: &str, z: i32, size: f32, y_sort: bool) -> SpriteComponent {
    SpriteComponent {
        texture_id: texture.into(),
        z_order: z,
        size: Some([size, size]),
        y_sort,
        ..SpriteComponent::default()
    }
}

#[test]
fn lower_on_screen_sorts_later() {
    let s = sprite("a", 0, 32.0, true);
    // Larger key = drawn later = on top. Down the screen is toward -y, so
    // the key has to rise as y falls or the whole thing is inside out.
    assert!(s.y_sort_key(-100.0) > s.y_sort_key(100.0));
    assert!(s.y_sort_key(0.0) > s.y_sort_key(0.5));
    assert_eq!(s.y_sort_key(0.0), 0);
}

#[test]
fn a_sprite_that_did_not_opt_in_has_no_position_key() {
    let s = sprite("a", 0, 32.0, false);
    // Flat zero whatever its position: this is what keeps an ordinary band
    // grouped by texture instead of scattered by where its sprites sit.
    assert_eq!(s.y_sort_key(-900.0), 0);
    assert_eq!(s.y_sort_key(900.0), 0);
}

#[test]
fn the_key_survives_positions_no_game_would_use() {
    let s = sprite("a", 0, 32.0, true);
    // A runaway position must not wrap around into the front of the band.
    assert!(s.y_sort_key(f32::MAX) < s.y_sort_key(0.0));
    assert!(s.y_sort_key(f32::MIN) > s.y_sort_key(0.0));
    // NaN sorts to the back rather than silently becoming 0 and landing in
    // the middle of the actors.
    assert_eq!(s.y_sort_key(f32::NAN), i32::MAX);
}

#[cfg(feature = "gpu-wgpu")]
mod rendered {
    use super::sprite;
    use glam::Vec2;
    use hecs::World;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::ecs::components::Transform;
    // The pass every runtime draws its world with. This test used to render
    // through `render_sprites_wgpu_multi`, which no runtime's main pass uses,
    // so it kept passing while the real pass ignored `y_sort` (BUGS #162).
    use jengine_core::ecs::wgpu_systems::{render_sprites_with_shadows_wgpu_materialized, SpriteShadowFlush};
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::text::built_in_font_atlas;
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;

    const W: u32 = 256;
    const H: u32 = 128;
    const RED: [u8; 3] = [220, 40, 40];
    const BLUE: [u8; 3] = [50, 80, 230];
    const GREEN: [u8; 3] = [40, 200, 60];

    fn solid(rgba: [u8; 4]) -> RawTextureData {
        RawTextureData { width: 32, height: 32, rgba: rgba.repeat(32 * 32) }
    }

    fn at(x: f32, y: f32) -> Transform {
        Transform { position: Vec2::new(x, y), ..Transform::default() }
    }

    /// The colour at a world point, after drawing `world`.
    fn render_and_sample(world: &World, x: f32, y: f32) -> Option<[u8; 3]> {
        let gpu = HeadlessGpu::new(W, H)?;
        let mut assets = AssetManager::new();
        for (id, c) in [("red", RED), ("blue", BLUE), ("green", GREEN)] {
            assets.insert_texture_bytes(id.into(), solid([c[0], c[1], c[2], 255]));
            assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).expect("upload");
        }
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
        let camera = Camera2D::new(W as f32, H as f32);
        let vp = camera.view_projection();
        let mut encoder = gpu.encoder();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            let (_, _, _, font) = built_in_font_atlas();
            let fonts = FrameFonts::builtin_only(&font, "__font/builtin");
            let flush = |b: &mut WgpuSpriteBatch, tex: &str, _kind: SpriteShadowFlush<'_>| {
                let texture = assets.get_wgpu_texture(tex).expect("texture uploaded");
                b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
            };
            batch.begin_frame();
            render_sprites_with_shadows_wgpu_materialized(
                world,
                &SortLayers::default(),
                &mut batch,
                &fonts,
                Vec2::splat(64.0),
                jengine_core::ecs::wgpu_systems::WorldLayer::All,
                flush,
            );
        }
        gpu.submit(encoder);
        let image = gpu.read_rgba();
        let px = (x + W as f32 / 2.0) as u32;
        let py = (H as f32 / 2.0 - y) as u32;
        let i = ((py * image.width + px) * 4) as usize;
        Some([image.rgba[i], image.rgba[i + 1], image.rgba[i + 2]])
    }

    fn is(c: [u8; 3], want: [u8; 3]) -> bool {
        c.iter().zip(want.iter()).all(|(a, b)| (*a as i16 - *b as i16).abs() <= 8)
    }

    /// Two actors on one floor, overlapping between y -10 and y 10. The
    /// blue one stands lower, so it is nearer the camera.
    ///
    /// Blue is spawned **first** deliberately: with no y sorting the later
    /// red would cover it, which is exactly the bug. So this asserts the
    /// order changed, not merely that it happens to be right.
    fn two_actors(y_sort: bool) -> World {
        let mut world = World::new();
        world.spawn((at(0.0, -20.0), sprite("blue", 0, 60.0, y_sort)));
        world.spawn((at(0.0, 20.0), sprite("red", 0, 60.0, y_sort)));
        world
    }

    #[test]
    fn the_nearer_actor_covers_the_further_one() {
        let Some(c) = render_and_sample(&two_actors(true), 0.0, 0.0) else {
            eprintln!("skipped: no GPU adapter for the headless renderer");
            return;
        };
        assert!(is(c, BLUE), "the actor standing lower should be in front, got {c:?}");
    }

    #[test]
    fn without_the_flag_the_spawn_order_still_decides() {
        let Some(c) = render_and_sample(&two_actors(false), 0.0, 0.0) else {
            eprintln!("skipped: no GPU adapter for the headless renderer");
            return;
        };
        // Not an endorsement - a record of what the flag is opting out of,
        // and proof the two paths differ.
        assert!(is(c, RED), "with no y sorting the last spawned covers, got {c:?}");
    }

    #[test]
    fn sorting_happens_inside_the_band_not_across_it() {
        // Green is lower on the screen than red and would be in front of
        // it - but it is a backdrop, one z below. A band boundary has to
        // outrank position or `z_order` would stop meaning anything the
        // moment a scene turned this on.
        let mut world = World::new();
        world.spawn((at(0.0, -20.0), sprite("green", -1, 60.0, true)));
        world.spawn((at(0.0, 20.0), sprite("red", 0, 60.0, true)));
        let Some(c) = render_and_sample(&world, 0.0, 0.0) else {
            eprintln!("skipped: no GPU adapter for the headless renderer");
            return;
        };
        assert!(is(c, RED), "a lower z stays behind however low it sits, got {c:?}");
    }
}
