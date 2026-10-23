//! A shader graph's `Noise` and `Voronoi` nodes build a pipeline and draw
//! noise, through the real render pass.
//!
//! Until Sep 2026 a graph had no noise at all - Unity's Shader Graph has Simple
//! Noise, Gradient Noise and Voronoi, Godot a noise texture - and a dissolve,
//! a flicker or a water caustic had to paste noise in as raw WGSL. The
//! showcase's Shader Gallery is built on these two. A graph that does not
//! compile falls back to the default sprite shader in silence
//! (`GraphPipelineCache`), so the test checks the pipeline builds *and* that
//! the pixels vary as noise does. Without a GPU adapter it prints why and
//! passes.
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
use jengine_core::renderer::material::{BlendMode, MaterialData};
use jengine_core::renderer::shader_graph::{NodeId, NodeType, ShaderGraph};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 128;
const H: u32 = 128;

/// Render one sprite filling the frame through `graph`; the red channel of
/// every pixel, 0..255.
fn render(graph: &ShaderGraph) -> Option<Vec<u8>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("shader_graph_noise: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white.png".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white.png", &gpu.device, &gpu.queue).expect("upload");
    assets.insert_material("g".into(), MaterialData { shader_graph_path: Some("mem/g.shader".into()), ..Default::default() });
    let (_, _, _, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");

    let layout = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut cache = GraphPipelineCache::new();
    let pipeline = cache
        .insert_from_graph(
            &gpu.device,
            gpu.color_format,
            &layout,
            GraphPipelineKey { shader_graph_path: "mem/g.shader".into(), blend_mode: BlendMode::Alpha },
            graph,
        )
        .expect("the graph compiles to a pipeline");

    let mut world = World::new();
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "white.png".into(), size: Some([W as f32, H as f32]), ..Default::default() },
        MaterialComponent { material_id: "g".into(), ..Default::default() },
    ));
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        let assets = &assets;
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
    Some(img.rgba.chunks(4).map(|p| p[0]).collect())
}

/// output = (v, v, v, 1) for the float `value` of node `src`, output `pin`.
fn grey_from(graph: &mut ShaderGraph, src: NodeId, pin: usize) {
    let one = graph.add_node(NodeType::FloatConstant { value: 1.0 }, [0.0, 0.0]);
    let rgba = graph.add_node(NodeType::Combine { components: 4 }, [0.0, 0.0]);
    for i in 0..3 {
        graph.connect(src, pin, rgba, i);
    }
    graph.connect(one, 0, rgba, 3);
    let out = graph.output_node().unwrap().id;
    graph.connect(rgba, 0, out, 0);
}

fn uv_times(graph: &mut ShaderGraph, node: NodeType, scale: f32) -> NodeId {
    let uv = graph.add_node(NodeType::UvInput, [0.0, 0.0]);
    let s = graph.add_node(NodeType::FloatConstant { value: scale }, [0.0, 0.0]);
    let n = graph.add_node(node, [0.0, 0.0]);
    graph.connect(uv, 0, n, 0);
    graph.connect(s, 0, n, 1);
    n
}

/// Spread (max - min) and how many distinct values: noise varies smoothly
/// across the frame; a failed build (the default shader) is flat white.
fn spread(px: &[u8]) -> (u8, usize) {
    let (lo, hi) = (*px.iter().min().unwrap(), *px.iter().max().unwrap());
    let mut seen = [false; 256];
    for &p in px {
        seen[p as usize] = true;
    }
    (hi - lo, seen.iter().filter(|s| **s).count())
}

#[test]
fn a_noise_node_draws_smooth_varied_noise() {
    let mut graph = ShaderGraph::new("noise");
    let n = uv_times(&mut graph, NodeType::Noise { octaves: 4 }, 6.0);
    grey_from(&mut graph, n, 0);
    let Some(px) = render(&graph) else { return };
    let (range, distinct) = spread(&px);
    assert!(range > 100 && distinct > 60, "noise should vary widely and smoothly: range {range}, {distinct} distinct values");
    // Smooth: neighbours rarely differ by much.
    let jumps = px.windows(2).filter(|w| (w[0] as i32 - w[1] as i32).abs() > 40).count();
    assert!(jumps < px.len() / 50, "value noise is smooth, not static: {jumps} big jumps");
}

