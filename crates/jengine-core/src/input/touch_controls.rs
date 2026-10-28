//! On-screen controls: sticks and buttons a touch screen presses keys with.
//!
//! A phone has no keyboard, and nearly every game here reads one - through
//! input actions mostly, raw keys often, and packages through the same
//! [`InputState`]. So a control does the one thing all of those already
//! understand: it **presses keys**. A binding names a key (`"Space"`) or an
//! input action (`"jump"`, which presses the first key bound to it), and a
//! game written for a keyboard plays by touch without a line changed.
//!
//! The layout is the [`TouchControls`] component, authored on any entity in a
//! scene (a prefab carries a standard one between scenes). A scene with none
//! gets no controls, which is right for menus: those are tapped.
//!
//! Every host does the same three things, and nothing else:
//!
//! 1. push each finger event, in physical pixels, with
//!    [`TouchControlsState::push`];
//! 2. call [`TouchControlsState::tick`] once a frame before scripts run - it
//!    grabs fingers that land on a control, turns the rest into the left
//!    mouse button (first finger only, as touch always has been), and presses
//!    and releases keys;
//! 3. draw [`TouchControlsState::draw_list`] over everything else
//!    (`ecs::wgpu_systems::render_touch_controls_wgpu`).
//!
//! Positions are in **virtual UI pixels** (see `ui::scaling`), so a layout
//! authored against the project's `width` x `height` scales with the rest of
//! the UI. Hosts pass the UI scale factor and the virtual size.
//!
//! Known limit: a key held both by a control and by a physical keyboard is
//! released when the control lets go. A phone has one or the other.

use std::collections::{BTreeMap, HashSet};

use glam::Vec2;

use super::{InputState, KeyCode, Touch, TouchStage};
use crate::asset::RawTextureData;
use crate::ui::scaling::UiScale;
use crate::ecs::components::{ScreenAnchor, TouchControl, TouchControls, TouchControlsShow};

/// Texture id of the filled disc controls are drawn with.
pub const DISC_TEXTURE_ID: &str = "__touch_disc";
/// Texture id of the ring around a stick.
pub const RING_TEXTURE_ID: &str = "__touch_ring";

/// The pointer used when the mouse, not a finger, works a control (a desktop
/// testing a layout with `show: "always"`).
const MOUSE_FINGER: u64 = u64::MAX;

/// A finger event as a host saw it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
}

#[derive(Debug, Clone, Copy)]
struct PendingTouch {
    id: u64,
    phase: TouchPhase,
    /// Physical pixels, y down.
    pos: Vec2,
}

#[derive(Debug, Clone, Copy)]
struct Finger {
    /// Virtual UI pixels, y down.
    pos: Vec2,
    /// The control it grabbed, if it landed on one.
    control: Option<usize>,
}

/// One thing to draw: a disc or ring, and an optional label, in virtual UI
/// pixels with y **down**.
#[derive(Debug, Clone, PartialEq)]
pub struct TouchDraw {
    pub center: Vec2,
    pub radius: f32,
    pub color: [f32; 4],
    pub ring: bool,
    pub label: Option<String>,
}

/// Per-host state: fingers, the keys the controls hold, and whether a touch
/// has been seen (the default `show` waits for one).
#[derive(Debug, Default)]
pub struct TouchControlsState {
    pending: Vec<PendingTouch>,
    fingers: BTreeMap<u64, Finger>,
    held: HashSet<KeyCode>,
    /// The finger standing in for the left mouse button.
    mouse_finger: Option<u64>,
    seen_touch: bool,
    /// A controller was used more recently than the screen was touched:
    /// the controls step aside, and come back on the next touch.
    controller_in_use: bool,
}

