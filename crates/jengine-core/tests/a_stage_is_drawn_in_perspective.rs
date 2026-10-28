//! **A 3D stage** (Oct 2026; `ecs::stage`): a perspective camera over a
//! diorama of flat pieces - the HD-2D look. Quads are projected into their
//! meshes and billboards placed by the motion step, everything upright
//! sorts by how far into the scene it stands with floors under all of it,
//! scripts reach it through the shared commands (`stgp`, `stgs`), and the
//! velocity pass hands `depth_of_field` each floor vertex's own depth - so a
//! floor is sharp at the target and softens into the distance, which a
//! mesh with one z (or no place in the velocity pass at all, as before)
//! could not do. Found by Visual Showcase 2.
#![cfg(feature = "stage")]

use glam::{Vec2, Vec3};
use hecs::{Entity, World};
use jengine_core::ecs::components::{Mesh2D, Name, PointLight2D, SpriteComponent, Transform};
use jengine_core::ecs::stage::{Stage, StageBillboard, StageQuad};
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};

struct Host(World);
impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

fn mesh(w: &World, e: Entity) -> Mesh2D {
    (*w.get::<&Mesh2D>(e).unwrap()).clone()
}

fn tick(world: &mut World) {
    jengine_core::ecs::behaviors::tick_motion(world, 1.0 / 60.0, None, false);
}

#[test]
fn a_stage_places_its_pieces_and_scripts_move_them() {
    let mut world = World::new();
    let stage = world.spawn((Name("Stage".into()), Transform::default(), Stage { follow: "Hero".into(), ..Default::default() }));
    let hero = world.spawn((
        Name("Hero".into()),
        Transform::default(),
        SpriteComponent { texture_id: "hero.png".into(), size: Some([24.0, 32.0]), anchor: [0.5, 0.0], ..Default::default() },
        StageBillboard::default(),
    ));
    let house = world.spawn((
        Transform::default(),
        Mesh2D { texture_id: "wall.png".into(), ..Default::default() },
        StageQuad { origin: [-3.0, 5.0, 0.0], u: [6.0, 0.0, 0.0], v: [0.0, 0.0, 4.0], repeat: [3.0, 2.0], ..Default::default() },
    ));
    let floor = world.spawn((
        Transform::default(),
        Mesh2D { texture_id: "grass.png".into(), ..Default::default() },
        StageQuad { origin: [-10.0, -10.0, 0.0], u: [20.0, 0.0, 0.0], v: [0.0, 30.0, 0.0], repeat: [5.0, 7.5], ground: true, ..Default::default() },
    ));
    let lamp = world.spawn((
        Transform::default(),
        PointLight2D::default(),
        StageBillboard { position: [2.0, 4.0, 2.0], light_radius: 5.0, ..Default::default() },
    ));
    let ids = jengine_core::scene::format::world_texture_ids(&world);
    assert!(ids.contains("wall.png") && ids.contains("grass.png"), "a quad's texture is its mesh's, preloaded");
    tick(&mut world);
    assert_eq!(mesh(&world, house).vertices.len(), 4 * 6, "cut into its repeats");
    assert!(mesh(&world, floor).vertices.len() >= 4 * 5 * 7, "the floor is tiled in its geometry");
    // The hero stands in front of the house, the floor under both.
    let hz = world.get::<&SpriteComponent>(hero).unwrap().z_order;
    assert!(hz > mesh(&world, house).z_order && mesh(&world, house).z_order > mesh(&world, floor).z_order);
    // The light shrank to its stage radius at its depth and lies on the floor.
    let l = (*world.get::<&PointLight2D>(lamp).unwrap()).clone();
    assert!(l.radius > 0.0 && l.radius < 5.0 * 32.0, "farther than the target, so smaller: {}", l.radius);
    assert!(l.squash < 1.0);

    // Scripts: walk the hero north through `stgp`, turn the camera through `stgs`.
    let mut host = Host(world);
    let v = serde_json::json!({ "e": hero.id(), "x": 0.0, "y": 9.0, "z": 0.0 });
    assert!(handle_world_command(&mut host, "stgp", &JsonArgs(v)), "the shared arm takes `stgp`");
    let v = serde_json::json!({ "e": stage.id(), "f": "follow", "v": "" });
    assert!(handle_world_command(&mut host, "stgs", &JsonArgs(v)), "the shared arm takes `stgs`");
    tick(&mut host.0);
    let world = host.0;
    let hz = world.get::<&SpriteComponent>(hero).unwrap().z_order;
    assert!(hz < mesh(&world, house).z_order, "behind the house now, so drawn before it");
    let (at, s) = world.get::<&Transform>(hero).map(|t| (t.position, t.scale.x)).unwrap();
    let cam = world.get::<&Stage>(stage).unwrap().camera(Vec2::ZERO);
    let (want, _) = cam.project(Vec3::new(0.0, 9.0, 0.0)).unwrap();
    assert!(at.distance(want) < 1e-3 && s < 1.0, "placed where its feet project, and smaller: {at} {s}");
}

