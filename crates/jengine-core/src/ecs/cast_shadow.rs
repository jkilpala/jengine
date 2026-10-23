//! Shadows laid on the ground (Oct 2026): the lighting here is flat land -
//! every occluder is infinitely tall, its shadow running to the edge of the
//! light - which is right seen from above and wrong seen from the side,
//! where a person's shadow lies on the ground at their feet, long at dawn
//! and dusk, short at noon. A [`CastShadow`] draws that shadow: the
//! sprite's own silhouette, darkened and sheared from its foot along the
//! light, the way side-view and top-down games have always drawn them.
//!
//! The light is the scene's: the brightest `directional_light` (a sun) and
//! the nearest `point_light`s that reach the sprite, up to `lights` of
//! them, so two lamps give two crossed shadows. In a **side** view the
//! sun's own direction says how high it stands (its fall per unit across),
//! and a lamp's height is where it hangs above the foot; in a **top** view
//! the direction lies on the ground and the elevation is the light's
//! `height` (the tangent, as everywhere else in the lighting). Below the
//! horizon a sun casts nothing; a shadow fades with the light's strength
//! and toward its tip.
//!
//! Drawn by `render_world_items` as a textured quad in the sprite pass,
//! just under its sprite, so no host was wired.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{DirectionalLight2D, PointLight2D, Transform, WorldTransform};

/// Which way the scene is seen, which decides how a light lays a shadow.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShadowView {
    /// From the side: the ground is a band receding up the screen.
    #[default]
    Side,
    /// From above: the ground is the screen.
    Top,
}

/// `cast_shadow`: lay this sprite's silhouette on the ground along the light.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CastShadow {
    pub view: ShadowView,
    /// How dark at its foot, 0..1.
    pub opacity: f32,
    pub color: [f32; 3],
    /// How dark at its tip, as a share of the foot's.
    pub fade: f32,
    /// The longest a shadow gets, in heights of the thing (a low sun's runs
    /// to the horizon).
    pub max_length: f32,
    /// Side view: how far up the screen the ground recedes per unit of
    /// shadow - the slant the ground is seen at.
    pub lean: f32,
    /// Side view: the shadow at noon, a footprint this share of the height.
    pub min_length: f32,
    /// Top view: how tall the thing stands, in world units (0: as tall as
    /// its sprite).
    pub height: f32,
    /// How many lights it casts for - the strongest first.
    pub lights: u32,
    /// Only this light (an entity name), if set.
    pub light: String,
    /// Drawn this far under its sprite's z.
    pub z_offset: i32,
}

impl Default for CastShadow {
    fn default() -> Self {
        Self {
            view: ShadowView::Side,
            opacity: 0.5,
            color: [0.0, 0.0, 0.02],
            fade: 0.35,
            max_length: 5.0,
            lean: 0.35,
            min_length: 0.08,
            height: 0.0,
            lights: 1,
            light: String::new(),
            z_offset: -1,
        }
    }
}

/// A light as a shadow sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShadowLight {
    /// The way its light travels (unit), its strength, and (top view) the
    /// tangent of its elevation.
    Sun { dir: Vec2, strength: f32, height: f32 },
    /// Where it is, its strength there, its reach and (top view) the
    /// tangent of its elevation.
    Lamp { at: Vec2, strength: f32, radius: f32, height: f32 },
}

