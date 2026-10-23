//! Pointer scrolling must be wired in every runtime, and must not fight clicks.
//!
//! # Why a source scan
//!
//! `apply_gui_wheel_scroll` was written to be shared by all three runtimes,
//! with a doc comment saying so — "a copy per runtime is exactly how GUI
//! features end up working in one of them only". **Only the two browser
//! runtimes ever called it.** A scrolling list in a native build could be moved
//! by nothing but a script, and nothing said so.
//!
//! That is the same shape as the patrol library (BUGS #7) and the input mirror
//! (BUGS #9): shared code exists, is correct, is tested, and one runtime never
//! calls it. Compiling proves nothing, and neither does a unit test of the
//! shared function. Only a check on the call sites does.

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");
const WEB_SCRIPTS: &str = include_str!("../../jengine-web/src/script_host.rs");
const NATIVE_CLICKS: &str = include_str!("../src/scripting/api.rs");

fn runtimes() -> [(&'static str, &'static str); 3] {
    [("native", NATIVE), ("viewport", VIEWPORT), ("web", WEB)]
}

#[test]
fn every_runtime_scrolls_gui_containers_with_the_wheel() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("apply_gui_wheel_scroll"),
            "{name} never calls `apply_gui_wheel_scroll`, so a scrolling GUI \
             container cannot be scrolled by the wheel there — the function is \
             shared precisely so this does not happen"
        );
    }
}

#[test]
fn every_runtime_drives_the_pointer_scroll_gesture() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("update_gui_scroll_drag"),
            "{name} never calls `update_gui_scroll_drag`, so a list cannot be \
             dragged or flicked there and the elastic ends never spring back"
        );
    }
}

/// A drag must not also press the button it started on.
///
/// Clicks fire on release, and the release is swallowed when the gesture turned
/// out to be a scroll. Firing on press made dragging a list of buttons
/// impossible — the press alone activated one — so this is what makes the
/// gesture usable rather than a nicety.
#[test]
fn a_scroll_gesture_swallows_the_click_that_ends_it() {
    for (name, src) in [
        ("viewport", VIEWPORT),
        ("web", WEB_SCRIPTS),
        ("native", NATIVE_CLICKS),
    ] {
        assert!(
            src.contains("gui_scroll_dragging") || src.contains("gui_scroll.is_dragging()"),
            "{name}'s GUI click dispatch does not consult the scroll gesture, so \
             dragging a list there activates whichever button the drag began on"
        );
    }
}

/// And clicks must fire on the release edge, not the press edge.
///
/// A press-fired click cannot be vetoed: at press time no drag has been
/// committed yet, because a gesture is only a scroll once it has moved.
#[test]
fn gui_clicks_fire_on_release() {
    for (name, src, marker) in [
        ("viewport", VIEWPORT, "app.input.mouse_left_just_released"),
        ("web", WEB_SCRIPTS, "!app.input.mouse_left_just_released"),
        ("native", NATIVE_CLICKS, "!input.mouse_left_just_released"),
    ] {
        assert!(
            src.contains(marker),
            "{name} does not gate its GUI click dispatch on the release edge. A \
             press-fired click cannot be vetoed by a scroll gesture, because at \
             press time the gesture has not moved yet."
        );
    }
}

/// The runtimes must publish the gesture onto `InputState`.
///
/// Native's click dispatch is inside `ScriptSystem::update`, which sees an
/// `InputState` and nothing else — so the flag is the only channel it has.
#[test]
fn every_runtime_publishes_the_gesture_to_input_state() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("input.gui_scroll_dragging = "),
            "{name} never publishes its scroll gesture to `InputState`, so the \
             click dispatch cannot see it"
        );
    }
}

/// The elastic offset must never be written to a `.gui` file.
///
/// A stretch is a moment inside a gesture, not a property of a document.
/// Serializing one would reopen a saved layout mid-bounce.
#[test]
fn the_elastic_offset_is_not_serialized() {
    use jengine_core::ui::gui_document::NodeStyle;

    let style = NodeStyle { overscroll_x: 12.0, overscroll_y: -34.0, ..Default::default() };
    let json = serde_json::to_string(&style).expect("serialize");
    assert!(
        !json.contains("overscroll"),
        "the elastic offset was serialized into a document: {json}"
    );

    // And it comes back as zero, not as whatever was last on screen.
    let round: NodeStyle = serde_json::from_str(&json).expect("deserialize");
    assert_eq!((round.overscroll_x, round.overscroll_y), (0.0, 0.0));
}

/// Layout applies the stretch on top of the clamped offset.
///
/// The clamp is load-bearing: `gui_set(id, "scroll_y", 99999)` is how a script
/// says "jump to the end". The stretch has to be additive so it can leave the
/// range without breaking that.
#[test]
fn layout_adds_the_stretch_after_clamping_the_scroll() {
    use jengine_core::ui::gui_document::load_gui;
    use jengine_core::ui::layout::{compute_layout, rect_of};

    // A 100px-tall viewport holding 300px of content: 200px of scroll range.
    let doc_json = r#"{
      "name": "test",
      "anchor": "TopLeft",
      "root": {
        "id": "root", "node_type": { "type": "Panel" },
        "style": { "width": {"type":"Fixed","value":200.0},
                   "height": {"type":"Fixed","value":100.0},
                   "overflow": "Scroll" },
        "children": [
          { "id": "inner", "node_type": { "type": "Panel" },
            "style": { "width": {"type":"Fixed","value":200.0},
                       "height": {"type":"Fixed","value":300.0} },
            "children": [] }
        ]
      }
    }"#;

    let mut doc = load_gui(doc_json).expect("parse");
    let base = rect_of(&compute_layout(&doc, 400.0, 400.0), "inner").expect("inner").y;

    // Past the end: the clamp pins the scroll, so nothing moves...
    doc.root.style.scroll_y = 99_999.0;
    let clamped = rect_of(&compute_layout(&doc, 400.0, 400.0), "inner").expect("inner").y;
    assert!(
        (clamped - base).abs() > 1.0,
        "a huge scroll_y should have scrolled to the end"
    );
    let pinned = clamped;
    doc.root.style.scroll_y = 1_000_000.0;
    let still = rect_of(&compute_layout(&doc, 400.0, 400.0), "inner").expect("inner").y;
    assert_eq!(still, pinned, "scroll_y is not being clamped any more");

    // ...but the stretch moves it further, which is the whole point.
    doc.root.style.overscroll_y = 30.0;
    let stretched = rect_of(&compute_layout(&doc, 400.0, 400.0), "inner").expect("inner").y;
    assert_ne!(
        stretched, pinned,
        "the elastic offset did not reach layout — it must be added after the clamp"
    );
}
