//! A disabled `.gui` button refuses the click, in every runtime.
//!
//! `GuiNodeType::Button` has carried a `disabled` flag since the format was
//! written. It serialized, it loaded, the GUI editor offered it - and
//! nothing anywhere read it. A disabled button drew exactly like an enabled
//! one and fired its callback exactly like an enabled one, so a shop could
//! neither show nor enforce what the player could not afford. Every game
//! that wanted the effect hid the button instead, which is not the same
//! thing: "you cannot afford this" and "this does not exist" read very
//! differently to a player.
//!
//! The other half is that `gui_set` could not write the flag, so the
//! affordability of a row could not change while the game ran.
//!
//! There are three hit-testing sites - native (`process_gui_clicks`), the
//! editor viewport and the web runtime - and each computes its own layout.
//! This checks the shared decision, and that each site asks for it.

use jengine_core::ui::gui_document::{GuiDocument, GuiNode, GuiNodeType, apply_gui_node_prop, load_gui, save_gui};

fn button(id: &str, disabled: bool) -> GuiNode {
    GuiNode {
        id: id.into(),
        node_type: GuiNodeType::Button {
            label: "BUY".into(),
            font_id: String::new(),
            font_size: 16.0,
            text_color: [1.0; 4],
            normal_color: [0.2, 0.2, 0.2, 1.0],
            hover_color: [0.3, 0.3, 0.3, 1.0],
            pressed_color: [0.4, 0.4, 0.4, 1.0],
            disabled,
            alignment: Default::default(),
            hover_scale: 1.0,
        },
        style: Default::default(),
        children: vec![],
    }
}

fn doc() -> GuiDocument {
    let mut d = GuiDocument::default();
    d.root.children = vec![button("btn_on", false), button("btn_off", true), {
        let mut p = GuiNode::new_panel("panel");
        p.children = vec![button("btn_nested_off", true)];
        p
    }];
    d
}

#[test]
fn a_disabled_button_refuses_the_click_and_an_enabled_one_does_not() {
    let d = doc();
    assert!(!d.refuses_clicks("btn_on"));
    assert!(d.refuses_clicks("btn_off"));
    assert!(d.refuses_clicks("btn_nested_off"), "the check is depth-first, not root-only");
    // A node that is not there cannot be clicked either - a handler
    // registered against a stale id must not fire on a rect that no longer
    // exists.
    assert!(d.refuses_clicks("btn_gone"));
}

#[test]
fn a_panel_is_not_a_button_and_never_refuses() {
    // Only a button has the flag. A panel is not clickable in the first
    // place, but a handler *can* be registered against one, and turning
    // every non-button into "refuses clicks" would silently break that.
    let d = doc();
    assert!(!d.refuses_clicks("panel"));
    assert!(!d.refuses_clicks("root"));
}

#[test]
fn gui_set_can_turn_a_button_off_and_on_again() {
    let mut node = button("b", false);
    assert!(apply_gui_node_prop(&mut node, "disabled", &serde_json::json!(true)));
    assert!(node.refuses_clicks());
    // Scripts stringify, and "1"/"0" arrive as numbers - both spellings work
    // for every other flag, so both must work here.
    assert!(apply_gui_node_prop(&mut node, "disabled", &serde_json::json!("false")));
    assert!(!node.refuses_clicks());
    assert!(apply_gui_node_prop(&mut node, "disabled", &serde_json::json!(1)));
    assert!(node.refuses_clicks());
}

#[test]
fn gui_set_can_recolour_a_button_for_each_state() {
    // The colour a shop row wants to change is the button's own background,
    // not the panel behind it.
    let mut node = button("b", false);
    for prop in ["normal_color", "hover_color", "pressed_color"] {
        assert!(
            apply_gui_node_prop(&mut node, prop, &serde_json::json!([0.9, 0.1, 0.1, 1.0])),
            "{prop} was rejected"
        );
    }
    let GuiNodeType::Button { normal_color, hover_color, pressed_color, .. } = &node.node_type else {
        panic!("not a button");
    };
    for c in [normal_color, hover_color, pressed_color] {
        assert_eq!(*c, [0.9, 0.1, 0.1, 1.0]);
    }
}

#[test]
fn the_flag_survives_a_save_and_load() {
    let loaded = load_gui(&save_gui(&doc())).expect("round trip");
    assert!(loaded.refuses_clicks("btn_off"));
    assert!(!loaded.refuses_clicks("btn_on"));
}

