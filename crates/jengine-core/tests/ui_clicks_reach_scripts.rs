//! **An entity-UI button click reaches a script**, as a `ui_click` event.
//!
//! Every runtime ran `update_ui_widgets`, which collects the buttons clicked
//! that frame into a list - and every runtime dropped the list. So the one way
//! for a script to act on a button was to poll `get_ui_prop(b, "pressed")`,
//! which is true on **every frame the button is held**: a "buy" button written
//! that way bought sixty times a second, and a script ticking slower than the
//! frame could miss a quick click entirely. Found by the arcade's first game
//! whose whole interface is entity UI.
//!
//! This drives the native path with a real Luau script: the widget pass, the
//! runner's `queue_engine_event`, then `ScriptSystem::run`. The browser hosts
//! make the same hand-off from arms that only run in a browser;
//! `entity_ui_reaches_every_runtime` checks they exist.

#![cfg(feature = "native")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::{ScriptSystem, UI_CLICK_EVENT, ui_click_payload, update_ui_widgets};
use jengine_core::input::InputState;
use jengine_core::ui::{UIAnchor, UIButton, UIElement};

const SHOP: &str = r#"
local buy

function init(entity)
    buy = find_by_name("Buy")
    on_event("ui_click", entity)
end

function update(entity, dt)
    -- The polled flag is what games had to use; count its frames too, to
    -- show what it would have done.
    if get_ui_prop(buy, "pressed") then
        set_custom(entity, "Probe", "pressed_frames", (get_custom(entity, "Probe", "pressed_frames") or 0) + 1)
    end
end

function on_ui_click(entity, data)
    if data ~= nil and data.button == buy then
        set_custom(entity, "Probe", "bought", (get_custom(entity, "Probe", "bought") or 0) + 1)
    end
end
"#;

const W: f32 = 800.0;
const H: f32 = 600.0;
/// The middle of the button, in the top-left-origin space the mouse arrives in.
const ON_BUTTON: (f32, f32) = (140.0, 145.0);

struct Frame {
    world: World,
    scripts: ScriptSystem,
}

impl Frame {
    /// The native runner's order: widgets, the clicks handed to the script
    /// system, then the scripts.
    fn step(&mut self, mut input: InputState) {
        let mut clicked = Vec::new();
        update_ui_widgets(&mut self.world, &mut input, ON_BUTTON, W, H, &mut clicked);
        for b in clicked {
            self.scripts.queue_engine_event(UI_CLICK_EVENT, ui_click_payload(b));
        }
        self.scripts.run(&mut self.world, 1.0 / 60.0, &input, None, None);
    }
}

fn input(down: bool, just_pressed: bool, just_released: bool) -> InputState {
    let mut i = InputState::new();
    i.mouse_left = down;
    i.mouse_left_just_pressed = just_pressed;
    i.mouse_left_just_released = just_released;
    i
}

fn probe(world: &World, e: hecs::Entity, field: &str) -> Option<f64> {
    let c = world.get::<&CustomComponents>(e).ok()?;
    c.0.get("Probe")?.get(field)?.as_f64()
}

#[test]
fn holding_a_button_for_half_a_second_buys_one_thing() {
    let dir = std::env::temp_dir().join(format!("jengine_ui_clicks_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shop.lua");
    std::fs::write(&path, SHOP).unwrap();

    let mut world = World::new();
    world.spawn((
        Name("Buy".into()),
        UIElement {
            anchor: UIAnchor::TopLeft,
            offset: Vec2::new(40.0, -120.0),
            size: Vec2::new(200.0, 50.0),
            ..Default::default()
        },
        UIButton::default(),
    ));
    let shop = world.spawn((
        Name("Shop".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_string_lossy().into(),
            cull_distance: None,
        },
    ));
    let mut f = Frame { world, scripts: ScriptSystem::new().expect("script system") };

    f.step(input(false, false, false)); // init
    f.step(input(true, true, false));
    for _ in 0..30 {
        f.step(input(true, false, false));
    }
    f.step(input(false, false, true));
    f.step(input(false, false, false)); // delivered on the run after the queue

    assert!(
        probe(&f.world, shop, "pressed_frames").unwrap_or(0.0) >= 30.0,
        "polling `pressed` sees every held frame - which is why it cannot be a click"
    );
    assert_eq!(
        probe(&f.world, shop, "bought"),
        Some(1.0),
        "one press and one release is one `ui_click`, with the button's id"
    );
}
