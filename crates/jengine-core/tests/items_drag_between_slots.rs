//! **UI drag and drop** (Oct 2026; `ui::drag_drop`): items dragged between
//! slots by the pointer - a ghost follows it, the slot under it is tinted to
//! say yes or no, and a drop moves, refuses, stacks or swaps - and the same
//! by focus and confirm, as a controller does it. Driven through
//! `update_ui_widgets`, the step every host calls. Found by the showcase's
//! Loot Satchel.

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::components::{CustomComponents, Name};
use jengine_core::input::{InputState, KeyCode};
use jengine_core::ui::drag_drop::{DragGhost, UIDraggable, UIDropSlot};
use jengine_core::ui::{UIAnchor, UIElement, UIImage};

const W: f32 = 800.0;
const H: f32 = 600.0;

fn element(x: f32, y: f32, z: i32) -> UIElement {
    UIElement {
        anchor: UIAnchor::BottomLeft,
        offset: Vec2::new(x, y),
        size: Vec2::new(64.0, 64.0),
        z_order: z,
        visible: true,
        focusable: true,
        ..Default::default()
    }
}

fn image() -> UIImage {
    UIImage { texture_id: "slot.png".into(), color: [1.0, 1.0, 1.0, 1.0], ..Default::default() }
}

fn slot(world: &mut World, name: &str, x: f32, accepts: &[&str]) -> Entity {
    world.spawn((
        Name(name.into()),
        element(x, 100.0, 1),
        image(),
        UIDropSlot { accepts: accepts.iter().map(|s| s.to_string()).collect(), ..Default::default() },
    ))
}

fn item(world: &mut World, name: &str, tag: &str, stack: &str, count: u32, in_slot: &str) -> Entity {
    let mut el = element(0.0, 0.0, 5);
    el.focusable = false;
    world.spawn((
        Name(name.into()),
        el,
        image(),
        UIDraggable { tag: tag.into(), stack: stack.into(), count, max_stack: 5, slot: in_slot.into(), ..Default::default() },
    ))
}

fn centre(world: &World, e: Entity) -> Vec2 {
    world.get::<&UIElement>(e).unwrap().screen_center(W, H)
}

fn slot_of(world: &World, e: Entity) -> String {
    world.get::<&UIDraggable>(e).unwrap().slot.clone()
}

fn count(world: &World, e: Entity) -> u32 {
    world.get::<&UIDraggable>(e).unwrap().count
}

fn ghosts(world: &World) -> usize {
    world.query::<&DragGhost>().iter().count()
}

/// One frame: the pointer at `at` (Y-up UI pixels), the left button as
/// given, and any keys pressed.
fn frame(world: &mut World, input: &mut InputState, at: Vec2, left: bool, keys: &[KeyCode]) {
    input.begin_frame();
    for k in [KeyCode::Enter, KeyCode::Backspace] {
        input.key_up(k);
    }
    input.begin_frame();
    for k in keys {
        input.key_down(*k);
    }
    input.mouse_left_just_pressed = left && !input.mouse_left;
    input.mouse_left_just_released = !left && input.mouse_left;
    input.mouse_left = left;
    let pointer = (at.x, H - at.y);
    jengine_core::ecs::systems::update_ui_widgets(world, input, pointer, W, H, &mut Vec::new());
}

/// Press on `from`, carry to `to`, let go.
fn drag(world: &mut World, input: &mut InputState, from: Vec2, to: Vec2) {
    frame(world, input, from, true, &[]);
    frame(world, input, to, true, &[]);
    frame(world, input, to, false, &[]);
}

