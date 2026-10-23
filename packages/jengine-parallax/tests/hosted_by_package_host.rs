//! BUGS #13 — the parallax package works end to end through a `PackageHost`.
//!
//! Parallax was the built-in package with **no hardcoded path in any runtime**,
//! so it was the one that proved the package system was not running: physics,
//! audio and particles all appeared to work because each runtime calls them
//! directly, while `parallax_scroll` — registered only through
//! `register_systems` — never executed anywhere.
//!
//! This drives the real package, not a stand-in: if the host stops calling
//! `PreRender`, or the component stops registering, these fail.

use hecs::World;
use jengine_core::ecs::components::{Camera2DComponent, Transform};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::host::PackageHost;
use jengine_core::package::system_schedule::SystemPhase;
use jengine_parallax::{ParallaxLayer, ParallaxPackage};

fn hosted() -> (PackageHost, ComponentRegistry) {
    // Core components first, then the package registers into the *same*
    // registry — exactly the order a runtime uses. A registry holding only the
    // package would leave `transform` an unknown key, and every entity in a v2
    // scene would load without one.
    let mut registry = ComponentRegistry::new();
    jengine_core::package::core_registration::register_core_components(&mut registry);
    let mut host = PackageHost::new();
    host.add_package(Box::new(ParallaxPackage));
    host.initialize(&mut registry);
    (host, registry)
}

fn camera(world: &mut World, x: f32, y: f32) -> hecs::Entity {
    let mut t = Transform::default();
    t.position.x = x;
    t.position.y = y;
    world.spawn((
        t,
        Camera2DComponent {
            is_active: true,
            ..Default::default()
        },
    ))
}

#[test]
fn the_layer_moves_when_the_camera_does() {
    let (mut host, _reg) = hosted();
    let mut world = World::new();
    camera(&mut world, 0.0, 0.0);

    // Half-speed layer: the classic distant-background setup.
    // Built through `Default` and mutated: the auto-scroll accumulators are
    // private, so an outside crate cannot use a struct literal — which is
    // itself the third-party experience worth exercising here.
    let mut cfg = ParallaxLayer::default();
    cfg.scroll_factor_x = 0.5;
    let layer = world.spawn((Transform::default(), cfg));

    host.run_phase(SystemPhase::PreRender, &mut world, 1.0 / 60.0);
    let at_origin = world.get::<&Transform>(layer).unwrap().position.x;

    // Move the camera and re-run: the layer must follow at its own rate.
    for (_, (t, cam)) in world.query::<(&mut Transform, &Camera2DComponent)>().iter() {
        if cam.is_active {
            t.position.x = 100.0;
        }
    }
    host.run_phase(SystemPhase::PreRender, &mut world, 1.0 / 60.0);
    let moved = world.get::<&Transform>(layer).unwrap().position.x;

    assert_ne!(
        moved, at_origin,
        "parallax layer did not react to the camera — the system never ran"
    );
}

#[test]
fn auto_scroll_advances_without_the_camera_moving() {
    let (mut host, _reg) = hosted();
    let mut world = World::new();
    camera(&mut world, 0.0, 0.0);
    let mut cfg = ParallaxLayer::default();
    cfg.auto_scroll_x = 60.0;
    let layer = world.spawn((Transform::default(), cfg));

    let before = world.get::<&Transform>(layer).unwrap().position.x;
    for _ in 0..10 {
        host.run_phase(SystemPhase::PreRender, &mut world, 1.0 / 60.0);
    }
    let after = world.get::<&Transform>(layer).unwrap().position.x;

    // Also proves the system receives the real `dt` — with dt of 0 the
    // accumulator would never advance.
    assert_ne!(after, before, "auto-scroll did not advance");
}

#[test]
fn the_layer_component_is_registered_for_scene_loading() {
    // v2 scenes deserialize through the `ComponentRegistry`, which is the only
    // way a package component can be authored — `ComponentBundle` in
    // `scene/format.rs` is core-only and has no parallax field. Without this
    // registration a `parallax_layer` key would be dropped as unknown.
    let (_host, registry) = hosted();
    assert!(
        registry.get("parallax_layer").is_some(),
        "parallax_layer missing from the registry the scene loader uses"
    );
}

