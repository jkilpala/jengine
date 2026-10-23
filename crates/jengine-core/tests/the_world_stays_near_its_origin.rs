//! **The world stays near its origin** (Oct 2026; `ecs::floating_origin`):
//! a car driving off at speed with a `floating_origin` has the whole world
//! moved back under it whenever it strays past the threshold - and nothing
//! notices. A crate resting on the ground stays resting, a box on a
//! kinematic platform is not flung by the jump, a falling ball keeps its
//! speed, the camera keeps following without sweeping across, and particles
//! in flight stay where they were in the world, and scenery that despawns
//! once the camera has passed it is not mistaken for passed. Found by the showcase's The
//! Endless Road. The shift itself is tested in the module.

#![cfg(all(feature = "native", feature = "physics"))]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::behaviors::{despawn_offscreen, tick_motion};
use jengine_core::ecs::components::{
    BodyType, Camera2DComponent, CameraFollow, Collider2D, ColliderShape, DespawnOffscreen, RigidBody2D, Transform,
    Velocity2D,
};
use jengine_core::ecs::floating_origin::FloatingOrigin;
use jengine_core::ecs::systems::{physics_frame_begin, physics_frame_end, update_camera};
use jengine_core::particles::{ParticleEmitter, ParticleSystem, tick_particles};
use jengine_core::physics::PhysicsWorld;
use jengine_core::renderer::camera::Camera2D;

const DT: f32 = 1.0 / 60.0;

struct Host {
    world: World,
    physics: PhysicsWorld,
    particles: ParticleSystem,
    camera: Camera2D,
    prev_camera: Vec2,
}

impl Host {
    /// A frame as every host runs one: motion (the shift is in it), the
    /// physics frame, the particles, what has left the view, the camera.
    fn frame(&mut self) {
        tick_motion(&mut self.world, DT, None, false);
        physics_frame_begin(&mut self.world, &mut self.physics, DT);
        let steps = self.physics.begin_frame(DT);
        for _ in 0..steps {
            self.physics.step_once();
        }
        let contacts = self.physics.end_frame();
        physics_frame_end(&mut self.world, &mut self.physics, &contacts, &mut self.particles, DT);
        tick_particles(&self.world, &mut self.particles, DT);
        let view = (self.camera.position, self.camera.half_extents());
        let travel = self.camera.position - self.prev_camera;
        for e in despawn_offscreen(&mut self.world, view, travel) {
            let _ = self.world.despawn(e);
        }
        self.prev_camera = self.camera.position;
        update_camera(&self.world, &mut self.camera, DT);
    }

    fn at(&self, e: hecs::Entity) -> Vec2 {
        self.world.get::<&Transform>(e).unwrap().position
    }
}

fn body(at: Vec2, kind: BodyType, shape: ColliderShape) -> (Transform, Velocity2D, RigidBody2D, Collider2D) {
    (
        Transform { position: at, ..Default::default() },
        Velocity2D::default(),
        RigidBody2D { body_type: kind, ..Default::default() },
        Collider2D { shape, ..Default::default() },
    )
}

