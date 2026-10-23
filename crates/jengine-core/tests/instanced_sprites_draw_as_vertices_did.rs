//! **Rectangles are drawn as instances** (Oct 2026), and land where the
//! vertices the CPU used to make put them.
//!
//! A sprite was four vertices the CPU turned and wrote - 144 bytes, and the
//! corner arithmetic, per sprite per frame; in a browser that upload was a
//! tenth of a frame of ten thousand sprites. `WgpuSpriteBatch` now queues
//! each rectangle as one 68-byte instance and `sprite_instance.wgsl` makes
//! the corners, with the CPU's arithmetic step for step. Meshes, triangles
//! and free quads stay vertices, so a flush that mixes them draws them in
//! runs, in the order they were queued.
//!
//! Here: turned, anchored, sheared and plain rectangles, a free quad and a
//! mesh between them, through the default pipelines and a graph material,
//! drawn once instanced and once as vertices (`set_instancing(false)`) - the
//! two images must match. Without a GPU adapter it prints why and passes.
#![cfg(feature = "gpu-wgpu")]

use glam::{Mat4, Vec2, Vec4};
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::graph_pipeline_cache::{GraphPipelineCache, GraphPipelineKey};
use jengine_core::renderer::material::BlendMode;
use jengine_core::renderer::shader_graph::{NodeType, ShaderGraph};

const W: u32 = 160;
const H: u32 = 120;

/// The texture times the vertex colour - a graph drawing what the default does.
fn tint_graph() -> ShaderGraph {
    let mut g = ShaderGraph::new("tint");
    let uv = g.add_node(NodeType::UvInput, [0.0, 0.0]);
    let tex = g.add_node(NodeType::TextureSample, [0.0, 0.0]);
    g.connect(uv, 0, tex, 0);
    let color = g.add_node(NodeType::VertexColorInput, [0.0, 0.0]);
    let mul = g.add_node(NodeType::Multiply, [0.0, 0.0]);
    g.connect(tex, 0, mul, 0);
    g.connect(color, 0, mul, 1);
    let out = g.output_node().unwrap().id;
    g.connect(mul, 0, out, 0);
    g
}

/// A checker, so a texel landing one place over shows.
fn checker() -> RawTextureData {
    let mut rgba = Vec::new();
    for y in 0..8u32 {
        for x in 0..8u32 {
            let on = (x + y) % 2 == 0;
            rgba.extend_from_slice(if on { &[255, 220, 40, 255] } else { &[40, 90, 255, 200] });
        }
    }
    RawTextureData { width: 8, height: 8, rgba }
}

fn draw(gpu: &HeadlessGpu, instanced: bool) -> Vec<u8> {
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("checker".into(), checker());
    assets.ensure_wgpu_texture("checker", &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    batch.set_instancing(instanced);
    let mut cache = GraphPipelineCache::new();
    let key = GraphPipelineKey { shader_graph_path: "mem/tint.shader".into(), blend_mode: BlendMode::Alpha };
    let graph = cache.insert_from_graph(&gpu.device, gpu.color_format, &batch, key, &tint_graph()).unwrap();
    let vp: Mat4 = Camera2D::new(W as f32, H as f32).view_projection();
    let tex = assets.get_wgpu_texture("checker").unwrap();

    let queue_all = |b: &mut WgpuSpriteBatch| {
        b.draw_sprite_uv(Vec2::new(-50.0, 30.0), Vec2::new(37.0, 23.0), 0.4, Vec4::ONE, [0.0, 0.0, 1.0, 1.0]);
        b.draw_sprite_anchored(Vec2::new(10.0, 25.0), Vec2::new(31.0, 19.0), -1.1, Vec4::new(1.0, 0.8, 0.8, 1.0), [0.25, 0.0, 0.75, 1.0], [0.2, 0.9]);
        // A free quad between the rectangles: drawn in its place.
        b.draw_sprite_quad(Vec2::new(-20.0, 0.0), [[-12.0, 14.0], [18.0, 10.0], [14.0, -9.0], [-10.0, -12.0]], Vec4::ONE, [0.0, 0.0, 1.0, 1.0]);
        b.draw_sprite_anchored_skewed(Vec2::new(45.0, -20.0), Vec2::new(24.0, 40.0), 0.3, 0.6, Vec4::new(0.7, 1.0, 0.7, 0.9), [0.0, 0.0, 1.0, 1.0], [0.5, 0.0]);
        b.draw_mesh(
            &[Vec2::new(-70.0, -40.0), Vec2::new(-40.0, -45.0), Vec2::new(-45.0, -15.0)],
            &[[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.5, 1.0, 1.0]],
            &[[1.0; 4]; 3],
            &[0, 1, 2],
        );
        b.draw_sprite_anchored(Vec2::new(-5.0, -30.0), Vec2::new(50.0, 14.0), 2.2, Vec4::ONE, [0.0, 0.0, 1.0, 1.0], [0.5, 0.5]);
    };

    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        batch.begin();
        queue_all(&mut batch);
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, tex);
        batch.set_blend(BlendMode::Additive);
        batch.begin();
        queue_all(&mut batch);
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, tex);
        batch.set_blend(BlendMode::Alpha);
        let shifted = Mat4::from_translation(glam::Vec3::new(0.25, 0.1, 0.0)) * vp;
        batch.begin();
        queue_all(&mut batch);
        batch.flush_with_pipeline(&gpu.device, &gpu.queue, &mut pass, &shifted, tex, &graph);
    }
    gpu.submit(encoder);
    gpu.read_rgba().rgba
}

#[test]
fn instanced_rectangles_draw_what_vertices_drew() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("instanced_sprites_draw_as_vertices_did: no GPU adapter - skipped");
        return;
    };
    let vertices = draw(&gpu, false);
    let instanced = draw(&gpu, true);
    assert!(vertices.iter().any(|&c| c > 0), "the scene drew something");
    let differ: Vec<usize> = (0..vertices.len() / 4)
        .filter(|&i| vertices[i * 4..i * 4 + 4] != instanced[i * 4..i * 4 + 4])
        .collect();
    assert!(
        differ.is_empty(),
        "{} of {} pixels differ, first at ({}, {})",
        differ.len(),
        vertices.len() / 4,
        differ.first().map_or(0, |i| i % W as usize),
        differ.first().map_or(0, |i| i / W as usize),
    );
}
