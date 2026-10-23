//! The world-space rectangle the current pass can see, so the sprite passes
//! can skip what is outside it.
//!
//! Sprites were never culled: every sprite in the world was collected,
//! sorted and uploaded every frame, wherever it was - the sprite entry
//! points did not even receive the camera. Only tilemaps culled. For a
//! scrolling game (a runner, a road racer, a digging game, a scrolling
//! shooter) or a survivors-like's two thousand foes, most of that work was for things off screen,
//! and the sort - the one measured cost in `architecture-debt.md` #1 - was
//! sorting them too.
//!
//! The view is set by the host once per pass, from the camera that pass draws
//! through ([`set_frame_view`] beside `camera.view_projection()`), and read
//! by every sprite collection. It is deliberately **per pass, not per
//! frame**: the editor's picture-in-picture draws the world through a second
//! camera in the same frame and sets its own view first. With no view set
//! nothing is culled, which is what a test that builds a batch and calls a
//! pass directly gets.
//!
//! The test is conservative on purpose. A sprite is kept if its position is
//! within the view grown by the sprite's full diagonal, which covers any
//! anchor and any rotation without computing either; a wrongly culled sprite
//! is a bug that reads as "it vanished", and a kept one costs what it always
//! cost.

use std::cell::Cell;

use glam::Vec2;

use crate::ecs::components::SpriteComponent;
use crate::renderer::camera::Camera2D;

/// A world-space axis-aligned rectangle: what the camera sees, already grown
/// for its rotation (see [`Camera2D::half_extents`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameView {
    pub center: Vec2,
    pub half: Vec2,
    /// The camera's `cull_mask`: the render layers this pass draws.
    pub cull_mask: u32,
    /// What the pass is for (`Camera2D::pass`).
    pub pass: crate::renderer::camera::CameraPass,
    /// The world corners of the camera's view (`Camera2D::view_corners`),
    /// where a canvas group's picture is drawn.
    pub corners: [Vec2; 4],
}

impl FrameView {
    /// The view of `camera`, as it is now.
    pub fn of(camera: &Camera2D) -> Self {
        Self {
            center: camera.position,
            half: camera.half_extents(),
            cull_mask: camera.cull_mask,
            pass: camera.pass,
            corners: camera.view_corners(),
        }
    }

    /// Whether a sprite drawn at world `position` with `scale` might touch
    /// the view. `default_size` is what a sprite with no size of its own is
    /// drawn at, the same value the pass hands `drawn_size`.
    #[inline]
    pub fn sees_sprite(&self, position: Vec2, scale: Vec2, sprite: &SpriteComponent, default_size: Vec2) -> bool {
        let reach = sprite_reach(sprite, scale, default_size);
        self.sees_disc(position, reach)
    }

    /// Whether a disc of `radius` about `position` overlaps the view.
    #[inline]
    pub fn sees_disc(&self, position: Vec2, radius: f32) -> bool {
        let d = (position - self.center).abs();
        d.x <= self.half.x + radius && d.y <= self.half.y + radius
    }
}

/// How far from its position a sprite can reach: the length of its drawn
/// diagonal, or of its farthest explicit corner. Anchor and rotation can
/// move the quad about its position but never further than this.
#[inline]
pub fn sprite_reach(sprite: &SpriteComponent, scale: Vec2, default_size: Vec2) -> f32 {
    if let Some(corners) = sprite.corners {
        return corners
            .iter()
            .map(|c| Vec2::new(c[0], c[1]).length())
            .fold(0.0, f32::max);
    }
    (sprite.drawn_size(default_size) * scale).length()
}

thread_local! {
    static FRAME_VIEW: Cell<Option<FrameView>> = const { Cell::new(None) };
}

/// Set the view for the passes that follow, from the camera they draw
/// through. Call beside `camera.view_projection()`, once per pass.
pub fn set_frame_view(camera: &Camera2D) {
    FRAME_VIEW.with(|v| v.set(Some(FrameView::of(camera))));
}

/// Cull against nothing until the next [`set_frame_view`].
pub fn clear_frame_view() {
    FRAME_VIEW.with(|v| v.set(None));
}

