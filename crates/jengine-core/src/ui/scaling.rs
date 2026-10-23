//! How a UI authored at one size is drawn at another.
//!
//! Both UI systems lay out in **pixels**: a `.gui` node is `Fixed 260.0`, a
//! `UIElement` is 420x300 at offset (40, -120). Nothing scaled them, so a UI
//! authored against a 1280x720 window simply ran off the edges of a smaller
//! one - scene 33's three columns need 860px and the editor's viewport panel
//! is narrower than that, so the third column was cut in half.
//!
//! The workaround was to do it in the game. One game's
//! `equipment_ui.lua` carried thirty lines computing
//! `math.min(sw / PANEL_W, sh / PANEL_H, 1.0)` and dividing every position,
//! size and font size by it - correct, and re-implemented per UI by whoever
//! noticed.
//!
//! # How the scale is applied
//!
//! Not by scaling every rect. The UI is laid out and drawn in **virtual
//! pixels** - `screen / factor` - and the orthographic matrix does the rest,
//! so layout, glyph advances and widget maths are untouched and there is no
//! second code path to keep in step. A runtime needs exactly three things:
//!
//! 1. lay out and render against [`UiScale::virtual_size`],
//! 2. draw through [`UiScale::projection`],
//! 3. convert the pointer with [`UiScale::to_ui`] before hit-testing UI.
//!
//! Scissor rectangles are the one exception: `set_scissor_rect` takes physical
//! pixels, so a clip rect goes back through [`UiScale::scissor`].
//!
//! # Wide screens
//!
//! A phone is about 20:9, wider than the 16:9 most UIs are authored at, so a
//! uniform scale leaves spare width. [`UiScaleMode::ScaleWithScreen`] hands
//! it to the layout - the surface is simply wider - which is right for a UI
//! anchored to its edges and wrong for one authored in absolute positions:
//! A management sim's 99 `TopLeft` elements all sat left of centre on a phone, the
//! spare 320 px piled up on the right. [`UiScaleMode::ScaleAndCenter`] lays
//! the UI out in the authored size instead and centres that area, so the
//! spare goes to both sides equally. Which is right depends on how the UI was
//! written, so it is the project's choice (`settings.ui_scale_mode`).
//!
//! # Notches and cut-outs
//!
//! A phone's camera notch, punch-hole or rounded corner covers part of the
//! screen. [`UiScale::safe`] is the band on each edge a platform says is
//! covered ([`SafeInsets`], physical pixels): Android reports its display
//! cut-out, a browser `env(safe-area-inset-*)`, and the editor's device
//! preview a simulated one. The UI - both systems, in every mode - and the
//! on-screen touch controls are laid out inside what is left; the **world**
//! still fills the whole screen, which is what a game wants under a notch.
//! With no insets every answer here is what it was before they existed.
//!
//! World-space coordinates are not affected. `get_mouse_position()` and
//! `get_screen_size()` keep reporting physical pixels, because scripts use
//! them to convert the cursor into world space (test lab scene 34) and that
//! conversion has nothing to do with how the UI is laid out.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::layout::LayoutRect;

/// What a UI does when the window is not the size it was authored for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UiScaleMode {
    /// One UI pixel is one screen pixel, whatever the window size.
    ///
    /// A UI authored for a large window overflows a small one, which is how
    /// every runtime behaved before scaling existed. Projects whose scripts
    /// already fit their own UI want this, so they do not fit it twice.
    ConstantPixelSize,
    /// Scale the whole UI by `min(screen_w / reference_w, screen_h / reference_h)`.
    ///
    /// Uniform, so nothing is stretched: the shorter axis decides, and the
    /// other gets margin. This is the default because a project's
    /// `width`/`height` already say what it was authored for.
    ///
    /// The margin belongs to the layout: an element anchored to the right
    /// edge sits at the screen's right edge.
    #[default]
    ScaleWithScreen,
    /// Scale as [`Self::ScaleWithScreen`] does, but lay the UI out in exactly
    /// the authored size, centred on the screen: the margin is split between
    /// the two sides. For a UI placed in absolute positions against
    /// `width` x `height`, which otherwise drifts to the top-left of any
    /// screen of another shape. An element anchored to an edge reaches the
    /// edge of the authored area, not of the screen.
    ScaleAndCenter,
}

impl UiScaleMode {
    /// The mode `settings.ui_scale_mode` names. Anything unknown, or nothing,
    /// is the default - a newer project file still loads.
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("ConstantPixelSize") => Self::ConstantPixelSize,
            Some("ScaleAndCenter") => Self::ScaleAndCenter,
            _ => Self::ScaleWithScreen,
        }
    }
}

