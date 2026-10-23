//! A sprite whose texture no scene named still gets its texture, in every
//! runtime.
//!
//! Each runtime preloads the textures its scene file (and prefabs) name. A
//! sprite that arrives any other way - a match-3 package creates its gem
//! sprites itself, naming textures from its own component - used to be drawn
//! by nobody: native logged "dropping draws for missing texture" and moved
//! on, the editor never knew to read the file, and web never fetched it.
//! Found running the arcade's Match-3 natively: the board played, and every
//! gem was invisible.
//!
//! The renderers already notice a missing texture (each keeps a set of ids it
//! dropped draws for); `asset::textures_to_request` turns that set into
//! requests, and each runtime answers them its own way. This checks all four
//! halves are connected.

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");
const EDITOR: &str = include_str!("../../../editor/src/lib/Viewport.svelte");

#[test]
fn native_loads_what_the_renderer_missed_after_each_frame() {
    assert!(NATIVE.contains("asset::textures_to_request("), "native never turns missing textures into loads");
    let present = NATIVE.find("frame.present(").expect("native presents frames");
    assert!(
        NATIVE[present..present + 200].contains("load_requested_textures(state)"),
        "native does not load missing textures after presenting a frame"
    );
}

#[test]
fn the_viewport_asks_and_the_editor_answers() {
    assert!(
        VIEWPORT.contains("pub fn viewport_take_texture_requests()")
            && VIEWPORT.contains("asset::textures_to_request("),
        "the viewport does not hand missing textures to the editor"
    );
    assert!(
        EDITOR.contains("viewport_take_texture_requests()") && EDITOR.contains("loadRequestedTextures();"),
        "the editor never asks the viewport which textures it is missing"
    );
}

#[test]
fn web_fetches_what_the_renderer_missed() {
    assert!(WEB.contains("asset::textures_to_request("), "web never turns missing textures into fetches");
    assert!(WEB.contains("poll_missing_textures(&app_loop);"), "web never polls for missing textures");
}
