//! **UI drag and drop** (Oct 2026): entity-UI items picked up and put down
//! in slots - an inventory, a loadout, a crafting grid.
//!
//! A [`UIDraggable`] is an item: an entity-UI element (its `ui_image` the
//! picture, its `ui_text`, if any, the count) that lives in the slot its
//! `slot` names and is kept placed there, centred. A [`UIDropSlot`] is a
//! place: it takes an item whose `tag` is in its `accepts` (any, when that
//! is empty).
//!
//! **With the pointer**: press on an item and a **ghost** of it follows the
//! pointer (the item itself dims where it was); the slot under the pointer
//! is tinted `accept_color` or `refuse_color`; let go over a slot that
//! takes it and the item moves there. A slot already holding an item
//! **stacks** them when both have the same non-empty `stack` key (as many as
//! `max_stack` allows; an item emptied this way is hidden and leaves its
//! slot), else **swaps** them when the item's old slot takes the other one,
//! else refuses. Let go anywhere else, or press the right button, and the
//! item goes home.
//!
//! **With a controller** (or the keyboard), the same through focus: confirm
//! (A, Enter, Space) on a focused slot picks up its item, the ghost then
//! sits over whichever slot has focus, and confirm again puts it down; B or
//! Backspace puts it back. Make the slots `focusable`.
//!
//! It runs at the start of `update_ui_widgets`, which every host calls with
//! the pointer in UI space, so no host was wired. A script sees each item's
//! custom `Drag` fields (`slot`, `count`, `held`) and `on_ui_drop(entity)`
//! on the item's own script after it lands.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{CustomComponents, Name};
use crate::input::{InputState, KeyCode};
use crate::ui::{UIAnchor, UIElement, UIImage, UIText};

/// An item that can be picked up and put in a slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UIDraggable {
    /// What kind of thing it is, for a slot's `accepts`.
    pub tag: String,
    /// Items with the same non-empty key stack into one slot.
    pub stack: String,
    /// How many it is.
    pub count: u32,
    /// The most one slot holds of a stack.
    pub max_stack: u32,
    /// The name of the slot it is in (none: it stays where it is authored).
    pub slot: String,
    /// The ghost's opacity, and the item's own while it is held.
    pub ghost_alpha: f32,
    pub held_alpha: f32,
    #[serde(skip)]
    pub state: DragState,
}

impl Default for UIDraggable {
    fn default() -> Self {
        Self {
            tag: String::new(),
            stack: String::new(),
            count: 1,
            max_stack: 1,
            slot: String::new(),
            ghost_alpha: 0.85,
            held_alpha: 0.35,
            state: DragState::default(),
        }
    }
}

/// Where a pick-up is.
#[derive(Debug, Clone, Default)]
pub struct DragState {
    /// Held by the pointer (`Some(grab offset)`) or by focus (`Some(ZERO)`
    /// with `by_focus`).
    pub held: Option<Vec2>,
    pub by_focus: bool,
    /// The ghost entity following the pointer.
    pub ghost: Option<Entity>,
    /// The item's own alpha before it dimmed.
    pub alpha: Option<f32>,
    /// What was last written to the custom `Drag` fields.
    pub published: Option<(String, u32, bool)>,
}

/// A place an item can be put.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UIDropSlot {
    /// The tags it takes (none: any).
    pub accepts: Vec<String>,
    /// Multiplied into its `ui_image` while an item it takes is over it,
    /// and one it refuses.
    pub accept_color: [f32; 4],
    pub refuse_color: [f32; 4],
    /// Its own colour, kept while it is tinted.
    #[serde(skip)]
    pub base: Option<[f32; 4]>,
}

impl Default for UIDropSlot {
    fn default() -> Self {
        Self {
            accepts: Vec::new(),
            accept_color: [0.6, 1.0, 0.6, 1.0],
            refuse_color: [1.0, 0.45, 0.4, 1.0],
            base: None,
        }
    }
}

impl UIDropSlot {
    pub fn takes(&self, tag: &str) -> bool {
        self.accepts.is_empty() || self.accepts.iter().any(|a| a == tag)
    }
}

/// Marks the ghost of a held item (spawned and despawned by this module).
#[derive(Debug, Clone, Copy)]
pub struct DragGhost;

fn named(world: &World, name: &str) -> Option<Entity> {
    if name.is_empty() {
        return None;
    }
    world.query::<(&Name, &UIDropSlot)>().iter().find(|(_, (n, _))| n.0 == name).map(|(e, _)| e)
}

fn name_of(world: &World, e: Entity) -> String {
    world.get::<&Name>(e).map(|n| n.0.clone()).unwrap_or_default()
}

