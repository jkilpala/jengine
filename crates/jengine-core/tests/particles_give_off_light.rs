//! **Particles give off light** (`light` on a `particle_emitter`, Oct 2026;
//! Unity's Lights module). `particles::tick_particles` - which every host
//! already calls - leaves each emitter's particle lights on the component,
//! and `renderer::lighting::gather_lights` adds them as shadowless point
//! lights, so the GPU's light pass and `light_at` read one list. Found by
//! the showcase's Ember Cavern.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::Transform;
use jengine_core::particles::{ParticleEmitter, ParticleLights, ParticleSystem, set_emitter_field, tick_particles};
use jengine_core::renderer::lighting::{gather_lights, has_lights, light_at};

fn emitter(light: ParticleLights) -> ParticleEmitter {
    ParticleEmitter {
        emission_rate: 400.0,
        max_particles: 40,
        lifetime: [1.0, 1.0],
        speed: [0.0, 0.0],
        color_start: [1.0, 0.5, 0.2, 1.0],
        color_end: [1.0, 0.5, 0.2, 0.0],
        size_start: [8.0, 8.0],
        size_end: [8.0, 8.0],
        light: Some(light),
        ..Default::default()
    }
}

fn world_with(em: ParticleEmitter) -> (World, hecs::Entity) {
    let mut world = World::new();
    let e = world.spawn((Transform { position: Vec2::new(100.0, 0.0), ..Default::default() }, em));
    (world, e)
}

fn tick(world: &World, sys: &mut ParticleSystem, frames: usize, dt: f32) {
    for _ in 0..frames {
        tick_particles(world, sys, dt);
    }
}

#[test]
fn each_particle_lights_where_it_is_in_its_colour() {
    let (world, _) = world_with(emitter(ParticleLights { radius: 60.0, ..Default::default() }));
    let mut sys = ParticleSystem::new();
    tick(&world, &mut sys, 1, 0.1);
    let lights = gather_lights(&world);
    assert_eq!(lights.point_lights.len(), 40, "one a particle, as many as are alive");
    let (at, l) = &lights.point_lights[0];
    assert!(at.distance(Vec2::new(100.0, 0.0)) < 1e-3);
    assert_eq!(l.color, [1.0, 0.5, 0.2]);
    assert!(!l.casts_shadows, "particle lights cast no shadows");
    assert!((l.radius - 60.0).abs() < 1e-3);
    // The CPU answer sees them: light at the emitter, none far away.
    let near = light_at(Vec2::new(100.0, 0.0), &lights);
    let far = light_at(Vec2::new(400.0, 0.0), &lights);
    assert!(near.x > far.x + 0.5, "{near} vs {far}");
}

#[test]
fn every_and_max_choose_which_particles_carry_one() {
    let (world, _) = world_with(emitter(ParticleLights { every: 3, ..Default::default() }));
    let mut sys = ParticleSystem::new();
    tick(&world, &mut sys, 1, 0.1);
    let n = gather_lights(&world).point_lights.len();
    assert!((6..=22).contains(&n), "about a third of 40: {n}");
    // The same particles keep theirs: a frame later the count is the same.
    tick(&world, &mut sys, 1, 0.01);
    assert_eq!(gather_lights(&world).point_lights.len(), n);

    let (world, _) = world_with(emitter(ParticleLights { max: 5, ..Default::default() }));
    let mut sys = ParticleSystem::new();
    tick(&world, &mut sys, 1, 0.1);
    assert_eq!(gather_lights(&world).point_lights.len(), 5, "capped");
}

#[test]
fn a_fading_shrinking_particle_dims_and_narrows_its_light() {
    let mut em = emitter(ParticleLights { radius: 100.0, ..Default::default() });
    em.emission_rate = 0.0;
    em.burst_count = 1;
    em.size_end = [0.0, 0.0];
    let (world, _) = world_with(em);
    let mut sys = ParticleSystem::new();
    tick(&world, &mut sys, 1, 0.01);
    let young = gather_lights(&world).point_lights[0].1.clone();
    tick(&world, &mut sys, 70, 0.01);
    let old = gather_lights(&world).point_lights[0].1.clone();
    assert!(old.intensity < young.intensity * 0.4, "{} -> {}", young.intensity, old.intensity);
    assert!(old.radius < young.radius * 0.4, "{} -> {}", young.radius, old.radius);
    tick(&world, &mut sys, 40, 0.01);
    assert!(gather_lights(&world).point_lights.is_empty(), "dead particles give no light");
}

#[test]
fn a_fixed_colour_and_no_fade_are_honoured() {
    let (world, _) = world_with(emitter(ParticleLights {
        use_particle_color: false,
        color: [0.2, 0.4, 1.0],
        fade: false,
        intensity: 2.0,
        ..Default::default()
    }));
    let mut sys = ParticleSystem::new();
    tick(&world, &mut sys, 60, 0.015);
    let lights = gather_lights(&world);
    assert!(lights.point_lights.iter().all(|(_, l)| l.color == [0.2, 0.4, 1.0] && l.intensity == 2.0));
}

#[test]
fn a_scene_lit_only_by_particles_still_runs_the_light_pass() {
    let (world, _) = world_with(emitter(ParticleLights::default()));
    assert!(has_lights(&world));
    let (world, _) = world_with(ParticleEmitter::default());
    assert!(!has_lights(&world), "an emitter without light is not a light");
}

#[test]
fn scripts_reach_the_light_through_particles_set() {
    let mut em = ParticleEmitter::default();
    set_emitter_field(&mut em, "light.radius", &serde_json::json!(30.0)).unwrap();
    let l = em.light.expect("setting a field of `light` makes one");
    assert_eq!(l.radius, 30.0);
    assert_eq!(l.every, 1, "the rest as default");
    set_emitter_field(&mut em, "light", &serde_json::Value::Null).unwrap();
    assert!(em.light.is_none(), "null turns it off");
    // And the scene key round-trips.
    let back: ParticleEmitter = serde_json::from_value(serde_json::json!({ "light": { "every": 4 } })).unwrap();
    assert_eq!(back.light.unwrap().every, 4);
}
