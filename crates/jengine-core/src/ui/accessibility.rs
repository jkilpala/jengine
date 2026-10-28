//! Menus a screen reader can read and work (Oct 2026; AccessKit, Godot's
//! accessibility nodes, Unity's `AccessibilityHierarchy`): the UI -
//! entity-UI widgets and `.gui` nodes - as a tree of buttons, labels, text
//! fields, toggles and sliders with their names, values, places and which
//! one has focus, handed to the platform's accessibility API every frame it
//! changes, and the screen reader's own "focus this" / "press this" fed back
//! into the menu navigation every script already answers.
//!
//! [`gather`] builds the tree at the end of `ecs::systems::navigate_ui`
//! (which every host calls once a frame), [`apply_actions`] takes the
//! screen reader's requests at its start - a press reaches a script as a
//! click or a D-pad press does (`NavOutcome`). Natively the runtime hands
//! [`latest`] to AccessKit's winit adapter (the one host call); in the
//! browsers [`web::sync`] mirrors it into a visually hidden DOM - buttons a
//! screen reader can press, and a live region that says what focus moved
//! to. `Accessibility.announce(text)` says anything else ([`announce`]).
//!
//! A node's name is its `accessible` component's `label`, else its text: a
//! button's `UIText` (on it or a child), a `.gui` button's label, a field's
//! text or placeholder.

use std::cell::RefCell;
use std::collections::HashMap;

use hecs::{Entity, World};

pub use super::Accessible;


/// What a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Button,
    Label,
    TextInput,
    Toggle,
    Slider,
    ProgressBar,
    Image,
}

impl Role {
    /// How a screen reader says it, for the browsers' live region.
    pub fn spoken(self) -> &'static str {
        match self {
            Role::Button => "button",
            Role::Label => "",
            Role::TextInput => "edit text",
            Role::Toggle => "checkbox",
            Role::Slider => "slider",
            Role::ProgressBar => "progress",
            Role::Image => "image",
        }
    }
}

/// One thing a screen reader can find.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: u64,
    pub role: Role,
    pub label: String,
    pub description: String,
    /// A field's text, a slider's or progress bar's number.
    pub value: Option<String>,
    /// A slider's or progress bar's (value, min, max).
    pub numeric: Option<(f64, f64, f64)>,
    pub toggled: Option<bool>,
    /// Where it is on screen, physical pixels, y down: x, y, w, h.
    pub bounds: [f32; 4],
    pub focusable: bool,
}

/// The whole UI as a screen reader sees it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tree {
    pub nodes: Vec<Node>,
    /// The node with focus, if any.
    pub focus: Option<u64>,
    /// The latest announcement and a counter that changes with each one, so
    /// the same words said twice are said twice.
    pub announcement: (u64, String),
}

/// What a screen reader asks of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Focus(u64),
    Click(u64),
    /// A slider's "increase" (+1) or "decrease" (-1): a tenth of its range,
    /// as a press of right or left is.
    Step(u64, i8),
}

/// Where a node's id leads back to.
#[derive(Debug, Clone)]
enum Target {
    Entity(Entity),
    Gui { holder: Entity, gui_id: String, node: String },
}

#[derive(Default)]
struct State {
    latest: Tree,
    generation: u64,
    targets: HashMap<u64, Target>,
    actions: Vec<Action>,
    announcement: (u64, String),
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Queue a screen reader's request; the next `navigate_ui` carries it out.
pub fn request(action: Action) {
    STATE.with(|s| s.borrow_mut().actions.push(action));
}

impl Node {
    /// What a screen reader says for the node: its name, what it is, its
    /// state or value, and its description - "Subtitles, checkbox, checked".
    pub fn speech(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.label.trim().is_empty() {
            parts.push(self.label.clone());
        }
        if !self.role.spoken().is_empty() {
            parts.push(self.role.spoken().to_string());
        }
        match (self.toggled, &self.value) {
            (Some(on), _) => parts.push(if on { "checked" } else { "not checked" }.to_string()),
            (None, Some(v)) if !v.is_empty() && self.role != Role::Label => parts.push(v.clone()),
            _ => {}
        }
        if !self.description.is_empty() {
            parts.push(self.description.clone());
        }
        parts.join(", ")
    }
}

/// `Accessibility.speech()`: the focused node as a screen reader says it,
/// or empty with nothing focused - what a game captions, or speaks itself
/// where there is no screen reader.
pub fn focused_speech() -> String {
    STATE.with(|s| {
        let s = s.borrow();
        let t = &s.latest;
        t.nodes.iter().find(|n| Some(n.id) == t.focus).map(Node::speech).unwrap_or_default()
    })
}

/// Have the screen reader say `text` (`Accessibility.announce`).
pub fn announce(text: &str) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.announcement.0 += 1;
        s.announcement.1 = text.to_string();
    });
}

