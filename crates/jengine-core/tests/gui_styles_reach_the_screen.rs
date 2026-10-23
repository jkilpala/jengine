//! **A `.gui` node can be rounded, faded, scaled and shadowed, and a button
//! shows its hover** (Sep 2026). A menu is made of these: a panel with soft
//! corners and a drop shadow, a whole screen fading in (`opacity`, inherited),
//! a button that pops when it is picked (`scale`, inherited and applied by
//! layout, so it is clicked where it is drawn). And `hover_color` /
//! `pressed_color` were authored on every `.gui` button and drawn by none -
//! `navigate_ui` now tracks the pointer in every runtime.
//!
//! Found by the showcase's Menus & HUD.
#![cfg(feature = "gpu-wgpu")]

use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::GuiComponent;
use jengine_core::ecs::systems::track_gui_pointer;
use jengine_core::ecs::wgpu_systems::{render_gui_wgpu, UiFlush};
use jengine_core::input::InputState;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::ui::gui_document::load_gui;
use jengine_core::ui::layout::{compute_layout, rect_of, LayoutResult};
use jengine_core::ui::scaling::UiScale;

const W: u32 = 320;
const H: u32 = 320;

const DOC: &str = r#"{
  "name": "Styles",
  "anchor": "TopLeft",
  "root": {
    "id": "root",
    "node_type": { "type": "Panel" },
    "style": {
      "width": { "type": "Percent", "value": 100.0 },
      "height": { "type": "Percent", "value": 100.0 },
      "padding": { "top": 20.0, "right": 20.0, "bottom": 20.0, "left": 20.0 },
      "bg_color": [0.1, 0.1, 0.1, 1.0],
      "flex": { "direction": "Column", "gap": 30.0, "align_items": "Start", "justify_content": "Start" }
    },
    "children": [
      { "id": "card", "node_type": { "type": "Panel" },
        "style": { "width": { "type": "Fixed", "value": 160.0 }, "height": { "type": "Fixed", "value": 80.0 },
                   "bg_color": [1.0, 1.0, 1.0, 1.0], "corner_radius": 24.0,
                   "shadow": { "color": [0.0, 0.0, 0.0, 1.0], "offset": [0.0, 14.0], "blur": 4.0 } },
        "children": [] },
      { "id": "ghost", "node_type": { "type": "Panel" },
        "style": { "width": { "type": "Fixed", "value": 160.0 }, "height": { "type": "Fixed", "value": 40.0 },
                   "bg_color": [1.0, 1.0, 1.0, 1.0], "opacity": 0.5,
                   "flex": { "direction": "Row", "gap": 0.0, "align_items": "Start", "justify_content": "Start" } },
        "children": [
          { "id": "ghost_inner", "node_type": { "type": "Panel" },
            "style": { "width": { "type": "Fixed", "value": 20.0 }, "height": { "type": "Fixed", "value": 20.0 }, "opacity": 0.5 },
            "children": [] }
        ] },
      { "id": "pop", "node_type": { "type": "Button", "label": "", "normal_color": [0.2, 0.2, 0.2, 1.0],
                                    "hover_color": [0.0, 1.0, 0.0, 1.0], "pressed_color": [0.0, 0.0, 1.0, 1.0] },
        "style": { "width": { "type": "Fixed", "value": 100.0 }, "height": { "type": "Fixed", "value": 40.0 }, "scale": 1.5,
                   "border_color": [1.0, 0.0, 0.0, 1.0], "border_width": 6.0 },
        "children": [] }
    ]
  }
}"#;

fn layout() -> LayoutResult {
    compute_layout(&load_gui(DOC).unwrap(), W as f32, H as f32)
}

#[test]
fn scale_and_opacity_are_inherited_and_laid_out() {
    let l = layout();
    let entry = |id: &str| l.iter().find(|e| e.id == id).unwrap();
    // `pop` is 100 x 40, scaled 1.5 about its centre.
    let pop = entry("pop");
    assert_eq!((pop.rect.width, pop.rect.height, pop.scale), (150.0, 60.0, 1.5));
    assert_eq!(entry("ghost").opacity, 0.5);
    assert_eq!(entry("ghost_inner").opacity, 0.25, "a child's opacity multiplies its parent's");
    assert_eq!((entry("card").opacity, entry("card").scale), (1.0, 1.0));
}

