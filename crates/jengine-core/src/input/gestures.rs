//! Gestures (Oct 2026): what a phone game means by its fingers - a tap, a
//! double tap, a long press, a swipe, a one-finger pan, a two-finger pinch
//! and twist - recognised once, here, from [`InputState::touches`], so a
//! game reads `Input.get_gestures()` rather than tracking raw fingers
//! itself. The left mouse button is a finger too when no finger is down,
//! and the wheel a pinch, so a desktop can try everything a phone does.
//!
//! [`GestureRecognizer::update`] runs inside
//! `InputState::update_interactions`, which every host calls once a frame
//! before scripts, on the wall clock - so no host was wired; the frame's
//! gestures are `InputState::gestures`, copied into the script snapshot
//! natively and published beside the touches in the browsers.
//!
//! [`InputState::touches`]: super::InputState::touches

use std::collections::HashMap;

use super::{Touch, TouchStage};

/// How far a finger may wander and still be tapping or pressing, in pixels.
pub const SLOP: f32 = 12.0;
/// A tap is shorter than this, in seconds.
pub const TAP_TIME: f64 = 0.3;
/// A second tap this soon after the first, this near it, is a double tap.
pub const DOUBLE_TIME: f64 = 0.35;
pub const DOUBLE_SLOP: f32 = 30.0;
/// Held still this long is a long press.
pub const LONG_PRESS: f64 = 0.5;
/// A swipe is a lift after moving this far, this fast.
pub const SWIPE_DISTANCE: f32 = 50.0;
pub const SWIPE_SPEED: f32 = 300.0;

/// What kind of gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureKind {
    Tap,
    DoubleTap,
    LongPress,
    Swipe,
    /// One finger dragging: `dx, dy` this frame.
    Pan,
    /// Two fingers: `scale` is how much further apart they are than last frame.
    Pinch,
    /// Two fingers: `angle` is how far they turned since last frame.
    Rotate,
}

impl GestureKind {
    pub fn name(self) -> &'static str {
        match self {
            GestureKind::Tap => "tap",
            GestureKind::DoubleTap => "double_tap",
            GestureKind::LongPress => "long_press",
            GestureKind::Swipe => "swipe",
            GestureKind::Pan => "pan",
            GestureKind::Pinch => "pinch",
            GestureKind::Rotate => "rotate",
        }
    }
}

/// One gesture this frame. Points are window pixels, y down, as touches are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gesture {
    pub kind: GestureKind,
    /// Where: the finger, or the middle of two.
    pub x: f32,
    pub y: f32,
    /// A pan's move this frame, or a swipe's whole move (screen, y down).
    pub dx: f32,
    pub dy: f32,
    /// A pinch's factor this frame (1 is no change).
    pub scale: f32,
    /// A twist this frame, radians, anticlockwise on screen.
    pub angle: f32,
    /// A swipe's speed, pixels a second.
    pub speed: f32,
    /// `"began"`, `"changed"` or `"ended"` for pan, pinch and rotate;
    /// `"ended"` for the rest.
    pub phase: &'static str,
}

impl Gesture {
    fn at(kind: GestureKind, x: f32, y: f32) -> Self {
        Gesture { kind, x, y, dx: 0.0, dy: 0.0, scale: 1.0, angle: 0.0, speed: 0.0, phase: "ended" }
    }
}

#[derive(Debug, Clone, Copy)]
struct Track {
    start: (f32, f32),
    last: (f32, f32),
    born: f64,
    moved: bool,
    pressed_long: bool,
    panning: bool,
    /// Another finger was down with it at some point: not a tap or swipe.
    shared: bool,
}

/// The fingers' story so far.
#[derive(Debug, Clone, Default)]
pub struct GestureRecognizer {
    tracks: HashMap<u64, Track>,
    last_tap: Option<(f64, (f32, f32))>,
    /// The two fingers of a pinch and their last distance and angle.
    pair: Option<(u64, u64, f32, f32)>,
}

