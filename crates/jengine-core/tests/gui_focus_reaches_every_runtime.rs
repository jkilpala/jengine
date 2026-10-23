//! Menus work without a pointer, in every runtime.
//!
//! Tab once was the whole of it - `update_gui_focus` cycled a document's
//! focusable nodes, and for a long time no host even called that. A game on a
//! controller has no pointer, so a menu has to be walked with a D-pad, a stick
//! or the arrow keys and pressed with A: `ecs::systems::navigate_ui`, which
//! every host calls once a frame, and whose presses each host delivers the way
//! it delivers a click. This checks the hosts call it and deliver both kinds
//! of press, and what it does.

use std::path::Path;

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::GuiComponent;
use jengine_core::ecs::systems::navigate_ui;
use jengine_core::input::{InputState, KeyCode};
use jengine_core::ui::gui_document::load_gui;
use jengine_core::ui::scaling::UiScale;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

#[test]
fn every_host_navigates_menus_and_delivers_the_presses() {
    for (host, path, gui_delivery) in [
        (
            "native runtime",
            "jengine-runtime/src/wgpu_scene_runner.rs",
            "pending_gui_activations.extend(nav.gui_activations)",
        ),
        ("editor viewport", "jengine-editor-viewport/src/lib.rs", "dispatch_gui_click_impl(app, gui_id, node_id)"),
        ("web runtime", "jengine-web/src/lib.rs", "dispatch_gui_click(app, gui_id, node_id)"),
    ] {
        let src = read(path);
        assert!(src.contains("navigate_ui("), "{host} does not call `navigate_ui`: no menu there can be used without a pointer");
        assert!(src.contains(gui_delivery), "{host} navigates but drops the `.gui` presses it returns");
        assert!(src.contains("nav.ui_clicks"), "{host} navigates but drops the entity-UI presses it returns");
        assert!(src.contains("apply_text_input("), "{host} lost its text input call");
    }
}

const SCREEN: (f32, f32) = (1280.0, 720.0);

fn fresh_input() -> InputState {
    let mut i = InputState::new();
    i.screen_size = SCREEN;
    i
}

fn button(id: &str, x: f32, y: f32) -> String {
    format!(
        r#"{{ "id": "{id}", "node_type": {{ "type": "Button", "label": "{id}", "font_id": "", "font_size": 12.0 }},
            "style": {{ "focusable": true, "constraints": {{ "left": {{ "type": "Px", "value": {x} }}, "top": {{ "type": "Px", "value": {y} }} }},
                       "width": {{ "type": "Fixed", "value": 100.0 }}, "height": {{ "type": "Fixed", "value": 40.0 }} }},
            "children": [] }}"#
    )
}

/// A 2 x 2 grid of buttons: a (top left), b (top right), c, d.
fn grid() -> (World, AssetManager, hecs::Entity) {
    let doc = format!(
        r#"{{ "name": "Grid", "anchor": "TopLeft",
             "root": {{ "id": "root", "node_type": {{ "type": "Panel" }},
               "style": {{ "width": {{ "type": "Percent", "value": 100.0 }}, "height": {{ "type": "Percent", "value": 100.0 }} }},
               "children": [{}, {}, {}, {}] }} }}"#,
        button("a", 100.0, 100.0),
        button("b", 300.0, 100.0),
        button("c", 100.0, 200.0),
        button("d", 300.0, 200.0),
    );
    let mut assets = AssetManager::new();
    assets.insert_gui("grid.gui".into(), load_gui(&doc).expect("the grid parses"));
    let mut world = World::new();
    let e = world.spawn((GuiComponent { gui_id: "grid.gui".into(), ..Default::default() },));
    (world, assets, e)
}

fn focus(world: &World, e: hecs::Entity) -> Option<String> {
    world.get::<&GuiComponent>(e).unwrap().focused_node_id.clone()
}

/// One frame with `key` pressed on it, then let go.
fn tap(world: &mut World, assets: &mut AssetManager, input: &mut InputState, key: KeyCode) -> jengine_core::ecs::systems::NavOutcome {
    input.key_down(key);
    let out = navigate_ui(world, assets, input, &UiScale::default(), 0.016);
    input.begin_frame();
    input.key_up(key);
    navigate_ui(world, assets, input, &UiScale::default(), 0.016);
    input.begin_frame();
    out
}