/// A striped floor in perspective through the velocity pass and
/// `depth_of_field`, focused on the stage's target: returns the frame's red
/// channel, and the rows the floor's near, target and far points fall on.
#[cfg(all(feature = "gpu-wgpu", feature = "depth_of_field"))]
fn floor_through_depth_of_field() -> Option<(Vec<u8>, [usize; 3])> {
    use std::collections::HashMap;
    use jengine_core::renderer::material::UniformValue;
    let mut params = HashMap::new();
    params.insert("focus".to_string(), UniformValue::Float(0.0));
    params.insert("range".to_string(), UniformValue::Float(2.0));
    params.insert("strength".to_string(), UniformValue::Float(0.3));
    params.insert("max_radius".to_string(), UniformValue::Float(8.0));
    floor_through("depth_of_field", params)
}

/// The striped floor through one post effect: the frame's red, and the rows
/// the floor's near, target and far points fall on.
#[cfg(all(feature = "gpu-wgpu", any(feature = "depth_of_field", feature = "depth_haze")))]
fn floor_through(
    effect: &str,
    params: std::collections::HashMap<String, jengine_core::renderer::material::UniformValue>,
) -> Option<(Vec<u8>, [usize; 3])> {
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::ecs::wgpu_systems::{begin_velocity, render_velocity_wgpu};
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::backend::wgpu::{VELOCITY_FORMAT, WgpuPostProcessStack, WgpuSpriteBatch};
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings};

    const W: u32 = 160;
    const H: u32 = 120;
    let gpu = HeadlessGpu::new(W, H)?;
    let scene_format = wgpu::TextureFormat::Rgba16Float;
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, scene_format, gpu.color_format).expect("post stack");
    stack.set_settings(PostProcessSettings {
        effects: vec![PostProcessEffectSettings { name: effect.into(), enabled: true, params }],
        ..Default::default()
    });
    // Stripes: 32 texels, black and white, each its own colour - nearest, so
    // a sharp stripe edge is black next to white with no grey between.
    let mut stripes = Vec::new();
    for x in 0..32 {
        let v = if x % 2 == 0 { 255 } else { 0 };
        stripes.extend_from_slice(&[v, v, v, 255]);
    }
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("stripes".into(), RawTextureData { width: 32, height: 1, rgba: stripes });
    assets.ensure_wgpu_texture("stripes", &gpu.device, &gpu.queue).unwrap();

    let mut world = World::new();
    world.spawn((Transform::default(), Stage { pitch: 0.5, distance: 30.0, zoom: 24.0, ..Default::default() }));
    world.spawn((
        Transform::default(),
        Mesh2D { texture_id: "stripes".into(), ..Default::default() },
        StageQuad { origin: [-12.0, -12.0, 0.0], u: [24.0, 0.0, 0.0], v: [0.0, 60.0, 0.0], repeat: [6.0, 1.0], ground: true, ..Default::default() },
    ));
    tick(&mut world);
    let cam = world.query::<&Stage>().iter().next().unwrap().1.camera(Vec2::ZERO);
    let row = |y: f32| (H as f32 / 2.0 - cam.project(Vec3::new(0.0, y, 0.0)).unwrap().0.y).round() as usize;
    let rows = [row(-3.0), row(0.0), row(4.0)];

    let camera = Camera2D::new(W as f32, H as f32);
    jengine_core::renderer::frame_view::set_frame_view(&camera);
    let vp = camera.view_projection();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, scene_format).unwrap();
    let mut encoder = gpu.encoder();
    batch.begin_frame();
    batch.set_target_format(&gpu.device, scene_format);
    {
        // The colour pass: the floor mesh as the sprite pass queues it.
        let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin();
        for (_, (t, mesh)) in world.query::<(&Transform, &Mesh2D)>().iter() {
            let positions: Vec<Vec2> = mesh.vertices.iter().map(|v| t.position + Vec2::from(v.position)).collect();
            let uvq: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| [v.uv[0] * v.q, v.uv[1] * v.q, v.q]).collect();
            let colors = vec![[1.0f32; 4]; positions.len()];
            batch.draw_mesh(&positions, &uvq, &colors, &mesh.triangles());
            assert!(!batch.has_overflow());
        }
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture("stripes").unwrap());
    }
    let (camera_motion, _) = begin_velocity(&camera);
    let pipeline = batch.velocity_pipeline(&gpu.device);
    batch.set_target_format(&gpu.device, VELOCITY_FORMAT);
    {
        let clear = wgpu::Color { r: camera_motion[0] as f64, g: camera_motion[1] as f64, b: stack.depth_clear() as f64, a: 0.0 };
        let mut vpass = stack.velocity_target().begin_pass(&mut encoder, Some(clear));
        let drawn = render_velocity_wgpu(&world, &SortLayers::default(), &mut batch, Vec2::splat(16.0), &camera, 1.0, |b, tex| {
            b.flush_with_pipeline(&gpu.device, &gpu.queue, &mut vpass, &vp, assets.get_wgpu_texture(tex).unwrap(), &pipeline);
        });
        assert_eq!(drawn, 1, "the velocity pass draws the floor mesh");
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    jengine_core::renderer::frame_view::clear_frame_view();
    Some((img.rgba.chunks(4).map(|p| p[0]).collect(), rows))
}

