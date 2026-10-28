//! **Portals** (Oct 2026; `ecs::portal`): a linked pair of lines, each
//! drawn as the view out of the other. A view of the partner's front region
//! is drawn into a texture and the sprite pass draws it behind this portal,
//! turned half round - so what stands to the right in front of B shows to
//! the left behind A, as walking through A would bring you out of B. Drawn
//! through the real sprite pass and views on a headless device; carrying
//! bodies across is unit-tested in `ecs::portal`. Found by the showcase's
//! Portal Lab. Without a GPU adapter it prints why and passes.
#![cfg(all(feature = "gpu-wgpu", feature = "portal"))]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{Name, SpriteComponent, Transform};
use jengine_core::ecs::portal::{Portal, clear_active, portal_views};
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, VIEW_TARGET_FORMAT, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 128;

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

#[test]
fn what_stands_before_one_portal_shows_behind_the_other_turned_half_round() {
    let Some(gpu) = HeadlessGpu::new(W, W) else {
        eprintln!("portals_look_through: no GPU adapter - skipped");
        return;
    };
    let (_r, _w, _h, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let render = |paired: bool| -> Vec<[u8; 3]> {
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
        assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
        let mut world = World::new();
        // A at x -32, B at x 32, both along y = 0 facing up.
        let pair = |name: &str| Portal { pair: if paired { name.into() } else { String::new() }, width: 48.0, depth: 40.0, ..Default::default() };
        world.spawn((Name("A".into()), at(-32.0, 0.0), pair("B")));
        world.spawn((Name("B".into()), at(32.0, 0.0), pair("A")));
        // In front of B: red to its right, green to its left.
        world.spawn((at(42.0, 20.0), SpriteComponent { color: [1.0, 0.0, 0.0, 1.0], size: Some([8.0, 8.0]), ..Default::default() }));
        world.spawn((at(22.0, 20.0), SpriteComponent { color: [0.0, 1.0, 0.0, 1.0], size: Some([8.0, 8.0]), ..Default::default() }));
        jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
        let main = Camera2D::new(W as f32, W as f32);
        let views = portal_views(&world, &main);
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        batch.begin_frame();
        let mut encoder = gpu.encoder();
        macro_rules! draw {
            ($pass:expr, $vp:expr) => {{
                let assets = &assets;
                render_sprites_with_shadows_wgpu_materialized(
                    &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                    |b, tex, kind| {
                        if let SpriteShadowFlush::Sprite { .. } = kind {
                            b.flush(&gpu.device, &gpu.queue, $pass, $vp, assets.get_wgpu_texture(tex).unwrap());
                        }
                    },
                );
            }};
        }
        // Every view's target first, as a host sizes them: one view's pass
        // draws the other portal's window.
        for v in &views {
            assets.ensure_render_texture(&v.target, v.size.0, v.size.1, VIEW_TARGET_FORMAT, &gpu.device);
        }
        for v in &views {
            batch.set_target_format(&gpu.device, VIEW_TARGET_FORMAT);
            set_frame_view(&v.camera);
            let vp = v.camera.view_projection();
            let target_view = assets.get_wgpu_texture(&v.target).unwrap().view.clone();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("portal"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            draw!(&mut pass, &vp);
        }
        batch.set_target_format(&gpu.device, gpu.color_format);
        set_frame_view(&main);
        let vp = main.view_projection();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            draw!(&mut pass, &vp);
        }
        gpu.submit(encoder);
        clear_frame_view();
        clear_active();
        gpu.read_rgba().rgba.chunks(4).map(|p| [p[0], p[1], p[2]]).collect()
    };
    // World (x, y) is pixel (64 + x, 64 - y).
    let px = |img: &[[u8; 3]], x: i32, y: i32| img[((64 - y) as u32 * W + (64 + x) as u32) as usize];
    let img = render(true);
    // Behind A, as far as they stand in front of B, left and right swapped:
    // red (10 right of B) 10 left of A, green (10 left of B) 10 right of A.
    let red = px(&img, -42, -20);
    let green = px(&img, -22, -20);
    assert!(red[0] > 200 && red[1] < 40, "red shows to A's left: {red:?}");
    assert!(green[1] > 200 && green[0] < 40, "green to A's right: {green:?}");
    // And behind B, A's front is empty: nothing shows there.
    assert!(px(&img, 42, -20).iter().all(|&c| c < 10));
    let none = render(false);
    assert!(px(&none, -42, -20).iter().all(|&c| c < 10), "unpaired, a portal shows nothing");
}
