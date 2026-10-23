//! A Scene Color sprite sits at its z: what is below it is drawn first and
//! is what it reads, what is above it is drawn over it afterwards.
//!
//! It used to draw after the whole world, over every other sprite whatever
//! its z - the showcase's lake covered anything floating on it. A host now
//! draws the world up to the lowest reader's z (`ZWindow::UpTo`), copies it,
//! draws the readers, then draws the rest (`ZWindow::Above`); a lit frame
//! draws that rest into a transparent scene buffer and composites it over
//! the frame (`WgpuLightingPass::composite_over`). These tests run that
//! sequence by hand, unlit and lit: red everywhere at z 0, a reader at z 10
//! that shows the inverse of what is under it (red -> cyan), and a blue box
//! at z 20 over half the reader. Without a GPU adapter they print why and
//! pass.
//!
//! Since Oct 2026 a host cuts at **every** band of readers
//! (`SceneReaderPlan`), so a reader sees the readers below it: a second
//! inverter at z 30 over the first shows red again, where one copy taken
//! below both would have shown it cyan.
#![cfg(feature = "gpu-wgpu")]

mod support;

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::{AmbientLight, MaterialComponent, SpriteComponent, Transform};
use jengine_core::ecs::wgpu_systems::{
    SceneReaderPlan, SpriteShadowFlush, WorldLayer, ZWindow, render_sprites_with_shadows_wgpu_materialized,
    scene_reader_split, set_z_window,
};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuSpriteBatch};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::graph_pipeline_cache::{GraphPipelineCache, GraphPipelineKey};
use jengine_core::renderer::material::BlendMode;
use jengine_core::renderer::material_params::{note_material, reads_scene};
use jengine_core::renderer::shader_graph::{NodeType, ShaderGraph};

const W: u32 = 96;
const H: u32 = 48;
const MATERIAL: &str = "materials/test_invert_scene";
const SHADER: &str = "mem/test_invert_scene.shader";

/// `1 - SceneColor`: red under it draws cyan.
fn inverter() -> ShaderGraph {
    let mut g = ShaderGraph::new("invert");
    let zero = g.add_node(NodeType::Vec2Constant { value: [0.0, 0.0] }, [0.0, 0.0]);
    let scene = g.add_node(NodeType::SceneColor, [0.0, 0.0]);
    g.connect(zero, 0, scene, 0);
    let one = g.add_node(NodeType::Vec4Constant { value: [1.0, 1.0, 1.0, 2.0] }, [0.0, 0.0]);
    let inv = g.add_node(NodeType::Subtract, [0.0, 0.0]);
    g.connect(one, 0, inv, 0);
    g.connect(scene, 0, inv, 1);
    let out = g.output_node().unwrap().id;
    g.connect(inv, 0, out, 0);
    g
}

fn world() -> World {
    let mut world = World::new();
    let white = |size: [f32; 2], color: [f32; 4], z: i32| SpriteComponent {
        texture_id: "white".into(),
        size: Some(size),
        color,
        z_order: z,
        ..Default::default()
    };
    world.spawn((Transform::default(), white([W as f32, H as f32], [1.0, 0.0, 0.0, 1.0], 0)));
    // The reader: the middle 48 x 32.
    world.spawn((
        Transform::default(),
        white([48.0, 32.0], [1.0; 4], 10),
        MaterialComponent { material_id: MATERIAL.into(), ..Default::default() },
    ));
    // In front of it, over its right half.
    world.spawn((
        Transform { position: Vec2::new(12.0, 0.0), ..Default::default() },
        white([24.0, 32.0], [0.0, 0.0, 1.0, 1.0], 20),
    ));
    world
}

struct Rig {
    gpu: HeadlessGpu,
    assets: AssetManager,
    batch: WgpuSpriteBatch,
    pipeline: std::sync::Arc<wgpu::RenderPipeline>,
}

