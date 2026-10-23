//! **Liquids** (`ecs::fluid`, Oct 2026): a `fluid` pours particles that act
//! as one liquid - they fall, pool in a box made of ordinary colliders, keep
//! their distance from each other rather than collapsing into a point, cool,
//! and set solid below `freeze_below`. And a `canvas_group` with a
//! `threshold` draws the soft dots of such a liquid as one hard-edged
//! surface (the default sprite shader's `params[1]`).

mod support;

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::behaviors::tick_motion;
use jengine_core::ecs::components::{Collider2D, ColliderShape, Transform};
use jengine_core::ecs::fluid::{Fluid, set_fluid_field, tick_fluids};

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

fn wall(world: &mut World, x: f32, y: f32, w: f32, h: f32) {
    world.spawn((
        at(x, y),
        Collider2D { shape: ColliderShape::Box { width: w, height: h }, ..Default::default() },
    ));
}

/// A box 200 wide with walls 20 thick, its floor's top at y = 0, and a fluid
/// pouring into it from above.
fn tank(rate: f32) -> (World, hecs::Entity) {
    let mut world = World::new();
    wall(&mut world, 0.0, -10.0, 240.0, 20.0);
    wall(&mut world, -110.0, 100.0, 20.0, 220.0);
    wall(&mut world, 110.0, 100.0, 20.0, 220.0);
    let f = world.spawn((
        at(0.0, 160.0),
        Fluid { pour_rate: rate, max_particles: 300, pour_speed: 80.0, pour_width: 30.0, ..Default::default() },
    ));
    (world, f)
}

fn run(world: &mut World, seconds: f32) {
    let steps = (seconds * 60.0) as usize;
    for _ in 0..steps {
        tick_fluids(world, 1.0 / 60.0);
    }
}

#[test]
fn a_pour_fills_a_box_and_settles_in_it() {
    let (mut world, f) = tank(200.0);
    run(&mut world, 2.0);
    world.get::<&mut Fluid>(f).unwrap().pour_rate = 0.0;
    run(&mut world, 4.0);
    let fluid = world.get::<&Fluid>(f).unwrap();
    assert_eq!(fluid.particles.len(), 300, "the pour stops at max_particles");
    for p in &fluid.particles {
        assert!(
            p.position.x > -100.0 && p.position.x < 100.0 && p.position.y > -1.0 && p.position.y < 200.0,
            "every particle is in the box: {:?}",
            p.position
        );
    }
    let speed = fluid.particles.iter().map(|p| p.velocity.length()).sum::<f32>() / 300.0;
    assert!(speed < 25.0, "and it has settled: mean speed {speed}");
    // A liquid spreads to a level: the pool is about as wide as the box.
    let (lo, hi) = fluid.particles.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.position.x), hi.max(p.position.x)));
    assert!(hi - lo > 160.0, "spread across the box floor: {lo}..{hi}");
}

#[test]
fn particles_keep_their_distance() {
    let (mut world, f) = tank(200.0);
    run(&mut world, 2.0);
    world.get::<&mut Fluid>(f).unwrap().pour_rate = 0.0;
    run(&mut world, 3.0);
    let fluid = world.get::<&Fluid>(f).unwrap();
    let mut closest = f32::MAX;
    let mut crowded = 0;
    for (i, a) in fluid.particles.iter().enumerate() {
        for b in &fluid.particles[i + 1..] {
            let d = a.position.distance(b.position);
            closest = closest.min(d);
            if d < fluid.radius * 0.15 {
                crowded += 1;
            }
        }
    }
    assert!(crowded < 6, "a liquid does not collapse into clumps: {crowded} pairs nearly on top of each other (closest {closest})");
}

