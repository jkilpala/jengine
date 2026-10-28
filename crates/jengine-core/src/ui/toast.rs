//! **Toasts** (Oct 2026): short notices that slide in from a corner of the
//! screen, stack, wait and slide away - an achievement unlocked, an item
//! found, a friend online.
//!
//! A [`ToastStack`] on any entity owns one corner's toasts.
//! `Toast.show(e, title, text, icon, seconds)` queues one; at most
//! `max_shown` are on screen at once, the newest nearest the corner and the
//! older ones easing along to make room, the rest waiting their turn. Each
//! slides in from beyond its edge over `slide` seconds, stays its seconds,
//! and slides out the way it came. Each toast is entity UI the module owns
//! (a [`ToastPart`]): a card (nine-sliced from `texture` when there is one),
//! its icon, its title and its wrapped text. Run at the start of
//! `update_ui_widgets`, which every host calls with the UI's size, on the
//! wall clock - so no host was wired, and a paused game still shows them.
//! The stack's own script's `on_toast(entity)` is called as each appears.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ui::{TextAlign, UIAnchor, UIElement, UIImage, UIText, UiNineSlice};

/// Which corner a stack lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToastCorner {
    #[default]
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}

/// One notice.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Toast {
    pub title: String,
    pub text: String,
    pub icon: String,
    pub seconds: f32,
}

/// A toast on screen.
#[derive(Debug, Clone, Default)]
pub struct Shown {
    pub toast: Toast,
    /// When it came on screen, and when it began to leave (ms).
    pub since_ms: f64,
    pub leaving_ms: Option<f64>,
    /// How far from the corner it sits now, UI pixels (eased towards its
    /// place in the stack).
    pub along: f32,
    /// The card, the icon, the title and the text.
    pub parts: [Option<Entity>; 4],
}

/// A corner of toasts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToastStack {
    pub corner: ToastCorner,
    /// Room between the stack and the screen's edges, UI pixels.
    pub margin: [f32; 2],
    /// A toast's size, and the room between two.
    pub size: [f32; 2],
    pub gap: f32,
    pub max_shown: u32,
    /// Seconds a toast stays when `Toast.show` gives none.
    pub seconds: f32,
    /// Seconds to slide in, and out.
    pub slide: f32,
    /// The card's picture, nine-sliced with `border` pixels kept; empty is
    /// a plain card in `color`.
    pub texture: String,
    pub border: f32,
    pub color: [f32; 4],
    pub title_color: [f32; 4],
    pub text_color: [f32; 4],
    pub title_size: f32,
    pub text_size: f32,
    /// The icon's size, UI pixels (0: no icon).
    pub icon_size: f32,
    pub padding: f32,
    pub z_order: i32,
    /// Pictures toasts will show as icons, loaded with the scene - a toast
    /// names its icon only when it is shown, too late to load a file then.
    pub icons: Vec<String>,
    /// Waiting, and on screen (never saved).
    #[serde(skip)]
    pub waiting: Vec<Toast>,
    #[serde(skip)]
    pub shown: Vec<Shown>,
    /// When it last ran (ms), for easing (never saved).
    #[serde(skip)]
    pub last_ms: Option<f64>,
}

impl Default for ToastStack {
    fn default() -> Self {
        Self {
            corner: ToastCorner::TopRight,
            margin: [24.0, 24.0],
            size: [360.0, 86.0],
            gap: 10.0,
            max_shown: 4,
            seconds: 4.0,
            slide: 0.35,
            texture: String::new(),
            border: 16.0,
            color: [0.08, 0.09, 0.13, 0.92],
            title_color: [1.0, 0.85, 0.4, 1.0],
            text_color: [0.88, 0.9, 0.95, 1.0],
            title_size: 20.0,
            text_size: 16.0,
            icon_size: 56.0,
            padding: 14.0,
            z_order: 220,
            icons: Vec::new(),
            waiting: Vec::new(),
            shown: Vec::new(),
            last_ms: None,
        }
    }
}

/// An element a toast stack owns.
#[derive(Debug, Clone, Copy)]
pub struct ToastPart {
    pub owner: Entity,
}

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