/// The item in slot `slot`, other than `but`.
fn occupant(world: &World, slot: &str, but: Option<Entity>) -> Option<Entity> {
    world
        .query::<(&UIDraggable, &UIElement)>()
        .iter()
        .find(|(e, (d, el))| d.slot == slot && el.visible && Some(*e) != but)
        .map(|(e, _)| e)
}

/// Put an element's centre at `centre` (Y-up UI pixels), whatever its anchor.
fn place(el: &mut UIElement, centre: Vec2) {
    el.anchor = UIAnchor::BottomLeft;
    el.offset = centre - el.size * 0.5;
}

fn alpha_of(world: &World, e: Entity) -> f32 {
    world.get::<&UIImage>(e).map(|i| i.color[3]).unwrap_or(1.0)
}

fn set_alpha(world: &mut World, e: Entity, a: f32) {
    if let Ok(mut i) = world.get::<&mut UIImage>(e) {
        i.color[3] = a;
    }
    if let Ok(mut t) = world.get::<&mut UIText>(e) {
        t.color[3] = a;
    }
}

fn confirm(input: &InputState) -> bool {
    (0..input.pad_count()).any(|i| input.is_pad_button_just_pressed(i, 0))
        || input.is_just_pressed(KeyCode::Enter)
        || input.is_just_pressed(KeyCode::Space)
}

fn cancel(input: &InputState) -> bool {
    (0..input.pad_count()).any(|i| input.is_pad_button_just_pressed(i, 1))
        || input.is_just_pressed(KeyCode::Backspace)
        || input.mouse_right_just_pressed
}

/// What putting `item` (from slot `from`) into `to` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Drop {
    Refused,
    Moved,
    /// Into the item already there, `n` of them.
    Stacked(Entity, u32),
    /// The other item goes to `from`.
    Swapped(Entity),
}

/// Decide a drop without changing anything.
pub fn judge(world: &World, item: Entity, to: Entity) -> Drop {
    let Ok(d) = world.get::<&UIDraggable>(item) else { return Drop::Refused };
    let Ok(slot) = world.get::<&UIDropSlot>(to) else { return Drop::Refused };
    if !slot.takes(&d.tag) {
        return Drop::Refused;
    }
    let to_name = name_of(world, to);
    if to_name == d.slot {
        return Drop::Refused;
    }
    let Some(other) = occupant(world, &to_name, Some(item)) else { return Drop::Moved };
    let Ok(o) = world.get::<&UIDraggable>(other) else { return Drop::Refused };
    if !d.stack.is_empty() && d.stack == o.stack && o.count < o.max_stack {
        return Drop::Stacked(other, d.count.min(o.max_stack - o.count));
    }
    let from_takes = named(world, &d.slot)
        .and_then(|f| world.get::<&UIDropSlot>(f).ok().map(|s| s.takes(&o.tag)))
        .unwrap_or(false);
    if from_takes { Drop::Swapped(other) } else { Drop::Refused }
}

/// Put `item` in `to`, as [`judge`] says. Whether anything moved.
pub fn drop_into(world: &mut World, item: Entity, to: Entity) -> bool {
    let verdict = judge(world, item, to);
    let to_name = name_of(world, to);
    match verdict {
        Drop::Refused => return false,
        Drop::Moved => {
            if let Ok(mut d) = world.get::<&mut UIDraggable>(item) {
                d.slot = to_name;
            }
        }
        Drop::Stacked(other, n) => {
            if let Ok(mut o) = world.get::<&mut UIDraggable>(other) {
                o.count += n;
            }
            let emptied = match world.get::<&mut UIDraggable>(item) {
                Ok(mut d) => {
                    d.count -= n;
                    d.count == 0
                }
                Err(_) => false,
            };
            if emptied {
                if let Ok(mut d) = world.get::<&mut UIDraggable>(item) {
                    d.slot.clear();
                }
                if let Ok(mut el) = world.get::<&mut UIElement>(item) {
                    el.visible = false;
                }
            }
        }
        Drop::Swapped(other) => {
            let from = world.get::<&UIDraggable>(item).map(|d| d.slot.clone()).unwrap_or_default();
            if let Ok(mut o) = world.get::<&mut UIDraggable>(other) {
                o.slot = from;
            }
            if let Ok(mut d) = world.get::<&mut UIDraggable>(item) {
                d.slot = to_name;
            }
        }
    }
    crate::ecs::callbacks::push(item, "on_ui_drop");
    true
}

