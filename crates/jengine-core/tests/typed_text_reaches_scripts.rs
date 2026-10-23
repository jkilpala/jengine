//! `Input.get_typed()`: the characters typed this frame, as the keyboard
//! layout made them - for a command that is a character rather than a key.
//!
//! Found by a roguelike, whose `?` (help) was "shift + Slash": a US keyboard's
//! `?`. On a Finnish one `?` is shift and the key beside 0, so help never
//! opened, and `<` / `>` (the stairs) had the same problem. Every host already
//! recorded the characters for text fields (`InputState::typed_chars`); this
//! hands them to scripts too.

use jengine_core::input::{InputState, KeyCode};

/// A real Luau script reads what was typed, frame by frame - and nothing
/// once the frame is over.
#[cfg(feature = "native")]

/// The per-frame publish both browser hosts share (`adapters::js::mirror`).
const MIRROR: &str = include_str!("../src/scripting/bridge/adapters/js/mirror.rs");

/// A host's source as far as what it publishes goes: its own, plus the shared
/// mirror - but only if it calls it, so a host that dropped the call fails.
fn with_mirror(src: &str) -> String {
    if src.contains("mirror::sync_world_to_play_namespace(") {
        format!("{src}
{MIRROR}")
    } else {
        src.to_string()
    }
}

#[test]
fn a_script_reads_the_characters_typed_this_frame() {
    use hecs::World;
    use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
    use jengine_core::ecs::systems::ScriptSystem;
    const SRC: &str = r#"
function update(entity, dt)
    local typed = Input.get_typed()
    local seen = Entity.get_custom(entity, "Probe", "seen") or ""
    Entity.set_custom(entity, "Probe", "seen", seen .. "[" .. typed .. "]")
    if typed:find("?", 1, true) then Entity.set_custom(entity, "Probe", "help", true) end
end
"#;
    let dir = std::env::temp_dir().join(format!("jengine_typed_{}_{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("typed.lua");
    std::fs::write(&path, SRC).unwrap();
    let mut world = World::new();
    let e = world.spawn((
        Name("Probe".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let mut input = InputState::new();
    let mut frame = |input: &mut InputState, world: &mut World, typed: &str| {
        input.begin_frame();
        // A Finnish `?`: shift and the key beside 0 (which the layout-blind
        // key code calls Minus) - the layout's character is what counts.
        if !typed.is_empty() {
            input.key_down(KeyCode::Minus);
            input.type_str(typed);
        }
        system.run(world, 1.0 / 60.0, input, None, None);
        input.key_up(KeyCode::Minus);
    };
    frame(&mut input, &mut world, "");
    frame(&mut input, &mut world, "?");
    frame(&mut input, &mut world, "");
    frame(&mut input, &mut world, "<>");
    let read = |key: &str| {
        world.get::<&CustomComponents>(e).ok().and_then(|c| c.0.get("Probe").and_then(|p| p.get(key)).cloned())
    };
    assert_eq!(read("seen").and_then(|v| v.as_str().map(str::to_string)).as_deref(), Some("[][?][][<>]"));
    assert_eq!(read("help"), Some(serde_json::json!(true)));
}

/// The browser preamble reads what its host published, and an empty string
/// before anything was.
#[test]
fn the_browser_preamble_reads_the_published_text() {
    const SRC: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
    let start = SRC.find("  function input_get_typed(").expect("input_get_typed is in the preamble");
    let end = start + SRC[start..].find("\n  }}").unwrap() + 4;
    let func = SRC[start..end].replace("{{", "{").replace("}}", "}");
    let script = format!(
        "var window = {{ __jengine_play: {{}} }};\n{func}\n\
         var a = input_get_typed();\n\
         window.__jengine_play.typed = '?<';\n\
         console.log(JSON.stringify([a, input_get_typed()]));"
    );
    let out = match std::process::Command::new("node").arg("-e").arg(&script).output() {
        Ok(out) => out,
        Err(_) => {
            eprintln!("the_browser_preamble_reads_the_published_text: no node - skipped");
            return;
        }
    };
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.trim(), r#"["","?<"]"#, "{text}\n{}", String::from_utf8_lossy(&out.stderr));
}

/// Both browser hosts publish the frame's characters under the name the
/// preamble reads.
#[test]
fn both_browser_hosts_publish_the_typed_text() {
    for (host, src) in [
        ("editor viewport", include_str!("../../jengine-editor-viewport/src/lib.rs")),
        ("web", include_str!("../../jengine-web/src/script_host.rs")),
    ] {
        let src = with_mirror(src);
        assert!(
            src.contains("JsValue::from_str(\"typed\")") && src.contains("typed_chars()"),
            "{host} does not publish `typed` for Input.get_typed"
        );
    }
}
