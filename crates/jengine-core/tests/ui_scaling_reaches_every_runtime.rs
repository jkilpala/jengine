//! A UI authored at one size must be scaled the same way by every runtime.
//!
//! # Why a source scan
//!
//! Scaling is not a function a runtime calls once - it is three things a
//! runtime has to do together: lay the UI out against
//! [`UiScale::virtual_size`], draw through `UiScale::projection`, and convert
//! the pointer with `UiScale::to_ui` before hit-testing. Do two of the three
//! and the UI draws correctly but cannot be clicked, or answers clicks
//! somewhere it is not drawn. Nothing about that fails to compile, and the
//! unit tests of `UiScale` itself pass either way.
//!
//! Since `ScaleAndCenter` (Sep 2026) the projection, the pointer and the clip
//! rects carry an offset as well as a scale ([`UiScale::factor`] alone is no
//! longer the whole story), so the tests below require the shared helpers and
//! refuse the hand-written arithmetic each host used to carry.
//!
//! That is the shape of every bug in this file's neighbours - the GUI scroll
//! that only two runtimes called, the patrol library (BUGS #7), the input
//! mirror (#9), `update_ui_buttons` which no runtime called at all (#61).
//!
//! [`UiScale::virtual_size`]: jengine_core::ui::scaling::UiScale::virtual_size
//! [`UiScale::factor`]: jengine_core::ui::scaling::UiScale::factor

mod support;

use jengine_core::ui::gui_document::load_gui;
use jengine_core::ui::layout::{compute_layout, rect_of};
use jengine_core::ui::scaling::{UiScale, UiScaleMode};

static NATIVE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs")));
static VIEWPORT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs")));
static WEB: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs")));

fn runtimes() -> [(&'static str, &'static str); 3] {
    [("native", NATIVE.as_str()), ("viewport", VIEWPORT.as_str()), ("web", WEB.as_str())]
}

#[test]
fn every_runtime_lays_the_ui_out_in_virtual_pixels() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("ui_scale.virtual_size(") || src.contains(".virtual_size("),
            "{name} never asks for the virtual UI size, so its UI is laid out in \
             raw window pixels and a document authored for a larger window runs \
             off the edges there"
        );
    }
}

#[test]
fn every_runtime_builds_its_ui_projection_through_the_scale() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("ui_scale.projection("),
            "{name}'s UI projection does not come from `UiScale::projection`, so \
             the layout and the matrix disagree about where the UI is"
        );
        assert!(
            !src.contains("orthographic_rh_gl(0.0, uw, 0.0, uh"),
            "{name} builds its own UI projection, which knows the scale but not \
             the `ScaleAndCenter` offset - a centred UI draws in the corner"
        );
    }
}

/// The pointer has to be divided into the same space.
///
/// A UI that draws scaled but hit-tests unscaled is worse than one that does
/// neither: the widget is visibly under the cursor and does not respond.
#[test]
fn every_runtime_hit_tests_the_ui_in_virtual_pixels() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("ui_scale.to_ui("),
            "{name} hit-tests UI with its own pointer arithmetic while drawing it \
             scaled, so clicks land somewhere the widget is not"
        );
        assert!(
            !src.contains("mouse_position.0 / ui_factor")
                && !src.contains("mouse_position.0 / factor")
                && !src.contains("mouse_position.0 / ui_pointer_factor"),
            "{name} divides the pointer by the factor itself, which misses the \
             `ScaleAndCenter` offset - a centred UI answers clicks beside itself"
        );
    }
}

/// And the scissor has to be converted back.
///
/// `set_scissor_rect` is the one place that wants physical pixels; a clip rect
/// left in virtual ones crops a scrolling container to the wrong box.
#[test]
fn every_runtime_converts_clip_rects_back_to_physical_pixels() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("ui_scale.scissor("),
            "{name} does not convert clip rects with `UiScale::scissor`, which \
             `set_scissor_rect` needs in physical pixels - a scroll container \
             clips to the wrong box"
        );
        assert!(
            !src.contains("r.x * ui_factor"),
            "{name} converts clip rects itself, missing the `ScaleAndCenter` offset"
        );
    }
}

