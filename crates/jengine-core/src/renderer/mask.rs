//! **Sprite masks** (Sep 2026): a shape that clips other things to its
//! inside, or its outside - a portal showing another place, a window onto a
//! layer behind, a frame a picture stays within.
//!
//! An entity with a `sprite_mask` is the shape (a rectangle, an ellipse or a
//! polygon, placed by its transform); anything drawn by an entity with
//! `masked` naming it - a sprite, mesh, path, world text, or a particle
//! emitter's particles - is clipped to it. The shape rides in the sprite
//! batch's globals ([`MaskUniform`]) and every sprite shader - the default
//! one and every generated graph - keeps a fragment by
//! `sprite_mask.wgsl`'s `mask_coverage`, so the passes need nothing of their
//! own and a host needs no wiring: the sprite collector groups by mask as it
//! does by blend.
//!
//! A mask is a shape - with a soft edge (`feather`) - or, since Oct 2026, a
//! **texture's alpha** ([`MaskShape::Texture`], Unity's `SpriteMask`): any
//! painted silhouette clips, a keyhole or a splash of ink or a torn page. The
//! texture is the second texture every sprite flush binds (group 0, bindings
//! 3 and 4 - a white texel when nothing asks); the collector fetches it
//! through the host's own flush callback, by asking the batch to keep the
//! next texture it is handed (`WgpuSpriteBatch::request_mask_texture`), so a
//! host still needs no wiring. [`coverage`] is the same arithmetic on the
//! CPU, for the tests ([`coverage_sampled`] with a texture).

#[cfg(feature = "gpu-wgpu")]
use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::ecs::components::WorldTransform;

/// At most this many points in a polygon mask.
pub const MAX_MASK_POINTS: usize = 16;

/// The shape of a mask, in its entity's own space (before its transform).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MaskShape {
    /// A `size[0]` x `size[1]` rectangle about the entity's origin.
    Rect { size: [f32; 2] },
    /// An ellipse filling that rectangle.
    Ellipse { size: [f32; 2] },
    /// A closed outline, up to [`MAX_MASK_POINTS`] points; either winding.
    Polygon { points: Vec<[f32; 2]> },
    /// A texture's alpha over a `size[0]` x `size[1]` quad about the
    /// entity's origin, read from `uv_rect` (`[u0, v0, u1, v1]`, the whole
    /// texture by default). Soft where the alpha is soft, unless
    /// `alpha_cutoff` is over 0, which keeps what is above it with a hard
    /// edge - Unity's `alphaCutoff`.
    Texture {
        texture_id: String,
        size: [f32; 2],
        #[serde(default = "whole_texture")]
        uv_rect: [f32; 4],
        #[serde(default)]
        alpha_cutoff: f32,
    },
}

fn whole_texture() -> [f32; 4] {
    [0.0, 0.0, 1.0, 1.0]
}

impl MaskShape {
    /// The texture a [`MaskShape::Texture`] reads.
    pub fn texture_id(&self) -> Option<&str> {
        match self {
            MaskShape::Texture { texture_id, .. } if !texture_id.is_empty() => Some(texture_id),
            _ => None,
        }
    }
}

impl Default for MaskShape {
    fn default() -> Self {
        MaskShape::Ellipse { size: [200.0, 200.0] }
    }
}

/// A mask as the shaders read it, in the sprite batch's globals after the
/// material parameters. Zero is "no mask".
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "gpu-wgpu", derive(Pod, Zeroable))]
pub struct MaskUniform {
    /// World to the shape's own space: `local.x = xf[0].xyz . (x, y, 1)`,
    /// `local.y = xf[1].xyz . (x, y, 1)`.
    pub xf: [[f32; 4]; 2],
    /// `kind` (0 none, 1 rectangle, 2 ellipse, 3 polygon, 4 texture),
    /// `outside` (1 keeps what is outside the shape), the polygon's point
    /// count (a texture's alpha cutoff), the feather.
    pub info: [f32; 4],
    /// A rectangle's, ellipse's or texture's half size in `pts[0].xy` (and a
    /// texture's uv rect in `pts[1]`); a polygon's points two to a `vec4`.
    pub pts: [[f32; 4]; MAX_MASK_POINTS / 2],
}