/// The ratchet: every runtime hit-tests through the one function, and that
/// function consults the document. Each used to carry its own hit test - this
/// test only insisted each called `refuses_clicks` - until Oct 2026.
#[test]
fn every_hit_test_asks_the_document() {
    let layout = include_str!("../src/ui/layout.rs");
    let at = layout.find("pub fn click_hits").expect("ui::layout::click_hits");
    assert!(layout[at..].contains("refuses_clicks"), "click_hits does not ask whether a node is disabled");
    for (path, what) in [
        ("src/scripting/api.rs", "native process_gui_clicks"),
        ("../jengine-editor-viewport/src/lib.rs", "the editor viewport"),
        ("../jengine-web/src/script_host.rs", "the web runtime"),
    ] {
        let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(src.contains("click_hits("), "{what} hit-tests gui clicks for itself again ({path})");
    }
}

/// And the renderer has to draw the difference, or the refusal is invisible.
#[test]
fn the_renderer_dims_a_disabled_button() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ecs/wgpu_systems.rs"),
    )
    .unwrap();
    let button_arm = src
        .split("GuiNodeType::Button {")
        .nth(1)
        .expect("the gui button draw arm");
    let arm = &button_arm[..button_arm.len().min(1200)];
    assert!(
        arm.contains("disabled") && arm.contains("dimmed"),
        "the .gui button draw arm ignores `disabled`, so a refused click looks like a dead one"
    );
}

/// Every runtime asks one function which bound nodes a click hit
/// (`ui::layout::click_hits`, Oct 2026) - it was three hit tests, two of
/// which laid a document out again for every bound node.
#[test]
fn a_click_hits_only_bound_enabled_nodes_of_shown_documents() {
    use jengine_core::asset::AssetManager;
    use jengine_core::ui::layout::{click_hits, compute_layout};
    let mut assets = AssetManager::new();
    assets.insert_gui("shop".into(), doc());
    assets.insert_gui("hidden".into(), doc());
    let size = (800.0, 600.0);
    let centre = |id: &str| {
        let e = compute_layout(&doc(), size.0, size.1).into_iter().find(|e| e.id == id).unwrap();
        (e.rect.x + e.rect.width / 2.0, e.rect.y + e.rect.height / 2.0)
    };
    let visible: std::collections::HashSet<String> = ["shop".to_string()].into();
    let bound = [("shop", "btn_on"), ("shop", "btn_off"), ("hidden", "btn_on"), ("missing", "btn_on")];
    assert_eq!(
        click_hits(bound, &visible, &assets, centre("btn_on"), size),
        vec![("shop".to_string(), "btn_on".to_string())],
        "the enabled button of the shown document, and nothing else"
    );
    assert!(click_hits(bound, &visible, &assets, centre("btn_off"), size).is_empty(), "a disabled button refuses");
    assert!(click_hits(bound, &visible, &assets, (-50.0, -50.0), size).is_empty(), "a miss hits nothing");
}

/// Native's mouse path goes through `click_hits` too: a release over a bound,
/// enabled button of a shown document fires its handler; over a disabled one,
/// in a hidden document, or mid-scroll, nothing does.
#[test]
fn native_fires_a_click_released_over_the_button() {
    use jengine_core::asset::AssetManager;
    use jengine_core::scripting::api::{ScriptInput, ScriptServices};
    use jengine_core::ui::layout::compute_layout;
    let mut assets = AssetManager::new();
    assets.insert_gui("shop".into(), doc());
    let size = (800.0, 600.0);
    let centre = |id: &str| {
        let e = compute_layout(&doc(), size.0, size.1).into_iter().find(|e| e.id == id).unwrap();
        (e.rect.x + e.rect.width / 2.0, e.rect.y + e.rect.height / 2.0)
    };
    let mut services = ScriptServices::default();
    services.gui_on_click(3, "shop".into(), "btn_on".into(), "on_buy".into());
    services.gui_on_click(3, "shop".into(), "btn_off".into(), "on_sold_out".into());
    let shown: std::collections::HashSet<String> = ["shop".to_string()].into();
    let release = |at: (f32, f32), dragging: bool| ScriptInput {
        mouse_position: at,
        mouse_left_just_released: true,
        gui_scroll_dragging: dragging,
        screen_size: size,
        ..Default::default()
    };
    services.process_gui_clicks(&release(centre("btn_on"), false), &assets, &shown);
    assert_eq!(services.fired_gui_clicks, [(3, "on_buy".to_string(), "btn_on".to_string())]);
    services.process_gui_clicks(&release(centre("btn_off"), false), &assets, &shown);
    assert!(services.fired_gui_clicks.is_empty(), "a disabled button refuses");
    services.process_gui_clicks(&release(centre("btn_on"), false), &assets, &Default::default());
    assert!(services.fired_gui_clicks.is_empty(), "a hidden document is not clicked");
    services.process_gui_clicks(&release(centre("btn_on"), true), &assets, &shown);
    assert!(services.fired_gui_clicks.is_empty(), "the release that ends a scroll is not a click");
}
