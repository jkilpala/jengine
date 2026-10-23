//! A documented event handler gets its payload, in every runtime.
//!
//! `on_<event>(entity, data)` is the spelling the docs give and native has
//! always called, with `data` a table. The browser hosts learned to call
//! `on_<event>` at all only with BUGS #150 - and then called it with the
//! general `on_event` argument list, `(self, name, sender, data_text)`. So
//! in the editor and in web builds every documented handler received the
//! event's **name** as its `data`: `data.button` on a `ui_click` was nil,
//! and a management sim built of entity UI - every button of which is a
//! `ui_click` - sat on its menu
//! and did nothing. Test Lab scenes 40, 48, 67 and 100 have handlers of the
//! same shape. Native tests played that game through its UI and passed,
//! because native was right.
//!
//! The argument rule lives once in core (`event_handler_args`, over
//! `event_handler_data`, which is testable here); these check both hosts
//! build their arguments through it rather than by hand.

use jengine_core::scripting::host_commands::event_handler_data;
use serde_json::json;

#[test]
fn a_handler_gets_the_payload_as_a_value_not_as_text() {
    assert_eq!(event_handler_data(Some(r#"{"button":42}"#)), json!({ "button": 42 }));
    assert_eq!(event_handler_data(Some("7")), json!(7));
    assert_eq!(event_handler_data(Some("[1,2]")), json!([1, 2]));
    // Text that is not JSON is the string it is; no payload is nil.
    assert_eq!(event_handler_data(Some("hello there")), json!("hello there"));
    assert_eq!(event_handler_data(None), serde_json::Value::Null);
}

fn dispatch_body(rel: &str) -> String {
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
        .replace("\r\n", "\n");
    let start = src.find("fn dispatch_event_to_listeners(").unwrap_or_else(|| panic!("{rel}: no dispatch_event_to_listeners"));
    let end = start + src[start..].find("\n}\n").expect("end of the function");
    src[start..end].to_string()
}

#[test]
fn both_browser_hosts_build_handler_arguments_through_the_shared_rule() {
    // Both hosts' listener dispatch is `adapters::js::scripts::dispatch_event`
    // since Oct 2026; each host's own function only calls it.
    let shared = include_str!("../src/scripting/bridge/adapters/js/scripts.rs").replace("
", "
");
    let at = shared.find("pub fn dispatch_event(").expect("scripts::dispatch_event");
    let body = &shared[at..at + shared[at..].find("
}
").expect("end of the function")];
    assert!(body.contains("event_handler_args("), "scripts::dispatch_event no longer uses event_handler_args");
    assert!(
        !body.contains("args.push(&JsValue::from_str(name))"),
        "scripts::dispatch_event builds the on_event argument list by hand again - for every handler"
    );
    for rel in ["crates/jengine-editor-viewport/src/lib.rs", "crates/jengine-web/src/script_host.rs"] {
        let body = dispatch_body(rel);
        if body.contains("scripts::dispatch_event(") {
            continue;
        }
        assert!(body.contains("event_handler_args("), "{rel}: dispatch no longer uses event_handler_args");
        assert!(
            !body.contains("args.push(&JsValue::from_str(name))"),
            "{rel}: dispatch builds the on_event argument list by hand again - for every handler"
        );
    }
}

/// Native, both ways a script sends: a table payload arrives as a table.
/// The Luau binding took a string only, so `Event.send(name, e, { d = 1 })`
/// threw natively ("error converting Lua table to String") while the editor
/// delivered the table - found by Test Lab scene 170's lanterns. And JSON
/// text arrived natively as the text, where the browsers hand the value.
#[cfg(feature = "native")]
#[test]
fn a_native_script_sends_a_table_and_the_listener_gets_a_table() {
    use hecs::World;
    use jengine_core::ecs::components::{CustomComponents, ScriptComponent, ScriptLanguage, Transform};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::input::InputState;

    let dir = std::env::temp_dir().join(format!("jengine_event_payload_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sender = dir.join("sender.lua");
    let listener = dir.join("listener.lua");
    std::fs::write(&listener, r#"
function init(entity)
    Event.on("ping", entity)
end

function update(entity, dt)
end

function on_ping(entity, data)
    if type(data) == "table" then
        set_custom(entity, "Probe", "d", data.d)
        set_custom(entity, "Probe", "word", data.word)
    elseif type(data) == "number" then
        set_custom(entity, "Probe", "number", data)
    else
        set_custom(entity, "Probe", "text", data)
    end
end

function on_poke(entity, data)
    set_custom(entity, "Probe", "poked", data.n)
end
"#).unwrap();
    std::fs::write(&sender, r#"
local sent = false

function update(entity, dt)
    if sent then return end
    sent = true
    local target = Entity.find_by_name("Listener")
    Event.send("ping", entity, { d = 3, word = "hi" })
    Event.send("ping", entity, 7)
    Event.send("ping", entity, "plain words")
    Event.send_to("poke", entity, target, { n = 5 })
end
"#).unwrap();
    let mut world = World::new();
    let script = |p: &std::path::Path| ScriptComponent {
        language: ScriptLanguage::Lua,
        source_path: p.to_string_lossy().into(),
        cull_distance: None,
    };
    let l = world.spawn((
        jengine_core::ecs::components::Name("Listener".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        script(&listener),
    ));
    world.spawn((Transform::default(), script(&sender)));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..4 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    let read = |field: &str| {
        let c = world.get::<&CustomComponents>(l).unwrap();
        c.0.get("Probe").and_then(|p| p.get(field)).cloned()
    };
    assert_eq!(read("d").and_then(|v| v.as_f64()), Some(3.0), "the table arrived as a table");
    assert_eq!(read("word").and_then(|v| v.as_str().map(String::from)).as_deref(), Some("hi"));
    assert_eq!(read("number").and_then(|v| v.as_f64()), Some(7.0), "a number as a number");
    assert_eq!(read("text").and_then(|v| v.as_str().map(String::from)).as_deref(), Some("plain words"));
    assert_eq!(read("poked").and_then(|v| v.as_f64()), Some(5.0), "send_to's table too");
    let _ = std::fs::remove_dir_all(&dir);
}
