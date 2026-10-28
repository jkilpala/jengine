//! **Sprite outlines** (Oct 2026): a glowing ring round any sprite's shape -
//! the unit you have selected, the door you can open, the card you are
//! hovering.
//!
//! A [`SpriteOutline`] on an entity with a sprite draws, just behind it, a
//! copy of its quad grown by `width + glow` on every side through the
//! default sprite shader's **outline mode**: each pixel takes the sprite's
//! alpha grown outward - solid out to `width`, fading to nothing over `glow`
//! past it - so the ring follows the picture's own outline, not its
//! rectangle, and a carved or painted sprite is ringed as it now is.
//! `pulse` breathes it.
//!
//! The style rides in the default shader's `params` (outline mode is
//! `params[2].x == 2`, the region's bounds `params[0]`, which only text
//! reads), so nothing else moved; [`quad`] is the grown quad and [`params`]
//! the style, collected by `render_world_items` - no host was wired. A plain
//! sprite only: a jelly, a tilted one or a nine-sliced or tiled fill draws
//! no outline.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

/// A glowing ring round the sprite's shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpriteOutline {
    /// Off draws nothing (and costs nothing) - selection turns it on.
    pub enabled: bool,
    /// Linear colour, alpha included.
    pub color: [f32; 4],
    /// The solid ring's width, world units.
    pub width: f32,
    /// How far past it the ring fades out, world units.
    pub glow: f32,
    /// Breaths a second (0: steady).
    pub pulse: f32,
    /// How far a breath fades it, 0..1.
    pub pulse_amount: f32,
}

impl Default for SpriteOutline {
    fn default() -> Self {
        Self { enabled: true, color: [1.0, 0.8, 0.25, 1.0], width: 2.0, glow: 6.0, pulse: 0.0, pulse_amount: 0.5 }
    }
}

impl SpriteOutline {
    /// How far the ring reaches past the sprite's shape, world units.
    pub fn reach(&self) -> f32 {
        self.width.max(0.0) + self.glow.max(0.0)
    }

    /// The colour's alpha at `seconds`, after the breath.
    pub fn alpha_at(&self, seconds: f64) -> f32 {
        if self.pulse <= 0.0 {
            return self.color[3];
        }
        let wave = 0.5 - 0.5 * (seconds * self.pulse as f64 * std::f64::consts::TAU).cos() as f32;
        self.color[3] * (1.0 - self.pulse_amount.clamp(0.0, 1.0) * wave)
    }
}


pub use super::sprite_quad::grown as quad;

/// The outline's style as the default sprite shader's parameters, in the
/// bits a draw item carries: `params[0]` the `uv` rectangle (samples past it
/// read clear), `params[1]` `(0, width, glow, 0)` - its `.x` is a canvas
/// group's threshold and must stay 0 - `params[2]` `(2, du, dv, 0)`, the
/// texture coordinate a world unit moves, and `params[3]` the colour.
pub fn params(o: &SpriteOutline, size: Vec2, scale: Vec2, uv: [f32; 4], alpha: f32) -> [u32; 16] {
    let world = (size * Vec2::new(scale.x.abs(), scale.y.abs())).max(Vec2::splat(1e-6));
    let du = (uv[2] - uv[0]).abs() / world.x;
    let dv = (uv[3] - uv[1]).abs() / world.y;
    let (u0, u1) = (uv[0].min(uv[2]), uv[0].max(uv[2]));
    let (v0, v1) = (uv[1].min(uv[3]), uv[1].max(uv[3]));
    let f = [
        u0, v0, u1, v1,
        0.0, o.width.max(0.0), o.glow.max(0.0), 0.0,
        2.0, du, dv, 0.0,
        o.color[0], o.color[1], o.color[2], alpha,
    ];
    f.map(f32::to_bits)
}

/// The ring's strength at a point `d` world units outside the shape (0 or
/// less is on it): what the shader works out from its samples, exactly.
pub fn strength(o: &SpriteOutline, d: f32) -> f32 {
    let (w, g) = (o.width.max(0.0), o.glow.max(0.0));
    if d <= w {
        1.0
    } else if g <= 0.0 {
        0.0
    } else {
        (1.0 - (d - w) / g).clamp(0.0, 1.0)
    }
}