/// The offset from a thing's foot to its shadow's tip, and how strong the
/// shadow is, for a thing `tall` high whose foot is at `foot`. `None` when
/// the light casts none (a sun below the horizon, a lamp out of reach).
pub fn tip(shadow: &CastShadow, light: &ShadowLight, foot: Vec2, tall: f32) -> Option<(Vec2, f32)> {
    let longest = shadow.max_length.max(0.0) * tall;
    match (shadow.view, *light) {
        (ShadowView::Side, ShadowLight::Sun { dir, strength, .. }) => {
            if dir.y >= -1e-4 || strength <= 0.0 {
                return None;
            }
            // A sun falling `-dir.y` for every `dir.x` across throws the
            // top of a thing `tall` high that far along the ground.
            let along = (tall * dir.x / -dir.y).clamp(-longest, longest);
            let up = (along.abs() * shadow.lean).max(shadow.min_length * tall);
            Some((Vec2::new(along, up), strength))
        }
        (ShadowView::Side, ShadowLight::Lamp { at, strength, radius, .. }) => {
            let reach = 1.0 - (foot.distance(at) / radius.max(1e-3));
            let above = at.y - foot.y;
            if reach <= 0.0 || strength <= 0.0 || above <= 0.0 {
                return None;
            }
            // Similar triangles: a lamp `above` the foot, `across` away.
            let across = foot.x - at.x;
            let along = if above > tall { tall * across / (above - tall) } else { longest * across.signum() };
            let along = along.clamp(-longest, longest);
            let up = (along.abs() * shadow.lean).max(shadow.min_length * tall);
            Some((Vec2::new(along, up), strength * reach.min(1.0)))
        }
        (ShadowView::Top, ShadowLight::Sun { dir, strength, height }) => {
            if strength <= 0.0 {
                return None;
            }
            let rise = if shadow.height > 0.0 { shadow.height } else { tall };
            let len = (rise / height.max(1e-3)).min(longest);
            Some((dir.normalize_or_zero() * len, strength))
        }
        (ShadowView::Top, ShadowLight::Lamp { at, strength, radius, height }) => {
            let d = foot - at;
            let reach = 1.0 - d.length() / radius.max(1e-3);
            if reach <= 0.0 || strength <= 0.0 {
                return None;
            }
            // The lamp hangs `height * radius` above the ground, as the
            // lights' highlights take it to.
            let hang = height.max(1e-3) * radius;
            let rise = if shadow.height > 0.0 { shadow.height } else { tall };
            let len = if hang > rise { rise * d.length() / (hang - rise) } else { longest };
            Some((d.normalize_or_zero() * len.min(longest), strength * reach.min(1.0)))
        }
    }
}

/// The lights in `world`, as shadows see them.
pub fn lights(world: &World) -> Vec<(String, ShadowLight)> {
    let mut out = Vec::new();
    let name = |e: Entity| world.get::<&crate::ecs::components::Name>(e).map(|n| n.0.clone()).unwrap_or_default();
    for (e, (l, t, w)) in world.query::<(&DirectionalLight2D, &Transform, Option<&WorldTransform>)>().iter() {
        let turn = w.map_or(t.rotation, |w| w.rotation);
        let a = l.direction + turn;
        let strength = l.intensity * l.color.iter().cloned().fold(0.0, f32::max).min(1.0);
        out.push((name(e), ShadowLight::Sun { dir: Vec2::from_angle(a), strength, height: l.height }));
    }
    for (e, (l, t, w)) in world.query::<(&PointLight2D, &Transform, Option<&WorldTransform>)>().iter() {
        if l.subtract {
            continue;
        }
        let at = w.map_or(t.position, |w| w.position);
        let strength = l.intensity * l.color.iter().cloned().fold(0.0, f32::max).min(1.0);
        out.push((name(e), ShadowLight::Lamp { at, strength, radius: l.radius, height: l.height }));
    }
    out
}

/// One shadow to draw: its four corners (foot left, foot right, tip right,
/// tip left) and its strength.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowQuad {
    pub corners: [Vec2; 4],
    pub strength: f32,
}

/// The shadows a thing whose sprite's foot runs from `foot_l` to `foot_r`
/// and stands `tall` casts, the strongest `shadow.lights` of them.
pub fn quads(shadow: &CastShadow, all: &[(String, ShadowLight)], foot_l: Vec2, foot_r: Vec2, tall: f32) -> Vec<ShadowQuad> {
    let foot = (foot_l + foot_r) * 0.5;
    let mut out: Vec<ShadowQuad> = all
        .iter()
        .filter(|(n, _)| shadow.light.is_empty() || *n == shadow.light)
        .filter_map(|(_, l)| tip(shadow, l, foot, tall))
        .map(|(t, strength)| ShadowQuad { corners: [foot_l, foot_r, foot_r + t, foot_l + t], strength })
        .collect();
    out.sort_by(|a, b| b.strength.total_cmp(&a.strength));
    out.truncate(shadow.lights.max(1) as usize);
    out
}