/// Every place the mode is read from `project.jengine` knows every mode.
#[test]
fn every_runtime_reads_the_mode_the_same_way() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("UiScaleMode::from_setting("),
            "{name} parses `ui_scale_mode` itself, and so treats any mode it was \
             not written with - `ScaleAndCenter` - as the default"
        );
    }
}

/// The feature has to actually solve the case it was built for.
///
/// Scene 33 needs 860 virtual pixels of width; the editor's viewport panel is
/// narrower than that, which is how the third column came to be cut in half.
#[test]
fn a_document_wider_than_the_window_still_fits_once_scaled() {
    // Three 300-wide columns, 24 gaps, 16 padding either side = 980.
    let doc_json = r#"{
      "name": "three columns",
      "anchor": "Center",
      "root": {
        "id": "root", "node_type": { "type": "Panel" },
        "style": {
          "width": {"type":"Auto"}, "height": {"type":"Auto"},
          "padding": {"top":0.0,"right":16.0,"bottom":0.0,"left":16.0},
          "flex": {"direction":"Row","gap":24.0,"align_items":"Start","justify_content":"Center"}
        },
        "children": [
          {"id":"a","node_type":{"type":"Panel"},"style":{"width":{"type":"Fixed","value":300.0},"height":{"type":"Fixed","value":400.0}},"children":[]},
          {"id":"b","node_type":{"type":"Panel"},"style":{"width":{"type":"Fixed","value":300.0},"height":{"type":"Fixed","value":400.0}},"children":[]},
          {"id":"c","node_type":{"type":"Panel"},"style":{"width":{"type":"Fixed","value":300.0},"height":{"type":"Fixed","value":400.0}},"children":[]}
        ]
      }
    }"#;
    let doc = load_gui(doc_json).expect("parse");
    let scale = UiScale::from_reference(1280.0, 720.0);

    // The editor's viewport panel, roughly.
    let (screen_w, screen_h) = (850.0, 610.0);

    // Unscaled - what every runtime used to do - the last column is off the
    // right-hand edge.
    let unscaled = compute_layout(&doc, screen_w, screen_h);
    let last = rect_of(&unscaled, "c").expect("column c");
    assert!(
        last.x + last.width > screen_w,
        "this fixture is supposed to overflow an 850px window"
    );

    // Scaled, the same document fits, because layout sees a virtual surface
    // large enough to hold it and the projection shrinks the result.
    let (vw, vh) = scale.virtual_size(screen_w, screen_h);
    let scaled = compute_layout(&doc, vw, vh);
    let last = rect_of(&scaled, "c").expect("column c");
    let factor = scale.factor(screen_w, screen_h);
    assert!(
        (last.x + last.width) * factor <= screen_w + 0.5,
        "still off the edge: right edge {} in a {screen_w}px window",
        (last.x + last.width) * factor
    );
}

