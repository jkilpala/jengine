//! Patrol routes must actually arrive in every runtime.
//!
//! # The bug
//!
//! `Path2D`, `Path2DLibrary`, `PatrolPathRef` and `update_patrol_paths` had all
//! existed since the component was added, with seven tests covering Loop, Once
//! and PingPong traversal. **Nothing ever filled the library.** Native passed a
//! field that was initialised to `None` and never assigned; the editor viewport
//! and the web runtime passed a literal `None`. So `PatrolPathRef` moved
//! nothing, anywhere, and there was no way to author a route at all — the
//! feature was complete except for its input (BUGS #7).
//!
//! That is a distinct failure from the parity bugs elsewhere in this repo. It
//! is not a runtime disagreeing with another; every runtime agreed, and all of
//! them did nothing. A system whose resource is optional and never provided
//! compiles, runs, passes its unit tests, and is dead.
//!
//! So this checks the wiring rather than the algorithm — the algorithm was
//! never the problem.

use jengine_core::scene::format::read_scene_metadata;

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");

/// No runtime may call the patrol system with a hardcoded `None`.
///
/// This is the exact line that made the feature dead in three places, and it
/// is invisible: it compiles, and the system is documented as tolerating
/// `None`, so it reads like a deliberate choice rather than an unfinished one.
#[test]
fn no_runtime_hardcodes_an_empty_patrol_library() {
    for (name, src) in [("native", NATIVE), ("viewport", VIEWPORT), ("web", WEB)] {
        // The patrol system runs inside the shared motion step since Sep 2026;
        // each host hands that step the library.
        for (idx, _) in src.match_indices("tick_motion(") {
            let end = src[idx..].find(';').map_or(src.len(), |n| idx + n);
            let call = &src[idx..end];
            assert!(
                !call.contains("None"),
                "{name} calls the patrol system with a hardcoded `None`:\n  {}\n\
                 The system tolerates `None`, so this compiles and runs and does \
                 nothing — which is how `PatrolPathRef` came to move nothing in \
                 any runtime (BUGS #7). Pass the scene's library.",
                call.trim()
            );
        }
    }
}

/// And each runtime must build one, or the check above passes for a runtime
/// that stopped calling the system at all.
#[test]
fn every_runtime_builds_a_patrol_library_from_the_scene() {
    for (name, src) in [("native", NATIVE), ("viewport", VIEWPORT), ("web", WEB)] {
        assert!(
            src.contains("Path2DLibrary::new()"),
            "{name} never builds a `Path2DLibrary`, so patrol routes declared by \
             a scene cannot reach it"
        );
    }
}

#[test]
fn a_scene_declares_its_patrol_routes() {
    let json = r#"{
        "name": "Test",
        "paths": {
            "guard": { "points": [[0.0, 0.0], [100.0, 0.0]], "looped": true },
            "lift":  { "points": [[0.0, 0.0], [0.0, 80.0]] }
        },
        "entities": []
    }"#;
    let meta = read_scene_metadata(json).expect("metadata");
    assert_eq!(meta.paths.len(), 2);
    let guard = meta.paths.get("guard").expect("guard route");
    assert_eq!(guard.len(), 2);
    assert!(guard.looped);
    assert!(!meta.paths["lift"].looped, "`looped` defaults to false");
}

/// A scene with no routes is the common case and must stay cheap and quiet.
#[test]
fn a_scene_without_paths_declares_none() {
    let meta = read_scene_metadata(r#"{"name":"Test","entities":[]}"#).expect("metadata");
    assert!(meta.paths.is_empty());
}

/// One malformed route must not take the scene down with it.
///
/// The patrol system already skips an id it cannot resolve, so the worst case
/// is one entity standing still — much better than a level that will not load.
#[test]
fn a_malformed_route_is_skipped_rather_than_failing_the_scene() {
    let json = r#"{
        "name": "Test",
        "paths": {
            "good": { "points": [[1.0, 2.0]] },
            "bad":  { "points": "not an array" }
        },
        "entities": []
    }"#;
    let meta = read_scene_metadata(json).expect("a bad route must not fail the parse");
    assert!(meta.paths.contains_key("good"));
    assert!(!meta.paths.contains_key("bad"));
}

/// End to end, through the parts a runtime uses: scene JSON in, movement out.
#[test]
fn a_declared_route_walks_an_entity() {
    use jengine_core::ecs::behaviors::update_patrol_paths;
    use jengine_core::ecs::components::{PatrolPathRef, Transform};
    use jengine_core::ecs::path::Path2DLibrary;

    let json = r#"{
        "name": "Test",
        "paths": { "route": { "points": [[0.0, 0.0], [100.0, 0.0]] } },
        "entities": []
    }"#;
    let meta = read_scene_metadata(json).expect("metadata");
    let mut lib = Path2DLibrary::new();
    for (id, path) in meta.paths {
        lib.insert(id, path);
    }

    let mut world = hecs::World::new();
    let e = world.spawn((
        Transform::default(),
        PatrolPathRef {
            path_id: "route".to_string(),
            speed: 50.0,
            index: 1,
            ..Default::default()
        },
    ));
    update_patrol_paths(&mut world, Some(&lib), 1.0);
    let moved = world.get::<&Transform>(e).unwrap().position;
    assert!(
        (moved.x - 50.0).abs() < 1e-4 && moved.y.abs() < 1e-4,
        "expected 50 units toward the second waypoint, got {moved:?}"
    );
}

/// Scene 64 really walks, through the same steps a runtime takes.
///
/// The scene is the only place a human checks this, so it is worth knowing it
/// is wired before opening it: an entity that stands still there looks
/// identical to the bug this closed.
#[test]
fn scene_64_walkers_move_along_their_routes() {
    use jengine_core::ecs::behaviors::update_patrol_paths;
    use jengine_core::ecs::components::{Name, Transform};
    use jengine_core::ecs::path::Path2DLibrary;
    use jengine_core::package::component_registry::ComponentRegistry;
    use jengine_core::package::core_registration::register_core_components;
    use jengine_core::scene::load_scene_auto;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../projects/engine-test-lab/assets/scenes/64-patrol-paths.scene");
    let text = std::fs::read_to_string(&path).expect("read scene 64");

    let meta = read_scene_metadata(&text).expect("metadata");
    assert_eq!(meta.paths.len(), 3, "scene 64 should declare three routes");
    let mut lib = Path2DLibrary::new();
    for (id, route) in meta.paths {
        assert!(route.len() >= 2, "a route needs at least two waypoints to walk");
        lib.insert(id, route);
    }

    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let mut world = hecs::World::new();
    load_scene_auto(&text, &mut world, &registry).expect("load scene 64");

    let start: Vec<(String, glam::Vec2)> = world
        .query::<(&Name, &Transform, &jengine_core::ecs::components::PatrolPathRef)>()
        .iter()
        .map(|(_, (n, t, _))| (n.0.clone(), t.position))
        .collect();
    assert_eq!(start.len(), 3, "scene 64 should have three patrol walkers");

    for _ in 0..30 {
        update_patrol_paths(&mut world, Some(&lib), 1.0 / 30.0);
    }

    for (name, from) in start {
        let now = world
            .query::<(&Name, &Transform)>()
            .iter()
            .find(|(_, (n, _))| n.0 == name)
            .map(|(_, (_, t))| t.position)
            .expect("walker still exists");
        assert!(
            now.distance(from) > 1.0,
            "`{name}` has not moved after a second of patrolling — its route id              probably does not match any key in the scene's `paths` block"
        );
    }
}