impl MaskUniform {
    /// No mask: everything is kept.
    pub const NONE: MaskUniform = MaskUniform { xf: [[0.0; 4]; 2], info: [0.0; 4], pts: [[0.0; 4]; MAX_MASK_POINTS / 2] };

    pub fn is_none(&self) -> bool {
        self.info[0] == 0.0
    }
}

/// The uniform for `shape` placed by `at`. `feather` is the soft edge, in the
/// shape's own units; 0 is a hard edge.
pub fn pack(shape: &MaskShape, at: &WorldTransform, outside: bool, feather: f32) -> MaskUniform {
    // The inverse of the transform's linear part (scale, shear, rotation).
    let ex = at.apply_linear(Vec2::X);
    let ey = at.apply_linear(Vec2::Y);
    let det = ex.x * ey.y - ey.x * ex.y;
    let inv = if det.abs() > 1e-8 { 1.0 / det } else { 0.0 };
    // [a b; c d] = M^-1 with M = [ex ey].
    let (a, b, c, d) = (ey.y * inv, -ey.x * inv, -ex.y * inv, ex.x * inv);
    let (px, py) = (at.position.x, at.position.y);
    let xf = [[a, b, -(a * px + b * py), 0.0], [c, d, -(c * px + d * py), 0.0]];
    let mut pts = [[0.0f32; 4]; MAX_MASK_POINTS / 2];
    let (kind, n) = match shape {
        MaskShape::Rect { size } => {
            pts[0] = [size[0].abs() * 0.5, size[1].abs() * 0.5, 0.0, 0.0];
            (1.0, 0)
        }
        MaskShape::Ellipse { size } => {
            pts[0] = [size[0].abs() * 0.5, size[1].abs() * 0.5, 0.0, 0.0];
            (2.0, 0)
        }
        MaskShape::Polygon { points } => {
            let n = points.len().min(MAX_MASK_POINTS);
            for (i, p) in points.iter().take(n).enumerate() {
                pts[i / 2][(i % 2) * 2] = p[0];
                pts[i / 2][(i % 2) * 2 + 1] = p[1];
            }
            if n < 3 { (0.0, 0) } else { (3.0, n) }
        }
        MaskShape::Texture { size, uv_rect, alpha_cutoff, .. } => {
            pts[0] = [size[0].abs() * 0.5, size[1].abs() * 0.5, 0.0, 0.0];
            pts[1] = *uv_rect;
            let info = [4.0, if outside { 1.0 } else { 0.0 }, alpha_cutoff.clamp(0.0, 1.0), feather.max(0.0)];
            return MaskUniform { xf, info, pts };
        }
    };
    MaskUniform { xf, info: [kind, if outside { 1.0 } else { 0.0 }, n as f32, feather.max(0.0)], pts }
}

fn point(m: &MaskUniform, i: usize) -> Vec2 {
    let v = m.pts[i / 2];
    if i % 2 == 0 { Vec2::new(v[0], v[1]) } else { Vec2::new(v[2], v[3]) }
}

/// How much of a fragment at `world` the mask keeps, 0..1 - what
/// `sprite_mask.wgsl`'s `mask_coverage` computes, line for line. A texture
/// mask reads as opaque everywhere on its quad here; [`coverage_sampled`]
/// takes the texture.
pub fn coverage(m: &MaskUniform, world: Vec2) -> f32 {
    coverage_sampled(m, world, |_| 1.0)
}