impl TouchControlsState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a finger event, in physical pixels (y down). Handled by the
    /// next [`Self::tick`].
    pub fn push(&mut self, id: u64, phase: TouchPhase, x: f32, y: f32) {
        self.pending.push(PendingTouch { id, phase, pos: Vec2::new(x, y) });
    }

    /// Whether a real finger has touched the screen yet.
    pub fn seen_touch(&self) -> bool {
        self.seen_touch
    }

    /// Behave as if the screen had been touched, so `show: "touch"` layouts
    /// show - the editor's device preview, which is a touch screen being
    /// pretended at with a mouse.
    pub fn assume_touch_screen(&mut self, on: bool) {
        self.seen_touch = on;
    }

    /// Forget every finger: the app left the foreground, or the scene is
    /// about to be replaced. Held keys come up on the next tick.
    pub fn release_all(&mut self) {
        self.pending.clear();
        self.fingers.clear();
        self.mouse_finger = None;
    }

    /// Whether `controls` are showing (and so take fingers) right now.
    ///
    /// Never while a controller is in use: a player holding a pad does not
    /// want thumb-sized circles over the game, and a store checking
    /// controller support looks for exactly that. The next touch (or click)
    /// brings them back.
    pub fn is_shown(&self, controls: Option<&TouchControls>) -> bool {
        if self.controller_in_use {
            return false;
        }
        match controls.map(|c| c.show) {
            Some(TouchControlsShow::Always) => true,
            Some(TouchControlsShow::Touch) => self.seen_touch,
            Some(TouchControlsShow::Never) | None => false,
        }
    }

    /// Whether the controls are stepped aside for a controller.
    pub fn controller_in_use(&self) -> bool {
        self.controller_in_use
    }

    /// Handle the frame's finger events and press / release keys to match.
    ///
    /// Controls are placed in the safe part of the screen
    /// (`UiScale::screen_virtual_size`, from `UiScale::safe_origin`), so a
    /// notch covers none of them. `surface` is the screen in physical pixels.
    /// Call once a frame, after the host's input events and before scripts.
    pub fn tick(
        &mut self,
        controls: Option<&TouchControls>,
        input: &mut InputState,
        ui: &UiScale,
        surface: (f32, f32),
    ) {
        let (sw, sh) = surface;
        let factor = ui.factor(sw, sh);
        let origin = ui.safe_origin(sw, sh);
        let screen = Vec2::from(ui.screen_virtual_size(sw, sh));
        if input.gamepad_in_use() {
            self.controller_in_use = true;
        }
        // Last frame's lifted fingers are gone; the rest are held until an
        // event this frame says otherwise - unless no simulation step has
        // seen last frame's yet (fixed-step gameplay, `InputState::hold_edges`):
        // then a finger's `began` or `ended` waits for the step that will.
        if !input.carries_edges() {
            input.touches.retain(|t| t.stage != TouchStage::Ended);
            for t in &mut input.touches {
                t.stage = TouchStage::Held;
            }
        }
        // Fingers that landed this frame, and controls tapped by one that
        // also lifted this frame: pressed for this one frame anyway, or a
        // tap shorter than a frame (a quick flick, `adb shell input tap`)
        // lands and lifts before any key goes down, and does nothing.
        let mut landed: HashSet<u64> = HashSet::new();
        let mut tapped: Vec<usize> = Vec::new();
        for t in std::mem::take(&mut self.pending) {
            if t.id != MOUSE_FINGER {
                self.seen_touch = true;
            }
            if t.phase == TouchPhase::Started {
                // Back to the screen: the controls return, and this touch
                // can already use them.
                self.controller_in_use = false;
            }
            let pos = t.pos / factor - origin;
            let at = (t.pos.x, t.pos.y);
            match t.phase {
                TouchPhase::Started => {
                    let control = if self.is_shown(controls) {
                        controls.and_then(|c| hit(c, pos, screen))
                    } else {
                        None
                    };
                    self.fingers.insert(t.id, Finger { pos, control });
                    landed.insert(t.id);
                    input.touches.retain(|o| o.id != t.id);
                    input.touches.push(Touch {
                        id: t.id,
                        position: at,
                        start: at,
                        stage: TouchStage::Began,
                        on_control: control.is_some(),
                    });
                    // A finger on a control is not also a click on whatever
                    // is under it. The first free finger is the mouse.
                    if control.is_none() && self.mouse_finger.is_none() {
                        self.mouse_finger = Some(t.id);
                        input.mouse_position = (t.pos.x, t.pos.y);
                        input.set_mouse_button(true, true);
                    }
                }
                TouchPhase::Moved => {
                    if let Some(f) = self.fingers.get_mut(&t.id) {
                        f.pos = pos;
                    }
                    if let Some(o) = input.touches.iter_mut().find(|o| o.id == t.id) {
                        o.position = at;
                        // A finger that landed this frame is still news.
                        if o.stage != TouchStage::Began {
                            o.stage = TouchStage::Moved;
                        }
                    }
                    if self.mouse_finger == Some(t.id) {
                        input.mouse_position = (t.pos.x, t.pos.y);
                    }
                }
                TouchPhase::Ended => {
                    if let Some(f) = self.fingers.remove(&t.id)
                        && landed.contains(&t.id)
                        && let Some(control) = f.control
                    {
                        tapped.push(control);
                    }
                    if let Some(o) = input.touches.iter_mut().find(|o| o.id == t.id) {
                        o.position = at;
                        o.stage = TouchStage::Ended;
                    }
                    if self.mouse_finger == Some(t.id) {
                        self.mouse_finger = None;
                        input.mouse_position = (t.pos.x, t.pos.y);
                        input.set_mouse_button(true, false);
                    }
                }
            }
        }

        // A finger forgotten without an event (the app left the foreground:
        // `release_all`) lifts rather than staying down forever.
        for o in &mut input.touches {
            if !self.fingers.contains_key(&o.id) {
                o.stage = TouchStage::Ended;
            }
        }

        self.track_mouse(controls, input, factor, origin, screen);

        // A control that stopped showing (or a scene without one) lets go.
        if !self.is_shown(controls) {
            for f in self.fingers.values_mut() {
                f.control = None;
            }
        }
        let mut wanted = controls
            .map(|c| self.wanted_keys(c, input, screen))
            .unwrap_or_default();
        // Held for this frame only: the next tick does not want it, so it
        // comes up then - one press, as long as a frame.
        if let Some(c) = controls.filter(|c| self.is_shown(Some(c))) {
            for i in tapped {
                if let Some(TouchControl::Button(b)) = c.controls.get(i) {
                    wanted.extend(resolve(&b.press, input));
                }
            }
        }
        for key in self.held.difference(&wanted).copied().collect::<Vec<_>>() {
            input.key_up(key);
        }
        let pressing: Vec<_> = wanted.difference(&self.held).copied().collect();
        for key in &pressing {
            input.key_down(*key);
        }
        self.held = wanted;
        // A finger landing, or a control pressing its key, is the touch
        // screen in use - not the keyboard the control pretends to be.
        if !pressing.is_empty() || landed.iter().any(|id| *id != MOUSE_FINGER) {
            input.last_device = super::interactions::Device::Touch;
        }
    }

    /// The mouse works a control like a finger when the controls show on a
    /// desktop (`show: "always"`), so a layout can be tried without a phone.
    /// Its press edge is taken away so the click does not also land on the
    /// game; release is seen as the button no longer being down.
    fn track_mouse(
        &mut self,
        controls: Option<&TouchControls>,
        input: &mut InputState,
        factor: f32,
        origin: Vec2,
        screen: Vec2,
    ) {
        let pos = Vec2::new(input.mouse_position.0, input.mouse_position.1) / factor - origin;
        if self.fingers.contains_key(&MOUSE_FINGER) {
            if input.mouse_left {
                if let Some(f) = self.fingers.get_mut(&MOUSE_FINGER) {
                    f.pos = pos;
                }
            } else {
                self.fingers.remove(&MOUSE_FINGER);
                input.mouse_left_just_released = false;
            }
            return;
        }
        // A finger acting as the mouse is not the mouse working a control.
        if self.mouse_finger.is_some() || !input.mouse_left_just_pressed {
            return;
        }
        if !self.is_shown(controls) {
            return;
        }
        if let Some(control) = controls.and_then(|c| hit(c, pos, screen)) {
            self.fingers.insert(MOUSE_FINGER, Finger { pos, control: Some(control) });
            input.mouse_left_just_pressed = false;
        }
    }

    fn wanted_keys(&self, c: &TouchControls, input: &InputState, screen: Vec2) -> HashSet<KeyCode> {
        let mut keys = HashSet::new();
        for f in self.fingers.values() {
            let Some(i) = f.control else { continue };
            let Some(control) = c.controls.get(i) else { continue };
            match control {
                TouchControl::Button(b) => {
                    keys.extend(resolve(&b.press, input));
                }
                TouchControl::Stick(s) => {
                    let center = place(s.anchor, s.offset, screen);
                    let d = (f.pos - center) / s.radius.max(1.0);
                    let dead = s.dead_zone.clamp(0.0, 0.95);
                    // Per axis, so a diagonal presses two directions - what
                    // an eight-way stick on a keyboard game means.
                    if d.x < -dead {
                        keys.extend(resolve(&s.left, input));
                    }
                    if d.x > dead {
                        keys.extend(resolve(&s.right, input));
                    }
                    if d.y < -dead {
                        keys.extend(resolve(&s.up, input));
                    }
                    if d.y > dead {
                        keys.extend(resolve(&s.down, input));
                    }
                }
            }
        }
        keys
    }

    /// What to draw this frame, or nothing when the controls are hidden.
    pub fn draw_list(&self, controls: Option<&TouchControls>, screen: Vec2) -> Vec<TouchDraw> {
        let Some(c) = controls.filter(|c| self.is_shown(Some(c))) else {
            return Vec::new();
        };
        let alpha = c.opacity.clamp(0.0, 1.0);
        let mut out = Vec::new();
        for (i, control) in c.controls.iter().enumerate() {
            let finger = self.fingers.values().find(|f| f.control == Some(i));
            match control {
                TouchControl::Stick(s) => {
                    let center = place(s.anchor, s.offset, screen);
                    out.push(TouchDraw {
                        center,
                        radius: s.radius,
                        color: [1.0, 1.0, 1.0, 0.18 * alpha],
                        ring: false,
                        label: None,
                    });
                    out.push(TouchDraw {
                        center,
                        radius: s.radius,
                        color: [1.0, 1.0, 1.0, 0.6 * alpha],
                        ring: true,
                        label: None,
                    });
                    // The knob follows the finger, held inside the ring.
                    let knob = finger
                        .map(|f| {
                            let d = f.pos - center;
                            center + d.clamp_length_max(s.radius)
                        })
                        .unwrap_or(center);
                    out.push(TouchDraw {
                        center: knob,
                        radius: s.radius * 0.45,
                        color: [1.0, 1.0, 1.0, if finger.is_some() { 0.8 } else { 0.5 } * alpha],
                        ring: false,
                        label: None,
                    });
                }
                TouchControl::Button(b) => {
                    let pressed = finger.is_some();
                    out.push(TouchDraw {
                        center: place(b.anchor, b.offset, screen),
                        radius: if pressed { b.radius * 0.92 } else { b.radius },
                        color: [1.0, 1.0, 1.0, if pressed { 0.7 } else { 0.35 } * alpha],
                        ring: false,
                        label: (!b.label.is_empty()).then(|| b.label.clone()),
                    });
                }
            }
        }
        out
    }
}