/// The tree as last gathered, and a number that changes whenever it does.
pub fn latest() -> (Tree, u64) {
    STATE.with(|s| {
        let s = s.borrow();
        (s.latest.clone(), s.generation)
    })
}

fn gui_id_of(holder: Entity, node: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in holder.to_bits().get().to_le_bytes().iter().chain(node.as_bytes()) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h | (1 << 63)
}

/// Carry out what the screen reader asked since the last frame: focus moves
/// to the node (as navigation's focus), and a press is returned as the
/// activation or click a D-pad press would have made.
pub fn apply_actions(world: &mut World, assets: &mut crate::asset::AssetManager) -> (Vec<(String, String)>, Vec<u64>) {
    use crate::ecs::components::GuiComponent;
    let (actions, targets) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        (std::mem::take(&mut s.actions), s.targets.clone())
    });
    let (mut activations, mut clicks) = (Vec::new(), Vec::new());
    for a in actions {
        let (Action::Focus(id) | Action::Click(id) | Action::Step(id, _)) = a;
        let Some(target) = targets.get(&id) else { continue };
        let step = |value: &mut f32, min: f32, max: f32| {
            if let Action::Step(_, sign) = a {
                *value = (*value + (max - min) * 0.1 * sign as f32).clamp(min.min(max), max.max(min));
            }
        };
        match target {
            Target::Entity(e) => {
                for (other, el) in world.query_mut::<&mut crate::ui::UIElement>() {
                    el.focused = other == *e;
                }
                if let Ok(mut sl) = world.get::<&mut crate::ui::UISlider>(*e) {
                    let (min, max) = (sl.min, sl.max);
                    step(&mut sl.value, min, max);
                }
                if matches!(a, Action::Click(_)) {
                    clicks.push(e.id() as u64);
                }
            }
            Target::Gui { holder, gui_id, node } => {
                if let Ok(mut g) = world.get::<&mut GuiComponent>(*holder) {
                    g.focused_node_id = Some(node.clone());
                    g.nav_focus = true;
                }
                if let Some(n) = assets.get_gui_mut(gui_id).and_then(|d| d.root.find_node_mut(node))
                    && let crate::ui::gui_document::GuiNodeType::Slider { value, min, max, .. } = &mut n.node_type
                {
                    step(value, *min, *max);
                }
                if matches!(a, Action::Click(_)) {
                    activations.push((gui_id.clone(), node.clone()));
                }
            }
        }
    }
    (activations, clicks)
}

/// Build the tree from the world's UI and keep it for [`latest`]; the
/// browsers' mirror is brought up to date here too.
pub fn gather(world: &World, assets: &crate::asset::AssetManager, ui_scale: &crate::ui::scaling::UiScale, screen: (f32, f32)) {
    let (pw, ph) = screen;
    if pw <= 0.0 || ph <= 0.0 {
        return;
    }
    let (sw, sh) = ui_scale.virtual_size(pw, ph);
    let k = if sw > 0.0 { pw / sw } else { 1.0 };
    let mut tree = Tree::default();
    let mut targets = HashMap::new();
    gather_entity_ui(world, (sw, sh), k, &mut tree, &mut targets);
    gather_gui(world, assets, (sw, sh), k, &mut tree, &mut targets);
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        tree.announcement = s.announcement.clone();
        if tree != s.latest {
            s.latest = tree;
            s.generation += 1;
        }
        s.targets = targets;
    });
    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    web::sync();
}