/// Put `item` in the slot named `slot`, as a drop there would - what
/// `UI.drop` does. A held item is let go first.
pub fn drop_named(world: &mut World, item: Entity, slot: &str) -> bool {
    let target = named(world, slot);
    release(world, item);
    match target {
        Some(to) => drop_into(world, item, to),
        None => {
            log::warn!("UI.drop: no slot named '{slot}'");
            false
        }
    }
}

fn release(world: &mut World, item: Entity) {
    let (ghost, alpha) = match world.get::<&mut UIDraggable>(item) {
        Ok(mut d) => {
            d.state.held = None;
            d.state.by_focus = false;
            (d.state.ghost.take(), d.state.alpha.take())
        }
        Err(_) => (None, None),
    };
    if let Some(g) = ghost {
        let _ = world.despawn(g);
    }
    if let Some(a) = alpha {
        set_alpha(world, item, a);
    }
}

fn pick_up(world: &mut World, item: Entity, grab: Vec2, by_focus: bool) {
    let Ok(el) = world.get::<&UIElement>(item).map(|e| (*e).clone()) else { return };
    let (ghost_alpha, held_alpha) = match world.get::<&UIDraggable>(item) {
        Ok(d) => (d.ghost_alpha, d.held_alpha),
        Err(_) => return,
    };
    let alpha = alpha_of(world, item);
    let mut g = el.clone();
    g.z_order += 1000;
    g.focusable = false;
    g.focused = false;
    let image = world.get::<&UIImage>(item).ok().map(|i| {
        let mut i = (*i).clone();
        i.color[3] = alpha * ghost_alpha;
        i
    });
    let text = world.get::<&UIText>(item).ok().map(|t| (*t).clone());
    let ghost = world.spawn((g, DragGhost));
    if let Some(i) = image {
        let _ = world.insert_one(ghost, i);
    }
    if let Some(t) = text {
        let _ = world.insert_one(ghost, t);
    }
    set_alpha(world, item, alpha * held_alpha);
    if let Ok(mut d) = world.get::<&mut UIDraggable>(item) {
        d.state.held = Some(grab);
        d.state.by_focus = by_focus;
        d.state.ghost = Some(ghost);
        d.state.alpha = Some(alpha);
    }
}

/// The topmost visible slot whose rect holds `p`.
fn slot_at(world: &World, p: Vec2, w: f32, h: f32) -> Option<Entity> {
    let mut best: Option<(i32, Entity)> = None;
    for (e, (el, _)) in world.query::<(&UIElement, &UIDropSlot)>().iter() {
        if !el.visible {
            continue;
        }
        let (min, max) = el.screen_rect(w, h);
        if p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y && best.is_none_or(|(z, _)| el.z_order > z) {
            best = Some((el.z_order, e));
        }
    }
    best.map(|(_, e)| e)
}

