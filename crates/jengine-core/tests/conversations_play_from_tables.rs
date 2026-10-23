//! **Dialogue** (Oct 2026; `ecs::dialogue`, `Dialogue.*`): a conversation is
//! a data table under `dialogue/`, played by a `dialogue` component into the
//! scene's own text entities. Lines type themselves out, `advance` finishes a
//! line before it moves on, choices filter by condition and set variables a
//! later node branches on, and the end is called back - all driven through
//! the script command's own path (`host_commands`' `dlg` arm). Found by the
//! showcase's Village Chatter.

use hecs::{Entity, World};
use jengine_core::data::{self, DataRegistry};
use jengine_core::ecs::components::{CustomComponents, Name, TextComponent};
use jengine_core::ecs::dialogue::Dialogue;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};

struct Host(World);
impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

const TABLE: &str = r#"{ "dialogue/baker": { "schema": "Dialogue", "rows": [
  { "id": "start", "if": "met", "else": "hello", "next": "again" },
  { "id": "hello", "speaker": "Bess", "portrait": "faces/bess.png", "text": "Fresh bread! You must be new.", "set": { "met": true }, "next": "offer" },
  { "id": "again", "speaker": "Bess", "text": "Back again? {gold} coins left.", "next": "offer" },
  { "id": "offer", "speaker": "Bess", "text": "A loaf is three coins.", "choices": [
      { "text": "Buy a loaf", "if": "gold >= 3", "set": { "gold": "-3", "loaves": "+1" }, "next": "thanks" },
      { "text": "Beg", "if": "gold < 3", "next": "pity" },
      { "text": "Goodbye", "next": "" } ] },
  { "id": "thanks", "speaker": "Bess", "text": "Enjoy it!", "event": "sold" },
  { "id": "pity", "speaker": "Bess", "text": "Here, a crust." }
] } }"#;

fn text(world: &World, name: &str) -> String {
    let e = world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e).unwrap();
    world.get::<&TextComponent>(e).unwrap().text.clone()
}

fn field(world: &World, e: Entity, comp: &str, key: &str) -> serde_json::Value {
    world.get::<&CustomComponents>(e).unwrap().0[comp][key].clone()
}

fn label(name: &str) -> (Name, TextComponent) {
    (Name(name.into()), TextComponent { reveal_speed: 0.0, ..Default::default() })
}

#[test]
fn a_conversation_plays_branches_and_remembers() {
    data::install_global(std::sync::Arc::new(DataRegistry::from_tables_json(TABLE).unwrap()));
    let mut world = World::new();
    world.spawn(label("Line"));
    world.spawn(label("Who"));
    world.spawn(label("C1"));
    world.spawn(label("C2"));
    let mut vars = std::collections::HashMap::new();
    vars.insert("DialogueVars".to_string(), serde_json::json!({ "gold": 4 }));
    let village = world.spawn((Name("Village".into()), CustomComponents(vars)));
    let baker = world.spawn((
        Name("Baker".into()),
        Dialogue {
            table: "dialogue/baker".into(),
            vars: "Village".into(),
            text: "Line".into(),
            speaker: "Who".into(),
            choices: vec!["C1".into(), "C2".into()],
            reveal_speed: 20.0,
            ..Default::default()
        },
    ));
    let tick = |host: &mut Host, n: usize| {
        for _ in 0..n {
            jengine_core::ecs::behaviors::tick_motion(&mut host.0, 1.0 / 60.0, None, false);
        }
    };
    // Nothing shows a portrait until its line comes up, so the preload
    // reads them from the table.
    assert!(jengine_core::scene::format::world_texture_ids(&world).contains("faces/bess.png"), "portraits are preloaded");
    let mut host = Host(world);
    let cmd = |host: &mut Host, op: &str, a: &str| {
        let v = serde_json::json!({ "e": baker.id(), "op": op, "a": a });
        assert!(handle_world_command(host, "dlg", &JsonArgs(v)), "the shared arm takes `dlg`");
    };
    jengine_core::ecs::callbacks::take();

    // First meeting: `start` is passed over to `hello`, which sets `met`.
    cmd(&mut host, "start", "");
    assert_eq!(text(&host.0, "Line"), "Fresh bread! You must be new.");
    assert_eq!(text(&host.0, "Who"), "Bess");
    tick(&mut host, 2);
    assert_eq!(field(&host.0, baker, "Dialogue", "typing"), serde_json::json!(true), "it types itself out");
    // The first advance finishes the line; the second goes on.
    cmd(&mut host, "advance", "");
    assert_eq!(field(&host.0, baker, "Dialogue", "typing"), serde_json::json!(false), "advance shows it all");
    assert_eq!(field(&host.0, baker, "Dialogue", "node"), serde_json::json!("hello"));
    cmd(&mut host, "advance", "");
    assert_eq!(text(&host.0, "Line"), "A loaf is three coins.");
    assert_eq!(text(&host.0, "C1"), "", "the choices wait for the line");
    tick(&mut host, 1);
    assert_eq!(text(&host.0, "C1"), "", "still typing");
    cmd(&mut host, "advance", "");
    // Four coins: buying is offered, begging is not.
    assert_eq!(text(&host.0, "C1"), "1. Buy a loaf");
    assert_eq!(text(&host.0, "C2"), "2. Goodbye");
    cmd(&mut host, "advance", "");
    assert_eq!(text(&host.0, "Line"), "A loaf is three coins.", "a line with choices waits for one");
    cmd(&mut host, "choose", "1");
    assert_eq!(text(&host.0, "Line"), "Enjoy it!");
    assert_eq!(field(&host.0, village, "DialogueVars", "gold").as_f64(), Some(1.0), "the choice spent");
    assert_eq!(field(&host.0, village, "DialogueVars", "loaves").as_f64(), Some(1.0));
    assert_eq!(text(&host.0, "C1"), "", "no choices on show");
    cmd(&mut host, "advance", "");
    cmd(&mut host, "advance", "");
    assert_eq!(text(&host.0, "Line"), "", "the end clears the line");
    assert_eq!(field(&host.0, baker, "Dialogue", "playing"), serde_json::json!(false));
    let calls: Vec<&str> = jengine_core::ecs::callbacks::take().iter().map(|c| c.handler).collect();
    for want in ["on_dialogue_line", "on_sold", "on_dialogue_end"] {
        assert!(calls.contains(&want), "{want} called: {calls:?}");
    }

    // Meeting again: remembered, the variable filled in, and with one coin
    // the begging choice instead.
    cmd(&mut host, "start", "");
    assert_eq!(text(&host.0, "Line"), "Back again? 1 coins left.");
    cmd(&mut host, "advance", "");
    cmd(&mut host, "advance", "");
    cmd(&mut host, "advance", "");
    assert_eq!(text(&host.0, "C1"), "1. Beg");
    cmd(&mut host, "choose", "1");
    assert_eq!(text(&host.0, "Line"), "Here, a crust.");
    cmd(&mut host, "stop", "");
    assert_eq!(field(&host.0, baker, "Dialogue", "playing"), serde_json::json!(false));
}