fn gather_entity_ui(world: &World, (sw, sh): (f32, f32), k: f32, tree: &mut Tree, targets: &mut HashMap<u64, Target>) {
    use crate::ui::{UIButton, UIElement, UIImage, UISlider, UIText, UITextInput, UIToggle};
    // A child's text names its button.
    let mut child_text: HashMap<Entity, String> = HashMap::new();
    for (_, (p, t)) in world.query::<(&crate::ecs::components::Parent, &UIText)>().iter() {
        child_text.entry(p.0).or_insert_with(|| t.text.clone());
    }
    let mut q = world.query::<(
        &UIElement,
        Option<&Accessible>,
        Option<&UIButton>,
        Option<&UIText>,
        Option<&UITextInput>,
        Option<&UIToggle>,
        Option<&UISlider>,
        Option<&UIImage>,
    )>();
    let mut rows: Vec<(f32, f32, Node, Entity)> = Vec::new();
    for (e, (el, acc, button, text, input, toggle, slider, image)) in q.iter() {
        if !el.visible || acc.is_some_and(|a| a.hidden) {
            continue;
        }
        let role = if button.is_some() {
            Role::Button
        } else if input.is_some() {
            Role::TextInput
        } else if toggle.is_some() {
            Role::Toggle
        } else if slider.is_some() {
            Role::Slider
        } else if text.is_some() {
            Role::Label
        } else if image.is_some() && acc.is_some_and(|a| !a.label.is_empty()) {
            Role::Image
        } else {
            continue;
        };
        // A text that is a button's own label is read as the button.
        if role == Role::Label
            && let Ok(p) = world.get::<&crate::ecs::components::Parent>(e)
            && world.get::<&UIButton>(p.0).is_ok()
        {
            continue;
        }
        let label = acc
            .map(|a| a.label.clone())
            .filter(|l| !l.is_empty())
            .or_else(|| text.map(|t| t.text.clone()))
            .or_else(|| child_text.get(&e).cloned())
            .or_else(|| input.map(|i| i.placeholder.clone()))
            .unwrap_or_default();
        // An empty line of text (a status not yet set) says nothing.
        if role == Role::Label && label.trim().is_empty() {
            continue;
        }
        let (min, max) = el.screen_rect(sw, sh);
        let bounds = [min.x * k, (sh - max.y) * k, (max.x - min.x) * k, (max.y - min.y) * k];
        let id = e.to_bits().get();
        if el.focused {
            tree.focus = Some(id);
        }
        let node = Node {
            id,
            role,
            label,
            description: acc.map(|a| a.description.clone()).unwrap_or_default(),
            value: input.map(|i| i.text.clone()).or_else(|| slider.map(|s| percent(s.value, s.min, s.max))),
            numeric: slider.map(|s| (s.value as f64, s.min as f64, s.max as f64)),
            toggled: toggle.map(|t| t.checked),
            bounds,
            focusable: el.focusable || matches!(role, Role::Button | Role::TextInput | Role::Toggle | Role::Slider),
        };
        rows.push((bounds[1], bounds[0], node, e));
    }
    // Reading order: top to bottom, left to right.
    rows.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for (_, _, node, e) in rows {
        targets.insert(node.id, Target::Entity(e));
        tree.nodes.push(node);
    }
}

/// A slider's or a bar's value as a reader says it: how far along its range.
fn percent(value: f32, min: f32, max: f32) -> String {
    let span = max - min;
    let t = if span.abs() > f32::EPSILON { (value - min) / span } else { 0.0 };
    format!("{}%", (t.clamp(0.0, 1.0) * 100.0).round() as i32)
}

