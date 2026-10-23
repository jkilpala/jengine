//! **A pixel-perfect camera** (Sep 2026): the world drawn at the art's own
//! resolution and scaled up, instead of drawn at the screen's and
//! pixelated afterwards.
//!
//! A camera with `pixel_size` (world units per art pixel) draws the world -
//! sprites, lights, the post chain - into a target of one texel per art
//! pixel, then the output pass scales that up nearest. Two things make it
//! *perfect* rather than merely small:
//!
//! - **The target sits on the art's grid.** Its edge is snapped to a whole
//!   number of pixels from the world origin, so every texel covers exactly
//!   one art pixel of every sprite placed on that grid - no texel straddles
//!   two, which is what "mixels" are.
//! - **Scrolling stays smooth.** A camera snapped to whole pixels moves in
//!   pixel steps, a stutter at any slow pan. So the target has a texel of
//!   margin on every side, and the output pass shows it shifted by what the
//!   snap took off - the image scrolls by display pixels while every texel
//!   stays on the grid.
//!
//! The world area the screen shows is unchanged, so a screen point maps to
//! the same world point as without it: nothing that converts the mouse
//! needs to know. [`plan`] is all of it; every host sizes its world targets
//! by [`PixelPlan::size`], draws the world through [`PixelPlan::camera`] and
//! hands [`PixelPlan::source`] to the output pass.
//!
//! `pixelate` (a post effect) is the other way to the same look, at full
//! cost and with shimmer on a pan; the showcase's Pixel Perfect II shows the
//! two side by side.

use glam::Vec2;

use super::camera::Camera2D;

/// How a frame of a pixel-perfect camera is drawn.
#[derive(Debug, Clone, Copy)]
pub struct PixelPlan {
    /// The world target's size in texels, margin included.
    pub size: (u32, u32),
    /// The camera to draw the world through: on the art's grid, as wide as
    /// the target, no shake (the shake is in its position already).
    pub camera: Camera2D,
    /// The part of the target the screen shows, in UV (`u0, v0, du, dv`,
    /// v down), for the output pass.
    pub source: [f32; 4],
}

/// The plan for `camera`, or `None` when it draws at the screen's
/// resolution (`pixel_size` 0).
pub fn plan(camera: &Camera2D) -> Option<PixelPlan> {
    let px = camera.pixel_size;
    if !(px > 0.0) {
        return None;
    }
    let zoom = if camera.zoom > 0.0 { camera.zoom } else { 1.0 };
    let (vw, vh) = camera.viewport_size();
    // The world the screen shows, in world units and in art pixels.
    let view = Vec2::new(vw, vh) / zoom;
    let shown = view / px;
    // Whole texels, one extra on each side to scroll through.
    let n = Vec2::new(shown.x.ceil().max(1.0) + 2.0, shown.y.ceil().max(1.0) + 2.0);
    let half = n * px * 0.5;
    // In the camera's own frame, so a turned camera snaps along its axes.
    let (sin, cos) = camera.rotation.sin_cos();
    let centre = camera.position + camera.shake_offset;
    let local = Vec2::new(centre.x * cos + centre.y * sin, -centre.x * sin + centre.y * cos);
    // The target's lower-left corner on a whole pixel, and its centre from it.
    let corner = ((local - half) / px).round() * px;
    let snapped = corner + half;
    let frac = (local - snapped) / px;
    let world = Vec2::new(snapped.x * cos - snapped.y * sin, snapped.x * sin + snapped.y * cos);

    let mut target = *camera;
    target.position = world;
    target.shake_offset = Vec2::ZERO;
    target.resize(n.x * px * zoom, n.y * px * zoom);

    let source = [
        (frac.x + n.x * 0.5 - shown.x * 0.5) / n.x,
        (n.y * 0.5 - frac.y - shown.y * 0.5) / n.y,
        shown.x / n.x,
        shown.y / n.y,
    ];
    Some(PixelPlan { size: (n.x as u32, n.y as u32), camera: target, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(vp: glam::Mat4, p: Vec2) -> Vec2 {
        let c = vp * glam::Vec4::new(p.x, p.y, 0.0, 1.0);
        Vec2::new(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5)
    }

    fn camera(x: f32, y: f32, zoom: f32, rot: f32, px: f32) -> Camera2D {
        let mut c = Camera2D::new(1280.0, 720.0);
        c.position = Vec2::new(x, y);
        c.zoom = zoom;
        c.rotation = rot;
        c.pixel_size = px;
        c
    }

    #[test]
    fn off_without_a_pixel_size() {
        assert!(plan(&camera(0.0, 0.0, 1.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn a_320_by_180_view_is_a_322_by_182_target() {
        let p = plan(&camera(0.0, 0.0, 1.0, 0.0, 4.0)).unwrap();
        assert_eq!(p.size, (322, 182));
        let p = plan(&camera(0.0, 0.0, 2.0, 0.0, 4.0)).unwrap();
        assert_eq!(p.size, (162, 92), "zoomed in, fewer art pixels are in view");
    }

    /// Every world point lands where the plain camera puts it: through the
    /// snapped camera into the target, then back out through `source`.
    #[test]
    fn the_screen_shows_what_the_plain_camera_would() {
        for &(x, y, zoom, rot) in &[
            (0.0, 0.0, 1.0, 0.0),
            (13.7, -5.2, 1.0, 0.0),
            (101.3, 47.9, 1.5, 0.0),
            (-7.1, 3.3, 0.8, 0.4),
        ] {
            let cam = camera(x, y, zoom, rot, 4.0);
            let p = plan(&cam).unwrap();
            for q in [Vec2::new(0.0, 0.0), Vec2::new(210.0, -95.0), Vec2::new(-330.0, 160.0)] {
                let want = screen(cam.view_projection(), q);
                let t = screen(p.camera.view_projection(), q);
                let got = Vec2::new((t.x - p.source[0]) / p.source[2], (t.y - p.source[1]) / p.source[3]);
                assert!(got.distance(want) < 1e-4, "{x},{y} z{zoom} r{rot}: {q} shows at {got}, not {want}");
            }
        }
    }

    /// The target's texels fall on the art's grid: its left and bottom
    /// edges are whole pixels from the world origin, wherever the camera is.
    #[test]
    fn the_target_sits_on_the_art_grid() {
        for x in [0.0, 1.3, 2.0, 57.9, -33.3] {
            let p = plan(&camera(x, x * 0.7, 1.0, 0.0, 4.0)).unwrap();
            let (w, h) = p.camera.viewport_size();
            let left = p.camera.position.x - w * 0.5;
            let bottom = p.camera.position.y - h * 0.5;
            assert!((left / 4.0 - (left / 4.0).round()).abs() < 1e-3, "left edge {left} is off the grid");
            assert!((bottom / 4.0 - (bottom / 4.0).round()).abs() < 1e-3, "bottom edge {bottom} is off the grid");
        }
    }

    /// A slow pan moves the source by a fraction of a texel a frame, not by
    /// a texel every few frames: that is what keeps the scroll smooth.
    #[test]
    fn a_slow_pan_moves_the_source_smoothly() {
        let a = plan(&camera(9.0, 0.0, 1.0, 0.0, 4.0)).unwrap();
        let b = plan(&camera(9.5, 0.0, 1.0, 0.0, 4.0)).unwrap();
        assert_eq!(a.camera.position, b.camera.position, "under half a pixel, the target stays put");
        let texel = 1.0 / a.size.0 as f32;
        assert!(((b.source[0] - a.source[0]) - texel / 8.0).abs() < 1e-5, "and the source slides an eighth of a texel");
    }
}
