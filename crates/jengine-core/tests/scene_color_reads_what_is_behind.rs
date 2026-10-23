//! A graph's `SceneColor` node reads what was drawn under the sprite - on the
//! GPU, through the real batch and pipeline cache.
//!
//! Heat haze, refraction and water that bends what is under it all need the
//! scene behind a sprite (Unity's Scene Color, Godot's screen texture). No
//! graph could read it until Sep 2026; the showcase's Shader Gallery listed
//! it as the one thing it could not do. A runtime draws such sprites after the
//! world, over a copy of it (`WgpuSpriteBatch::capture_scene`); this test does
//! the same by hand: the left half red, the right half blue, then a sprite in
//! the red half whose graph samples half a screen to its right - it must come
//! out blue. Without a GPU adapter it prints why and passes.
#![cfg(feature = "gpu-wgpu")]

use glam::{Vec2, Vec4};
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::graph_pipeline_cache::{GraphPipelineCache, GraphPipelineKey};
use jengine_core::renderer::material::BlendMode;
use jengine_core::renderer::shader_graph::{NodeType, ShaderGraph};

const W: u32 = 128;
const H: u32 = 64;

fn scene_sampler(offset: [f32; 2]) -> ShaderGraph {
    let mut g = ShaderGraph::new("scene");
    let off = g.add_node(NodeType::Vec2Constant { value: offset }, [0.0, 0.0]);
    let scene = g.add_node(NodeType::SceneColor, [0.0, 0.0]);
    g.connect(off, 0, scene, 0);
    let out = g.output_node().unwrap().id;
    g.connect(scene, 0, out, 0);
    g
}

#[test]
fn a_scene_color_node_samples_the_copy_of_what_is_behind() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("scene_color_reads_what_is_behind: no GPU adapter - skipped");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white", &gpu.device, &gpu.queue).expect("upload");
    let white = assets.get_wgpu_texture("white").unwrap();

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut cache = GraphPipelineCache::new();
    let key = GraphPipelineKey { shader_graph_path: "mem/scene.shader".into(), blend_mode: BlendMode::Alpha };
    let pipeline = cache
        .insert_from_graph(&gpu.device, gpu.color_format, &batch, key.clone(), &scene_sampler([0.5, 0.0]))
        .expect("a graph with a SceneColor node builds");
    assert!(cache.reads_scene(&key.shader_graph_path), "the cache records that this graph reads the scene");

    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    batch.begin_frame();
    {
        // The world: red on the left, blue on the right.
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        let half = Vec2::new(W as f32 / 2.0, H as f32);
        batch.draw_sprite(Vec2::new(-(W as f32) / 4.0, 0.0), half, 0.0, Vec4::new(1.0, 0.0, 0.0, 1.0));
        batch.draw_sprite(Vec2::new(W as f32 / 4.0, 0.0), half, 0.0, Vec4::new(0.0, 0.0, 1.0, 1.0));
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, white);
    }
    // Between passes: copy what the world drew.
    batch.capture_scene(&gpu.device, &mut encoder, gpu.texture(), (W, H), gpu.color_format);
    // A graph that does NOT read the scene, built without group 1: after a
    // capture the batch binds the copy as group 1 for every graph flush, and
    // that has to be harmless for a pipeline that does not use it.
    let mut plain = ShaderGraph::new("plain");
    let uv = plain.add_node(NodeType::UvInput, [0.0, 0.0]);
    let tex = plain.add_node(NodeType::TextureSample, [0.0, 0.0]);
    plain.connect(uv, 0, tex, 0);
    let out = plain.output_node().unwrap().id;
    plain.connect(tex, 0, out, 0);
    let plain_pipeline = cache
        .insert_from_graph(
            &gpu.device,
            gpu.color_format,
            &batch,
            GraphPipelineKey { shader_graph_path: "mem/plain.shader".into(), blend_mode: BlendMode::Alpha },
            &plain,
        )
        .unwrap();
    gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    {
        // A 16 x 16 sprite in the red half, sampling half a screen right.
        let mut pass = gpu.color_pass_load(&mut encoder);
        batch.draw_sprite(Vec2::new(-(W as f32) / 4.0, 0.0), Vec2::splat(16.0), 0.0, Vec4::ONE);
        batch.flush_with_pipeline(&gpu.device, &gpu.queue, &mut pass, &vp, white, &pipeline);
        // And an ordinary graph sprite in the far corner, after the capture.
        batch.draw_sprite(Vec2::new(W as f32 / 2.0 - 4.0, H as f32 / 2.0 - 4.0), Vec2::splat(4.0), 0.0, Vec4::ONE);
        batch.flush_with_pipeline(&gpu.device, &gpu.queue, &mut pass, &vp, white, &plain_pipeline);
    }
    gpu.submit(encoder);
    let error = pollster::block_on(gpu.device.pop_error_scope());
    assert!(error.is_none(), "binding the scene copy for a graph that does not read it is a validation error: {error:?}");

    let img = gpu.read_rgba();
    let px = |x: u32, y: u32| {
        let i = ((y * W + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    let (under, beside) = (px(W / 4, H / 2), px(W / 4 - 12, H / 2));
    assert!(beside[0] > 200 && beside[2] < 30, "the red half around the sprite is still red: {beside:?}");
    assert!(
        under[2] > 200 && under[0] < 30,
        "the sprite should show the blue half a screen to its right, got {under:?}"
    );
}

#[test]
fn only_a_graph_that_reads_the_scene_gets_the_binding() {
    let with = jengine_core::renderer::shader_codegen::generate(&scene_sampler([0.0, 0.0])).unwrap().wgsl;
    assert!(with.contains("@group(1) @binding(0) var u_scene"));
    let mut plain = ShaderGraph::new("plain");
    let uv = plain.add_node(NodeType::UvInput, [0.0, 0.0]);
    let tex = plain.add_node(NodeType::TextureSample, [0.0, 0.0]);
    plain.connect(uv, 0, tex, 0);
    let out = plain.output_node().unwrap().id;
    plain.connect(tex, 0, out, 0);
    let without = jengine_core::renderer::shader_codegen::generate(&plain).unwrap().wgsl;
    assert!(!without.contains("u_scene"), "a graph that does not read the scene has no second group");
}

/// What a `SceneColor` node returns is opaque. The scene target's alpha is
/// not a coverage anyone meant: in the float target additive sprites add to
/// it past 1, and an alpha-blended sprite carrying that alpha then has
/// `1 - a < 0` and *subtracts* what is under it - the showcase's heat haze
/// punched a black hole in the fire it sat over. Here the other end of the
/// same mistake, which an 8-bit target can show: the right half is cleared
/// with alpha 0, and reading it must still draw it.
#[test]
fn a_scene_color_node_is_opaque_whatever_the_target_alpha() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("scene_color_reads_what_is_behind: no GPU adapter - skipped");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white", &gpu.device, &gpu.queue).expect("upload");
    let white = assets.get_wgpu_texture("white").unwrap();

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut cache = GraphPipelineCache::new();
    let key = GraphPipelineKey { shader_graph_path: "mem/scene.shader".into(), blend_mode: BlendMode::Alpha };
    let pipeline = cache.insert_from_graph(&gpu.device, gpu.color_format, &batch, key, &scene_sampler([0.5, 0.0])).unwrap();

    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    batch.begin_frame();
    {
        // Blue everywhere with alpha 0, then an opaque red left half.
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color { r: 0.0, g: 0.0, b: 1.0, a: 0.0 });
        batch.draw_sprite(Vec2::new(-(W as f32) / 4.0, 0.0), Vec2::new(W as f32 / 2.0, H as f32), 0.0, Vec4::new(1.0, 0.0, 0.0, 1.0));
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, white);
    }
    batch.capture_scene(&gpu.device, &mut encoder, gpu.texture(), (W, H), gpu.color_format);
    {
        let mut pass = gpu.color_pass_load(&mut encoder);
        batch.draw_sprite(Vec2::new(-(W as f32) / 4.0, 0.0), Vec2::splat(16.0), 0.0, Vec4::ONE);
        batch.flush_with_pipeline(&gpu.device, &gpu.queue, &mut pass, &vp, white, &pipeline);
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let i = (((H / 2) * W + W / 4) * 4) as usize;
    let under = [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]];
    assert!(
        under[2] > 200 && under[0] < 30,
        "the transparent-cleared blue read through SceneColor should draw as blue, got {under:?} - the node passed the target's alpha on"
    );
}