#[test]
fn a_button_is_hovered_where_it_is_drawn() {
    let mut world = World::new();
    let mut assets = AssetManager::new();
    assets.insert_gui("styles.gui".into(), load_gui(DOC).unwrap());
    let e = world.spawn((GuiComponent { gui_id: "styles.gui".into(), ..Default::default() },));
    let pop = rect_of(&layout(), "pop").unwrap();
    let mut input = InputState::default();
    input.screen_size = (W as f32, H as f32);
    // Inside the scaled button but outside the unscaled one.
    input.mouse_position = (pop.x + 14.0, pop.y + pop.height * 0.5);
    input.mouse_left = true;
    track_gui_pointer(&mut world, &assets, &input, &UiScale::constant(), 0.016);
    {
        let g = world.get::<&GuiComponent>(e).unwrap();
        assert_eq!(g.hovered_node_id.as_deref(), Some("pop"));
        assert!(g.pointer_down);
    }
    // Over a panel: nothing a hover colour applies to.
    let card = rect_of(&layout(), "card").unwrap();
    input.mouse_position = (card.x + 80.0, card.y + 40.0);
    track_gui_pointer(&mut world, &assets, &input, &UiScale::constant(), 0.016);
    assert_eq!(world.get::<&GuiComponent>(e).unwrap().hovered_node_id, None);
}

#[test]
fn rounded_faded_shadowed_and_hovered_nodes_draw() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let doc = load_gui(DOC).unwrap();
    let gui = GuiComponent {
        gui_id: "styles.gui".into(),
        hovered_node_id: Some("pop".into()),
        hover_t: 1.0,
        ..Default::default()
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(
        jengine_core::asset::WHITE_TEXTURE_ID.into(),
        RawTextureData { width: 1, height: 1, rgba: vec![255, 255, 255, 255] },
    );
    assets.ensure_wgpu_texture(jengine_core::asset::WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let ui_vp = glam::Mat4::orthographic_rh_gl(0.0, W as f32, 0.0, H as f32, -1.0, 1.0);
    batch.begin_frame();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        let white = assets.get_wgpu_texture(jengine_core::asset::WHITE_TEXTURE_ID).unwrap();
        let mut flush = |batch: &mut WgpuSpriteBatch, what: UiFlush<'_>| {
            if let UiFlush::Clip(_) = what {
                return;
            }
            batch.flush(&gpu.device, &gpu.queue, &mut pass, &ui_vp, white);
        };
        render_gui_wgpu(&[(doc, gui)], &mut batch, None, W as f32, H as f32, &mut flush);
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let px = |x: f32, y: f32| {
        let i = ((y as u32 * W + x as u32) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    let l = layout();
    let bg = px(300.0, 300.0)[0];
    let card = rect_of(&l, "card").unwrap();
    assert!(px(card.x + 80.0, card.y + 40.0)[0] > 240, "the card is white inside");
    let corner = px(card.x + 2.0, card.y + 2.0)[0];
    assert!((corner as i32 - bg as i32).abs() < 12, "its corner is rounded off: {corner} vs background {bg}");
    let under = px(card.x + 80.0, card.y + card.height + 8.0)[0];
    assert!(under + 20 < bg, "its shadow darkens what is under it: {under} vs {bg}");
    let ghost = rect_of(&l, "ghost").unwrap();
    let half = px(ghost.x + 120.0, ghost.y + 20.0)[0];
    assert!(half > bg + 40 && half < 230, "a node at opacity 0.5 is half there: {half}");
    let pop = rect_of(&l, "pop").unwrap();
    let edge = px(pop.x + 14.0, pop.y + pop.height * 0.5);
    assert!(edge[1] > 200 && edge[0] < 60, "the hovered button is its hover colour, drawn at its scale: {edge:?}");
    let rim = px(pop.x + pop.width * 0.5, pop.y + 3.0);
    assert!(rim[0] > 200 && rim[1] < 60, "its border draws over its body, scaled with it: {rim:?}");
}

/// A script animates a menu through `gui_set`: every new style is settable.
#[test]
fn gui_set_writes_the_new_styles() {
    use jengine_core::ui::gui_document::apply_gui_node_prop;
    let mut doc = load_gui(DOC).unwrap();
    let node = doc.root.find_node_mut("card").unwrap();
    for (prop, value) in [
        ("opacity", serde_json::json!(0.25)),
        ("scale", serde_json::json!(1.2)),
        ("corner_radius", serde_json::json!(9.0)),
        ("offset_y", serde_json::json!(-40.0)),
        ("border_width", serde_json::json!(3.0)),
        ("shadow", serde_json::json!({ "color": [0.0, 0.0, 0.0, 0.5], "offset": [0.0, 4.0], "blur": 8.0 })),
    ] {
        apply_gui_node_prop(node, prop, &value);
    }
    let st = &node.style;
    assert_eq!((st.opacity, st.scale, st.corner_radius, st.offset_y, st.border_width), (0.25, 1.2, 9.0, -40.0, 3.0));
    assert_eq!(st.shadow.map(|s| s.blur), Some(8.0));
    apply_gui_node_prop(node, "shadow", &serde_json::Value::Null);
    assert!(node.style.shadow.is_none(), "null takes the shadow away");
}
