//! **GPU particles that keep state** (Oct 2026; `particles::gpu_state`,
//! `simulate: "state"`): a compute pass steps every particle's position and
//! velocity once a frame, so they bounce off colliders, are pushed by moving
//! ones and trail a moving emitter. `gpu_state::step_cpu` is the compute
//! shader on the CPU, line for line: this runs both on one emitter and
//! compares, then draws it. Found by the showcase's Ember Storm.

mod support;

#[test]
fn every_host_steps_stateful_particles_through_the_shared_frame() {
    let src = support::with_frame_draw(include_str!("../src/ecs/frame_draw.rs"));
    assert!(src.contains("gpu_state::gather(world)") && src.contains("step_particle_states("));
}

#[cfg(feature = "gpu-wgpu")]
fn storm() -> jengine_core::particles::gpu::GpuParticles {
    use glam::Vec2;
    use jengine_core::particles::gpu::{GpuCollider, GpuParticles, GpuSimulation};
    let mut g = GpuParticles {
        count: 512,
        lifetime: [1.5, 2.5],
        speed: [80.0, 160.0],
        angle: [0.6, 2.5],
        gravity: [0.0, -400.0],
        drag: 0.4,
        noise: 60.0,
        noise_speed: 1.5,
        bounce: 0.5,
        friction: 0.2,
        inherit: 0.5,
        size: [6.0, 3.0],
        color_start: [1.0, 1.0, 1.0, 1.0],
        color_mid: [1.0, 1.0, 1.0, 1.0],
        color_end: [1.0, 1.0, 1.0, 1.0],
        simulate: GpuSimulation::State,
        seed: 11,
        ..Default::default()
    };
    g.world.colliders = vec![
        GpuCollider { centre: Vec2::new(0.0, -80.0), half: Vec2::new(300.0, 20.0), rotation: 0.0, circle: false, velocity: Vec2::ZERO },
        GpuCollider { centre: Vec2::new(60.0, -10.0), half: Vec2::splat(18.0), rotation: 0.0, circle: true, velocity: Vec2::new(-120.0, 0.0) },
    ];
    g
}

/// Sixty frames of one moving emitter, stepped on the GPU and on the CPU: the
/// same particles in the same places.
#[cfg(feature = "gpu-wgpu")]
#[test]
fn the_gpu_steps_what_the_cpu_steps() {
    use glam::Vec2;
    use jengine_core::particles::gpu_state::{StateEmitter, StepClock, initial, step_cpu, step_uniform};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;

    let Some(gpu) = HeadlessGpu::new(64, 64) else {
        eprintln!("stateful_particles_step_on_the_gpu: no GPU adapter - skipped");
        return;
    };
    let mut batch = WgpuSpriteBatch::new(&gpu.device, wgpu::TextureFormat::Rgba16Float).unwrap();
    if !batch.particle_states_supported(&gpu.device) {
        eprintln!("stateful_particles_step_on_the_gpu: no compute on this adapter - skipped");
        return;
    }
    let mut g = storm();
    let mut cpu = initial(&g);
    let mut clock = StepClock::default();
    for frame in 0..60 {
        g.time += 1.0 / 60.0;
        let e = StateEmitter {
            key: 7,
            emitter: g.clone(),
            origin: Vec2::new(-100.0 + frame as f32 * 3.0, 0.0),
            rotation: 0.0,
            scale: Vec2::ONE,
        };
        let mut encoder = gpu.encoder();
        batch.step_particle_states(&gpu.device, &gpu.queue, &mut encoder, std::slice::from_ref(&e));
        gpu.submit(encoder);
        let (u, next) = step_uniform(&e, &clock);
        clock = next;
        if let Some(u) = u {
            step_cpu(&mut cpu, &u);
        }
    }
    let on_gpu = batch.read_particle_state(&gpu.device, &gpu.queue, 7).expect("its buffer");
    assert_eq!(on_gpu.len(), cpu.len());
    let born = cpu.iter().filter(|p| p.age >= 0.0).count();
    assert!(born > 100, "a good share born: {born}");
    let close = on_gpu
        .iter()
        .zip(&cpu)
        .filter(|(a, b)| {
            Vec2::from(a.pos).distance(Vec2::from(b.pos)) < 1.0 && (a.age - b.age).abs() < 1e-3 && a.cycle == b.cycle
        })
        .count();
    // Floating point differs a little between the two (sin, exp), and a
    // particle grazing a collider can take the other branch: nearly all.
    assert!(close * 100 >= on_gpu.len() * 97, "{close} of {} match the CPU", on_gpu.len());
    // Nothing went through the floor.
    assert!(on_gpu.iter().filter(|p| p.age >= 0.0 && p.age < p.life).all(|p| p.pos[1] >= -60.5 || p.pos[0].abs() > 300.0));
}

