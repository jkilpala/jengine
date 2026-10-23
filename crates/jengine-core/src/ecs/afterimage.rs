//! **Afterimages** (Oct 2026): fading copies of a sprite left where it has
//! been - a ninja's dash, a speedster's blur, a ghost's trailing poses.
//!
//! An [`Afterimage`] on an entity with a sprite leaves an echo every
//! `interval` seconds while `emitting` (and moving faster than `min_speed`):
//! the sprite **as it was drawn then** - its texture, its `uv_rect` (so an
//! animation's pose is kept), its world quad (turn, scale, a flip by a
//! negative scale) - which fades from `color` to `end_color` over `lifetime`.
//! `flat` draws each echo as the sprite's shape in one colour (the default
//! sprite shader's silhouette fill, `params[2]` / `[3]`) rather than the
//! tinted picture. The echoes are the component's own (`echoes`, never
//! saved), so a trail is gone with its entity.
//!
//! Recorded in `tick_motion` from the entity's world transform - the pose
//! the last frame drew, which is where an echo belongs - and drawn by
//! `render_world_items` as `Skinned` quads at the sprite's z, under it, so
//! no host was wired.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::asset::TextureId;
use crate::ecs::components::{SpriteComponent, Transform, WorldTransform};
use crate::renderer::material::BlendMode;

/// One left-behind pose.
#[derive(Debug, Clone)]
pub struct Echo {
    pub texture: TextureId,
    pub uv: [f32; 4],
    /// Bottom left, bottom right, top right, top left, in the world.
    pub corners: [Vec2; 4],
    pub age: f32,
}

/// Leaves fading copies of its sprite behind it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Afterimage {
    /// Whether it is leaving echoes now (those left fade either way).
    pub emitting: bool,
    /// Seconds between echoes.
    pub interval: f32,
    /// Seconds an echo lasts.
    pub lifetime: f32,
    /// An echo's colour when it is left (linear, alpha included).
    pub color: [f32; 4],
    /// Its colour as it vanishes.
    pub end_color: [f32; 4],
    /// Only leave echoes while moving at least this fast (world units a
    /// second); 0 leaves them standing still too.
    pub min_speed: f32,
    /// At most this many at once: the oldest goes first.
    pub max_echoes: usize,
    /// Draw each echo as the sprite's shape in its colour, not the tinted
    /// picture.
    pub flat: bool,
    /// How the echoes blend: `Alpha`, or `Additive` for a glowing streak.
    pub blend_mode: BlendMode,
    #[serde(skip)]
    pub echoes: Vec<Echo>,
    #[serde(skip)]
    pub clock: f32,
    #[serde(skip)]
    pub last: Option<Vec2>,
}

impl Default for Afterimage {
    fn default() -> Self {
        Self {
            emitting: true,
            interval: 0.04,
            lifetime: 0.3,
            color: [0.45, 0.8, 1.0, 0.7],
            end_color: [0.25, 0.35, 1.0, 0.0],
            min_speed: 0.0,
            max_echoes: 24,
            flat: false,
            blend_mode: BlendMode::Alpha,
            echoes: Vec::new(),
            clock: 0.0,
            last: None,
        }
    }
}

/// The pose `sprite` has at `wt`, as an echo.
fn echo_of(sprite: &SpriteComponent, wt: &WorldTransform) -> Echo {
    let (corners, _, _) = crate::ecs::silhouette::quad(sprite, wt, Vec2::from(SpriteComponent::FALLBACK_SIZE));
    Echo { texture: sprite.texture_id, uv: sprite.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]), corners, age: 0.0 }
}

fn push(a: &mut Afterimage, echo: Echo) {
    let max = a.max_echoes.max(1);
    while a.echoes.len() >= max {
        a.echoes.remove(0);
    }
    a.echoes.push(echo);
}

