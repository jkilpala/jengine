//! Rounded rectangles, rounded rings and soft shadows as triangle meshes,
//! for the GUI renderer (Sep 2026). Pure geometry: the renderer hands the
//! result to `WgpuSpriteBatch::draw_mesh`, which draws per-vertex colour -
//! so a shadow's edge fades by its vertices' alpha and needs no blur pass.
//!
//! Everything is laid out as a fan from the centre and as quads between two
//! outlines, in the `a b c, a c d` order `draw_mesh` packs into one quad slot.

use glam::Vec2;

use super::layout::LayoutRect;

/// A triangle mesh in the form `WgpuSpriteBatch::draw_mesh` takes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShapeMesh {
    pub positions: Vec<Vec2>,
    pub uvq: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// Segments per quarter circle: more for a bigger corner.
fn corner_segments(radius: f32) -> usize {
    ((radius / 3.0).ceil() as usize).clamp(2, 10)
}

/// The perimeter of `rect` with corners of `radius`, anticlockwise from the
/// right edge, `segs + 1` points per corner.
fn outline(rect: LayoutRect, radius: f32, segs: usize) -> Vec<Vec2> {
    let r = radius.clamp(0.0, rect.width.min(rect.height) * 0.5);
    let (x0, y0, x1, y1) = (rect.x + r, rect.y + r, rect.x + rect.width - r, rect.y + rect.height - r);
    let corners = [(x1, y1, 0.0f32), (x0, y1, 0.5), (x0, y0, 1.0), (x1, y0, 1.5)];
    let mut pts = Vec::with_capacity(4 * (segs + 1));
    for (cx, cy, start) in corners {
        for i in 0..=segs {
            let a = (start + 0.5 * i as f32 / segs as f32) * std::f32::consts::PI;
            pts.push(Vec2::new(cx + a.cos() * r, cy + a.sin() * r));
        }
    }
    pts
}

fn uv_at(rect: LayoutRect, uv: [f32; 4], p: Vec2) -> [f32; 3] {
    let fx = if rect.width > 0.0 { (p.x - rect.x) / rect.width } else { 0.0 };
    // Rects here are y-up (the renderer flips them); v runs down.
    let fy = if rect.height > 0.0 { (p.y - rect.y) / rect.height } else { 0.0 };
    [uv[0] + (uv[2] - uv[0]) * fx, uv[3] - (uv[3] - uv[1]) * fy, 1.0]
}

impl ShapeMesh {
    /// A filled rounded rectangle, textured by `uv` across `rect`.
    pub fn rounded_rect(rect: LayoutRect, radius: f32, color: [f32; 4], uv: [f32; 4]) -> Self {
        let pts = outline(rect, radius, corner_segments(radius));
        let centre = Vec2::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
        let mut m = ShapeMesh::default();
        m.positions.push(centre);
        m.positions.extend(pts.iter().copied());
        m.uvq = m.positions.iter().map(|p| uv_at(rect, uv, *p)).collect();
        m.colors = vec![color; m.positions.len()];
        let n = pts.len() as u32;
        for i in 0..n {
            m.indices.extend([0, 1 + i, 1 + (i + 1) % n]);
        }
        m
    }

    /// A band `width` wide just inside the edge of a rounded rectangle - its
    /// border.
    pub fn rounded_ring(rect: LayoutRect, radius: f32, width: f32, color: [f32; 4]) -> Self {
        let segs = corner_segments(radius);
        let w = width.clamp(0.0, rect.width.min(rect.height) * 0.5);
        let inner = LayoutRect { x: rect.x + w, y: rect.y + w, width: rect.width - 2.0 * w, height: rect.height - 2.0 * w };
        Self::band(outline(rect, radius, segs), outline(inner, (radius - w).max(0.0), segs), color, color)
    }

    /// A rounded rectangle whose edge fades out over `feather` pixels past
    /// `rect` - a soft shadow.
    pub fn soft_rounded_rect(rect: LayoutRect, radius: f32, feather: f32, color: [f32; 4]) -> Self {
        let mut core = Self::rounded_rect(rect, radius, color, [0.0, 0.0, 1.0, 1.0]);
        if feather <= 0.0 {
            return core;
        }
        let segs = corner_segments(radius);
        let outer = LayoutRect {
            x: rect.x - feather,
            y: rect.y - feather,
            width: rect.width + 2.0 * feather,
            height: rect.height + 2.0 * feather,
        };
        let clear = [color[0], color[1], color[2], 0.0];
        let ring = Self::band(outline(outer, radius + feather, segs), outline(rect, radius, segs), clear, color);
        core.append(ring);
        core
    }