fn rig() -> Option<Rig> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("scene_readers_sort_with_the_world: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white", &gpu.device, &gpu.queue).expect("upload");
    let batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut cache = GraphPipelineCache::new();
    let key = GraphPipelineKey { shader_graph_path: SHADER.into(), blend_mode: BlendMode::Alpha };
    let pipeline = cache.insert_from_graph(&gpu.device, gpu.color_format, &batch, key, &inverter()).expect("builds");
    note_material(MATERIAL, SHADER);
    assert!(reads_scene(MATERIAL));
    Some(Rig { gpu, assets, batch, pipeline })
}

/// Draw one window of the world into `pass` as a host does: readers are
/// left for their own pass.
fn draw_world(
    gpu: &HeadlessGpu,
    assets: &AssetManager,
    batch: &mut WgpuSpriteBatch,
    world: &World,
    pass: &mut wgpu::RenderPass<'_>,
    layer: WorldLayer,
    vp: &glam::Mat4,
) {
    let (_, _, _, font) = jengine_core::renderer::text::built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    render_sprites_with_shadows_wgpu_materialized(
        world, &SortLayers::default(), batch, &fonts, Vec2::splat(64.0), layer,
        |b, tex, kind| {
            if let SpriteShadowFlush::Sprite { material_id: Some(m) } = kind
                && reads_scene(m)
            {
                b.discard_pending();
                return;
            }
            b.flush(&gpu.device, &gpu.queue, pass, vp, assets.get_wgpu_texture(tex).expect("uploaded"));
        },
    );
}

fn draw_readers(
    gpu: &HeadlessGpu,
    assets: &AssetManager,
    batch: &mut WgpuSpriteBatch,
    pipeline: &wgpu::RenderPipeline,
    world: &World,
    pass: &mut wgpu::RenderPass<'_>,
    vp: &glam::Mat4,
) {
    let (_, _, _, font) = jengine_core::renderer::text::built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    render_sprites_with_shadows_wgpu_materialized(
        world, &SortLayers::default(), batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
        |b, tex, kind| match kind {
            SpriteShadowFlush::Sprite { material_id: Some(m) } if reads_scene(m) => {
                b.flush_with_pipeline(&gpu.device, &gpu.queue, pass, vp, assets.get_wgpu_texture(tex).unwrap(), pipeline);
            }
            _ => b.discard_pending(),
        },
    );
}