/// Where a control's centre is, in virtual pixels with y down.
///
/// `offset` points **inward** from the anchored edge: `[120, 120]` on
/// `bottom_right` is 120 px left of the right edge and 120 px up from the
/// bottom. On a centred axis it is a plain displacement (x right, y down).
pub fn place(anchor: ScreenAnchor, offset: [f32; 2], screen: Vec2) -> Vec2 {
    use ScreenAnchor::*;
    let x = match anchor {
        TopLeft | Left | BottomLeft => offset[0],
        TopRight | Right | BottomRight => screen.x - offset[0],
        Top | Center | Bottom => screen.x * 0.5 + offset[0],
    };
    let y = match anchor {
        TopLeft | Top | TopRight => offset[1],
        BottomLeft | Bottom | BottomRight => screen.y - offset[1],
        Left | Center | Right => screen.y * 0.5 + offset[1],
    };
    Vec2::new(x, y)
}

/// The control under `pos`, the nearest when two overlap. A stick answers a
/// little outside its ring: a thumb lands roughly, not precisely.
fn hit(c: &TouchControls, pos: Vec2, screen: Vec2) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, control) in c.controls.iter().enumerate() {
        let (center, reach) = match control {
            TouchControl::Stick(s) => (place(s.anchor, s.offset, screen), s.radius * 1.3),
            TouchControl::Button(b) => (place(b.anchor, b.offset, screen), b.radius * 1.1),
        };
        let d = pos.distance(center);
        if d <= reach && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// The key a binding presses: an input action's first keyboard key, or a key
/// by name. Actions win, so a binding reads the way the game's own code does.
fn resolve(binding: &str, input: &InputState) -> Option<KeyCode> {
    if binding.is_empty() {
        return None;
    }
    if let Some(keys) = input.get_bindings(binding) {
        return keys.iter().find_map(|k| KeyCode::from_name(k));
    }
    KeyCode::from_name(binding)
}

/// Register the textures controls are drawn with. Every host calls this
/// where it registers `"__white"`.
pub fn insert_textures(assets: &mut crate::asset::AssetManager) {
    assets.insert_texture_bytes(DISC_TEXTURE_ID.to_string(), disc_texture());
    assets.insert_texture_bytes(RING_TEXTURE_ID.to_string(), ring_texture());
}

/// A soft-edged filled disc, white, for tinting.
pub fn disc_texture() -> RawTextureData {
    circle_texture(|d| 1.0 - smooth(0.94, 1.0, d))
}

/// A ring, white, for the edge of a stick.
pub fn ring_texture() -> RawTextureData {
    circle_texture(|d| smooth(0.84, 0.88, d) * (1.0 - smooth(0.96, 1.0, d)))
}

fn circle_texture(alpha_at: impl Fn(f32) -> f32) -> RawTextureData {
    const SIZE: u32 = 128;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let half = SIZE as f32 * 0.5;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let d = Vec2::new(x as f32 + 0.5 - half, y as f32 + 0.5 - half).length() / half;
            let a = (alpha_at(d).clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba.extend_from_slice(&[255, 255, 255, a]);
        }
    }
    RawTextureData { width: SIZE, height: SIZE, rgba }
}

