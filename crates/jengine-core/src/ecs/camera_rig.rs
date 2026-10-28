//! Cameras that hand the view to one another, and a shape a camera's view
//! stays inside (Oct 2026; Cinemachine's priorities, blends and confiner).
//!
//! The **main camera** is the active, untargeted `Camera2DComponent` with the
//! highest `priority` - the first of them on a tie, which is what a scene of
//! one camera, or of several all at 0, has always got ([`main_camera`]).
//! When the main camera changes to one with a `blend`, `update_camera` eases
//! the view from where it was to where the new camera is over that many
//! seconds (`renderer::camera::CameraBlend`), the new camera following its
//! target the whole time as if no blend were running. A camera's `confine`
//! is a polygon in world units that its **view** - not only its centre -
//! stays inside ([`confine`]), so a gallery of any shape shows no wall's far
//! side.

use crate::detmath::DetMath;
use glam::Vec2;
use hecs::{Entity, World};

use crate::ecs::components::Camera2DComponent;

/// The camera that draws the screen: the active camera with no target of the
/// highest `priority`, the first of them on a tie. Every "which camera is
/// the active one" question asks this, so they all agree.
pub fn main_camera(world: &World) -> Option<Entity> {
    let mut best: Option<(Entity, i32)> = None;
    for (e, cam) in world.query::<&Camera2DComponent>().iter() {
        if !cam.is_active || !cam.target.is_empty() {
            continue;
        }
        if best.is_none_or(|(_, p)| cam.priority > p) {
            best = Some((e, cam.priority));
        }
    }
    best.map(|(e, _)| e)
}

/// `Camera.set(e, field, value)`: any field of the entity's camera by name -
/// `priority` to hand it the view, `blend`, `confine`, `zoom`, `is_active`.
/// `screen_rect` has its own setter (`Camera.set_screen_rect`), which also
/// gives the camera its target.
pub fn set_camera_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    if field == "screen_rect" {
        return Err("use Camera.set_screen_rect for 'screen_rect'".into());
    }
    let mut cam = world.get::<&mut Camera2DComponent>(entity).map_err(|_| "the entity has no camera".to_string())?;
    let mut whole = serde_json::to_value(&*cam).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    // Fields left out of the JSON while at their defaults.
    let known = ["priority", "blend", "confine", "stretch", "pixel_size", "integer_scale", "target", "target_size", "cull_mask"];
    if !map.contains_key(field) && !known.contains(&field) {
        return Err(format!("'{field}' is not a camera field"));
    }
    // An empty Lua table arrives as `{}`; for a list field it means none.
    let empty_table = value.as_object().is_some_and(|o| o.is_empty());
    let value = if empty_table && matches!(map.get(field), Some(serde_json::Value::Array(_)) | None) {
        serde_json::Value::Array(Vec::new())
    } else {
        value.clone()
    };
    map.insert(field.to_string(), value);
    *cam = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    Ok(())
}

/// How far inside a wall the view may sit and still fit - so a view resting
/// exactly against one is not refused for rounding.
const TOUCH: f32 = 0.01;

/// Where a view of half-size `half` should be centred to show as much of
/// `want` as it can while staying inside `polygon` (world units, either
/// winding, any shape without self-crossings).
///
/// The view is pushed off every wall it overlaps and round every inward
/// corner poking into it; when that is not enough - a corridor narrower than
/// the push could judge - it slides from `from` (last frame's centre, if
/// that fitted) toward the pushed point as far as it fits, and along each
/// axis. A view larger than the shape keeps only its centre inside.
pub fn confine(polygon: &[[f32; 2]], want: Vec2, half: Vec2, from: Option<Vec2>) -> Vec2 {
    if polygon.len() < 3 || !want.is_finite() {
        return want;
    }
    let pts: Vec<Vec2> = polygon.iter().map(|p| Vec2::from(*p)).collect();
    let half = half.max(Vec2::ZERO);
    let pushed = depenetrate(&pts, want, half);
    if view_fits(&pts, pushed, half) {
        return pushed;
    }
    let start = from
        .filter(|&p| p.is_finite() && view_fits(&pts, p, half))
        .or_else(|| nearest_fitting_sample(&pts, want, half));
    let Some(start) = start else {
        return nearest_inside(&pts, want);
    };
    let fits = |c: Vec2| view_fits(&pts, c, half);
    let p = furthest(start, pushed, &fits);
    let p = furthest(p, Vec2::new(pushed.x, p.y), &fits);
    furthest(p, Vec2::new(p.x, pushed.y), &fits)
}