/// Drawn from its buffer: lit above the floor, and nothing under it.
#[cfg(feature = "gpu-wgpu")]
#[test]
fn stateful_particles_draw_from_their_buffer() {
    use glam::Vec2;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::particles::gpu_state::StateEmitter;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::backend::wgpu::{WgpuPostProcessStack, WgpuSpriteBatch};
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::material::BlendMode;
    use jengine_core::renderer::postprocess::PostProcessSettings;

    const W: u32 = 160;
    const H: u32 = 160;
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("stateful_particles_step_on_the_gpu: no GPU adapter - skipped");
        return;
    };
    let scene_format = wgpu::TextureFormat::Rgba16Float;
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, scene_format, gpu.color_format).expect("post stack");
    stack.set_settings(PostProcessSettings { effects: Vec::new(), ..Default::default() });
    let mut batch = WgpuSpriteBatch::new(&gpu.device, scene_format).unwrap();
    if !batch.particle_states_supported(&gpu.device) {
        eprintln!("stateful_particles_step_on_the_gpu: no compute on this adapter - skipped");
        return;
    }
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white", &gpu.device, &gpu.queue).unwrap();
    // Dropped from above onto a floor whose top is at y = -60.
    let mut g = storm();
    g.speed = [0.0, 20.0];
    g.noise = 0.0;
    g.lifetime = [6.0, 6.0];
    g.world.colliders.truncate(1);
    let camera = Camera2D::new(W as f32, H as f32);
    for _ in 0..150 {
        g.time += 1.0 / 60.0;
        let e = StateEmitter { key: 3, emitter: g.clone(), origin: Vec2::new(0.0, 60.0), rotation: 0.0, scale: Vec2::ONE };
        let mut encoder = gpu.encoder();
        batch.step_particle_states(&gpu.device, &gpu.queue, &mut encoder, std::slice::from_ref(&e));
        gpu.submit(encoder);
    }
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    batch.begin_frame();
    batch.set_target_format(&gpu.device, scene_format);
    batch.set_blend(BlendMode::Additive);
    {
        let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin();
        let u = g.uniform(Vec2::new(0.0, 60.0), 0.0, Vec2::ONE, [[0.0; 4]; 4]);
        batch.set_gpu_particles(Some((u, g.count, Some(3))));
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture("white").unwrap());
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let lit = |x: u32, y: u32| img.rgba[((y * W + x) * 4) as usize] > 40;
    // Screen y = 80 - world y: the floor's top (-60) is row 140; a particle
    // resting on it reaches half its size (3) into it.
    let above = (0..W).flat_map(|x| (100..139).map(move |y| (x, y))).filter(|&(x, y)| lit(x, y)).count();
    let under = (0..W).flat_map(|x| (147..H).map(move |y| (x, y))).filter(|&(x, y)| lit(x, y)).count();
    assert!(above > 20, "particles rest on the floor: {above} lit pixels just above it");
    assert_eq!(under, 0, "and none under it");
}
