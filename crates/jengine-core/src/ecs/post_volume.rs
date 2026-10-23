//! Post volumes (Oct 2026; Unity's Volume, Godot's `WorldEnvironment` per
//! area): a box whose post-effect values the camera takes on as it goes in.
//!
//! A [`PostVolume`] names effects and the values it wants for them; inside
//! its box it applies them whole, and over `blend` world units outside it
//! they fade in - so walking from a warm hall into a cold chapel turns the
//! grade as you go, with no script. Volumes stack by `priority`, lowest
//! first, each blending over what the ones below (and the scene) made.
//!
//! The scene's own post settings stay where they were: the stack keeps them
//! (and every `PostProcess.set_*` a script makes) and is handed, each frame,
//! a **blended copy** for the main camera (`blended`, called from
//! `frame_draw`), so leaving every volume gives back exactly the scene's
//! values. A number, a vec2, vec3 or vec4 is blended; an int switches at
//! half way. An effect a volume names that the scene does not have is added
//! at the volume's values; one the scene lists disabled is switched on and
//! blended **from the values the scene gave it** - so author a room's
//! effect in the scene, disabled, to have it fade in rather than appear.

use std::collections::HashMap;

use glam::Vec2;
use hecs::World;
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Transform, WorldTransform};
use crate::renderer::material::UniformValue;
use crate::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings};

/// One effect's values in a volume.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct VolumeEffect {
    pub name: String,
    /// Switch the effect on (or off) inside; absent leaves it as it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    pub params: HashMap<String, UniformValue>,
}

/// `post_volume`: a box whose post-effect values the camera takes on.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PostVolume {
    /// The box, width and height, about the entity.
    pub size: [f32; 2],
    /// How far outside the box it fades in, world units (0: a hard edge).
    pub blend: f32,
    /// Its strength at full (0..1).
    pub weight: f32,
    /// Lower volumes blend first; a higher one blends over them.
    pub priority: i32,
    /// The exposure it wants (HDR scenes), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure: Option<f32>,
    pub effects: Vec<VolumeEffect>,
    pub enabled: bool,
}

impl Default for PostVolume {
    fn default() -> Self {
        Self {
            size: [800.0, 600.0],
            blend: 200.0,
            weight: 1.0,
            priority: 0,
            exposure: None,
            effects: Vec::new(),
            enabled: true,
        }
    }
}

impl PostVolume {
    /// How much of the volume applies to a camera at `p`, its box centred on
    /// `centre`: whole inside, fading to nothing `blend` outside.
    pub fn weight_at(&self, centre: Vec2, p: Vec2) -> f32 {
        if !self.enabled || self.weight <= 0.0 {
            return 0.0;
        }
        let half = Vec2::from(self.size) * 0.5;
        let d = ((p - centre).abs() - half).max(Vec2::ZERO).length();
        let k = if d <= 0.0 {
            1.0
        } else if self.blend > 0.0 {
            (1.0 - d / self.blend).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Smoothed, so walking through the fade has no corner in it.
        let k = k * k * (3.0 - 2.0 * k);
        k * self.weight.clamp(0.0, 1.0)
    }
}

fn lerp_value(from: &UniformValue, to: &UniformValue, t: f32) -> UniformValue {
    let l = |a: f32, b: f32| a + (b - a) * t;
    match (from, to) {
        (UniformValue::Float(a), UniformValue::Float(b)) => UniformValue::Float(l(*a, *b)),
        (UniformValue::Vec2(a), UniformValue::Vec2(b)) => UniformValue::Vec2([l(a[0], b[0]), l(a[1], b[1])]),
        (UniformValue::Vec3(a), UniformValue::Vec3(b)) => {
            UniformValue::Vec3([l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2])])
        }
        (UniformValue::Vec4(a), UniformValue::Vec4(b)) => {
            UniformValue::Vec4([l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2]), l(a[3], b[3])])
        }
        _ => if t >= 0.5 { to.clone() } else { from.clone() },
    }
}

/// Blend `volume` over `settings` at weight `w`.
pub fn apply(settings: &mut PostProcessSettings, volume: &PostVolume, w: f32) {
    if w <= 0.0 {
        return;
    }
    if let Some(e) = volume.exposure {
        let from = settings.exposure();
        settings.exposure = Some(from + (e - from) * w);
    }
    for ov in &volume.effects {
        match settings.effects.iter_mut().find(|e| e.name == ov.name) {
            Some(effect) => {
                match ov.enabled {
                    Some(true) => effect.enabled = true,
                    Some(false) if w >= 0.5 => effect.enabled = false,
                    _ => {}
                }
                for (k, v) in &ov.params {
                    let next = match effect.params.get(k) {
                        Some(cur) => lerp_value(cur, v, w),
                        None => v.clone(),
                    };
                    effect.params.insert(k.clone(), next);
                }
            }
            None if ov.enabled != Some(false) => settings.effects.push(PostProcessEffectSettings {
                name: ov.name.clone(),
                enabled: true,
                params: ov.params.clone(),
            }),
            None => {}
        }
    }
}

