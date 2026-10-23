//! A project's `settings.background_color` is what every runtime clears to.
//!
//! The editor sent it to the viewport (`viewport_set_clear_color`), and that
//! was the only runtime that honoured it: native cleared to a constant and
//! kept the parsed setting as `#[allow(dead_code)]`, and web read a
//! `bg_color` key that no project file contains. Found building a
//! vector-style space shooter, whose black vector-monitor background came
//! out grey natively. A source scan, like the other `*_reaches_every_runtime` guards.

const NATIVE_RUNNER: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const NATIVE_LIB: &str = include_str!("../../jengine-runtime/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");
const EDITOR: &str = include_str!("../../../editor/src/lib/Viewport.svelte");

#[test]
fn native_clears_to_the_project_background() {
    assert!(
        NATIVE_LIB.contains("s.clear_color = self.settings.background_color;"),
        "native never hands the project's background_color to the scene runner"
    );
    assert!(
        NATIVE_RUNNER.contains("state.clear_color.map(f64::from)"),
        "native's scene pass does not clear to state.clear_color"
    );
}

#[test]
fn web_reads_the_key_project_files_use() {
    assert!(
        WEB.contains("#[serde(alias = \"background_color\")]"),
        "web reads its clear colour under a key project files do not use"
    );
}

#[test]
fn the_editor_sends_it_to_the_viewport() {
    assert!(EDITOR.contains("proj.settings.background_color") && EDITOR.contains("viewport_set_clear_color("));
}
