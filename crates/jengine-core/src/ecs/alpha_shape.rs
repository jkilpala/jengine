//! **Shadows in the shape of the sprite** (Oct 2026): a `light_occluder`
//! with `from_alpha` casts the silhouette of its sprite's opaque pixels -
//! a tree's crown and trunk, the pickets of a fence - instead of the
//! sprite's box (`from_sprite`) or a polygon typed in by hand. Unity's
//! ShadowCaster2D "use renderer silhouette", Godot's polygon from a bitmap.
//!
//! The outline is traced from the texture once - its region of it, at the
//! occluder's `alpha_cutoff` - and simplified to within `alpha_tolerance`
//! texels, then kept in the component ([`LightOccluder::traced`], never
//! saved) until the sprite's texture, region, size or anchor change. Each
//! separate island of the sprite is a loop of its own, and holes are left
//! out: a shadow is the shape's outside. [`trace_occluder_shapes`] does the
//! work and needs the texture's pixels, so it runs in the one per-frame
//! step every host already makes with its assets - `carve::apply_carves`
//! calls it - and `gather_lights` reads the loops, so the GPU, the CPU's
//! `light_at` and every host see the same shape with no host wiring.

use std::collections::HashMap;

use glam::Vec2;
use hecs::World;

use super::carve::TexelFrame;
use super::components::{LightOccluder, SpriteComponent};
use crate::asset::AssetManager;

/// A traced outline, kept in the occluder: the loops in the entity's local
/// space (before its transform), and what they were traced from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TracedShape {
    pub key: u64,
    pub loops: Vec<Vec<[f32; 2]>>,
}

/// The most corners one loop keeps; a loop with more is simplified harder.
/// Every corner is an edge each nearby light spends from its pool of 448.
pub const MAX_LOOP_POINTS: usize = 48;

/// Trace (or keep) the outline of every `from_alpha` occluder whose sprite's
/// pixels are loaded. Cheap when nothing changed: one hash per occluder.
pub fn trace_occluder_shapes(world: &mut World, assets: &AssetManager) {
    for (_, (occ, sprite)) in world.query_mut::<(&mut LightOccluder, &SpriteComponent)>() {
        if !occ.from_alpha {
            continue;
        }
        let source = crate::asset::resolve_texture_id(&sprite.texture_id);
        let Some(tex) = assets.get_texture_bytes(source) else { continue };
        let frame = TexelFrame::of(sprite, tex.width, tex.height);
        let key = shape_key(source, &frame, occ.alpha_cutoff, occ.alpha_tolerance, tex.width, tex.height);
        if occ.traced.as_ref().is_some_and(|t| t.key == key) {
            continue;
        }
        let cutoff = (occ.alpha_cutoff.clamp(0.0, 1.0) * 255.0) as u8;
        let loops = trace_alpha(&tex.rgba, tex.width, tex.height, &frame, cutoff, occ.alpha_tolerance);
        occ.traced = Some(TracedShape { key, loops });
    }
}