/// The view the current pass culls against, if a host set one.
pub fn frame_view() -> Option<FrameView> {
    FRAME_VIEW.with(|v| v.get())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> FrameView {
        let mut c = Camera2D::new(800.0, 600.0);
        c.position = Vec2::new(1000.0, 0.0);
        c.zoom = 2.0; // sees 400 x 300 world units
        FrameView::of(&c)
    }

    fn sprite(size: [f32; 2]) -> SpriteComponent {
        SpriteComponent { size: Some(size), ..Default::default() }
    }

    #[test]
    fn a_sprite_inside_is_seen_and_one_far_outside_is_not() {
        let v = view();
        let s = sprite([32.0, 32.0]);
        assert!(v.sees_sprite(Vec2::new(1000.0, 0.0), Vec2::ONE, &s, Vec2::ONE));
        assert!(v.sees_sprite(Vec2::new(1199.0, 149.0), Vec2::ONE, &s, Vec2::ONE), "at the corner");
        assert!(!v.sees_sprite(Vec2::new(1300.0, 0.0), Vec2::ONE, &s, Vec2::ONE));
        assert!(!v.sees_sprite(Vec2::new(1000.0, -400.0), Vec2::ONE, &s, Vec2::ONE));
    }

    #[test]
    fn a_sprite_just_outside_is_kept_if_its_size_can_reach_in() {
        let v = view();
        // Right edge of the view is x = 1200. A 100-wide sprite centred at
        // 1240 reaches back to 1190 whatever its anchor, so it stays.
        assert!(v.sees_sprite(Vec2::new(1240.0, 0.0), Vec2::ONE, &sprite([100.0, 10.0]), Vec2::ONE));
        // Scale counts.
        assert!(v.sees_sprite(Vec2::new(1290.0, 0.0), Vec2::new(2.0, 1.0), &sprite([100.0, 10.0]), Vec2::ONE));
        // The reach is the diagonal (~100.5), so 1310 is the first x past it.
        assert!(!v.sees_sprite(Vec2::new(1310.0, 0.0), Vec2::ONE, &sprite([100.0, 10.0]), Vec2::ONE));
    }

    #[test]
    fn a_rotated_or_oddly_anchored_sprite_is_never_culled_wrongly() {
        // The reach is the full diagonal, so a 100x10 sprite turned upright
        // (reaching 50 up) is kept by the same test that keeps it flat.
        let v = view();
        let mut s = sprite([100.0, 10.0]);
        s.anchor = [0.0, 0.0];
        // Top edge is y = 150; a sprite at y = 190 with a 100-long side can
        // hang down into view when rotated. Diagonal is ~100.5, so kept.
        assert!(v.sees_sprite(Vec2::new(1000.0, 190.0), Vec2::ONE, &s, Vec2::ONE));
    }

    #[test]
    fn explicit_corners_use_their_farthest_corner() {
        let v = view();
        let mut s = sprite([1.0, 1.0]);
        s.corners = Some([[-300.0, 0.0], [10.0, 0.0], [10.0, 5.0], [-300.0, 5.0]]);
        // Drawn quad reaches 300 to the left of its position at x = 1450:
        // that is x = 1150, inside the view.
        assert!(v.sees_sprite(Vec2::new(1450.0, 0.0), Vec2::ONE, &s, Vec2::ONE));
        s.corners = Some([[-10.0, 0.0], [10.0, 0.0], [10.0, 5.0], [-10.0, 5.0]]);
        assert!(!v.sees_sprite(Vec2::new(1450.0, 0.0), Vec2::ONE, &s, Vec2::ONE));
    }

    #[test]
    fn a_sprite_with_no_size_uses_the_pass_default() {
        let v = view();
        let s = SpriteComponent::default();
        assert!(v.sees_sprite(Vec2::new(1230.0, 0.0), Vec2::ONE, &s, Vec2::splat(64.0)));
        assert!(!v.sees_sprite(Vec2::new(1230.0, 0.0), Vec2::ONE, &s, Vec2::splat(1.0)));
    }

    #[test]
    fn a_turned_camera_sees_a_wider_box() {
        let mut c = Camera2D::new(800.0, 600.0);
        c.zoom = 1.0;
        let flat = FrameView::of(&c);
        c.rotation = std::f32::consts::FRAC_PI_4;
        let turned = FrameView::of(&c);
        assert!(turned.half.x > flat.half.x && turned.half.y > flat.half.y);
    }

    #[test]
    fn nothing_is_culled_until_a_host_sets_a_view() {
        clear_frame_view();
        assert_eq!(frame_view(), None);
        let c = Camera2D::new(10.0, 10.0);
        set_frame_view(&c);
        assert_eq!(frame_view(), Some(FrameView::of(&c)));
        clear_frame_view();
        assert_eq!(frame_view(), None);
    }
}
