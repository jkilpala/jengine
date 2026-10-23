//! **A graph moves the vertices it draws** (Oct 2026; Godot's `VERTEX`,
//! Unity Shader Graph's vertex Position): a `VertexOffset` node's input is
//! added to every vertex before projection. Shader graphs used to be colour
//! only, so grass, banners and anything that sways had to be animated by a
//! script moving whole sprites, or as a sheet of frames.
//!
//! Here a 32 x 32 white sprite in a 64 x 32 view, one world unit a pixel,
//! is drawn through a graph whose offset is `(16, 0) * (1 - uv.y)`: its top
//! edge moves 16 to the right and its foot stays - the lean of a blade of
//! grass. Without a GPU adapter it prints why and passes. The codegen is
//! unit-tested in `renderer::shader_codegen`.
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

const W: u32 = 64;
const H: u32 = 32;

/// White, its top leaning `lean` to the right.
fn lean_graph(lean: f32) -> ShaderGraph {
    let mut g = ShaderGraph::new("lean");
    let white = g.add_node(NodeType::ColorInput { color: [1.0; 4] }, [0.0, 0.0]);
    let out = g.output_node().unwrap().id;
    g.connect(white, 0, out, 0);
    let uv = g.add_node(NodeType::UvInput, [0.0, 0.0]);
    let split = g.add_node(NodeType::Split, [0.0, 0.0]);
    g.connect(uv, 0, split, 0);
    let one = g.add_node(NodeType::FloatConstant { value: 1.0 }, [0.0, 0.0]);
    let up = g.add_node(NodeType::Subtract, [0.0, 0.0]);
    g.connect(one, 0, up, 0);
    g.connect(split, 1, up, 1);
    let dir = g.add_node(NodeType::Vec2Constant { value: [lean, 0.0] }, [0.0, 0.0]);
    let off = g.add_node(NodeType::Multiply, [0.0, 0.0]);
    g.connect(dir, 0, off, 0);
    g.connect(up, 0, off, 1);
    let vo = g.add_node(NodeType::VertexOffset, [0.0, 0.0]);
    g.connect(off, 0, vo, 0);
    g
}

/// The sprite drawn through `graph`, as white-or-not per pixel.
fn draw(gpu: &HeadlessGpu, graph: &ShaderGraph) -> Vec<bool> {
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("art/white.png".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("art/white.png", &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let mut cache = GraphPipelineCache::new();
    let key = GraphPipelineKey { shader_graph_path: "mem/lean.shader".into(), blend_mode: BlendMode::Alpha };
    let pipeline = cache.insert_from_graph(&gpu.device, gpu.color_format, &batch, key, graph).unwrap();
    let mut world = World::new();
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "art/white.png".into(), size: Some([32.0, 32.0]), ..Default::default() },
        MaterialComponent { material_id: "materials/lean".into(), ..Default::default() },
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
    img.rgba.chunks(4).map(|p| p[0] > 128).collect()
}

/// The first and last white column of row `y`.
fn span(px: &[bool], y: u32) -> Option<(u32, u32)> {
    let row = &px[(y * W) as usize..((y + 1) * W) as usize];
    let first = row.iter().position(|&w| w)? as u32;
    let last = row.iter().rposition(|&w| w)? as u32;
    Some((first, last))
}

#[test]
fn a_graph_leans_a_sprite_by_moving_its_top() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("graphs_move_their_vertices: no GPU adapter - skipped");
        return;
    };
    let still = draw(&gpu, &lean_graph(0.0));
    assert_eq!(span(&still, 1), Some((16, 47)), "unmoved, the sprite covers x 16..48");
    assert_eq!(span(&still, 30), Some((16, 47)));
    let leaning = draw(&gpu, &lean_graph(16.0));
    let (top, foot) = (span(&leaning, 0).unwrap(), span(&leaning, 31).unwrap());
    assert!((top.0 as i32 - 32).abs() <= 1, "the top moved 16 to the right: {top:?}");
    assert!((foot.0 as i32 - 16).abs() <= 1, "the foot stayed: {foot:?}");
    let middle = span(&leaning, 16).unwrap();
    assert!((middle.0 as i32 - 24).abs() <= 1, "half way up, half the lean: {middle:?}");
}
