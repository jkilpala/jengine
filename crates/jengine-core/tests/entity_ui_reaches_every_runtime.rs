//! Entity-UI widgets must be placed where they were authored, and must answer
//! the pointer in every runtime.
//!
//! # Why a source scan
//!
//! `update_ui_buttons` - now [`update_ui_widgets`] - has existed for as long
//! as the widgets have. It sets `UIButton.hovered` / `pressed`, which is the
//! only channel entity UI has for input: `get_ui_prop(button, "pressed")` is
//! documented as the entity-UI answer to `gui_on_click`. **No runtime ever
//! called it.** The flags were therefore always false, a button could not be
//! clicked in the editor, in a native build or on the web, and nothing said
//! so: the function compiled, was reachable, and was wrong only by omission
//! (BUGS #61). The slider, toggle and text input had no interaction code at
//! all - a slider could be moved only by a script setting its value.
//!
//! That is the same shape as the GUI scroll (`gui_scroll_reaches_every_runtime`),
//! the patrol library (#7) and the input mirror (#9). Only a check on the call
//! sites catches it.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::systems::update_ui_widgets;
use jengine_core::input::{InputState, KeyCode};
use jengine_core::ui::{UIAnchor, UIButton, UIElement, UIPanel, UISlider, UITextInput, UIToggle};

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");
const RENDERER: &str = include_str!("../src/ecs/wgpu_systems.rs");

fn runtimes() -> [(&'static str, &'static str); 3] {
    [("native", NATIVE), ("viewport", VIEWPORT), ("web", WEB)]
}

#[test]
fn every_runtime_drives_entity_ui_widgets() {
    for (name, src) in runtimes() {
        assert!(
            src.contains("update_ui_widgets"),
            "{name} never calls `update_ui_widgets`, so no entity-UI widget \
             answers the pointer there - a button reports itself idle forever, \
             a slider cannot be dragged, and nothing says why"
        );
    }
}

/// The renderer and the hit test must place a widget the same way.
///
/// They used to compute the rect separately, from the same wrong formula. One
/// of them being fixed alone would be worse than neither: the widget would be
/// operable somewhere it is not drawn.
#[test]
fn the_renderer_places_widgets_through_the_shared_helper() {
    assert!(
        RENDERER.contains("screen_center("),
        "`render_ui_wgpu` no longer places widgets through `UIElement::screen_center`, \
         so it can drift from the rect the pointer is tested against"
    );
}

const W: f32 = 800.0;
const H: f32 = 600.0;

/// 40 in from the left, 120 down from the top, 200x50 - scene 40's shape.
fn element() -> UIElement {
    UIElement {
        anchor: UIAnchor::TopLeft,
        offset: Vec2::new(40.0, -120.0),
        size: Vec2::new(200.0, 50.0),
        ..Default::default()
    }
}

/// The middle of [`element`], in the top-left-origin space the mouse arrives in.
const INSIDE: (f32, f32) = (140.0, 145.0);

fn pointer(down: bool, just_pressed: bool) -> InputState {
    let mut input = InputState::default();
    input.mouse_left = down;
    input.mouse_left_just_pressed = just_pressed;
    input
}

fn released() -> InputState {
    let mut input = InputState::default();
    input.mouse_left_just_released = true;
    input
}

/// Press at `at`, then let go at `at`: what a click is.
fn click(world: &mut World, at: (f32, f32)) -> Vec<u64> {
    press_release(world, at, at)
}

fn press_release(world: &mut World, down_at: (f32, f32), up_at: (f32, f32)) -> Vec<u64> {
    let mut clicked = Vec::new();
    update_ui_widgets(world, &mut pointer(true, true), down_at, W, H, &mut clicked);
    assert!(clicked.is_empty(), "a press alone is not a click - it fires on release");
    update_ui_widgets(world, &mut released(), up_at, W, H, &mut clicked);
    clicked
}

#[test]
fn a_click_on_the_button_presses_it() {
    let mut world = World::new();
    let e = world.spawn((element(), UIButton::default()));

    let mut clicked = Vec::new();
    update_ui_widgets(&mut world, &mut pointer(true, true), INSIDE, W, H, &mut clicked);
    let btn = world.get::<&UIButton>(e).expect("button");
    assert!(btn.hovered, "the pointer was inside the button's drawn rect");
    assert!(btn.pressed);
    drop(btn);
    assert!(clicked.is_empty(), "held down, not yet clicked");

    let clicked = click(&mut world, INSIDE);
    assert_eq!(clicked, vec![e.id() as u64], "pressed and released inside the button is a click");
}

