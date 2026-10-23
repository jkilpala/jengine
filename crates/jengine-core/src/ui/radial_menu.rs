//! **Radial menus** (Oct 2026): a wheel of sections chosen by direction from
//! its middle - a spell wheel, a weapon wheel, an emote ring - with the
//! mouse or a stick.
//!
//! A [`UIRadialMenu`] on an entity-UI element (its middle is the wheel's)
//! names its `sections`: other entity-UI elements, by entity name, laid out
//! round the wheel `radius` UI pixels out, the first at `start` degrees
//! (90 is the top) and the rest clockwise. Each frame ([`update_radial_menus`],
//! at the start of `update_ui_widgets`, which every host calls with the
//! pointer in UI pixels - so no host was wired) the section the pointer or
//! the left stick points at is **hovered**: grown by `hover_scale` and
//! tinted `hover_tint`, the rest `idle_tint`. Nearer the middle than
//! `dead_zone` (or a stick at rest) hovers nothing. A click, the pad's A,
//! Enter or Space **picks** the hovered section: the menu's custom
//! `RadialMenu` fields (`hovered`, `picked`, `picks`, `label`) are written
//! and its own script's `on_radial_pick` called (`ecs::callbacks`). `open`
//! false hides the wheel and its sections.

use std::collections::HashMap;

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{CustomComponents, Name};
use crate::input::{InputState, KeyCode};
use crate::ui::{UIAnchor, UIElement, UIImage};

/// A wheel of sections chosen by direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UIRadialMenu {
    /// The sections' entity names, first at `start`, then clockwise.
    pub sections: Vec<String>,
    /// How far out the sections' middles sit, UI pixels.
    pub radius: f32,
    /// Nearer the middle than this, the pointer hovers nothing.
    pub dead_zone: f32,
    /// Where the first section sits, degrees (0 right, 90 the top).
    pub start: f32,
    /// How much the hovered section grows.
    pub hover_scale: f32,
    /// The hovered section's tint, and the others'.
    pub hover_tint: [f32; 4],
    pub idle_tint: [f32; 4],
    /// Shown and answering; closed, the wheel and its sections are hidden.
    pub open: bool,
    /// A direction to point at instead of the pointer or the stick,
    /// degrees (0 right, 90 up) - a script choosing for the player, a
    /// tutorial. None: theirs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aim: Option<f32>,
    #[serde(skip)]
    pub state: RadialState,
}

impl Default for UIRadialMenu {
    fn default() -> Self {
        Self {
            sections: Vec::new(),
            radius: 130.0,
            dead_zone: 36.0,
            start: 90.0,
            hover_scale: 1.3,
            hover_tint: [1.0, 1.0, 1.0, 1.0],
            idle_tint: [0.7, 0.7, 0.78, 0.9],
            open: true,
            aim: None,
            state: RadialState::default(),
        }
    }
}

/// What a wheel is doing (never saved).
#[derive(Debug, Clone, Default)]
pub struct RadialState {
    pub hovered: Option<usize>,
    /// How many picks so far, and the last.
    pub picks: u32,
    pub picked: Option<usize>,
    /// Each section's own size, before it was grown.
    pub sizes: HashMap<u64, Vec2>,
    /// What was last written to the custom fields.
    published: Option<(Option<usize>, u32, bool)>,
}

/// The section a direction `angle` (radians, 0 right, counter-clockwise)
/// points at, for `n` sections the first at `start` degrees, clockwise.
pub fn section_at(angle: f32, n: usize, start_deg: f32) -> usize {
    if n == 0 {
        return 0;
    }
    let step = std::f32::consts::TAU / n as f32;
    let from_first = (start_deg.to_radians() - angle).rem_euclid(std::f32::consts::TAU);
    ((from_first / step).round() as usize) % n
}

/// Where section `i` of `n` sits from the middle, for radius `r`.
pub fn section_offset(i: usize, n: usize, start_deg: f32, r: f32) -> Vec2 {
    let a = start_deg.to_radians() - i as f32 * std::f32::consts::TAU / n.max(1) as f32;
    Vec2::new(a.cos(), a.sin()) * r
}

fn named(world: &World, name: &str) -> Option<Entity> {
    world.query::<(&Name, &UIElement)>().iter().find(|(_, (n, _))| n.0 == name).map(|(e, _)| e)
}

fn confirm(input: &InputState) -> bool {
    input.mouse_left_just_pressed
        || (0..input.pad_count()).any(|i| input.is_pad_button_just_pressed(i, 0))
        || input.is_just_pressed(KeyCode::Enter)
        || input.is_just_pressed(KeyCode::Space)
}