/// Write the traced outline of each [`SpriteCollider`]'s sprite into its
/// `collider` (see the component). Cheap when nothing changed: one hash per
/// entity. Runs beside [`trace_occluder_shapes`] in `carve::apply_carves`.
pub fn trace_sprite_colliders(world: &mut World, assets: &AssetManager) {
    use crate::ecs::components::{Collider2D, ColliderShape, SpriteCollider, Transform};
    let mut written: Vec<(hecs::Entity, u64, Vec<[f32; 2]>, Vec<u32>)> = Vec::new();
    for (e, (sc, sprite, transform)) in world.query::<(&SpriteCollider, &SpriteComponent, Option<&Transform>)>().iter() {
        let source = crate::asset::resolve_texture_id(&sprite.texture_id);
        let Some(tex) = assets.get_texture_bytes(source) else { continue };
        let frame = TexelFrame::of(sprite, tex.width, tex.height);
        let scale = transform.map_or(glam::Vec2::ONE, |t| t.scale);
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            shape_key(source, &frame, sc.alpha_cutoff, sc.alpha_tolerance, tex.width, tex.height).hash(&mut h);
            (scale.x.to_bits(), scale.y.to_bits()).hash(&mut h);
            h.finish().max(1)
        };
        if sc.key == key {
            continue;
        }
        let cutoff = (sc.alpha_cutoff.clamp(0.0, 1.0) * 255.0) as u8;
        // Every island, one loop each, in one shape.
        let (mut vertices, mut starts) = (Vec::new(), Vec::new());
        for l in trace_alpha(&tex.rgba, tex.width, tex.height, &frame, cutoff, sc.alpha_tolerance) {
            if l.len() < 3 {
                continue;
            }
            if !vertices.is_empty() {
                starts.push(vertices.len() as u32);
            }
            vertices.extend(l.into_iter().map(|p| [p[0] * scale.x, p[1] * scale.y]));
        }
        written.push((e, key, vertices, starts));
    }
    for (e, key, vertices, loops) in written {
        if let Ok(mut sc) = world.get::<&mut SpriteCollider>(e) {
            sc.key = key;
        }
        if vertices.len() < 3 {
            continue;
        }
        let shape = ColliderShape::ConvexDecomposition { vertices, loops };
        if let Ok(mut c) = world.get::<&mut Collider2D>(e) {
            c.shape = shape;
            continue;
        }
        let _ = world.insert_one(e, Collider2D { shape, ..Default::default() });
    }
}

fn shape_key(source: &str, frame: &TexelFrame, cutoff: f32, tolerance: f32, w: u32, h: u32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    for v in [frame.size.x, frame.size.y, frame.anchor.x, frame.anchor.y, cutoff, tolerance] {
        v.to_bits().hash(&mut hasher);
    }
    for v in frame.texels {
        v.to_bits().hash(&mut hasher);
    }
    (w, h).hash(&mut hasher);
    hasher.finish()
}

/// The outlines of the opaque pixels (alpha above `cutoff`) inside `frame`'s
/// region of an `w` x `h` RGBA image, as loops in the sprite's local space:
/// one per island, holes left out, each simplified to within `tolerance`
/// texels (harder if it would keep more than [`MAX_LOOP_POINTS`]).
pub fn trace_alpha(rgba: &[u8], w: u32, h: u32, frame: &TexelFrame, cutoff: u8, tolerance: f32) -> Vec<Vec<[f32; 2]>> {
    let [tx0, ty0, tx1, ty1] = frame.texels;
    let x0 = tx0.min(tx1).floor().max(0.0) as i32;
    let x1 = (tx0.max(tx1).ceil() as i32).min(w as i32);
    let y0 = ty0.min(ty1).floor().max(0.0) as i32;
    let y1 = (ty0.max(ty1).ceil() as i32).min(h as i32);
    let solid = |x: i32, y: i32| -> bool {
        x >= x0 && x < x1 && y >= y0 && y < y1 && rgba[((y as u32 * w + x as u32) * 4 + 3) as usize] > cutoff
    };
    // Every edge between a solid texel and an empty one, going clockwise
    // round the solid texel (y down): the solid side is on the right.
    let mut next: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
    let mut edges = 0usize;
    for y in y0..y1 {
        for x in x0..x1 {
            if !solid(x, y) {
                continue;
            }
            let mut add = |a: (i32, i32), b: (i32, i32)| {
                next.entry(a).or_default().push(b);
                edges += 1;
            };
            if !solid(x, y - 1) {
                add((x, y), (x + 1, y));
            }
            if !solid(x + 1, y) {
                add((x + 1, y), (x + 1, y + 1));
            }
            if !solid(x, y + 1) {
                add((x + 1, y + 1), (x, y + 1));
            }
            if !solid(x - 1, y) {
                add((x, y + 1), (x, y));
            }
        }
    }
    // Chain them into loops. Where two texels touch only at a corner, two
    // edges leave one point: turning right (towards the solid side) keeps
    // the two texels' loops apart.
    let mut loops: Vec<Vec<(i32, i32)>> = Vec::new();
    let mut used = 0usize;
    while used < edges {
        let Some((&start, _)) = next.iter().find(|(_, v)| !v.is_empty()) else { break };
        let mut pts = vec![start];
        let mut at = start;
        let mut dir = (0i32, 0i32);
        loop {
            let outs = next.get_mut(&at).expect("an edge leaves every point on a loop");
            let pick = if outs.len() > 1 {
                let right = (-dir.1, dir.0);
                outs.iter().position(|&b| (b.0 - at.0, b.1 - at.1) == right).unwrap_or(0)
            } else {
                0
            };
            let to = outs.swap_remove(pick);
            used += 1;
            dir = (to.0 - at.0, to.1 - at.1);
            at = to;
            if at == start {
                break;
            }
            pts.push(at);
        }
        loops.push(pts);
    }
    // Which edge the chaining began on came out of a `HashMap`, so it differed
    // run to run - and the simplifier keeps a loop's first point, which one
    // texel from a corner cost the square its corner. Every loop starts at its
    // lowest-then-leftmost point instead (on an outer boundary always a real
    // corner), and the loops come in that order: the same outline every time.
    for l in &mut loops {
        let first = (0..l.len()).min_by_key(|&i| (l[i].1, l[i].0)).unwrap_or(0);
        l.rotate_left(first);
    }
    loops.sort_by_key(|l| (l[0].1, l[0].0));
    loops
        .into_iter()
        // Outer boundaries wind one way and holes the other (y down,
        // clockwise round the solid: a positive area).
        .filter(|l| signed_area(l) > 0.0)
        .map(|l| {
            let pts: Vec<Vec2> = l.iter().map(|&(x, y)| Vec2::new(x as f32, y as f32)).collect();
            let mut tol = tolerance.max(0.5);
            let mut simple = simplify_loop(&pts, tol);
            while simple.len() > MAX_LOOP_POINTS {
                tol *= 1.5;
                simple = simplify_loop(&pts, tol);
            }
            simple.into_iter().map(|t| frame.local_of(t).to_array()).collect::<Vec<_>>()
        })
        .filter(|l: &Vec<[f32; 2]>| l.len() >= 3)
        .collect()
}

