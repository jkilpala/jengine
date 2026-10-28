//! **Speech bubbles** (Oct 2026): what a character says, in a bubble over
//! its head that sizes to its words, points its tail at the speaker, stays
//! on screen and keeps out of the other bubbles' way.
//!
//! A [`SpeechBubble`] on a speaker holds what it says (`text`, set by
//! `Bubble.say(e, text, seconds)`) and how it looks. Each frame
//! ([`update_speech_bubbles`], at the start of `update_ui_widgets`, which
//! every host calls with the UI's size - so no host was wired) the speaker's
//! head (its position plus `offset`) is put on the screen through the screen
//! camera's view as last drawn (`offscreen_marker::main_view`), the words
//! are wrapped to `max_width` with the built-in font's own measurements, and
//! the bubbles are arranged ([`arrange`]): each above its speaker, pushed up
//! clear of any it would overlap, then kept inside the screen. Each bubble
//! is two entity-UI elements the module owns (a [`BubblePart`]): the bubble
//! - an image, nine-sliced from `texture` when there is one - with the text
//! on it, and the tail, stretched from the bubble down to the speaker. A
//! bubble goes after `duration` seconds (0 keeps it), and with its speaker.
//! Words appear at `reveal_speed` characters a second, as UI text's do.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Transform, WorldTransform};
use crate::ui::{TextAlign, UIAnchor, UIElement, UIImage, UIText, UiNineSlice};

/// What a speaker says, in a bubble over its head.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpeechBubble {
    /// The words; empty says nothing.
    pub text: String,
    /// Seconds the bubble stays; 0 keeps it until the words change.
    pub duration: f32,
    /// The widest a line may be, UI pixels, before it wraps.
    pub max_width: f32,
    pub font_size: f32,
    /// Room between the words and the bubble's edge.
    pub padding: f32,
    /// Where the tail points, from the speaker's position, world units.
    pub offset: [f32; 2],
    /// The bubble's picture, nine-sliced with `border` pixels kept at the
    /// edges; empty draws a plain rectangle.
    pub texture: String,
    pub border: f32,
    /// The tail's picture, its point at the bottom; empty draws a stem.
    pub tail_texture: String,
    /// How long the tail is at least, and how wide.
    pub tail: [f32; 2],
    pub color: [f32; 4],
    pub text_color: [f32; 4],
    /// Characters a second (0 shows them at once).
    pub reveal_speed: f32,
    pub z_order: i32,
    #[serde(skip)]
    pub state: BubbleState,
}

impl Default for SpeechBubble {
    fn default() -> Self {
        Self {
            text: String::new(),
            duration: 4.0,
            max_width: 260.0,
            font_size: 18.0,
            padding: 12.0,
            offset: [0.0, 60.0],
            texture: String::new(),
            border: 14.0,
            tail_texture: String::new(),
            tail: [14.0, 18.0],
            color: [1.0, 1.0, 1.0, 0.95],
            text_color: [0.08, 0.08, 0.12, 1.0],
            reveal_speed: 0.0,
            z_order: 60,
            state: BubbleState::default(),
        }
    }
}

/// A bubble's running state (never saved).
#[derive(Debug, Clone, Default)]
pub struct BubbleState {
    /// The words the bubble was made for, and when (ms).
    pub shown: String,
    pub since_ms: Option<f64>,
    /// The elements drawing it.
    pub bubble: Option<Entity>,
    pub tail: Option<Entity>,
    /// Where it was put last frame, UI pixels: its rectangle's middle and
    /// size.
    pub rect: Option<(Vec2, Vec2)>,
}

/// An element a speech bubble owns.
#[derive(Debug, Clone, Copy)]
pub struct BubblePart {
    pub owner: Entity,
}


pub use super::text_wrap::wrap;