#[test]
fn nothing_runs_in_other_phases() {
    // The runtime calls `run_phase` at every boundary in its frame. The
    // parallax system registers in PreRender and must not fire on the rest,
    // or the layer would advance several times per frame.
    let (mut host, _reg) = hosted();
    let mut world = World::new();
    camera(&mut world, 0.0, 0.0);
    let mut cfg = ParallaxLayer::default();
    cfg.auto_scroll_x = 60.0;
    let layer = world.spawn((Transform::default(), cfg));

    let before = world.get::<&Transform>(layer).unwrap().position.x;
    for phase in [
        SystemPhase::PreUpdate,
        SystemPhase::ScriptExec,
        SystemPhase::Physics,
        SystemPhase::PostPhysics,
        SystemPhase::Animation,
    ] {
        host.run_phase(phase, &mut world, 1.0 / 60.0);
    }
    assert_eq!(
        world.get::<&Transform>(layer).unwrap().position.x,
        before,
        "parallax ran outside PreRender"
    );
}

#[test]
fn test_lab_scene_59_loads_its_parallax_layers() {
    // End-to-end against the real asset. The test-lab's own scene loader test
    // lives in jengine-core, which cannot depend on this package, so it builds
    // a core-only registry — under which `parallax_layer` is just an unknown
    // key that lands in `CustomComponents` and is silently dropped. That test
    // would pass on a scene whose parallax never loads, so the assertion has
    // to be made here.
    use jengine_core::scene::load_scene_auto;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../projects/engine-test-lab/assets/scenes/59-packages.scene");
    let json = std::fs::read_to_string(&path).expect("scene 59 exists");

    let (mut host, registry) = hosted();
    let mut world = World::new();
    load_scene_auto(&json, &mut world, &registry).expect("scene 59 parses");

    let layers = world.query::<&ParallaxLayer>().iter().count();
    assert_eq!(layers, 4, "expected 4 parallax layers, got {layers}");

    // And it actually moves once hosted — the scene is not just well-formed.
    let ids: Vec<hecs::Entity> = world
        .query::<&ParallaxLayer>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    let drifter = ids
        .into_iter()
        .find(|e| world.get::<&ParallaxLayer>(*e).unwrap().auto_scroll_x != 0.0)
        .expect("scene 59 has an auto-scrolling band");
    let before = world.get::<&Transform>(drifter).unwrap().position.x;
    for _ in 0..30 {
        host.run_phase(SystemPhase::PreRender, &mut world, 1.0 / 60.0);
    }
    assert_ne!(
        world.get::<&Transform>(drifter).unwrap().position.x,
        before,
        "scene 59's drifting band did not move"
    );
}

#[test]
fn two_repeating_copies_always_cover_the_view() {
    // A strip 1400 wide in two copies, wrapping by 2800, behind a camera that
    // travels 20,000 px: at every step one copy or the other must cover each
    // edge of a 1280-wide view. Before `repeat_x` did anything, both copies
    // were left behind after a screen or two.
    use jengine_core::ecs::components::SpriteComponent;
    let (mut host, _reg) = hosted();
    let mut world = World::new();
    let cam = camera(&mut world, 0.0, 0.0);
    let strip = |offset: f32| {
        let mut cfg = ParallaxLayer::default();
        cfg.scroll_factor_x = 0.3;
        cfg.repeat_x = true;
        cfg.repeat_width = 2800.0;
        cfg.offset_x = offset;
        cfg.z_order_override = Some(-50);
        let sprite = SpriteComponent { size: Some([1400.0, 700.0]), ..SpriteComponent::default() };
        (Transform::default(), sprite, cfg)
    };
    let a = world.spawn(strip(0.0));
    let b = world.spawn(strip(1400.0));
    let mut x = 0.0;
    while x < 20_000.0 {
        world.get::<&mut Transform>(cam).unwrap().position.x = x;
        host.run_phase(SystemPhase::PreRender, &mut world, 1.0 / 60.0);
        let covers = |edge: f32| {
            [a, b].iter().any(|&e| (world.get::<&Transform>(e).unwrap().position.x - edge).abs() <= 700.0)
        };
        assert!(covers(x - 640.0) && covers(x + 640.0), "a gap in the sky with the camera at {x}");
        x += 37.0;
    }
    assert_eq!(world.get::<&SpriteComponent>(a).unwrap().z_order, -50, "z_order_override applies");
}
