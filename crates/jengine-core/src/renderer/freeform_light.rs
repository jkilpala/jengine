//! Freeform lights (Oct 2026; Unity's Freeform Light 2D): light in the shape
//! of a polygon - a window's pool on a floor, a glowing pond, a lit doorway -
//! full inside, fading over `falloff` world units outside its edge.
//!
//! The polygon (concave allowed) is drawn as a mesh into the light buffer,
//! additively, beside the lightmaps (`ecs::wgpu_systems::render_lightmaps_wgpu`,
//! which every host already runs - so no host was wired): its inside
//! ear-clipped at full strength, and outside a ring per [`RINGS`] step -
//! strips along each edge, a fan round each convex corner, the two strips
//! meeting at their mitre at a concave one - each vertex weighted by
//! [`falloff_curve`]. The CPU answer, [`light_at`], is that mesh's own
//! arithmetic: full inside, else the curve interpolated linearly between the
//! rings at the distance to the outline. The distance field's level lines
//! are exactly those strips, fans and mitres, so the two agree but for the
//! chords of a corner's arc.
//!
//! A freeform light casts no shadows, as Unity's does not by default.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// Where the falloff's rings sit, as fractions of `falloff`.
pub const RINGS: [f32; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
/// The widest arc one segment of a corner's fan stands for.
const ARC_STEP: f32 = std::f32::consts::PI / 12.0;

fn default_points() -> Vec<[f32; 2]> {
    vec![[-100.0, -100.0], [100.0, -100.0], [100.0, 100.0], [-100.0, 100.0]]
}
fn default_color() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}
fn default_intensity() -> f32 {
    1.0
}
fn default_falloff() -> f32 {
    60.0
}

/// `freeform_light`: light inside `points` (local, turned and scaled with
/// the entity), fading over `falloff` world units outside them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FreeformLight {
    #[serde(default = "default_points")]
    pub points: Vec<[f32; 2]>,
    /// Linear RGB.
    #[serde(default = "default_color")]
    pub color: [f32; 3],
    #[serde(default = "default_intensity")]
    pub intensity: f32,
    /// How far past the outline the light reaches, in world units.
    #[serde(default = "default_falloff")]
    pub falloff: f32,
}

impl Default for FreeformLight {
    fn default() -> Self {
        Self { points: default_points(), color: default_color(), intensity: default_intensity(), falloff: default_falloff() }
    }
}

/// A freeform light placed in the world: its outline anticlockwise, its
/// colour times its intensity.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedFreeform {
    pub points: Vec<Vec2>,
    pub color: Vec3,
    pub falloff: f32,
}

/// How strong the light is `t` of the way across its falloff: a smooth
/// shoulder, sampled at [`RINGS`].
pub fn falloff_curve(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// The curve as the mesh draws it: linear between the rings.
pub fn ring_weight(t: f32) -> f32 {
    if t <= 0.0 {
        return 1.0;
    }
    if t >= 1.0 {
        return 0.0;
    }
    for w in RINGS.windows(2) {
        if t <= w[1] {
            let k = (t - w[0]) / (w[1] - w[0]);
            return falloff_curve(w[0]) + (falloff_curve(w[1]) - falloff_curve(w[0])) * k;
        }
    }
    0.0
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// Place a light's outline by its entity's transform; `None` for one with
/// fewer than three points or no light.
pub fn place(light: &FreeformLight, position: Vec2, rotation: f32, scale: Vec2) -> Option<PlacedFreeform> {
    if light.points.len() < 3 || light.intensity <= 0.0 || light.color.iter().all(|c| *c <= 0.0) {
        return None;
    }
    let rot = glam::Mat2::from_angle(rotation);
    let mut points: Vec<Vec2> = light.points.iter().map(|p| position + rot * (Vec2::from(*p) * scale)).collect();
    points.dedup_by(|a, b| a.distance_squared(*b) < 1e-8);
    if points.len() > 3 && points[0].distance_squared(points[points.len() - 1]) < 1e-8 {
        points.pop();
    }
    if points.len() < 3 {
        return None;
    }
    let area: f32 = (0..points.len()).map(|i| cross(points[i], points[(i + 1) % points.len()])).sum();
    if area < 0.0 {
        points.reverse();
    }
    Some(PlacedFreeform { points, color: Vec3::from(light.color) * light.intensity, falloff: light.falloff.max(0.0) })
}

fn inside(points: &[Vec2], p: Vec2) -> bool {
    let n = points.len();
    let mut c = false;
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            c = !c;
        }
    }
    c
}

fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    let l = d.length_squared();
    let t = if l > 0.0 { ((p - a).dot(d) / l).clamp(0.0, 1.0) } else { 0.0 };
    (a + d * t).distance(p)
}

/// The light at `p`: full inside the outline, fading over the falloff
/// outside it, as the mesh draws it.
pub fn light_at(p: Vec2, light: &PlacedFreeform) -> Vec3 {
    let pts = &light.points;
    if inside(pts, p) {
        return light.color;
    }
    if light.falloff <= 0.0 {
        return Vec3::ZERO;
    }
    let n = pts.len();
    let d = (0..n).map(|i| segment_distance(p, pts[i], pts[(i + 1) % n])).fold(f32::MAX, f32::min);
    light.color * ring_weight(d / light.falloff)
}

/// Ear-clip a simple anticlockwise polygon into triangles.
pub fn triangulate(points: &[Vec2]) -> Vec<u32> {
    let n = points.len();
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity((n.saturating_sub(2)) * 3);
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (a, b, c) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (pa, pb, pc) = (points[a], points[b], points[c]);
            if cross(pb - pa, pc - pb) <= 1e-9 {
                continue; // reflex or straight: not an ear
            }
            let blocked = idx.iter().any(|&j| {
                j != a && j != b && j != c && {
                    let p = points[j];
                    cross(pb - pa, p - pa) >= 0.0 && cross(pc - pb, p - pb) >= 0.0 && cross(pa - pc, p - pc) >= 0.0
                }
            });
            if blocked {
                continue;
            }
            out.extend_from_slice(&[a as u32, b as u32, c as u32]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            break; // not simple: fall back to a fan for what is left
        }
    }
    for i in 1..idx.len().saturating_sub(1) {
        out.extend_from_slice(&[idx[0] as u32, idx[i] as u32, idx[i + 1] as u32]);
    }
    out
}

