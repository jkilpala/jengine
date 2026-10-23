//! **A script reads a falling body's velocity natively** (Oct 2026). The
//! frame's `ScriptSystem::run` lends the physics world (`physics_ptr`)
//! instead of copying every body's velocity into the snapshot - a cost cut
//! that also stopped filling `velocities`, so `Physics.get_velocity` on a
//! physics body answered its `Velocity2D` seed: 0 for anything spawned at
//! rest, however fast it fell. Only `fixed_update` and a test that filled the
//! map by hand still saw rapier's numbers. Found by Test Lab scene 212,
//! whose readout said a sliding puck fell at 0 a second.

#![cfg(all(feature = "native", feature = "physics"))]

use hecs::World;
use jengine_core::ecs::components::{
    BodyType, Collider2D, ColliderShape, CustomComponents, Name, RigidBody2D, ScriptComponent, ScriptLanguage,
    Transform,
};
use jengine_core::ecs::systems::{ScriptSystem, ensure_physics_bodies};
use jengine_core::input::InputState;
use jengine_core::physics::PhysicsWorld;

#[test]
fn a_falling_body_reads_its_speed_and_spin() {
    let dir = std::env::temp_dir().join(format!("jengine_velocity_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("probe.lua");
    std::fs::write(
        &path,
        r#"
function update(entity, dt)
    local vx, vy = Physics.get_velocity(entity)
    Entity.set_custom(entity, "Probe", "vy", vy)
    Entity.set_custom(entity, "Probe", "spin", Physics.get_angular_velocity(entity))
end
"#,
    )
    .unwrap();
    let mut world = World::new();
    let e = world.spawn((
        Name("Ball".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 10.0 }, ..Default::default() },
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    physics.set_angular_velocity(e.id() as u64, 2.0);
    for _ in 0..30 {
        physics.step(1.0 / 60.0);
    }
    let (_, want) = physics.get_velocity(e.id() as u64).unwrap();
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    // As the native runner does: lend the physics world for the run.
    system.set_physics_ptr(&physics);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    system.clear_physics_ptr();
    let c = world.get::<&CustomComponents>(e).unwrap();
    let probe = c.0.get("Probe").and_then(|p| p.as_object()).expect("the script wrote its probe");
    let vy = probe["vy"].as_f64().unwrap() as f32;
    let spin = probe["spin"].as_f64().unwrap() as f32;
    assert!(want < -400.0, "half a second of falling: {want}");
    assert!((vy - want).abs() < 1.0, "the script reads the body's fall ({want}), not its seed: {vy}");
    assert!((spin - 2.0).abs() < 0.1, "and its spin: {spin}");
}