/// (reader's left half, reader's right half under the box, outside both).
fn read(rig: &Rig) -> ([u8; 3], [u8; 3], [u8; 3]) {
    let img = rig.gpu.read_rgba();
    let px = |x: u32, y: u32| {
        let i = ((y * W + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    (px(W / 2 - 12, H / 2), px(W / 2 + 12, H / 2), px(4, 4))
}

fn check((reader, boxed, outside): ([u8; 3], [u8; 3], [u8; 3]), what: &str) {
    assert!(outside[0] > 200 && outside[2] < 30, "{what}: red where nothing is over it: {outside:?}");
    assert!(reader[0] < 30 && reader[1] > 200 && reader[2] > 200, "{what}: the reader shows cyan over red: {reader:?}");
    assert!(boxed[2] > 200 && boxed[0] < 30 && boxed[1] < 30, "{what}: the box at z 20 is over the reader: {boxed:?}");
}

#[test]
fn the_split_is_the_lowest_reader_when_something_stands_in_front() {
    note_material(MATERIAL, SHADER);
    // Without the pipeline cache having compiled the graph, nothing is
    // known to read the scene - the host's own condition.
    let mut w = world();
    let split = scene_reader_split(&w, &SortLayers::default());
    if split.is_none() {
        // Recorded by another test's compile in this process, or not yet:
        // record it the way a compile does, through a real cache, only when
        // a GPU exists - otherwise the rest of this check needs no GPU.
        let Some(_rig) = rig() else { return };
    }
    assert_eq!(scene_reader_split(&w, &SortLayers::default()), Some(10));
    // Nothing above the reader: one pass, as before.
    let boxed: Vec<hecs::Entity> = w
        .query::<&SpriteComponent>()
        .iter()
        .filter(|(_, s)| s.z_order == 20)
        .map(|(e, _)| e)
        .collect();
    for e in boxed {
        w.despawn(e).unwrap();
    }
    assert_eq!(scene_reader_split(&w, &SortLayers::default()), None);
}

#[test]
fn unlit_the_world_above_a_reader_draws_over_it() {
    let Some(mut rig) = rig() else { return };
    let world = world();
    let t = scene_reader_split(&world, &SortLayers::default()).expect("split");
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = rig.gpu.encoder();
    rig.batch.begin_frame();
    set_z_window(Some(ZWindow::UpTo(t)));
    {
        let Rig { gpu, assets, batch, .. } = &mut rig;
        let mut pass = color_pass(&mut encoder, gpu.view(), Some(wgpu::Color::BLACK));
        draw_world(gpu, assets, batch, &world, &mut pass, WorldLayer::All, &vp);
    }
    set_z_window(None);
    rig.batch.capture_scene(&rig.gpu.device, &mut encoder, rig.gpu.texture(), (W, H), rig.gpu.color_format);
    {
        let Rig { gpu, assets, batch, pipeline } = &mut rig;
        let mut pass = color_pass(&mut encoder, gpu.view(), None);
        draw_readers(gpu, assets, batch, pipeline, &world, &mut pass, &vp);
    }
    set_z_window(Some(ZWindow::Above(t)));
    {
        let Rig { gpu, assets, batch, .. } = &mut rig;
        let mut pass = color_pass(&mut encoder, gpu.view(), None);
        draw_world(gpu, assets, batch, &world, &mut pass, WorldLayer::All, &vp);
    }
    set_z_window(None);
    rig.gpu.submit(encoder);
    check(read(&rig), "unlit");
}

#[test]
fn lit_the_world_above_a_reader_is_composited_over_it() {
    let Some(mut rig) = rig() else { return };
    let mut world = world();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 1.0 },));
    let t = scene_reader_split(&world, &SortLayers::default()).expect("split");
    let lights = jengine_core::renderer::lighting::gather_lights(&world);
    let lp = WgpuLightingPass::new(&rig.gpu.device, W, H, rig.gpu.color_format).expect("lighting pass");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = rig.gpu.encoder();
    rig.batch.begin_frame();

    // The part up to the reader, lit and composited onto the frame.
    set_z_window(Some(ZWindow::UpTo(t)));
    {
        let Rig { gpu, assets, batch, .. } = &mut rig;
        let mut pass = lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        draw_world(gpu, assets, batch, &world, &mut pass, WorldLayer::Scene, &vp);
    }
    lp.set_hdr(&rig.gpu.queue, false);
    lp.render_lights(&rig.gpu.device, &rig.gpu.queue, &mut encoder, &camera, &lights);
    lp.composite(&mut encoder, rig.gpu.view());

    // The reader, over a copy of the lit frame.
    set_z_window(None);
    rig.batch.capture_scene(&rig.gpu.device, &mut encoder, rig.gpu.texture(), (W, H), rig.gpu.color_format);
    {
        let Rig { gpu, assets, batch, pipeline } = &mut rig;
        let mut pass = color_pass(&mut encoder, gpu.view(), None);
        draw_readers(gpu, assets, batch, pipeline, &world, &mut pass, &vp);
    }

    // The part above it: a transparent scene buffer, composited over.
    set_z_window(Some(ZWindow::Above(t)));
    {
        let Rig { gpu, assets, batch, .. } = &mut rig;
        let mut pass = lp.overlay_scene_pass(&mut encoder);
        draw_world(gpu, assets, batch, &world, &mut pass, WorldLayer::Scene, &vp);
    }
    lp.composite_over(&mut encoder, rig.gpu.view());
    set_z_window(None);
    rig.gpu.submit(encoder);
    check(read(&rig), "lit");
}

fn color_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &'a wgpu::TextureView,
    clear: Option<wgpu::Color>,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("scene_readers_sort_with_the_world"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    })
}