/// Holding a button is one click, not one a frame. Polling `pressed` - the
/// only channel there was - bought sixty of something a second.
#[test]
fn holding_a_button_down_is_one_click() {
    let mut world = World::new();
    world.spawn((element(), UIButton::default()));
    let mut clicked = Vec::new();
    update_ui_widgets(&mut world, &mut pointer(true, true), INSIDE, W, H, &mut clicked);
    for _ in 0..30 {
        update_ui_widgets(&mut world, &mut pointer(true, false), INSIDE, W, H, &mut clicked);
    }
    update_ui_widgets(&mut world, &mut released(), INSIDE, W, H, &mut clicked);
    assert_eq!(clicked.len(), 1);
}

/// Dragging off before letting go backs out of a click, and a release that
/// began somewhere else is not one.
#[test]
fn a_click_needs_the_press_and_the_release_on_the_same_button() {
    let mut world = World::new();
    world.spawn((element(), UIButton::default()));
    let outside = (700.0, 500.0);
    assert!(press_release(&mut world, INSIDE, outside).is_empty(), "dragged off: cancelled");
    assert!(press_release(&mut world, outside, INSIDE).is_empty(), "pressed elsewhere");
    assert_eq!(click(&mut world, INSIDE).len(), 1, "and a clean click after both still works");
}

/// Every runtime hands the clicks to scripts as `ui_click`. The list was
/// collected by all three and dropped by all three.
#[test]
fn every_runtime_delivers_ui_clicks_to_scripts() {
    const WEB_HOST: &str = include_str!("../../jengine-web/src/script_host.rs");
    for (name, src) in [("native", NATIVE), ("viewport", VIEWPORT), ("web", WEB_HOST)] {
        assert!(
            src.contains("UI_CLICK_EVENT") && src.contains("ui_click_payload"),
            "{name} does not deliver entity-UI clicks as `ui_click` - a button there              can only be polled"
        );
    }
    assert!(WEB.contains("dispatch_ui_clicks(app, &clicked)"), "web never calls its dispatch");
}

#[test]
fn the_mirror_of_the_button_is_not_the_button() {
    let mut world = World::new();
    let e = world.spawn((element(), UIButton::default()));

    // Same x, mirrored y: what the old hit test accepted instead.
    let clicked = click(&mut world, (INSIDE.0, H - INSIDE.1));

    let btn = world.get::<&UIButton>(e).expect("button");
    assert!(
        !btn.hovered && !btn.pressed,
        "the hit test is comparing a Y-down mouse against a Y-up rect again"
    );
    drop(btn);
    assert!(clicked.is_empty());
}

#[test]
fn a_disabled_button_ignores_the_pointer() {
    let mut world = World::new();
    let e = world.spawn((element(), UIButton { disabled: true, ..Default::default() }));

    let clicked = click(&mut world, INSIDE);

    let btn = world.get::<&UIButton>(e).expect("button");
    assert!(!btn.hovered && !btn.pressed);
    drop(btn);
    assert!(clicked.is_empty(), "a disabled button must not report a click");
}

#[test]
fn a_hidden_button_is_not_clickable() {
    let mut world = World::new();
    let e = world.spawn((
        UIElement { visible: false, ..element() },
        UIButton::default(),
    ));

    let clicked = click(&mut world, INSIDE);

    assert!(!world.get::<&UIButton>(e).expect("button").pressed);
    assert!(clicked.is_empty());
}

// ── the three widgets that had no interaction code at all ────────────────

#[test]
fn a_toggle_flips_when_it_is_clicked() {
    let mut world = World::new();
    let e = world.spawn((element(), UIToggle::default()));
    assert!(!world.get::<&UIToggle>(e).expect("toggle").checked);

    click(&mut world, INSIDE);
    assert!(world.get::<&UIToggle>(e).expect("toggle").checked);

    click(&mut world, INSIDE);
    assert!(!world.get::<&UIToggle>(e).expect("toggle").checked, "a second click flips it back");
}

#[test]
fn a_toggle_flips_once_per_press_not_once_per_frame() {
    // The press edge is what counts. Holding the button down over a toggle
    // would otherwise flip it sixty times a second.
    let mut world = World::new();
    let e = world.spawn((element(), UIToggle::default()));

    click(&mut world, INSIDE);
    let mut held = pointer(true, false);
    for _ in 0..10 {
        update_ui_widgets(&mut world, &mut held, INSIDE, W, H, &mut Vec::new());
    }

    assert!(world.get::<&UIToggle>(e).expect("toggle").checked);
}

#[test]
fn clicking_a_slider_track_sets_its_value() {
    let mut world = World::new();
    let e = world.spawn((element(), UISlider { value: 0.0, ..Default::default() }));

    // Three quarters along a track running x = 40..240.
    click(&mut world, (190.0, 145.0));

    let slider = world.get::<&UISlider>(e).expect("slider");
    assert!((slider.value - 0.75).abs() < 1e-5, "value is {}", slider.value);
}

