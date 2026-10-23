//! Every host sets the frame view before its sprite passes, from the camera
//! that pass draws through.
//!
//! `renderer::frame_view` culls nothing until a host sets it, so a host that
//! forgets is not wrong - just as slow as before, in that runtime only, which
//! is the kind of difference nothing else here would notice. The editor's
//! picture-in-picture and its asset preview draw through cameras of their
//! own and must set theirs too, or they would cull against the main view and
//! read as "sprites missing in the thumbnail".

use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// Every `view_projection()` a host builds is followed by a
/// `set_frame_view` for the same camera within a few lines.
fn every_projection_sets_the_view(host: &str, src: &str) {
    let lines: Vec<&str> = src.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with("//") || !t.contains(".view_projection()") {
            continue;
        }
        let Some(cam) = t.split(".view_projection()").next().and_then(|s| s.rsplit(|c: char| !c.is_alphanumeric() && c != '_').next()) else {
            continue;
        };
        // The editor grid draws no sprites through its own projection.
        let enclosing_fn = lines[..i]
            .iter()
            .rev()
            .find(|l| l.starts_with("fn ") || l.starts_with("pub fn "))
            .copied()
            .unwrap_or("");
        if enclosing_fn.contains("fn draw_editor_grid") {
            continue;
        }
        let window = lines[i..lines.len().min(i + 6)].join("\n");
        assert!(
            window.contains(&format!("set_frame_view(&{cam})")),
            "{host}:{}: `{cam}.view_projection()` is not followed by `set_frame_view(&{cam})`, so that pass culls against whatever view was set last",
            i + 1
        );
    }
}

#[test]
fn the_native_runtime_sets_the_frame_view() {
    let src = read("jengine-runtime/src/wgpu_scene_runner.rs");
    assert!(src.contains("set_frame_view("));
    every_projection_sets_the_view("native", &src);
}

#[test]
fn the_editor_viewport_sets_the_frame_view_for_each_of_its_cameras() {
    let src = read("jengine-editor-viewport/src/lib.rs");
    assert!(src.matches("set_frame_view(").count() >= 3, "main view, picture-in-picture and asset preview");
    every_projection_sets_the_view("viewport", &src);
}

#[test]
fn the_web_runtime_sets_the_frame_view() {
    let src = read("jengine-web/src/lib.rs");
    assert!(src.contains("set_frame_view("));
    every_projection_sets_the_view("web", &src);
}
