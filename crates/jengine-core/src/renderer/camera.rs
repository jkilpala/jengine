//! 2D orthographic camera.

use glam::{Mat4, Vec2};

/// What a camera's pass draws (`Camera2D::pass`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CameraPass {
    /// The screen: canvas groups' members are skipped and their pictures drawn.
    #[default]
    Main,
    /// A camera drawing into a texture (`ecs::views`): everything, as is.
    View,
    /// A canvas group's picture: that group's members only (its key,
    /// `ecs::canvas_group::group_key`).
    CanvasGroup(u32),
    /// A mirror's view (`ecs::mirror`, its `mirror_key`): everything but its
    /// own reflection.
    Mirror(u32),
}

/// A 2D camera with orthographic projection.
///
/// Holds optional per-frame **shake state** (amplitude, remaining duration,
/// frequency, phase) so callers can apply screen-shake without bolting on a
/// separate resource. `tick_effects` decays the state and updates
/// [`Camera2D::shake_offset`], which `view_projection` then folds into the
/// view matrix. The fields are pub-read so editor/runtime UI can inspect
/// them; mutation goes through [`Camera2D::start_shake`].
#[derive(Debug, Clone, Copy)]
pub struct Camera2D {
    pub position: Vec2,
    pub zoom: f32,
    /// How far the view is turned, in radians, anticlockwise. The world
    /// swings the other way on screen: a camera at 90 degrees puts world
    /// +Y to the *right*. That sign is the useful one - a camera whose
    /// rotation matches a car's holds the car's nose still on screen.
    /// Driven by the active camera entity's `Transform.rotation`, so
    /// `set_rotation` on it is the whole script API.
    pub rotation: f32,
    /// World units per art pixel for a pixel-perfect camera, 0 for none -
    /// see `renderer::pixel_perfect`. Copied from the active camera
    /// component by `update_camera`.
    pub pixel_size: f32,
    /// Whether a pixel-perfect camera draws every art pixel as a whole
    /// number of screen pixels ([`Camera2D::integer_view`]). Copied from the
    /// camera component by `update_camera`.
    pub integer_scale: bool,
    /// The render layers this camera draws (`RenderLayers`), all by default.
    /// Copied from the camera component by `update_camera` (or by
    /// `ecs::views` for a view) and carried into the frame's `FrameView`.
    pub cull_mask: u32,
    /// What this camera's pass is for: the screen, a view into a texture,
    /// or a canvas group's picture (`ecs::canvas_group`). Carried into the
    /// frame's `FrameView`, where `in_view_layers` reads it.
    pub pass: CameraPass,
    viewport_width: f32,
    viewport_height: f32,
    /// Current shake offset in world units, applied additively to
    /// `position` when forming the view matrix. Recomputed each frame by
    /// [`Camera2D::tick_effects`].
    pub shake_offset: Vec2,
    /// Current shake amplitude in world units.
    shake_amp: f32,
    /// Remaining shake duration in seconds. Shake is active iff > 0.
    shake_remaining: f32,
    /// Total duration the current shake was started with. Kept so the
    /// amplitude can fade linearly across the whole shake rather than
    /// cutting off at the end.
    shake_duration: f32,
    /// Frequency (Hz) of the noise oscillator driving the offset.
    shake_freq: f32,
    /// Phase accumulator for the noise oscillator.
    shake_phase: f32,
    /// Where the followed target was last frame - for its velocity, which
    /// the look-ahead leads by (`CameraFollow::look_ahead`).
    pub follow_prev: Option<Vec2>,
    /// The look-ahead's current lead, eased toward velocity x time.
    pub look_ahead: Vec2,
    /// The camera entity that drew the last frame
    /// (`ecs::camera_rig::main_camera`), so `update_camera` sees another
    /// take over.
    pub main_camera: Option<hecs::Entity>,
    /// The view easing from the last main camera to this one, while it runs.
    pub blend: Option<CameraBlend>,
    /// The floating origin this camera last followed
    /// (`ecs::floating_origin::follow`, in `update_camera`).
    pub origin_seen: Option<(f64, f64)>,
}

/// The view easing from one main camera to the next (`ecs::camera_rig`):
/// where it was when the new one took over, and the new one's own view,
/// which goes on following its target as if no blend were running.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraBlend {
    pub from_position: Vec2,
    pub from_zoom: f32,
    pub from_rotation: f32,
    pub live_position: Vec2,
    pub live_zoom: f32,
    pub elapsed: f32,
    pub duration: f32,
}

