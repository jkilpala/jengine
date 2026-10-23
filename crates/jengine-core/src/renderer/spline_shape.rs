//! **Spline shapes** (Oct 2026): terrain drawn from a curve, as Unity's
//! SpriteShape draws it - a handful of control points become a filled
//! polygon tiled with one texture and a strip along its edge tiled with
//! another (grass along the top of a hill, the dirt under it), and with
//! `collider` on, a solid body of the same shape.
//!
//! Everything here is geometry, worked out from the points and kept until
//! they change ([`SplineShape::mesh`]): the curve through the points
//! ([`sample_curve`], Catmull-Rom), the fill polygon ([`fill_polygon`]) cut
//! into triangles ([`triangulate`]) and those clipped to the fill's tile grid
//! ([`tile_triangles`]), and the edge strip split wherever its texture
//! repeats ([`edge_strip`]). The tiling is in the geometry because every
//! sampler in the engine clamps - a UV past 1 would smear the texture's edge
//! rather than repeat it - which is also how a tiled sprite fill draws.
//!
//! The renderer draws the fill and then the edge in the sprite pass at the
//! shape's `z_order`, lit like a sprite; the physics sync builds the collider
//! from the fill's triangles, and so does the browsers' ray snapshot.

use std::sync::{Arc, Mutex};

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// The geometry of a [`SplineShape`], in its entity's local space.
#[derive(Debug, Default)]
pub struct SplineMesh {
    /// The curve through the control points.
    pub curve: Vec<Vec2>,
    /// The fill's outline, as [`triangulate`]d.
    pub polygon: Vec<Vec2>,
    /// The fill's triangles, untiled: what the collider is made of.
    pub triangles: Vec<[usize; 3]>,
    /// The fill, tiled: positions and UVs, three to a triangle.
    pub fill: Vec<(Vec2, [f32; 2])>,
    /// The edge strip: positions and UVs, three to a triangle.
    pub edge: Vec<(Vec2, [f32; 2])>,
}

/// What a shape's mesh was built from, and the mesh. Not saved, not cloned:
/// a copy builds its own the first time it is drawn.
#[derive(Default)]
pub struct SplineCache(Mutex<Option<(u64, Arc<SplineMesh>)>>);

impl Clone for SplineCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for SplineCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SplineCache")
    }
}

/// The `spline_shape` component.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SplineShape {
    /// Control points, local to the entity, in order.
    pub points: Vec<[f32; 2]>,
    /// A loop (an island, a platform) rather than an open line (a hillside).
    pub closed: bool,
    /// A smooth curve through the points (Catmull-Rom); off, straight lines.
    pub smooth: bool,
    /// Segments between two points of a smooth curve.
    pub detail: u32,
    /// An open line is filled down to this local y (it should be below every
    /// point). A loop fills its inside.
    pub fill_bottom: f32,
    /// Fill texture; empty is the white texture, so `fill_color` alone. A
    /// `fill_color` with no alpha draws no fill (the collider stays).
    pub fill_texture: String,
    /// The fill texture's size in world units: it repeats every this many.
    pub fill_tile: f32,
    pub fill_color: [f32; 4],
    /// Edge texture (empty: no edge), laid along the curve: its u runs along
    /// the curve, v across it, the top row outermost.
    pub edge_texture: String,
    /// How thick the edge strip is.
    pub edge_width: f32,
    /// How long one repeat of the edge texture is along the curve.
    pub edge_tile: f32,
    /// How much of the strip lies outside the fill (0..1): 0.5 centres it on
    /// the curve, 0.3 lets grass overhang a little and sink mostly in.
    pub edge_offset: f32,
    pub edge_color: [f32; 4],
    /// Build a static collider of the fill's shape.
    pub collider: bool,
    pub friction: f32,
    pub z_order: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    #[serde(skip)]
    pub cache: SplineCache,
}

impl Default for SplineShape {
    fn default() -> Self {
        Self {
            points: vec![[-200.0, 0.0], [-80.0, 40.0], [60.0, 10.0], [200.0, 30.0]],
            closed: false,
            smooth: true,
            detail: 12,
            fill_bottom: -200.0,
            fill_texture: String::new(),
            fill_tile: 128.0,
            fill_color: [1.0, 1.0, 1.0, 1.0],
            edge_texture: String::new(),
            edge_width: 32.0,
            edge_tile: 128.0,
            edge_offset: 0.5,
            edge_color: [1.0, 1.0, 1.0, 1.0],
            collider: true,
            friction: 0.8,
            z_order: 0,
            sort_layer: None,
            cache: SplineCache::default(),
        }
    }
}

