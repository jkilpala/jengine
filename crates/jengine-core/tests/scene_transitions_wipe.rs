//! **Scene transitions** (Oct 2026; `renderer::transition`, the pass in
//! `WgpuTransition`): the last frame of the old scene, held in a texture,
//! drawn over the new scene's frame as each style leaves it. These draw the
//! real pass on a headless device - the old frame red, the new one green -
//! and look at where each style has opened onto the new scene halfway. The
//! frame copy and the timing ride `WgpuFrame::present`, which needs a
//! window; the state machine is unit-tested in `renderer::transition`.
//! Found by the showcase's Curtain Call. Without a GPU adapter they print
//! why and pass.
#![cfg(feature = "gpu-wgpu")]

use jengine_core::renderer::backend::wgpu::WgpuTransition;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::transition::Style;

const W: u32 = 64;
const H: u32 = 32;

/// The new frame (green) with the old one (red) drawn over it as `style`
/// leaves it at `progress`: each pixel's (red, green).
fn frame(gpu: &HeadlessGpu, style: Style, progress: f32) -> Vec<(u8, u8)> {
    let mut t = WgpuTransition::new(&gpu.device, gpu.color_format, gpu.color_format);
    t.prepare(&gpu.device, &gpu.queue, W, H);
    gpu.queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: t.held_texture().expect("held"),
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[255u8, 0, 0, 255].repeat((W * H) as usize),
        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: Some(H) },
        wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
    );
    let mut encoder = gpu.encoder();
    {
        let _clear = gpu.color_pass(&mut encoder, wgpu::Color { r: 0.0, g: 1.0, b: 0.0, a: 1.0 });
    }
    t.draw(&mut encoder, &gpu.queue, gpu.view(), style, progress);
    gpu.submit(encoder);
    gpu.read_rgba().rgba.chunks(4).map(|p| (p[0], p[1])).collect()
}

fn old(p: (u8, u8)) -> bool {
    p.0 > 200 && p.1 < 50
}

fn new(p: (u8, u8)) -> bool {
    p.1 > 200 && p.0 < 50
}

fn at(img: &[(u8, u8)], x: u32, y: u32) -> (u8, u8) {
    img[(y * W + x) as usize]
}

#[test]
fn each_style_opens_onto_the_new_scene_its_own_way() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("scene_transitions_wipe: no GPU adapter - skipped");
        return;
    };
    // At the start it is all the old frame, at the end all the new.
    for style in [Style::Fade, Style::Iris, Style::Diamond, Style::Slide, Style::Wipe, Style::Dissolve] {
        let start = frame(&gpu, style, 0.0);
        assert!(start.iter().all(|&p| old(p)), "{style:?} starts on the old frame");
        let end = frame(&gpu, style, 1.0);
        assert!(end.iter().all(|&p| new(p)), "{style:?} ends on the new one");
    }
    // An iris opens in the middle; the corners are still old.
    let iris = frame(&gpu, Style::Iris, 0.5);
    assert!(new(at(&iris, W / 2, H / 2)) && old(at(&iris, 0, 0)) && old(at(&iris, W - 1, H - 1)));
    // A slide has moved the old frame half off to the left; a wipe has
    // uncovered the left side.
    let slide = frame(&gpu, Style::Slide, 0.5);
    assert!(old(at(&slide, 4, H / 2)) && new(at(&slide, W - 4, H / 2)), "the old frame is on the left");
    let wipe = frame(&gpu, Style::Wipe, 0.5);
    assert!(new(at(&wipe, 4, H / 2)) && old(at(&wipe, W - 4, H / 2)), "the new scene is on the left");
    // A crossfade is between the two everywhere.
    let fade = frame(&gpu, Style::Fade, 0.5);
    assert!(fade.iter().all(|&(r, g)| r > 80 && g > 80), "{:?}", fade[0]);
    // Diamonds open on the left first: more new there than on the right.
    let d = frame(&gpu, Style::Diamond, 0.4);
    let share = |xs: std::ops::Range<u32>| {
        let n = xs.len() as u32 * H;
        (0..H).flat_map(|y| xs.clone().map(move |x| (x, y))).filter(|&(x, y)| new(at(&d, x, y))).count() as f32 / n as f32
    };
    assert!(share(0..W / 4) > share(W * 3 / 4..W) + 0.2, "{} vs {}", share(0..W / 4), share(W * 3 / 4..W));
}

#[test]
fn a_dissolve_burns_away_the_darkest_of_its_texture_first() {
    let Some(gpu) = HeadlessGpu::new(W, H) else { return };
    // A mask dark on the left, light on the right, handed over as a
    // texture's bytes would be.
    let mut rgba = Vec::new();
    for _ in 0..4 {
        for x in 0..8u32 {
            let v = (x * 255 / 7) as u8;
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
    }
    jengine_core::renderer::transition::arm("dissolve", 1.0, "mask.png");
    let mut assets = jengine_core::asset::AssetManager::new();
    assets.insert_texture_bytes("mask.png".into(), jengine_core::asset::RawTextureData { width: 8, height: 4, rgba });
    jengine_core::renderer::transition::sync_mask(&assets);
    let img = frame(&gpu, Style::Dissolve, 0.5);
    assert!(new(at(&img, 2, H / 2)) && old(at(&img, W - 3, H / 2)), "dark burns first: {:?} / {:?}", at(&img, 2, H / 2), at(&img, W - 3, H / 2));
    jengine_core::renderer::transition::reset();
}

/// The parts no headless device can run: `present` draws the held frame and
/// copies the finished one, the swap starts the transition, and native
/// holds its swap a frame so the old scene is presented first.
#[test]
fn the_frame_and_the_hosts_carry_it() {
    let read = |p: &str| std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(p)).unwrap();
    let renderer = read("crates/jengine-core/src/renderer/backend/wgpu/renderer.rs");
    let present = &renderer[renderer.find("pub fn present(").expect("present")..];
    let present = &present[..present.find("\n    }\n").expect("end of present")];
    assert!(present.contains("t.draw(") && present.contains("t.copy_from("), "present draws and copies");
    assert!(present.find("t.draw(") < present.find("c.encode("), "a screenshot includes the transition");
    let streaming = read("crates/jengine-core/src/scene/streaming.rs");
    assert!(streaming.contains("transition::on_scene_replaced()"), "the swap starts it");
    let native = read("crates/jengine-runtime/src/wgpu_scene_runner.rs");
    assert!(native.contains("transition::holds_scene_change()"), "native holds its swap a frame");
}
