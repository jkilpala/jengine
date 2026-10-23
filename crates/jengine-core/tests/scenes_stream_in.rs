//! **Scenes load in the background and join the running one** (Oct 2026):
//! `Scene.load_additive(path, x, y)` lays a scene into the one running as a
//! chunk once its assets are in - native decodes its textures on a worker
//! thread, the browsers fetch - and `Scene.unload(chunk)` takes it out;
//! `Scene.preload(path)` loads a scene's assets without showing it so a later
//! `Scene.load` is a swap. Before this a scene came only whole and at once,
//! decoding every texture on the frame it was asked for. Found by the
//! showcase's The Long Corridor.
//!
//! `scene::streaming`'s own tests cover the requests and the merge; this
//! drives the script surface natively and checks every host loads, lays in,
//! takes out and forgets chunks.

use jengine_core::scene::streaming::{self as st, Request, Status};

fn read(p: &str) -> String {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(p)).unwrap().replace("\r\n", "\n")
}

#[cfg(feature = "native")]
#[test]
fn a_script_asks_for_chunks_and_reads_how_far_they_have_got() {
    use hecs::World;
    use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::input::InputState;
    const SRC: &str = r#"
local a, b
local frame = 0
function update(entity, dt)
    frame = frame + 1
    if frame == 1 then
        a = Scene.load_additive("scenes/rooms/hall_1.scene", 1280, 0)
        b = Scene.load_additive("scenes/rooms/hall_2.scene", 2560, 0)
        Scene.preload("scenes/boss.scene")
        Entity.set_custom(entity, "Probe", "a", a)
        Entity.set_custom(entity, "Probe", "b", b)
        Entity.set_custom(entity, "Probe", "first", Scene.status(a))
    elseif frame == 2 then
        Scene.unload(b)
        Entity.set_custom(entity, "Probe", "a_progress", Scene.progress(a))
        Entity.set_custom(entity, "Probe", "a_now", Scene.status(a))
        Entity.set_custom(entity, "Probe", "b_now", Scene.status(b))
        Entity.set_custom(entity, "Probe", "boss", Scene.status("scenes/boss.scene"))
    end
end
"#;
    let dir = std::env::temp_dir().join(format!("jengine_stream_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("corridor.lua");
    std::fs::write(&path, SRC).unwrap();
    let mut world = World::new();
    let e = world.spawn((
        Name("Probe".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    let read = |k: &str| {
        world.get::<&CustomComponents>(e).ok().and_then(|c| c.0.get("Probe").and_then(|p| p.get(k)).cloned())
    };
    let a = read("a").and_then(|v| v.as_f64()).expect("a handle") as u32;
    let b = read("b").and_then(|v| v.as_f64()).expect("a handle") as u32;
    assert_ne!(a, b);
    assert_eq!(read("first"), Some(serde_json::json!("loading")));
    // The host takes the requests and reports.
    let reqs: Vec<Request> = st::take_requests()
        .into_iter()
        .filter(|r| match r {
            Request::Additive { handle, .. } => *handle == a || *handle == b,
            Request::Preload { path } => path == "scenes/boss.scene",
            Request::Unload { .. } => false,
        })
        .collect();
    assert_eq!(reqs.len(), 3);
    assert!(reqs.contains(&Request::Additive { handle: a, path: "scenes/rooms/hall_1.scene".into(), offset: [1280.0, 0.0] }));
    st::set_status(a, Status::Loading(0.5));
    st::set_preload("scenes/boss.scene", Status::Loaded);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    let read = |k: &str| {
        world.get::<&CustomComponents>(e).ok().and_then(|c| c.0.get("Probe").and_then(|p| p.get(k)).cloned())
    };
    assert_eq!(read("a_progress"), Some(serde_json::json!(0.5)));
    assert_eq!(read("a_now"), Some(serde_json::json!("loading")));
    assert_eq!(read("b_now"), Some(serde_json::json!("unloaded")));
    assert_eq!(read("boss"), Some(serde_json::json!("loaded")));
    assert!(st::take_requests().contains(&Request::Unload { handle: b }));
}

#[test]
fn every_host_streams_lays_in_takes_out_and_forgets_chunks() {
    let runner = read("crates/jengine-runtime/src/wgpu_scene_runner.rs");
    assert!(runner.contains("sync_streaming(state);"), "native never drains streaming requests");
    assert!(runner.contains("streaming::merge(") && runner.contains("st::unload_chunk("));
    assert!(runner.contains("streaming::scene_replaced()"), "native keeps chunks of a replaced scene");
    assert!(read("crates/jengine-runtime/src/streaming_native.rs").contains("std::thread::spawn"), "textures decode on the main thread");
    let web = read("crates/jengine-web/src/lib.rs");
    assert!(web.contains("poll_streaming(&app_loop)") && web.contains("st::merge(") && web.contains("attach_scripts("));
    assert!(web.contains("streaming::scene_replaced()"));
    let vp = read("crates/jengine-editor-viewport/src/lib.rs");
    for f in ["fn viewport_take_stream_requests(", "fn viewport_add_scene_chunk(", "fn viewport_set_stream_status("] {
        assert!(vp.contains(f), "the viewport has no {f}");
    }
    assert!(vp.matches("streaming::scene_replaced()").count() >= 3, "a transition, a pop and Stop all replace the world");
    let svelte = read("editor/src/lib/Viewport.svelte");
    assert!(svelte.contains("viewport_take_stream_requests()") && svelte.contains("viewport_add_scene_chunk("));
    // The script surface in every runtime.
    let preamble = read("crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs");
    let hook = read("crates/jengine-core/src/scripting/bridge/adapters/js/streaming.rs");
    let luau = read("crates/jengine-core/src/scripting/bridge/adapters/luau.rs");
    for (flat, op) in [
        ("scene_load_additive", "load_additive"),
        ("scene_unload", "unload"),
        ("scene_preload", "preload"),
        ("scene_status", "status"),
        ("scene_progress", "progress"),
    ] {
        let at = preamble.find(&format!("function {flat}(")).unwrap_or_else(|| panic!("no preamble {flat}"));
        let line = &preamble[at..at + preamble[at..].find('\n').unwrap()];
        assert!(line.contains(&format!("__streaming('{op}'")), "{flat} does not ask the hook for '{op}'");
        assert!(hook.contains(&format!("\"{op}\" =>")), "the hook has no '{op}'");
        assert!(luau.contains(&format!("globals.set(\"{flat}\"")), "{flat} is not bound natively");
    }
}