#[test]
fn arrows_walk_a_grid_and_enter_presses_the_focused_button() {
    let (mut world, mut assets, e) = grid();
    let mut input = fresh_input();
    // The first press only shows where focus is.
    let out = tap(&mut world, &mut assets, &mut input, KeyCode::Down);
    assert_eq!(focus(&world, e).as_deref(), Some("a"));
    assert!(out.gui_activations.is_empty());
    tap(&mut world, &mut assets, &mut input, KeyCode::Right);
    assert_eq!(focus(&world, e).as_deref(), Some("b"));
    tap(&mut world, &mut assets, &mut input, KeyCode::Down);
    assert_eq!(focus(&world, e).as_deref(), Some("d"));
    tap(&mut world, &mut assets, &mut input, KeyCode::Down);
    assert_eq!(focus(&world, e).as_deref(), Some("d"), "the bottom edge stops focus");
    let out = tap(&mut world, &mut assets, &mut input, KeyCode::Enter);
    assert_eq!(out.gui_activations, [("grid.gui".to_string(), "d".to_string())]);
}

#[test]
fn a_controller_drives_it_too_and_a_hidden_menu_takes_nothing() {
    let (mut world, mut assets, e) = grid();
    let mut input = fresh_input();
    let scale = UiScale::default();
    // Using the pad at all puts focus on the menu.
    input.update_gamepad(0, [13].into_iter().collect(), vec![0.0; 4]);
    navigate_ui(&mut world, &mut assets, &mut input, &scale, 0.016);
    assert_eq!(focus(&world, e).as_deref(), Some("a"));
    input.update_gamepad(0, Default::default(), vec![0.0; 4]);
    navigate_ui(&mut world, &mut assets, &mut input, &scale, 0.016);
    input.update_gamepad(0, [13].into_iter().collect(), vec![0.0; 4]);
    navigate_ui(&mut world, &mut assets, &mut input, &scale, 0.016);
    assert_eq!(focus(&world, e).as_deref(), Some("c"), "D-pad down");
    input.update_gamepad(0, [0].into_iter().collect(), vec![0.0; 4]);
    let out = navigate_ui(&mut world, &mut assets, &mut input, &scale, 0.016);
    assert_eq!(out.gui_activations, [("grid.gui".to_string(), "c".to_string())], "A presses it");

    let (mut world, mut assets, e) = grid();
    world.get::<&mut GuiComponent>(e).unwrap().visible = false;
    let mut input = fresh_input();
    let out = tap(&mut world, &mut assets, &mut input, KeyCode::Enter);
    assert_eq!(focus(&world, e), None);
    assert!(out.gui_activations.is_empty());
}

/// Enter, Space and A may be the game's own keys, so they never create focus,
/// and they press only what navigation focused - not a button a click
/// happened to focus. Otherwise a tower-defence game with its shop on screen
/// would buy a tower every time the player pressed Space to start a wave.
#[test]
fn a_games_own_keys_press_nothing_navigation_did_not_focus() {
    let (mut world, mut assets, e) = grid();
    let mut input = fresh_input();
    let out = tap(&mut world, &mut assets, &mut input, KeyCode::Space);
    assert_eq!(focus(&world, e), None, "Space alone shows no focus");
    assert!(out.gui_activations.is_empty());

    // A click focuses "b" (as clicking a focusable node does)...
    {
        let mut g = world.get::<&mut GuiComponent>(e).unwrap();
        g.focused_node_id = Some("b".into());
        g.nav_focus = false;
    }
    let out = tap(&mut world, &mut assets, &mut input, KeyCode::Space);
    assert!(out.gui_activations.is_empty(), "...and Space does not press it again");

    // Picking up the arrows makes it navigation's, and then Enter presses.
    tap(&mut world, &mut assets, &mut input, KeyCode::Down);
    assert_eq!(focus(&world, e).as_deref(), Some("d"));
    let out = tap(&mut world, &mut assets, &mut input, KeyCode::Enter);
    assert_eq!(out.gui_activations, [("grid.gui".to_string(), "d".to_string())]);

    // A pointer press hands it back.
    input.set_mouse_button(true, true);
    navigate_ui(&mut world, &mut assets, &mut input, &UiScale::default(), 0.016);
    input.set_mouse_button(true, false);
    input.begin_frame();
    let out = tap(&mut world, &mut assets, &mut input, KeyCode::Enter);
    assert!(out.gui_activations.is_empty());
}

/// A document whose script runs its own cursor opts out, and is left alone.
#[test]
fn a_document_that_is_not_navigable_is_left_alone() {
    let (mut world, mut assets, e) = grid();
    assets.get_gui_mut("grid.gui").unwrap().navigable = false;
    let mut input = fresh_input();
    tap(&mut world, &mut assets, &mut input, KeyCode::Down);
    tap(&mut world, &mut assets, &mut input, KeyCode::Tab);
    assert_eq!(focus(&world, e), None);
    // And it round-trips: `false` is written, `true` (the default) is not.
    let json = serde_json::to_string(assets.get_gui("grid.gui").unwrap()).unwrap();
    assert!(json.contains("\"navigable\":false"));
    assets.get_gui_mut("grid.gui").unwrap().navigable = true;
    let json = serde_json::to_string(assets.get_gui("grid.gui").unwrap()).unwrap();
    assert!(!json.contains("navigable"));
}