/// Where a toast `along` pixels from the corner sits, `slid` of the way in
/// (0 off screen, 1 in place): the bottom-left corner of its card, UI
/// pixels (y up), on a screen `w` x `h`.
pub fn card_at(s: &ToastStack, along: f32, slid: f32, w: f32, h: f32) -> Vec2 {
    let size = Vec2::from(s.size);
    let (mx, my) = (s.margin[0], s.margin[1]);
    let right = matches!(s.corner, ToastCorner::TopRight | ToastCorner::BottomRight);
    let top = matches!(s.corner, ToastCorner::TopRight | ToastCorner::TopLeft);
    let x_in = if right { w - mx - size.x } else { mx };
    let x_out = if right { w + 8.0 } else { -size.x - 8.0 };
    let x = x_out + (x_in - x_out) * ease_out(slid);
    let y = if top { h - my - size.y - along } else { my + along };
    Vec2::new(x, y)
}

/// One frame of every toast stack at `now_ms`, for a UI `w` x `h` pixels.
pub fn update_toasts_at(world: &mut World, w: f32, h: f32, now_ms: f64) {
    let stacks: Vec<Entity> = world.query::<&ToastStack>().iter().map(|(e, _)| e).collect();
    let mut keep: Vec<Entity> = Vec::new();
    let mut appeared: Vec<Entity> = Vec::new();
    for e in stacks {
        let mut make: Vec<usize> = Vec::new();
        {
            let Ok(mut s) = world.get::<&mut ToastStack>(e) else { continue };
            let dt = s.last_ms.map_or(0.0, |l| ((now_ms - l) / 1000.0).clamp(0.0, 0.25)) as f32;
            s.last_ms = Some(now_ms);
            let slide_ms = (s.slide.max(0.01) * 1000.0) as f64;
            // Those whose time is up begin to leave; those gone are dropped.
            for t in s.shown.iter_mut() {
                let life = if t.toast.seconds > 0.0 { t.toast.seconds } else { 4.0 };
                if t.leaving_ms.is_none() && now_ms - t.since_ms >= (life * 1000.0) as f64 + slide_ms {
                    t.leaving_ms = Some(now_ms);
                }
            }
            // (Their parts go in the sweep below, with any no stack owns.)
            s.shown.retain(|t| t.leaving_ms.is_none_or(|l| now_ms - l < slide_ms));
            // The next waiting take the places free.
            while (s.shown.iter().filter(|t| t.leaving_ms.is_none()).count() as u32) < s.max_shown.max(1) && !s.waiting.is_empty() {
                let mut toast = s.waiting.remove(0);
                if toast.seconds <= 0.0 {
                    toast.seconds = s.seconds;
                }
                s.shown.push(Shown { toast, since_ms: now_ms, ..Default::default() });
                make.push(s.shown.len() - 1);
                appeared.push(e);
            }
            // Places: newest nearest the corner; a leaving one keeps its own.
            let step = s.size[1] + s.gap;
            let staying: Vec<usize> = (0..s.shown.len()).rev().filter(|&i| s.shown[i].leaving_ms.is_none()).collect();
            let k = 1.0 - (-14.0 * dt).exp();
            for (slot, &i) in staying.iter().enumerate() {
                let target = slot as f32 * step;
                let t = &mut s.shown[i];
                t.along = if make.contains(&i) { target } else { t.along + (target - t.along) * k };
            }
        }
        // Make the new ones' parts, then place every part.
        let s = (*world.get::<&ToastStack>(e).unwrap()).clone();
        let mut parts_for: Vec<(usize, [Option<Entity>; 4])> = Vec::new();
        for &i in &make {
            let t = &s.shown[i].toast;
            let card = UIImage {
                texture_id: s.texture.clone(),
                color: s.color,
                nine_slice: (!s.texture.is_empty()).then_some(UiNineSlice {
                    left: s.border,
                    right: s.border,
                    top: s.border,
                    bottom: s.border,
                    scale: 1.0,
                }),
                ..Default::default()
            };
            let el = UIElement { anchor: UIAnchor::BottomLeft, size: Vec2::from(s.size), visible: true, z_order: s.z_order, ..Default::default() };
            let card = world.spawn((el.clone(), card, ToastPart { owner: e }));
            let icon = (!t.icon.is_empty() && s.icon_size > 0.0).then(|| {
                world.spawn((
                    UIElement { z_order: s.z_order + 1, size: Vec2::splat(s.icon_size), ..el.clone() },
                    UIImage { texture_id: t.icon.clone(), color: [1.0; 4], ..Default::default() },
                    ToastPart { owner: e },
                ))
            });
            let title = world.spawn((
                UIElement { z_order: s.z_order + 1, ..el.clone() },
                UIText { text: t.title.clone(), font_size: s.title_size, color: s.title_color, alignment: TextAlign::Left, ..Default::default() },
                ToastPart { owner: e },
            ));
            let text_w = s.size[0] - s.padding * 3.0 - if icon.is_some() { s.icon_size } else { 0.0 };
            let (wrapped, _) = crate::ui::text_wrap::wrap(&t.text, s.text_size, text_w.max(40.0));
            let body = world.spawn((
                UIElement { z_order: s.z_order + 1, ..el.clone() },
                UIText { text: wrapped, font_size: s.text_size, color: s.text_color, alignment: TextAlign::Left, ..Default::default() },
                ToastPart { owner: e },
            ));
            parts_for.push((i, [Some(card), icon, Some(title), Some(body)]));
        }
        if let Ok(mut real) = world.get::<&mut ToastStack>(e) {
            for (i, parts) in &parts_for {
                real.shown[*i].parts = *parts;
            }
        }
        let s = (*world.get::<&ToastStack>(e).unwrap()).clone();
        let slide_ms = (s.slide.max(0.01) * 1000.0) as f64;
        for t in &s.shown {
            let slid = match t.leaving_ms {
                Some(l) => 1.0 - ((now_ms - l) / slide_ms) as f32,
                None => ((now_ms - t.since_ms) / slide_ms) as f32,
            }
            .clamp(0.0, 1.0);
            let at = card_at(&s, t.along, slid, w, h);
            let size = Vec2::from(s.size);
            let pad = s.padding;
            let icon_w = if t.parts[1].is_some() { s.icon_size + pad } else { 0.0 };
            let title_h = s.title_size * 1.4;
            let places = [
                (at, size),
                (at + Vec2::new(pad, (size.y - s.icon_size) * 0.5), Vec2::splat(s.icon_size)),
                (at + Vec2::new(pad + icon_w, size.y - pad - title_h), Vec2::new(size.x - 2.0 * pad - icon_w, title_h)),
                (at + Vec2::new(pad + icon_w, pad), Vec2::new(size.x - 2.0 * pad - icon_w, size.y - 2.0 * pad - title_h)),
            ];
            for (part, (off, sz)) in t.parts.iter().zip(places) {
                let Some(x) = part else { continue };
                keep.push(*x);
                if let Ok(mut el) = world.get::<&mut UIElement>(*x) {
                    el.offset = off;
                    el.size = sz;
                }
            }
        }
    }
    // Parts no toast on screen owns any more - gone ones', a removed
    // stack's - go.
    let stale: Vec<Entity> = world.query::<&ToastPart>().iter().map(|(x, _)| x).filter(|x| !keep.contains(x)).collect();
    for x in stale {
        let _ = world.despawn(x);
    }
    for e in appeared {
        crate::ecs::callbacks::push(e, "on_toast");
    }
}

