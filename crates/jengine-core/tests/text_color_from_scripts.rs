//! Scripts can colour world text: `set_text_color`, and `tween_color` on an
//! entity whose only drawable is text.
//!
//! Text had no colour setter, and a colour tween started from the sprite's
//! colour and wrote the sprite's - so on a text entity it never started. A
//! judgement that pops and fades ("PERFECT", "MISS") took one entity per
//! colour and could not fade at all.

#![cfg(feature = "native")]

use hecs::{Entity, World};
use jengine_core::ecs::components::{Name, ScriptComponent, ScriptLanguage, TextComponent, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
use serde_json::json;

const SRC: &str = r#"
local frames = 0
function update(entity, dt)
    frames = frames + 1
    if frames == 1 then
        set_text_color(find_by_name("Judgement"), 0.2, 1.0, 0.4)
        tween_color(find_by_name("Fading"), 1, 1, 1, 0, 0.1, "linear")
    end
end
"#;

fn text(s: &str) -> TextComponent {
    TextComponent { text: s.into(), color: [1.0, 1.0, 1.0, 1.0], ..Default::default() }
}

#[test]
fn set_text_color_and_tween_color_reach_text_natively() {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_text_color_test",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("text_color.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let judgement = world.spawn((Name("Judgement".into()), Transform::default(), text("PERFECT")));
    let fading = world.spawn((Name("Fading".into()), Transform::default(), text("GOOD")));
    world.spawn((
        Name("Driver".into()),
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..30 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    assert_eq!(world.get::<&TextComponent>(judgement).unwrap().color, [0.2, 1.0, 0.4, 1.0]);
    let faded = world.get::<&TextComponent>(fading).unwrap().color;
    assert!(faded[3] < 0.01, "the tween faded the text out ({faded:?})");
}

struct Host(World);

impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

#[test]
fn the_browser_hosts_set_text_color_through_the_shared_arm() {
    let mut host = Host(World::new());
    let e = host.0.spawn((Transform::default(), text("MISS")));
    let args = JsonArgs::new(json!({ "e": e.id(), "r": 1.0, "g": 0.2, "b": 0.2 }));
    assert!(handle_world_command(&mut host, "stc", &args));
    assert_eq!(host.0.get::<&TextComponent>(e).unwrap().color, [1.0, 0.2, 0.2, 1.0], "alpha defaults to 1");
}
