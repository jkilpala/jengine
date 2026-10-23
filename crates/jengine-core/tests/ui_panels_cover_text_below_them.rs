//! An entity-UI panel covers the text under it: widgets are drawn in
//! `z_order`, text included, so a menu panel opened over a HUD hides the
//! HUD's words rather than wearing them. Written while checking a racing game's
//! screenshot where a tagline seemed to show through a menu (it was the
//! panel's 92% opacity, not the order) - nothing else checked that text
//! obeys `z_order` among the widgets.
//! Also: an image with no texture draws as a plain white quad (a speech
//! bubble's card and tail), not as nothing.
//! Without a GPU adapter it prints why and passes, like `render_golden`.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::TextShadow;
use jengine_core::ecs::wgpu_systems::{UiFlush, render_ui_wgpu};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::text::built_in_font_atlas;
use jengine_core::ui::{TextAlign, UIAnchor, UIElement, UIImage, UIPanel, UIText};

const W: u32 = 256;
const H: u32 = 64;

fn draw(gpu: &HeadlessGpu, world: &World) -> Vec<u8> {
    let (rgba, aw, ah, font) = built_in_font_atlas();
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    assets.mark_sdf("__font");
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let vp = glam::Mat4::orthographic_rh_gl(0.0, W as f32, 0.0, H as f32, -1.0, 1.0);
    batch.begin_frame();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color { r: 0.0, g: 0.0, b: 0.5, a: 1.0 });
        let (device, queue) = (&gpu.device, &gpu.queue);
        let mut flush = |batch: &mut WgpuSpriteBatch, what: UiFlush<'_>| {
            let id = match what {
                UiFlush::Quad(None) => WHITE_TEXTURE_ID,
                UiFlush::Quad(Some(id)) | UiFlush::Font(id) => id,
                UiFlush::Clip(_) => return,
            };
            assets.ensure_wgpu_texture(id, device, queue).expect("known texture");
            batch.flush(device, queue, &mut pass, &vp, assets.get_wgpu_texture(id).expect("uploaded"));
        };
        render_ui_wgpu(world, &mut batch, Some(&fonts), W as f32, H as f32, &mut flush);
    }
    gpu.submit(encoder);
    gpu.read_rgba().rgba
}

fn whole_screen(z: i32) -> UIElement {
    UIElement { anchor: UIAnchor::TopLeft, offset: Vec2::ZERO, size: Vec2::new(W as f32, H as f32), visible: true, z_order: z, ..Default::default() }
}

fn bright(rgba: &[u8]) -> usize {
    rgba.chunks_exact(4).filter(|p| p[0] > 128 && p[1] > 128).count()
}

#[test]
fn a_panel_above_text_hides_it() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("ui_panels_cover_text_below_them: no GPU adapter - skipped");
        return;
    };
    for shadow in [None, Some(TextShadow { offset: [2.0, -2.0], color: [0.0, 0.0, 0.0, 0.85], softness: 0.0 })] {
        let text = || UIText {
            text: "WWWWWWWWWWWW".into(),
            font_id: String::new(),
            font_size: 40.0,
            color: [1.0, 1.0, 1.0, 1.0],
            alignment: TextAlign::Center,
            shadow,
            ..Default::default()
        };
        // The text alone is drawn...
        let mut world = World::new();
        world.spawn((whole_screen(2), text()));
        let alone = bright(&draw(&gpu, &world));
        assert!(alone > 200, "the text did not draw at all ({alone} px)");
        // ...and an opaque panel above it covers it, whichever was spawned first.
        for panel_first in [false, true] {
            let mut world = World::new();
            if panel_first {
                world.spawn((whole_screen(10), UIPanel { bg_color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }));
                world.spawn((whole_screen(2), text()));
            } else {
                world.spawn((whole_screen(2), text()));
                world.spawn((whole_screen(10), UIPanel { bg_color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }));
            }
            let covered = bright(&draw(&gpu, &world));
            assert_eq!(covered, 0, "text at z 2 shows through a panel at z 10 (shadow {}, panel spawned first {panel_first})", shadow.is_some());
        }
    }
}

/// An image whose `texture_id` is empty is the white texture tinted by its
/// colour. It used to be flushed as `Quad(Some(""))`, a texture no host has,
/// so a `speech_bubble` without art drew its words and no card (Iron Girders).
#[test]
fn an_image_with_no_texture_draws_its_colour() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("ui_panels_cover_text_below_them: no GPU adapter - skipped");
        return;
    };
    let mut world = World::new();
    world.spawn((whole_screen(1), UIImage { texture_id: String::new(), color: [1.0, 1.0, 0.0, 1.0], ..Default::default() }));
    let lit = bright(&draw(&gpu, &world));
    assert_eq!(lit, (W * H) as usize, "an untextured image covered {lit} of {} pixels", W * H);
}