/// The scene's settings with every volume near `at` blended over them, in
/// priority order - or `None` when no volume reaches `at`, so the stack
/// draws its own settings untouched.
pub fn blended(world: &World, base: &PostProcessSettings, at: Vec2) -> Option<PostProcessSettings> {
    let mut volumes: Vec<(i32, f32, PostVolume)> = world
        .query::<(&PostVolume, &Transform, Option<&WorldTransform>)>()
        .iter()
        .filter_map(|(_, (v, t, wt))| {
            let centre = wt.map_or(t.position, |w| w.position);
            let w = v.weight_at(centre, at);
            (w > 0.0).then(|| (v.priority, w, v.clone()))
        })
        .collect();
    if volumes.is_empty() {
        return None;
    }
    volumes.sort_by_key(|(p, _, _)| *p);
    let mut out = base.clone();
    for (_, w, v) in &volumes {
        apply(&mut out, v, *w);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grade(brightness: f32, tint: [f32; 4], enabled: bool) -> PostProcessEffectSettings {
        let mut params = HashMap::new();
        params.insert("brightness".to_string(), UniformValue::Float(brightness));
        params.insert("tint_color".to_string(), UniformValue::Vec4(tint));
        PostProcessEffectSettings { name: "color_grade".into(), enabled, params }
    }

    fn cold() -> PostVolume {
        let mut params = HashMap::new();
        params.insert("brightness".to_string(), UniformValue::Float(0.6));
        params.insert("tint_color".to_string(), UniformValue::Vec4([0.5, 0.7, 1.0, 0.3]));
        PostVolume {
            size: [200.0, 100.0],
            blend: 100.0,
            effects: vec![VolumeEffect { name: "color_grade".into(), enabled: Some(true), params }],
            ..Default::default()
        }
    }

    fn brightness(s: &PostProcessSettings) -> f32 {
        match s.effects[0].params["brightness"] {
            UniformValue::Float(v) => v,
            _ => panic!(),
        }
    }

    #[test]
    fn a_volume_applies_inside_and_fades_out_over_its_blend() {
        let v = cold();
        assert_eq!(v.weight_at(Vec2::ZERO, Vec2::new(90.0, 40.0)), 1.0, "inside");
        let half = v.weight_at(Vec2::ZERO, Vec2::new(150.0, 0.0));
        assert!((half - 0.5).abs() < 1e-5, "half way through the blend: {half}");
        assert_eq!(v.weight_at(Vec2::ZERO, Vec2::new(201.0, 0.0)), 0.0, "past it");
        assert_eq!(PostVolume { enabled: false, ..cold() }.weight_at(Vec2::ZERO, Vec2::ZERO), 0.0);
    }

    #[test]
    fn values_blend_from_the_scenes_and_leaving_gives_them_back() {
        let mut world = World::new();
        world.spawn((Transform::default(), cold()));
        let base = PostProcessSettings { effects: vec![grade(1.0, [1.0, 1.0, 1.0, 0.0], false)], ..Default::default() };
        assert!(blended(&world, &base, Vec2::new(500.0, 0.0)).is_none(), "out of reach: the scene's own");
        let inside = blended(&world, &base, Vec2::ZERO).unwrap();
        assert!(inside.effects[0].enabled, "the volume switches the scene's disabled grade on");
        assert!((brightness(&inside) - 0.6).abs() < 1e-5);
        let half = blended(&world, &base, Vec2::new(150.0, 0.0)).unwrap();
        assert!((brightness(&half) - 0.8).abs() < 1e-5, "half way: half way between: {}", brightness(&half));
        assert!(!base.effects[0].enabled, "and the scene's settings are untouched");
    }

    #[test]
    fn higher_priority_blends_over_lower() {
        let mut world = World::new();
        world.spawn((Transform::default(), cold()));
        let mut warm = cold();
        warm.priority = 1;
        warm.effects[0].params.insert("brightness".into(), UniformValue::Float(1.4));
        world.spawn((Transform::default(), warm));
        let base = PostProcessSettings { effects: vec![grade(1.0, [1.0; 4], true)], ..Default::default() };
        let out = blended(&world, &base, Vec2::ZERO).unwrap();
        assert!((brightness(&out) - 1.4).abs() < 1e-5, "the higher one wins where both are whole");
    }
}
