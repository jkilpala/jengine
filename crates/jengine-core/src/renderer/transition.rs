//! **Scene transitions** (Oct 2026): the outgoing frame kept and wiped away
//! over the incoming scene - an iris, a diamond wipe, a slide, a straight
//! wipe, a crossfade or a dissolve through a texture (`Scene.transition`).
//!
//! The state is the process's, like the screenshot queue, and rides on the
//! frame every host already makes:
//!
//! - [`arm`] (the script) records the look; the script's own scene load
//!   follows it, by whatever path that host takes.
//! - While armed, `WgpuFrame::present` copies each finished frame into a
//!   texture the renderer keeps ([`frame_plan`]`.capture`), so it holds the
//!   last frame of the old scene whenever the swap comes. Native swaps in a
//!   frame it never presents, so its runner holds the swap one frame while
//!   [`holds_scene_change`] (nothing captured yet); the browser hosts
//!   present the old scene while they fetch the new one.
//! - The swap (`scene::streaming::scene_replaced`, which every host calls)
//!   starts it playing ([`on_scene_replaced`]), and `present` draws the
//!   held frame over each new one through `transition.wgsl` until it ends.
//!
//! A dissolve's texture is read from the assets in `carve::apply_carves`
//! ([`sync_mask`]), the per-frame step every host makes with them, and
//! handed to the GPU as bytes; until it is (or with none) the dissolve uses
//! noise. A surface that cannot be copied from (no `COPY_SRC`) plays no
//! transition: the scene simply changes.

use std::cell::RefCell;

/// How the old frame leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Fade,
    Iris,
    Diamond,
    Slide,
    Wipe,
    Dissolve,
}

impl Style {
    pub fn from_name(name: &str) -> Style {
        match name.to_ascii_lowercase().as_str() {
            "iris" | "circle" => Style::Iris,
            "diamond" | "diamonds" => Style::Diamond,
            "slide" | "push" => Style::Slide,
            "wipe" => Style::Wipe,
            "dissolve" => Style::Dissolve,
            _ => Style::Fade,
        }
    }

    /// Its number in `transition.wgsl`.
    pub fn index(self) -> u32 {
        self as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    /// Waiting for the scene to change, copying frames; since when (ms).
    Armed { since: f64 },
    /// Wiping away the held frame; since when (ms).
    Playing { since: f64 },
}

/// A dissolve's texture: its size and RGBA bytes.
#[derive(Debug, Clone)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

struct State {
    phase: Phase,
    style: Style,
    seconds: f32,
    texture: String,
    captured: bool,
    supported: bool,
    mask: Option<Mask>,
    /// Bumped when `mask` changes, so the GPU uploads it once.
    mask_rev: u64,
    /// The time to use instead of the wall clock (tests).
    clock: Option<f64>,
}

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State {
        phase: Phase::Idle,
        style: Style::Fade,
        seconds: 0.6,
        texture: String::new(),
        captured: false,
        supported: true,
        mask: None,
        mask_rev: 0,
        clock: None,
    }) };
}

/// How long an armed transition waits for its scene before giving up (a
/// load that never came).
pub const ARMED_TIMEOUT_MS: f64 = 10_000.0;

fn now(s: &State) -> f64 {
    s.clock.unwrap_or_else(crate::profiling::now_ms)
}

/// Set the clock this thread's transitions read (tests); `None` is the wall
/// clock again.
pub fn set_clock(ms: Option<f64>) {
    STATE.with(|s| s.borrow_mut().clock = ms);
}

/// `Scene.transition`: the next scene change wipes the old frame away in
/// `style` over `seconds`, a dissolve through `texture` (empty for noise).
pub fn arm(style: &str, seconds: f32, texture: &str) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let since = now(&s);
        s.phase = Phase::Armed { since };
        s.style = Style::from_name(style);
        s.seconds = if seconds.is_finite() { seconds.clamp(0.05, 10.0) } else { 0.6 };
        if s.texture != texture {
            s.texture = texture.to_string();
            s.mask = None;
            s.mask_rev += 1;
        }
        s.captured = false;
    });
}

/// The renderer says whether its frames can be copied at all.
pub fn set_supported(on: bool) {
    STATE.with(|s| s.borrow_mut().supported = on);
}

/// A frame of the old scene was copied.
pub fn mark_captured() {
    STATE.with(|s| s.borrow_mut().captured = true);
}

/// Whether a host should hold a scene change for one frame, so the old
/// scene is presented (and copied) first. Native swaps scenes in a frame
/// it never presents.
pub fn holds_scene_change() -> bool {
    STATE.with(|s| {
        let s = s.borrow();
        matches!(s.phase, Phase::Armed { .. }) && !s.captured && s.supported
    })
}