/// Age the echoes and leave new ones, on the entity's own clock
/// (`time_scale`). Called from `tick_motion`.
pub fn tick_afterimages(world: &mut World, dt: f32) {
    for (_, (a, s, t, w, ts)) in world.query_mut::<(
        &mut Afterimage,
        &SpriteComponent,
        &Transform,
        Option<&WorldTransform>,
        Option<&crate::ecs::time_scale::TimeScale>,
    )>() {
        let dt = dt.max(0.0) * crate::ecs::time_scale::factor(ts);
        let life = a.lifetime.max(1e-3);
        for echo in &mut a.echoes {
            echo.age += dt;
        }
        a.echoes.retain(|e| e.age < life);
        let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        let speed = match a.last {
            Some(p) if dt > 0.0 => wt.position.distance(p) / dt,
            _ => 0.0,
        };
        a.last = Some(wt.position);
        let moving = a.min_speed <= 0.0 || speed >= a.min_speed;
        if !a.emitting || !moving {
            // Ready to leave one the moment it sets off again.
            a.clock = a.interval;
            continue;
        }
        a.clock += dt;
        if a.clock >= a.interval.max(1e-3) {
            a.clock = (a.clock - a.interval.max(1e-3)).min(a.interval.max(1e-3));
            push(a, echo_of(s, &wt));
        }
    }
}

/// An echo's colour at its age: `color` easing to `end_color`.
pub fn color_at(a: &Afterimage, age: f32) -> [f32; 4] {
    let k = (age / a.lifetime.max(1e-3)).clamp(0.0, 1.0);
    let mut c = [0.0; 4];
    for (i, v) in c.iter_mut().enumerate() {
        *v = a.color[i] + (a.end_color[i] - a.color[i]) * k;
    }
    c
}

/// The default sprite shader's silhouette fill in `color` - for `flat`
/// echoes, whose fade then rides on each vertex's alpha.
pub fn flat_params(a: &Afterimage) -> [u32; 16] {
    let style = crate::ecs::silhouette::Silhouette { color: [a.color[0], a.color[1], a.color[2], 1.0], outline: 0.0, fill: 1.0 };
    crate::ecs::silhouette::params(&style)
}

