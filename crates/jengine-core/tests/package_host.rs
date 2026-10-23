//! BUGS #13 — a package's systems actually run.
//!
//! `JEnginePackage::register_systems` takes a `SystemSchedule` and **no
//! runtime owned one**, so the method was called by nobody: a package could
//! declare a system and it would never execute. Physics, audio and particles
//! looked fine only because each runtime calls them directly on a hardcoded
//! path; parallax has no such path and was inert.
//!
//! These tests use a stand-in third-party package rather than a built-in one,
//! because the thing that has to work is *someone else's* crate.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use hecs::World;
use jengine_core::ecs::components::Transform;
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::host::PackageHost;
use jengine_core::package::script_api::{ScriptApiRegistry, ScriptValueType};
use jengine_core::package::system_schedule::{SystemPhase, SystemSchedule};
use jengine_core::package::{JEnginePackage, PackageMetadata};
use serde::{Deserialize, Serialize};

/// A component a package brings with it — the registry has never seen this
/// type, exactly like a third party's.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Spin {
    speed: f32,
}

/// Records the order systems ran in, so phase ordering can be asserted rather
/// than assumed.
type Trace = Arc<std::sync::Mutex<Vec<&'static str>>>;

struct TestPackage {
    ticks: Arc<AtomicUsize>,
    trace: Trace,
}

impl JEnginePackage for TestPackage {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "test-package".into(),
            version: "1.2.3".into(),
            description: "stand-in for a third-party package".into(),
            author: String::new(),
            engine_version: None,
            dependencies: vec![],
        }
    }

    fn register_components(&self, registry: &mut ComponentRegistry) {
        registry.register::<Spin>("spin", "Spin", "Test");
    }

    fn register_systems(&self, schedule: &mut SystemSchedule) {
        let ticks = Arc::clone(&self.ticks);
        let trace = Arc::clone(&self.trace);
        schedule.add_system("spin_system", SystemPhase::PreRender, 0, move |ctx| {
            let (world, dt, _) = ctx.split();
            ticks.fetch_add(1, Ordering::SeqCst);
            trace.lock().unwrap().push("spin");
            for (_, (t, spin)) in world.query_mut::<(&mut Transform, &Spin)>() {
                t.rotation += spin.speed * dt;
            }
        });

        // A second system in an earlier phase, to pin down ordering.
        let trace = Arc::clone(&self.trace);
        schedule.add_system("early_system", SystemPhase::PreUpdate, 0, move |_| {
            trace.lock().unwrap().push("early");
        });

        // Same phase as `spin`, higher priority number => runs after it.
        let trace = Arc::clone(&self.trace);
        schedule.add_system("late_system", SystemPhase::PreRender, 10, move |_| {
            trace.lock().unwrap().push("late");
        });
    }

    fn register_script_api(&self, api: &mut ScriptApiRegistry) {
        api.add_function(
            "spin_set_speed",
            "test-package",
            "Set an entity's spin speed",
            vec![],
            ScriptValueType::Nil,
        );
    }
}

fn host_with_test_package() -> (PackageHost, Arc<AtomicUsize>, Trace, ComponentRegistry) {
    let ticks = Arc::new(AtomicUsize::new(0));
    let trace: Trace = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut host = PackageHost::new();
    host.add_package(Box::new(TestPackage {
        ticks: Arc::clone(&ticks),
        trace: Arc::clone(&trace),
    }));
    let mut registry = ComponentRegistry::new();
    host.initialize(&mut registry);
    (host, ticks, trace, registry)
}

#[test]
fn a_registered_system_runs_and_mutates_the_world() {
    let (mut host, ticks, _trace, _reg) = host_with_test_package();
    let mut world = World::new();
    let e = world.spawn((Transform::default(), Spin { speed: 2.0 }));

    host.run_phase(SystemPhase::PreRender, &mut world, 0.5);

    assert_eq!(ticks.load(Ordering::SeqCst), 1, "system did not run");
    // 2.0 rad/s * 0.5s — the system received the real dt, not a default.
    assert!((world.get::<&Transform>(e).unwrap().rotation - 1.0).abs() < 1e-6);
}

#[test]
fn only_the_requested_phase_runs() {
    // A runtime calls `run_phase` at each boundary in its frame; a system must
    // not fire on every one of those calls.
    let (mut host, ticks, _trace, _reg) = host_with_test_package();
    let mut world = World::new();

    host.run_phase(SystemPhase::Physics, &mut world, 0.016);
    host.run_phase(SystemPhase::Animation, &mut world, 0.016);
    assert_eq!(ticks.load(Ordering::SeqCst), 0, "ran in the wrong phase");

    host.run_phase(SystemPhase::PreRender, &mut world, 0.016);
    assert_eq!(ticks.load(Ordering::SeqCst), 1);
}

#[test]
fn phase_then_priority_decides_order() {
    let (mut host, _ticks, trace, _reg) = host_with_test_package();
    let mut world = World::new();

    // Called in frame order, as a runtime does.
    host.run_phase(SystemPhase::PreUpdate, &mut world, 0.016);
    host.run_phase(SystemPhase::PreRender, &mut world, 0.016);

    assert_eq!(
        *trace.lock().unwrap(),
        vec!["early", "spin", "late"],
        "systems ran out of order"
    );
}

