//! The world-only commands the two browser hosts each used to carry an arm
//! for - `set_custom`, `add_tag` / `remove_tag`, `set_material`, `set_tile`,
//! `gui_show` / `gui_hide` - now one arm in `host_commands`, which is what
//! lets a native test drive them with `JsonArgs` (Oct 2026, debt #2).

use hecs::{Entity, World};
use jengine_core::ecs::components::{CustomComponents, GuiComponent, MaterialComponent, Tags};
use jengine_core::scripting::host_commands::{JsonArgs, SHARED_WORLD_TAGS, ScriptWorldHost, handle_world_command};
use jengine_core::tilemap::{Tilemap, TilemapLayer};
use serde_json::json;

struct Host(World);
impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

fn send(host: &mut Host, tag: &str, v: serde_json::Value) {
    assert!(SHARED_WORLD_TAGS.contains(&tag), "{tag} is not listed as shared");
    assert!(handle_world_command(host, tag, &JsonArgs(v)), "{tag} was not handled");
}

#[test]
fn custom_fields_keep_tables() {
    let mut host = Host(World::new());
    let e = host.0.spawn(());
    let id = e.id();
    // The browser hosts stored a table as null: their value conversion read
    // numbers, strings and booleans only. Native has kept tables since Oct 2026.
    send(&mut host, "sc", json!({ "e": id, "cn": "Stats", "fn": "loot", "v": [1, 2, { "gold": 3 }] }));
    send(&mut host, "sc", json!({ "e": id, "cn": "Stats", "fn": "hp", "v": 7 }));
    let bag = host.0.get::<&CustomComponents>(e).expect("the bag is made");
    assert_eq!(bag.0["Stats"]["loot"], json!([1, 2, { "gold": 3 }]));
    assert_eq!(bag.0["Stats"]["hp"], json!(7));
}

#[test]
fn tags_are_added_and_removed() {
    let mut host = Host(World::new());
    let e = host.0.spawn(());
    let id = e.id();
    send(&mut host, "at", json!({ "e": id, "tag": "enemy" }));
    send(&mut host, "at", json!({ "e": id, "tag": "flying" }));
    send(&mut host, "rt", json!({ "e": id, "tag": "enemy" }));
    let tags = host.0.get::<&Tags>(e).expect("the tag set is made");
    assert!(tags.0.contains("flying") && !tags.0.contains("enemy"), "{:?}", tags.0);
}

#[test]
fn a_material_is_made_when_there_is_none() {
    let mut host = Host(World::new());
    let e = host.0.spawn(());
    send(&mut host, "smat", json!({ "e": e.id(), "m": "glow" }));
    assert_eq!(host.0.get::<&MaterialComponent>(e).unwrap().material_id, "glow");
}

#[test]
fn set_tile_writes_and_clears_by_every_spelling() {
    let mut host = Host(World::new());
    let layer = TilemapLayer { name: "ground".into(), tiles: vec![0; 4], width: 2, height: 2, ..Default::default() };
    let e = host.0.spawn((Tilemap { layers: vec![layer], ..Default::default() },));
    let id = e.id();
    let tile = |host: &Host, x, y| host.0.get::<&Tilemap>(e).unwrap().layers[0].get_tile(x, y);
    send(&mut host, "stl", json!({ "e": id, "ln": "ground", "x": 1, "y": 0, "tid": 5 }));
    assert_eq!(tile(&host, 1, 0), Some(5));
    for clear in [json!(null), json!(0), json!(-1)] {
        send(&mut host, "stl", json!({ "e": id, "ln": "ground", "x": 1, "y": 0, "tid": 9 }));
        send(&mut host, "stl", json!({ "e": id, "ln": "ground", "x": 1, "y": 0, "tid": clear }));
        assert_eq!(tile(&host, 1, 0), None, "cleared by {clear}");
    }
}

#[test]
fn gui_documents_show_and_hide_by_id() {
    let mut host = Host(World::new());
    let gui = |id: &str| GuiComponent { gui_id: id.into(), visible: true, ..Default::default() };
    let menu = host.0.spawn((gui("menu"),));
    let hud = host.0.spawn((gui("hud"),));
    let visible = |host: &Host, e| host.0.get::<&GuiComponent>(e).unwrap().visible;
    send(&mut host, "gh", json!({ "gid": "menu" }));
    assert!(!visible(&host, menu) && visible(&host, hud));
    // No id: every document.
    send(&mut host, "gh", json!({}));
    assert!(!visible(&host, hud));
    send(&mut host, "gs", json!({ "gid": "hud" }));
    assert!(visible(&host, hud) && !visible(&host, menu));
}