fn signed_area(l: &[(i32, i32)]) -> f64 {
    let mut a = 0i64;
    for i in 0..l.len() {
        let (p, q) = (l[i], l[(i + 1) % l.len()]);
        a += p.0 as i64 * q.1 as i64 - q.0 as i64 * p.1 as i64;
    }
    a as f64 * 0.5
}

/// Ramer-Douglas-Peucker on a closed loop: split at the point farthest from
/// the first, simplify both halves, join.
fn simplify_loop(pts: &[Vec2], tol: f32) -> Vec<Vec2> {
    if pts.len() <= 3 {
        return pts.to_vec();
    }
    let far = (1..pts.len())
        .max_by(|&a, &b| pts[a].distance_squared(pts[0]).total_cmp(&pts[b].distance_squared(pts[0])))
        .unwrap_or(pts.len() / 2);
    let mut first: Vec<Vec2> = pts[..=far].to_vec();
    let mut second: Vec<Vec2> = pts[far..].to_vec();
    second.push(pts[0]);
    first = rdp(&first, tol);
    second = rdp(&second, tol);
    first.pop();
    second.pop();
    first.extend(second);
    drop_straight(first, tol)
}

/// Remove the corners that are not corners: the split points `simplify_loop`
/// keeps whatever their place, where they fall on a straight run.
fn drop_straight(mut pts: Vec<Vec2>, tol: f32) -> Vec<Vec2> {
    let mut i = 0;
    while pts.len() > 3 && i < pts.len() {
        let n = pts.len();
        let (a, p, b) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let ab = b - a;
        if (ab.perp_dot(p - a) / ab.length().max(1e-6)).abs() <= tol.min(0.5) {
            pts.remove(i);
        } else {
            i += 1;
        }
    }
    pts
}