/// How far in from each edge of the screen the display is usable, in
/// physical pixels - a notch, a camera hole, a rounded corner, a home bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SafeInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl SafeInsets {
    pub const ZERO: Self = Self { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };

    pub fn is_zero(&self) -> bool {
        *self == Self::ZERO
    }
}

/// The size a UI was authored for, and what to do about a window that is not
/// that size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UiScale {
    /// The window size the UI was authored against, in pixels. A project's
    /// `settings.width` / `settings.height`.
    pub reference: Vec2,
    pub mode: UiScaleMode,
    /// What the screen's own shape takes away - set by the host each time it
    /// learns it (start, resume, rotation), never authored.
    #[serde(skip)]
    pub safe: SafeInsets,
}

impl Default for UiScale {
    fn default() -> Self {
        Self { reference: Vec2::new(1280.0, 720.0), mode: UiScaleMode::default(), safe: SafeInsets::ZERO }
    }
}

impl UiScale {
    /// The scale for a project authored at `width` x `height`.
    pub fn from_reference(width: f32, height: f32) -> Self {
        Self { reference: Vec2::new(width, height), ..Self::default() }
    }

    /// Never scale: one UI pixel is one screen pixel.
    pub fn constant() -> Self {
        Self { mode: UiScaleMode::ConstantPixelSize, ..Self::default() }
    }

    /// The part of the screen outside the safe insets, in physical pixels.
    fn safe_size(&self, screen_w: f32, screen_h: f32) -> (f32, f32) {
        if screen_w <= 0.0 || screen_h <= 0.0 {
            return (screen_w, screen_h);
        }
        let s = self.safe;
        (
            (screen_w - s.left.max(0.0) - s.right.max(0.0)).max(1.0),
            (screen_h - s.top.max(0.0) - s.bottom.max(0.0)).max(1.0),
        )
    }

    /// How many screen pixels one UI pixel is worth.
    ///
    /// Always finite and positive: a degenerate reference or an unmapped
    /// surface falls back to 1.0 rather than dividing the whole UI by zero and
    /// rendering nothing. Measured against the safe part of the screen, so a
    /// notch shrinks the UI rather than covering it.
    pub fn factor(&self, screen_w: f32, screen_h: f32) -> f32 {
        if self.mode == UiScaleMode::ConstantPixelSize {
            return 1.0;
        }
        let (rw, rh) = (self.reference.x, self.reference.y);
        if rw <= 0.0 || rh <= 0.0 || screen_w <= 0.0 || screen_h <= 0.0 {
            return 1.0;
        }
        let (sw, sh) = self.safe_size(screen_w, screen_h);
        let f = (sw / rw).min(sh / rh);
        if f.is_finite() && f > 0.0 { f } else { 1.0 }
    }

    /// The size to lay the UI out against, in virtual pixels.
    ///
    /// With `ScaleWithScreen` one axis comes back as the reference and the
    /// other is longer - that extra is the margin the uniform scale leaves,
    /// and anchoring to an edge uses it rather than pretending it is not
    /// there. With `ScaleAndCenter` it is the reference itself, and the
    /// margin is outside it, split by [`Self::origin`].
    pub fn virtual_size(&self, screen_w: f32, screen_h: f32) -> (f32, f32) {
        let (w, h) = self.screen_virtual_size(screen_w, screen_h);
        if self.mode == UiScaleMode::ScaleAndCenter && self.reference.x > 0.0 && self.reference.y > 0.0 {
            // The uniform scale makes one axis exactly the reference and the
            // other at least it; `min` only absorbs rounding.
            (self.reference.x.min(w), self.reference.y.min(h))
        } else {
            (w, h)
        }
    }

    /// The safe part of the screen in virtual pixels, margin included. What
    /// something that belongs at the screen's edges is placed in - on-screen
    /// touch controls, which a thumb has to reach whatever the UI does, and
    /// which a notch must not cover. Its top-left is [`Self::safe_origin`].
    pub fn screen_virtual_size(&self, screen_w: f32, screen_h: f32) -> (f32, f32) {
        let f = self.factor(screen_w, screen_h);
        let (sw, sh) = self.safe_size(screen_w, screen_h);
        (sw / f, sh / f)
    }

    /// Where the safe part of the screen starts, in virtual pixels.
    pub fn safe_origin(&self, screen_w: f32, screen_h: f32) -> Vec2 {
        let f = self.factor(screen_w, screen_h);
        Vec2::new(self.safe.left.max(0.0) / f, self.safe.top.max(0.0) / f)
    }

