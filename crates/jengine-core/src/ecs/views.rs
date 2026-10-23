//! **More than one camera a frame** (Sep 2026): a camera with a `target`
//! draws the world into a texture of that name, which any sprite, mesh or
//! UI image can show by naming it as its `texture_id`.
//!
//! The main camera is the active one with no target, as before. Every
//! active camera *with* a target is a **view**: each frame, before the main
//! camera, the host draws the world through it with the same world pass -
//! lights, shadows, Scene Color, the post chain - into its own buffers, and
//! the result is the texture. So a security monitor shows the other room
//! lit as the room is lit, a minimap is a second camera far above, and
//! split-screen is two views shown side by side - which a camera's
//! `screen_rect` sets up by itself ([`sync_screen_rects`]).
//!
//! Until then there was one camera a frame, and every one of those needed a
//! second drawing of the world that nothing could make - the editor's
//! picture-in-picture was the only second view, drawn unlit and straight
//! onto the display. What this module decides is pure (which cameras are
//! views, and what each sees); `wgpu_systems::sync_view_targets` makes
//! their buffers.

use glam::Vec2;
use hecs::World;

use crate::ecs::components::{Camera2DComponent, Transform, WorldTransform};
use crate::renderer::camera::Camera2D;

/// One view to draw this frame.
#[derive(Debug, Clone)]
pub struct ViewCamera {
    /// The texture id it draws into.
    pub target: String,
    /// The texture's size in pixels.
    pub size: (u32, u32),
    /// What it sees: the camera entity's world position and turn, its zoom
    /// and pixel size, its authored area fitted to the texture's shape.
    pub camera: Camera2D,
    /// Clear to transparent rather than the scene's background, and draw
    /// with no post effects: a canvas group's picture
    /// (`ecs::canvas_group`), composited over the world.
    pub transparent: bool,
}

/// Every active camera with a `target`, in a stable order (by target id,
/// so two views of each other draw the same way round every frame).
/// `surface` sizes a target whose `target_size` is left at zero - to its
/// share of the screen for a camera with a `screen_rect`.
pub fn render_target_cameras(world: &World, surface: (u32, u32)) -> Vec<ViewCamera> {
    let mut views: Vec<ViewCamera> = world
        .query::<(&Camera2DComponent, Option<&Transform>, Option<&WorldTransform>)>()
        .iter()
        .filter(|(_, (c, _, _))| c.is_active && !c.target.is_empty())
        .map(|(_, (c, t, w))| {
            let share = if has_screen_rect(c) { [c.screen_rect[2], c.screen_rect[3]] } else { [1.0, 1.0] };
            let size = (
                if c.target_size[0] > 0 {
                    c.target_size[0]
                } else {
                    ((surface.0 as f32 * share[0]).round() as u32).max(1)
                },
                if c.target_size[1] > 0 {
                    c.target_size[1]
                } else {
                    ((surface.1 as f32 * share[1]).round() as u32).max(1)
                },
            );
            let view = c.view_on(Vec2::new(size.0 as f32, size.1 as f32));
            let mut camera = Camera2D::new(view.x, view.y);
            camera.zoom = if c.zoom > 0.0 { c.zoom } else { 1.0 };
            camera.pixel_size = c.pixel_size.max(0.0);
            camera.integer_scale = c.integer_scale;
            camera.cull_mask = c.cull_mask;
            camera.pass = crate::renderer::camera::CameraPass::View;
            match (w, t) {
                (Some(w), _) => {
                    camera.position = w.position;
                    camera.rotation = w.rotation;
                }
                (None, Some(t)) => {
                    camera.position = t.position;
                    camera.rotation = t.rotation;
                }
                (None, None) => {}
            }
            ViewCamera { target: c.target.clone(), size, camera, transparent: false }
        })
        .collect();
    views.sort_by(|a, b| a.target.cmp(&b.target));
    views.dedup_by(|a, b| a.target == b.target);
    views
}

/// Whether a camera draws onto a part of the screen (`screen_rect`).
pub fn has_screen_rect(c: &Camera2DComponent) -> bool {
    c.screen_rect[2] > 0.0 && c.screen_rect[3] > 0.0
}

/// The entity-UI image that shows a camera's `screen_rect`: made, kept in
/// place and removed by [`sync_screen_rects`]. Not registered, so never saved.
#[derive(Debug, Clone, Copy)]
pub struct ScreenRectImage(pub hecs::Entity);

/// The implied target of a camera with a `screen_rect` and no target of its
/// own: this and its entity id. Given back when the rect is cleared.
pub const SCREEN_TARGET: &str = "__screen/";

/// Where screen-rect images sit among the UI: under all of it, a whole
/// screen's 1,000 lower still, and less for a smaller part.
pub const SCREEN_RECT_Z: i32 = -10_000;

