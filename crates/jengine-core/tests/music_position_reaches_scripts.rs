//! `get_music_position()` reads the playhead the runtime publishes.
//!
//! There was no way to ask where the music was: a rhythm game could start a
//! song but had to time its notes by counting frames, which drifts from the
//! audio. Each runtime now publishes the music's position after its audio
//! update (`audio::publish_music_position`); scripts and packages read it.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

const SRC: &str = r#"
function update(entity, dt)
    set_custom(entity, "Probe", "t", get_music_position())
end
"#;

#[test]
fn scripts_and_packages_read_the_published_music_position() {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_music_position_test",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("music.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let e = world.spawn((
        Name("Listener".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    let read = |world: &World| {
        let custom = world.get::<&CustomComponents>(e).unwrap();
        custom.0.get("Probe").and_then(|p| p.get("t")).and_then(|v| v.as_f64()).unwrap_or(f64::NAN)
    };

    jengine_core::audio::publish_music_position(None);
    assert_eq!(jengine_core::audio::music_position(), None);
    for _ in 0..2 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    assert_eq!(read(&world), 0.0, "no music reads as 0");

    jengine_core::audio::publish_music_position(Some(12.5));
    assert_eq!(jengine_core::audio::music_position(), Some(12.5));
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    assert_eq!(read(&world), 12.5, "the script sees the published playhead");
    jengine_core::audio::publish_music_position(None);
}