#[test]
fn tab_still_walks_the_document_order_and_wraps() {
    let (mut world, mut assets, e) = grid();
    let mut input = fresh_input();
    tap(&mut world, &mut assets, &mut input, KeyCode::Tab);
    assert_eq!(focus(&world, e).as_deref(), Some("a"));
    for want in ["b", "c", "d", "a"] {
        tap(&mut world, &mut assets, &mut input, KeyCode::Tab);
        assert_eq!(focus(&world, e).as_deref(), Some(want));
    }
    input.key_down(KeyCode::ShiftLeft);
    tap(&mut world, &mut assets, &mut input, KeyCode::Tab);
    assert_eq!(focus(&world, e).as_deref(), Some("d"), "Shift-Tab goes back");
}

#[test]
fn a_row_below_the_fold_is_scrolled_into_view() {
    let rows: Vec<String> = (0..10)
        .map(|i| {
            format!(
                r#"{{ "id": "row{i}", "node_type": {{ "type": "Button", "label": "{i}", "font_id": "", "font_size": 12.0 }},
                    "style": {{ "focusable": true, "width": {{ "type": "Fixed", "value": 200.0 }}, "height": {{ "type": "Fixed", "value": 40.0 }} }},
                    "children": [] }}"#
            )
        })
        .collect();
    let doc = format!(
        r#"{{ "name": "List", "anchor": "TopLeft",
             "root": {{ "id": "list", "node_type": {{ "type": "Panel" }},
               "style": {{ "overflow": "Scroll", "width": {{ "type": "Fixed", "value": 220.0 }}, "height": {{ "type": "Fixed", "value": 130.0 }},
                          "flex": {{ "direction": "Column", "gap": 10.0 }} }},
               "children": [{}] }} }}"#,
        rows.join(",")
    );
    let mut assets = AssetManager::new();
    assets.insert_gui("list.gui".into(), load_gui(&doc).expect("the list parses"));
    let mut world = World::new();
    let e = world.spawn((GuiComponent { gui_id: "list.gui".into(), ..Default::default() },));
    let mut input = fresh_input();
    let scroll = |assets: &AssetManager| assets.get_gui("list.gui").unwrap().root.style.scroll_y;

    tap(&mut world, &mut assets, &mut input, KeyCode::Down); // focus row0
    for _ in 0..5 {
        tap(&mut world, &mut assets, &mut input, KeyCode::Down);
    }
    assert_eq!(focus(&world, e).as_deref(), Some("row5"));
    // Row 5 spans 250..290; the view is 130 high, so at least 160 scrolled.
    assert!(scroll(&assets) >= 159.0, "scrolled {}", scroll(&assets));
    let layout = jengine_core::ui::layout::compute_layout(assets.get_gui("list.gui").unwrap(), SCREEN.0, SCREEN.1);
    let row = layout.iter().find(|r| r.id == "row5").unwrap();
    let clip = row.clip.expect("rows are clipped by the list");
    assert!(row.rect.y >= clip.y - 0.5 && row.rect.y + row.rect.height <= clip.y + clip.height + 0.5, "row 5 is fully in view");

    // And back up to the top scrolls back.
    for _ in 0..5 {
        tap(&mut world, &mut assets, &mut input, KeyCode::Up);
    }
    assert_eq!(scroll(&assets), 0.0);
}

#[test]
fn entity_ui_is_walked_pressed_and_slid() {
    use jengine_core::ui::{UIAnchor, UIButton, UIElement, UISlider};
    let el = |y: f32, focusable: bool| UIElement {
        anchor: UIAnchor::TopLeft,
        offset: glam::Vec2::new(100.0, -y),
        size: glam::Vec2::new(200.0, 40.0),
        focusable,
        ..Default::default()
    };
    let mut world = World::new();
    let first = world.spawn((el(100.0, true), UIButton::default()));
    let _hud = world.spawn((el(150.0, false), UIButton::default()));
    let slider = world.spawn((el(200.0, true), UISlider { value: 0.5, min: 0.0, max: 1.0, ..Default::default() }));
    let mut assets = AssetManager::new();
    let mut input = fresh_input();
    let focused = |w: &World, e: hecs::Entity| w.get::<&UIElement>(e).unwrap().focused;

    tap(&mut world, &mut assets, &mut input, KeyCode::Down);
    assert!(focused(&world, first));
    let out = tap(&mut world, &mut assets, &mut input, KeyCode::Space);
    assert_eq!(out.ui_clicks, [first.id() as u64]);
    tap(&mut world, &mut assets, &mut input, KeyCode::Down);
    assert!(focused(&world, slider), "the unfocusable button in between is skipped");
    tap(&mut world, &mut assets, &mut input, KeyCode::Right);
    assert!((world.get::<&UISlider>(slider).unwrap().value - 0.6).abs() < 1e-5, "right slides a focused slider");
    assert!(focused(&world, slider), "and does not move focus");

    // A pointer press lets go of focus.
    input.set_mouse_button(true, true);
    navigate_ui(&mut world, &mut assets, &mut input, &UiScale::default(), 0.016);
    assert!(!focused(&world, slider));
}

