//! A lightmap drawn by a graph material lights what is under it - through the
//! real lighting pass, with the material's parameters.
//!
//! A lightmap was a texture and nothing else, so light could not move: the
//! showcase's Underwater wanted caustics rippling over its seabed and had only
//! a still picture of them. A `LightmapComponent` on an entity with a
//! `material` is now drawn through that material's graph into the light
//! buffer - here a graph that outputs its `tint` parameter times `uv.x` (the tint's alpha is 1: an additive blend weights by alpha), over
//! a full-screen lightmap, under zero ambient: the left edge must stay dark,
//! the right lit in the tint's colour. Without a GPU adapter it prints why and
//! passes.
#![cfg(feature = "gpu-wgpu")]

use std::collections::HashMap;

use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{AmbientLight, LightmapComponent, MaterialComponent, Transform};
use jengine_core::ecs::wgpu_systems::render_lightmaps_wgpu;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuSpriteBatch};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::graph_pipeline_cache::{GraphPipelineCache, GraphPipelineKey};
use jengine_core::renderer::lighting::gather_lights;
use jengine_core::renderer::material::{BlendMode, UniformValue};
use jengine_core::renderer::shader_graph::{NodeType, ShaderGraph};

const W: u32 = 128;
const H: u32 = 64;

/// `tint * uv.x`.
fn ramp_graph() -> ShaderGraph {
    let mut g = ShaderGraph::new("light ramp");
    let uv = g.add_node(NodeType::UvInput, [0.0, 0.0]);
    let split = g.add_node(NodeType::Split, [0.0, 0.0]);
    g.connect(uv, 0, split, 0);
    let tint = g.add_node(NodeType::MaterialParam { name: "tint".into() }, [0.0, 0.0]);
    let mul = g.add_node(NodeType::Multiply, [0.0, 0.0]);
    g.connect(tint, 0, mul, 0);
    g.connect(split, 0, mul, 1);
    let out = g.output_node().unwrap().id;
    g.connect(mul, 0, out, 0);
    g
}

#[test]
fn a_lightmap_with_a_graph_material_draws_its_light() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("lightmap_graph_materials: no GPU adapter - skipped");
        return;
    };
    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();

    // The material and its graph, as the asset manager and the pipeline
    // cache record them on load and compile.
    let mut cache = GraphPipelineCache::new();
    let key = GraphPipelineKey { shader_graph_path: "mem/light_ramp.shader".into(), blend_mode: BlendMode::Additive };
    let pipeline = cache.insert_from_graph(&gpu.device, gpu.color_format, &batch, key, &ramp_graph()).unwrap();
    jengine_core::renderer::material_params::note_material("materials/light_ramp", "mem/light_ramp.shader");

    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    let mut uniforms = HashMap::new();
    uniforms.insert("tint".to_string(), UniformValue::Vec4([0.0, 1.0, 0.0, 1.0]));
    world.spawn((
        Transform::default(),
        LightmapComponent { texture_id: Default::default(), size: [W as f32, H as f32], ..Default::default() },
        MaterialComponent { material_id: "materials/light_ramp".into(), uniforms, ..Default::default() },
    ));

    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &gather_lights(&world));
    let mut materials_seen = Vec::new();
    {
        let mut pass = lp.light_buffer_pass(&mut encoder);
        render_lightmaps_wgpu(&world, &mut batch, |b, tex_id, material| {
            materials_seen.push(material.map(str::to_string));
            let tex = assets.get_wgpu_texture(tex_id).expect("the white texture");
            b.flush_with_pipeline(&gpu.device, &gpu.queue, &mut pass, &vp, tex, &pipeline);
        });
    }
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    assert_eq!(materials_seen, vec![Some("materials/light_ramp".to_string())], "the flush is told the material");

    let img = gpu.read_rgba();
    let px = |x: u32| {
        let i = (((H / 2) * W + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    let (left, right) = (px(2), px(W - 3));
    assert!(left[1] < 40, "uv.x is ~0 at the left edge, so no light there: {left:?}");
    assert!(right[1] > 200 && right[0] < 30 && right[2] < 30, "the right edge takes the green tint: {right:?}");
}