fn gather_gui(
    world: &World,
    assets: &crate::asset::AssetManager,
    (sw, sh): (f32, f32),
    k: f32,
    tree: &mut Tree,
    targets: &mut HashMap<u64, Target>,
) {
    use crate::ecs::components::GuiComponent;
    use crate::ui::gui_document::GuiNodeType;
    for (holder, g) in world.query::<&GuiComponent>().iter() {
        if !g.visible {
            continue;
        }
        let Some(doc) = assets.get_gui(&g.gui_id) else { continue };
        let layout = crate::ui::layout::compute_layout(doc, sw, sh);
        for entry in layout.iter() {
            let Some(node) = doc.root.find_node(&entry.id) else { continue };
            if !node.style.visible || entry.opacity <= 0.0 {
                continue;
            }
            let (role, label, value, numeric, toggled) = match &node.node_type {
                GuiNodeType::Text { text, .. } => (Role::Label, text.clone(), None, None, None),
                GuiNodeType::Button { label, .. } => (Role::Button, label.clone(), None, None, None),
                GuiNodeType::TextInput { text, placeholder, .. } => {
                    (Role::TextInput, placeholder.clone(), Some(text.clone()), None, None)
                }
                GuiNodeType::Toggle { checked, .. } => (Role::Toggle, entry.id.clone(), None, None, Some(*checked)),
                GuiNodeType::Slider { value, min, max, .. } => (
                    Role::Slider,
                    entry.id.clone(),
                    Some(percent(*value, *min, *max)),
                    Some((*value as f64, *min as f64, *max as f64)),
                    None,
                ),
                GuiNodeType::ProgressBar { value, min, max, .. } => (
                    Role::ProgressBar,
                    entry.id.clone(),
                    Some(percent(*value, *min, *max)),
                    Some((*value as f64, *min as f64, *max as f64)),
                    None,
                ),
                _ => continue,
            };
            let acc = node.style.accessible.as_ref();
            if acc.is_some_and(|a| a.hidden) {
                continue;
            }
            let label = acc.map(|a| a.label.clone()).filter(|l| !l.is_empty()).unwrap_or(label);
            if label.trim().is_empty() && value.is_none() {
                continue;
            }
            let id = gui_id_of(holder, &entry.id);
            if g.focused_node_id.as_deref() == Some(entry.id.as_str()) {
                tree.focus = Some(id);
            }
            let r = entry.rect;
            tree.nodes.push(Node {
                id,
                role,
                label,
                description: acc.map(|a| a.description.clone()).unwrap_or_default(),
                value,
                numeric,
                toggled,
                bounds: [r.x * k, r.y * k, r.width * k, r.height * k],
                focusable: role != Role::Label,
            });
            targets.insert(id, Target::Gui { holder, gui_id: g.gui_id.clone(), node: entry.id.clone() });
        }
    }
}

