//! **Speech bubbles** (Oct 2026; `ui::speech_bubble`): a speaker's words in
//! a bubble over its head that sizes to them, points its tail down at it,
//! stays on screen and keeps clear of other bubbles. Wrapping and arranging
//! are unit-tested in the module; this drives it as a host does - the words
//! through `Bubble.say`, the shared host command a browser script reaches,
//! and the bubbles through `update_ui_widgets` - and checks they follow
//! their speakers. Found by the showcase's Talk of the Town.
#![cfg(feature = "speech_bubble")]

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::components::Transform;
use jengine_core::ecs::views::{MainView, set_main_view};
use jengine_core::input::InputState;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
use jengine_core::ui::speech_bubble::{BubblePart, SpeechBubble};
use jengine_core::ui::{UIElement, UIText};

struct Host(World);

impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

fn frame(world: &mut World) {
    let mut input = InputState::new();
    let mut clicked = Vec::new();
    jengine_core::ecs::systems::update_ui_widgets(world, &mut input, (0.0, 0.0), 800.0, 600.0, &mut clicked);
}

fn rect_of(world: &World, who: Entity) -> (Vec2, Vec2) {
    world.get::<&SpeechBubble>(who).unwrap().state.rect.expect("a bubble")
}

#[test]
fn two_neighbours_talk_and_their_bubbles_follow_without_touching() {
    set_main_view(Some(MainView { center: Vec2::ZERO, axis_x: Vec2::X, axis_y: Vec2::Y, half: Vec2::new(400.0, 300.0) }));
    let mut world = World::new();
    let baker = world.spawn((Transform { position: Vec2::new(-20.0, -100.0), ..Default::default() },));
    let smith = world.spawn((Transform { position: Vec2::new(20.0, -100.0), ..Default::default() },));
    let mut host = Host(world);
    let say = |host: &mut Host, e: Entity, text: &str| {
        let args = JsonArgs(serde_json::json!({ "e": e.id(), "x": text, "s": 0.0 }));
        assert!(handle_world_command(host, "bbsy", &args), "a shared command");
    };
    say(&mut host, baker, "Fresh bread, warm from the oven, two loaves for a copper!");
    say(&mut host, smith, "Horseshoes!");
    let mut world = host.0;
    frame(&mut world);
    assert_eq!(world.query::<&BubblePart>().iter().count(), 4, "two bubbles, two tails");
    let (a, sa) = rect_of(&world, baker);
    let (b, sb) = rect_of(&world, smith);
    let apart = (a.x - b.x).abs() * 2.0 >= sa.x + sb.x || (a.y - b.y).abs() * 2.0 >= sa.y + sb.y;
    assert!(apart, "standing side by side, their bubbles do not overlap: {a} {sa} / {b} {sb}");
    let lines = world.get::<&UIText>(world.get::<&SpeechBubble>(baker).unwrap().state.bubble.unwrap()).unwrap().text.clone();
    assert!(lines.contains('\n'), "the long one wraps: {lines:?}");
    // The smith walks right; his bubble goes with him.
    world.get::<&mut Transform>(smith).unwrap().position.x = 250.0;
    frame(&mut world);
    let (b2, _) = rect_of(&world, smith);
    assert!(b2.x > b.x + 150.0, "followed: {b} -> {b2}");
    let el = (*world.get::<&UIElement>(world.get::<&SpeechBubble>(smith).unwrap().state.bubble.unwrap()).unwrap()).clone();
    assert!(el.visible);
    // Said again, the bubble is made afresh, so a typewriter types the line
    // again rather than leaving it finished.
    let before = world.get::<&SpeechBubble>(smith).unwrap().state.bubble.unwrap();
    let mut host = Host(world);
    say(&mut host, smith, "Horseshoes!");
    let mut world = host.0;
    frame(&mut world);
    let after = world.get::<&SpeechBubble>(smith).unwrap().state.bubble.unwrap();
    assert_ne!(before, after, "a new bubble for a new saying");
    assert!(!world.contains(before), "the old one goes");
    assert_eq!(world.query::<&BubblePart>().iter().count(), 4);
    set_main_view(None);
}