/// Native delivers a `.gui` press through `process_gui_clicks`, the same
/// handlers a click fires - and not for a hidden document or a disabled button.
#[test]
fn native_fires_a_queued_press_as_a_click() {
    use jengine_core::scripting::api::{ScriptInput, ScriptServices};
    let (_, assets, _) = grid();
    let mut services = ScriptServices::default();
    services.gui_on_click(7, "grid.gui".into(), "c".into(), "on_pick".into());
    services.pending_gui_activations.push(("grid.gui".into(), "c".into()));
    let visible = ["grid.gui".to_string()].into_iter().collect();
    services.process_gui_clicks(&ScriptInput::default(), &assets, &visible);
    assert_eq!(services.fired_gui_clicks, [(7, "on_pick".to_string(), "c".to_string())]);
    assert!(services.pending_gui_activations.is_empty());

    services.pending_gui_activations.push(("grid.gui".into(), "c".into()));
    services.process_gui_clicks(&ScriptInput::default(), &assets, &Default::default());
    assert!(services.fired_gui_clicks.is_empty(), "a hidden document is not pressed");
}

/// A focused `.gui` slider takes left and right as its value - a tenth of
/// its range a press - as an entity-UI slider always has; up and down still
/// move focus off it. Before, a controller could reach a `.gui` slider and
/// never move it. Found by the showcase's Readable Menus.
#[test]
fn a_focused_slider_slides_on_left_and_right() {
    let doc = format!(
        r#"{{ "name": "Settings", "anchor": "TopLeft",
             "root": {{ "id": "root", "node_type": {{ "type": "Panel" }},
               "style": {{ "width": {{ "type": "Percent", "value": 100.0 }}, "height": {{ "type": "Percent", "value": 100.0 }} }},
               "children": [{}, {}, {}] }} }}"#,
        r#"{ "id": "music", "node_type": { "type": "Slider", "value": 5.0, "min": 0.0, "max": 10.0 },
             "style": { "focusable": true, "constraints": { "left": { "type": "Px", "value": 100.0 }, "top": { "type": "Px", "value": 100.0 } },
                        "width": { "type": "Fixed", "value": 200.0 }, "height": { "type": "Fixed", "value": 20.0 } }, "children": [] }"#,
        button("beside", 400.0, 100.0),
        button("below", 100.0, 200.0),
    );
    let mut assets = AssetManager::new();
    assets.insert_gui("settings.gui".into(), load_gui(&doc).expect("the settings parse"));
    let mut world = World::new();
    let e = world.spawn((GuiComponent { gui_id: "settings.gui".into(), ..Default::default() },));
    let mut input = fresh_input();
    let value = |assets: &AssetManager| match &assets.get_gui("settings.gui").unwrap().root.find_node("music").unwrap().node_type {
        jengine_core::ui::gui_document::GuiNodeType::Slider { value, .. } => *value,
        _ => unreachable!(),
    };
    tap(&mut world, &mut assets, &mut input, KeyCode::Down);
    assert_eq!(focus(&world, e).as_deref(), Some("music"));
    tap(&mut world, &mut assets, &mut input, KeyCode::Right);
    tap(&mut world, &mut assets, &mut input, KeyCode::Right);
    assert_eq!(focus(&world, e).as_deref(), Some("music"), "right slid it rather than leaving for the button beside it");
    assert!((value(&assets) - 7.0).abs() < 1e-4, "{}", value(&assets));
    for _ in 0..9 {
        tap(&mut world, &mut assets, &mut input, KeyCode::Left);
    }
    assert_eq!(value(&assets), 0.0, "held at its end");
    tap(&mut world, &mut assets, &mut input, KeyCode::Down);
    assert_eq!(focus(&world, e).as_deref(), Some("below"), "down still leaves it");
}