/// `Outline.set(e, field, value)`: any field by name.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut o = world.get::<&mut SpriteOutline>(e).map_err(|_| "no sprite_outline on that entity".to_string())?;
    let mut v = serde_json::to_value(&*o).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("sprite_outline has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    *o = serde_json::from_value(v).map_err(|e| format!("sprite_outline.{field}: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::{SpriteComponent, WorldTransform};

    fn sprite() -> SpriteComponent {
        SpriteComponent { size: Some([40.0, 20.0]), ..Default::default() }
    }

    #[test]
    fn the_quad_grows_by_the_reach_and_carries_the_picture_on() {
        let wt = WorldTransform::default();
        let q = quad(&sprite(), &wt, Vec2::new(40.0, 20.0), [0.0, 0.0, 1.0, 1.0], 5.0);
        assert!((q[0].0 - Vec2::new(-25.0, -15.0)).length() < 1e-4, "{:?}", q[0].0);
        assert!((q[2].0 - Vec2::new(25.0, 15.0)).length() < 1e-4, "{:?}", q[2].0);
        // 5 units is an eighth of the width and a quarter of the height.
        assert!((q[0].1[0] + 0.125).abs() < 1e-5 && (q[0].1[1] - 1.25).abs() < 1e-5, "{:?}", q[0].1);
        assert!((q[2].1[0] - 1.125).abs() < 1e-5 && (q[2].1[1] + 0.25).abs() < 1e-5, "{:?}", q[2].1);
    }

    #[test]
    fn a_scaled_sprite_grows_by_world_units_not_its_own() {
        let wt = WorldTransform { scale: Vec2::splat(2.0), ..Default::default() };
        let q = quad(&sprite(), &wt, Vec2::new(40.0, 20.0), [0.0, 0.0, 1.0, 1.0], 5.0);
        assert!((q[2].0 - Vec2::new(45.0, 25.0)).length() < 1e-4, "{:?}", q[2].0);
        let p = params(&SpriteOutline::default(), Vec2::new(40.0, 20.0), Vec2::splat(2.0), [0.0, 0.0, 1.0, 1.0], 1.0);
        assert!((f32::from_bits(p[9]) - 1.0 / 80.0).abs() < 1e-7, "a world unit's u");
        assert_eq!(f32::from_bits(p[4]), 0.0, "never a canvas group's threshold");
        assert_eq!(f32::from_bits(p[8]), 2.0, "outline mode");
    }

    #[test]
    fn the_ring_is_solid_then_fades() {
        let o = SpriteOutline { width: 2.0, glow: 4.0, ..Default::default() };
        assert_eq!(strength(&o, -1.0), 1.0);
        assert_eq!(strength(&o, 2.0), 1.0);
        assert!((strength(&o, 4.0) - 0.5).abs() < 1e-6);
        assert_eq!(strength(&o, 7.0), 0.0);
        assert_eq!(strength(&SpriteOutline { glow: 0.0, ..o }, 2.5), 0.0, "no glow: a hard edge");
    }

    #[test]
    fn a_pulse_breathes_the_alpha() {
        let o = SpriteOutline { pulse: 1.0, pulse_amount: 0.6, ..Default::default() };
        assert!((o.alpha_at(0.0) - 1.0).abs() < 1e-6, "full at the start");
        assert!((o.alpha_at(0.5) - 0.4).abs() < 1e-5, "faded by its amount at the trough");
        assert_eq!(SpriteOutline::default().alpha_at(0.3), 1.0, "steady");
    }

    #[test]
    fn set_reaches_any_field() {
        let mut w = World::new();
        let e = w.spawn((SpriteOutline::default(),));
        set_field(&mut w, e, "enabled", &serde_json::json!(false)).unwrap();
        set_field(&mut w, e, "color", &serde_json::json!([0.2, 1.0, 0.4, 1.0])).unwrap();
        let o = (*w.get::<&SpriteOutline>(e).unwrap()).clone();
        assert!(!o.enabled && o.color[0] == 0.2);
        assert!(set_field(&mut w, e, "nope", &serde_json::json!(1)).is_err());
    }
}