/// A click at the place a node is *drawn* must hit that node.
///
/// This is the composition every runtime performs: divide the physical pointer
/// by the factor, hit-test against a layout computed at the virtual size. Get
/// the direction of that division wrong and the UI is clickable at a mirrored
/// or magnified offset from where it appears - visibly under the cursor and
/// not responding.
#[test]
fn a_click_where_the_button_is_drawn_hits_the_button() {
    let doc_json = r#"{
      "name": "one button",
      "anchor": "TopLeft",
      "root": {
        "id": "root", "node_type": { "type": "Panel" },
        "style": {
          "width": {"type":"Percent","value":100.0},
          "height": {"type":"Percent","value":100.0},
          "padding": {"top":100.0,"right":0.0,"bottom":0.0,"left":200.0}
        },
        "children": [
          {"id":"btn","node_type":{"type":"Button","label":"click me"},
           "style":{"width":{"type":"Fixed","value":340.0},"height":{"type":"Fixed","value":40.0}},
           "children":[]}
        ]
      }
    }"#;
    let doc = load_gui(doc_json).expect("parse");
    let scale = UiScale::from_reference(1280.0, 720.0);

    // The window from the screenshot that found this.
    let (pw, ph) = (810.0, 498.0);
    let factor = scale.factor(pw, ph);
    assert!(factor < 1.0, "this window is smaller than the authored size");

    let (vw, vh) = scale.virtual_size(pw, ph);
    let layout = compute_layout(&doc, vw, vh);
    let rect = rect_of(&layout, "btn").expect("button");

    // Where the button actually appears on the physical surface.
    let drawn_x = (rect.x + rect.width / 2.0) * factor;
    let drawn_y = (rect.y + rect.height / 2.0) * factor;
    assert!(
        drawn_x < pw && drawn_y < ph,
        "the fixture should be drawing on screen: ({drawn_x}, {drawn_y})"
    );

    // What a runtime does with the pointer at that spot.
    let hit = layout
        .iter()
        .find(|e| e.id == "btn")
        .expect("button")
        .contains(drawn_x / factor, drawn_y / factor);
    assert!(hit, "a click where the button is drawn missed it");

    // And the un-divided pointer must NOT hit, or the division is a no-op and
    // this test proves nothing.
    let unscaled_miss = !layout
        .iter()
        .find(|e| e.id == "btn")
        .expect("button")
        .contains(drawn_x, drawn_y);
    assert!(
        unscaled_miss,
        "the raw pointer hits too, so this fixture cannot tell the two apart"
    );
}

/// Opting out must be exactly the old behaviour, not nearly.
///
/// A project whose scripts already fit their own UI - one game's
/// `equipment_ui.lua` computes `min(sw / PANEL_W, sh / PANEL_H, 1.0)` by hand -
/// would otherwise fit it twice.
#[test]
fn constant_pixel_size_lays_out_exactly_as_before() {
    let scale = UiScale { mode: UiScaleMode::ConstantPixelSize, ..UiScale::default() };
    for (w, h) in [(850.0, 610.0), (1920.0, 1080.0), (320.0, 240.0)] {
        assert_eq!(scale.factor(w, h), 1.0);
        assert_eq!(scale.virtual_size(w, h), (w, h));
    }
}

/// Every host tells `UiScale` where a notch covers the screen.
///
/// The UI and the touch controls lay themselves out clear of
/// `UiScale::safe`, so the only way to get this wrong is a host that never
/// sets it: the UI then sits under the notch on that platform and nothing
/// says so. Each sets it from its own source - the native runner from the
/// Android display cut-out (zero on a desktop), the web runtime from the
/// browser's `env(safe-area-inset-*)`, the editor from its device preview.
#[test]
fn every_runtime_learns_the_safe_area() {
    const ANDROID: &str = include_str!("../../jengine-runtime/src/android.rs");
    const RUNTIME_LIB: &str = include_str!("../../jengine-runtime/src/lib.rs");
    assert!(NATIVE.as_str().contains("fn refresh_safe_insets("), "native never reads the safe area");
    assert!(
        NATIVE.as_str().contains("refresh_safe_insets(state)") && RUNTIME_LIB.contains("refresh_safe_insets(&mut s)"),
        "native reads the safe area but not at startup, resume and rotation"
    );
    assert!(
        NATIVE.as_str().contains("services.ui_scale.safe = insets"),
        "native tells the renderer about the notch and not the script system that hit-tests `.gui` clicks"
    );
    assert!(ANDROID.contains("getDisplayCutout"), "Android never asks for its display cut-out");
    assert!(WEB.as_str().contains("read_safe_insets(") && WEB.as_str().contains("safe-area-inset"), "web never reads env(safe-area-inset-*)");
    assert!(
        VIEWPORT.as_str().contains("pub fn viewport_set_safe_area(") && VIEWPORT.as_str().contains("apply_safe_fraction(app)"),
        "the editor has no device preview safe area"
    );
}