#[test]
fn items_are_dragged_stacked_swapped_and_refused() {
    let mut world = World::new();
    let bag1 = slot(&mut world, "Bag1", 100.0, &[]);
    let bag2 = slot(&mut world, "Bag2", 200.0, &[]);
    let bag3 = slot(&mut world, "Bag3", 300.0, &[]);
    let weapon = slot(&mut world, "Weapon", 500.0, &["sword"]);
    let potion = item(&mut world, "Potion", "potion", "potion", 2, "Bag1");
    let potion2 = item(&mut world, "Potion2", "potion", "potion", 4, "Bag2");
    let sword = item(&mut world, "Sword", "sword", "", 1, "Bag3");
    let mut input = InputState::default();
    frame(&mut world, &mut input, Vec2::ZERO, false, &[]);
    // Slots stay where they are: their centres, once.
    let (cb1, cb2, cb3, cw) = (centre(&world, bag1), centre(&world, bag2), centre(&world, bag3), centre(&world, weapon));
    assert!(centre(&world, sword).distance(cb3) < 0.01, "an item sits centred in its slot");

    // The sword to the weapon slot: a ghost while held, the slot says yes.
    frame(&mut world, &mut input, cb3, true, &[]);
    assert_eq!(ghosts(&world), 1, "a ghost follows the pointer");
    assert!(world.get::<&UIImage>(sword).unwrap().color[3] < 0.5, "the item dims where it was");
    let over = cw + Vec2::new(5.0, 3.0);
    frame(&mut world, &mut input, over, true, &[]);
    let ghost = world.query::<&DragGhost>().iter().next().map(|(e, _)| e).unwrap();
    assert!(centre(&world, ghost).distance(over) < 0.01, "the ghost is where the pointer is");
    let tint = world.get::<&UIImage>(weapon).unwrap().color;
    assert!(tint[1] > tint[0], "a slot that takes it is tinted green: {tint:?}");
    frame(&mut world, &mut input, over, false, &[]);
    assert_eq!(slot_of(&world, sword), "Weapon");
    assert_eq!(ghosts(&world), 0, "the ghost goes");
    assert_eq!(world.get::<&UIImage>(sword).unwrap().color[3], 1.0, "the item is itself again");
    assert_eq!(world.get::<&UIImage>(weapon).unwrap().color, [1.0, 1.0, 1.0, 1.0], "the tint goes");
    assert_eq!(world.get::<&CustomComponents>(sword).unwrap().0["Drag"]["slot"], "Weapon");

    // A potion is no weapon: refused, it goes home.
    frame(&mut world, &mut input, cb1, true, &[]);
    frame(&mut world, &mut input, cw, true, &[]);
    let tint = world.get::<&UIImage>(weapon).unwrap().color;
    assert!(tint[0] > tint[1], "a slot that refuses is tinted red: {tint:?}");
    frame(&mut world, &mut input, cw, false, &[]);
    assert_eq!(slot_of(&world, potion), "Bag1", "refused");

    // Two potions onto four, at most five a slot: one goes over.
    drag(&mut world, &mut input, cb1, cb2);
    assert_eq!((count(&world, potion2), count(&world, potion)), (5, 1), "stacked up to the limit");
    assert_eq!(slot_of(&world, potion), "Bag1", "the rest stays");

    // Into an empty slot: moved. Then the sword onto it: swapped.
    drag(&mut world, &mut input, cb1, cb3);
    assert_eq!(slot_of(&world, potion), "Bag3");
    drag(&mut world, &mut input, cw, cb1);
    assert_eq!(slot_of(&world, sword), "Bag1");
    drag(&mut world, &mut input, cb1, cb3);
    assert_eq!((slot_of(&world, sword), slot_of(&world, potion)), ("Bag3".into(), "Bag1".into()), "swapped");

    // Backspace (or B, or the right button) while held: it goes home.
    frame(&mut world, &mut input, cb2, true, &[]);
    frame(&mut world, &mut input, cw, true, &[KeyCode::Backspace]);
    frame(&mut world, &mut input, cw, false, &[]);
    assert_eq!(slot_of(&world, potion2), "Bag2", "cancelled");
    assert_eq!(ghosts(&world), 0);

    // By focus, as a controller does: confirm on the sword's slot, focus
    // the weapon slot (the ghost goes with it), confirm.
    let focus = |world: &mut World, on: Entity| {
        for (e, el) in world.query_mut::<&mut UIElement>() {
            el.focused = e == on;
        }
    };
    focus(&mut world, bag3);
    frame(&mut world, &mut input, Vec2::ZERO, false, &[KeyCode::Enter]);
    assert_eq!(ghosts(&world), 1, "picked up by confirm");
    focus(&mut world, weapon);
    frame(&mut world, &mut input, Vec2::ZERO, false, &[]);
    let ghost = world.query::<&DragGhost>().iter().next().map(|(e, _)| e).unwrap();
    assert!(centre(&world, ghost).distance(cw) < 0.01, "the ghost sits over the focused slot");
    frame(&mut world, &mut input, Vec2::ZERO, false, &[KeyCode::Enter]);
    assert_eq!(slot_of(&world, sword), "Weapon", "put down by confirm");
    assert_eq!(ghosts(&world), 0);

    // `UI.drop`, through the script command's own path: the same rules.
    use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
    struct Host(World);
    impl ScriptWorldHost for Host {
        fn world(&mut self) -> &mut World {
            &mut self.0
        }
        fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
            self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
        }
    }
    let mut host = Host(world);
    let drop = |host: &mut Host, e: Entity, slot: &str| {
        let v = serde_json::json!({ "e": e.id(), "s": slot });
        assert!(handle_world_command(host, "uidp", &JsonArgs(v)), "the shared arm takes `uidp`");
    };
    drop(&mut host, potion, "Weapon");
    assert_eq!(slot_of(&host.0, potion), "Bag1", "a weapon slot still refuses a potion");
    drop(&mut host, potion, "Bag3");
    assert_eq!(slot_of(&host.0, potion), "Bag3", "moved");
}