/// The mesh the light is drawn as: positions, each vertex's weight (0..1),
/// and triangle indices.
pub fn mesh(light: &PlacedFreeform) -> (Vec<Vec2>, Vec<f32>, Vec<u32>) {
    let pts = &light.points;
    let n = pts.len();
    let mut pos: Vec<Vec2> = pts.clone();
    let mut wt: Vec<f32> = vec![1.0; n];
    let mut ix: Vec<u32> = triangulate(pts);
    if light.falloff <= 0.0 {
        return (pos, wt, ix);
    }
    // Each edge's outward normal (anticlockwise: to the right).
    let normal = |i: usize| {
        let d = (pts[(i + 1) % n] - pts[i]).normalize_or_zero();
        Vec2::new(d.y, -d.x)
    };
    let convex = |i: usize| cross(pts[i] - pts[(i + n - 1) % n], pts[(i + 1) % n] - pts[i]) > 1e-6;
    // Where an edge's strip ends at a vertex, ring `r` out: along the
    // edge's normal at a convex vertex (the corner's fan fills between), at
    // the mitre of the two edges at a concave one.
    let end = |v: usize, edge: usize, r: f32| -> Vec2 {
        if convex(v) {
            pts[v] + normal(edge) * r
        } else {
            let (n0, n1) = (normal((v + n - 1) % n), normal(v));
            let m = n0 + n1;
            let denom = (1.0 + n0.dot(n1)).max(0.05);
            pts[v] + m * (r / denom)
        }
    };
    let push = |p: Vec2, w: f32, pos: &mut Vec<Vec2>, wt: &mut Vec<f32>| -> u32 {
        pos.push(p);
        wt.push(w);
        (pos.len() - 1) as u32
    };
    for k in 0..RINGS.len() - 1 {
        let (r0, r1) = (RINGS[k] * light.falloff, RINGS[k + 1] * light.falloff);
        let (w0, w1) = (falloff_curve(RINGS[k]), falloff_curve(RINGS[k + 1]));
        for e in 0..n {
            let (a, b) = (e, (e + 1) % n);
            let i0 = push(end(a, e, r0), w0, &mut pos, &mut wt);
            let i1 = push(end(b, e, r0), w0, &mut pos, &mut wt);
            let i2 = push(end(b, e, r1), w1, &mut pos, &mut wt);
            let i3 = push(end(a, e, r1), w1, &mut pos, &mut wt);
            ix.extend_from_slice(&[i0, i1, i2, i0, i2, i3]);
        }
        // A fan round each convex corner, from one edge's normal to the next.
        for v in 0..n {
            if !convex(v) {
                continue;
            }
            let (n0, n1) = (normal((v + n - 1) % n), normal(v));
            let a0 = n0.y.atan2(n0.x);
            let mut turn = n1.y.atan2(n1.x) - a0;
            while turn < 0.0 {
                turn += std::f32::consts::TAU;
            }
            let steps = ((turn / ARC_STEP).ceil() as usize).max(1);
            for s in 0..steps {
                let t0 = a0 + turn * s as f32 / steps as f32;
                let t1 = a0 + turn * (s + 1) as f32 / steps as f32;
                let (d0, d1) = (Vec2::new(t0.cos(), t0.sin()), Vec2::new(t1.cos(), t1.sin()));
                let i0 = push(pts[v] + d0 * r0, w0, &mut pos, &mut wt);
                let i1 = push(pts[v] + d0 * r1, w1, &mut pos, &mut wt);
                let i2 = push(pts[v] + d1 * r1, w1, &mut pos, &mut wt);
                let i3 = push(pts[v] + d1 * r0, w0, &mut pos, &mut wt);
                ix.extend_from_slice(&[i0, i1, i2, i0, i2, i3]);
            }
        }
    }
    (pos, wt, ix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> PlacedFreeform {
        place(&FreeformLight { falloff: 30.0, ..Default::default() }, Vec2::ZERO, 0.0, Vec2::ONE).unwrap()
    }

    #[test]
    fn full_inside_fading_outside() {
        let l = square();
        assert_eq!(light_at(Vec2::ZERO, &l), Vec3::ONE);
        assert_eq!(light_at(Vec2::new(131.0, 0.0), &l), Vec3::ZERO);
        let mid = light_at(Vec2::new(115.0, 0.0), &l).x;
        assert!((mid - ring_weight(0.5)).abs() < 1e-5 && mid > 0.3 && mid < 0.7, "{mid}");
    }

    #[test]
    fn a_concave_outline_triangulates_whole() {
        // An L: its area is the sum of its triangles'.
        let pts: Vec<Vec2> = [[0.0, 0.0], [200.0, 0.0], [200.0, 100.0], [100.0, 100.0], [100.0, 200.0], [0.0, 200.0]]
            .iter()
            .map(|p| Vec2::from(*p))
            .collect();
        let tri = triangulate(&pts);
        let area: f32 = tri
            .chunks(3)
            .map(|t| cross(pts[t[1] as usize] - pts[t[0] as usize], pts[t[2] as usize] - pts[t[0] as usize]).abs() * 0.5)
            .sum();
        assert!((area - 30_000.0).abs() < 1e-2, "{area}");
    }
}
