//! `set_sprite_region` on a sprite that came with a region of its own (a
//! prefab's) sticks.
//!
//! Every runtime wrote the new region's `uv_rect` but left the sprite's own
//! `region` pending, so when regions were resolved the prefab's region
//! overwrote the script's choice. A spawned card set to a skill frame drew
//! the prefab's attack frame (the arcade's deckbuilder shop).
#![cfg(feature = "native")]

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::SpriteComponent;
use jengine_core::ecs::systems::{apply_script_commands, resolve_sprite_regions};
use jengine_core::renderer::atlas::AtlasData;
use jengine_core::scripting::api::ScriptCommand;

const NATIVE: &str = include_str!("../src/ecs/systems.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/script_host.rs");
/// Both browser hosts' `ssr` arm, since Oct 2026 (debt #2).
const DISPATCH: &str = include_str!("../src/scripting/bridge/adapters/js/host_dispatch.rs");

#[test]
fn the_scripts_region_survives_region_resolution() {
    let atlas: AtlasData = serde_json::from_value(serde_json::json!({
        "width": 64, "height": 32,
        "regions": [
            { "name": "attack", "x": 0, "y": 0, "width": 32, "height": 32 },
            { "name": "skill", "x": 32, "y": 0, "width": 32, "height": 32 }
        ]
    }))
    .unwrap();
    let mut assets = AssetManager::new();
    assets.register_atlas("sheet.png".into(), atlas);

    let mut world = World::new();
    let card = world.spawn((SpriteComponent { texture_id: "sheet.png".into(), region: Some("attack".into()), ..Default::default() },));
    apply_script_commands(
        &mut world,
        &[ScriptCommand::SetSpriteRegion { entity_id: card.id() as u64, atlas_id: "sheet.png".into(), region_name: "skill".into() }],
        None,
        Some(&mut assets),
    );
    resolve_sprite_regions(&mut world, &assets);
    let s = world.get::<&SpriteComponent>(card).unwrap();
    assert_eq!(s.region.as_deref(), Some("skill"));
    assert_eq!(s.uv_rect.map(|uv| uv[0]), Some(0.5), "the skill half of the sheet, not the attack half");
}

#[test]
fn the_browser_hosts_mark_the_region_resolved_too() {
    // A browser host's arm is the shared one when it calls the dispatcher.
    let with_dispatch = |src: &str| {
        if src.contains("handle_host_command(") { format!("{src}
{DISPATCH}") } else { src.to_string() }
    };
    for (name, src) in [("native", NATIVE.to_string()), ("viewport", with_dispatch(VIEWPORT)), ("web", with_dispatch(WEB))] {
        assert!(src.contains("sprite.region_resolved = true;"), "{name}'s set_sprite_region marks the region resolved");
    }
}
