//! Script getters that answered their defaults in silence (Oct 2026).
//!
//! In the browsers `get_material`, `Animation.get_current` / `get_state` /
//! `is_playing`, `Sprite.get_anchor`, `Particles.get_count`, `Scene.get_name`
//! and `gui_on_click`'s short form read `__jengine_play` fields no host
//! published. Natively the animation getters read maps nothing filled (and
//! `is_animation_playing` was bound twice, the later binding reading one of
//! them), and no runtime recorded the scene's name at all. Every name bound,
//! every call returned, so no ratchet saw it - which is what the second test
//! here is for.

/// Each `__jengine_play.<field>` the preamble reads must be written by
/// something: the preamble itself, the shared mirror and publishers, a host,
/// or the editor's Lua runtime. A field read and never written is a getter
/// that answers its default forever.
#[test]
fn every_play_field_the_preamble_reads_is_written() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");
    let preamble = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
    let mut writers = String::new();
    let mut add_dir = |dir: &str, ext: &str| {
        let mut stack = vec![std::path::PathBuf::from(format!("{root}{dir}"))];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).unwrap_or_else(|e| panic!("{}: {e}", d.display())).flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == ext) {
                    writers.push_str(&std::fs::read_to_string(&p).unwrap());
                }
            }
        }
    };
    add_dir("crates/jengine-core/src/scripting/bridge/adapters/js", "rs");
    add_dir("crates/jengine-web/src", "rs");
    add_dir("crates/jengine-editor-viewport/src", "rs");
    writers.push_str(&std::fs::read_to_string(format!("{root}editor/src/lib/lua-runtime.ts")).unwrap());
    writers.push_str(&std::fs::read_to_string(format!("{root}editor/src/lib/Viewport.svelte")).unwrap());

    let mut read = std::collections::BTreeSet::new();
    for line in preamble.lines() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        let mut rest = line;
        while let Some(i) = rest.find("__jengine_play.") {
            let after = &rest[i + "__jengine_play.".len()..];
            let name: String = after.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !name.is_empty() {
                read.insert(name);
            }
            rest = after;
        }
    }
    let written = |f: &str| {
        writers.contains(&format!("\"{f}\""))
            || writers.contains(&format!("'{f}'"))
            || writers.contains(&format!(".{f} = "))
            || writers.contains(&format!(".{f}="))
    };
    let unwritten: Vec<&String> = read.iter().filter(|f| !written(f)).collect();
    assert!(read.len() > 40, "the scan found only {} fields - has the preamble moved?", read.len());
    assert!(
        unwritten.is_empty(),
        "the browser preamble reads __jengine_play fields nothing writes, so their getters \
         answer a default forever: {unwritten:?}. Publish them (adapters::js::mirror)."
    );
}

#[cfg(feature = "native")]
#[test]
fn a_script_reads_animation_anchor_material_and_scene_name() {
    use hecs::World;
    use jengine_core::animation::{AnimStateMachine, SpriteAnimator};
    use jengine_core::ecs::components::{
        CustomComponents, MaterialComponent, Name, ScriptComponent, ScriptLanguage, SpriteComponent, Transform,
    };
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::input::InputState;
    const SRC: &str = r#"
function update(entity, dt)
    local ax, ay = Sprite.get_anchor(entity)
    Entity.set_custom(entity, "Probe", "clip", Animation.get_current(entity))
    Entity.set_custom(entity, "Probe", "state", Animation.get_state(entity))
    Entity.set_custom(entity, "Probe", "playing", Animation.is_playing(entity))
    Entity.set_custom(entity, "Probe", "anchor_y", ay)
    Entity.set_custom(entity, "Probe", "material", Material.get(entity))
    Entity.set_custom(entity, "Probe", "scene", Scene.get_name())
end
"#;
    let dir = std::env::temp_dir().join(format!("jengine_getters_{}_{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("getters.lua");
    std::fs::write(&path, SRC).unwrap();
    let mut world = World::new();
    let mut animator = SpriteAnimator::default();
    animator.current_clip = Some("walk".into());
    animator.playing = true;
    let machine = AnimStateMachine { current_state: Some("Idle".into()), ..Default::default() };
    let sprite = SpriteComponent { anchor: [0.5, 0.0], ..Default::default() };
    let material = MaterialComponent { material_id: "glow".into(), ..Default::default() };
    let e = world.spawn((
        Name("Probe".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        animator,
        machine,
        sprite,
        material,
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    jengine_core::scene::set_current_name("Village");
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..2 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    let read = |key: &str| {
        world.get::<&CustomComponents>(e).ok().and_then(|c| c.0.get("Probe").and_then(|p| p.get(key)).cloned())
    };
    assert_eq!(read("clip"), Some(serde_json::json!("walk")));
    assert_eq!(read("state"), Some(serde_json::json!("Idle")));
    assert_eq!(read("playing"), Some(serde_json::json!(true)));
    assert_eq!(read("anchor_y").and_then(|v| v.as_f64()), Some(0.0));
    assert_eq!(read("material"), Some(serde_json::json!("glow")));
    assert_eq!(read("scene"), Some(serde_json::json!("Village")));
}

#[test]
fn a_scene_name_is_read_from_its_json() {
    assert_eq!(jengine_core::scene::name_of(r#"{ "name": "53-CameraFx", "entities": [] }"#).as_deref(), Some("53-CameraFx"));
    assert_eq!(jengine_core::scene::name_of(r#"{ "entities": [] }"#), None);
}
