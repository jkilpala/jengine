//! A graph-material sprite drawn as a **cutout** covers only where its graph
//! is opaque.
//!
//! A lit frame draws the world twice (`WorldLayer`): in the emission layer a
//! lit sprite is drawn black to hide unlit things behind it, and in the scene
//! layer an unlit one is. Those cutouts went through the default sprite
//! pipeline, which knows nothing of the graph - so a graph whose alpha *is* its
//! shape (fog, a dissolve, a soft decal) cut out its whole quad. The
//! showcase's Day & Night laid lit fog over the town, and it blacked out the
//! pond and every lamp glow under it. A cutout now keeps its material and the
//! graph draws it: black, at its own alpha (`WgpuSpriteBatch::set_cutout`).
//!
//! Here: an unlit red quad, and over it a lit sprite whose graph is white with
//! alpha `uv.x`, drawn in the emission layer. The red must survive on the left
//! (alpha 0) and be cut out on the right (alpha 1). Without a GPU adapter it
//! prints why and passes.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{MaterialComponent, SpriteComponent, Transform, Unlit};
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::graph_pipeline_cache::{GraphPipelineCache, GraphPipelineKey};
use jengine_core::renderer::material::BlendMode;
use jengine_core::renderer::shader_graph::{NodeType, ShaderGraph};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 128;
const H: u32 = 32;

/// White, with alpha `uv.x`.
fn fade_graph() -> ShaderGraph {
    let mut g = ShaderGraph::new("fade");
    let uv = g.add_node(NodeType::UvInput, [0.0, 0.0]);
    let split = g.add_node(NodeType::Split, [0.0, 0.0]);
    g.connect(uv, 0, split, 0);
    let one = g.add_node(NodeType::FloatConstant { value: 1.0 }, [0.0, 0.0]);
    let rgba = g.add_node(NodeType::Combine { components: 4 }, [0.0, 0.0]);
    g.connect(one, 0, rgba, 0);
    g.connect(one, 0, rgba, 1);
    g.connect(one, 0, rgba, 2);
    g.connect(split, 0, rgba, 3);
    let out = g.output_node().unwrap().id;
    g.connect(rgba, 0, out, 0);
    g
}

#[test]
fn a_graph_cutout_covers_only_its_opaque_part() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("graph_cutouts_keep_their_alpha: no GPU adapter - skipped");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let mut cache = GraphPipelineCache::new();
    let key = GraphPipelineKey { shader_graph_path: "mem/fade.shader".into(), blend_mode: BlendMode::Alpha };
    let pipeline = cache.insert_from_graph(&gpu.device, gpu.color_format, &batch, key, &fade_graph()).unwrap();

    let quad = |color: [f32; 4], z: i32| SpriteComponent {
        texture_id: Default::default(),
        color,
        size: Some([W as f32, H as f32]),
        z_order: z,
        ..Default::default()
    };
    let mut world = World::new();
    world.spawn((Transform::default(), quad([1.0, 0.0, 0.0, 1.0], 0), Unlit {}));
    world.spawn((
        Transform::default(),
        quad([1.0; 4], 1),
        MaterialComponent { material_id: "materials/fade".into(), ..Default::default() },
    ));

    let (_, _, _, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::Emission,
            |b, tex, kind| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                match kind {
                    SpriteShadowFlush::Sprite { material_id: Some("materials/fade") } => {
                        b.flush_with_pipeline(&gpu.device, &gpu.queue, &mut pass, &vp, t, &pipeline)
                    }
                    _ => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                };
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let red = |x: u32| img.rgba[(((H / 2) * W + x) * 4) as usize];
    assert!(red(2) > 230, "where the graph is clear, the unlit red shows: {}", red(2));
    // uv.x is 0.98 three pixels from the right: 2% of the red (sRGB ~38).
    assert!(red(W - 3) < 50, "where it is opaque, it is cut out: {}", red(W - 3));
    let mid = red(W / 2);
    assert!(mid > 120 && mid < 220, "half way, half cut out: {mid}");
}
