//! Partly drawn sprites (Oct 2026; Unity's `Image.fillAmount`, Godot's
//! `TextureProgressBar`): a [`SpriteFill::Bar`] draws the first `amount` of
//! its sprite from one side, a [`SpriteFill::Radial`] the first `amount` of a
//! turn about its middle - a health bar, a cooldown, and the warning shape
//! that fills before a boss strikes. `Sprite.set_fill(e, amount)` moves it.
//!
//! A bar is one rectangle cropped, texture and all, so it goes through the
//! ordinary sprite path (`for_each_fill_quad`) and every pass that draws
//! sprites - normals, shadows - draws it cropped too. A radial fill is a fan
//! of triangles from the middle to the sprite's edge, which the sprite pass
//! draws as a mesh with the sprite's own texture, colour, blend and mask;
//! the passes that only draw rectangles draw it whole.
//!
//! [`SpriteFill::Bar`]: crate::ecs::components::SpriteFill::Bar
//! [`SpriteFill::Radial`]: crate::ecs::components::SpriteFill::Radial

use glam::Vec2;

use crate::ecs::components::FillFrom;

/// The local rectangle a sprite of `size` with `anchor` covers, `(x0, x1,
/// y0, y1)` with +y up.
pub fn local_rect(size: Vec2, anchor: [f32; 2]) -> (f32, f32, f32, f32) {
    (-anchor[0] * size.x, (1.0 - anchor[0]) * size.x, -anchor[1] * size.y, (1.0 - anchor[1]) * size.y)
}

/// The part of the sprite a bar fill draws: its local rectangle and the
/// matching part of `uv` (`[u0, v0, u1, v1]`, v down).
pub fn bar(size: Vec2, anchor: [f32; 2], uv: [f32; 4], amount: f32, from: FillFrom) -> ((f32, f32, f32, f32), [f32; 4]) {
    let a = amount.clamp(0.0, 1.0);
    let (x0, x1, y0, y1) = local_rect(size, anchor);
    let [u0, v0, u1, v1] = uv;
    match from {
        FillFrom::Left => ((x0, x0 + (x1 - x0) * a, y0, y1), [u0, v0, u0 + (u1 - u0) * a, v1]),
        FillFrom::Right => ((x1 - (x1 - x0) * a, x1, y0, y1), [u1 - (u1 - u0) * a, v0, u1, v1]),
        FillFrom::Bottom => ((x0, x1, y0, y0 + (y1 - y0) * a), [u0, v1 - (v1 - v0) * a, u1, v1]),
        FillFrom::Top => ((x0, x1, y1 - (y1 - y0) * a, y1), [u0, v0, u1, v0 + (v1 - v0) * a]),
    }
}