#[test]
fn it_cools_and_sets_where_it_lies() {
    let (mut world, f) = tank(300.0);
    {
        let mut fl = world.get::<&mut Fluid>(f).unwrap();
        fl.cooling = 0.5;
        fl.freeze_below = 0.25;
    }
    run(&mut world, 1.0);
    world.get::<&mut Fluid>(f).unwrap().pour_rate = 0.0;
    run(&mut world, 2.0);
    let before: Vec<Vec2> = world.get::<&Fluid>(f).unwrap().particles.iter().map(|p| p.position).collect();
    assert!(world.get::<&Fluid>(f).unwrap().particles.iter().all(|p| p.frozen), "cold, every particle has set");
    run(&mut world, 1.0);
    let fluid = world.get::<&Fluid>(f).unwrap();
    for (p, b) in fluid.particles.iter().zip(&before) {
        assert_eq!(p.position, *b, "a set particle does not move");
    }
    // Its colour follows its heat.
    let mut g = Fluid { gradient: vec![[0.0, 0.1, 0.1, 0.1, 1.0], [1.0, 1.0, 0.8, 0.2, 1.0]], ..Default::default() };
    assert_eq!(g.color_at(1.0), [1.0, 0.8, 0.2, 1.0]);
    assert_eq!(g.color_at(0.0), [0.1, 0.1, 0.1, 1.0]);
    let mid = g.color_at(0.5);
    assert!((mid[0] - 0.55).abs() < 1e-4);
    g.gradient.clear();
    assert_eq!(g.color_at(0.3), [1.0; 4]);
}

#[test]
fn a_script_pours_and_empties_it_and_the_scene_keeps_it() {
    let (mut world, f) = tank(0.0);
    set_fluid_field(&world, f, "pour_rate", &serde_json::json!(120.0)).unwrap();
    set_fluid_field(&world, f, "max_particles", &serde_json::json!(40.0)).unwrap();
    assert!(set_fluid_field(&world, f, "wetness", &serde_json::json!(1.0)).is_err());
    run(&mut world, 1.0);
    assert_eq!(world.get::<&Fluid>(f).unwrap().particles.len(), 40, "a count from a script's float");
    set_fluid_field(&world, f, "viscosity", &serde_json::json!(9.0)).unwrap();
    assert_eq!(world.get::<&Fluid>(f).unwrap().particles.len(), 40, "a field set keeps the liquid");
    set_fluid_field(&world, f, "clear", &serde_json::Value::Null).unwrap();
    assert!(world.get::<&Fluid>(f).unwrap().particles.is_empty(), "clear empties it");
    // The shared command every browser host runs.
    use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
    struct Host(World);
    impl ScriptWorldHost for Host {
        fn world(&mut self) -> &mut World {
            &mut self.0
        }
        fn resolve_entity(&mut self, id: u32) -> Option<hecs::Entity> {
            self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
        }
    }
    let mut host = Host(world);
    let args = JsonArgs(serde_json::json!({ "e": f.id(), "f": "pour_rate", "v": 55.0 }));
    assert!(handle_world_command(&mut host, "sffl", &args));
    assert_eq!(host.0.get::<&Fluid>(f).unwrap().pour_rate, 55.0);
    // Saved without its particles; loaded, it pours again.
    let scene = jengine_core::scene::format::save_scene(&host.0);
    let saved = scene.entities.iter().find_map(|e| e.components.fluid.as_ref()).expect("saved");
    assert_eq!(saved.pour_rate, 55.0);
    assert!(saved.particles.is_empty());
}

#[test]
fn every_host_runs_and_draws_it() {
    let motion = include_str!("../src/ecs/behaviors.rs");
    assert!(motion.contains("crate::ecs::fluid::tick_fluids(world, dt)"), "tick_motion runs the fluids");
    let draw = include_str!("../src/ecs/wgpu_systems.rs");
    assert!(draw.contains("crate::ecs::fluid::quads(fluid)"), "the sprite pass draws them");
    let lights = include_str!("../src/renderer/lighting.rs");
    assert!(lights.contains("crate::ecs::fluid::Fluid"), "gather_lights adds their light");
    // tick_motion is what every host calls; a fluid moves through it.
    let (mut world, f) = tank(120.0);
    tick_motion(&mut world, 1.0 / 30.0, None, false);
    assert!(!world.get::<&Fluid>(f).unwrap().particles.is_empty());
}

