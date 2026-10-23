//! **Mode 7** (Oct 2026): a textured ground plane seen in perspective, and
//! sprites standing on it scaled by their distance - the SNES racing view.
//!
//! A [`Mode7`] is a screen rectangle (`size`, centred on its entity) showing
//! the ground texture (`texture_id`, covering `ground_size` ground units about
//! the ground origin) from a camera at ground point (`x`, `y`) facing `angle`
//! (radians, 0 along +x), `height` above it, with a horizontal field of view
//! `fov`; the horizon sits `horizon` of the way up the rectangle, and the
//! ground is drawn out to `far`. The whole visible ground is **one quad** -
//! the view's trapezoid on the ground mapped to the rectangle below the
//! horizon - with a perspective weight a vertex (`q = 1 / depth`), so the
//! sprite shader textures it in true perspective and no scanline pass is
//! needed. Samplers clamp, so a ground texture with a border of grass reads
//! as grass forever. With `follow` naming an entity that has a
//! [`Mode7Sprite`], the camera sits `follow_distance` behind it, facing its
//! way.
//!
//! A [`Mode7Sprite`] stands on the ground of the plane named by `plane` (the
//! first plane when empty): each frame its `Transform` is written to where
//! its ground point (`x`, `y`) falls on the screen and its scale to how big
//! it is there, so its sprite (anchored at its feet, `size` in ground units)
//! shrinks into the distance - and `y_sort` on the sprite, which sorts by
//! screen y, then puts the near in front. With `frames` it picks one of a
//! strip of views (frame 0 seen from behind, going round anticlockwise) by
//! its `angle` against the camera's. Behind the camera or past `far` it is
//! hidden (its scale written 0).
//!
//! Both are stepped in `tick_motion` and the plane drawn by
//! `render_world_items` as a `Skinned` item, so no host was wired. Scripts
//! move things with `Mode7.place(e, x, y, angle)` and change any field with
//! `Mode7.set(e, field, value)` (either component).

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Name, SpriteComponent, Transform, WorldTransform};

/// A ground plane drawn in perspective.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Mode7 {
    pub texture_id: String,
    /// Ground units the texture covers, centred on the ground origin.
    pub ground_size: [f32; 2],
    /// The camera: where it stands on the ground and which way it faces.
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    /// How high above the ground it looks from, in ground units.
    pub height: f32,
    /// Horizontal field of view, radians.
    pub fov: f32,
    /// Where the horizon is, 0 (bottom) to 1 (top) of the rectangle.
    pub horizon: f32,
    /// How far the ground is drawn, in ground units.
    pub far: f32,
    /// The screen rectangle, world units, centred on the entity.
    pub size: [f32; 2],
    /// Colour multiplied into the ground near the camera, and at `far`.
    pub near_color: [f32; 4],
    pub far_color: [f32; 4],
    /// An entity with a `mode7_sprite` the camera follows (empty: none).
    pub follow: String,
    /// How far behind it the camera sits.
    pub follow_distance: f32,
    pub z_order: i32,
}

impl Default for Mode7 {
    fn default() -> Self {
        Self {
            texture_id: String::new(),
            ground_size: [1024.0, 1024.0],
            x: 0.0,
            y: 0.0,
            angle: 0.0,
            height: 24.0,
            fov: 1.4,
            horizon: 0.6,
            far: 900.0,
            size: [1280.0, 720.0],
            near_color: [1.0, 1.0, 1.0, 1.0],
            far_color: [1.0, 1.0, 1.0, 1.0],
            follow: String::new(),
            follow_distance: 40.0,
            z_order: -50,
        }
    }
}

/// A sprite standing on a Mode 7 plane.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Mode7Sprite {
    /// The plane it stands on, by name (empty: the first).
    pub plane: String,
    /// Where it stands, in ground units, and which way it faces.
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    /// Views in its texture, a strip left to right (0 or 1: one view).
    pub frames: u32,
}

impl Default for Mode7Sprite {
    fn default() -> Self {
        Self { plane: String::new(), x: 0.0, y: 0.0, angle: 0.0, frames: 0 }
    }
}

/// How a plane's camera sees: its frame on the ground and its lens.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub eye: Vec2,
    pub fwd: Vec2,
    pub right: Vec2,
    pub focal: f32,
    pub height: f32,
    pub far: f32,
    /// The rectangle: its centre, half size, and the horizon's world y.
    pub centre: Vec2,
    pub half: Vec2,
    pub horizon_y: f32,
}

impl Mode7 {
    pub fn view(&self, centre: Vec2) -> View {
        let fwd = Vec2::new(self.angle.cos(), self.angle.sin());
        let half = Vec2::from(self.size) * 0.5;
        View {
            eye: Vec2::new(self.x, self.y),
            fwd,
            right: Vec2::new(fwd.y, -fwd.x),
            focal: half.x / (self.fov.clamp(0.1, 3.0) * 0.5).tan(),
            height: self.height.max(0.01),
            far: self.far.max(1.0),
            centre,
            half,
            horizon_y: centre.y - half.y + self.horizon.clamp(0.05, 0.95) * 2.0 * half.y,
        }
    }
}