#[test]
fn package_components_land_in_the_runtime_registry() {
    // The registry passed to `initialize` is the one the scene loader
    // consults. If a package registered into a private copy, its components
    // would deserialize as unknown keys and be silently dropped.
    let (_host, _ticks, _trace, registry) = host_with_test_package();
    assert!(
        registry.get("spin").is_some(),
        "package component missing from the loader's registry"
    );
}

#[test]
fn a_system_that_wants_a_missing_resource_is_a_no_op() {
    // This is what lets a runtime register the built-in physics package while
    // still driving physics itself: the package's system looks up
    // `PhysicsWorld`, does not find it, and does nothing — instead of
    // double-stepping the simulation or panicking.
    struct NeedsResource;
    impl JEnginePackage for NeedsResource {
        fn metadata(&self) -> PackageMetadata {
            PackageMetadata {
                name: "needs-resource".into(),
                version: "0.1.0".into(),
                description: String::new(),
                author: String::new(),
                engine_version: None,
                dependencies: vec![],
            }
        }
        fn register_components(&self, _: &mut ComponentRegistry) {}
        fn register_script_api(&self, _: &mut ScriptApiRegistry) {}
        fn register_systems(&self, schedule: &mut SystemSchedule) {
            schedule.add_system("wants_u64", SystemPhase::Physics, 0, |ctx| {
                let (world, _, resources) = ctx.split();
                if let Some(v) = resources.get::<u64>() {
                    // Only runs when the runtime shared the resource.
                    world.spawn((Transform::default(),));
                    let _ = v;
                }
            });
        }
    }

    let mut host = PackageHost::new();
    host.add_package(Box::new(NeedsResource));
    host.initialize(&mut ComponentRegistry::new());
    let mut world = World::new();

    host.run_phase(SystemPhase::Physics, &mut world, 0.016);
    assert_eq!(world.len(), 0, "ran without its resource");

    host.insert_resource(7u64);
    host.run_phase(SystemPhase::Physics, &mut world, 0.016);
    assert_eq!(world.len(), 1, "did not run once the resource was shared");
}

#[test]
fn a_system_can_hand_results_back_through_resources() {
    // How a package returns something to the runtime — the built-in physics
    // package publishes contact events this way.
    struct Producer;
    impl JEnginePackage for Producer {
        fn metadata(&self) -> PackageMetadata {
            PackageMetadata {
                name: "producer".into(),
                version: "0.1.0".into(),
                description: String::new(),
                author: String::new(),
                engine_version: None,
                dependencies: vec![],
            }
        }
        fn register_components(&self, _: &mut ComponentRegistry) {}
        fn register_script_api(&self, _: &mut ScriptApiRegistry) {}
        fn register_systems(&self, schedule: &mut SystemSchedule) {
            schedule.add_system("produce", SystemPhase::PostPhysics, 0, |ctx| {
                let (_, dt, resources) = ctx.split();
                resources.insert(dt);
            });
        }
    }

    let mut host = PackageHost::new();
    host.add_package(Box::new(Producer));
    host.initialize(&mut ComponentRegistry::new());
    let mut world = World::new();

    host.run_phase(SystemPhase::PostPhysics, &mut world, 0.25);
    assert_eq!(host.resource::<f32>().copied(), Some(0.25));
}

#[test]
fn initializing_twice_does_not_double_register() {
    // A second `initialize` would register every system again, so each would
    // run twice per frame — a silent double-step.
    let (mut host, ticks, _trace, mut reg) = host_with_test_package();
    host.initialize(&mut reg);

    let mut world = World::new();
    host.run_phase(SystemPhase::PreRender, &mut world, 0.016);
    assert_eq!(ticks.load(Ordering::SeqCst), 1, "system registered twice");
}

#[test]
fn metadata_and_system_listing_are_introspectable() {
    // "Is my package's system actually registered?" had no answer before.
    let (host, _ticks, _trace, _reg) = host_with_test_package();

    let names: Vec<String> = host.packages().into_iter().map(|m| m.name).collect();
    assert_eq!(names, vec!["test-package"]);

    let systems: Vec<&str> = host.systems().into_iter().map(|(n, _, _)| n).collect();
    assert!(systems.contains(&"spin_system"), "got {systems:?}");
    assert!(
        host.script_api().function_names().contains(&"spin_set_speed"),
        "package script API missing"
    );
}

#[test]
fn a_duplicate_package_is_ignored_rather_than_registered_twice() {
    // Now that a project can declare a package the runtime also builds in —
    // the editor viewport ships parallax, particles and physics — a duplicate
    // is easy to hit, and registering it twice would run every one of its
    // systems twice per frame.
    let ticks = Arc::new(AtomicUsize::new(0));
    let trace: Trace = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut host = PackageHost::new();
    for _ in 0..2 {
        host.add_package(Box::new(TestPackage {
            ticks: Arc::clone(&ticks),
            trace: Arc::clone(&trace),
        }));
    }
    host.initialize(&mut ComponentRegistry::new());

    assert_eq!(host.packages().len(), 1, "duplicate package registered");

    let mut world = World::new();
    host.run_phase(SystemPhase::PreRender, &mut world, 0.016);
    assert_eq!(ticks.load(Ordering::SeqCst), 1, "system ran twice");
}
