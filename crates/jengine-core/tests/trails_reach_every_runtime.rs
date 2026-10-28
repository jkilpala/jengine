//! **Trails record after the hierarchy resolves, in every runtime.**
//!
//! Recording first lived in `behaviors::tick_motion`, reading each entity's
//! `WorldTransform` - which is only as fresh as the last resolve, and the
//! hosts do not run their pieces in one order: native runs scripts *before*
//! motion, so a script that turned a sword's pivot to the start of a swing,
//! cleared its trail and set it emitting had the trail record, that same
//! frame, the tip where the *previous* frame left it - at rest. The
//! showcase's Trails drew every slash with a fold back to the resting tip.
//! Recording right after the resolve reads what the frame actually ends
//! with, whatever ran before it.
#![cfg(feature = "trails")]

mod support;

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Parent, Trail, Transform};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::ecs::trails::tick_trails;

static NATIVE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs")));
static VIEWPORT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs")));
static WEB: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs")));

#[test]
fn every_runtime_records_trails_right_after_resolving_the_hierarchy() {
    for (name, src) in [("native", NATIVE.as_str()), ("viewport", VIEWPORT.as_str()), ("web", WEB.as_str())] {
        let resolve = src
            .find("hierarchy::resolve_world_transforms(")
            .unwrap_or_else(|| panic!("{name} never resolves the hierarchy"));
        let record = src
            .find("trails::tick_trails(")
            .unwrap_or_else(|| panic!("{name} never records trails - every trail is invisible there"));
        assert!(record > resolve, "{name} records trails before the hierarchy resolves");
        let between = src[resolve..record].lines().count();
        assert!(between <= 10, "{name}: {between} lines between the resolve and the recording");
    }
    let motion = include_str!("../src/ecs/behaviors.rs");
    assert!(!motion.contains("tick_trails("), "trails are recorded in the motion step again");
}

#[test]
fn a_swing_started_this_frame_records_where_it_starts() {
    // The script's frame: turn the pivot to the start, clear, emit - then the
    // host resolves and records.
    let mut world = World::new();
    let pivot = world.spawn((Transform::default(),));
    let tip = world.spawn((
        Transform { position: Vec2::new(100.0, 0.0), ..Default::default() },
        Parent(pivot),
        Trail { emitting: false, ..Trail::default() },
    ));
    resolve_world_transforms(&mut world);
    tick_trails(&mut world, 1.0 / 60.0);

    world.get::<&mut Transform>(pivot).unwrap().rotation = std::f32::consts::FRAC_PI_2;
    {
        let mut t = world.get::<&mut Trail>(tip).unwrap();
        t.points.clear();
        t.emitting = true;
    }
    resolve_world_transforms(&mut world);
    tick_trails(&mut world, 1.0 / 60.0);

    let t = world.get::<&Trail>(tip).unwrap();
    assert_eq!(t.points.len(), 1);
    let p = t.points[0].position;
    assert!(p.distance(Vec2::new(0.0, 100.0)) < 1e-3, "recorded {p:?}, not the turned tip");
}