/// `CastShadow.set(e, field, value)`: any field by name.
pub fn set_shadow_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut s = world.get::<&mut CastShadow>(entity).map_err(|_| "no cast_shadow on that entity".to_string())?;
    let mut whole = serde_json::to_value(&*s).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a cast_shadow field"));
    }
    map.insert(field.to_string(), value.clone());
    *s = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sun(angle: f32) -> ShadowLight {
        ShadowLight::Sun { dir: Vec2::from_angle(angle), strength: 1.0, height: 0.5 }
    }

    #[test]
    fn a_side_sun_lays_shadows_long_low_and_short_high() {
        let s = CastShadow::default();
        // Morning: the light travels west and down at 45 degrees - a shadow as
        // long as the thing is tall, to the west.
        let (t, k) = tip(&s, &sun(-std::f32::consts::FRAC_PI_4 * 3.0), Vec2::ZERO, 100.0).unwrap();
        assert!((t.x + 100.0).abs() < 0.5 && k == 1.0, "{t}");
        assert!((t.y - 35.0).abs() < 0.5, "and lies back into the ground by its lean: {t}");
        // Noon: straight down - a footprint.
        let (t, _) = tip(&s, &sun(-std::f32::consts::FRAC_PI_2), Vec2::ZERO, 100.0).unwrap();
        assert!(t.x.abs() < 0.01 && (t.y - 8.0).abs() < 0.01, "{t}");
        // A sun at the horizon runs to the longest; below it, nothing.
        let (t, _) = tip(&s, &sun(-0.001), Vec2::ZERO, 100.0).unwrap();
        assert!((t.x - 500.0).abs() < 0.01, "{t}");
        assert!(tip(&s, &sun(0.3), Vec2::ZERO, 100.0).is_none());
    }

    #[test]
    fn a_lamp_throws_shadows_away_from_itself() {
        let s = CastShadow::default();
        let lamp = ShadowLight::Lamp { at: Vec2::new(0.0, 300.0), strength: 1.0, radius: 1000.0, height: 0.5 };
        // A thing 100 high, 200 to the right of a lamp 300 up: 100 x 200 / 200.
        let (t, k) = tip(&s, &lamp, Vec2::new(200.0, 0.0), 100.0).unwrap();
        assert!((t.x - 100.0).abs() < 0.01, "{t}");
        assert!(k > 0.6 && k < 0.7, "fainter with distance: {k}");
        let (t, _) = tip(&s, &lamp, Vec2::new(-200.0, 0.0), 100.0).unwrap();
        assert!((t.x + 100.0).abs() < 0.01, "to the left, the other way: {t}");
        assert!(tip(&s, &lamp, Vec2::new(2000.0, 0.0), 100.0).is_none(), "out of reach");
    }

    #[test]
    fn from_above_the_shadow_lies_along_the_light() {
        let s = CastShadow { view: ShadowView::Top, height: 50.0, ..Default::default() };
        let (t, _) = tip(&s, &ShadowLight::Sun { dir: Vec2::X, strength: 1.0, height: 0.5 }, Vec2::ZERO, 80.0).unwrap();
        assert!((t - Vec2::new(100.0, 0.0)).length() < 0.01, "a 50-high thing under a sun at tan 0.5: {t}");
    }

    #[test]
    fn two_lamps_give_two_shadows_strongest_first() {
        let s = CastShadow { lights: 2, ..Default::default() };
        let all = vec![
            ("a".to_string(), ShadowLight::Lamp { at: Vec2::new(-300.0, 300.0), strength: 1.0, radius: 1000.0, height: 0.5 }),
            ("b".to_string(), ShadowLight::Lamp { at: Vec2::new(500.0, 300.0), strength: 1.0, radius: 1000.0, height: 0.5 }),
            ("sun".to_string(), sun(0.5)),
        ];
        let q = quads(&s, &all, Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0), 100.0);
        assert_eq!(q.len(), 2);
        assert!(q[0].strength >= q[1].strength);
        assert!(q[0].corners[2].x > 10.0, "the nearer lamp, on the left, throws it right");
        let only = CastShadow { light: "b".into(), ..Default::default() };
        let q = quads(&only, &all, Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0), 100.0);
        assert_eq!(q.len(), 1);
        assert!(q[0].corners[2].x < -10.0, "only lamp b, on the right");
    }
}