    /// Where the layout area's top-left sits on the screen, in virtual
    /// pixels: past the safe inset, plus half the margin with
    /// `ScaleAndCenter`.
    pub fn origin(&self, screen_w: f32, screen_h: f32) -> Vec2 {
        let (sw, sh) = self.screen_virtual_size(screen_w, screen_h);
        let (lw, lh) = self.virtual_size(screen_w, screen_h);
        self.safe_origin(screen_w, screen_h)
            + Vec2::new(((sw - lw) * 0.5).max(0.0), ((sh - lh) * 0.5).max(0.0))
    }

    /// How far the layout area sits from the safe area's top-left, in
    /// virtual pixels - what a host hands `render_touch_controls_wgpu`, whose
    /// draws are placed in the safe area and drawn in layout space.
    pub fn controls_offset(&self, screen_w: f32, screen_h: f32) -> Vec2 {
        self.origin(screen_w, screen_h) - self.safe_origin(screen_w, screen_h)
    }

    /// The other way: a point in the UI's own space (top-left, y down) in
    /// physical pixels - where a text field's caret is on the screen, for an
    /// IME's candidate window.
    pub fn to_screen(&self, x: f32, y: f32, screen_w: f32, screen_h: f32) -> (f32, f32) {
        let f = self.factor(screen_w, screen_h);
        let o = self.origin(screen_w, screen_h);
        ((x + o.x) * f, (y + o.y) * f)
    }

    /// A pointer in physical pixels (top-left, y down) in the UI's own
    /// space - what every UI hit test compares against.
    pub fn to_ui(&self, x: f32, y: f32, screen_w: f32, screen_h: f32) -> (f32, f32) {
        let f = self.factor(screen_w, screen_h);
        let o = self.origin(screen_w, screen_h);
        (x / f - o.x, y / f - o.y)
    }

    /// The UI projection: layout space (y up, `(0, 0)` bottom-left of the
    /// layout area) onto the screen, the layout area where [`Self::origin`]
    /// puts it.
    pub fn projection(&self, screen_w: f32, screen_h: f32) -> glam::Mat4 {
        let f = self.factor(screen_w, screen_h);
        let (full_w, full_h) = (screen_w / f, screen_h / f);
        let (_, lh) = self.virtual_size(screen_w, screen_h);
        let o = self.origin(screen_w, screen_h);
        // Layout space is y up from the layout area's bottom edge; the
        // screen's bottom is `full_h - o.y - lh` below it (not `o.y`: a notch
        // on one edge makes the margins unequal).
        glam::Mat4::orthographic_rh_gl(
            -o.x,
            full_w - o.x,
            -(full_h - o.y - lh),
            lh + o.y,
            -1.0,
            1.0,
        )
    }

