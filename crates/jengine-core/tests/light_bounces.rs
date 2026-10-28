//! **Global illumination** (`global_illumination`, Oct 2026): light that
//! bounces, by radiance cascades on the GPU. The lit outlines of the
//! occluders shine again, so a pixel no light reaches is lit by a red-lit
//! floor beside it - red - and an occluder between them takes that away.
//! Checked on real pixels: the cascades are all in WGSL, with no CPU twin.
#![cfg(all(feature = "gpu-wgpu", feature = "gi"))]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::{
    AmbientLight, GlobalIllumination, LightOccluder, PointLight2D, SpriteComponent, Transform,
};
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuRenderTarget, WgpuSpriteBatch};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::lighting::{gather_lights, light_at};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 128;
const H: u32 = 64;
const FLOAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

fn bar(x0: f32, y0: f32, x1: f32, y1: f32) -> (Transform, LightOccluder) {
    (
        Transform::default(),
        LightOccluder { polygon: vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], ..Default::default() },
    )
}

/// A white room with no ambient light, a floor bar along the bottom, and a
/// red lamp just above the floor's left end that reaches nothing else.
fn room(gi: Option<f32>, wall: bool) -> World {
    let mut w = World::new();
    w.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    w.spawn((
        Transform { position: Vec2::new(0.0, 0.0), ..Default::default() },
        SpriteComponent { texture_id: "white.png".into(), size: Some([W as f32, H as f32]), ..Default::default() },
    ));
    w.spawn(bar(-62.0, -30.0, 62.0, -22.0));
    if wall {
        w.spawn(bar(-4.0, -18.0, 4.0, 31.0));
    }
    w.spawn((
        Transform { position: Vec2::new(-40.0, -14.0), ..Default::default() },
        PointLight2D { color: [1.0, 0.0, 0.0], intensity: 1.0, radius: 22.0, falloff: 1.0, ..Default::default() },
    ));
    if let Some(strength) = gi {
        w.spawn((GlobalIllumination { strength, scale: 2, cascades: 0 },));
    }
    w
}

fn f16(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f32;
    match e {
        0 => s * m * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => s * (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

/// The frame, in linear floats (r, g, b per pixel).
fn render(gpu: &HeadlessGpu, world: &World) -> Vec<[f32; 3]> {
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white.png".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white.png", &gpu.device, &gpu.queue).unwrap();
    let lp = WgpuLightingPass::new(&gpu.device, W, H, FLOAT).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, FLOAT).unwrap();
    let dest = WgpuRenderTarget::new(&gpu.device, W, H, FLOAT).unwrap();
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let atlas = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&atlas.3, "__font");
    let mut encoder = gpu.encoder();
    {
        let mut pass = lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE));
        batch.begin_frame();
        let assets = &assets;
        render_sprites_with_shadows_wgpu_materialized(
            world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, kind| {
                if let SpriteShadowFlush::Sprite { .. } = kind {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(tex).unwrap());
                }
            },
        );
    }
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &gather_lights(world));
    lp.composite(&mut encoder, &dest.view);
    // Read the float frame back: rows padded to 256 bytes.
    let row = (W * 8).div_ceil(256) * 256;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("light_bounces readback"),
        size: (row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture { texture: &dest.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(H) },
        },
        wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
    );
    gpu.submit(encoder);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    gpu.device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity((W * H) as usize);
    for y in 0..H {
        for x in 0..W {
            let at = (y * row + x * 8) as usize;
            let h = |i: usize| f16(u16::from_le_bytes([data[at + i * 2], data[at + i * 2 + 1]]));
            out.push([h(0), h(1), h(2)]);
        }
    }
    out
}

/// The pixel `(x, y)` world units from the frame's centre, y up.
fn at(img: &[[f32; 3]], x: i32, y: i32) -> [f32; 3] {
    img[((H as i32 / 2 - y) as u32 * W + (W as i32 / 2 + x) as u32) as usize]
}

#[test]
fn light_bounces_off_a_lit_floor_and_not_through_a_wall() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("light_bounces: no GPU adapter - skipped");
        return;
    };
    let probe = (24, 12);
    // No light reaches the probe directly.
    let direct = light_at(Vec2::new(probe.0 as f32, probe.1 as f32), &gather_lights(&room(None, false)));
    assert!(direct.x < 1e-4, "the lamp does not reach it: {direct}");
    let dark = at(&render(&gpu, &room(None, false)), probe.0, probe.1);
    assert!(dark[0] < 0.01, "without a bounce it is dark: {dark:?}");
    let lit = at(&render(&gpu, &room(Some(6.0), false)), probe.0, probe.1);
    assert!(lit[0] > 0.02, "the red floor lights it: {lit:?}");
    assert!(lit[1] < lit[0] * 0.2 && lit[2] < lit[0] * 0.2, "red, as the floor is lit red: {lit:?}");
    // The lamp's own neighbourhood is lit as before, the bounce on top.
    let near = at(&render(&gpu, &room(Some(6.0), false)), -40, -14);
    assert!(near[0] > 0.5, "the lamp still lights its own place: {near:?}");
    // A wall between the lit floor and the probe takes most of it away.
    let walled = at(&render(&gpu, &room(Some(6.0), true)), probe.0, probe.1);
    assert!(walled[0] < lit[0] * 0.6, "the wall shades the bounce: {walled:?} against {lit:?}");
}