/// [`coverage`] with a texture mask's alpha at a uv (0..1 across the
/// texture, v down) from `alpha`.
pub fn coverage_sampled(m: &MaskUniform, world: Vec2, alpha: impl Fn(Vec2) -> f32) -> f32 {
    let kind = m.info[0] as i32;
    if kind == 0 {
        return 1.0;
    }
    let l = Vec2::new(
        m.xf[0][0] * world.x + m.xf[0][1] * world.y + m.xf[0][2],
        m.xf[1][0] * world.x + m.xf[1][1] * world.y + m.xf[1][2],
    );
    let h = Vec2::new(m.pts[0][0], m.pts[0][1]).max(Vec2::splat(1e-4));
    if kind == 4 {
        let q = l / h * Vec2::new(0.5, -0.5) + Vec2::splat(0.5);
        let r = m.pts[1];
        let uv = Vec2::new(r[0] + (r[2] - r[0]) * q.x, r[1] + (r[3] - r[1]) * q.y);
        let within = l.x.abs() <= h.x && l.y.abs() <= h.y;
        let mut keep = if within { alpha(uv) } else { 0.0 };
        let cutoff = m.info[2];
        if cutoff > 0.0 {
            keep = smoothstep(cutoff - 0.02, cutoff + 0.02, keep);
        }
        return if m.info[1] > 0.5 { 1.0 - keep } else { keep };
    }
    // Signed distance, negative inside.
    let d = match kind {
        1 => {
            let q = l.abs() - h;
            q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0)
        }
        2 => ((l / h).length() - 1.0) * h.x.min(h.y),
        _ => {
            let n = m.info[2] as usize;
            let mut inside = false;
            let mut dist = f32::MAX;
            let mut j = n - 1;
            for i in 0..n {
                let (a, b) = (point(m, i), point(m, j));
                if (a.y > l.y) != (b.y > l.y) && l.x < (b.x - a.x) * (l.y - a.y) / (b.y - a.y) + a.x {
                    inside = !inside;
                }
                let e = b - a;
                let t = ((l - a).dot(e) / e.dot(e).max(1e-8)).clamp(0.0, 1.0);
                dist = dist.min((l - a - e * t).length());
                j = i;
            }
            if inside { -dist } else { dist }
        }
    };
    let feather = m.info[3].max(1e-4);
    let keep = (0.5 - d / feather).clamp(0.0, 1.0);
    if m.info[1] > 0.5 { 1.0 - keep } else { keep }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32, rot: f32) -> WorldTransform {
        WorldTransform { position: Vec2::new(x, y), rotation: rot, scale: Vec2::ONE, shear: 0.0 }
    }

    #[test]
    fn an_ellipse_keeps_its_inside() {
        let m = pack(&MaskShape::Ellipse { size: [200.0, 100.0] }, &at(50.0, 0.0, 0.0), false, 0.0);
        assert_eq!(coverage(&m, Vec2::new(50.0, 0.0)), 1.0);
        assert_eq!(coverage(&m, Vec2::new(140.0, 0.0)), 1.0, "inside along the long axis");
        assert_eq!(coverage(&m, Vec2::new(50.0, 60.0)), 0.0, "outside along the short one");
        let out = pack(&MaskShape::Ellipse { size: [200.0, 100.0] }, &at(50.0, 0.0, 0.0), true, 0.0);
        assert_eq!(coverage(&out, Vec2::new(50.0, 60.0)), 1.0, "an outside mask keeps the rest");
    }

    #[test]
    fn a_turned_rectangle_turns_its_mask() {
        let m = pack(&MaskShape::Rect { size: [100.0, 20.0] }, &at(0.0, 0.0, std::f32::consts::FRAC_PI_2), false, 0.0);
        assert_eq!(coverage(&m, Vec2::new(0.0, 45.0)), 1.0, "the long side now runs up");
        assert_eq!(coverage(&m, Vec2::new(45.0, 0.0)), 0.0);
    }

    #[test]
    fn a_polygon_is_its_outline_and_the_feather_is_soft() {
        // An arch: a square with a peaked top.
        let arch = MaskShape::Polygon { points: vec![[-50.0, -50.0], [50.0, -50.0], [50.0, 20.0], [0.0, 60.0], [-50.0, 20.0]] };
        let m = pack(&arch, &at(0.0, 0.0, 0.0), false, 10.0);
        assert_eq!(coverage(&m, Vec2::new(0.0, 0.0)), 1.0);
        assert_eq!(coverage(&m, Vec2::new(45.0, 50.0)), 0.0, "beside the peak is outside");
        let edge = coverage(&m, Vec2::new(50.0, 0.0));
        assert!((edge - 0.5).abs() < 0.05, "half kept on the edge itself: {edge}");
    }
}
