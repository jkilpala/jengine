//! **Flow along a path** (Oct 2026; `ecs::flow`): water running along a
//! drawn curve, carrying what floats in it and drawn as a ribbon whose
//! texture runs downstream. Two halves: a leaf dropped into a bending
//! channel is carried round the bend by the real motion step
//! (`tick_motion`), and stays in the water; and the ribbon, rendered through
//! the real sprite pass on a headless device, covers the channel's width
//! with its texture running along the curve - the stripes have moved
//! downstream once the water has run on. Found by the showcase's Millrace.
//! The pixel half passes with a note without a GPU adapter.
#![cfg(feature = "flow")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Name, PathCommand, Tags, Transform, VectorPath, Velocity2D};
use jengine_core::ecs::flow::FlowPath;

fn channel(world: &mut World, commands: Vec<PathCommand>) {
    world.spawn((Transform::default(), Name("Race".into()), VectorPath { commands, ..Default::default() }));
}

#[test]
fn a_leaf_is_carried_round_the_bend() {
    let mut world = World::new();
    // East, a rounded bend, then north - a race has bends, not corners: at
    // a square corner a leaf's lag past the turn carries it over the bank,
    // as it would a real one.
    channel(&mut world, vec![
        PathCommand::MoveTo { x: 0.0, y: 0.0 },
        PathCommand::LineTo { x: 200.0, y: 0.0 },
        PathCommand::QuadTo { cx: 300.0, cy: 0.0, x: 300.0, y: 100.0 },
        PathCommand::LineTo { x: 300.0, y: 300.0 },
    ]);
    world.spawn((FlowPath { path: "Race".into(), width: 60.0, speed: 120.0, grip: 6.0, tag: "floats".into(), ..Default::default() },));
    let leaf = world.spawn((
        Transform { position: Vec2::new(20.0, 0.0), ..Default::default() },
        Velocity2D::default(),
        Tags(["floats".to_string()].into_iter().collect()),
    ));
    let mut furthest_out = 0.0f32;
    for _ in 0..(60 * 6) {
        jengine_core::ecs::behaviors::tick_motion(&mut world, 1.0 / 60.0, None, false);
        let p = world.get::<&Transform>(leaf).unwrap().position;
        // Distance from the channel's middle line, the bend's arc included.
        let off = if p.x <= 200.0 {
            p.y.abs()
        } else if p.y >= 100.0 {
            (p.x - 300.0).abs()
        } else {
            ((p - Vec2::new(200.0, 100.0)).length() - 100.0).abs()
        };
        furthest_out = furthest_out.max(off);
    }
    let p = world.get::<&Transform>(leaf).unwrap().position;
    assert!(p.y > 150.0 && (p.x - 300.0).abs() < 30.0, "carried east, round the bend and north: {p}");
    assert!(furthest_out < 30.0, "kept in the water all the way: {furthest_out}");
}

#[cfg(feature = "gpu-wgpu")]
mod pixels {
    use super::*;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 96;
    const H: u32 = 32;

    /// A straight channel across the view, 16 wide, its texture red in the
    /// top half and green in the bottom, repeating every 64 units, run on by
    /// `scroll`; the rows through the middle and 12 below it.
    fn render(scroll: f32) -> Option<(Vec<[u8; 3]>, Vec<[u8; 3]>)> {
        let gpu = HeadlessGpu::new(W, H).or_else(|| {
            eprintln!("water_flows_along_its_path: no GPU adapter - skipped");
            None
        })?;
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
        let mut rgba = Vec::new();
        for y in 0..8 {
            for _ in 0..4 {
                rgba.extend_from_slice(if y < 4 { &[255, 0, 0, 255] } else { &[0, 255, 0, 255] });
            }
        }
        assets.insert_texture_bytes("water.png".into(), RawTextureData { width: 4, height: 8, rgba });
        for id in [WHITE_TEXTURE_ID, "water.png"] {
            assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
        }
        let (rgba, aw, ah, font) = built_in_font_atlas();
        let mut fa = AssetManager::new();
        fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
        let fonts = FrameFonts::builtin_only(&font, "__font");
        let mut world = World::new();
        channel(&mut world, vec![PathCommand::MoveTo { x: -48.0, y: 0.0 }, PathCommand::LineTo { x: 48.0, y: 0.0 }]);
        world.spawn((FlowPath {
            path: "Race".into(),
            width: 16.0,
            tile: 64.0,
            texture: "water.png".into(),
            color: [1.0, 1.0, 1.0, 1.0],
            soft: 0.1,
            scroll,
            ..Default::default()
        },));
        let cam = Camera2D::new(W as f32, H as f32);
        set_frame_view(&cam);
        let vp = cam.view_projection();
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        let mut encoder = gpu.encoder();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            batch.begin_frame();
            render_sprites_with_shadows_wgpu_materialized(
                &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                |b, tex, kind| {
                    let t = assets.get_wgpu_texture(tex).expect("uploaded");
                    if let SpriteShadowFlush::Sprite { .. } = kind {
                        b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                    }
                },
            );
        }
        gpu.submit(encoder);
        clear_frame_view();
        let img = gpu.read_rgba();
        let row = |y: u32| (0..W).map(|x| {
            let i = ((y * W + x) * 4) as usize;
            [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
        }).collect::<Vec<_>>();
        Some((row(H / 2), row(H / 2 + 12)))
    }

    fn red(p: [u8; 3]) -> bool {
        p[0] > 200 && p[1] < 40
    }

    fn green(p: [u8; 3]) -> bool {
        p[1] > 200 && p[0] < 40
    }

    #[test]
    fn the_ribbon_covers_the_channel_and_its_stripes_run_downstream() {
        let Some((still, outside)) = render(0.0) else { return };
        // x = d - 48 on screen: d in [0, 32) is the first half of a repeat
        // (red), [32, 64) the second (green).
        assert!(red(still[8]) && green(still[56]) && red(still[88]), "stripes along the water: {:?} {:?} {:?}", still[8], still[56], still[88]);
        assert!(outside.iter().all(|p| p.iter().all(|&c| c < 8)), "nothing past the banks");
        let Some((run, _)) = render(16.0) else { return };
        // Run on 16: the stripes have moved 16 downstream (to +x).
        assert!(red(run[40]) && green(still[40]), "the red has moved on into what was green: {:?} {:?}", run[40], still[40]);
        assert!(green(run[72]), "and the green after it: {:?}", run[72]);
    }
}
