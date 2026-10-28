//! A sprite's quad in the world, and the default sprite shader's silhouette
//! mode - what the optional drawing features (`silhouette`, `afterimage`,
//! `sprite_outline`, `snow`) share, kept in core so each can be left out of
//! a build on its own (`docs/engine-features-plan.md`).

use glam::Vec2;

use crate::ecs::components::{SpriteComponent, WorldTransform};

/// A sprite's quad in the world: its corners, and their box.
pub fn corners(sprite: &SpriteComponent, wt: &WorldTransform, default_size: Vec2) -> ([Vec2; 4], Vec2, Vec2) {
    let size = sprite.drawn_size(default_size);
    let (ax, ay) = (sprite.anchor[0], sprite.anchor[1]);
    let corners = [
        Vec2::new(-ax * size.x, -ay * size.y),
        Vec2::new((1.0 - ax) * size.x, -ay * size.y),
        Vec2::new((1.0 - ax) * size.x, (1.0 - ay) * size.y),
        Vec2::new(-ax * size.x, (1.0 - ay) * size.y),
    ]
    .map(|c| wt.apply(c));
    let (mut lo, mut hi) = (corners[0], corners[0]);
    for c in &corners[1..] {
        lo = lo.min(*c);
        hi = hi.max(*c);
    }
    (corners, lo, hi)
}

/// A sprite's quad grown by `reach` world units on every side, and each
/// corner's texture coordinate carried on past the sprite's
/// `uv` rectangle at the same rate - so the picture lands where it is drawn
/// and the margin reads outside it. Corners bottom-left first,
/// anticlockwise, as `(position, [u, v, 1])`.
pub fn grown(sprite: &SpriteComponent, wt: &WorldTransform, size: Vec2, uv: [f32; 4], reach: f32) -> [(Vec2, [f32; 3]); 4] {
    let (ax, ay) = (sprite.anchor[0], sprite.anchor[1]);
    // The reach in the sprite's own units: the transform scales it up again.
    let scale = Vec2::new(wt.scale.x.abs().max(1e-6), wt.scale.y.abs().max(1e-6));
    let grow = Vec2::splat(reach) / scale;
    let size = size.max(Vec2::splat(1e-6));
    let corner = |fx: f32, fy: f32| {
        // fx, fy: 0 and 1 are the sprite's own edges; the grown corners lie
        // past them.
        let local = Vec2::new((fx - ax) * size.x, (fy - ay) * size.y);
        let u = uv[0] + (uv[2] - uv[0]) * fx;
        // v runs down the texture: the bottom edge is v1.
        let v = uv[3] + (uv[1] - uv[3]) * fy;
        (wt.apply(local), [u, v, 1.0])
    };
    let (lo, hi) = (-grow / size, Vec2::ONE + grow / size);
    [corner(lo.x, lo.y), corner(hi.x, lo.y), corner(hi.x, hi.y), corner(lo.x, hi.y)]
}

/// The default sprite shader's silhouette mode as `params[2]` and
/// `params[3]`, in the bits a draw item carries: drawn in `color`, outlined
/// `outline` texels wide and filled at `fill` (`ecs::silhouette`, and the
/// `flat` echoes of `ecs::afterimage`).
pub fn silhouette_params(color: [f32; 4], outline: f32, fill: f32) -> [u32; 16] {
    let mut p = [0u32; 16];
    let f = [1.0, outline.max(0.0), fill.clamp(0.0, 1.0), 0.0, color[0], color[1], color[2], color[3]];
    for (i, v) in f.iter().enumerate() {
        p[8 + i] = v.to_bits();
    }
    p
}
