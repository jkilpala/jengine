//! Every runtime has the project's skeletal clips.
//!
//! A `SkeletalPlayer` names a clip by asset id and plays whatever the
//! `AssetManager` holds under it - so a runtime that never loads `.skelanim`
//! files plays nothing, silently: the rig holds its bind pose. Native walks
//! `assets/`, the editor pushes every clip in the project, and the web build
//! loaded none until Sep 2026, when the builder began writing them all into
//! `skelanim_manifest.json`. A source check, because the browser half only
//! runs in a browser.

fn read(rel: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn native_loads_every_clip_under_assets() {
    let src = read("crates/jengine-runtime/src/wgpu_scene_runner.rs");
    assert!(src.contains("load_skeletal_clips_from_dir"), "native no longer loads .skelanim clips");
}

#[test]
fn the_editor_pushes_every_clip_into_the_viewport() {
    assert!(read("editor/src/lib/Viewport.svelte").contains("viewport_load_skeletal_clip"));
    assert!(read("crates/jengine-editor-viewport/src/lib.rs").contains("insert_skeletal_clip"));
}

#[test]
fn a_web_build_ships_every_clip_and_loads_them() {
    let builder = read("crates/jengine-builder/src/web.rs");
    assert!(builder.contains("\"skelanim_manifest.json\""), "the web build no longer writes the clip manifest");
    let web = read("crates/jengine-web/src/lib.rs");
    assert!(web.contains("assets/skelanim_manifest.json"), "the web runtime no longer reads the clip manifest");
    assert!(web.contains("insert_skeletal_clip"), "the web runtime reads the manifest and inserts nothing");
    assert!(web.contains("install_skeletal_clips(&mut assets, skeletal)"), "the web runtime never calls its clip loader");
}