/// One frame of drag and drop. `mouse` is in Y-up UI pixels. Returns
/// whether an item is held by the pointer, which then owns it - nothing
/// under the ghost reacts to the press that started it.
pub fn update_drag_drop(world: &mut World, input: &InputState, mouse: Vec2, w: f32, h: f32) -> bool {
    if world.query::<&UIDraggable>().iter().next().is_none() {
        return false;
    }
    let _span = crate::profiling::trace::span("ui.drag_drop");
    let held: Option<(Entity, Vec2, bool)> = world
        .query::<&UIDraggable>()
        .iter()
        .find_map(|(e, d)| d.state.held.map(|g| (e, g, d.state.by_focus)));
    let focused_slot = world
        .query::<(&UIElement, &UIDropSlot)>()
        .iter()
        .find(|(_, (el, _))| el.focused)
        .map(|(e, _)| e);

    match held {
        None => {
            if input.mouse_left_just_pressed {
                // The topmost visible item under the pointer.
                let mut best: Option<(i32, Entity, Vec2)> = None;
                for (e, (el, _)) in world.query::<(&UIElement, &UIDraggable)>().iter() {
                    if !el.visible {
                        continue;
                    }
                    let (min, max) = el.screen_rect(w, h);
                    if mouse.x >= min.x && mouse.x <= max.x && mouse.y >= min.y && mouse.y <= max.y
                        && best.is_none_or(|(z, _, _)| el.z_order > z)
                    {
                        best = Some((el.z_order, e, mouse - (min + max) * 0.5));
                    }
                }
                if let Some((_, item, grab)) = best {
                    pick_up(world, item, grab, false);
                }
            } else if confirm(input)
                && let Some(slot) = focused_slot
                && let Some(item) = occupant(world, &name_of(world, slot), None)
            {
                pick_up(world, item, Vec2::ZERO, true);
            }
        }
        Some((item, _, by_focus)) => {
            if cancel(input) {
                release(world, item);
            } else if by_focus {
                if confirm(input)
                    && let Some(slot) = focused_slot
                {
                    drop_into(world, item, slot);
                    release(world, item);
                }
            } else if !input.mouse_left {
                if let Some(slot) = slot_at(world, mouse, w, h) {
                    drop_into(world, item, slot);
                }
                release(world, item);
            }
        }
    }

    // Place every item in its slot, the ghost where it is held, and tint
    // the slot under it.
    let slots: Vec<(Entity, Vec2)> = world
        .query::<(&UIElement, &UIDropSlot, &Name)>()
        .iter()
        .map(|(e, (el, _, _))| (e, el.screen_center(w, h)))
        .collect();
    let items: Vec<(Entity, String)> =
        world.query::<&UIDraggable>().iter().filter(|(_, d)| !d.slot.is_empty()).map(|(e, d)| (e, d.slot.clone())).collect();
    for (item, slot) in items {
        if let Some(s) = named(world, &slot)
            && let Some(&(_, c)) = slots.iter().find(|(e, _)| *e == s)
            && let Ok(mut el) = world.get::<&mut UIElement>(item)
        {
            place(&mut el, c);
        }
    }
    let held_now: Option<(Entity, Vec2, bool, Option<Entity>)> = world
        .query::<&UIDraggable>()
        .iter()
        .find_map(|(e, d)| d.state.held.map(|g| (e, g, d.state.by_focus, d.state.ghost)));
    let mut over: Option<(Entity, bool)> = None;
    if let Some((item, grab, by_focus, ghost)) = held_now {
        let at = if by_focus {
            focused_slot.and_then(|s| slots.iter().find(|(e, _)| *e == s).map(|(_, c)| *c))
        } else {
            Some(mouse - grab)
        };
        if let (Some(at), Some(g)) = (at, ghost)
            && let Ok(mut el) = world.get::<&mut UIElement>(g)
        {
            place(&mut el, at);
        }
        let target = if by_focus { focused_slot } else { slot_at(world, mouse, w, h) };
        if let Some(t) = target {
            over = Some((t, judge(world, item, t) != Drop::Refused));
        }
    }
    let all_slots: Vec<Entity> = slots.iter().map(|(e, _)| *e).collect();
    for s in all_slots {
        let tint = match over {
            Some((t, ok)) if t == s => {
                let slot = world.get::<&UIDropSlot>(s).map(|x| (*x).clone()).ok();
                slot.map(|x| if ok { x.accept_color } else { x.refuse_color })
            }
            _ => None,
        };
        let base = world.get::<&UIDropSlot>(s).ok().and_then(|x| x.base);
        let Ok(mut img) = world.get::<&mut UIImage>(s) else { continue };
        match (tint, base) {
            (Some(t), b) => {
                let b = b.unwrap_or(img.color);
                img.color = [b[0] * t[0], b[1] * t[1], b[2] * t[2], b[3] * t[3]];
                drop(img);
                if let Ok(mut x) = world.get::<&mut UIDropSlot>(s) {
                    x.base = Some(b);
                }
            }
            (None, Some(b)) => {
                img.color = b;
                drop(img);
                if let Ok(mut x) = world.get::<&mut UIDropSlot>(s) {
                    x.base = None;
                }
            }
            (None, None) => {}
        }
    }
    publish(world);
    held_now.is_some_and(|(_, _, by_focus, _)| !by_focus)
}

/// Each item's count into its own text, and its custom `Drag` fields.
fn publish(world: &mut World) {
    let items: Vec<(Entity, String, u32, bool, Option<(String, u32, bool)>)> = world
        .query::<&UIDraggable>()
        .iter()
        .map(|(e, d)| (e, d.slot.clone(), d.count, d.state.held.is_some(), d.state.published.clone()))
        .collect();
    for (e, slot, count, held, was) in items {
        let now = (slot.clone(), count, held);
        if was.as_ref() == Some(&now) {
            continue;
        }
        if let Ok(mut t) = world.get::<&mut UIText>(e) {
            t.text = if count > 1 { count.to_string() } else { String::new() };
        }
        let v = serde_json::json!({ "slot": slot, "count": count, "held": held });
        let missing = match world.get::<&mut CustomComponents>(e) {
            Ok(mut c) => {
                c.0.insert("Drag".into(), v);
                None
            }
            Err(_) => Some(v),
        };
        if let Some(v) = missing {
            let mut map = std::collections::HashMap::new();
            map.insert("Drag".into(), v);
            let _ = world.insert_one(e, CustomComponents(map));
        }
        if let Ok(mut d) = world.get::<&mut UIDraggable>(e) {
            d.state.published = Some(now);
        }
    }
}
