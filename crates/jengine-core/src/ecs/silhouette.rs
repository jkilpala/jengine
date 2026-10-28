//! **Silhouettes** (Oct 2026): a thing hidden behind a wall drawn over the
//! wall as a bright outline - the thief you can still follow through the
//! manor.
//!
//! A [`Silhouette`] on an entity with a sprite, and a [`SilhouetteOccluder`]
//! on the sprites that may hide it (walls, roofs, tree crowns). Wherever an
//! occluder sorts above the entity (a higher `z_order`) and their quads
//! overlap, the entity's sprite is drawn again **over that occluder** - one
//! z above it, unlit - as `color`, outlined `outline` texels wide and filled
//! at `fill`, and clipped to the occluder's own opaque pixels (a texture
//! mask of its sprite, `renderer::mask`). So the outline shows exactly where
//! the wall covers the thief and nowhere else, whatever shape the wall is.
//!
//! The style rides in the default sprite shader's `params[2]` / `params[3]`
//! (zero for every other sprite, so nothing else moved); [`hidden_by`] is
//! which occluder hides which entity, decided on the CPU from the sprites'
//! quads. Collected by `render_world_items`, so no host was wired.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{SpriteComponent, Transform, WorldTransform};

/// Drawn as an outline over whatever `silhouette_occluder` hides it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Silhouette {
    /// Linear colour, alpha included.
    pub color: [f32; 4],
    /// Outline width, in the sprite's texels (0: none).
    pub outline: f32,
    /// How opaque the shape is filled inside the outline.
    pub fill: f32,
}

impl Default for Silhouette {
    fn default() -> Self {
        Self { color: [0.4, 0.9, 1.0, 1.0], outline: 1.5, fill: 0.25 }
    }
}

/// A sprite that may hide a silhouette.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SilhouetteOccluder {
    /// Where its alpha counts as covering (0.5 by default).
    pub alpha_cutoff: f32,
}

impl Default for SilhouetteOccluder {
    fn default() -> Self {
        Self { alpha_cutoff: 0.5 }
    }
}

/// The silhouette's style as the default sprite shader's `params[2]` and
/// `params[3]`, in the bits a draw item carries.
pub fn params(s: &Silhouette) -> [u32; 16] {
    super::sprite_quad::silhouette_params(s.color, s.outline, s.fill)
}

pub use super::sprite_quad::corners as quad;


/// The mask an occluder clips a silhouette to: its sprite's alpha over its
/// quad, placed at the quad's centre.
pub fn mask_of(sprite: &SpriteComponent, wt: &WorldTransform, occluder: &SilhouetteOccluder, default_size: Vec2, texture: &str)
-> (crate::renderer::mask::MaskShape, WorldTransform) {
    let size = sprite.drawn_size(default_size);
    let (ax, ay) = (sprite.anchor[0], sprite.anchor[1]);
    let mut at = wt.clone();
    at.position = wt.apply(Vec2::new((0.5 - ax) * size.x, (0.5 - ay) * size.y));
    let shape = crate::renderer::mask::MaskShape::Texture {
        texture_id: texture.to_string(),
        size: [size.x, size.y],
        uv_rect: sprite.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]),
        alpha_cutoff: occluder.alpha_cutoff.clamp(0.001, 1.0),
    };
    (shape, at)
}

/// Which occluder hides which silhouette: every pair where the occluder
/// sorts above (a higher `z_order`) and the two quads' boxes overlap.
pub fn hidden_by(world: &World, default_size: Vec2) -> Vec<(Entity, Entity)> {
    let placed = |t: &Transform, w: Option<&WorldTransform>| w.cloned().unwrap_or_else(|| WorldTransform::from(t));
    let walls: Vec<(Entity, i32, Vec2, Vec2)> = world
        .query::<(&Transform, &SpriteComponent, Option<&WorldTransform>, &SilhouetteOccluder)>()
        .iter()
        .filter(|(_, (_, s, ..))| s.color[3] > 0.0)
        .map(|(e, (t, s, w, _))| {
            let (_, lo, hi) = quad(s, &placed(t, w), default_size);
            (e, s.z_order, lo, hi)
        })
        .collect();
    if walls.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (e, (t, s, w, _)) in world.query::<(&Transform, &SpriteComponent, Option<&WorldTransform>, &Silhouette)>().iter() {
        if s.color[3] <= 0.0 {
            continue;
        }
        let (_, lo, hi) = quad(s, &placed(t, w), default_size);
        for &(wall, z, wlo, whi) in &walls {
            if wall != e && z > s.z_order && lo.x < whi.x && hi.x > wlo.x && lo.y < whi.y && hi.y > wlo.y {
                out.push((e, wall));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> Transform {
        Transform { position: Vec2::new(x, y), ..Default::default() }
    }

    fn sprite(z: i32, w: f32, h: f32) -> SpriteComponent {
        SpriteComponent { size: Some([w, h]), z_order: z, ..Default::default() }
    }

    #[test]
    fn only_an_overlapping_occluder_above_hides() {
        let mut world = World::new();
        let thief = world.spawn((at(0.0, 0.0), sprite(5, 20.0, 40.0), Silhouette::default()));
        let wall = world.spawn((at(10.0, 0.0), sprite(10, 30.0, 100.0), SilhouetteOccluder::default()));
        let _floor = world.spawn((at(0.0, 0.0), sprite(0, 500.0, 500.0), SilhouetteOccluder::default()));
        let _far = world.spawn((at(200.0, 0.0), sprite(10, 30.0, 100.0), SilhouetteOccluder::default()));
        let pairs = hidden_by(&world, Vec2::splat(64.0));
        assert_eq!(pairs, vec![(thief, wall)], "below it or beside it hides nothing");
    }

    #[test]
    fn the_style_rides_in_params_two_and_three() {
        let p = params(&Silhouette { color: [1.0, 0.5, 0.25, 0.75], outline: 2.0, fill: 0.3 });
        let f: Vec<f32> = p.iter().map(|b| f32::from_bits(*b)).collect();
        assert_eq!(&f[..8], &[0.0; 8], "params 0 and 1 are the text and canvas group's");
        assert_eq!(&f[8..], &[1.0, 2.0, 0.3, 0.0, 1.0, 0.5, 0.25, 0.75]);
    }

    #[test]
    fn the_mask_sits_on_the_occluders_quad() {
        let s = SpriteComponent { size: Some([40.0, 80.0]), anchor: [0.5, 0.0], ..Default::default() };
        let wt = WorldTransform::from(&at(100.0, 50.0));
        let (shape, at) = mask_of(&s, &wt, &SilhouetteOccluder::default(), Vec2::splat(64.0), "wall.png");
        assert_eq!(at.position, Vec2::new(100.0, 90.0), "the quad's centre, anchored at its foot");
        match shape {
            crate::renderer::mask::MaskShape::Texture { texture_id, size, alpha_cutoff, .. } => {
                assert_eq!((texture_id.as_str(), size, alpha_cutoff), ("wall.png", [40.0, 80.0], 0.5));
            }
            other => panic!("{other:?}"),
        }
    }
}