/// The world was replaced (`scene::streaming::scene_replaced`): an armed
/// transition with a frame in hand starts playing; one without is dropped.
pub fn on_scene_replaced() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if let Phase::Armed { .. } = s.phase {
            let since = now(&s);
            s.phase = if s.captured { Phase::Playing { since } } else { Phase::Idle };
            if !s.captured {
                log::info!("Scene.transition: no frame of the old scene was copied - the scene just changes");
            }
        }
    });
}

/// What `present` does this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FramePlan {
    /// Copy the finished frame into the held texture.
    pub capture: bool,
    /// Draw the held frame over this one: its style and how far it has gone
    /// (0 all old, 1 all new, eased).
    pub draw: Option<(Style, f32)>,
}

/// What this frame's present should do, ending a transition that has run
/// its course or waited too long.
pub fn frame_plan() -> FramePlan {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let t = now(&s);
        match s.phase {
            Phase::Idle => FramePlan { capture: false, draw: None },
            Phase::Armed { since } => {
                if t - since > ARMED_TIMEOUT_MS || !s.supported {
                    s.phase = Phase::Idle;
                    return FramePlan { capture: false, draw: None };
                }
                FramePlan { capture: true, draw: None }
            }
            Phase::Playing { since } => {
                let k = ((t - since) / (s.seconds as f64 * 1000.0)) as f32;
                if k >= 1.0 {
                    s.phase = Phase::Idle;
                    return FramePlan { capture: false, draw: None };
                }
                let k = k.max(0.0);
                FramePlan { capture: false, draw: Some((s.style, k * k * (3.0 - 2.0 * k))) }
            }
        }
    })
}

/// Whether a transition is armed or playing.
pub fn is_active() -> bool {
    STATE.with(|s| s.borrow().phase != Phase::Idle)
}

/// Read the dissolve's texture from the assets once it is loaded. Called
/// from `carve::apply_carves`.
pub fn sync_mask(assets: &crate::asset::AssetManager) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.phase == Phase::Idle || s.texture.is_empty() || s.mask.is_some() {
            return;
        }
        if let Some(img) = assets.get_texture_bytes(&s.texture) {
            s.mask = Some(Mask { width: img.width, height: img.height, rgba: img.rgba.clone() });
            s.mask_rev += 1;
        }
    });
}

/// The dissolve's texture if it changed since `rev`: the new revision and
/// the bytes (`None` inside for "use noise").
pub fn mask_if_changed(rev: u64) -> Option<(u64, Option<Mask>)> {
    STATE.with(|s| {
        let s = s.borrow();
        (s.mask_rev != rev).then(|| (s.mask_rev, s.mask.clone()))
    })
}

/// Back to idle (a host leaving Play, tests).
pub fn reset() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.phase = Phase::Idle;
        s.captured = false;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transition_copies_until_the_swap_then_plays_and_ends() {
        reset();
        set_supported(true);
        set_clock(Some(1000.0));
        assert_eq!(frame_plan(), FramePlan { capture: false, draw: None });
        arm("iris", 0.5, "");
        assert!(holds_scene_change(), "nothing copied yet: native holds its swap");
        assert!(frame_plan().capture);
        mark_captured();
        assert!(!holds_scene_change(), "now the swap may come");
        on_scene_replaced();
        let p = frame_plan();
        assert_eq!((p.capture, p.draw.map(|d| d.0)), (false, Some(Style::Iris)));
        assert_eq!(p.draw.unwrap().1, 0.0, "it starts all old");
        set_clock(Some(1250.0));
        let k = frame_plan().draw.unwrap().1;
        assert!((k - 0.5).abs() < 1e-6, "halfway, eased: {k}");
        set_clock(Some(1500.0));
        assert_eq!(frame_plan().draw, None, "and ends");
        assert!(!is_active());
        set_clock(None);
    }

    #[test]
    fn without_a_copied_frame_the_scene_just_changes() {
        reset();
        set_supported(true);
        set_clock(Some(0.0));
        arm("wipe", 1.0, "");
        on_scene_replaced();
        assert_eq!(frame_plan().draw, None);
        // A load that never comes stops the copying.
        arm("wipe", 1.0, "");
        set_clock(Some(ARMED_TIMEOUT_MS + 1.0));
        assert!(!frame_plan().capture && !is_active());
        // A surface that cannot be copied holds nothing.
        set_supported(false);
        arm("wipe", 1.0, "");
        assert!(!holds_scene_change());
        set_supported(true);
        reset();
        set_clock(None);
    }

    #[test]
    fn styles_by_name() {
        assert_eq!(Style::from_name("Diamond"), Style::Diamond);
        assert_eq!(Style::from_name("slide"), Style::Slide);
        assert_eq!(Style::from_name("anything"), Style::Fade);
        assert_eq!(Style::Dissolve.index(), 5);
    }
}