    /// A clip rect in the UI's space (framebuffer convention: top-left, y
    /// down) as `set_scissor_rect` wants it: physical pixels, clamped to the
    /// surface.
    pub fn scissor(&self, r: &LayoutRect, screen_w: u32, screen_h: u32) -> (u32, u32, u32, u32) {
        let f = self.factor(screen_w as f32, screen_h as f32);
        let o = self.origin(screen_w as f32, screen_h as f32);
        let x = (((r.x + o.x) * f).max(0.0) as u32).min(screen_w);
        let y = (((r.y + o.y) * f).max(0.0) as u32).min(screen_h);
        (
            x,
            y,
            ((r.width * f).max(0.0) as u32).min(screen_w - x),
            ((r.height * f).max(0.0) as u32).min(screen_h - y),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_authored_size_renders_one_to_one() {
        let s = UiScale::from_reference(1280.0, 720.0);
        assert_eq!(s.factor(1280.0, 720.0), 1.0);
        assert_eq!(s.virtual_size(1280.0, 720.0), (1280.0, 720.0));
    }

    #[test]
    fn a_smaller_window_shrinks_the_ui_to_fit() {
        let s = UiScale::from_reference(1280.0, 720.0);
        // The editor's viewport panel, roughly: narrower than authored.
        let f = s.factor(850.0, 610.0);
        assert!(f < 1.0);
        // Scene 33 needs 860 virtual px; at this factor it has 850 / f of them.
        let (vw, _) = s.virtual_size(850.0, 610.0);
        assert!(vw >= 860.0, "a 860px-wide document still does not fit: {vw}");
    }

    #[test]
    fn the_shorter_axis_decides_so_nothing_is_stretched() {
        let s = UiScale::from_reference(1000.0, 1000.0);
        // Twice as wide, same height: the height must win, or the UI stretches.
        assert_eq!(s.factor(2000.0, 1000.0), 1.0);
        let (vw, vh) = s.virtual_size(2000.0, 1000.0);
        assert_eq!((vw, vh), (2000.0, 1000.0), "the extra width is margin");
    }

    #[test]
    fn constant_pixel_size_is_what_every_runtime_used_to_do() {
        let s = UiScale::constant();
        assert_eq!(s.factor(300.0, 200.0), 1.0);
        assert_eq!(s.virtual_size(300.0, 200.0), (300.0, 200.0));
    }

    #[test]
    fn a_degenerate_surface_does_not_divide_the_ui_by_zero() {
        let s = UiScale::from_reference(1280.0, 720.0);
        assert_eq!(s.factor(0.0, 0.0), 1.0, "before the first resize");
        let bad = UiScale { reference: Vec2::ZERO, mode: UiScaleMode::ScaleWithScreen, ..UiScale::default() };
        assert_eq!(bad.factor(800.0, 600.0), 1.0, "a project with no size authored");
    }

    /// A 20:9 phone showing a UI authored at 16:9.
    const PHONE: (f32, f32) = (2400.0, 1080.0);

    #[test]
    fn scale_with_screen_gives_the_margin_to_the_layout() {
        let s = UiScale::from_reference(1280.0, 720.0);
        assert_eq!(s.virtual_size(PHONE.0, PHONE.1), (1600.0, 720.0));
        assert_eq!(s.origin(PHONE.0, PHONE.1), Vec2::ZERO);
    }

    #[test]
    fn scale_and_center_lays_out_in_the_authored_size_centred() {
        let s = UiScale { mode: UiScaleMode::ScaleAndCenter, ..UiScale::from_reference(1280.0, 720.0) };
        assert_eq!(s.factor(PHONE.0, PHONE.1), 1.5, "the scale itself is unchanged");
        assert_eq!(s.virtual_size(PHONE.0, PHONE.1), (1280.0, 720.0));
        assert_eq!(s.screen_virtual_size(PHONE.0, PHONE.1), (1600.0, 720.0));
        // 320 spare virtual px, half each side.
        assert_eq!(s.origin(PHONE.0, PHONE.1), Vec2::new(160.0, 0.0));
        // A tall screen splits the spare height instead.
        assert_eq!(s.origin(1080.0, 2400.0).x, 0.0);
        assert!(s.origin(1080.0, 2400.0).y > 0.0);
    }

    #[test]
    fn the_pointer_and_the_projection_agree_about_where_the_ui_is() {
        let s = UiScale { mode: UiScaleMode::ScaleAndCenter, ..UiScale::from_reference(1280.0, 720.0) };
        // The screen's physical centre is the layout's centre.
        assert_eq!(s.to_ui(1200.0, 540.0, PHONE.0, PHONE.1), (640.0, 360.0));
        // The layout's left edge is 160 virtual px (240 physical) in.
        assert_eq!(s.to_ui(240.0, 0.0, PHONE.0, PHONE.1), (0.0, 0.0));
        // And the projection puts layout x = 0 exactly there: NDC -1 is the
        // screen's left edge, so 240 of 2400 px is NDC -0.8.
        let p = s.projection(PHONE.0, PHONE.1);
        let left = p.transform_point3(glam::Vec3::new(0.0, 360.0, 0.0));
        assert!((left.x - (-0.8)).abs() < 1e-5, "{left}");
        let centre = p.transform_point3(glam::Vec3::new(640.0, 360.0, 0.0));
        assert!(centre.x.abs() < 1e-5 && centre.y.abs() < 1e-5, "{centre}");
    }

    #[test]
    fn a_clip_rect_lands_where_its_content_is_drawn() {
        let s = UiScale { mode: UiScaleMode::ScaleAndCenter, ..UiScale::from_reference(1280.0, 720.0) };
        let r = LayoutRect { x: 0.0, y: 0.0, width: 100.0, height: 50.0 };
        assert_eq!(s.scissor(&r, 2400, 1080), (240, 0, 150, 75));
        // Without centring, as before: just the factor.
        let plain = UiScale::from_reference(1280.0, 720.0);
        assert_eq!(plain.scissor(&r, 2400, 1080), (0, 0, 150, 75));
    }

    /// A 2400 x 1080 phone with a 120 px camera cut-out on its left edge.
    fn notched(mode: UiScaleMode) -> UiScale {
        UiScale {
            mode,
            safe: SafeInsets { left: 120.0, ..SafeInsets::ZERO },
            ..UiScale::from_reference(1280.0, 720.0)
        }
    }

    #[test]
    fn a_notch_moves_the_ui_out_from_under_it() {
        for mode in [UiScaleMode::ScaleWithScreen, UiScaleMode::ScaleAndCenter, UiScaleMode::ConstantPixelSize] {
            let s = notched(mode);
            let f = s.factor(PHONE.0, PHONE.1);
            // The layout area's left edge is at or right of the notch.
            assert!(s.origin(PHONE.0, PHONE.1).x * f >= 120.0 - 1e-3, "{mode:?}");
            // A click on the layout's left edge is a click at x = 0 in it.
            let left_edge = s.origin(PHONE.0, PHONE.1).x * f;
            let (ux, _) = s.to_ui(left_edge, 0.0, PHONE.0, PHONE.1);
            assert!(ux.abs() < 1e-3, "{mode:?}: {ux}");
            // And the projection draws layout x = 0 exactly there.
            let p = s.projection(PHONE.0, PHONE.1);
            let ndc = p.transform_point3(glam::Vec3::new(0.0, 0.0, 0.0)).x;
            let px = (ndc + 1.0) * 0.5 * PHONE.0;
            assert!((px - left_edge).abs() < 0.01, "{mode:?}: drawn at {px}, clicked at {left_edge}");
            // The layout's top is the screen's top; the bottom too.
            let (_, lh) = s.virtual_size(PHONE.0, PHONE.1);
            let top = p.transform_point3(glam::Vec3::new(0.0, lh + s.origin(PHONE.0, PHONE.1).y, 0.0)).y;
            assert!((top - 1.0).abs() < 1e-4, "{mode:?}: {top}");
        }
    }

    #[test]
    fn scale_and_center_centres_in_what_is_left() {
        let s = notched(UiScaleMode::ScaleAndCenter);
        // 2280 of 2400 px usable: the height still decides, 1.5.
        assert_eq!(s.factor(PHONE.0, PHONE.1), 1.5);
        // Usable width 2280 / 1.5 = 1520; the 1280 layout sits in the middle
        // of it, which starts at 120 / 1.5 = 80.
        assert_eq!(s.origin(PHONE.0, PHONE.1), Vec2::new(80.0 + 120.0, 0.0));
        assert_eq!(s.screen_virtual_size(PHONE.0, PHONE.1), (1520.0, 720.0));
        assert_eq!(s.safe_origin(PHONE.0, PHONE.1), Vec2::new(80.0, 0.0));
        assert_eq!(s.controls_offset(PHONE.0, PHONE.1), Vec2::new(120.0, 0.0));
    }

    #[test]
    fn no_insets_changes_nothing() {
        let with = UiScale { safe: SafeInsets::ZERO, ..UiScale::from_reference(1280.0, 720.0) };
        let plain = UiScale::from_reference(1280.0, 720.0);
        assert_eq!(with.projection(PHONE.0, PHONE.1), plain.projection(PHONE.0, PHONE.1));
        assert_eq!(with.origin(PHONE.0, PHONE.1), Vec2::ZERO);
        assert_eq!(with.controls_offset(PHONE.0, PHONE.1), Vec2::ZERO);
    }

    #[test]
    fn the_setting_names_every_mode_and_defaults_the_rest() {
        assert_eq!(UiScaleMode::from_setting(Some("ScaleAndCenter")), UiScaleMode::ScaleAndCenter);
        assert_eq!(UiScaleMode::from_setting(Some("ConstantPixelSize")), UiScaleMode::ConstantPixelSize);
        assert_eq!(UiScaleMode::from_setting(Some("ScaleWithScreen")), UiScaleMode::ScaleWithScreen);
        assert_eq!(UiScaleMode::from_setting(Some("FromTheFuture")), UiScaleMode::ScaleWithScreen);
        assert_eq!(UiScaleMode::from_setting(None), UiScaleMode::ScaleWithScreen);
    }

    #[test]
    fn the_mode_round_trips_through_a_project_file() {
        let json = r#"{"reference":[1920.0,1080.0],"mode":"ConstantPixelSize"}"#;
        let s: UiScale = serde_json::from_str(json).expect("parse");
        assert_eq!(s.mode, UiScaleMode::ConstantPixelSize);
        assert_eq!(s.reference, Vec2::new(1920.0, 1080.0));
        // And the name is the one a project file writes.
        assert!(serde_json::to_string(&s).unwrap().contains("ConstantPixelSize"));
    }
}