/// One frame of every radial menu. `mouse` is in Y-up UI pixels.
pub fn update_radial_menus(world: &mut World, input: &InputState, mouse: Vec2, w: f32, h: f32) {
    let menus: Vec<Entity> = world.query::<&UIRadialMenu>().iter().map(|(e, _)| e).collect();
    if menus.is_empty() {
        return;
    }
    // The left stick of the first pad that has one pushed.
    let stick = (0..input.pad_count())
        .map(|p| Vec2::new(input.pad_axis(p, 0), input.pad_axis(p, 1)))
        .find(|v| v.length() > 0.5);
    for menu in menus {
        let Ok(m) = world.get::<&UIRadialMenu>(menu).map(|m| (*m).clone()) else { continue };
        let Ok(el) = world.get::<&UIElement>(menu).map(|e| (*e).clone()) else { continue };
        let (lo, hi) = el.screen_rect(w, h);
        let centre = (lo + hi) * 0.5;
        let n = m.sections.len();
        let open = m.open && el.visible;
        let entities: Vec<Option<Entity>> = m.sections.iter().map(|s| named(world, s)).collect();
        let mut state = m.state.clone();
        // What is pointed at.
        let towards = match (m.aim, stick) {
            (Some(a), _) => Some(Vec2::from_angle(a.to_radians()) * m.radius),
            (None, Some(s)) => Some(s),
            (None, None) => {
                let d = mouse - centre;
                (d.length() >= m.dead_zone && d.length() <= m.radius * 2.0).then_some(d)
            }
        };
        let hovered = if open && n > 0 { towards.map(|d| section_at(d.y.atan2(d.x), n, m.start)) } else { None };
        state.hovered = hovered;
        if open && hovered.is_some() && confirm(input) {
            state.picks += 1;
            state.picked = hovered;
            crate::ecs::callbacks::push(menu, "on_radial_pick");
        }
        // Lay the sections out, grow and tint the hovered one.
        for (i, e) in entities.iter().enumerate() {
            let Some(e) = *e else { continue };
            let key = e.to_bits().get();
            if let Ok(mut sel) = world.get::<&mut UIElement>(e) {
                let base = *state.sizes.entry(key).or_insert(sel.size);
                let grow = if hovered == Some(i) { m.hover_scale } else { 1.0 };
                sel.size = base * grow;
                sel.visible = open;
                sel.anchor = UIAnchor::BottomLeft;
                sel.offset = centre + section_offset(i, n, m.start, m.radius) - sel.size * 0.5;
                sel.z_order = el.z_order + if hovered == Some(i) { 2 } else { 1 };
            }
            if let Ok(mut img) = world.get::<&mut UIImage>(e) {
                img.color = if hovered == Some(i) { m.hover_tint } else { m.idle_tint };
            }
        }
        // What a script reads.
        let now = (state.hovered, state.picks, open);
        if state.published != Some(now) {
            let label = state.picked.and_then(|i| m.sections.get(i).cloned()).unwrap_or_default();
            let v = serde_json::json!({
                "hovered": state.hovered.map_or(-1, |i| i as i64),
                "picked": state.picked.map_or(-1, |i| i as i64),
                "picks": state.picks,
                "label": label,
            });
            let missing = match world.get::<&mut CustomComponents>(menu) {
                Ok(mut c) => {
                    c.0.insert("RadialMenu".into(), v);
                    None
                }
                Err(_) => Some(v),
            };
            if let Some(v) = missing {
                let mut map = HashMap::new();
                map.insert("RadialMenu".into(), v);
                let _ = world.insert_one(menu, CustomComponents(map));
            }
            state.published = Some(now);
        }
        if let Ok(mut real) = world.get::<&mut UIRadialMenu>(menu) {
            real.state = state;
        }
    }
}