/// Split screen as one field (Oct 2026): every camera with a `screen_rect`
/// is given a target of its own (`__screen/<id>`) when it names none - so it
/// is a view, which every rule for "the main camera" already passes over;
/// cleared again, the rect gives that target back -
/// and an entity-UI image of that target over its part of the screen, under
/// every other UI. A view draws the world with its own lights and post
/// chain, and entity UI draws after the main frame's post chain, so each
/// part is graded once.
///
/// `screen_w` / `screen_h` are the UI's size in its own pixels, which is
/// why this runs at the start of `update_ui_widgets` - every host calls it
/// each frame with that size. Under `ScaleAndCenter` the UI is the authored
/// area, so the parts divide that area rather than the window.
pub fn sync_screen_rects(world: &mut World, screen_w: f32, screen_h: f32) {
    use crate::ui::{UIAnchor, UIElement, UIImage};
    for (_, c) in world.query_mut::<&mut Camera2DComponent>() {
        if !has_screen_rect(c) && c.target.starts_with(SCREEN_TARGET) {
            c.target.clear();
        }
    }
    let cams: Vec<(hecs::Entity, [f32; 4], String, bool)> = world
        .query_mut::<&mut Camera2DComponent>()
        .into_iter()
        .filter(|(_, c)| has_screen_rect(c))
        .map(|(e, c)| {
            if c.target.is_empty() {
                c.target = format!("{SCREEN_TARGET}{}", e.id());
            }
            (e, c.screen_rect, c.target.clone(), c.is_active)
        })
        .collect();
    let images: Vec<(hecs::Entity, hecs::Entity)> =
        world.query::<&ScreenRectImage>().iter().map(|(e, m)| (e, m.0)).collect();
    if cams.is_empty() && images.is_empty() {
        return;
    }
    for (image, cam) in &images {
        if !cams.iter().any(|(c, ..)| c == cam) {
            let _ = world.despawn(*image);
        }
    }
    for (cam, r, target, active) in cams {
        let element = UIElement {
            anchor: UIAnchor::TopLeft,
            offset: Vec2::new(r[0] * screen_w, -r[1] * screen_h),
            size: Vec2::new(r[2] * screen_w, r[3] * screen_h),
            visible: active,
            // A smaller part draws over a larger one, so an inset sits on
            // the view it is set into.
            z_order: SCREEN_RECT_Z - (r[2] * r[3] * 1000.0) as i32,
            ..Default::default()
        };
        match images.iter().find(|(_, c)| *c == cam) {
            Some((image, _)) => {
                if let Ok(mut el) = world.get::<&mut UIElement>(*image) {
                    *el = element;
                }
                if let Ok(mut img) = world.get::<&mut UIImage>(*image)
                    && img.texture_id != target
                {
                    img.texture_id = target;
                }
            }
            None => {
                world.spawn((element, UIImage { texture_id: target, ..Default::default() }, ScreenRectImage(cam)));
            }
        }
    }
}

/// `Camera.set_screen_rect(camera, x, y, w, h)`: the part of the screen the
/// camera draws onto; all zero for none. Applied by [`sync_screen_rects`].
pub fn set_screen_rect(world: &World, camera: hecs::Entity, rect: [f32; 4]) {
    if let Ok(mut c) = world.get::<&mut Camera2DComponent>(camera) {
        c.screen_rect = rect;
    } else {
        log::warn!("Camera.set_screen_rect: entity {} has no camera", camera.id());
    }
}

/// The texture ids cameras draw into - textures the world makes rather than
/// files, so nothing should try to load them.
pub fn render_target_ids(world: &World) -> std::collections::BTreeSet<String> {
    world
        .query::<&Camera2DComponent>()
        .iter()
        .filter(|(_, c)| !c.target.is_empty())
        .map(|(_, c)| c.target.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(target: &str, active: bool) -> Camera2DComponent {
        Camera2DComponent { is_active: active, target: target.into(), ..Default::default() }
    }

    #[test]
    fn a_view_is_an_active_camera_with_a_target() {
        let mut world = World::new();
        world.spawn((cam("", true), Transform::default()));
        world.spawn((cam("views/off", false), Transform::default()));
        world.spawn((
            Camera2DComponent { target_size: [320, 180], zoom: 2.0, ..cam("views/map", true) },
            Transform { position: Vec2::new(40.0, -10.0), ..Default::default() },
        ));
        let views = render_target_cameras(&world, (1280, 720));
        assert_eq!(views.len(), 1, "the main camera and an inactive one are not views");
        let v = &views[0];
        assert_eq!(v.target, "views/map");
        assert_eq!(v.size, (320, 180));
        assert_eq!(v.camera.position, Vec2::new(40.0, -10.0));
        assert_eq!(v.camera.zoom, 2.0);
        assert_eq!(v.camera.viewport_size(), (1280.0, 720.0), "the authored area, fitted to the texture's shape");
    }

    #[test]
    fn a_view_with_no_size_is_the_screens_and_fits_its_shape() {
        let mut world = World::new();
        world.spawn((Camera2DComponent { target_size: [0, 0], ..cam("views/half", true) },));
        let v = &render_target_cameras(&world, (640, 720))[0];
        assert_eq!(v.size, (640, 720));
        assert_eq!(v.camera.viewport_size(), (1280.0, 1440.0), "a tall target shows more world up and down");
    }

    #[test]
    fn render_targets_are_not_files() {
        let mut world = World::new();
        world.spawn((cam("views/map", false),));
        assert!(render_target_ids(&world).contains("views/map"));
    }
}