fn smooth(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::{TouchButton, TouchStick};
    use crate::input::InputAction;

    const SCREEN: Vec2 = Vec2::new(1280.0, 720.0);

    fn ui() -> UiScale {
        UiScale::from_reference(1280.0, 720.0)
    }

    fn layout(show: TouchControlsShow) -> TouchControls {
        TouchControls {
            show,
            opacity: 1.0,
            controls: vec![
                TouchControl::Stick(TouchStick {
                    anchor: ScreenAnchor::BottomLeft,
                    offset: [150.0, 150.0],
                    radius: 100.0,
                    dead_zone: 0.3,
                    up: "Up".into(),
                    down: "Down".into(),
                    left: "Left".into(),
                    right: "Right".into(),
                }),
                TouchControl::Button(TouchButton {
                    anchor: ScreenAnchor::BottomRight,
                    offset: [120.0, 120.0],
                    radius: 60.0,
                    press: "jump".into(),
                    label: "A".into(),
                }),
            ],
        }
    }

    fn input_with_jump() -> InputState {
        let mut input = InputState::new();
        input.load_actions(&[InputAction {
            name: "jump".into(),
            keys: vec!["Pad0_A".into(), "Space".into()],
            dead_zone: 0.0,
        }]);
        input
    }

    #[test]
    fn anchors_place_controls_inward_from_their_edge() {
        assert_eq!(place(ScreenAnchor::BottomLeft, [150.0, 150.0], SCREEN), Vec2::new(150.0, 570.0));
        assert_eq!(place(ScreenAnchor::BottomRight, [120.0, 120.0], SCREEN), Vec2::new(1160.0, 600.0));
        assert_eq!(place(ScreenAnchor::TopRight, [10.0, 20.0], SCREEN), Vec2::new(1270.0, 20.0));
        assert_eq!(place(ScreenAnchor::Center, [0.0, -100.0], SCREEN), Vec2::new(640.0, 260.0));
    }

    #[test]
    fn a_button_presses_the_first_key_of_its_action() {
        let c = layout(TouchControlsShow::Touch);
        let mut s = TouchControlsState::new();
        let mut input = input_with_jump();

        s.push(1, TouchPhase::Started, 1160.0, 600.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        // `Pad0_A` is a gamepad binding, not a key; Space is the first key.
        assert!(input.is_pressed(KeyCode::Space));
        assert!(input.is_action_pressed("jump"));
        assert!(input.is_just_pressed(KeyCode::Space));
        // Not a click on the game as well.
        assert!(!input.mouse_left);

        input.begin_frame();
        s.push(1, TouchPhase::Ended, 1160.0, 600.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(!input.is_pressed(KeyCode::Space));
        assert!(input.is_just_released(KeyCode::Space));
    }

    /// A tap that lands and lifts inside one frame still presses the button,
    /// for that frame. Found on the emulator, where `adb shell
    /// input tap` on a game's button did nothing; a quick flick on a slow phone is the
    /// same thing.
    #[test]
    fn a_tap_shorter_than_a_frame_still_presses_its_button() {
        let c = layout(TouchControlsShow::Touch);
        let mut s = TouchControlsState::new();
        let mut input = input_with_jump();

        s.push(1, TouchPhase::Started, 1160.0, 600.0);
        s.push(1, TouchPhase::Ended, 1160.0, 600.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(input.is_just_pressed(KeyCode::Space), "the tap pressed JUMP");
        assert!(input.is_action_pressed("jump"));
        assert!(!input.mouse_left, "and was not a click on the game");

        input.begin_frame();
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(!input.is_pressed(KeyCode::Space), "for one frame only");
        assert!(input.is_just_released(KeyCode::Space));
    }

    #[test]
    fn the_stick_presses_directions_past_its_dead_zone() {
        let c = layout(TouchControlsShow::Touch);
        let mut s = TouchControlsState::new();
        let mut input = InputState::new();

        // Down on the centre: nothing yet.
        s.push(7, TouchPhase::Started, 150.0, 570.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(!input.is_pressed(KeyCode::Left) && !input.is_pressed(KeyCode::Up));

        // Up and to the left: a diagonal is two keys.
        s.push(7, TouchPhase::Moved, 80.0, 500.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(input.is_pressed(KeyCode::Left));
        assert!(input.is_pressed(KeyCode::Up));
        assert!(!input.is_pressed(KeyCode::Right) && !input.is_pressed(KeyCode::Down));

        // Back inside the dead zone lets go.
        s.push(7, TouchPhase::Moved, 160.0, 575.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(!input.is_pressed(KeyCode::Left) && !input.is_pressed(KeyCode::Up));
    }

    #[test]
    fn a_finger_off_the_controls_is_the_mouse() {
        let c = layout(TouchControlsShow::Touch);
        let mut s = TouchControlsState::new();
        let mut input = InputState::new();

        // Physical pixels at a UI scale of 2: the control test happens in
        // virtual pixels, the mouse stays physical.
        s.push(3, TouchPhase::Started, 1280.0, 400.0);
        s.tick(Some(&c), &mut input, &ui(), (2560.0, 1440.0));
        assert!(input.mouse_left && input.mouse_left_just_pressed);
        assert_eq!(input.mouse_position, (1280.0, 400.0));

        // A second finger on the stick does not disturb the mouse.
        s.push(4, TouchPhase::Started, 300.0, 1140.0);
        s.push(4, TouchPhase::Moved, 500.0, 1140.0);
        s.tick(Some(&c), &mut input, &ui(), (2560.0, 1440.0));
        assert!(input.is_pressed(KeyCode::Right));
        assert_eq!(input.mouse_position, (1280.0, 400.0));
    }

    #[test]
    fn every_finger_is_a_touch_with_a_stage() {
        let c = layout(TouchControlsShow::Touch);
        let mut s = TouchControlsState::new();
        let mut input = InputState::new();
        let frame = |s: &mut TouchControlsState, input: &mut InputState| {
            input.begin_frame();
            s.tick(Some(&c), input, &ui(), (1280.0, 720.0));
        };

        // Two fingers land in one frame - one on the button, one on the game.
        s.push(1, TouchPhase::Started, 600.0, 300.0);
        s.push(2, TouchPhase::Started, 1160.0, 600.0);
        frame(&mut s, &mut input);
        assert_eq!(input.touches.len(), 2);
        let (a, b) = (input.touches[0], input.touches[1]);
        assert_eq!((a.id, a.stage, a.position, a.on_control), (1, TouchStage::Began, (600.0, 300.0), false));
        assert_eq!((b.id, b.stage, b.on_control), (2, TouchStage::Began, true));

        // Nothing happens: both are held.
        frame(&mut s, &mut input);
        assert!(input.touches.iter().all(|t| t.stage == TouchStage::Held));

        // One moves; it remembers where it started.
        s.push(1, TouchPhase::Moved, 640.0, 320.0);
        frame(&mut s, &mut input);
        let a = input.touches[0];
        assert_eq!((a.stage, a.position, a.start), (TouchStage::Moved, (640.0, 320.0), (600.0, 300.0)));
        assert_eq!(input.touches[1].stage, TouchStage::Held);

        // One lifts: reported as ended for exactly one frame.
        s.push(2, TouchPhase::Ended, 1160.0, 600.0);
        frame(&mut s, &mut input);
        assert_eq!(input.touches.iter().find(|t| t.id == 2).unwrap().stage, TouchStage::Ended);
        frame(&mut s, &mut input);
        assert!(input.touches.iter().all(|t| t.id != 2));

        // A tap shorter than a frame is still seen, as ended.
        s.push(9, TouchPhase::Started, 100.0, 100.0);
        s.push(9, TouchPhase::Ended, 100.0, 100.0);
        frame(&mut s, &mut input);
        assert_eq!(input.touches.iter().find(|t| t.id == 9).unwrap().stage, TouchStage::Ended);

        // Leaving the foreground lifts whatever was down.
        s.release_all();
        frame(&mut s, &mut input);
        assert!(input.touches.iter().all(|t| t.stage == TouchStage::Ended));
        frame(&mut s, &mut input);
        assert!(input.touches.is_empty());
    }

    #[test]
    fn the_mouse_is_not_a_touch() {
        let c = layout(TouchControlsShow::Always);
        let mut s = TouchControlsState::new();
        let mut input = InputState::new();
        input.set_mouse_button(true, true);
        input.mouse_position = (1160.0, 600.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(input.touches.is_empty());
    }

    #[test]
    fn hidden_controls_take_no_fingers_and_draw_nothing() {
        let c = layout(TouchControlsShow::Never);
        let mut s = TouchControlsState::new();
        let mut input = input_with_jump();
        s.push(1, TouchPhase::Started, 1160.0, 600.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(!input.is_pressed(KeyCode::Space));
        assert!(input.mouse_left, "the tap is an ordinary click");
        assert!(s.draw_list(Some(&c), SCREEN).is_empty());
    }

    #[test]
    fn the_default_show_waits_for_a_touch() {
        let c = layout(TouchControlsShow::Touch);
        let mut s = TouchControlsState::new();
        let mut input = InputState::new();
        assert!(s.draw_list(Some(&c), SCREEN).is_empty(), "a desktop never sees them");
        s.push(1, TouchPhase::Started, 640.0, 100.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(!s.draw_list(Some(&c), SCREEN).is_empty());
    }

    #[test]
    fn the_controls_step_aside_for_a_controller_and_return_on_a_touch() {
        let c = layout(TouchControlsShow::Touch);
        let mut s = TouchControlsState::new();
        let mut input = input_with_jump();
        s.push(1, TouchPhase::Started, 640.0, 100.0);
        s.push(1, TouchPhase::Ended, 640.0, 100.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(!s.draw_list(Some(&c), SCREEN).is_empty());

        // A pad's button: the controls go, and stay gone after it is let go.
        input.update_gamepad(0, [0].into_iter().collect(), vec![0.0; 4]);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(s.controller_in_use());
        assert!(s.draw_list(Some(&c), SCREEN).is_empty());
        input.update_gamepad(0, Default::default(), vec![0.0; 4]);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(s.draw_list(Some(&c), SCREEN).is_empty());

        // A stick at rest is not use: drift under the dead zone changes nothing.
        let mut idle = TouchControlsState::new();
        let mut resting = input_with_jump();
        resting.update_gamepad(0, Default::default(), vec![0.1, -0.05, 0.0, 0.0]);
        idle.tick(Some(&c), &mut resting, &ui(), (1280.0, 720.0));
        assert!(!idle.controller_in_use());

        // A touch brings them back, and that same touch can press a button.
        s.push(2, TouchPhase::Started, 1160.0, 600.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(!s.controller_in_use());
        assert!(input.is_pressed(KeyCode::Space));
    }

    #[test]
    fn the_mouse_works_controls_that_always_show() {
        let c = layout(TouchControlsShow::Always);
        let mut s = TouchControlsState::new();
        let mut input = input_with_jump();

        input.mouse_position = (1160.0, 600.0);
        input.set_mouse_button(true, true);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(input.is_pressed(KeyCode::Space));
        assert!(!input.mouse_left_just_pressed, "the click was the control's");
        assert!(!s.seen_touch(), "a mouse is not a touch screen");

        input.begin_frame();
        input.set_mouse_button(true, false);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(!input.is_pressed(KeyCode::Space));
        assert!(!input.mouse_left_just_released);
    }

    #[test]
    fn releasing_everything_lets_the_keys_up() {
        let c = layout(TouchControlsShow::Touch);
        let mut s = TouchControlsState::new();
        let mut input = input_with_jump();
        s.push(1, TouchPhase::Started, 1160.0, 600.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(input.is_pressed(KeyCode::Space));
        s.release_all();
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        assert!(!input.is_pressed(KeyCode::Space));
    }

    #[test]
    fn the_knob_follows_the_finger_inside_the_ring() {
        let c = layout(TouchControlsShow::Touch);
        let mut s = TouchControlsState::new();
        let mut input = InputState::new();
        s.push(1, TouchPhase::Started, 150.0, 570.0);
        s.push(1, TouchPhase::Moved, 450.0, 570.0);
        s.tick(Some(&c), &mut input, &ui(), (1280.0, 720.0));
        let draws = s.draw_list(Some(&c), SCREEN);
        let knob = &draws[2];
        assert_eq!(knob.center, Vec2::new(250.0, 570.0), "held at the rim");
        assert_eq!(draws[3].label.as_deref(), Some("A"));
    }

    /// A notch on the left pushes the stick right, and a finger finds it
    /// where it is drawn.
    #[test]
    fn controls_stay_clear_of_a_notch() {
        use crate::ui::scaling::SafeInsets;
        let c = layout(TouchControlsShow::Touch);
        let notched = UiScale { safe: SafeInsets { left: 100.0, ..SafeInsets::ZERO }, ..ui() };
        let surface = (1380.0, 720.0); // 100 px notch + the 1280 x 720 that is left
        let mut s = TouchControlsState::new();
        let mut input = InputState::new();
        // The stick's centre, 150 px in from the safe edge, is 250 px from
        // the screen's.
        s.push(1, TouchPhase::Started, 250.0, 570.0);
        s.push(1, TouchPhase::Moved, 350.0, 570.0);
        s.tick(Some(&c), &mut input, &notched, surface);
        assert!(input.is_pressed(KeyCode::Right));
        // A finger on the old, un-notched spot is past the stick's reach.
        let mut s = TouchControlsState::new();
        let mut input = InputState::new();
        s.push(2, TouchPhase::Started, 20.0, 570.0);
        s.tick(Some(&c), &mut input, &notched, surface);
        assert!(input.mouse_left, "not the stick: the notch took that spot");
        // Drawn where it answers: in safe-area coordinates, offset by the
        // notch when drawn (`UiScale::controls_offset` / the host).
        assert_eq!(notched.safe_origin(surface.0, surface.1), Vec2::new(100.0, 0.0));
    }

    #[test]
    fn the_textures_are_white_discs_that_fade_at_the_edge() {
        let t = disc_texture();
        let at = |x: u32, y: u32| t.rgba[((y * t.width + x) * 4 + 3) as usize];
        assert_eq!(at(64, 64), 255);
        assert_eq!(at(0, 0), 0);
        let r = ring_texture();
        let ring_at = |x: u32, y: u32| r.rgba[((y * r.width + x) * 4 + 3) as usize];
        assert_eq!(ring_at(64, 64), 0, "a ring is empty in the middle");
        assert!(ring_at(64, 4) > 128, "and solid near its edge");
    }
}