/// One frame of every toast stack, on the wall clock.
pub fn update_toasts(world: &mut World, w: f32, h: f32) {
    update_toasts_at(world, w, h, crate::profiling::now_ms());
}

/// `Toast.show(e, title, text, icon, seconds)`: queue a toast on the stack
/// (seconds 0 or less: the stack's own).
pub fn show(world: &mut World, e: Entity, title: &str, text: &str, icon: &str, seconds: f32) -> Result<(), String> {
    let mut s = world.get::<&mut ToastStack>(e).map_err(|_| "no toast_stack on that entity".to_string())?;
    s.waiting.push(Toast { title: title.into(), text: text.into(), icon: icon.into(), seconds });
    Ok(())
}

/// `Toast.set(e, field, value)`: any field by name, keeping the toasts.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut s = world.get::<&mut ToastStack>(e).map_err(|_| "no toast_stack on that entity".to_string())?;
    let mut v = serde_json::to_value(&*s).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("toast_stack has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    let mut next: ToastStack = serde_json::from_value(v).map_err(|e| format!("toast_stack.{field}: {e}"))?;
    next.waiting = std::mem::take(&mut s.waiting);
    next.shown = std::mem::take(&mut s.shown);
    next.last_ms = s.last_ms;
    *s = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_top_right_toast_slides_in_from_the_right_edge() {
        let s = ToastStack::default();
        let out = card_at(&s, 0.0, 0.0, 1280.0, 720.0);
        let half = card_at(&s, 0.0, 0.5, 1280.0, 720.0);
        let home = card_at(&s, 0.0, 1.0, 1280.0, 720.0);
        assert!(out.x >= 1280.0, "off screen to start: {out}");
        assert!((home.x + 360.0 - (1280.0 - 24.0)).abs() < 1e-3, "in place against the margin: {home}");
        assert!(half.x > home.x && half.x < out.x, "on its way: {half}");
        assert!((home.y + 86.0 - (720.0 - 24.0)).abs() < 1e-3, "top edge at the margin: {home}");
        let second = card_at(&s, 96.0, 1.0, 1280.0, 720.0);
        assert!((second.y - (home.y - 96.0)).abs() < 1e-3, "the next one down: {second}");
        let bl = ToastStack { corner: ToastCorner::BottomLeft, ..Default::default() };
        let p = card_at(&bl, 0.0, 1.0, 1280.0, 720.0);
        assert_eq!(p, Vec2::new(24.0, 24.0));
    }

    #[test]
    fn toasts_stack_queue_and_leave() {
        let mut w = World::new();
        let e = w.spawn((ToastStack { max_shown: 2, seconds: 2.0, slide: 0.5, ..Default::default() },));
        for k in 0..3 {
            show(&mut w, e, &format!("T{k}"), "words", "", 0.0).unwrap();
        }
        update_toasts_at(&mut w, 1280.0, 720.0, 0.0);
        {
            let s = w.get::<&ToastStack>(e).unwrap();
            assert_eq!(s.shown.len(), 2, "two on screen");
            assert_eq!(s.waiting.len(), 1, "one waiting");
            // Newest (T1) nearest the corner, T0 one place along.
            assert_eq!(s.shown[1].along, 0.0);
            assert_eq!(s.shown[0].along, 96.0);
        }
        assert_eq!(w.query::<&ToastPart>().iter().count(), 6, "card, title and text each - no icons");
        // Past their time and their slide out: gone, the third shown.
        let mut now = 0.0;
        while now < 3200.0 {
            now += 16.0;
            update_toasts_at(&mut w, 1280.0, 720.0, now);
        }
        let s = w.get::<&ToastStack>(e).unwrap();
        assert_eq!(s.shown.len(), 1, "{:?}", s.shown.iter().map(|t| &t.toast.title).collect::<Vec<_>>());
        assert_eq!(s.shown[0].toast.title, "T2");
        assert!(s.waiting.is_empty());
        drop(s);
        assert_eq!(w.query::<&ToastPart>().iter().count(), 3, "the gone ones' parts went with them");
    }

    #[test]
    fn set_keeps_the_toasts() {
        let mut w = World::new();
        let e = w.spawn((ToastStack::default(),));
        show(&mut w, e, "Hi", "", "", 0.0).unwrap();
        set_field(&mut w, e, "corner", &serde_json::json!("bottom_left")).unwrap();
        let s = w.get::<&ToastStack>(e).unwrap();
        assert_eq!(s.corner, ToastCorner::BottomLeft);
        assert_eq!(s.waiting.len(), 1);
    }
}