/// Every runtime draws the world in the two parts: a host that forgets
/// would draw readers over everything again, silently, in that runtime only.
#[test]
fn every_runtime_splits_its_world_around_the_readers() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../");
    for host in [
        "jengine-runtime/src/wgpu_scene_runner.rs",
        "jengine-web/src/lib.rs",
        "jengine-editor-viewport/src/lib.rs",
    ] {
        let src = support::with_frame_draw(&std::fs::read_to_string(format!("{root}{host}")).expect(host));
        let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        for call in [
            "SceneReaderPlan::of(",
            "plan.world_window(part)",
            "plan.readers_after(part)",
            "set_z_window(None)",
            "composite_over(",
            "overlay_scene_pass(",
        ] {
            assert!(code.contains(call), "{host} does not call `{call}` - it draws Scene Color sprites over everything");
        }
    }
}

/// The world with a second inverter at z 30, over the left of the first:
/// x 30..42 of the frame.
fn stacked() -> World {
    let mut world = world();
    world.spawn((
        Transform { position: Vec2::new(-12.0, 0.0), ..Default::default() },
        SpriteComponent { texture_id: "white".into(), size: Some([12.0, 32.0]), color: [1.0; 4], z_order: 30, ..Default::default() },
        MaterialComponent { material_id: MATERIAL.into(), ..Default::default() },
    ));
    world
}

#[test]
fn the_plan_cuts_at_every_band_of_readers() {
    let Some(_rig) = rig() else { return };
    let plan = SceneReaderPlan::of(&stacked(), &SortLayers::default());
    assert_eq!(plan.cuts(), &[10, 30]);
    assert_eq!(plan.parts(), 2, "nothing but a reader above z 30, so no third part");
    assert_eq!(plan.world_window(0), Some(ZWindow::UpTo(10)));
    assert_eq!(plan.world_window(1), Some(ZWindow::Between(10, 30)), "the box at z 20 is in the second part");
    assert_eq!(plan.readers_after(0), Some(ZWindow::Band(10)));
    assert_eq!(plan.readers_after(1), Some(ZWindow::Band(30)));
    let single = SceneReaderPlan::of(&world(), &SortLayers::default());
    assert_eq!((single.parts(), single.world_window(1)), (2, Some(ZWindow::Above(10))), "one band: the old split");
    let none = SceneReaderPlan::of(&World::new(), &SortLayers::default());
    assert_eq!((none.parts(), none.world_window(0), none.readers_after(0)), (1, None, None));
}

/// The plan, run as a host runs it, unlit.
#[test]
fn a_reader_sees_the_readers_below_it() {
    let Some(mut rig) = rig() else { return };
    let world = stacked();
    let plan = SceneReaderPlan::of(&world, &SortLayers::default());
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = rig.gpu.encoder();
    rig.batch.begin_frame();
    for part in 0..plan.parts() {
        set_z_window(plan.world_window(part));
        {
            let Rig { gpu, assets, batch, .. } = &mut rig;
            let clear = (part == 0).then_some(wgpu::Color::BLACK);
            let mut pass = color_pass(&mut encoder, gpu.view(), clear);
            draw_world(gpu, assets, batch, &world, &mut pass, WorldLayer::All, &vp);
        }
        if let Some(band) = plan.readers_after(part) {
            set_z_window(Some(band));
            rig.batch.capture_scene(&rig.gpu.device, &mut encoder, rig.gpu.texture(), (W, H), rig.gpu.color_format);
            let Rig { gpu, assets, batch, pipeline } = &mut rig;
            let mut pass = color_pass(&mut encoder, gpu.view(), None);
            draw_readers(gpu, assets, batch, pipeline, &world, &mut pass, &vp);
        }
    }
    set_z_window(None);
    rig.gpu.submit(encoder);
    let img = rig.gpu.read_rgba();
    let px = |x: u32| {
        let i = (((H / 2) * W + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    let (lower, both, boxed) = (px(27), px(36), px(60));
    assert!(lower[0] < 30 && lower[1] > 200 && lower[2] > 200, "the first reader alone: cyan over red {lower:?}");
    assert!(both[0] > 200 && both[1] < 30 && both[2] < 30, "the second inverts the first's cyan back to red: {both:?}");
    assert!(boxed[2] > 200 && boxed[0] < 30, "the box between them is over the first: {boxed:?}");
}
