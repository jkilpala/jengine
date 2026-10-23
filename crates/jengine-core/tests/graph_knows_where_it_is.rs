//! A graph knows where it is drawn: `PositionInput` is the fragment's world
//! position, `ScreenPosition` its place on the screen, and `WorldToScreen`
//! takes any world point there - which is what a reflection is made of.
//!
//! `PositionInput` used to return the sprite's UV while its doc said world
//! position, so a graph could not place anything in the world, and there was
//! no screen position at all: `SceneColor` took an offset in screen space
//! from nowhere. Here a 64 x 32 view, one world unit a pixel, is covered by
//! a graph drawing `(Position.x / 64 + 0.5, Screen.y, WorldToScreen(Position).x)`.
//! Without a GPU adapter it prints why and passes.
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

fn srgb_to_linear(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn where_graph() -> ShaderGraph {
    let mut g = ShaderGraph::new("where");
    let pos = g.add_node(NodeType::PositionInput, [0.0, 0.0]);
    let scr = g.add_node(NodeType::ScreenPosition, [0.0, 0.0]);
    let w2s = g.add_node(NodeType::WorldToScreen, [0.0, 0.0]);
    g.connect(pos, 0, w2s, 0);
    // Position.x / 64 + 0.5
    let scale = g.add_node(NodeType::Vec2Constant { value: [1.0 / 64.0, 1.0 / 64.0] }, [0.0, 0.0]);
    let half = g.add_node(NodeType::Vec2Constant { value: [0.5, 0.5] }, [0.0, 0.0]);
    let mul = g.add_node(NodeType::Multiply, [0.0, 0.0]);
    g.connect(pos, 0, mul, 0);
    g.connect(scale, 0, mul, 1);
    let add = g.add_node(NodeType::Add, [0.0, 0.0]);
    g.connect(mul, 0, add, 0);
    g.connect(half, 0, add, 1);
    let p = g.add_node(NodeType::Split, [0.0, 0.0]);
    g.connect(add, 0, p, 0);
    let s = g.add_node(NodeType::Split, [0.0, 0.0]);
    g.connect(scr, 0, s, 0);
    let ws = g.add_node(NodeType::Split, [0.0, 0.0]);
    g.connect(w2s, 0, ws, 0);
    let one = g.add_node(NodeType::FloatConstant { value: 1.0 }, [0.0, 0.0]);
    let rgba = g.add_node(NodeType::Combine { components: 4 }, [0.0, 0.0]);
    g.connect(p, 0, rgba, 0);
    g.connect(s, 1, rgba, 1);
    g.connect(ws, 0, rgba, 2);
    g.connect(one, 0, rgba, 3);
    let out = g.output_node().unwrap().id;
    g.connect(rgba, 0, out, 0);
    g
}

#[test]
fn a_graph_knows_its_world_and_screen_position() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("graph_knows_where_it_is: no GPU adapter - skipped");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("art/white.png".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("art/white.png", &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let mut cache = GraphPipelineCache::new();
    let key = GraphPipelineKey { shader_graph_path: "mem/where.shader".into(), blend_mode: BlendMode::Alpha };
    let pipeline = cache.insert_from_graph(&gpu.device, gpu.color_format, &batch, key, &where_graph()).unwrap();

    let mut world = World::new();
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "art/white.png".into(), size: Some([W as f32, H as f32]), ..Default::default() },
        MaterialComponent { material_id: "materials/where".into(), ..Default::default() },
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
    for (px, py) in [(16u32, 8u32), (48, 24), (40, 4)] {
        let i = ((py * W + px) * 4) as usize;
        let c = [srgb_to_linear(img.rgba[i]), srgb_to_linear(img.rgba[i + 1]), srgb_to_linear(img.rgba[i + 2])];
        let u = (px as f32 + 0.5) / W as f32;
        let v = (py as f32 + 0.5) / H as f32;
        assert!((c[0] - u).abs() < 0.01, "world x at ({px}, {py}): {c:?}, wanted {u}");
        assert!((c[1] - v).abs() < 0.01, "screen y, down: {c:?}, wanted {v}");
        assert!((c[2] - u).abs() < 0.01, "the world point back on the screen: {c:?}, wanted {u}");
    }
}
