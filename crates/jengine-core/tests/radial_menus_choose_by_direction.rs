//! **Radial menus** (Oct 2026; `ui::radial_menu`): a wheel of entity-UI
//! sections chosen by direction from its middle. The choosing is
//! unit-tested in the module; this drives it the way a host does - through
//! `update_ui_widgets`, with the pointer in the top-left screen space every
//! host reports - and through `Radial.set`, the shared host command a
//! browser script reaches. Found by the showcase's Spell Wheel.
#![cfg(feature = "radial_menu")]

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::components::{CustomComponents, Name};
use jengine_core::input::InputState;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
use jengine_core::ui::radial_menu::UIRadialMenu;
use jengine_core::ui::{UIAnchor, UIElement, UIImage};

struct Host(World);

impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

fn element(x: f32, y: f32, w: f32) -> UIElement {
    UIElement { anchor: UIAnchor::BottomLeft, offset: Vec2::new(x, y), size: Vec2::splat(w), visible: true, ..Default::default() }
}

#[test]
fn a_host_frame_hovers_and_picks_and_a_script_closes_it() {
    let mut world = World::new();
    for n in ["Fire", "Ice", "Wind", "Earth"] {
        world.spawn((Name(n.into()), element(0.0, 0.0, 40.0), UIImage::default()));
    }
    let wheel = world.spawn((
        element(350.0, 250.0, 100.0),
        UIRadialMenu { sections: ["Fire", "Ice", "Wind", "Earth"].iter().map(|s| s.to_string()).collect(), radius: 100.0, ..Default::default() },
    ));
    let mut input = InputState::new();
    let mut clicked = Vec::new();
    // The pointer above the middle: screen y counts down, so 300 - 80.
    input.mouse_left_just_pressed = true;
    jengine_core::ecs::systems::update_ui_widgets(&mut world, &mut input, (400.0, 220.0), 800.0, 600.0, &mut clicked);
    let c = world.get::<&CustomComponents>(wheel).unwrap().0["RadialMenu"].clone();
    assert_eq!((c["hovered"].as_i64(), c["label"].as_str()), (Some(0), Some("Fire")), "up is Fire, picked: {c}");
    let mut host = Host(world);
    let args = JsonArgs(serde_json::json!({ "e": wheel.id(), "f": "open", "v": false }));
    assert!(handle_world_command(&mut host, "rdst", &args), "a shared command");
    let mut input = InputState::new();
    jengine_core::ecs::systems::update_ui_widgets(&mut host.0, &mut input, (400.0, 220.0), 800.0, 600.0, &mut clicked);
    let hidden = host.0.query::<(&Name, &UIElement)>().iter().all(|(_, (_, e))| !e.visible);
    assert!(hidden, "closed, its sections are hidden");
}