impl View {
    /// Where a ground point falls on the screen and how many world units a
    /// ground unit is there; `None` behind the camera or past `far`.
    pub fn project(&self, p: Vec2) -> Option<(Vec2, f32)> {
        let d = p - self.eye;
        let depth = d.dot(self.fwd);
        if depth < 1.0 || depth > self.far {
            return None;
        }
        let k = self.focal / depth;
        Some((Vec2::new(self.centre.x + d.dot(self.right) * k, self.horizon_y - self.height * k), k))
    }

    /// The ground point under a screen point below the horizon.
    pub fn unproject(&self, s: Vec2) -> Option<Vec2> {
        let below = self.horizon_y - s.y;
        if below <= 0.0 {
            return None;
        }
        let depth = self.height * self.focal / below;
        let lateral = (s.x - self.centre.x) * depth / self.focal;
        Some(self.eye + self.fwd * depth + self.right * lateral)
    }
}

/// The plane as one quad: positions (screen, world units), `(u q, v q, q)`,
/// colours and indices. Empty if the horizon is below the rectangle.
#[allow(clippy::type_complexity)]
pub fn quad(m: &Mode7, centre: Vec2) -> (Vec<Vec2>, Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let v = m.view(centre);
    let bottom = v.centre.y - v.half.y;
    let near_below = v.horizon_y - bottom;
    if near_below <= 0.0 {
        return Default::default();
    }
    let near = v.height * v.focal / near_below;
    let far = v.far.max(near * 1.01);
    let far_y = v.horizon_y - v.height * v.focal / far;
    let (l, r) = (v.centre.x - v.half.x, v.centre.x + v.half.x);
    let screen = [Vec2::new(l, bottom), Vec2::new(r, bottom), Vec2::new(r, far_y), Vec2::new(l, far_y)];
    let size = Vec2::from(m.ground_size).max(Vec2::splat(1.0));
    let mut uvq = Vec::with_capacity(4);
    for (i, s) in screen.iter().enumerate() {
        let depth = if i < 2 { near } else { far };
        let lateral = (s.x - v.centre.x) * depth / v.focal;
        let g = v.eye + v.fwd * depth + v.right * lateral;
        // Texture v runs down the image: ground +y is up the texture.
        let (u, vv) = (g.x / size.x + 0.5, 0.5 - g.y / size.y);
        let q = 1.0 / depth;
        uvq.push([u * q, vv * q, q]);
    }
    let colors = vec![m.near_color, m.near_color, m.far_color, m.far_color];
    (screen.to_vec(), uvq, colors, vec![0, 1, 2, 0, 2, 3])
}

fn named(world: &World, name: &str) -> Option<Entity> {
    world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e)
}

/// Follow, then place every sprite on its plane. Called from `tick_motion`.
pub fn tick_mode7(world: &mut World) {
    let planes: Vec<Entity> = world.query::<&Mode7>().iter().map(|(e, _)| e).collect();
    if planes.is_empty() {
        return;
    }
    // Cameras that follow a sprite.
    for &p in &planes {
        let follow = world.get::<&Mode7>(p).map(|m| (m.follow.clone(), m.follow_distance)).unwrap_or_default();
        if follow.0.is_empty() {
            continue;
        }
        let Some(t) = named(world, &follow.0) else { continue };
        let Some((x, y, a)) = world.get::<&Mode7Sprite>(t).ok().map(|s| (s.x, s.y, s.angle)) else { continue };
        if let Ok(mut m) = world.get::<&mut Mode7>(p) {
            m.angle = a;
            m.x = x - a.cos() * follow.1;
            m.y = y - a.sin() * follow.1;
        }
    }
    let views: Vec<(Entity, String, View, f32)> = planes
        .iter()
        .filter_map(|&p| {
            let m = world.get::<&Mode7>(p).ok()?;
            let centre = world
                .get::<&WorldTransform>(p)
                .map(|w| w.position)
                .or_else(|_| world.get::<&Transform>(p).map(|t| t.position))
                .unwrap_or_default();
            let name = world.get::<&Name>(p).map(|n| n.0.clone()).unwrap_or_default();
            Some((p, name, m.view(centre), m.angle))
        })
        .collect();
    for (_, (s, t, sprite)) in world.query_mut::<(&Mode7Sprite, &mut Transform, Option<&mut SpriteComponent>)>() {
        let Some((_, _, v, cam_angle)) = views.iter().find(|(_, n, ..)| s.plane.is_empty() || *n == s.plane) else { continue };
        match v.project(Vec2::new(s.x, s.y)) {
            Some((at, k)) => {
                t.position = at;
                t.scale = Vec2::splat(k);
            }
            None => t.scale = Vec2::ZERO,
        }
        t.rotation = 0.0;
        if let Some(sp) = sprite
            && s.frames > 1
        {
            // Frame 0 seen from behind; round anticlockwise.
            let rel = (s.angle - cam_angle).rem_euclid(std::f32::consts::TAU);
            let n = s.frames as f32;
            let f = ((rel / std::f32::consts::TAU * n).round() as u32) % s.frames;
            sp.uv_rect = Some([f as f32 / n, 0.0, (f + 1) as f32 / n, 1.0]);
        }
    }
}

