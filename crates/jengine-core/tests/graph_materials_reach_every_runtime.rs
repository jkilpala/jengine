//! A graph material draws in every runtime, lit scene or not.
//!
//! Two holes this closes, both silent - the sprite simply drew with the
//! default shader:
//!
//! - **Web built no graph pipelines at all.** Its world-pass flush ignored
//!   the material id ("jengine-web doesn't yet do per-material pipeline
//!   routing"), and nothing fetched a `.material` or its `.shader`. Now it
//!   fetches them on first sight (`poll_missing_materials`, the lazy path
//!   textures take) and routes like the others.
//! - **The emission pass flushed plain in all three.** With lighting on,
//!   unlit sprites draw in that pass (`WorldLayer::Emission`), so an unlit
//!   sprite's graph material - how a glowing effect is usually set up -
//!   vanished in any lit scene. Found building the showcase's Shader Gallery.
//!
//! Each runtime now resolves a material through one `material_pipeline`
//! function, and both passes call it. Source-level, like
//! `render_passes_reach_every_runtime`: what went wrong was the calling.

mod support;

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    support::with_frame_draw(&std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("reading {rel}: {e}")))
}

fn runtimes() -> [(&'static str, String); 3] {
    [
        ("native", read("crates/jengine-runtime/src/wgpu_scene_runner.rs")),
        ("viewport", read("crates/jengine-editor-viewport/src/lib.rs")),
        ("web", read("crates/jengine-web/src/lib.rs")),
    ]
}

#[test]
fn every_runtime_routes_materials_through_one_function() {
    for (name, src) in runtimes() {
        assert!(src.contains("fn material_pipeline("), "{name}: no `material_pipeline` function");
        let calls = src.matches("material_pipeline(").count() - 1;
        assert!(calls >= 2, "{name}: `material_pipeline` is called {calls} time(s) - the world pass and the emission pass both need it");
    }
}

#[test]
fn the_emission_pass_draws_graph_materials() {
    for (name, src) in runtimes() {
        let at = src.find("WorldLayer::Emission").unwrap_or_else(|| panic!("{name}: no emission pass"));
        // The emission pass's flush closure follows the layer argument.
        let window = &src[at..(at + 2500).min(src.len())];
        assert!(
            window.contains("material_pipeline(") && window.contains("flush_with_pipeline("),
            "{name}: the emission pass flushes plain, so an unlit sprite's graph material draws with the default shader in a lit scene"
        );
    }
}

#[test]
fn web_fetches_materials_and_their_graphs() {
    let src = read("crates/jengine-web/src/lib.rs");
    assert!(src.contains("poll_missing_materials(&app_loop);"), "web never polls for the materials a draw asked for");
    assert!(src.contains("is_builtin_material(mid)"), "web would request `.material` files for built-in ids");
    assert!(
        !src.contains("jengine-web doesn't yet do per-material pipeline"),
        "the note that web ignores materials is back"
    );
}

#[test]
fn builtin_material_ids_name_no_file() {
    use jengine_core::renderer::material::is_builtin_material;
    for id in ["default_sprite", "additive", "multiply", "screen", "lit_sprite", ""] {
        assert!(is_builtin_material(id), "{id}");
    }
    assert!(!is_builtin_material("materials/dissolve"));
}

/// A graph that reads the scene (`SceneColor`) cannot draw in the world pass -
/// the copy it samples is taken from that pass - so every runtime skips it
/// there (and in the emission pass), captures the target, and draws those
/// sprites in a pass of their own. A runtime that forgot the skip would draw
/// the pipeline without its second bind group, a validation error; one that
/// forgot the pass would lose the sprite.
#[test]
fn every_runtime_draws_scene_reading_graphs_after_a_capture() {
    for (name, src) in runtimes() {
        assert!(src.contains("scene_skipped = true;"), "{name}: the world pass does not skip scene-reading graphs");
        // A lit frame leaves scene readers out before the flush, so the flush
        // alone never learns there is one: every lit scene's Scene Color
        // sprites drew nothing until the hosts asked the world.
        assert!(
            src.contains("let mut scene_skipped = jengine_core::ecs::wgpu_systems::has_scene_readers(")
                // Inside core (`ecs::frame_draw`), where every host's frame is drawn.
                || src.contains("let mut scene_skipped = wgpu_systems::has_scene_readers("),
            "{name}: the scene pass runs only if the world flush met a scene reader - never, in a lit scene"
        );
        assert!(src.contains("capture_scene("), "{name}: nothing captures the scene for them");
        let at = src.find("WorldLayer::Emission").expect("emission pass");
        let window = &src[at..(at + 3000).min(src.len())];
        assert!(window.contains("material_reads_scene("), "{name}: the emission pass would draw a scene-reading graph unbound");
        // After the lighting composite, from the post chain's target: in a lit
        // scene the copy taken before it was the unlit albedo, so a shockwave
        // bent the floor and none of the glows.
        let composite = src.find(".composite(").unwrap_or_else(|| panic!("{name}: no composite"));
        let capture = src.find("capture_scene(").unwrap();
        assert!(capture > composite, "{name}: the scene is captured before the lighting composite");
        let pass = &src[capture..(capture + 600).min(src.len())];
        // `post` is the pass's own post chain - the screen's or a view's
        // (`ecs::views`).
        let reads_post = |needle: &str| {
            pass.contains(needle) || src[..capture].rfind(needle).is_some_and(|i| capture - i < 400)
        };
        assert!(reads_post("post_process.scene_target()") || reads_post("post.scene_target()"),
            "{name}: the capture should read the post chain's scene target, the lit image");
    }
}

/// A graph material that only a spawned prefab uses reaches the browser hosts
/// before the first one is drawn. The editor pushed only the materials scene
/// entities named, so Spellcraft's shockwaves - a graph over a white quad,
/// spawned from a prefab - drew with the default shader and flashed as white
/// boxes in the editor while native drew them. Now the viewport queues every
/// prefab's material as the prefab arrives (and any a draw misses) for the
/// editor to push, and web fetches them from its first frame.
#[test]
fn a_prefabs_graph_material_reaches_the_browser_hosts() {
    use jengine_core::scene::format::{EntityData, prefab_material_ids};
    let wave: EntityData = serde_json::from_str(
        r#"{ "name": "Wave", "components": {
            "sprite": { "texture_id": "", "color": [1, 1, 1, 1], "z_order": 0 },
            "material": { "material_id": "materials/shockwave" } } }"#,
    )
    .unwrap();
    let glow: EntityData = serde_json::from_str(
        r#"{ "name": "Glow", "components": {
            "material": { "material_id": "default_sprite", "blend_mode": "Additive", "uniforms": {} } } }"#,
    )
    .unwrap();
    let ids = prefab_material_ids([&wave, &glow]);
    assert_eq!(ids.into_iter().collect::<Vec<_>>(), vec!["materials/shockwave".to_string()], "built-ins name no file");

    let viewport = read("crates/jengine-editor-viewport/src/lib.rs");
    assert!(viewport.contains("prefab_material_ids("), "the viewport does not queue a prefab's materials");
    assert!(viewport.contains("pub fn viewport_take_material_requests"), "the viewport cannot ask for a material");
    let web = read("crates/jengine-web/src/lib.rs");
    assert!(web.contains("prefab_material_ids("), "web fetches a prefab's material only on its first draw");
    let editor = read("editor/src/lib/Viewport.svelte");
    assert!(
        editor.contains("viewport_take_material_requests()"),
        "the editor never answers the viewport's material requests"
    );
}