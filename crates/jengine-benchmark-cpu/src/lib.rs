//! A headless CPU benchmark: the same engine code timed natively and as
//! WebAssembly, so a build setting (`opt-level`, a `wasm-opt` level) can be
//! judged by what it costs in speed as well as what it saves in size
//! (`docs/build-sizes.md`). BunnyMark measures the GPU; this measures what
//! the build settings change.
//!
//! Two workloads, both deterministic:
//!
//! * **frame** - what every host runs each frame without drawing: the motion
//!   step over a few thousand moving sprites (a quarter with a child), the
//!   physics frame over a pile of boxes settling on a floor, particles and
//!   the hierarchy resolve.
//! * **scene** - loading that world back from its v1 scene JSON, which is
//!   serde over every component, a third of the web module's code.
//!
//! Natively `jengine-benchmark-cpu` times them; on the web `bench.mjs` times
//! the exports of a `wasm-pack --target nodejs` build.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::behaviors::tick_motion;
use jengine_core::ecs::components::{
    BodyType, Collider2D, ColliderShape, Name, Parent, RigidBody2D, SpriteComponent, Transform, Velocity2D,
};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::ecs::systems::{physics_frame_begin, physics_frame_end};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::particles::{tick_particles, ParticleEmitter, ParticleSystem};
use jengine_core::physics::PhysicsWorld;
use jengine_core::scene::format::EntityData;

pub const DT: f32 = 1.0 / 60.0;
pub const SPRITES: usize = 3000;
pub const BOXES: usize = 400;
pub const EMITTERS: usize = 8;

/// The world, its physics and particles, and the scene it saves as.
pub struct Bench {
    world: World,
    physics: PhysicsWorld,
    particles: ParticleSystem,
    registry: ComponentRegistry,
    scene_json: String,
    bullet: EntityData,
    checksum: f64,
}

/// What a script spawns by the hundred: a bullet, as a `.prefab` holds it.
pub const BULLET_PREFAB: &str = r#"{
  "name": "Bullet",
  "components": {
    "transform": { "position": [0.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] },
    "sprite": { "texture_id": "sprites/bullet.png", "size": [8.0, 8.0], "color": [1.0, 0.9, 0.4, 1.0], "z_order": 5 },
    "velocity": { "x": 0.0, "y": 600.0 },
    "collider": { "shape": { "Circle": { "radius": 4.0 } }, "is_sensor": true },
    "lifetime": { "remaining": 2.0 },
    "tags": ["bullet", "player_shot"]
  }
}"#;

impl Default for Bench {
    fn default() -> Self {
        Self::new()
    }
}

impl Bench {
    pub fn new() -> Bench {
        let mut world = World::new();
        build_world(&mut world);
        let scene_json = serde_json::to_string(&jengine_core::scene::save_scene(&world)).expect("a scene serialises");
        let mut registry = ComponentRegistry::new();
        jengine_core::package::core_registration::register_core_components(&mut registry);
        Bench {
            world,
            physics: PhysicsWorld::new(0.0, -9.81, 100.0),
            particles: ParticleSystem::new(),
            registry,
            scene_json,
            bullet: jengine_core::scene::from_json_text(BULLET_PREFAB).expect("the bullet prefab parses"),
            checksum: 0.0,
        }
    }

    /// Spawn the bullet prefab `n` times into a fresh world, the way the
    /// native runtime spawns a script's `spawn_prefab`: decoded once
    /// (`PreparedPrefab`), cloned per spawn.
    pub fn spawn(&mut self, n: u32) {
        let mut world = World::new();
        let prepared = jengine_core::scene::prefab::PreparedPrefab::new(&self.bullet, &self.registry);
        for _ in 0..n {
            prepared.spawn(&mut world, &self.bullet.name);
        }
        self.checksum += world.len() as f64;
    }

    /// Spawn the bullet prefab `n` times through the component registry -
    /// the browser hosts' and behaviour graphs' way.
    pub fn spawn_registry(&mut self, n: u32) {
        let mut world = World::new();
        for _ in 0..n {
            jengine_core::behavior::command::spawn_entity_data(&mut world, &self.registry, &self.bullet);
        }
        self.checksum += world.len() as f64;
    }

    /// Run `n` frames.
    pub fn frames(&mut self, n: u32) {
        for _ in 0..n {
            tick_motion(&mut self.world, DT, None, false);
            physics_frame_begin(&mut self.world, &mut self.physics, DT);
            let steps = self.physics.begin_frame(DT);
            for _ in 0..steps {
                self.physics.step_once();
            }
            let contacts = self.physics.end_frame();
            physics_frame_end(&mut self.world, &mut self.physics, &contacts, &mut self.particles, DT);
            tick_particles(&self.world, &mut self.particles, DT);
            resolve_world_transforms(&mut self.world);
        }
        // Read the result, so nothing the frames did can be optimised away.
        self.checksum += self.world.query::<&Transform>().iter().map(|(_, t)| t.position.y as f64).sum::<f64>();
    }