/// A run of echoes of one texture: world positions, `(u, v, q)`, colours
/// and indices, as a `Skinned` item takes them.
#[derive(Debug, Default)]
pub struct EchoRun {
    pub texture: TextureId,
    pub positions: Vec<Vec2>,
    pub uvq: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// The echoes as runs of one texture each, oldest first.
pub fn runs(a: &Afterimage) -> Vec<EchoRun> {
    let mut out: Vec<EchoRun> = Vec::new();
    for echo in &a.echoes {
        if out.last().is_none_or(|r| r.texture != echo.texture) {
            out.push(EchoRun { texture: echo.texture, ..Default::default() });
        }
        let tris = out.last_mut().expect("just pushed");
        let base = tris.positions.len() as u32;
        let [u0, v0, u1, v1] = echo.uv;
        tris.positions.extend_from_slice(&echo.corners);
        tris.uvq.extend_from_slice(&[[u0, v1, 1.0], [u1, v1, 1.0], [u1, v0, 1.0], [u0, v0, 1.0]]);
        tris.colors.extend_from_slice(&[color_at(a, echo.age); 4]);
        tris.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    out
}

/// `Afterimage.set(e, field, value)`: any field by name, through serde.
/// Switching `emitting` on leaves an echo at once.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut a = world.get::<&mut Afterimage>(e).map_err(|_| "no afterimage on that entity".to_string())?;
    let mut v = serde_json::to_value(&*a).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("afterimage has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    let mut next: Afterimage = serde_json::from_value(v).map_err(|e| format!("afterimage.{field}: {e}"))?;
    next.echoes = std::mem::take(&mut a.echoes);
    next.last = a.last;
    next.clock = if next.emitting && !a.emitting { next.interval } else { a.clock };
    *a = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runner(world: &mut World, x: f32) -> Entity {
        world.spawn((
            Transform { position: Vec2::new(x, 0.0), ..Default::default() },
            SpriteComponent { texture_id: "ninja.png".into(), size: Some([20.0, 40.0]), uv_rect: Some([0.0, 0.0, 0.25, 1.0]), ..Default::default() },
            Afterimage { interval: 0.05, lifetime: 0.2, ..Default::default() },
        ))
    }

    fn step(world: &mut World, e: Entity, dx: f32, dt: f32) {
        world.get::<&mut Transform>(e).unwrap().position.x += dx;
        tick_afterimages(world, dt);
    }

    #[test]
    fn echoes_are_left_where_it_was_and_fade_away() {
        let mut world = World::new();
        let e = runner(&mut world, 0.0);
        for _ in 0..10 {
            step(&mut world, e, 10.0, 0.025);
        }
        let a = (*world.get::<&Afterimage>(e).unwrap()).clone();
        // Every other frame for a quarter second, the oldest gone after 0.2.
        assert!((3..=5).contains(&a.echoes.len()), "{}", a.echoes.len());
        let xs: Vec<f32> = a.echoes.iter().map(|e| (e.corners[0].x + e.corners[1].x) / 2.0).collect();
        assert!(xs.windows(2).all(|w| w[1] > w[0]), "oldest first, along the way it went: {xs:?}");
        assert!(xs.iter().all(|&x| x <= 100.0), "never ahead of it: {xs:?}");
        assert_eq!(a.echoes[0].uv, [0.0, 0.0, 0.25, 1.0], "the pose it had");
        let (old, new) = (color_at(&a, a.echoes[0].age), color_at(&a, a.echoes.last().unwrap().age));
        assert!(old[3] < new[3], "older echoes are fainter: {old:?} vs {new:?}");
        // Stopped emitting, they all fade out.
        set_field(&mut world, e, "emitting", &serde_json::json!(false)).unwrap();
        for _ in 0..10 {
            step(&mut world, e, 10.0, 0.025);
        }
        assert!(world.get::<&Afterimage>(e).unwrap().echoes.is_empty());
    }

    #[test]
    fn standing_still_leaves_none_under_a_minimum_speed() {
        let mut world = World::new();
        let e = runner(&mut world, 0.0);
        set_field(&mut world, e, "min_speed", &serde_json::json!(100.0)).unwrap();
        for _ in 0..10 {
            step(&mut world, e, 0.0, 0.025);
        }
        assert!(world.get::<&Afterimage>(e).unwrap().echoes.is_empty());
        // Setting off, one is left at once.
        step(&mut world, e, 10.0, 0.025);
        assert_eq!(world.get::<&Afterimage>(e).unwrap().echoes.len(), 1);
    }

    #[test]
    fn a_turned_flipped_sprite_is_echoed_as_drawn() {
        let mut world = World::new();
        let e = runner(&mut world, 0.0);
        {
            let mut t = world.get::<&mut Transform>(e).unwrap();
            t.scale = Vec2::new(-1.0, 1.0);
        }
        for _ in 0..2 {
            step(&mut world, e, 0.0, 0.05);
        }
        let a = (*world.get::<&Afterimage>(e).unwrap()).clone();
        let c = a.echoes[0].corners;
        assert!(c[0].x > c[1].x, "mirrored, the quad's left corner is on the right: {c:?}");
        let runs = runs(&a);
        assert_eq!(runs.len(), 1, "one texture, one run");
        assert_eq!(runs[0].indices.len(), 6 * a.echoes.len());
    }

    #[test]
    fn flat_echoes_use_the_silhouette_fill() {
        let a = Afterimage { flat: true, color: [1.0, 0.0, 0.5, 0.6], ..Default::default() };
        let p = flat_params(&a);
        assert_eq!(f32::from_bits(p[8]), 1.0, "on");
        assert_eq!(f32::from_bits(p[10]), 1.0, "filled");
        assert_eq!((f32::from_bits(p[12]), f32::from_bits(p[14])), (1.0, 0.5));
        assert_eq!(f32::from_bits(p[15]), 1.0, "alpha rides on the vertices");
    }
}
