//! A project-local jEngine package, used by the test lab to prove that a
//! project's **own** Rust package runs in the native runtime *and* in the
//! editor viewport.
//!
//! It lives under `projects/engine-test-lab/packages/`, not in the engine's
//! `packages/` directory, on purpose: a package that ships with the engine
//! proves nothing about whether a third party's package can work. Scene 60
//! uses it.
//!
//! It deliberately does the simplest visible thing — rotate tagged entities —
//! so that when it does not run, the reason is the package plumbing and never
//! the package's own logic.

use jengine_core::ecs::components::Transform;
use jengine_core::input::{InputState, KeyCode};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::script_api::{ScriptApiRegistry, ScriptValueType};
use jengine_core::package::system_schedule::{SystemPhase, SystemSchedule};
use jengine_core::package::{JEnginePackage, PackageMetadata};
use serde::{Deserialize, Serialize};

/// How much faster a bar spins while Space is held - the visible proof
/// that a package can read the frame's input, in the editor and native
/// alike, without a script forwarding it.
pub const HELD_SPACE_FACTOR: f32 = 4.0;

/// Spins its entity at `speed` radians per second.
///
/// A component the engine has never heard of — which is the point. It reaches
/// a scene only through `ComponentRegistry`, so a scene using it must be v2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spin {
    /// Radians per second. Negative spins the other way.
    pub speed: f32,
}

impl Default for Spin {
    fn default() -> Self {
        Self { speed: 1.0 }
    }
}

/// The package. Named `SpinPackage` rather than `TestlabSpinPackage`, so the
/// builder's name guess would be wrong — `package.toml` declares
/// `entry_point` explicitly, which is the case that field exists for.
pub struct SpinPackage;

impl JEnginePackage for SpinPackage {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "testlab-spin".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "Spins entities carrying a Spin component".to_string(),
            author: String::new(),
            engine_version: Some("0.1.0".to_string()),
            dependencies: vec![],
        }
    }

    fn register_components(&self, registry: &mut ComponentRegistry) {
        registry.register::<Spin>("spin", "Spin", "Test Lab");
    }

    fn register_systems(&self, schedule: &mut SystemSchedule) {
        // `Animation` rather than `PreRender`: this moves a transform, and
        // PreRender runs after the hierarchy resolve, so a spun parent would
        // not carry its children until the next frame.
        schedule.add_system("testlab_spin", SystemPhase::Animation, 0, |ctx| {
            let (world, dt, resources) = ctx.split();
            // The frame's input, lent by the host (`run_phase_with`). Absent
            // in a bare `run_phase`, in which case nothing is held.
            let boost = resources
                .get::<InputState>()
                .is_some_and(|input| input.is_pressed(KeyCode::Space));
            let factor = if boost { HELD_SPACE_FACTOR } else { 1.0 };
            for (_, (transform, spin)) in world.query_mut::<(&mut Transform, &Spin)>() {
                transform.rotation += spin.speed * factor * dt;
            }
        });
    }

    fn register_script_api(&self, api: &mut ScriptApiRegistry) {
        // Metadata only — it gives the editor a tooltip. Installing a real
        // callable global is a separate mechanism; see docs/writing-packages.md.
        api.add_function(
            "spin_speed",
            "testlab-spin",
            "Speed of an entity's Spin component, in radians per second",
            vec![],
            ScriptValueType::Float,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jengine_core::package::host::PackageHost;

    #[test]
    fn the_spin_system_rotates_a_tagged_entity() {
        let mut registry = ComponentRegistry::new();
        jengine_core::package::core_registration::register_core_components(&mut registry);
        let mut host = PackageHost::new();
        host.add_package(Box::new(SpinPackage));
        host.initialize(&mut registry);

        let mut world = hecs::World::new();
        let e = world.spawn((Transform::default(), Spin { speed: 2.0 }));
        let untouched = world.spawn((Transform::default(),));

        host.run_phase(SystemPhase::Animation, &mut world, 0.5);

        assert!((world.get::<&Transform>(e).unwrap().rotation - 1.0).abs() < 1e-6);
        // An entity without the component must be left alone.
        assert_eq!(world.get::<&Transform>(untouched).unwrap().rotation, 0.0);
    }

    #[test]
    fn test_lab_scene_60_loads_and_spins() {
        // End to end against the real asset. The test-lab's scene loader test
        // lives in jengine-core, which cannot depend on a project package, so
        // it builds a registry without `spin` — under which the key is just an
        // unknown component that lands in `CustomComponents` and is dropped.
        // That test would pass on a scene whose spin never loads.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/scenes/60-project-packages.scene");
        let json = std::fs::read_to_string(&path).expect("scene 60 exists");

        let mut registry = ComponentRegistry::new();
        jengine_core::package::core_registration::register_core_components(&mut registry);
        let mut host = PackageHost::new();
        host.add_package(Box::new(SpinPackage));
        host.initialize(&mut registry);

        let mut world = hecs::World::new();
        jengine_core::scene::load_scene_auto(&json, &mut world, &registry)
            .expect("scene 60 parses");

        let spinners = world.query::<&Spin>().iter().count();
        assert_eq!(spinners, 4, "expected 4 spinning bars, got {spinners}");

        let ids: Vec<hecs::Entity> = world.query::<&Spin>().iter().map(|(e, _)| e).collect();
        let before: Vec<f32> = ids
            .iter()
            .map(|e| world.get::<&Transform>(*e).unwrap().rotation)
            .collect();
        for _ in 0..10 {
            host.run_phase(SystemPhase::Animation, &mut world, 1.0 / 60.0);
        }
        for (i, e) in ids.iter().enumerate() {
            assert_ne!(
                world.get::<&Transform>(*e).unwrap().rotation,
                before[i],
                "bar {i} did not spin"
            );
        }
    }

    #[test]
    fn holding_space_spins_faster_because_the_host_lent_its_input() {
        use jengine_core::package::host::FrameResources;
        use jengine_core::renderer::camera::Camera2D;
        let mut registry = ComponentRegistry::new();
        let mut host = PackageHost::new();
        host.add_package(Box::new(SpinPackage));
        host.initialize(&mut registry);
        let mut world = hecs::World::new();
        let e = world.spawn((Transform::default(), Spin { speed: 1.0 }));
        let mut input = InputState::new();
        input.begin_frame();
        input.key_down(KeyCode::Space);
        let mut assets = jengine_core::asset::AssetManager::new();
        host.run_phase_with(
            SystemPhase::Animation,
            &mut world,
            0.5,
            FrameResources { input: &mut input, assets: &mut assets, camera: Camera2D::new(1.0, 1.0) },
        );
        let turned = world.get::<&Transform>(e).unwrap().rotation;
        assert!((turned - 0.5 * HELD_SPACE_FACTOR).abs() < 1e-6, "held Space: {turned}");
        assert!(input.is_pressed(KeyCode::Space), "the host has its input back");
        // The bare form lends nothing, so nothing is held.
        host.run_phase(SystemPhase::Animation, &mut world, 0.5);
        let turned = world.get::<&Transform>(e).unwrap().rotation;
        assert!((turned - 0.5 * HELD_SPACE_FACTOR - 0.5).abs() < 1e-6, "no input: {turned}");
    }

    #[test]
    fn the_component_registers_under_the_name_scenes_use() {
        let mut registry = ComponentRegistry::new();
        let mut host = PackageHost::new();
        host.add_package(Box::new(SpinPackage));
        host.initialize(&mut registry);
        assert!(registry.get("spin").is_some());
    }
}