/// Whether a view centred at `c` lies inside the polygon: its centre is in,
/// and no wall passes through it.
pub fn view_fits(pts: &[Vec2], c: Vec2, half: Vec2) -> bool {
    if !point_in_polygon(pts, c) {
        return false;
    }
    let (lo, hi) = (c - half + TOUCH, c + half - TOUCH);
    let n = pts.len();
    (0..n).all(|i| !segment_enters_box(pts[i], pts[(i + 1) % n], lo, hi))
}

/// From `a` toward `b`, the farthest point that still fits (`a` must).
fn furthest(a: Vec2, b: Vec2, fits: &impl Fn(Vec2) -> bool) -> Vec2 {
    if fits(b) {
        return b;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..18 {
        let m = (lo + hi) * 0.5;
        if fits(a.lerp(b, m)) {
            lo = m;
        } else {
            hi = m;
        }
    }
    a.lerp(b, lo)
}

/// Push a view off the walls it overlaps. Walls push straight inward by how
/// far the view crosses them; an inward (reflex) corner inside the view
/// pushes it off along the shorter way out, since its two walls' lines run
/// through room the view may have.
fn depenetrate(pts: &[Vec2], want: Vec2, half: Vec2) -> Vec2 {
    let n = pts.len();
    let ccw = signed_area(pts) > 0.0;
    let reflex: Vec<bool> = (0..n)
        .map(|i| {
            let (p, v, q) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let turn = (v - p).perp_dot(q - v);
            if ccw { turn < 0.0 } else { turn > 0.0 }
        })
        .collect();
    let mut c = nearest_inside(pts, want);
    for _ in 0..8 {
        let mut moved = false;
        let inside = |p: Vec2, c: Vec2| {
            let d = (p - c).abs();
            d.x < half.x - TOUCH && d.y < half.y - TOUCH
        };
        for i in 0..n {
            let j = (i + 1) % n;
            let (a, b) = (pts[i], pts[j]);
            let d = b - a;
            let len = d.length();
            if len < 1e-6 || !segment_enters_box(a, b, c - half + TOUCH, c + half - TOUCH) {
                continue;
            }
            if (reflex[i] && inside(a, c)) || (reflex[j] && inside(b, c)) {
                continue;
            }
            let inward = if ccw { Vec2::new(-d.y, d.x) } else { Vec2::new(d.y, -d.x) } / len;
            let s = inward.dot(c - a);
            let r = inward.x.abs() * half.x + inward.y.abs() * half.y;
            if s < r {
                c += inward * (r - s);
                moved = true;
            }
        }
        for (i, &v) in pts.iter().enumerate() {
            if !reflex[i] || !inside(v, c) {
                continue;
            }
            let d = c - v;
            let out = half - d.abs();
            let sign = |x: f32| if x < 0.0 { -1.0 } else { 1.0 };
            if out.x < out.y {
                c.x += out.x * sign(d.x);
            } else {
                c.y += out.y * sign(d.y);
            }
            moved = true;
        }
        if !moved {
            break;
        }
    }
    c
}

/// The fitting point nearest `want` on a grid over the polygon's bounds, for
/// a view that has nowhere fitting to slide from (the frame a camera takes
/// over). `None` when the view fits nowhere on it.
fn nearest_fitting_sample(pts: &[Vec2], want: Vec2, half: Vec2) -> Option<Vec2> {
    let (lo, hi) = pts.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    const N: usize = 32;
    let mut best: Option<(Vec2, f32)> = None;
    for iy in 0..=N {
        for ix in 0..=N {
            let p = lo + (hi - lo) * Vec2::new(ix as f32, iy as f32) / N as f32;
            let p = depenetrate(pts, p, half);
            if view_fits(pts, p, half) {
                let d = p.distance_squared(want);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((p, d));
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

/// `p` if it is inside the polygon, else the nearest point of its outline.
fn nearest_inside(pts: &[Vec2], p: Vec2) -> Vec2 {
    if point_in_polygon(pts, p) {
        return p;
    }
    let n = pts.len();
    let mut best = (pts[0], f32::MAX);
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let d = b - a;
        let t = if d.length_squared() > 0.0 { ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        let q = a + d * t;
        let dist = q.distance_squared(p);
        if dist < best.1 {
            best = (q, dist);
        }
    }
    best.0
}

fn signed_area(pts: &[Vec2]) -> f32 {
    let n = pts.len();
    (0..n).map(|i| pts[i].perp_dot(pts[(i + 1) % n])).sum::<f32>() * 0.5
}

/// Even-odd point-in-polygon.
pub fn point_in_polygon(pts: &[Vec2], p: Vec2) -> bool {
    let n = pts.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (pts[i], pts[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Whether segment `a`-`b` passes through the open box `lo`..`hi`
/// (Liang-Barsky).
fn segment_enters_box(a: Vec2, b: Vec2, lo: Vec2, hi: Vec2) -> bool {
    if lo.x >= hi.x || lo.y >= hi.y {
        return false;
    }
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-d.x, a.x - lo.x), (d.x, hi.x - a.x), (-d.y, a.y - lo.y), (d.y, hi.y - a.y)] {
        if p == 0.0 {
            if q <= 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return false;
                }
                t0 = t0.max(r);
            } else {
                if r < t0 {
                    return false;
                }
                t1 = t1.min(r);
            }
        }
    }
    t0 < t1
}


/// Screen pixels -> world, through the scene's active `Camera2DComponent`
/// (or an unzoomed camera at the origin the size of the screen). The same
/// arithmetic as `Camera2D::screen_to_world` and the browser preamble's
/// `get_mouse_world_position`, the camera's own rotation included.
pub fn mouse_world_position(world: &hecs::World, mouse: (f32, f32), screen: (f32, f32)) -> glam::Vec2 {
    use crate::ecs::components::{Camera2DComponent, Transform};
    let (sw, sh) = (screen.0.max(1.0), screen.1.max(1.0));
    let (mut cx, mut cy, mut vw, mut vh, mut zoom) = (0.0, 0.0, sw, sh, 1.0);
    let mut rotation = 0.0f32;
    let main = crate::ecs::camera_rig::main_camera(world);
    for (e, (t, cam)) in world.query::<(&Transform, &Camera2DComponent)>().iter() {
        if Some(e) != main {
            continue;
        }
        cx = t.position.x;
        cy = t.position.y;
        rotation = t.rotation;
        if cam.viewport_width > 0.0 && cam.viewport_height > 0.0 {
            // The world the screen actually shows, as the projection has it.
            let view = cam.view_on(glam::Vec2::new(sw, sh));
            vw = view.x;
            vh = view.y;
        }
        if cam.zoom > 0.0 {
            zoom = cam.zoom;
        }
        break;
    }
    let nx = (mouse.0 - sw * 0.5) / sw;
    let ny = (sh * 0.5 - mouse.1) / sh;
    let (ox, oy) = (nx * (vw / zoom), ny * (vh / zoom));
    let (sin, cos) = rotation.dsin_cos();
    glam::Vec2::new(cx + ox * cos - oy * sin, cy + ox * sin + oy * cos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(p: &[[f32; 2]]) -> Vec<Vec2> {
        p.iter().map(|q| Vec2::from(*q)).collect()
    }

    /// An L: a wide gallery along the bottom and a tall one up its left.
    const L: [[f32; 2]; 6] = [[0.0, 0.0], [1000.0, 0.0], [1000.0, 400.0], [400.0, 400.0], [400.0, 1000.0], [0.0, 1000.0]];

    #[test]
    fn a_rectangle_confines_like_a_clamp() {
        let room = [[0.0, 0.0], [1000.0, 0.0], [1000.0, 600.0], [0.0, 600.0]];
        let half = Vec2::new(200.0, 100.0);
        for want in [Vec2::new(-500.0, 300.0), Vec2::new(500.0, 900.0), Vec2::new(990.0, 5.0), Vec2::new(500.0, 300.0)] {
            let got = confine(&room, want, half, None);
            let clamp = want.clamp(Vec2::new(200.0, 100.0), Vec2::new(800.0, 500.0));
            assert!(got.distance(clamp) < 0.5, "{want} -> {got}, a clamp gives {clamp}");
        }
    }

    #[test]
    fn the_view_never_leaves_an_l_shaped_gallery() {
        let p = pts(&L);
        let half = Vec2::new(160.0, 90.0);
        let mut from = None;
        for iy in -4..=24 {
            for ix in -4..=24 {
                let want = Vec2::new(ix as f32 * 50.0, iy as f32 * 50.0);
                let got = confine(&L, want, half, from);
                assert!(view_fits(&p, got, half), "{want} -> {got} shows past a wall");
                from = Some(got);
            }
        }
    }

    #[test]
    fn a_view_rounds_the_inside_corner_without_jumping() {
        // Walk the target round the L's inside corner; the view follows in
        // steps no longer than the target's.
        let p = pts(&L);
        let half = Vec2::new(160.0, 90.0);
        let mut at = confine(&L, Vec2::new(900.0, 200.0), half, None);
        let path: Vec<Vec2> = (0..=100)
            .map(|i| {
                let t = i as f32 / 100.0;
                if t < 0.5 { Vec2::new(900.0 - 1400.0 * t, 200.0) } else { Vec2::new(200.0, 200.0 + 1400.0 * (t - 0.5)) }
            })
            .collect();
        for w in path.windows(2) {
            let next = confine(&L, w[1], half, Some(at));
            assert!(view_fits(&p, next, half));
            assert!(next.distance(at) <= w[1].distance(w[0]) + 1.0, "jumped {} for a step of {}", next.distance(at), w[1].distance(w[0]));
            at = next;
        }
        assert!(at.y > 800.0, "the view went up the tall gallery: {at}");
    }

    #[test]
    fn a_view_larger_than_the_room_keeps_its_centre_inside() {
        let room = [[0.0, 0.0], [300.0, 0.0], [300.0, 200.0], [0.0, 200.0]];
        let got = confine(&room, Vec2::new(900.0, 100.0), Vec2::new(640.0, 360.0), None);
        assert!((got.x - 300.0).abs() < 1e-3 && (got.y - 100.0).abs() < 1e-3, "{got}");
    }

    #[test]
    fn either_winding_confines_the_same() {
        let mut cw = L;
        cw.reverse();
        let half = Vec2::new(160.0, 90.0);
        for want in [Vec2::new(800.0, 600.0), Vec2::new(-50.0, 500.0), Vec2::new(390.0, 390.0)] {
            let a = confine(&L, want, half, None);
            let b = confine(&cw, want, half, None);
            assert!(a.distance(b) < 0.5, "{want}: {a} against {b}");
        }
    }

    #[test]
    fn the_main_camera_is_the_highest_priority() {
        let mut world = World::new();
        let cam = |priority, is_active, target: &str| Camera2DComponent { priority, is_active, target: target.into(), ..Default::default() };
        let first = world.spawn((cam(0, true, ""),));
        let _tie = world.spawn((cam(0, true, ""),));
        assert_eq!(main_camera(&world), Some(first), "a tie goes to the first, as it always did");
        let high = world.spawn((cam(5, true, ""),));
        let _off = world.spawn((cam(9, false, ""),));
        let _view = world.spawn((cam(9, true, "mon"),));
        assert_eq!(main_camera(&world), Some(high), "inactive cameras and views never draw the screen");
    }

    #[test]
    fn camera_fields_are_set_by_name() {
        let mut world = World::new();
        let e = world.spawn((Camera2DComponent::default(),));
        // As a float, too: a Lua number reaches native as one.
        set_camera_field(&world, e, "priority", &serde_json::json!(3.0)).unwrap();
        set_camera_field(&world, e, "blend", &serde_json::json!(0.8)).unwrap();
        set_camera_field(&world, e, "confine", &serde_json::json!([[0, 0], [10, 0], [10, 10]])).unwrap();
        let c = world.get::<&Camera2DComponent>(e).unwrap();
        assert_eq!((c.priority, c.blend, c.confine.len()), (3, 0.8, 3));
        drop(c);
        // `Camera.set(cam, "confine", {})` - an empty Lua table is an object.
        set_camera_field(&world, e, "confine", &serde_json::json!({})).unwrap();
        assert!(world.get::<&Camera2DComponent>(e).unwrap().confine.is_empty());
        assert!(set_camera_field(&world, e, "speed", &serde_json::json!(1)).is_err());
        assert!(set_camera_field(&world, e, "screen_rect", &serde_json::json!([0, 0, 1, 1])).is_err());
    }
}