fn rdp(pts: &[Vec2], tol: f32) -> Vec<Vec2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let ab = b - a;
    let len = ab.length().max(1e-6);
    let (mut worst, mut at) = (0.0f32, 0usize);
    for (i, p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
        let d = (ab.perp_dot(*p - a) / len).abs();
        if d > worst {
            worst = d;
            at = i;
        }
    }
    if worst <= tol {
        return vec![a, b];
    }
    let mut left = rdp(&pts[..=at], tol);
    let right = rdp(&pts[at..], tol);
    left.pop();
    left.extend(right);
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32, solid: impl Fn(u32, u32) -> bool) -> Vec<u8> {
        let mut out = Vec::new();
        for y in 0..h {
            for x in 0..w {
                out.extend_from_slice(&[255, 255, 255, if solid(x, y) { 255 } else { 0 }]);
            }
        }
        out
    }

    fn frame(w: u32, h: u32) -> TexelFrame {
        let sprite = SpriteComponent { size: Some([w as f32, h as f32]), ..Default::default() };
        TexelFrame::of(&sprite, w, h)
    }

    #[test]
    fn a_square_traces_to_its_four_corners() {
        let rgba = image(16, 16, |x, y| (4..12).contains(&x) && (2..10).contains(&y));
        let loops = trace_alpha(&rgba, 16, 16, &frame(16, 16), 127, 1.0);
        assert_eq!(loops.len(), 1);
        let mut pts = loops[0].clone();
        pts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        // Centred sprite, y up: texels 4..12 across, 2..10 down.
        assert_eq!(pts, vec![[-4.0, -2.0], [-4.0, 6.0], [4.0, -2.0], [4.0, 6.0]]);
    }

    /// Each trace chains edges out of a freshly seeded `HashMap`; the answer
    /// must not depend on where that starts. It did: one run in a few lost a
    /// square's corner to the simplifier.
    #[test]
    fn the_same_pixels_trace_the_same_outline_every_time() {
        let rgba = image(24, 24, |x, y| (3..20).contains(&x) && (5..17).contains(&y) || (x == 21 && y == 4));
        let first = trace_alpha(&rgba, 24, 24, &frame(24, 24), 127, 1.0);
        for _ in 0..50 {
            assert_eq!(trace_alpha(&rgba, 24, 24, &frame(24, 24), 127, 1.0), first);
        }
    }

    #[test]
    fn islands_are_loops_and_holes_are_not() {
        // A ring (a square with a hole) and a separate dot.
        let rgba = image(32, 16, |x, y| {
            let ring = (2..14).contains(&x) && (2..14).contains(&y) && !((5..11).contains(&x) && (5..11).contains(&y));
            ring || ((20..26).contains(&x) && (4..10).contains(&y))
        });
        let loops = trace_alpha(&rgba, 32, 16, &frame(32, 16), 127, 1.0);
        assert_eq!(loops.len(), 2, "the ring's outside and the dot: {loops:?}");
        assert!(loops.iter().all(|l| l.len() == 4));
    }

    #[test]
    fn a_curve_is_simplified_within_its_tolerance() {
        let rgba = image(64, 64, |x, y| {
            let (dx, dy) = (x as f32 + 0.5 - 32.0, y as f32 + 0.5 - 32.0);
            dx * dx + dy * dy < 26.0 * 26.0
        });
        let loops = trace_alpha(&rgba, 64, 64, &frame(64, 64), 127, 1.5);
        assert_eq!(loops.len(), 1);
        let n = loops[0].len();
        assert!((8..=MAX_LOOP_POINTS).contains(&n), "a circle in a handful of corners, not hundreds: {n}");
        for p in &loops[0] {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            assert!((r - 26.0).abs() < 3.0, "on the circle: {r}");
        }
    }

    #[test]
    fn a_region_of_a_sheet_traces_only_itself() {
        // Two frames side by side; the sprite shows the right one.
        let rgba = image(32, 16, |x, y| x < 16 || ((20..28).contains(&x) && (4..12).contains(&y)));
        let sprite = SpriteComponent { size: Some([16.0, 16.0]), uv_rect: Some([0.5, 0.0, 1.0, 1.0]), ..Default::default() };
        let loops = trace_alpha(&rgba, 32, 16, &TexelFrame::of(&sprite, 32, 16), 127, 1.0);
        assert_eq!(loops.len(), 1);
        let xs: Vec<f32> = loops[0].iter().map(|p| p[0]).collect();
        assert!(xs.iter().all(|x| (-4.0..=4.0).contains(x)), "only the right frame's square: {loops:?}");
    }
}