/// A thresholded canvas group: two soft dots drawn into the group, cut at
/// half alpha. Without the threshold a dot's rim is faint; with it the rim is
/// gone and the middle is full colour - the dots read as one hard-edged
/// shape, which is what a liquid of particles needs.
#[cfg(feature = "gpu-wgpu")]
#[test]
fn a_thresholded_group_is_a_hard_edged_surface() {
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::ecs::canvas_group::{CanvasGroup, canvas_group_views, clear_members};
    use jengine_core::ecs::components::{Parent, SpriteComponent};
    use jengine_core::ecs::wgpu_systems::{
        SpriteShadowFlush, VIEW_TARGET_FORMAT, WorldLayer, render_sprites_with_shadows_wgpu_materialized,
    };
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 64;
    let Some(gpu) = HeadlessGpu::new(W, W) else {
        eprintln!("fluids_pour_and_settle: no GPU adapter - skipped");
        return;
    };
    // A soft dot: alpha falling from 1 at the middle to 0 at the rim.
    let n = 32u32;
    let mut rgba = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let d = ((x as f32 + 0.5 - 16.0).hypot(y as f32 + 0.5 - 16.0) / 16.0).min(1.0);
            rgba.extend_from_slice(&[255, 255, 255, ((1.0 - d) * 255.0) as u8]);
        }
    }
    let (_r, _w, _h, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let render = |threshold: f32| -> Vec<u8> {
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes("dot".into(), RawTextureData { width: n, height: n, rgba: rgba.clone() });
        assets.ensure_wgpu_texture("dot", &gpu.device, &gpu.queue).unwrap();
        let mut world = World::new();
        let dot = |x: f32| (at(x, 0.0), SpriteComponent { texture_id: "dot".into(), color: [1.0, 0.0, 0.0, 1.0], size: Some([32.0, 32.0]), ..Default::default() });
        let root = world.spawn(dot(-8.0));
        world.insert_one(root, CanvasGroup { threshold, ..Default::default() }).unwrap();
        let other = world.spawn(dot(0.0));
        world.insert_one(other, Parent(root)).unwrap();
        world.get::<&mut Transform>(other).unwrap().position = Vec2::new(16.0, 0.0);
        jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
        let main = Camera2D::new(W as f32, W as f32);
        let views = canvas_group_views(&world, &main, (W, W));
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        batch.begin_frame();
        let mut encoder = gpu.encoder();
        for v in &views {
            assets.ensure_render_texture(&v.target, W, W, VIEW_TARGET_FORMAT, &gpu.device);
            batch.set_target_format(&gpu.device, VIEW_TARGET_FORMAT);
            set_frame_view(&v.camera);
            let vp = v.camera.view_projection();
            let target_view = assets.get_wgpu_texture(&v.target).unwrap().view.clone();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("group"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let assets = &assets;
            render_sprites_with_shadows_wgpu_materialized(
                &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                |b, tex, kind| {
                    if let SpriteShadowFlush::Sprite { .. } = kind {
                        b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(tex).unwrap());
                    }
                },
            );
        }
        batch.set_target_format(&gpu.device, gpu.color_format);
        set_frame_view(&main);
        let vp = main.view_projection();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            let assets = &assets;
            render_sprites_with_shadows_wgpu_materialized(
                &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                |b, tex, kind| {
                    if let SpriteShadowFlush::Sprite { .. } = kind {
                        b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(tex).unwrap());
                    }
                },
            );
        }
        gpu.submit(encoder);
        clear_frame_view();
        clear_members();
        let img = gpu.read_rgba();
        (0..W).map(|x| img.rgba[((32 * W + x) * 4) as usize]).collect()
    };
    let soft = render(0.0);
    let hard = render(0.5);
    // World x = -22 is near the left dot's rim: faint as it is, gone when cut.
    let rim = 32 - 22;
    assert!(soft[rim] > 10 && soft[rim] < 120, "a soft rim without the threshold: {}", soft[rim]);
    assert!(hard[rim] < 6, "cut away with it: {}", hard[rim]);
    // Between the dots (world 0): soft, partly see-through; cut, full red.
    assert!(hard[32] > 240, "the merged middle is the full colour: {}", hard[32]);
    assert!(hard[32] > soft[32], "solid where the soft picture was not: {} vs {}", hard[32], soft[32]);
}