#[test]
fn a_slider_keeps_the_pointer_until_the_button_comes_up() {
    // Dragging past the end is how you reach the maximum, and letting go of
    // the drag there would strand the value short of it.
    let mut world = World::new();
    let e = world.spawn((element(), UISlider { value: 0.0, ..Default::default() }));

    update_ui_widgets(&mut world, &mut pointer(true, true), (140.0, 145.0), W, H, &mut Vec::new());
    let mut held = pointer(true, false);
    // Well past the right-hand end, and off the track vertically too.
    update_ui_widgets(&mut world, &mut held, (900.0, 30.0), W, H, &mut Vec::new());
    assert_eq!(world.get::<&UISlider>(e).expect("slider").value, 1.0);

    // Released, the slider stops following.
    let mut up = pointer(false, false);
    update_ui_widgets(&mut world, &mut up, (0.0, 145.0), W, H, &mut Vec::new());
    assert_eq!(
        world.get::<&UISlider>(e).expect("slider").value,
        1.0,
        "the slider followed the pointer after the button came up"
    );
}

#[test]
fn a_slider_spans_its_own_range() {
    let mut world = World::new();
    let e = world.spawn((
        element(),
        UISlider { value: 0.0, min: 0.0, max: 100.0, ..Default::default() },
    ));

    click(&mut world, (140.0, 145.0)); // halfway
    let slider = world.get::<&UISlider>(e).expect("slider");
    assert!((slider.value - 50.0).abs() < 1e-4, "value is {}", slider.value);
}

#[test]
fn clicking_a_text_input_focuses_it_and_typing_reaches_it() {
    let mut world = World::new();
    let e = world.spawn((element(), UITextInput::default()));

    click(&mut world, INSIDE);
    assert!(world.get::<&UITextInput>(e).expect("field").focused);

    let mut typing = pointer(false, false);
    typing.type_str("hi");
    update_ui_widgets(&mut world, &mut typing, INSIDE, W, H, &mut Vec::new());
    assert_eq!(world.get::<&UITextInput>(e).expect("field").text, "hi");
}

#[test]
fn backspace_in_a_field_does_not_also_fire_its_other_binding() {
    // The test lab binds Backspace to "back to hub", so a field that deletes a
    // character without consuming the key throws you out of the scene
    // mid-word.
    let mut world = World::new();
    let e = world.spawn((
        element(),
        UITextInput { text: "abc".into(), ..Default::default() },
    ));
    click(&mut world, INSIDE);

    let mut input = pointer(false, false);
    input.key_down(KeyCode::Backspace);
    update_ui_widgets(&mut world, &mut input, INSIDE, W, H, &mut Vec::new());

    assert_eq!(world.get::<&UITextInput>(e).expect("field").text, "ab");
    assert!(
        !input.is_just_pressed(KeyCode::Backspace),
        "the field deleted a character and left the keystroke for everyone else"
    );
}

#[test]
fn clicking_away_drops_focus() {
    let mut world = World::new();
    let e = world.spawn((element(), UITextInput::default()));
    click(&mut world, INSIDE);
    assert!(world.get::<&UITextInput>(e).expect("field").focused);

    click(&mut world, (700.0, 500.0));
    assert!(
        !world.get::<&UITextInput>(e).expect("field").focused,
        "focus survived a click on empty space"
    );
}

#[test]
fn only_one_field_holds_focus() {
    let mut world = World::new();
    let first = world.spawn((element(), UITextInput::default()));
    let second = world.spawn((
        UIElement { offset: Vec2::new(40.0, -300.0), ..element() },
        UITextInput::default(),
    ));

    click(&mut world, INSIDE);
    click(&mut world, (140.0, 325.0));

    assert!(!world.get::<&UITextInput>(first).expect("first").focused);
    assert!(world.get::<&UITextInput>(second).expect("second").focused);
}

#[test]
fn the_topmost_widget_takes_the_click() {
    // Two buttons stacked. The renderer draws ascending `z_order`, so the
    // higher one is on top and is the one under the cursor.
    let mut world = World::new();
    let under = world.spawn((UIElement { z_order: 0, ..element() }, UIButton::default()));
    let over = world.spawn((UIElement { z_order: 5, ..element() }, UIButton::default()));

    let clicked = click(&mut world, INSIDE);

    assert!(clicked.contains(&(over.id() as u64)));
    assert!(
        !clicked.contains(&(under.id() as u64)),
        "a click reached a button through the one drawn over it"
    );
}

#[test]
fn a_panel_drawn_over_a_button_does_not_block_it() {
    // Decoration is not a shield: panels, labels and images are drawn, not
    // operated, so they must not swallow the pointer.
    let mut world = World::new();
    world.spawn((UIElement { z_order: 9, ..element() }, UIPanel::default()));
    let button = world.spawn((UIElement { z_order: 0, ..element() }, UIButton::default()));

    assert_eq!(click(&mut world, INSIDE), vec![button.id() as u64]);
}
