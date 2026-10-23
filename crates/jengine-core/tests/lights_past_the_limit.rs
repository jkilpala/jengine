//! A world can hold more lights than a frame draws, and the ones the screen
//! sees are the ones drawn.
//!
//! The lighting pass drew the first 256 point lights in ECS order, wherever
//! they were, and dropped the rest - so in a world with more, a light on
//! screen went dark while one far off it kept its slot (and still cost a
//! draw). A lantern festival or a lit city is thousands of lights, a few
//! hundred of which the screen can see: the showcase's Ten Thousand Lights.
//! `render_lights` now draws what reaches the camera's view, the nearest
//! `MAX_LIGHTS_PER_KIND` (1024) of a kind when more do (`lighting::lights_for_view`).
//!
//! And lights, occluders and lightmaps are placed by their world transform:
//! a light parented to a moving thing stayed at its local offset.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Parent, PointLight2D, Transform};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::renderer::lighting::{LightData, MAX_LIGHTS_PER_KIND, gather_lights, lights_for_view};

fn light(x: f32, y: f32, radius: f32) -> (Vec2, PointLight2D) {
    (Vec2::new(x, y), PointLight2D { radius, ..Default::default() })
}

#[test]
fn every_light_is_kept_when_all_fit_and_reach() {
    let data = LightData { point_lights: vec![light(0.0, 0.0, 50.0), light(90.0, 0.0, 50.0)], ..Default::default() };
    let chosen = lights_for_view(&data, Vec2::splat(-100.0), Vec2::splat(100.0));
    assert!(matches!(chosen, std::borrow::Cow::Borrowed(_)), "nothing to leave out - no copy");
    assert_eq!(chosen.point_lights.len(), 2);
}

#[test]
fn lights_that_cannot_reach_the_view_are_left_out() {
    let data = LightData {
        point_lights: vec![light(0.0, 0.0, 10.0), light(5000.0, 0.0, 100.0), light(150.0, 0.0, 60.0)],
        ..Default::default()
    };
    let chosen = lights_for_view(&data, Vec2::splat(-100.0), Vec2::splat(100.0));
    let xs: Vec<f32> = chosen.point_lights.iter().map(|(p, _)| p.x).collect();
    assert!(xs.contains(&0.0), "{xs:?}");
    assert!(xs.contains(&150.0), "a light outside the view whose reach crosses into it lights it: {xs:?}");
    assert!(!xs.contains(&5000.0), "{xs:?}");
}

#[test]
fn past_the_limit_the_nearest_are_drawn() {
    // More lights in view than a frame draws, in a row from the middle
    // outward - but listed farthest first, the order that used to decide.
    let mut lights: Vec<_> = (0..MAX_LIGHTS_PER_KIND + 150).map(|i| light(i as f32 * 2.0, 0.0, 1.0)).collect();
    lights.reverse();
    let data = LightData { point_lights: lights, ..Default::default() };
    let chosen = lights_for_view(&data, Vec2::new(-40000.0, -10.0), Vec2::new(40000.0, 10.0));
    assert_eq!(chosen.point_lights.len(), MAX_LIGHTS_PER_KIND);
    let farthest = chosen.point_lights.iter().map(|(p, _)| p.x).fold(0.0, f32::max);
    assert!(farthest < 2.0 * MAX_LIGHTS_PER_KIND as f32, "the nearest to the middle are kept, got one at {farthest}");
}

#[test]
fn a_light_parented_to_a_moving_thing_goes_with_it() {
    let mut world = World::new();
    let lantern_bearer = world.spawn((Transform { position: Vec2::new(300.0, 40.0), ..Default::default() },));
    world.spawn((
        Transform { position: Vec2::new(10.0, 20.0), ..Default::default() },
        PointLight2D::default(),
        Parent(lantern_bearer),
    ));
    resolve_world_transforms(&mut world);
    let data = gather_lights(&world);
    assert_eq!(data.point_lights[0].0, Vec2::new(310.0, 60.0), "the light sits where its bearer carries it");
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use jengine_core::ecs::components::AmbientLight;
    use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;

    const W: u32 = 64;
    const H: u32 = 64;

    /// 300 lights far off screen, first in world order, then one in the
    /// middle of it: the pass must draw the one the screen can see.
    #[test]
    fn a_light_on_screen_is_drawn_whatever_comes_before_it() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("lights_past_the_limit: no GPU adapter - skipped");
            return;
        };
        let mut world = World::new();
        world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
        for i in 0..300 {
            world.spawn((
                Transform { position: Vec2::new(10_000.0 + i as f32 * 50.0, 0.0), ..Default::default() },
                PointLight2D { radius: 20.0, falloff: 1.0, ..Default::default() },
            ));
        }
        world.spawn((Transform::default(), PointLight2D { radius: 100.0, falloff: 1.0, ..Default::default() }));
        let lights = gather_lights(&world);

        let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
        let camera = Camera2D::new(W as f32, H as f32);
        let mut encoder = gpu.encoder();
        drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
        lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
        lp.composite(&mut encoder, gpu.view());
        gpu.submit(encoder);
        let img = gpu.read_rgba();
        let centre = img.rgba[(((H / 2) * W + W / 2) * 4) as usize];
        assert!(centre > 200, "the light on screen was dropped for 300 off it: {centre}/255");
    }

    /// A thousand lights in view: the pass's buffer starts with room for a
    /// few dozen and has to grow. The two edge lights sort last (the
    /// farthest from the middle), so they sit in the highest slots - a
    /// point light, and a spot light, whose slots follow every point
    /// light's.
    #[test]
    fn the_light_buffer_grows_with_the_scene() {
        use jengine_core::ecs::components::SpotLight2D;
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("lights_past_the_limit: no GPU adapter - skipped");
            return;
        };
        let mut world = World::new();
        world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
        for i in 0..999 {
            // A crowd of faint, tiny lights in the middle.
            let a = i as f32 * 0.7;
            world.spawn((
                Transform { position: Vec2::new(a.cos() * 2.0, a.sin() * 2.0), ..Default::default() },
                PointLight2D { radius: 3.0, falloff: 1.0, intensity: 0.001, ..Default::default() },
            ));
        }
        world.spawn((
            Transform { position: Vec2::new(26.0, 26.0), ..Default::default() },
            PointLight2D { radius: 12.0, falloff: 1.0, ..Default::default() },
        ));
        world.spawn((
            Transform { position: Vec2::new(-26.0, 30.0), ..Default::default() },
            SpotLight2D {
                radius: 40.0,
                falloff: 1.0,
                direction: -std::f32::consts::FRAC_PI_2,
                angle: 0.6,
                ..Default::default()
            },
        ));
        let lights = gather_lights(&world);
        assert_eq!(lights.point_lights.len(), 1000);

        let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
        let camera = Camera2D::new(W as f32, H as f32);
        let mut encoder = gpu.encoder();
        drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
        lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
        lp.composite(&mut encoder, gpu.view());
        gpu.submit(encoder);
        let img = gpu.read_rgba();
        // World (x, y) to the pixel under it: y runs down on screen.
        let at = |x: f32, y: f32| {
            let px = (x + W as f32 / 2.0) as u32;
            let py = (H as f32 / 2.0 - y) as u32;
            img.rgba[((py * W + px) * 4) as usize]
        };
        assert!(at(26.0, 26.0) > 150, "the last point light, past the first buffer: {}", at(26.0, 26.0));
        assert!(at(-26.0, 15.0) > 60, "the spot light after them: {}", at(-26.0, 15.0));
        assert!(at(0.0, -24.0) < 10, "and nothing lights what no light reaches: {}", at(0.0, -24.0));
    }
}
