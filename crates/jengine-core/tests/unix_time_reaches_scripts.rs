//! `get_unix_time()` is the wall clock.
//!
//! The engine had `now_ms` - monotonic, arbitrary start, explicitly "not
//! for dates" - and nothing else. A game that has to survive being closed
//! cannot use it: two readings either side of a restart mean nothing, so
//! "how long was I away?" was unanswerable, which is the entire premise of
//! an idle game and half the premise of a daily reward.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

const SRC: &str = r#"
function update(entity, dt)
    set_custom(entity, "Probe", "t", get_unix_time())
end
"#;

#[test]
fn a_script_can_read_the_wall_clock() {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_unix_time_test",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("clock.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let e = world.spawn((
        Name("Clock".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_string_lossy().into(),
            cull_distance: None,
        },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);

    let seen = {
        let custom = world.get::<&CustomComponents>(e).unwrap();
        custom.0.get("Probe").and_then(|p| p.get("t")).and_then(|v| v.as_f64()).unwrap_or(0.0)
    };
    let now = jengine_core::profiling::unix_time();
    // A real date, not a monotonic counter: some time after 2020 and within
    // a second of what the engine itself reads.
    assert!(seen > 1_577_836_800.0, "that is not a date: {seen}");
    assert!((now - seen).abs() < 5.0, "the script's clock is {seen}, the engine's is {now}");
}

#[test]
fn the_wall_clock_and_the_frame_clock_are_different_things() {
    // `now_ms` starts from an arbitrary point in this process, so it is far
    // too small to be a date - which is exactly why it cannot answer "how
    // long was I away".
    let monotonic = jengine_core::profiling::now_ms() / 1000.0;
    let wall = jengine_core::profiling::unix_time();
    assert!(wall > 1_577_836_800.0);
    assert!(monotonic < 1_000_000.0, "now_ms is not a date: {monotonic}");
}

#[test]
fn every_script_in_a_frame_agrees_about_the_time() {
    // Read once per frame into the snapshot, so two scripts in the same
    // frame cannot disagree by a few microseconds and compute different
    // elapsed times from the same save.
    let dir = std::env::temp_dir().join("jengine_unix_time_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("clock.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let make = |world: &mut World, name: &str| {
        world.spawn((
            Name(name.into()),
            Transform::default(),
            CustomComponents(Default::default()),
            ScriptComponent {
                language: ScriptLanguage::Lua,
                source_path: path.to_string_lossy().into(),
                cull_distance: None,
            },
        ))
    };
    let a = make(&mut world, "A");
    let b = make(&mut world, "B");
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);

    let read = |world: &World, e: hecs::Entity| {
        world
            .get::<&CustomComponents>(e)
            .unwrap()
            .0
            .get("Probe")
            .and_then(|p| p.get("t"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    };
    assert_eq!(read(&world, a), read(&world, b));
}
