//! `Platform.*` on the native (Luau) path (`docs/web-platforms.md`).
//!
//! Natively there is no web portal, so the calls must answer exactly as a web
//! build for no portal does: `Platform.name()` is "none", the gameplay calls
//! do nothing, and `Platform.show_ad` still answers - through the
//! `platform_ad_finished` event, not shown - so a game's ad flow is the same
//! code in every runtime. These drive the real `ScriptSystem`.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{Name, ScriptComponent, ScriptLanguage, Tags, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

const SRC: &str = r#"
local asked = false

function init(entity)
    Entity.add_tag(entity, "name:" .. Platform.name())
    Event.on("platform_ad_finished", entity)
    -- Nothing to tell natively; none of these may throw.
    Platform.game_ready()
    Platform.gameplay_start()
    Platform.happy_time()
    Platform.submit_score(100, "high-scores")
    Platform.gameplay_stop()
end

function update(entity, dt)
    if not asked then
        asked = true
        Platform.show_ad("rewarded", "extra-life")
    end
end

function on_platform_ad_finished(entity, data)
    Entity.add_tag(entity, "ad:" .. tostring(data.kind) .. ":" .. tostring(data.shown) .. ":" .. tostring(data.rewarded))
end
"#;

fn script_on_disk(src: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jengine_platform_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("platform.lua");
    std::fs::write(&path, src).unwrap();
    path
}

#[test]
fn platform_calls_answer_natively_as_a_page_without_a_portal() {
    let path = script_on_disk(SRC);
    let mut world = World::new();
    let e = world.spawn((
        Name("Game".to_string()),
        Transform::default(),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_str().unwrap().to_string(),
            cull_distance: None,
        },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..5 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }

    let tags = world.get::<&Tags>(e).expect("the script tagged its entity").0.clone();
    assert!(tags.iter().any(|t| t == "name:none"), "Platform.name() natively: {tags:?}");
    assert!(
        tags.iter().any(|t| t == "ad:rewarded:false:false"),
        "Platform.show_ad must answer through platform_ad_finished, not shown: {tags:?}"
    );
}