impl CameraBlend {
    /// How far along the blend is, eased in and out (0 to 1).
    pub fn weight(&self) -> f32 {
        let t = (self.elapsed / self.duration.max(1e-6)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

impl Camera2D {
    /// Visible extents in world units before zoom is applied. The fields are
    /// private (they are kept in sync with the surface by `resize`), and the
    /// screen-to-world conversion behind `get_mouse_world_position` needs
    /// them.
    pub fn viewport_size(&self) -> (f32, f32) {
        (self.viewport_width, self.viewport_height)
    }

    pub fn new(viewport_width: f32, viewport_height: f32) -> Self {
        Self {
            position: Vec2::ZERO,
            zoom: 1.0,
            rotation: 0.0,
            pixel_size: 0.0,
            integer_scale: false,
            cull_mask: u32::MAX,
            pass: CameraPass::Main,
            viewport_width,
            viewport_height,
            shake_offset: Vec2::ZERO,
            shake_amp: 0.0,
            shake_remaining: 0.0,
            shake_duration: 0.0,
            shake_freq: 0.0,
            shake_phase: 0.0,
            follow_prev: None,
            look_ahead: Vec2::ZERO,
            main_camera: None,
            blend: None,
            origin_seen: None,
        }
    }

    /// Start (or restart) a screen-shake effect.
    ///
    /// * `amplitude` — peak offset magnitude in world units.
    /// * `duration` — total seconds the shake should last. The amplitude
    ///   linearly fades to zero over this duration.
    /// * `frequency` — oscillation rate in Hz; defaults to **20.0** when
    ///   the caller passes `0.0` (matches a typical "punchy" feel).
    ///
    /// Restarting overrides any in-flight shake — this is intentional so
    /// rapid hits feel weighty rather than queueing.
    pub fn start_shake(&mut self, amplitude: f32, duration: f32, frequency: f32) {
        self.shake_amp = amplitude.max(0.0);
        self.shake_remaining = duration.max(0.0);
        self.shake_duration = self.shake_remaining;
        self.shake_freq = if frequency > 0.0 { frequency } else { 20.0 };
        self.shake_phase = 0.0;
    }

    /// Advance shake state by `dt` seconds.
    ///
    /// Updates [`Camera2D::shake_offset`] in place and decays the remaining
    /// duration. Safe to call every frame regardless of whether a shake is
    /// active; produces zero offset when no shake is running.
    pub fn tick_effects(&mut self, dt: f32) {
        if self.shake_remaining <= 0.0 || self.shake_amp <= 0.0 {
            self.shake_offset = Vec2::ZERO;
            self.shake_remaining = 0.0;
            self.shake_duration = 0.0;
            return;
        }
        self.shake_phase += dt * self.shake_freq;
        // Linear fade across the whole shake: full amplitude at the start,
        // zero as `shake_remaining` reaches 0.
        let decay = (self.shake_remaining / self.shake_duration.max(f32::EPSILON)).min(1.0);
        // Two decorrelated sine waves with co-prime multipliers — cheap
        // pseudo-noise that avoids the obvious "back-and-forth" look of a
        // single sin().
        let t = self.shake_phase;
        let sx = (t * std::f32::consts::TAU).sin()
            + 0.5 * (t * std::f32::consts::TAU * 1.7).sin();
        let sy = (t * std::f32::consts::TAU * 1.13 + 1.7).sin()
            + 0.5 * (t * std::f32::consts::TAU * 2.3 + 0.3).sin();
        let amp = self.shake_amp * decay;
        self.shake_offset = Vec2::new(sx, sy) * (amp / 1.5);
        self.shake_remaining = (self.shake_remaining - dt).max(0.0);
    }

    /// Whether a shake is currently active.
    pub fn is_shaking(&self) -> bool {
        self.shake_remaining > 0.0 && self.shake_amp > 0.0
    }

    /// How much world a camera authored as `authored` shows on a `surface`
    /// of another shape.
    ///
    /// The authored area is always all visible, and the view widens along
    /// whichever axis the surface has extra of - a 20:9 phone showing a
    /// 1280x720 camera sees 1600x720 of world - so a world unit stays
    /// square. Until Sep 2026 the native and web runtimes drew the authored
    /// size stretched to the surface (a square was drawn wide on a phone)
    /// while the editor's Game view already widened; this is the one rule
    /// all of them use now, projection and mouse picking alike.
    ///
    /// `stretch` is the old behaviour, kept for a camera that asks for it.
    /// Only the surface's *shape* matters, so fitting an already fitted size
    /// again changes nothing.
    pub fn fit_view(authored: Vec2, surface: Vec2, stretch: bool) -> Vec2 {
        if stretch || authored.x <= 0.0 || authored.y <= 0.0 || surface.x <= 0.0 || surface.y <= 0.0 {
            return authored;
        }
        // Cross-multiplied rather than through an aspect ratio, so a
        // 1280 x 720 camera on a 2400 x 1080 screen is exactly 1600 wide.
        if surface.x * authored.y > authored.x * surface.y {
            Vec2::new(authored.y * surface.x / surface.y, authored.y)
        } else {
            Vec2::new(authored.x, authored.x * surface.y / surface.x)
        }
    }

    /// The view a pixel-perfect camera with integer scaling shows on a
    /// `surface` (in pixels), given the `view` [`Self::fit_view`] chose for
    /// it (in world units): a little more world, so that one art pixel
    /// (`pixel_size` world units, at `zoom`) is a **whole** number of screen
    /// pixels - the largest that keeps the authored area in view, at least
    /// one. Godot's integer scaling with an expanded aspect; Unity's pixel
    /// perfect camera crops instead, and a crop would have every
    /// screen-to-world conversion learn the black bars.
    ///
    /// Without it a 320 x 180 game on a 1366 x 768 laptop draws art pixels
    /// four and five screen pixels wide by turns - a nearest upscale by 4.27
    /// - so a sprite walking right ripples. `view` back unchanged when there
    /// is no pixel size, or nothing to fit.
    pub fn integer_view(view: Vec2, surface: Vec2, pixel_size: f32, zoom: f32) -> Vec2 {
        if !(pixel_size > 0.0) || view.x <= 0.0 || view.y <= 0.0 || surface.x <= 0.0 || surface.y <= 0.0 {
            return view;
        }
        let zoom = if zoom > 0.0 { zoom } else { 1.0 };
        // Screen pixels one art pixel takes as fitted; a sliver of slack so
        // an exact fit (1920 for 320 at 6) is not floored to 5.
        let fitted = pixel_size * zoom * (surface.x / view.x).min(surface.y / view.y);
        let k = (fitted + 1e-3).floor().max(1.0);
        surface * pixel_size * zoom / k
    }

    /// Resize the camera viewport.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.viewport_width = width;
        self.viewport_height = height;
    }

    /// Get the viewport width.
    pub fn width(&self) -> f32 {
        self.viewport_width
    }

    /// Get the viewport height.
    pub fn height(&self) -> f32 {
        self.viewport_height
    }

    /// The world point under a screen point, for this camera. The one
    /// place that arithmetic lives: `get_mouse_world_position` in every
    /// runtime, and the behaviour graphs, go through the same steps (they
    /// cannot call this - they have only the numbers - so the comment in
    /// each says so).
    ///
    /// `screen` is the surface size in pixels, `point` is measured from its
    /// top-left with y down.
    pub fn screen_to_world(&self, point: Vec2, screen: Vec2) -> Vec2 {
        let (sw, sh) = (screen.x.max(1.0), screen.y.max(1.0));
        let zoom = if self.zoom > 0.0 { self.zoom } else { 1.0 };
        let vw = if self.viewport_width > 0.0 { self.viewport_width } else { sw };
        let vh = if self.viewport_height > 0.0 { self.viewport_height } else { sh };
        let nx = (point.x - sw * 0.5) / sw;
        let ny = (sh * 0.5 - point.y) / sh;
        let offset = Vec2::new(nx * (vw / zoom), ny * (vh / zoom));
        let (sin, cos) = self.rotation.sin_cos();
        let turned = Vec2::new(offset.x * cos - offset.y * sin, offset.x * sin + offset.y * cos);
        self.position + turned
    }

    /// How much of the world the view covers, as half-extents. A turned
    /// camera sees a wider box than its viewport: the corners swing out.
    pub fn half_extents(&self) -> Vec2 {
        let zoom = if self.zoom > 0.0 { self.zoom } else { 1.0 };
        let (hw, hh) = (self.viewport_width * 0.5 / zoom, self.viewport_height * 0.5 / zoom);
        let (sin, cos) = self.rotation.sin_cos();
        Vec2::new(hw * cos.abs() + hh * sin.abs(), hw * sin.abs() + hh * cos.abs())
    }

    /// The world corners of what the camera sees - top left, top right,
    /// bottom right, bottom left - turned, zoomed and shaken as drawn.
    pub fn view_corners(&self) -> [Vec2; 4] {
        let inv = self.view_projection().inverse();
        [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)].map(|(x, y)| {
            let p = inv * glam::Vec4::new(x, y, 0.0, 1.0);
            Vec2::new(p.x, p.y)
        })
    }

    /// Compute the view-projection matrix.
    ///
    /// Origin is at center of screen. +X right, +Y up.
    pub fn view_projection(&self) -> Mat4 {
        // Guarded like `screen_to_world` and `half_extents`: a scene authored
        // with a zoom of 0 used to divide here alone, so its projection was
        // infinite and it drew nothing, silently.
        let zoom = if self.zoom > 0.0 { self.zoom } else { 1.0 };
        let half_w = self.viewport_width / (2.0 * zoom);
        let half_h = self.viewport_height / (2.0 * zoom);

        let projection = Mat4::orthographic_rh_gl(
            -half_w,
            half_w,
            -half_h,
            half_h,
            -1.0,
            1.0,
        );

        let view = Mat4::from_rotation_z(-self.rotation)
            * Mat4::from_translation(glam::Vec3::new(
                -(self.position.x + self.shake_offset.x),
                -(self.position.y + self.shake_offset.y),
                0.0,
            ));

        projection * view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_camera_has_no_shake() {
        let cam = Camera2D::new(320.0, 240.0);
        assert!(!cam.is_shaking());
        assert_eq!(cam.shake_offset, Vec2::ZERO);
    }

    #[test]
    fn start_shake_then_tick_produces_nonzero_offset() {
        let mut cam = Camera2D::new(320.0, 240.0);
        cam.start_shake(10.0, 1.0, 20.0);
        cam.tick_effects(0.016);
        assert!(cam.is_shaking(), "shake should still be active");
        assert!(
            cam.shake_offset.length() > 0.0,
            "expected non-zero shake offset, got {:?}",
            cam.shake_offset
        );
        assert!(
            cam.shake_offset.length() <= 10.0 * 1.001,
            "shake offset {:?} exceeded amplitude",
            cam.shake_offset
        );
    }

    #[test]
    fn shake_decays_to_zero_after_duration() {
        let mut cam = Camera2D::new(320.0, 240.0);
        cam.start_shake(10.0, 0.5, 20.0);
        // Burn through the full duration plus one more tick.
        for _ in 0..40 {
            cam.tick_effects(0.02);
        }
        assert!(!cam.is_shaking());
        assert_eq!(cam.shake_offset, Vec2::ZERO);
    }

    #[test]
    fn shake_amplitude_fades_across_the_duration() {
        let mut cam = Camera2D::new(320.0, 240.0);
        cam.start_shake(10.0, 1.0, 20.0);
        // Sample the peak magnitude over a window of ticks so we compare
        // envelopes rather than individual points on the oscillator.
        let peak = |cam: &mut Camera2D, ticks: usize| {
            let mut m: f32 = 0.0;
            for _ in 0..ticks {
                cam.tick_effects(0.01);
                m = m.max(cam.shake_offset.length());
            }
            m
        };
        let early = peak(&mut cam, 20); // t = 0.0 .. 0.2
        let _mid = peak(&mut cam, 60); // t = 0.2 .. 0.8
        let late = peak(&mut cam, 15); // t = 0.8 .. 0.95
        assert!(
            late < early * 0.5,
            "amplitude should fade over the duration: early={early} late={late}"
        );
    }

    #[test]
    fn restarting_shake_overrides_remaining_time() {
        let mut cam = Camera2D::new(320.0, 240.0);
        cam.start_shake(5.0, 0.1, 20.0);
        cam.tick_effects(0.05);
        cam.start_shake(20.0, 1.0, 30.0);
        assert!(cam.is_shaking());
        // Amplitude bumped back up, duration restored.
        cam.tick_effects(0.016);
        assert!(cam.shake_offset.length() > 0.0);
    }

    #[test]
    fn shake_offset_affects_view_projection() {
        let mut cam = Camera2D::new(320.0, 240.0);
        let vp_no_shake = cam.view_projection();
        cam.shake_offset = Vec2::new(5.0, -3.0);
        let vp_shaken = cam.view_projection();
        assert_ne!(vp_no_shake, vp_shaken);
    }
}