impl SplineShape {
    /// What the mesh is made from, as one number.
    pub fn key(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| {
            h ^= v;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        for p in &self.points {
            mix(p[0].to_bits() as u64);
            mix(p[1].to_bits() as u64);
        }
        mix(self.closed as u64 | (self.smooth as u64) << 1);
        mix(self.detail as u64);
        for v in [self.fill_bottom, self.fill_tile, self.edge_width, self.edge_tile, self.edge_offset] {
            mix(v.to_bits() as u64);
        }
        mix(self.edge_texture.is_empty() as u64);
        h
    }

    /// The shape's geometry, built the first time it is asked for and again
    /// only when what it is made of changes.
    pub fn mesh(&self) -> Arc<SplineMesh> {
        let key = self.key();
        let mut cache = self.cache.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((k, m)) = cache.as_ref()
            && *k == key
        {
            return Arc::clone(m);
        }
        let m = Arc::new(self.build());
        *cache = Some((key, Arc::clone(&m)));
        m
    }

    fn build(&self) -> SplineMesh {
        let points: Vec<Vec2> = self.points.iter().map(|p| Vec2::from(*p)).collect();
        if points.len() < 2 {
            return SplineMesh::default();
        }
        let curve = sample_curve(&points, self.closed, self.smooth, self.detail.max(1));
        let polygon = fill_polygon(&curve, self.closed, self.fill_bottom);
        let triangles = triangulate(&polygon);
        let fill = tile_triangles(&polygon, &triangles, self.fill_tile);
        let edge = if self.edge_texture.is_empty() || self.edge_width <= 0.0 {
            Vec::new()
        } else {
            // The strip's outer side is the left of the way it runs, which
            // is outside the fill when the fill winds clockwise (y up) - a
            // hill drawn left to right does. Anything drawn the other way
            // round (a loop drawn anticlockwise, a hill right to left) is
            // turned round first, so the grass is on top whichever way the
            // points were placed.
            let flipped: Vec<Vec2>;
            let along = if signed_area(&polygon) > 0.0 {
                flipped = curve.iter().rev().copied().collect();
                &flipped
            } else {
                &curve
            };
            edge_strip(along, self.closed, self.edge_width, self.edge_tile, self.edge_offset)
        };
        SplineMesh { curve, polygon, triangles, fill, edge }
    }
}

/// The curve through `points`: a centripetal Catmull-Rom spline with
/// `detail` segments a span when `smooth`, else the points themselves. A
/// closed curve does not repeat its first point at the end.
pub fn sample_curve(points: &[Vec2], closed: bool, smooth: bool, detail: u32) -> Vec<Vec2> {
    let n = points.len();
    if !smooth || n < 3 {
        return points.to_vec();
    }
    let at = |i: isize| -> Vec2 {
        if closed {
            points[i.rem_euclid(n as isize) as usize]
        } else if i < 0 {
            // Mirror the first span so the curve leaves the end point along it.
            points[0] * 2.0 - points[1]
        } else if i as usize >= n {
            points[n - 1] * 2.0 - points[n - 2]
        } else {
            points[i as usize]
        }
    };
    let spans = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(spans * detail as usize + 1);
    for s in 0..spans as isize {
        let (p0, p1, p2, p3) = (at(s - 1), at(s), at(s + 1), at(s + 2));
        for k in 0..detail {
            out.push(catmull_rom(p0, p1, p2, p3, k as f32 / detail as f32));
        }
    }
    if !closed {
        out.push(points[n - 1]);
    }
    out
}

/// Centripetal Catmull-Rom between `p1` and `p2` at `t` (Barry and
/// Goldman's form): no cusps or loops where points bunch up.
fn catmull_rom(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let knot = |a: Vec2, b: Vec2| a.distance(b).sqrt().max(1e-4);
    let t0 = 0.0;
    let t1 = t0 + knot(p0, p1);
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let u = t1 + (t2 - t1) * t;
    let a1 = p0 * ((t1 - u) / (t1 - t0)) + p1 * ((u - t0) / (t1 - t0));
    let a2 = p1 * ((t2 - u) / (t2 - t1)) + p2 * ((u - t1) / (t2 - t1));
    let a3 = p2 * ((t3 - u) / (t3 - t2)) + p3 * ((u - t2) / (t3 - t2));
    let b1 = a1 * ((t2 - u) / (t2 - t0)) + a2 * ((u - t0) / (t2 - t0));
    let b2 = a2 * ((t3 - u) / (t3 - t1)) + a3 * ((u - t1) / (t3 - t1));
    b1 * ((t2 - u) / (t2 - t1)) + b2 * ((u - t1) / (t2 - t1))
}

/// The outline the fill covers: a closed curve itself, an open one with its
/// ends dropped to `bottom`.
pub fn fill_polygon(curve: &[Vec2], closed: bool, bottom: f32) -> Vec<Vec2> {
    if closed || curve.len() < 2 {
        return curve.to_vec();
    }
    let mut poly = curve.to_vec();
    let (first, last) = (curve[0], curve[curve.len() - 1]);
    poly.push(Vec2::new(last.x, bottom));
    poly.push(Vec2::new(first.x, bottom));
    poly
}

fn signed_area(poly: &[Vec2]) -> f32 {
    let n = poly.len();
    (0..n).map(|i| poly[i].perp_dot(poly[(i + 1) % n])).sum::<f32>() * 0.5
}

/// Cut a simple polygon into triangles (ear clipping), as index triples into
/// `poly`, each anticlockwise whichever way the polygon winds.
pub fn triangulate(poly: &[Vec2]) -> Vec<[usize; 3]> {
    let n = poly.len();
    if n < 3 {
        return Vec::new();
    }
    let ccw = signed_area(poly) > 0.0;
    let mut idx: Vec<usize> = if ccw { (0..n).collect() } else { (0..n).rev().collect() };
    let mut out = Vec::with_capacity(n - 2);
    let cross = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a);
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            if cross(a, b, c) <= 1e-6 {
                continue; // reflex or flat: not an ear
            }
            let inside = idx.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let p = poly[j];
                cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
            });
            if inside {
                continue;
            }
            out.push([ia, ib, ic]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            // Not simple (it crosses itself): drop the flattest vertex and go
            // on, so a bad drawing still draws rather than vanishing.
            idx.remove(0);
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

/// `tris` of `poly` clipped to a grid of `tile`-sized cells, each piece with
/// UVs local to its cell (v down), three vertices to a triangle.
pub fn tile_triangles(poly: &[Vec2], tris: &[[usize; 3]], tile: f32) -> Vec<(Vec2, [f32; 2])> {
    let tile = tile.max(1.0);
    let mut out = Vec::new();
    for t in tris {
        let tri = [poly[t[0]], poly[t[1]], poly[t[2]]];
        let lo = tri[0].min(tri[1]).min(tri[2]);
        let hi = tri[0].max(tri[1]).max(tri[2]);
        let (cx0, cx1) = ((lo.x / tile).floor() as i64, (hi.x / tile).ceil() as i64);
        let (cy0, cy1) = ((lo.y / tile).floor() as i64, (hi.y / tile).ceil() as i64);
        for cy in cy0..cy1 {
            for cx in cx0..cx1 {
                let (x0, y0) = (cx as f32 * tile, cy as f32 * tile);
                let clipped = clip_to_rect(&tri, x0, y0, x0 + tile, y0 + tile);
                if clipped.len() < 3 {
                    continue;
                }
                let uv = |p: Vec2| [(p.x - x0) / tile, 1.0 - (p.y - y0) / tile];
                for k in 1..clipped.len() - 1 {
                    for p in [clipped[0], clipped[k], clipped[k + 1]] {
                        out.push((p, uv(p)));
                    }
                }
            }
        }
    }
    out
}

/// Sutherland-Hodgman: `poly` clipped to the rectangle `x0..x1` x `y0..y1`.
fn clip_to_rect(poly: &[Vec2], x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Vec2> {
    let mut out = poly.to_vec();
    let edges: [(Vec2, f32); 4] =
        [(Vec2::new(1.0, 0.0), -x0), (Vec2::new(-1.0, 0.0), x1), (Vec2::new(0.0, 1.0), -y0), (Vec2::new(0.0, -1.0), y1)];
    for (n, d) in edges {
        if out.is_empty() {
            break;
        }
        let input = std::mem::take(&mut out);
        let inside = |p: Vec2| n.dot(p) + d >= 0.0;
        for i in 0..input.len() {
            let (a, b) = (input[i], input[(i + 1) % input.len()]);
            let (ia, ib) = (inside(a), inside(b));
            if ia {
                out.push(a);
            }
            if ia != ib {
                let (da, db) = (n.dot(a) + d, n.dot(b) + d);
                out.push(a + (b - a) * (da / (da - db)));
            }
        }
    }
    out
}

/// A strip `width` wide along `curve`, `offset` of it on the left of the way
/// the curve runs, its texture repeating every `tile` along it: the strip is
/// cut wherever the repeat restarts, so every piece's u stays in 0..1. Three
/// vertices to a triangle; v is 0 on the outer side.
pub fn edge_strip(curve: &[Vec2], closed: bool, width: f32, tile: f32, offset: f32) -> Vec<(Vec2, [f32; 2])> {
    let mut pts = curve.to_vec();
    if closed && pts.len() > 2 {
        pts.push(pts[0]);
    }
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    let tile = tile.max(1.0);
    // The normal at each point: the average of its two segments' left
    // normals, lengthened at a bend so the strip keeps its width.
    let left = |a: Vec2, b: Vec2| (b - a).perp().normalize_or_zero();
    let normal = |i: usize| -> Vec2 {
        let prev = if i > 0 { Some(left(pts[i - 1], pts[i])) } else if closed { Some(left(pts[n - 2], pts[0])) } else { None };
        let next = if i + 1 < n { Some(left(pts[i], pts[i + 1])) } else if closed { Some(left(pts[0], pts[1])) } else { None };
        match (prev, next) {
            (Some(a), Some(b)) => {
                let m = (a + b).normalize_or_zero();
                let c = m.dot(a).max(0.35);
                m / c
            }
            (Some(a), None) | (None, Some(a)) => a,
            _ => Vec2::Y,
        }
    };
    let (out_w, in_w) = (width * offset.clamp(0.0, 1.0), width * (1.0 - offset.clamp(0.0, 1.0)));
    let mut out = Vec::new();
    let mut s = 0.0f32; // arc length so far
    for i in 0..n - 1 {
        let (a, b) = (pts[i], pts[i + 1]);
        let (na, nb) = (normal(i), normal(i + 1));
        let len = a.distance(b);
        if len <= 1e-5 {
            continue;
        }
        // Cut the segment where the texture repeats.
        let mut cuts = vec![0.0f32];
        let mut next_repeat = ((s / tile).floor() + 1.0) * tile;
        while next_repeat < s + len - 1e-4 {
            cuts.push((next_repeat - s) / len);
            next_repeat += tile;
        }
        cuts.push(1.0);
        for w in cuts.windows(2) {
            let (t0, t1) = (w[0], w[1]);
            let (p0, p1) = (a.lerp(b, t0), a.lerp(b, t1));
            let (n0, n1) = (na.lerp(nb, t0), na.lerp(nb, t1));
            let u0 = (s + t0 * len) / tile;
            let base = u0.floor();
            let (u0, u1) = (u0 - base, ((s + t1 * len) / tile - base).min(1.0));
            let (o0, o1) = (p0 + n0 * out_w, p1 + n1 * out_w);
            let (i0, i1) = (p0 - n0 * in_w, p1 - n1 * in_w);
            out.extend_from_slice(&[(o0, [u0, 0.0]), (i0, [u0, 1.0]), (i1, [u1, 1.0])]);
            out.extend_from_slice(&[(o0, [u0, 0.0]), (i1, [u1, 1.0]), (o1, [u1, 0.0])]);
        }
        s += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area_of(tris: &[(Vec2, [f32; 2])]) -> f32 {
        tris.chunks(3).map(|t| ((t[1].0 - t[0].0).perp_dot(t[2].0 - t[0].0) * 0.5).abs()).sum()
    }

    #[test]
    fn a_smooth_curve_passes_through_its_points() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 50.0), Vec2::new(200.0, 0.0), Vec2::new(300.0, 30.0)];
        let c = sample_curve(&pts, false, true, 8);
        assert_eq!(c.len(), 3 * 8 + 1);
        for (k, p) in pts.iter().enumerate() {
            let i = k * 8;
            assert!(c[i.min(c.len() - 1)].distance(*p) < 1e-3, "point {k} is on the curve");
        }
        assert!(c[4].y > 20.0, "and it bends between them rather than going straight");
    }

    #[test]
    fn a_polygon_triangulates_to_its_own_area_either_way_round() {
        // An L, which has a reflex corner an ear test must refuse.
        let l = vec![
            Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0), Vec2::new(200.0, 100.0),
            Vec2::new(100.0, 100.0), Vec2::new(100.0, 200.0), Vec2::new(0.0, 200.0),
        ];
        for poly in [l.clone(), l.iter().rev().copied().collect::<Vec<_>>()] {
            let tris = triangulate(&poly);
            assert_eq!(tris.len(), 4);
            let area: f32 = tris
                .iter()
                .map(|t| (poly[t[1]] - poly[t[0]]).perp_dot(poly[t[2]] - poly[t[0]]) * 0.5)
                .sum();
            assert!((area - 30000.0).abs() < 1e-2, "anticlockwise triangles covering it exactly: {area}");
        }
    }

    #[test]
    fn tiling_keeps_the_area_and_every_uv_in_its_cell() {
        let poly = vec![Vec2::new(-130.0, -40.0), Vec2::new(250.0, -40.0), Vec2::new(250.0, 170.0), Vec2::new(-130.0, 90.0)];
        let tris = triangulate(&poly);
        let tiled = tile_triangles(&poly, &tris, 64.0);
        let area = (380.0 * 130.0) + 380.0 * 80.0 * 0.5;
        assert!((area_of(&tiled) - area).abs() < 0.5, "{} vs {area}", area_of(&tiled));
        assert!(tiled.iter().all(|(_, uv)| (-1e-4..=1.0001).contains(&uv[0]) && (-1e-4..=1.0001).contains(&uv[1])));
        // The texture lines up across a cell border: a world point's u is
        // its position in the grid wherever it falls.
        for (p, uv) in &tiled {
            let fx = (p.x / 64.0).rem_euclid(1.0);
            assert!((uv[0] - fx).abs() < 1e-3 || (uv[0] - 1.0).abs() < 1e-3 && fx < 1e-3);
        }
    }

    #[test]
    fn the_edge_strip_restarts_its_texture_on_the_repeat() {
        let curve = vec![Vec2::new(0.0, 0.0), Vec2::new(250.0, 0.0)];
        let strip = edge_strip(&curve, false, 20.0, 100.0, 0.25);
        assert_eq!(strip.len(), 3 * 6, "two triangles for each of three pieces (0-100, 100-200, 200-250)");
        assert!(strip.iter().all(|(_, uv)| (0.0..=1.0).contains(&uv[0])));
        let top = strip.iter().map(|(p, _)| p.y).fold(f32::MIN, f32::max);
        let bottom = strip.iter().map(|(p, _)| p.y).fold(f32::MAX, f32::min);
        assert!((top - 5.0).abs() < 1e-4 && (bottom + 15.0).abs() < 1e-4, "a quarter above the line: {top}, {bottom}");
        // The last piece is half a repeat: its far u is 0.5.
        assert!(strip.iter().any(|(p, uv)| (p.x - 250.0).abs() < 1e-3 && (uv[0] - 0.5).abs() < 1e-4));
    }

    #[test]
    fn the_edge_is_outside_whichever_way_the_points_run() {
        let square = [[-100.0, -100.0], [100.0, -100.0], [100.0, 100.0], [-100.0, 100.0]];
        for points in [square.to_vec(), square.iter().rev().copied().collect()] {
            let s = SplineShape {
                points,
                closed: true,
                smooth: false,
                edge_texture: "grass".into(),
                edge_offset: 1.0, // all of it outside
                ..Default::default()
            };
            let m = s.mesh();
            for (p, _) in &m.edge {
                assert!(p.x.abs() >= 99.9 || p.y.abs() >= 99.9, "{p:?} lies inside the loop");
            }
        }
        // And a hill drawn right to left still has its grass on top.
        let s = SplineShape {
            points: vec![[200.0, 0.0], [0.0, 30.0], [-200.0, 0.0]],
            smooth: false,
            edge_texture: "grass".into(),
            edge_offset: 1.0,
            ..Default::default()
        };
        assert!(s.mesh().edge.iter().all(|(p, _)| p.y >= -0.1), "the strip is above the line");
    }

    #[test]
    fn a_shape_builds_once_and_again_only_when_it_changes() {
        let mut s = SplineShape { fill_texture: "dirt".into(), edge_texture: "grass".into(), ..Default::default() };
        let a = s.mesh();
        assert!(Arc::ptr_eq(&a, &s.mesh()), "kept while nothing changes");
        assert!(!a.fill.is_empty() && !a.edge.is_empty() && !a.triangles.is_empty());
        s.points[1][1] += 10.0;
        assert!(!Arc::ptr_eq(&a, &s.mesh()), "built again when a point moves");
        let copy = s.clone();
        assert_eq!(copy.mesh().fill.len(), s.mesh().fill.len(), "a copy builds its own");
    }
}
