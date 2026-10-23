//! A graph's `TexelSize` node knows the texture it samples: `Texel` is one
//! texel in UV and `Size` the size in pixels - what an outline or an edge
//! detect samples its neighbours with, a texel apart at any texture size.
//!
//! A graph that outputs `(Size.x / 8, Size.y / 8, Texel.x, 1)` over a 4x2
//! texture must draw `(0.5, 0.25, 0.25)`. Without a GPU adapter it prints why
//! and passes.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::{MaterialComponent, SpriteComponent, Transform};
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::graph_pipeline_cache::{GraphPipelineCache, GraphPipelineKey};
use jengine_core::renderer::material::BlendMode;
use jengine_core::renderer::shader_graph::{NodeType, ShaderGraph};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 32;
const H: u32 = 32;

fn srgb_to_linear(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn size_graph() -> ShaderGraph {
    let mut g = ShaderGraph::new("texel size");
    let ts = g.add_node(NodeType::TexelSize, [0.0, 0.0]);
    let eighth = g.add_node(NodeType::Vec2Constant { value: [0.125, 0.125] }, [0.0, 0.0]);
    let scaled = g.add_node(NodeType::Multiply, [0.0, 0.0]);
    g.connect(ts, 1, scaled, 0);
    g.connect(eighth, 0, scaled, 1);
    let s = g.add_node(NodeType::Split, [0.0, 0.0]);
    g.connect(scaled, 0, s, 0);
    let t = g.add_node(NodeType::Split, [0.0, 0.0]);
    g.connect(ts, 0, t, 0);
    let one = g.add_node(NodeType::FloatConstant { value: 1.0 }, [0.0, 0.0]);
    let rgba = g.add_node(NodeType::Combine { components: 4 }, [0.0, 0.0]);
    g.connect(s, 0, rgba, 0);
    g.connect(s, 1, rgba, 1);
    g.connect(t, 0, rgba, 2);
    g.connect(one, 0, rgba, 3);
    let out = g.output_node().unwrap().id;
    g.connect(rgba, 0, out, 0);
    g
}

#[test]
fn a_graph_reads_its_textures_size() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("graph_texel_size: no GPU adapter - skipped");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("art/4x2.png".into(), RawTextureData { width: 4, height: 2, rgba: vec![255; 4 * 2 * 4] });
    assets.ensure_wgpu_texture("art/4x2.png", &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let mut cache = GraphPipelineCache::new();
    let key = GraphPipelineKey { shader_graph_path: "mem/texel_size.shader".into(), blend_mode: BlendMode::Alpha };
    let pipeline = cache.insert_from_graph(&gpu.device, gpu.color_format, &batch, key, &size_graph()).unwrap();

    let mut world = World::new();
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "art/4x2.png".into(), size: Some([W as f32, H as f32]), ..Default::default() },
        MaterialComponent { material_id: "materials/texel_size".into(), ..Default::default() },
    ));
    let (_, _, _, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, kind| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                match kind {
                    SpriteShadowFlush::Sprite { material_id: Some(_) } => {
                        b.flush_with_pipeline(&gpu.device, &gpu.queue, &mut pass, &vp, t, &pipeline)
                    }
                    _ => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                };
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let i = (((H / 2) * W + W / 2) * 4) as usize;
    let c = [srgb_to_linear(img.rgba[i]), srgb_to_linear(img.rgba[i + 1]), srgb_to_linear(img.rgba[i + 2])];
    assert!((c[0] - 0.5).abs() < 0.01, "Size.x / 8 = 0.5: {c:?}");
    assert!((c[1] - 0.25).abs() < 0.01, "Size.y / 8 = 0.25: {c:?}");
    assert!((c[2] - 0.25).abs() < 0.01, "Texel.x = 1/4: {c:?}");
}