/// Where bubbles go: for each, its middle (UI pixels, y up). `anchors` are
/// where the tails point, `sizes` the bubbles; each sits `lift` above its
/// anchor, pushed up clear of those placed before it (lowest anchor first),
/// `gap` apart, then kept `margin` inside `screen`.
pub fn arrange(anchors: &[Vec2], sizes: &[Vec2], screen: Vec2, lift: f32, gap: f32, margin: f32) -> Vec<Vec2> {
    let mut order: Vec<usize> = (0..anchors.len()).collect();
    order.sort_by(|a, b| anchors[*a].y.total_cmp(&anchors[*b].y).then(anchors[*a].x.total_cmp(&anchors[*b].x)));
    let mut out = vec![Vec2::ZERO; anchors.len()];
    let mut placed: Vec<(Vec2, Vec2)> = Vec::new();
    for i in order {
        let half = sizes[i] * 0.5;
        let lo_x = margin + half.x;
        let hi_x = (screen.x - margin - half.x).max(lo_x);
        let mut c = Vec2::new(anchors[i].x.clamp(lo_x, hi_x), (anchors[i].y + lift + half.y).max(margin + half.y));
        for _ in 0..64 {
            let hit = placed.iter().find(|(pc, ph)| (c.x - pc.x).abs() < half.x + ph.x + gap && (c.y - pc.y).abs() < half.y + ph.y + gap);
            match hit {
                Some((pc, ph)) => c.y = pc.y + ph.y + half.y + gap,
                None => break,
            }
        }
        c.y = c.y.min(screen.y - margin - half.y).max(margin + half.y);
        placed.push((c, half));
        out[i] = c;
    }
    out
}

/// A world point on the screen of `view`, in UI pixels (y up) for a screen
/// `w` x `h`.
fn to_screen(view: &crate::ecs::views::MainView, p: Vec2, w: f32, h: f32) -> Vec2 {
    let d = p - view.center;
    let l = Vec2::new(d.dot(view.axis_x) / view.half.x.max(1e-3), d.dot(view.axis_y) / view.half.y.max(1e-3));
    Vec2::new(w * 0.5 * (1.0 + l.x), h * 0.5 * (1.0 + l.y))
}

fn element(centre: Vec2, size: Vec2, z: i32) -> UIElement {
    UIElement { anchor: UIAnchor::BottomLeft, offset: centre - size * 0.5, size, visible: true, z_order: z, ..Default::default() }
}