/// The id the left mouse button tracks under.
const MOUSE: u64 = u64::MAX;

impl GestureRecognizer {
    /// This frame's gestures, from the fingers as they are now, the mouse
    /// (a finger while no real finger is down) and the wheel.
    pub fn update(&mut self, touches: &[Touch], mouse: Option<((f32, f32), bool)>, wheel: f32, now: f64) -> Vec<Gesture> {
        let mut out = Vec::new();
        let mut fingers: Vec<(u64, (f32, f32), bool)> =
            touches.iter().filter(|t| !t.on_control).map(|t| (t.id, t.position, t.stage == TouchStage::Ended)).collect();
        if fingers.is_empty()
            && let Some((pos, down)) = mouse
        {
            if down {
                fingers.push((MOUSE, pos, false));
            } else if self.tracks.contains_key(&MOUSE) {
                fingers.push((MOUSE, pos, true));
            }
        }
        // Fingers that vanished without an ended frame end now, where they were.
        let present: Vec<u64> = fingers.iter().map(|f| f.0).collect();
        let lost: Vec<(u64, (f32, f32))> =
            self.tracks.iter().filter(|(id, _)| !present.contains(id)).map(|(id, t)| (*id, t.last)).collect();
        for (id, at) in lost {
            fingers.push((id, at, true));
        }
        let live = fingers.iter().filter(|f| !f.2).count();
        for &(id, pos, ended) in &fingers {
            let t = self.tracks.entry(id).or_insert(Track {
                start: pos,
                last: pos,
                born: now,
                moved: false,
                pressed_long: false,
                panning: false,
                shared: false,
            });
            if live > 1 {
                t.shared = true;
            }
            let (ddx, ddy) = (pos.0 - t.last.0, pos.1 - t.last.1);
            let travel = ((pos.0 - t.start.0).powi(2) + (pos.1 - t.start.1).powi(2)).sqrt();
            if travel > SLOP {
                t.moved = true;
            }
            let age = now - t.born;
            if !t.shared && live == 1 {
                if t.moved && !ended && (ddx != 0.0 || ddy != 0.0 || !t.panning) {
                    let phase = if t.panning { "changed" } else { "began" };
                    t.panning = true;
                    out.push(Gesture { dx: ddx, dy: ddy, phase, ..Gesture::at(GestureKind::Pan, pos.0, pos.1) });
                }
                if !t.moved && !t.pressed_long && !ended && age >= LONG_PRESS {
                    t.pressed_long = true;
                    out.push(Gesture { phase: "began", ..Gesture::at(GestureKind::LongPress, pos.0, pos.1) });
                }
            }
            if ended {
                let t = *t;
                self.tracks.remove(&id);
                if t.panning {
                    out.push(Gesture { phase: "ended", ..Gesture::at(GestureKind::Pan, pos.0, pos.1) });
                }
                if t.shared {
                    continue;
                }
                let (mx, my) = (pos.0 - t.start.0, pos.1 - t.start.1);
                let dist = (mx * mx + my * my).sqrt();
                let speed = dist / (age.max(1e-3) as f32);
                if !t.moved && !t.pressed_long && age <= TAP_TIME {
                    let double = self.last_tap.is_some_and(|(at, p)| {
                        now - at <= DOUBLE_TIME && ((p.0 - pos.0).powi(2) + (p.1 - pos.1).powi(2)).sqrt() <= DOUBLE_SLOP
                    });
                    if double {
                        self.last_tap = None;
                        out.push(Gesture::at(GestureKind::DoubleTap, pos.0, pos.1));
                    } else {
                        self.last_tap = Some((now, pos));
                        out.push(Gesture::at(GestureKind::Tap, pos.0, pos.1));
                    }
                } else if dist >= SWIPE_DISTANCE && speed >= SWIPE_SPEED {
                    out.push(Gesture { dx: mx, dy: my, speed, ..Gesture::at(GestureKind::Swipe, pos.0, pos.1) });
                }
            } else {
                t.last = pos;
            }
        }
        // Two fingers down: pinch and twist about their middle.
        let down: Vec<(u64, (f32, f32))> = fingers.iter().filter(|f| !f.2).map(|f| (f.0, f.1)).collect();
        if down.len() == 2 {
            let (a, b) = (down[0], down[1]);
            let (vx, vy) = (b.1.0 - a.1.0, b.1.1 - a.1.1);
            let dist = (vx * vx + vy * vy).sqrt().max(1e-3);
            // Screen y is down: anticlockwise on screen is -atan2(vy, vx).
            let angle = (-vy).atan2(vx);
            let mid = ((a.1.0 + b.1.0) * 0.5, (a.1.1 + b.1.1) * 0.5);
            match self.pair {
                Some((pa, pb, d0, a0)) if (pa, pb) == (a.0, b.0) => {
                    let mut turn = angle - a0;
                    while turn > std::f32::consts::PI {
                        turn -= std::f32::consts::TAU;
                    }
                    while turn < -std::f32::consts::PI {
                        turn += std::f32::consts::TAU;
                    }
                    if (dist - d0).abs() > 0.01 {
                        out.push(Gesture { scale: dist / d0, phase: "changed", ..Gesture::at(GestureKind::Pinch, mid.0, mid.1) });
                    }
                    if turn.abs() > 1e-4 {
                        out.push(Gesture { angle: turn, phase: "changed", ..Gesture::at(GestureKind::Rotate, mid.0, mid.1) });
                    }
                }
                _ => {
                    out.push(Gesture { phase: "began", ..Gesture::at(GestureKind::Pinch, mid.0, mid.1) });
                    out.push(Gesture { phase: "began", ..Gesture::at(GestureKind::Rotate, mid.0, mid.1) });
                }
            }
            self.pair = Some((a.0, b.0, dist, angle));
        } else if self.pair.take().is_some() {
            let at = mouse.map(|m| m.0).unwrap_or((0.0, 0.0));
            out.push(Gesture { phase: "ended", ..Gesture::at(GestureKind::Pinch, at.0, at.1) });
            out.push(Gesture { phase: "ended", ..Gesture::at(GestureKind::Rotate, at.0, at.1) });
        }
        // The wheel pinches about the pointer.
        if wheel != 0.0 {
            let at = mouse.map(|m| m.0).unwrap_or((0.0, 0.0));
            out.push(Gesture {
                scale: 1.1f32.powf(wheel.clamp(-10.0, 10.0)),
                phase: "changed",
                ..Gesture::at(GestureKind::Pinch, at.0, at.1)
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finger(id: u64, x: f32, y: f32, stage: TouchStage) -> Touch {
        Touch { id, position: (x, y), start: (x, y), stage, on_control: false }
    }

    fn kinds(g: &[Gesture]) -> Vec<&'static str> {
        g.iter().map(|g| g.kind.name()).collect()
    }

    #[test]
    fn a_quick_touch_is_a_tap_and_two_are_a_double_tap() {
        let mut r = GestureRecognizer::default();
        assert!(r.update(&[finger(1, 100.0, 100.0, TouchStage::Began)], None, 0.0, 0.0).is_empty());
        assert_eq!(kinds(&r.update(&[finger(1, 101.0, 100.0, TouchStage::Ended)], None, 0.0, 0.1)), ["tap"]);
        r.update(&[finger(2, 104.0, 102.0, TouchStage::Began)], None, 0.0, 0.25);
        assert_eq!(kinds(&r.update(&[finger(2, 104.0, 102.0, TouchStage::Ended)], None, 0.0, 0.3)), ["double_tap"]);
        // A third, later: a tap again.
        r.update(&[finger(3, 104.0, 102.0, TouchStage::Began)], None, 0.0, 1.0);
        assert_eq!(kinds(&r.update(&[finger(3, 104.0, 102.0, TouchStage::Ended)], None, 0.0, 1.1)), ["tap"]);
    }

    #[test]
    fn holding_still_is_a_long_press_and_no_tap() {
        let mut r = GestureRecognizer::default();
        r.update(&[finger(1, 50.0, 50.0, TouchStage::Began)], None, 0.0, 0.0);
        assert!(r.update(&[finger(1, 50.0, 50.0, TouchStage::Held)], None, 0.0, 0.3).is_empty());
        assert_eq!(kinds(&r.update(&[finger(1, 51.0, 50.0, TouchStage::Held)], None, 0.0, 0.6)), ["long_press"]);
        assert!(r.update(&[finger(1, 51.0, 50.0, TouchStage::Held)], None, 0.0, 0.8).is_empty(), "once");
        assert!(r.update(&[finger(1, 51.0, 50.0, TouchStage::Ended)], None, 0.0, 0.9).is_empty(), "and no tap after");
    }

    #[test]
    fn a_drag_pans_and_a_fast_flick_swipes() {
        let mut r = GestureRecognizer::default();
        r.update(&[finger(1, 100.0, 300.0, TouchStage::Began)], None, 0.0, 0.0);
        let g = r.update(&[finger(1, 140.0, 300.0, TouchStage::Moved)], None, 0.0, 0.05);
        assert_eq!(kinds(&g), ["pan"]);
        assert_eq!((g[0].dx, g[0].phase), (40.0, "began"));
        let g = r.update(&[finger(1, 220.0, 290.0, TouchStage::Moved)], None, 0.0, 0.1);
        assert_eq!((g[0].dx, g[0].dy, g[0].phase), (80.0, -10.0, "changed"));
        let g = r.update(&[finger(1, 260.0, 290.0, TouchStage::Ended)], None, 0.0, 0.15);
        assert_eq!(kinds(&g), ["pan", "swipe"]);
        assert!(g[1].dx > 150.0 && g[1].speed > 1000.0, "{:?}", g[1]);
    }

    #[test]
    fn two_fingers_pinch_and_twist() {
        let mut r = GestureRecognizer::default();
        let two = |a: (f32, f32), b: (f32, f32), s: TouchStage| [finger(1, a.0, a.1, s), finger(2, b.0, b.1, s)];
        assert_eq!(kinds(&r.update(&two((100.0, 100.0), (200.0, 100.0), TouchStage::Began), None, 0.0, 0.0)), ["pinch", "rotate"]);
        // Twice as far apart, and turned a quarter anticlockwise on screen
        // (the second finger now straight above the first: y down is smaller).
        let g = r.update(&two((100.0, 100.0), (100.0, -100.0), TouchStage::Moved), None, 0.0, 0.1);
        let pinch = g.iter().find(|g| g.kind == GestureKind::Pinch).unwrap();
        let turn = g.iter().find(|g| g.kind == GestureKind::Rotate).unwrap();
        assert!((pinch.scale - 2.0).abs() < 1e-4, "{}", pinch.scale);
        assert!((turn.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-4, "{}", turn.angle);
        // Neither finger was a tap.
        let g = r.update(&two((100.0, 100.0), (100.0, -100.0), TouchStage::Ended), None, 0.0, 0.2);
        assert!(!kinds(&g).contains(&"tap"));
    }

    #[test]
    fn the_mouse_is_a_finger_and_the_wheel_a_pinch() {
        let mut r = GestureRecognizer::default();
        r.update(&[], Some(((10.0, 10.0), true)), 0.0, 0.0);
        assert_eq!(kinds(&r.update(&[], Some(((10.0, 10.0), false)), 0.0, 0.1)), ["tap"]);
        let g = r.update(&[], Some(((10.0, 10.0), false)), 2.0, 0.2);
        assert_eq!(kinds(&g), ["pinch"]);
        assert!((g[0].scale - 1.21).abs() < 1e-4);
    }
}
