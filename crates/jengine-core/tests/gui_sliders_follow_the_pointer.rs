//! **A `.gui` slider follows the pointer** (Oct 2026). It could be focused
//! and nudged with the arrows, and drew a thumb, but nothing moved it by
//! mouse or touch - so Hold'em Night's raise had to be `-` / `+` steps. An
//! entity-UI slider always dragged. Now a press on the slider takes it, the
//! thumb is wherever the pointer is along it (also past its ends, clamped),
//! and letting go lets go. It runs in `navigate_ui`, which every host calls.

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::GuiComponent;
use jengine_core::ecs::systems::navigate_ui;
use jengine_core::input::InputState;
use jengine_core::ui::gui_document::{GuiNodeType, load_gui};
use jengine_core::ui::layout::{compute_layout, rect_of};
use jengine_core::ui::scaling::UiScale;

const W: f32 = 400.0;
const H: f32 = 200.0;

const DOC: &str = r#"{
  "name": "Raise",
  "anchor": "TopLeft",
  "root": {
    "id": "root",
    "node_type": { "type": "Panel" },
    "style": {
      "width": { "type": "Percent", "value": 100.0 },
      "height": { "type": "Percent", "value": 100.0 },
      "padding": { "top": 40.0, "right": 40.0, "bottom": 40.0, "left": 40.0 },
      "flex": { "direction": "Column", "gap": 20.0, "align_items": "Start", "justify_content": "Start" }
    },
    "children": [
      { "id": "size", "node_type": { "type": "Slider", "value": 50.0, "min": 20.0, "max": 220.0 },
        "style": { "width": { "type": "Fixed", "value": 200.0 }, "height": { "type": "Fixed", "value": 24.0 } },
        "children": [] },
      { "id": "below", "node_type": { "type": "Panel" },
        "style": { "width": { "type": "Fixed", "value": 200.0 }, "height": { "type": "Fixed", "value": 24.0 } },
        "children": [] }
    ]
  }
}"#;

fn value(assets: &AssetManager) -> f32 {
    match &assets.get_gui("raise.gui").unwrap().root.find_node("size").unwrap().node_type {
        GuiNodeType::Slider { value, .. } => *value,
        _ => unreachable!(),
    }
}

fn frame(world: &mut World, assets: &mut AssetManager, input: &mut InputState, at: (f32, f32), down: bool) {
    let was = input.mouse_left;
    input.mouse_position = at;
    input.mouse_left = down;
    input.mouse_left_just_pressed = down && !was;
    navigate_ui(world, assets, input, &UiScale::constant(), 0.016);
}

#[test]
fn a_slider_is_dragged_by_the_pointer_and_let_go() {
    let mut world = World::new();
    let mut assets = AssetManager::new();
    assets.insert_gui("raise.gui".into(), load_gui(DOC).unwrap());
    world.spawn((GuiComponent { gui_id: "raise.gui".into(), ..Default::default() },));
    let mut input = InputState::default();
    input.screen_size = (W, H);
    let doc = load_gui(DOC).unwrap();
    let r = rect_of(&compute_layout(&doc, W, H), "size").unwrap();
    let mid_y = r.y + r.height * 0.5;

    // Pressed a quarter of the way along: the value is a quarter of the range.
    frame(&mut world, &mut assets, &mut input, (r.x + r.width * 0.25, mid_y), true);
    assert!((value(&assets) - 70.0).abs() < 0.01, "a quarter of 20..220, got {}", value(&assets));
    // Dragged past the end and below the slider: still held, clamped.
    frame(&mut world, &mut assets, &mut input, (r.x + r.width * 1.5, mid_y + 60.0), true);
    assert_eq!(value(&assets), 220.0);
    // Let go, then the pointer moves with the button up: nothing changes.
    frame(&mut world, &mut assets, &mut input, (r.x + r.width * 1.5, mid_y + 60.0), false);
    frame(&mut world, &mut assets, &mut input, (r.x, mid_y), false);
    assert_eq!(value(&assets), 220.0);
}

#[test]
fn a_press_elsewhere_never_takes_a_slider() {
    let mut world = World::new();
    let mut assets = AssetManager::new();
    assets.insert_gui("raise.gui".into(), load_gui(DOC).unwrap());
    world.spawn((GuiComponent { gui_id: "raise.gui".into(), ..Default::default() },));
    let mut input = InputState::default();
    input.screen_size = (W, H);
    let doc = load_gui(DOC).unwrap();
    let layout = compute_layout(&doc, W, H);
    let below = rect_of(&layout, "below").unwrap();
    let r = rect_of(&layout, "size").unwrap();
    // Pressed on the panel under it, then dragged across the slider held.
    frame(&mut world, &mut assets, &mut input, (below.x + 10.0, below.y + 10.0), true);
    frame(&mut world, &mut assets, &mut input, (r.x + r.width * 0.9, r.y + r.height * 0.5), true);
    assert_eq!(value(&assets), 50.0, "a drag that began elsewhere is not the slider's");
}