/// One frame of every speech bubble, for a UI `w` x `h` pixels.
pub fn update_speech_bubbles(world: &mut World, w: f32, h: f32) {
    let now = crate::profiling::now_ms();
    let speakers: Vec<(Entity, SpeechBubble, Vec2)> = world
        .query::<(&SpeechBubble, &Transform, Option<&WorldTransform>)>()
        .iter()
        .map(|(e, (b, t, wt))| (e, b.clone(), wt.map_or(t.position, |w| w.position)))
        .collect();
    let view = crate::ecs::views::main_view();
    // Who speaks now, and the size of what they say.
    let mut live: Vec<(Entity, SpeechBubble, Vec2, String, Vec2, bool)> = Vec::new();
    for (e, mut b, p) in speakers {
        // Said again (`say` clears the clock): the bubble is made afresh,
        // so a typewriter types the line again.
        let fresh = b.state.since_ms.is_none();
        if b.state.shown != b.text || fresh {
            b.state.shown = b.text.clone();
            b.state.since_ms = Some(now);
        }
        let age = (now - b.state.since_ms.unwrap_or(now)) / 1000.0;
        let speaking = !b.text.trim().is_empty() && (b.duration <= 0.0 || age < b.duration as f64);
        if let Ok(mut real) = world.get::<&mut SpeechBubble>(e) {
            real.state.shown = b.state.shown.clone();
            real.state.since_ms = b.state.since_ms;
        }
        if !speaking || view.is_none() {
            continue;
        }
        let (lines, words) = wrap(&b.text, b.font_size, b.max_width - 2.0 * b.padding);
        let size = words + Vec2::splat(2.0 * b.padding);
        let anchor = to_screen(view.as_ref().unwrap(), p + Vec2::from(b.offset), w, h);
        live.push((e, b, anchor, lines, size, fresh));
    }
    let anchors: Vec<Vec2> = live.iter().map(|l| l.2).collect();
    let sizes: Vec<Vec2> = live.iter().map(|l| l.4).collect();
    let lift = live.iter().fold(0.0f32, |a, l| a.max(l.1.tail[1]));
    let centres = arrange(&anchors, &sizes, Vec2::new(w, h), lift, 8.0, 8.0);
    // Draw them: make or move each bubble's two elements.
    let mut keep: Vec<Entity> = Vec::new();
    for ((e, b, anchor, lines, size, fresh), centre) in live.into_iter().zip(centres) {
        let bubble_el = element(centre, size, b.z_order);
        let image = UIImage {
            texture_id: b.texture.clone(),
            color: b.color,
            nine_slice: (!b.texture.is_empty()).then_some(UiNineSlice {
                left: b.border,
                right: b.border,
                top: b.border,
                bottom: b.border,
                scale: 1.0,
            }),
            ..Default::default()
        };
        let text = UIText {
            text: lines,
            font_size: b.font_size,
            color: b.text_color,
            alignment: TextAlign::Center,
            reveal_speed: b.reveal_speed,
            ..Default::default()
        };
        // The tail: from the bubble's bottom edge down to the anchor.
        let bottom = centre.y - size.y * 0.5;
        let reach = (bottom - anchor.y).clamp(b.tail[1] * 0.5, b.tail[1] * 5.0);
        let tail_x = anchor.x.clamp(centre.x - size.x * 0.5 + b.padding, centre.x + size.x * 0.5 - b.padding);
        let tail_size = Vec2::new(b.tail[0], reach + 2.0);
        let tail_el = element(Vec2::new(tail_x, bottom - reach * 0.5 + 1.0), tail_size, b.z_order - 1);
        let tail_img = UIImage { texture_id: b.tail_texture.clone(), color: b.color, ..Default::default() };
        let mut state = world.get::<&SpeechBubble>(e).map(|s| s.state.clone()).unwrap_or_default();
        if fresh {
            for x in [state.bubble.take(), state.tail.take()].into_iter().flatten() {
                let _ = world.despawn(x);
            }
        }
        let bubble = match state.bubble.filter(|x| world.contains(*x)) {
            Some(x) => {
                if let Ok(mut el) = world.get::<&mut UIElement>(x) {
                    *el = bubble_el;
                }
                // A new picture starts afresh; the same one keeps its
                // nine-slice resolved (`source_px`).
                if let Ok(mut im) = world.get::<&mut UIImage>(x) {
                    if im.texture_id == image.texture_id {
                        im.color = image.color;
                    } else {
                        *im = image;
                    }
                }
                if let Ok(mut t) = world.get::<&mut UIText>(x)
                    && (t.text != text.text
                        || t.color != text.color
                        || t.font_size != text.font_size
                        || t.reveal_speed != text.reveal_speed)
                {
                    t.text = text.text;
                    t.color = text.color;
                    t.font_size = text.font_size;
                    t.reveal_speed = text.reveal_speed;
                }
                x
            }
            None => world.spawn((bubble_el, image, text, BubblePart { owner: e })),
        };
        let tail = match state.tail.filter(|x| world.contains(*x)) {
            Some(x) => {
                if let Ok(mut el) = world.get::<&mut UIElement>(x) {
                    *el = tail_el;
                }
                if let Ok(mut im) = world.get::<&mut UIImage>(x) {
                    *im = tail_img;
                }
                x
            }
            None => world.spawn((tail_el, tail_img, BubblePart { owner: e })),
        };
        keep.push(bubble);
        keep.push(tail);
        if let Ok(mut s) = world.get::<&mut SpeechBubble>(e) {
            s.state.bubble = Some(bubble);
            s.state.tail = Some(tail);
            s.state.rect = Some((centre, size));
        }
    }
    // Bubbles no one is saying any more go.
    let stale: Vec<Entity> = world.query::<&BubblePart>().iter().map(|(x, _)| x).filter(|x| !keep.contains(x)).collect();
    for x in stale {
        let _ = world.despawn(x);
    }
    for (_, s) in world.query_mut::<&mut SpeechBubble>() {
        if s.state.bubble.is_some_and(|b| !keep.contains(&b)) {
            s.state.bubble = None;
            s.state.tail = None;
            s.state.rect = None;
        }
    }
}