/// In a lit frame the world is drawn twice - the scene buffer the lights
/// multiply, then the emission buffer - and an `Unlit` sprite is drawn into
/// the first as a black **cutout**. A scene-reading sprite is drawn after the
/// composite, over a copy of the lit image; if it left a cutout here, the
/// copy would hold a black box and the sprite would faithfully draw it. That
/// is what the showcase's Spellcraft shockwaves were, every one of them. Here
/// the lit scene pass runs with a flush that draws whatever it is handed:
/// the background must come through untouched where the sprite stands.
#[test]
fn a_lit_scene_pass_leaves_a_scene_reader_out_even_as_a_cutout() {
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::ecs::components::{MaterialComponent, SpriteComponent, Transform, Unlit};
    use jengine_core::ecs::wgpu_systems::{WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::font_asset::FrameFonts;

    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("scene_color_reads_what_is_behind: no GPU adapter - skipped");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white", &gpu.device, &gpu.queue).expect("upload");

    // The material and its graph, as the asset manager and the pipeline
    // cache record them on load and on compile.
    let batch_for_cache = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut cache = GraphPipelineCache::new();
    let key = GraphPipelineKey { shader_graph_path: "mem/lit_haze.shader".into(), blend_mode: BlendMode::Alpha };
    cache.insert_from_graph(&gpu.device, gpu.color_format, &batch_for_cache, key, &scene_sampler([0.0, 0.0])).unwrap();
    jengine_core::renderer::material_params::note_material("materials/lit_haze", "mem/lit_haze.shader");
    assert!(jengine_core::renderer::material_params::reads_scene("materials/lit_haze"));

    let mut world = hecs::World::new();
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "white".into(), size: Some([W as f32, H as f32]), color: [1.0, 0.0, 0.0, 1.0], ..Default::default() },
    ));
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "white".into(), size: Some([32.0, 32.0]), z_order: 5, ..Default::default() },
        MaterialComponent { material_id: "materials/lit_haze".into(), ..Default::default() },
        Unlit::default(),
    ));

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let (_, _, _, font) = jengine_core::renderer::text::built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::Scene,
            |b, tex, _| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let i = (((H / 2) * W + W / 2) * 4) as usize;
    let mid = [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]];
    assert!(mid[0] > 200, "the scene-reading sprite left a cutout in the lit scene pass: {mid:?}");
}