/// `Radial.set(e, field, value)`: any field by name (`open` to show it).
pub fn set_field(world: &mut World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut m = world.get::<&mut UIRadialMenu>(entity).map_err(|_| "no ui_radial_menu on that entity".to_string())?;
    let mut v = serde_json::to_value(&*m).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    // `aim` is left out while it is none, so it is not in the list.
    if !obj.contains_key(field) && field != "aim" {
        return Err(format!("ui_radial_menu has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    let mut next: UIRadialMenu = serde_json::from_value(v).map_err(|e| format!("ui_radial_menu.{field}: {e}"))?;
    next.state = std::mem::take(&mut m.state);
    *m = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(x: f32, y: f32, w: f32) -> UIElement {
        UIElement { anchor: UIAnchor::BottomLeft, offset: Vec2::new(x, y), size: Vec2::splat(w), visible: true, ..Default::default() }
    }

    /// A wheel at the middle of an 800 x 600 screen, four sections: N E S W.
    fn wheel(world: &mut World) -> (Entity, Vec<Entity>) {
        let names = ["Fire", "Ice", "Wind", "Earth"];
        let parts: Vec<Entity> = names
            .iter()
            .map(|n| world.spawn((Name((*n).into()), element(0.0, 0.0, 40.0), UIImage::default())))
            .collect();
        let menu = world.spawn((
            element(350.0, 250.0, 100.0),
            UIRadialMenu { sections: names.iter().map(|s| s.to_string()).collect(), radius: 100.0, ..Default::default() },
        ));
        (menu, parts)
    }

    #[test]
    fn sections_sit_round_the_wheel_clockwise_from_the_top() {
        assert_eq!(section_at(std::f32::consts::FRAC_PI_2, 4, 90.0), 0, "up is the first");
        assert_eq!(section_at(0.0, 4, 90.0), 1, "right is the second: clockwise");
        assert_eq!(section_at(-std::f32::consts::FRAC_PI_2, 4, 90.0), 2);
        assert_eq!(section_at(std::f32::consts::PI, 4, 90.0), 3);
        assert_eq!(section_at(0.3, 4, 90.0), 1, "near enough to right");
        let o = section_offset(1, 4, 90.0, 100.0);
        assert!((o - Vec2::new(100.0, 0.0)).length() < 1e-3, "{o}");
    }

    #[test]
    fn the_pointer_hovers_a_section_and_a_click_picks_it() {
        let mut world = World::new();
        let (menu, parts) = wheel(&mut world);
        let mut input = InputState::new();
        // The pointer to the right of the middle (400, 300).
        update_radial_menus(&mut world, &input, Vec2::new(480.0, 305.0), 800.0, 600.0);
        let m = (*world.get::<&UIRadialMenu>(menu).unwrap()).clone();
        assert_eq!(m.state.hovered, Some(1), "Ice, on the right");
        let ice = (*world.get::<&UIElement>(parts[1]).unwrap()).clone();
        assert!((ice.size.x - 52.0).abs() < 1e-3, "grown: {}", ice.size.x);
        let (lo, hi) = ice.screen_rect(800.0, 600.0);
        assert!(((lo + hi) * 0.5 - Vec2::new(500.0, 300.0)).length() < 1e-3, "on the right of the wheel");
        assert_eq!(world.get::<&UIImage>(parts[0]).unwrap().color, m.idle_tint);
        // A click picks it.
        input.mouse_left_just_pressed = true;
        update_radial_menus(&mut world, &input, Vec2::new(480.0, 305.0), 800.0, 600.0);
        let c = world.get::<&CustomComponents>(menu).unwrap().0["RadialMenu"].clone();
        assert_eq!((c["picked"].as_i64(), c["picks"].as_u64(), c["label"].as_str()), (Some(1), Some(1), Some("Ice")));
        // In the dead zone, nothing.
        input.mouse_left_just_pressed = false;
        update_radial_menus(&mut world, &input, Vec2::new(405.0, 300.0), 800.0, 600.0);
        assert_eq!(world.get::<&UIRadialMenu>(menu).unwrap().state.hovered, None);
        assert!((world.get::<&UIElement>(parts[1]).unwrap().size.x - 40.0).abs() < 1e-3, "back to its size");
    }

    #[test]
    fn a_script_can_aim_the_wheel() {
        let mut world = World::new();
        let (menu, _) = wheel(&mut world);
        set_field(&mut world, menu, "aim", &serde_json::json!(180.0)).unwrap();
        // The pointer says right; the aim says left, and wins.
        update_radial_menus(&mut world, &InputState::new(), Vec2::new(480.0, 300.0), 800.0, 600.0);
        assert_eq!(world.get::<&UIRadialMenu>(menu).unwrap().state.hovered, Some(3), "Earth, on the left");
        set_field(&mut world, menu, "aim", &serde_json::Value::Null).unwrap();
        update_radial_menus(&mut world, &InputState::new(), Vec2::new(480.0, 300.0), 800.0, 600.0);
        assert_eq!(world.get::<&UIRadialMenu>(menu).unwrap().state.hovered, Some(1), "the pointer again");
    }

    #[test]
    fn closed_it_hides_and_hovers_nothing() {
        let mut world = World::new();
        let (menu, parts) = wheel(&mut world);
        set_field(&mut world, menu, "open", &serde_json::json!(false)).unwrap();
        update_radial_menus(&mut world, &InputState::new(), Vec2::new(480.0, 300.0), 800.0, 600.0);
        assert!(parts.iter().all(|p| !world.get::<&UIElement>(*p).unwrap().visible));
        assert_eq!(world.get::<&UIRadialMenu>(menu).unwrap().state.hovered, None);
    }
}