/// `Bubble.say(e, text, seconds)`: say `text` for `seconds` (0 keeps it,
/// a negative keeps the bubble's own duration) - adding a bubble with the
/// default look to a speaker that has none.
pub fn say(world: &mut World, e: Entity, text: &str, seconds: f32) -> Result<(), String> {
    if !world.contains(e) {
        return Err("no such entity".into());
    }
    let missing = match world.get::<&mut SpeechBubble>(e) {
        Ok(mut b) => {
            b.text = text.to_string();
            if seconds >= 0.0 {
                b.duration = seconds;
            }
            b.state.since_ms = None;
            None
        }
        Err(_) => Some(SpeechBubble { text: text.to_string(), duration: if seconds >= 0.0 { seconds } else { 4.0 }, ..Default::default() }),
    };
    if let Some(b) = missing {
        world.insert_one(e, b).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// `Bubble.set(e, field, value)`: any field by name.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut b = world.get::<&mut SpeechBubble>(e).map_err(|_| "no speech_bubble on that entity".to_string())?;
    let mut v = serde_json::to_value(&*b).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("speech_bubble has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    let mut next: SpeechBubble = serde_json::from_value(v).map_err(|e| format!("speech_bubble.{field}: {e}"))?;
    next.state = std::mem::take(&mut b.state);
    *b = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::views::{MainView, set_main_view};

    fn overlap(a: (Vec2, Vec2), b: (Vec2, Vec2)) -> bool {
        (a.0.x - b.0.x).abs() * 2.0 < a.1.x + b.1.x && (a.0.y - b.0.y).abs() * 2.0 < a.1.y + b.1.y
    }

    #[test]
    fn long_words_wrap_and_short_ones_do_not() {
        let (one, s1) = wrap("Hello!", 18.0, 240.0);
        assert!(!one.contains('\n') && s1.x < 100.0, "{one:?} {s1}");
        let (many, s2) = wrap("Fresh bread, warm from the oven, two loaves for a copper!", 18.0, 200.0);
        assert!(many.lines().count() >= 3 && s2.x <= 200.0 + 1e-3, "{many:?} {s2}");
        assert!(s2.y > s1.y * 2.5, "taller for more lines");
    }

    #[test]
    fn crowded_bubbles_are_pushed_apart_and_kept_on_screen() {
        // Three speakers side by side, their bubbles would all overlap.
        let anchors = [Vec2::new(400.0, 300.0), Vec2::new(430.0, 305.0), Vec2::new(460.0, 300.0)];
        let sizes = [Vec2::new(160.0, 60.0); 3];
        let c = arrange(&anchors, &sizes, Vec2::new(800.0, 600.0), 18.0, 8.0, 8.0);
        for i in 0..3 {
            for j in i + 1..3 {
                assert!(!overlap((c[i], sizes[i]), (c[j], sizes[j])), "{i} and {j}: {:?}", c);
            }
            assert!(c[i].y - sizes[i].y * 0.5 >= anchors[i].y + 18.0 - 1e-3, "above its speaker");
        }
        // One at the screen's edge is pulled in.
        let c = arrange(&[Vec2::new(790.0, 590.0)], &[Vec2::new(160.0, 60.0)], Vec2::new(800.0, 600.0), 18.0, 8.0, 8.0);
        assert!(c[0].x + 80.0 <= 792.0 + 1e-3 && c[0].y + 30.0 <= 592.0 + 1e-3, "{}", c[0]);
    }

    #[test]
    fn a_speaker_says_something_and_the_bubble_goes_with_the_words() {
        let mut world = World::new();
        set_main_view(Some(MainView { center: Vec2::ZERO, axis_x: Vec2::X, axis_y: Vec2::Y, half: Vec2::new(400.0, 300.0) }));
        let baker = world.spawn((Transform { position: Vec2::new(0.0, -100.0), ..Default::default() },));
        say(&mut world, baker, "Bread!", 0.0).unwrap();
        update_speech_bubbles(&mut world, 800.0, 600.0);
        let parts: Vec<Entity> = world.query::<&BubblePart>().iter().map(|(e, _)| e).collect();
        assert_eq!(parts.len(), 2, "a bubble and its tail");
        let b = (*world.get::<&SpeechBubble>(baker).unwrap()).clone();
        let (centre, size) = b.state.rect.unwrap();
        // The speaker's head is world (0, -40): UI (400, 260).
        assert!((centre.x - 400.0).abs() < 1e-3 && centre.y - size.y * 0.5 > 260.0, "{centre} {size}");
        let text = world.get::<&UIText>(b.state.bubble.unwrap()).unwrap().text.clone();
        assert_eq!(text, "Bread!");
        // It stops talking: the bubble goes.
        say(&mut world, baker, "", 0.0).unwrap();
        update_speech_bubbles(&mut world, 800.0, 600.0);
        assert_eq!(world.query::<&BubblePart>().iter().count(), 0);
        set_main_view(None);
    }
}