#[test]
fn a_voronoi_node_draws_cells() {
    let mut graph = ShaderGraph::new("voronoi");
    let v = uv_times(&mut graph, NodeType::Voronoi, 5.0);
    grey_from(&mut graph, v, 0);
    let Some(px) = render(&graph) else { return };
    let (range, _) = spread(&px);
    assert!(range > 120, "the distance to the nearest cell point runs from near 0 to far: range {range}");

    // The per-cell value is flat inside a cell and jumps at its border.
    let mut graph = ShaderGraph::new("cells");
    let v = uv_times(&mut graph, NodeType::Voronoi, 5.0);
    grey_from(&mut graph, v, 1);
    let Some(px) = render(&graph) else { return };
    let (_, distinct) = spread(&px);
    assert!((8..=80).contains(&distinct), "a few dozen cells, each one flat value: {distinct} distinct values");
}

#[test]
fn a_graph_without_noise_carries_none_of_its_helpers() {
    let mut graph = ShaderGraph::new("plain");
    let uv = graph.add_node(NodeType::UvInput, [0.0, 0.0]);
    let tex = graph.add_node(NodeType::TextureSample, [0.0, 0.0]);
    graph.connect(uv, 0, tex, 0);
    let out = graph.output_node().unwrap().id;
    graph.connect(tex, 0, out, 0);
    let wgsl = jengine_core::renderer::shader_codegen::generate(&graph).unwrap().wgsl;
    assert!(!wgsl.contains("jn_fbm"), "the noise library is appended only when a node calls it");
}

#[test]
#[ignore = "writes a picture for eyeballing"]
fn dump_noise_picture() {
    let mut graph = ShaderGraph::new("noise");
    let n = uv_times(&mut graph, NodeType::Noise { octaves: 5 }, 12.0);
    grey_from(&mut graph, n, 0);
    let Some(px) = render(&graph) else { return };
    let rgba: Vec<u8> = px.iter().flat_map(|&v| [v, v, v, 255]).collect();
    let out = std::env::var("NOISE_PNG").unwrap_or_else(|_| "noise.png".into());
    image::save_buffer(&out, &rgba, W, H, image::ColorType::Rgba8).unwrap();
}

#[test]
fn noise_is_continuous_far_from_the_origin() {
    // Neighbouring pixels stay close everywhere, even far from the origin.
    // The noise hashes lattice cells with integers because a float hash
    // (fract(p * 123.34)) gave the Shader Gallery's lava and Test Lab scene 115
    // diagonal seams along the octaves' lattice lines on screen - one corner
    // hashed two ways by its two neighbouring cells. **This test did not
    // reproduce those seams**: the headless adapter compiles that arithmetic
    // differently from the display runtime's backend, so the float hash
    // passes here too. It guards continuity in general; scene 115 is where the
    // seam shows if it comes back.
    let mut graph = ShaderGraph::new("far");
    let uv = graph.add_node(NodeType::UvInput, [0.0, 0.0]);
    let off = graph.add_node(NodeType::Vec2Constant { value: [30_000.5, 17_000.25] }, [0.0, 0.0]);
    let add = graph.add_node(NodeType::Add, [0.0, 0.0]);
    graph.connect(uv, 0, add, 0);
    graph.connect(off, 0, add, 1);
    let s = graph.add_node(NodeType::FloatConstant { value: 3.0 }, [0.0, 0.0]);
    let n = graph.add_node(NodeType::Noise { octaves: 2 }, [0.0, 0.0]);
    graph.connect(add, 0, n, 0);
    graph.connect(s, 0, n, 1);
    grey_from(&mut graph, n, 0);
    let Some(px) = render(&graph) else { return };
    let mut worst = 0i32;
    for y in 1..H as usize - 1 {
        for x in 1..W as usize - 1 {
            let v = px[y * W as usize + x] as i32;
            worst = worst.max((v - px[y * W as usize + x + 1] as i32).abs());
            worst = worst.max((v - px[(y + 1) * W as usize + x] as i32).abs());
        }
    }
    assert!(worst < 24, "noise jumps by {worst}/255 between neighbouring pixels - a seam along the lattice");
}