    /// Quads between two outlines of equal length.
    fn band(outer: Vec<Vec2>, inner: Vec<Vec2>, outer_color: [f32; 4], inner_color: [f32; 4]) -> Self {
        let n = outer.len().min(inner.len()) as u32;
        let mut m = ShapeMesh::default();
        m.positions.extend(outer.iter().take(n as usize).copied());
        m.positions.extend(inner.iter().take(n as usize).copied());
        m.uvq = vec![[0.0, 0.0, 1.0]; m.positions.len()];
        m.colors = std::iter::repeat_n(outer_color, n as usize).chain(std::iter::repeat_n(inner_color, n as usize)).collect();
        for i in 0..n {
            let j = (i + 1) % n;
            let (o0, o1, i0, i1) = (i, j, n + i, n + j);
            m.indices.extend([o0, o1, i1, o0, i1, i0]);
        }
        m
    }

    fn append(&mut self, other: ShapeMesh) {
        let base = self.positions.len() as u32;
        self.positions.extend(other.positions);
        self.uvq.extend(other.uvq);
        self.colors.extend(other.colors);
        self.indices.extend(other.indices.into_iter().map(|i| i + base));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> LayoutRect {
        LayoutRect { x: 10.0, y: 20.0, width: 100.0, height: 40.0 }
    }

    fn inside(r: LayoutRect, p: Vec2, slack: f32) -> bool {
        p.x >= r.x - slack && p.x <= r.x + r.width + slack && p.y >= r.y - slack && p.y <= r.y + r.height + slack
    }

    #[test]
    fn a_rounded_rect_stays_in_its_rect_and_cuts_its_corners() {
        let m = ShapeMesh::rounded_rect(rect(), 12.0, [1.0; 4], [0.0, 0.0, 1.0, 1.0]);
        assert!(m.positions.iter().all(|p| inside(rect(), *p, 1e-3)));
        assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
        // No vertex sits at a corner: they are rounded off.
        let corner = Vec2::new(10.0, 20.0);
        assert!(m.positions.iter().all(|p| p.distance(corner) > 3.0));
        // The radius is capped at half the short side.
        let pill = ShapeMesh::rounded_rect(rect(), 500.0, [1.0; 4], [0.0, 0.0, 1.0, 1.0]);
        assert!(pill.positions.iter().all(|p| inside(rect(), *p, 1e-3)));
    }

    #[test]
    fn a_rounded_rect_maps_its_uv_across_the_rect() {
        let m = ShapeMesh::rounded_rect(rect(), 0.0, [1.0; 4], [0.25, 0.0, 0.75, 1.0]);
        for (p, uv) in m.positions.iter().zip(&m.uvq) {
            let fx = (p.x - 10.0) / 100.0;
            assert!((uv[0] - (0.25 + 0.5 * fx)).abs() < 1e-5);
        }
    }

    #[test]
    fn a_soft_shadow_fades_to_clear_at_its_edge() {
        let m = ShapeMesh::soft_rounded_rect(rect(), 8.0, 10.0, [0.0, 0.0, 0.0, 0.5]);
        let grown = LayoutRect { x: 0.0, y: 10.0, width: 120.0, height: 60.0 };
        assert!(m.positions.iter().all(|p| inside(grown, *p, 1e-3)));
        for (p, c) in m.positions.iter().zip(&m.colors) {
            if !inside(rect(), *p, 1e-3) {
                assert_eq!(c[3], 0.0, "outside the rect is the clear edge: {p:?}");
            }
        }
        assert!(m.colors.iter().any(|c| c[3] == 0.5));
    }

    #[test]
    fn a_ring_lies_within_its_width_of_the_edge() {
        let m = ShapeMesh::rounded_ring(rect(), 10.0, 3.0, [1.0; 4]);
        assert!(m.positions.iter().all(|p| inside(rect(), *p, 1e-3)));
        let shrunk = LayoutRect { x: 13.0, y: 23.0, width: 94.0, height: 34.0 };
        // The second half is the inner edge: inside the band's inner rect.
        let half = m.positions.len() / 2;
        assert!(m.positions[half..].iter().all(|p| inside(shrunk, *p, 1e-3)));
        assert!(!m.positions[..half].iter().all(|p| inside(shrunk, *p, 1e-3)), "the outer edge is outside it");
    }
}
