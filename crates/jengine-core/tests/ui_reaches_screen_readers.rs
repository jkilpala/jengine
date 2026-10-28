//! **Menus a screen reader can read and work** (Oct 2026;
//! `ui::accessibility`, AccessKit natively, a hidden ARIA DOM in the
//! browsers). Nothing in the UI reached the platform's accessibility API, so
//! a blind player met a silent window. Now the UI is gathered into a tree of
//! named, placed nodes once a frame inside `navigate_ui`, and a screen
//! reader's "focus" and "press" come back through the same navigation a
//! D-pad uses - so a press reaches a script as any click does. Found by the
//! showcase's Readable Menus.
#![cfg(feature = "accessibility")]

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::{GuiComponent, Parent};
use jengine_core::ecs::systems::navigate_ui;
use jengine_core::input::InputState;
use jengine_core::ui::accessibility::{self, Accessible, Action, Role};
use jengine_core::ui::gui_document::load_gui;
use jengine_core::ui::scaling::UiScale;
use jengine_core::ui::{UIButton, UIElement, UIText, UIToggle};

fn input() -> InputState {
    let mut i = InputState::new();
    i.screen_size = (1280.0, 720.0);
    i
}

fn element(x: f32, y: f32, focusable: bool) -> UIElement {
    serde_json::from_value(serde_json::json!({
        "anchor": "TopLeft", "offset": [x, y], "size": [200.0, 50.0], "focusable": focusable
    }))
    .unwrap()
}

fn text(s: &str) -> UIText {
    serde_json::from_value(serde_json::json!({ "text": s })).unwrap()
}

fn frame(world: &mut World, assets: &mut AssetManager) -> jengine_core::ecs::systems::NavOutcome {
    let mut i = input();
    navigate_ui(world, assets, &mut i, &UiScale::default(), 0.016)
}

/// An entity-UI menu: a title, a Play button named by its child text, an
/// icon button named by `accessible`, a toggle, and a hidden decoration.
fn menu() -> (World, AssetManager, hecs::Entity, hecs::Entity) {
    let mut world = World::new();
    world.spawn((element(100.0, -40.0, false), text("Main menu")));
    let play = world.spawn((element(100.0, -120.0, true), UIButton::default()));
    world.spawn((element(0.0, 0.0, false), text("Play"), Parent(play)));
    let gear = world.spawn((
        element(100.0, -200.0, true),
        UIButton::default(),
        Accessible { label: "Settings".into(), description: "Sound and controls".into(), hidden: false },
    ));
    world.spawn((element(100.0, -280.0, true), text("Subtitles"), UIToggle { checked: true, ..Default::default() }));
    world.spawn((element(400.0, -40.0, false), text("~~~"), Accessible { hidden: true, ..Default::default() }));
    world.spawn((element(400.0, -400.0, false), text("")));
    (world, AssetManager::new(), play, gear)
}

#[test]
fn entity_ui_reads_in_order_with_names_and_roles() {
    let (mut world, mut assets, play, _) = menu();
    frame(&mut world, &mut assets);
    let (tree, _) = accessibility::latest();
    let read: Vec<(Role, &str)> = tree.nodes.iter().map(|n| (n.role, n.label.as_str())).collect();
    assert_eq!(
        read,
        vec![(Role::Label, "Main menu"), (Role::Button, "Play"), (Role::Button, "Settings"), (Role::Toggle, "Subtitles")],
        "top to bottom, a button named by its child's text, the label not read twice, decoration hidden"
    );
    let settings = &tree.nodes[2];
    assert_eq!(settings.description, "Sound and controls");
    assert_eq!(tree.nodes[3].toggled, Some(true));
    // Places in physical pixels, y down.
    let p = tree.nodes.iter().find(|n| n.id == play.to_bits().get()).unwrap();
    assert_eq!(p.bounds, [100.0, 120.0, 200.0, 50.0], "{:?}", p.bounds);
    assert!(p.focusable);
}

#[test]
fn a_screen_reader_focuses_and_presses_through_navigation() {
    let (mut world, mut assets, play, gear) = menu();
    frame(&mut world, &mut assets);
    accessibility::request(Action::Focus(gear.to_bits().get()));
    frame(&mut world, &mut assets);
    assert!(world.get::<&UIElement>(gear).unwrap().focused, "focus moved where it was asked");
    assert_eq!(accessibility::focused_speech(), "Settings, button, Sound and controls", "what a reader says for it");
    let (tree, _) = accessibility::latest();
    assert_eq!(tree.focus, Some(gear.to_bits().get()), "and the tree says so");
    accessibility::request(Action::Click(play.to_bits().get()));
    let out = frame(&mut world, &mut assets);
    assert_eq!(out.ui_clicks, vec![play.id() as u64], "a press is a click, delivered as navigation's are");
    assert!(world.get::<&UIElement>(play).unwrap().focused);
}

