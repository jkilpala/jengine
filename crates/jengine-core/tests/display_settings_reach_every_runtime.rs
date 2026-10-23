//! **Display and cursor settings** (Oct 2026): what an options menu changes -
//! windowed, borderless or exclusive fullscreen, the window's size or the
//! fullscreen resolution, vsync, a frame cap, and the cursor (shown, kept
//! in, a system shape or a picture). `Display.*` / `Cursor.*` write what is
//! wanted into the process-wide `jengine_core::display`; each host applies
//! what changed once a frame and publishes what it has. Before this a game
//! could not leave the window it started in, and the only frame cap was the
//! project's. Found by the showcase's Options Menu.

use std::sync::Mutex;

use jengine_core::display::{self, CursorShape, Info, Supports, WindowMode};

/// The settings are process-wide; the tests here take turns.
static TURN: Mutex<()> = Mutex::new(());

#[cfg(feature = "native")]
fn run_script(src: &str, frames: usize) -> serde_json::Map<String, serde_json::Value> {
    use hecs::World;
    use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::input::InputState;
    let dir = std::env::temp_dir().join(format!("jengine_display_{}_{}", std::process::id(), src.len()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("options.lua");
    std::fs::write(&path, src).unwrap();
    let mut world = World::new();
    let e = world.spawn((
        Name("Probe".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..frames {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    let c = world.get::<&CustomComponents>(e).unwrap();
    c.0.get("Probe").and_then(|p| p.as_object().cloned()).unwrap_or_default()
}

#[cfg(feature = "native")]
#[test]
fn a_script_sets_what_it_wants_and_reads_what_it_has() {
    let _t = TURN.lock().unwrap_or_else(|p| p.into_inner());
    display::reset();
    // What a host would publish: a desktop at 1280 x 720, borderless.
    display::publish(Info {
        mode: WindowMode::Borderless,
        width: 1280,
        height: 720,
        vsync: false,
        max_fps: 30,
        modes: vec![(2560, 1440, 165), (1920, 1080, 144)],
        supports: Supports { fullscreen: true, vsync: true, cursor_lock: true, ..Default::default() },
    });
    let probe = run_script(
        r#"
function update(entity, dt)
    Display.set_mode("borderless")
    Display.set_size(1600, 900)
    Display.set_vsync(false)
    Display.set_max_fps(30)
    Cursor.set_visible(false)
    Cursor.set_confined(true)
    Cursor.set_locked(true)
    Cursor.set_image("sprites/hand.png", 4, 2)
    local w, h = Display.get_size()
    local modes = Display.get_resolutions()
    Entity.set_custom(entity, "Probe", "mode", Display.get_mode())
    Entity.set_custom(entity, "Probe", "w", w)
    Entity.set_custom(entity, "Probe", "h", h)
    Entity.set_custom(entity, "Probe", "first", modes[1].w .. "x" .. modes[1].h .. "@" .. modes[1].hz)
    Entity.set_custom(entity, "Probe", "count", #modes)
    Entity.set_custom(entity, "Probe", "vsync", Display.get_vsync())
    Entity.set_custom(entity, "Probe", "fps", Display.get_max_fps())
    Entity.set_custom(entity, "Probe", "can_vsync", Display.supports("vsync"))
    Entity.set_custom(entity, "Probe", "can_exclusive", Display.supports("exclusive"))
    Entity.set_custom(entity, "Probe", "can_lock", Display.supports("cursor_lock"))
end
"#,
        1,
    );
    let (_, want) = display::desired();
    assert_eq!(want.mode, WindowMode::Borderless);
    assert_eq!(want.size, Some((1600, 900)));
    assert!(!want.vsync && !want.cursor_visible && want.cursor_confined && want.cursor_locked);
    assert_eq!(want.max_fps, Some(30));
    assert_eq!(want.cursor, CursorShape::Image { texture_id: "sprites/hand.png".into(), hot_x: 4, hot_y: 2 });
    let s = |k: &str| probe.get(k).cloned().unwrap_or(serde_json::Value::Null);
    assert_eq!(s("mode"), "borderless");
    assert_eq!((s("w"), s("h")), (serde_json::json!(1280.0), serde_json::json!(720.0)), "what the host has, not what was asked");
    assert_eq!(s("first"), "2560x1440@165");
    assert_eq!(s("count"), serde_json::json!(2.0));
    assert_eq!(s("vsync"), false);
    assert_eq!(s("fps"), serde_json::json!(30.0));
    assert_eq!(s("can_vsync"), true);
    assert_eq!(s("can_exclusive"), false);
    assert_eq!(s("can_lock"), true);
    display::reset();
}

#[cfg(feature = "native")]
#[test]
fn a_cap_of_nil_is_the_projects_and_a_cursor_name_is_a_system_cursor() {
    let _t = TURN.lock().unwrap_or_else(|p| p.into_inner());
    display::reset();
    run_script(
        r#"
function update(entity, dt)
    Display.set_max_fps(60)
    Display.set_max_fps(nil)
    Cursor.set_icon("pointer")
    Display.set_mode("fullscreen")
    Display.set_mode("nonsense")
end
"#,
        1,
    );
    let (_, want) = display::desired();
    assert_eq!(want.max_fps, None, "nil: the project's own cap");
    assert_eq!(want.cursor, CursorShape::Icon("pointer"));
    assert_eq!(want.mode, WindowMode::Fullscreen, "an unknown mode is ignored");
    display::reset();
}

fn read(p: &str) -> String {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(p)).unwrap().replace("\r\n", "\n")
}

#[test]
fn every_runtime_applies_them() {
    // Native: once a frame, before the frame is drawn, through winit and
    // the swapchain.
    let runner = read("crates/jengine-runtime/src/wgpu_scene_runner.rs");
    assert!(runner.contains("state.display.sync("), "the native runner never applies display settings");
    let native = read("crates/jengine-runtime/src/display_native.rs");
    for call in ["set_fullscreen(", "request_inner_size(", "set_cursor_grab(", "set_cursor_visible(", "create_custom_cursor(", "set_vsync(", "FrameCap::new("] {
        assert!(native.contains(call), "display_native never calls {call}");
    }
    // Both browser hosts sync each frame; the editor gives the page back on
    // Stop; a web build takes a new cap.
    let web = read("crates/jengine-web/src/lib.rs");
    let viewport = read("crates/jengine-editor-viewport/src/lib.rs");
    assert!(web.contains("app.display.sync(") && web.contains("app.frame_cap = jengine_core::time::FrameCap::new(app.fps)"));
    assert!(viewport.contains("app.display.sync("), "the editor never applies display settings");
    let stop = &viewport[viewport.find("pub fn viewport_stop(").unwrap()..];
    assert!(stop[..1200].contains("display.release("), "Stop leaves the editor fullscreen or cursorless");
    assert!(read("crates/jengine-core/src/scripting/bridge/adapters/js/nav.rs").contains("install_display_callbacks("));
}

#[test]
fn every_script_function_reaches_the_browsers_hook() {
    let preamble = read("crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs");
    let hook = read("crates/jengine-core/src/scripting/bridge/adapters/js/display.rs");
    let luau = read("crates/jengine-core/src/scripting/bridge/adapters/luau.rs");
    for (flat, op) in [
        ("display_set_mode", "set_mode"),
        ("display_get_mode", "get_mode"),
        ("display_set_size", "set_size"),
        ("display_get_size", "get_size"),
        ("display_get_resolutions", "get_resolutions"),
        ("display_set_vsync", "set_vsync"),
        ("display_get_vsync", "get_vsync"),
        ("display_set_max_fps", "set_max_fps"),
        ("display_get_max_fps", "get_max_fps"),
        ("display_supports", "supports"),
        ("cursor_set_visible", "cursor_visible"),
        ("cursor_set_confined", "cursor_confined"),
        ("cursor_set_locked", "cursor_locked"),
        ("cursor_set_icon", "cursor_icon"),
        ("cursor_set_image", "cursor_image"),
    ] {
        let at = preamble.find(&format!("function {flat}(")).unwrap_or_else(|| panic!("no preamble {flat}"));
        let line = &preamble[at..at + preamble[at..].find('\n').unwrap()];
        assert!(line.contains(&format!("__display('{op}'")), "{flat} does not ask the hook for '{op}'");
        assert!(hook.contains(&format!("\"{op}\" =>")), "the hook has no '{op}'");
        assert!(luau.contains(&format!("globals.set(\"{flat}\"")), "{flat} is not bound natively");
    }
}