#[cfg(all(feature = "gpu-wgpu", feature = "depth_of_field"))]
#[test]
fn a_stage_floor_is_sharp_at_the_target_and_soft_away_from_it() {
    let Some((img, [near, target, far])) = floor_through_depth_of_field() else {
        eprintln!("a_stage_is_drawn_in_perspective: no GPU adapter - skipped");
        return;
    };
    const W: usize = 160;
    // Part-lit pixels across the middle of a row: stripes in focus are black
    // and white, out of focus grey.
    let grey = |row: usize| img[row * W + 30..row * W + 130].iter().filter(|&&r| r > 40 && r < 215).count();
    assert!(near > target && target > far, "near is low on the screen, far high: {near} {target} {far}");
    let (gn, gt, gf) = (grey(near), grey(target), grey(far));
    assert!(gt <= 12, "the floor at the target is sharp: {gt} grey of 100 in row {target}");
    assert!(gf >= 60, "the far floor is soft: {gf} grey in row {far}");
    assert!(gn >= 40, "and the near floor: {gn} grey in row {near}");
}

/// Depth haze on the same floor: the far rows pushed toward the haze's
/// colour by how deep they are, the floor at the target left alone - the
/// depth read per pixel, from the stage, as depth of field reads it.
#[cfg(all(feature = "gpu-wgpu", feature = "depth_haze"))]
#[test]
fn a_stage_floor_fades_into_the_haze_with_distance() {
    use std::collections::HashMap;
    use jengine_core::renderer::material::UniformValue;
    let mut params = HashMap::new();
    // Pure red haze, full strength from z 0 (the target) to z -40 (4 units on).
    params.insert("color".to_string(), UniformValue::Vec4([1.0, 0.0, 0.0, 1.0]));
    params.insert("near".to_string(), UniformValue::Float(0.0));
    params.insert("far".to_string(), UniformValue::Float(-40.0));
    let Some((img, [near, target, far])) = floor_through("depth_haze", params) else {
        eprintln!("a_stage_is_drawn_in_perspective: no GPU adapter - skipped");
        return;
    };
    const W: usize = 160;
    // Red only: black stripes go red as the haze takes them.
    let black = |row: usize| img[row * W + 30..row * W + 130].iter().filter(|&&r| r < 40).count();
    assert!(black(near) > 30 && black(target) > 30, "near and at the target the black stripes stay black: {} {}", black(near), black(target));
    assert_eq!(black(far), 0, "4 units on, the haze has taken them all");
}