/// Where a ray from the rectangle's middle at `angle` leaves it.
fn edge_point(rect: (f32, f32, f32, f32), angle: f32) -> Vec2 {
    let (x0, x1, y0, y1) = rect;
    let c = Vec2::new((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let half = Vec2::new((x1 - x0) * 0.5, (y1 - y0) * 0.5);
    let d = Vec2::from_angle(angle);
    let tx = if d.x.abs() > 1e-6 { half.x / d.x.abs() } else { f32::INFINITY };
    let ty = if d.y.abs() > 1e-6 { half.y / d.y.abs() } else { f32::INFINITY };
    c + d * tx.min(ty)
}

/// A radial fill's triangles in the sprite's local space: a fan from the
/// middle through the edge, `amount` of a turn from `start` (radians, 0
/// along +x, `PI / 2` up), clockwise or not. The rectangle's corners inside
/// the sweep are points of the fan, so a full turn is the whole rectangle.
/// Returns local points, their texture coordinates (`[u, v, 1]`) and the
/// triangles; empty below a sliver.
pub fn radial(
    size: Vec2,
    anchor: [f32; 2],
    uv: [f32; 4],
    amount: f32,
    start: f32,
    clockwise: bool,
) -> (Vec<Vec2>, Vec<[f32; 3]>, Vec<u32>) {
    let a = amount.clamp(0.0, 1.0);
    if a <= 1e-4 || size.x <= 0.0 || size.y <= 0.0 {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    let rect = local_rect(size, anchor);
    let (x0, x1, y0, y1) = rect;
    let c = Vec2::new((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let sweep = a * std::f32::consts::TAU;
    let dir = if clockwise { -1.0 } else { 1.0 };
    // The sweep as offsets from the start: steps, the end, and the corners
    // it passes (where the edge bends).
    let mut offsets: Vec<f32> = Vec::new();
    let steps = (sweep / (std::f32::consts::PI / 16.0)).ceil().max(1.0) as usize;
    for i in 0..=steps {
        offsets.push(sweep * i as f32 / steps as f32);
    }
    for corner in [Vec2::new(x1, y1), Vec2::new(x0, y1), Vec2::new(x0, y0), Vec2::new(x1, y0)] {
        let ang = (corner - c).to_angle();
        let off = ((ang - start) * dir).rem_euclid(std::f32::consts::TAU);
        if off > 0.0 && off < sweep {
            offsets.push(off);
        }
    }
    offsets.sort_by(|p, q| p.total_cmp(q));
    offsets.dedup_by(|p, q| (*p - *q).abs() < 1e-5);
    let [u0, v0, u1, v1] = uv;
    let to_uv = |p: Vec2| {
        [u0 + (p.x - x0) / (x1 - x0) * (u1 - u0), v0 + (y1 - p.y) / (y1 - y0) * (v1 - v0), 1.0]
    };
    let mut points = vec![c];
    for off in &offsets {
        points.push(edge_point(rect, start + dir * off));
    }
    let uvq = points.iter().map(|p| to_uv(*p)).collect();
    let mut indices = Vec::new();
    for i in 1..points.len() as u32 - 1 {
        indices.extend_from_slice(&[0, i, i + 1]);
    }
    (points, uvq, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(points: &[Vec2], indices: &[u32]) -> f32 {
        indices
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (points[t[0] as usize], points[t[1] as usize], points[t[2] as usize]);
                ((b - a).perp_dot(c - a) * 0.5).abs()
            })
            .sum()
    }

    #[test]
    fn a_radial_fill_covers_its_share_of_a_square() {
        let size = Vec2::new(100.0, 100.0);
        for amount in [0.125, 0.25, 0.5, 0.8, 1.0] {
            let (p, _, i) = radial(size, [0.5, 0.5], [0.0, 0.0, 1.0, 1.0], amount, std::f32::consts::FRAC_PI_2, true);
            let got = area(&p, &i);
            // From the top, clockwise: equal angles of a square from its
            // middle are not equal areas in general, but they are at
            // multiples of an eighth.
            if (amount * 8.0).fract() == 0.0 {
                assert!((got - 10000.0 * amount).abs() < 1.0, "{amount}: {got}");
            } else {
                assert!(got > 10000.0 * 0.5 && got < 10000.0, "{amount}: {got}");
            }
        }
        let (p, _, _) = radial(size, [0.5, 0.5], [0.0, 0.0, 1.0, 1.0], 0.25, std::f32::consts::FRAC_PI_2, true);
        assert!(p.iter().all(|q| q.x >= -1e-3), "clockwise from the top, a quarter is the top right: {p:?}");
        assert!(radial(size, [0.5, 0.5], [0.0, 0.0, 1.0, 1.0], 0.0, 0.0, true).0.is_empty());
    }

    #[test]
    fn a_bar_crops_its_rectangle_and_its_texture_alike() {
        let (r, uv) = bar(Vec2::new(200.0, 20.0), [0.5, 0.5], [0.0, 0.0, 1.0, 1.0], 0.25, FillFrom::Left);
        assert_eq!(r, (-100.0, -50.0, -10.0, 10.0));
        assert_eq!(uv, [0.0, 0.0, 0.25, 1.0]);
        let (r, uv) = bar(Vec2::new(20.0, 200.0), [0.5, 0.0], [0.0, 0.0, 1.0, 1.0], 0.5, FillFrom::Bottom);
        assert_eq!(r, (-10.0, 10.0, 0.0, 100.0));
        assert_eq!(uv, [0.0, 0.5, 1.0, 1.0], "the bottom half of the picture (v runs down)");
    }
}