#[test]
fn a_car_drives_far_and_nothing_near_it_notices_the_world_moving() {
    let mut world = World::new();
    world.spawn((
        Transform::default(),
        Camera2DComponent { is_active: true, viewport_width: 1280.0, viewport_height: 720.0, zoom: 1.0, ..Default::default() },
    ));
    // The car: no body, driven by its velocity, followed by the camera.
    let car = world.spawn((
        Transform::default(),
        Velocity2D { x: 3000.0, ..Default::default() },
        FloatingOrigin { threshold: 1500.0, ..Default::default() },
        CameraFollow::default(),
    ));
    // Ground only round the crate: one left behind by the shift would drop it.
    let ground = world.spawn(body(
        Vec2::new(800.0, -100.0),
        BodyType::Static,
        ColliderShape::Box { width: 600.0, height: 40.0 },
    ));
    let crate_ = world.spawn(body(Vec2::new(800.0, -60.0), BodyType::Dynamic, ColliderShape::Box { width: 40.0, height: 40.0 }));
    let platform = world.spawn(body(
        Vec2::new(-600.0, 200.0),
        BodyType::Kinematic,
        ColliderShape::Box { width: 300.0, height: 20.0 },
    ));
    let rider = world.spawn(body(Vec2::new(-600.0, 230.0), BodyType::Dynamic, ColliderShape::Box { width: 40.0, height: 40.0 }));
    let ball = world.spawn(body(Vec2::new(300.0, 5000.0), BodyType::Dynamic, ColliderShape::Circle { radius: 10.0 }));
    // A signpost riding just ahead of the car, which goes once passed.
    let sign = world.spawn((
        Transform { position: Vec2::new(200.0, 0.0), ..Default::default() },
        Velocity2D::default(),
        DespawnOffscreen { margin: 50.0, behind_only: true, ..Default::default() },
    ));
    // Particles that hang where they were born, in world space.
    let emitter = world.spawn((
        Transform { position: Vec2::new(200.0, 300.0), ..Default::default() },
        ParticleEmitter {
            emission_rate: 30.0,
            lifetime: [100.0, 100.0],
            speed: [0.0, 0.0],
            gravity_modifier: 0.0,
            world_space: true,
            active: true,
            ..Default::default()
        },
    ));
    let mut host = Host {
        world,
        physics: PhysicsWorld::new(0.0, -9.81, 100.0),
        particles: ParticleSystem::new(),
        camera: Camera2D::new(1280.0, 720.0),
        prev_camera: Vec2::ZERO,
    };
    // Settle with the car parked, then stop making particles.
    host.world.get::<&mut Velocity2D>(car).unwrap().x = 0.0;
    for _ in 0..60 {
        host.frame();
    }
    host.world.get::<&mut ParticleEmitter>(emitter).unwrap().emission_rate = 0.0;
    host.world.get::<&mut Velocity2D>(car).unwrap().x = 3000.0;
    host.world.get::<&mut Velocity2D>(sign).unwrap().x = 3000.0;
    let rel = |h: &Host, e| h.at(e) - h.at(ground);
    let (crate_at, rider_at) = (rel(&host, crate_), rel(&host, rider) - rel(&host, platform));

    let mut shifts = 0;
    let mut last_vy = host.physics.get_velocity(ball.id() as u64).map(|v| v.1);
    for _ in 0..90 {
        let car_before = host.at(car).x;
        host.frame();
        if host.at(car).x < car_before {
            shifts += 1;
        }
        // Every body is where its entity is: none was left behind.
        for (e, what) in [(ground, "ground"), (crate_, "crate"), (platform, "platform"), (rider, "rider")] {
            let (x, y) = host.physics.get_position(e.id() as u64).expect("a body");
            assert!((Vec2::new(x, y) - host.at(e)).length() < 1.0, "the {what}'s body is at its entity: {x}, {y} vs {}", host.at(e));
        }
        // Nothing near the car moved relative to the ground.
        assert!((rel(&host, crate_) - crate_at).length() < 1.0, "the crate rests where it was: {}", rel(&host, crate_));
        let on_platform = rel(&host, rider) - rel(&host, platform);
        assert!((on_platform - rider_at).length() < 1.0, "the box on the platform was not flung: {on_platform}");
        // The ball falls on at the speed gravity gives it, shift or not.
        let vy = host.physics.get_velocity(ball.id() as u64).map(|v| v.1);
        if let (Some(a), Some(b)) = (last_vy, vy) {
            assert!((b - a).abs() < 40.0, "the ball's speed changes only by gravity: {a} -> {b}");
        }
        last_vy = vy;
        assert!(host.world.contains(sign), "the signpost by the car was not taken for passed");
        // The camera is still with the car.
        assert!(
            (host.camera.position - host.at(car)).length() < 800.0,
            "the camera follows across the shift: camera {} car {}",
            host.camera.position,
            host.at(car)
        );
    }
    assert!(shifts >= 2, "the world was moved back more than once: {shifts}");
    // And the car has really come far: the custom total plus where it is.
    let moved = host.world.get::<&FloatingOrigin>(car).unwrap().moved;
    assert!(moved.x + host.at(car).x as f64 > 4000.0, "four and a half seconds of driving: {}", moved.x);

    // The hanging particles are still at their emitter, which moved with the world.
    let at = host.at(emitter);
    let em = host.world.get::<&ParticleEmitter>(emitter).unwrap().clone();
    let groups = host.particles.draw_data(&[(emitter.id() as u64, &em, at)], &Default::default());
    let drawn: Vec<Vec2> = groups.iter().flat_map(|g| g.particles.iter().map(|p| p.position)).collect();
    assert!(!drawn.is_empty(), "there are particles");
    for p in drawn {
        assert!((p - at).length() < 2.0, "a particle stays where it was born: {p} vs {at}");
    }
}