/// The browsers' side: the tree mirrored into a visually hidden DOM that a
/// screen reader reads - a button element per button it can press, and a
/// polite live region that says what focus moved to and what a script
/// announced.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web {
    use super::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    /// One node's element, which owns its handlers - dropped with it.
    struct Slot {
        el: web_sys::HtmlElement,
        role: Role,
        _click: Option<Closure<dyn Fn()>>,
        _focus: Option<Closure<dyn Fn()>>,
    }

    #[derive(Default)]
    struct Mirror {
        root: Option<web_sys::HtmlElement>,
        live: Option<web_sys::HtmlElement>,
        built: u64,
        slots: HashMap<u64, Slot>,
        order: Vec<u64>,
        said_focus: Option<u64>,
        said_announcement: u64,
    }

    thread_local! {
        static MIRROR: RefCell<Mirror> = RefCell::new(Mirror::default());
    }

    const HIDDEN: &str = "position:absolute;left:-10000px;top:0;width:1px;height:1px;overflow:hidden;";

    fn make(doc: &web_sys::Document, tag: &str) -> Option<web_sys::HtmlElement> {
        doc.create_element(tag).ok()?.dyn_into().ok()
    }

    /// A new element for `n`: its tag and role, and - if it can be pressed -
    /// a click that presses it and a focus that makes it the game's focus,
    /// so a reader's arrow keys (which reach the page's own key handler)
    /// move what the reader is on.
    fn slot(doc: &web_sys::Document, n: &Node) -> Option<Slot> {
        let tag = match n.role {
            Role::Button => "button",
            Role::TextInput => "input",
            _ => "div",
        };
        let el = make(doc, tag)?;
        match n.role {
            Role::Toggle => {
                let _ = el.set_attribute("role", "checkbox");
            }
            Role::Slider => {
                let _ = el.set_attribute("role", "slider");
            }
            Role::ProgressBar => {
                let _ = el.set_attribute("role", "progressbar");
            }
            Role::Image => {
                let _ = el.set_attribute("role", "img");
            }
            _ => {}
        }
        let (mut click, mut focus) = (None, None);
        if n.focusable {
            if tag == "div" {
                let _ = el.set_attribute("tabindex", "0");
            }
            let id = n.id;
            let c = Closure::wrap(Box::new(move || request(Action::Click(id))) as Box<dyn Fn()>);
            el.set_onclick(Some(c.as_ref().unchecked_ref()));
            let f = Closure::wrap(Box::new(move || request(Action::Focus(id))) as Box<dyn Fn()>);
            el.set_onfocus(Some(f.as_ref().unchecked_ref()));
            (click, focus) = (Some(c), Some(f));
        }
        Some(Slot { el, role: n.role, _click: click, _focus: focus })
    }

    /// What may change on an element without replacing it.
    fn refresh(el: &web_sys::HtmlElement, n: &Node) {
        let set = |k: &str, v: &str| {
            if el.get_attribute(k).as_deref() != Some(v) {
                let _ = el.set_attribute(k, v);
            }
        };
        set("aria-label", &n.label);
        if !n.description.is_empty() {
            set("aria-description", &n.description);
        }
        if let Some(t) = n.toggled {
            set("aria-checked", if t { "true" } else { "false" });
        }
        if let Some((v, lo, hi)) = n.numeric {
            set("aria-valuenow", &v.to_string());
            set("aria-valuemin", &lo.to_string());
            set("aria-valuemax", &hi.to_string());
        }
        if let Some(v) = &n.value {
            if n.numeric.is_some() {
                set("aria-valuetext", v);
            }
        }
        if n.role == Role::Label && el.inner_text() != n.label {
            el.set_inner_text(&n.label);
        }
    }

    /// Bring the DOM up to date with [`latest`]: one element per node, kept
    /// while the node lasts (replacing it would take a reader's focus off
    /// it), and the live region spoken into when focus moves.
    pub fn sync() {
        let (tree, generation) = latest();
        MIRROR.with(|m| {
            let mut m = m.borrow_mut();
            let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
            if m.root.is_none() {
                let Some(root) = make(&doc, "div") else { return };
                let _ = root.set_attribute("style", HIDDEN);
                let _ = root.set_attribute("id", "jengine-accessibility");
                let Some(live) = make(&doc, "div") else { return };
                let _ = live.set_attribute("aria-live", "polite");
                let _ = live.set_attribute("style", HIDDEN);
                if let Some(body) = doc.body() {
                    let _ = body.append_child(&root);
                    let _ = body.append_child(&live);
                }
                m.root = Some(root);
                m.live = Some(live);
            }
            if m.built != generation {
                m.built = generation;
                let root = m.root.clone().unwrap();
                let wanted: Vec<u64> = tree.nodes.iter().map(|n| n.id).collect();
                // Gone, or now something else: removed.
                let stale: Vec<u64> = m
                    .slots
                    .iter()
                    .filter(|(id, s)| tree.nodes.iter().find(|n| n.id == **id).is_none_or(|n| n.role != s.role))
                    .map(|(id, _)| *id)
                    .collect();
                for id in stale {
                    if let Some(s) = m.slots.remove(&id) {
                        s.el.remove();
                    }
                }
                let mut added = false;
                for n in &tree.nodes {
                    if !m.slots.contains_key(&n.id) {
                        let Some(s) = slot(&doc, n) else { continue };
                        m.slots.insert(n.id, s);
                        added = true;
                    }
                    refresh(&m.slots[&n.id].el, n);
                }
                // Put in reading order only when it changed.
                if added || m.order != wanted {
                    for id in &wanted {
                        if let Some(s) = m.slots.get(id) {
                            let _ = root.append_child(&s.el);
                        }
                    }
                    m.order = wanted;
                }
            }
            // Say what focus moved to, and what a script announced.
            let mut say = None;
            if tree.focus != m.said_focus {
                m.said_focus = tree.focus;
                if let Some(n) = tree.nodes.iter().find(|n| Some(n.id) == tree.focus) {
                    say = Some(n.speech());
                }
            }
            if tree.announcement.0 != m.said_announcement {
                m.said_announcement = tree.announcement.0;
                say = Some(tree.announcement.1.clone());
            }
            if let (Some(text), Some(live)) = (say, &m.live) {
                live.set_inner_text(&text);
            }
        });
    }
}