#[test]
fn a_gui_documents_buttons_and_text_are_read_and_pressed() {
    let doc = r#"{ "name": "Pause", "anchor": "TopLeft",
        "root": { "id": "root", "node_type": { "type": "Panel" },
          "style": { "width": { "type": "Percent", "value": 100.0 }, "height": { "type": "Percent", "value": 100.0 } },
          "children": [
            { "id": "title", "node_type": { "type": "Text", "text": "Paused", "font_id": "", "font_size": 20.0 },
              "style": { "constraints": { "left": { "type": "Px", "value": 50.0 }, "top": { "type": "Px", "value": 20.0 } },
                         "width": { "type": "Fixed", "value": 200.0 }, "height": { "type": "Fixed", "value": 30.0 } }, "children": [] },
            { "id": "resume", "node_type": { "type": "Button", "label": "Resume", "font_id": "", "font_size": 12.0 },
              "style": { "focusable": true, "constraints": { "left": { "type": "Px", "value": 50.0 }, "top": { "type": "Px", "value": 80.0 } },
                         "width": { "type": "Fixed", "value": 100.0 }, "height": { "type": "Fixed", "value": 40.0 } }, "children": [] },
            { "id": "music_slider", "node_type": { "type": "Slider", "value": 3.0, "min": 0.0, "max": 4.0 },
              "style": { "focusable": true, "accessible": { "label": "Music volume" },
                         "constraints": { "left": { "type": "Px", "value": 50.0 }, "top": { "type": "Px", "value": 140.0 } },
                         "width": { "type": "Fixed", "value": 200.0 }, "height": { "type": "Fixed", "value": 20.0 } }, "children": [] },
            { "id": "flourish", "node_type": { "type": "Text", "text": "* * *", "font_id": "", "font_size": 20.0 },
              "style": { "accessible": { "hidden": true },
                         "constraints": { "left": { "type": "Px", "value": 50.0 }, "top": { "type": "Px", "value": 180.0 } },
                         "width": { "type": "Fixed", "value": 200.0 }, "height": { "type": "Fixed", "value": 30.0 } }, "children": [] }
          ] } }"#;
    let mut assets = AssetManager::new();
    assets.insert_gui("pause.gui".into(), load_gui(doc).unwrap());
    let mut world = World::new();
    let holder = world.spawn((GuiComponent { gui_id: "pause.gui".into(), ..Default::default() },));
    frame(&mut world, &mut assets);
    let (tree, _) = accessibility::latest();
    let resume = tree.nodes.iter().find(|n| n.label == "Resume").expect("the button is in the tree");
    assert_eq!(resume.role, Role::Button);
    assert_eq!(resume.bounds, [50.0, 80.0, 100.0, 40.0]);
    assert!(tree.nodes.iter().any(|n| n.role == Role::Label && n.label == "Paused"));
    let slider = tree.nodes.iter().find(|n| n.role == Role::Slider).expect("the slider is in the tree");
    assert_eq!(slider.label, "Music volume", "named by style.accessible, not by its id");
    assert_eq!(slider.value.as_deref(), Some("75%"), "a value is read as how far along its range");
    assert!(!tree.nodes.iter().any(|n| n.label == "* * *"), "decoration is skipped");
    // "Increase" on the slider: a tenth of its range, as right is.
    accessibility::request(Action::Step(slider.id, 1));
    frame(&mut world, &mut assets);
    let after = accessibility::latest().0;
    let slid = after.nodes.iter().find(|n| n.role == Role::Slider).unwrap();
    assert_eq!(slid.value.as_deref(), Some("85%"));
    assert_eq!(after.focus, Some(slider.id), "and it is where focus is now");
    accessibility::request(Action::Click(resume.id));
    let out = frame(&mut world, &mut assets);
    assert_eq!(out.gui_activations, vec![("pause.gui".to_string(), "resume".to_string())]);
    assert_eq!(world.get::<&GuiComponent>(holder).unwrap().focused_node_id.as_deref(), Some("resume"));
}

#[test]
fn an_announcement_reaches_the_tree_each_time_it_is_made() {
    let (mut world, mut assets, _, _) = menu();
    frame(&mut world, &mut assets);
    let (before, generation) = accessibility::latest();
    accessibility::announce("Saved");
    frame(&mut world, &mut assets);
    let (after, later) = accessibility::latest();
    assert_eq!(after.announcement.1, "Saved");
    assert!(after.announcement.0 > before.announcement.0 && later > generation);
    accessibility::announce("Saved");
    frame(&mut world, &mut assets);
    assert!(accessibility::latest().0.announcement.0 > after.announcement.0, "the same words twice are said twice");
}

#[test]
fn the_native_runtime_hands_the_tree_on_and_shows_its_window_after() {
    let src = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
    assert!(src.contains("accessibility.update()"), "the native runtime never sends the tree to the screen reader");
    assert!(src.contains("accessibility.process_event("), "the adapter never sees window events");
    assert!(src.contains(".with_visible(false)"), "AccessKit needs the window hidden until its adapter exists");
}