/// `Mode7.place(e, x, y, angle)`: stand a sprite (or a plane's camera).
pub fn place(world: &mut World, e: Entity, x: f32, y: f32, angle: f32) -> Result<(), String> {
    if let Ok(mut s) = world.get::<&mut Mode7Sprite>(e) {
        (s.x, s.y, s.angle) = (x, y, angle);
        return Ok(());
    }
    if let Ok(mut m) = world.get::<&mut Mode7>(e) {
        (m.x, m.y, m.angle) = (x, y, angle);
        return Ok(());
    }
    Err("no mode7 or mode7_sprite on that entity".into())
}

fn set_json<T: Serialize + serde::de::DeserializeOwned>(c: &mut T, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut v = serde_json::to_value(&*c).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    *c = serde_json::from_value(v).map_err(|e| format!("{field}: {e}"))?;
    Ok(())
}

/// `Mode7.set(e, field, value)`: any field of the entity's `mode7_sprite`,
/// else of its `mode7`.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    if let Ok(mut s) = world.get::<&mut Mode7Sprite>(e)
        && set_json(&mut *s, field, value).is_ok()
    {
        return Ok(());
    }
    match world.get::<&mut Mode7>(e) {
        Ok(mut m) => set_json(&mut *m, field, value),
        Err(_) => Err("no mode7 or mode7_sprite on that entity".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane() -> Mode7 {
        Mode7 { height: 20.0, fov: 1.5, horizon: 0.5, far: 1000.0, size: [800.0, 400.0], ..Default::default() }
    }

    #[test]
    fn a_ground_point_projects_and_comes_back() {
        let m = Mode7 { x: 10.0, y: -5.0, angle: 0.7, ..plane() };
        let v = m.view(Vec2::new(100.0, 50.0));
        for p in [Vec2::new(60.0, 40.0), Vec2::new(200.0, 150.0), Vec2::new(30.0, 80.0)] {
            let (s, _) = v.project(p).expect("in front");
            let back = v.unproject(s).unwrap();
            assert!(back.distance(p) < 1e-2, "{p} -> {s} -> {back}");
        }
        // Farther is higher on the screen and smaller.
        let (near, kn) = v.project(m.view(Vec2::ZERO).eye + Vec2::new(0.7f32.cos(), 0.7f32.sin()) * 50.0).unwrap();
        let (far, kf) = v.project(Vec2::new(10.0, -5.0) + Vec2::new(0.7f32.cos(), 0.7f32.sin()) * 400.0).unwrap();
        assert!(far.y > near.y && kf < kn);
        // Behind: not drawn.
        assert!(v.project(Vec2::new(10.0, -5.0) - Vec2::new(0.7f32.cos(), 0.7f32.sin()) * 50.0).is_none());
    }

    #[test]
    fn the_plane_is_one_quad_textured_in_perspective() {
        let m = Mode7 { ground_size: [1000.0, 1000.0], ..plane() };
        let (pos, uvq, _, idx) = quad(&m, Vec2::ZERO);
        assert_eq!((pos.len(), idx.len()), (4, 6));
        // Its bottom is the rectangle's, its top where `far` falls - under
        // the horizon by height x focal / far.
        let v = m.view(Vec2::ZERO);
        assert!((pos[0].y + 200.0).abs() < 1e-3);
        assert!((pos[2].y + v.height * v.focal / v.far).abs() < 1e-3, "{}", pos[2].y);
        // A pixel's texture coordinate, divided by q, is the ground under it:
        // check the middle of the bottom edge against the camera's view.
        let mid = |a: [f32; 3], b: [f32; 3]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
        let c = mid(uvq[0], uvq[1]);
        let ground = v.unproject(Vec2::new(0.0, -200.0)).unwrap();
        let (u, vv) = (c[0] / c[2], c[1] / c[2]);
        assert!((u - (ground.x / 1000.0 + 0.5)).abs() < 1e-4 && (vv - (0.5 - ground.y / 1000.0)).abs() < 1e-4);
        // And up the screen, where perspective matters: interpolated in
        // screen space and divided by q, as the shader does, a point a third
        // of the way up lands on the ground under it.
        let (bottom, top) = (mid(uvq[0], uvq[1]), mid(uvq[3], uvq[2]));
        let t = 0.33;
        let y = pos[0].y + (pos[3].y - pos[0].y) * t;
        let c: Vec<f32> = (0..3).map(|i| bottom[i] + (top[i] - bottom[i]) * t).collect();
        let ground = v.unproject(Vec2::new(0.0, y)).unwrap();
        assert!((c[0] / c[2] - (ground.x / 1000.0 + 0.5)).abs() < 1e-3, "u in perspective");
        assert!((c[1] / c[2] - (0.5 - ground.y / 1000.0)).abs() < 1e-3, "v in perspective");
    }
}