    /// Load the saved scene `n` times, each into a fresh world.
    pub fn load_scene(&mut self, n: u32) {
        for _ in 0..n {
            let mut world = World::new();
            jengine_core::scene::load_scene_auto(&self.scene_json, &mut world, &self.registry).expect("the scene loads");
            self.checksum += world.len() as f64;
        }
    }

    pub fn scene_bytes(&self) -> usize {
        self.scene_json.len()
    }

    pub fn checksum(&self) -> f64 {
        self.checksum
    }
}

fn build_world(world: &mut World) {
    for i in 0..SPRITES {
        let (x, y) = ((i % 100) as f32 * 12.0, (i / 100) as f32 * 12.0);
        let angle = i as f32 * 0.37;
        let e = world.spawn((
            Name(format!("sprite_{i}")),
            Transform { position: Vec2::new(x, y), ..Default::default() },
            SpriteComponent { size: Some([16.0, 16.0]), ..Default::default() },
            Velocity2D { x: angle.cos() * 40.0, y: angle.sin() * 40.0, gravity: 0.0 },
        ));
        if i % 4 == 0 {
            world.spawn((
                Name(format!("child_{i}")),
                Transform { position: Vec2::new(4.0, 4.0), ..Default::default() },
                SpriteComponent { size: Some([8.0, 8.0]), ..Default::default() },
                Parent(e),
            ));
        }
    }
    let solid = |shape| Collider2D { shape, ..Default::default() };
    world.spawn((
        Name("floor".into()),
        Transform { position: Vec2::new(0.0, -400.0), ..Default::default() },
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        solid(ColliderShape::Box { width: 2000.0, height: 40.0 }),
    ));
    for i in 0..BOXES {
        let (col, row) = ((i % 20) as f32, (i / 20) as f32);
        world.spawn((
            Name(format!("box_{i}")),
            Transform { position: Vec2::new(col * 26.0 - 260.0 + row * 3.0, row * 26.0 - 300.0), ..Default::default() },
            RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
            solid(ColliderShape::Box { width: 24.0, height: 24.0 }),
        ));
    }
    for i in 0..EMITTERS {
        world.spawn((
            Name(format!("emitter_{i}")),
            Transform { position: Vec2::new(i as f32 * 100.0, 200.0), ..Default::default() },
            ParticleEmitter::default(),
        ));
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    /// The benchmark as JavaScript sees it; `bench.mjs` times each call.
    #[wasm_bindgen]
    pub struct WasmBench(super::Bench);

    #[wasm_bindgen]
    impl WasmBench {
        #[wasm_bindgen(constructor)]
        pub fn new() -> WasmBench {
            WasmBench(super::Bench::new())
        }
        pub fn frames(&mut self, n: u32) {
            self.0.frames(n)
        }
        pub fn load_scene(&mut self, n: u32) {
            self.0.load_scene(n)
        }
        pub fn spawn(&mut self, n: u32) {
            self.0.spawn(n)
        }
        pub fn spawn_registry(&mut self, n: u32) {
            self.0.spawn_registry(n)
        }
        pub fn scene_bytes(&self) -> usize {
            self.0.scene_bytes()
        }
        pub fn checksum(&self) -> f64 {
            self.0.checksum()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_workloads_run_and_the_pile_settles() {
        let mut b = Bench::new();
        assert!(b.scene_bytes() > 100_000, "the scene holds the whole world");
        b.frames(120);
        b.load_scene(1);
        // The boxes fell onto the floor rather than through it.
        let lowest = b.world.query::<(&Name, &Transform)>().iter().filter(|(_, (n, _))| n.0.starts_with("box_")).map(|(_, (_, t))| t.position.y).fold(f32::MAX, f32::min);
        assert!(lowest > -400.0 && lowest < -350.0, "the lowest box rests on the floor: {lowest}");
    }

    #[test]
    fn both_spawn_paths_give_a_bullet_every_component() {
        use jengine_core::ecs::components::{Lifetime, Tags};
        let b = Bench::new();
        for registry in [false, true] {
            let mut world = World::new();
            let e = if registry {
                jengine_core::behavior::command::spawn_entity_data(&mut world, &b.registry, &b.bullet)
            } else {
                jengine_core::scene::prefab::PreparedPrefab::new(&b.bullet, &b.registry).spawn(&mut world, "Bullet")
            };
            assert!(world.get::<&Transform>(e).is_ok(), "transform (registry: {registry})");
            assert!(world.get::<&SpriteComponent>(e).is_ok(), "sprite (registry: {registry})");
            assert!(world.get::<&Velocity2D>(e).is_ok(), "velocity (registry: {registry})");
            assert!(world.get::<&Collider2D>(e).is_ok(), "collider (registry: {registry})");
            assert!(world.get::<&Lifetime>(e).is_ok(), "lifetime (registry: {registry})");
            assert_eq!(world.get::<&Tags>(e).map(|t| t.0.len()).unwrap_or(0), 2, "tags (registry: {registry})");
        }
    }
}
