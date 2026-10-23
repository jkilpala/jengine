//! Mirrors (Oct 2026): a line that shows what is in front of it, reflected,
//! behind it.
//!
//! A [`Mirror`] lies along its entity's local x, `width` long, and faces its
//! local +y. Each frame the region `depth` deep in front of it is drawn by a
//! view of its own ([`mirror_views`], appended to every host's views by
//! `frame_draw`, as canvas groups are) into a texture, and the sprite pass
//! draws that texture over the region as deep **behind** the line, flipped
//! across it - so a top-down room with a mirror wall seems to go on through
//! the wall, the far side of the room shown in reverse. What it shows was
//! lit and shadowed by the same world pass as everything else.
//!
//! **A mirror sees mirrors.** The reflection quads are drawn in views too,
//! so a mirror whose depth reaches another mirror's reflection shows it -
//! from the frame before when that mirror's view is drawn after this one, so
//! two mirrors facing each other build their corridor of reflections a
//! frame a step. A view reads another's texture in its world pass while that
//! texture is written only by its own post pass. A mirror's own pass leaves
//! its own reflection out (`CameraPass::Mirror`) - it lies behind the line,
//! where the mirror cannot see anyway.
//!
//! **At scale.** A mirror costs a world pass, but over its own small region
//! at `resolution` texels a world unit, and none is drawn whose reflection
//! (or what it reflects) is nowhere near the main camera's view - so a maze
//! of mirrors pays for the ones on screen. Like a canvas group's picture,
//! a reflection has no post chain of its own (the screen's runs over it).

use std::cell::RefCell;
use std::collections::HashSet;

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Transform, WorldTransform};
use crate::renderer::camera::{Camera2D, CameraPass};

fn default_width() -> f32 {
    200.0
}
fn default_depth() -> f32 {
    200.0
}
fn default_resolution() -> f32 {
    1.0
}
fn default_tint() -> [f32; 4] {
    [1.0; 4]
}
fn default_enabled() -> bool {
    true
}

/// `mirror`: what is in front of this line, shown behind it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mirror {
    /// The mirror's length, along the entity's local x.
    #[serde(default = "default_width")]
    pub width: f32,
    /// How far in front it reflects - and so how far behind the line the
    /// reflection is drawn.
    #[serde(default = "default_depth")]
    pub depth: f32,
    /// Texels per world unit of the reflection's texture: 1 matches a
    /// camera at zoom 1; lower is cheaper and softer.
    #[serde(default = "default_resolution")]
    pub resolution: f32,
    /// Multiplies the reflection (linear colour); alpha fades it - glass
    /// that shows a little of itself is about 0.85.
    #[serde(default = "default_tint")]
    pub tint: [f32; 4],
    /// Where the reflection sorts among everything else.
    #[serde(default)]
    pub z_order: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    /// Off, the mirror draws nothing and costs nothing.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl Default for Mirror {
    fn default() -> Self {
        Self {
            width: default_width(),
            depth: default_depth(),
            resolution: default_resolution(),
            tint: default_tint(),
            z_order: 0,
            sort_layer: None,
            enabled: true,
        }
    }
}

/// The largest side a reflection's texture is given.
pub const MAX_MIRROR_TEXELS: u32 = 2048;

/// The mirror's key in its view's [`CameraPass::Mirror`].
pub fn mirror_key(mirror: Entity) -> u32 {
    mirror.id() + 1
}

/// The texture a mirror's view draws into: a name nothing else uses.
pub fn target_id(mirror: Entity) -> String {
    format!("__mirror/{}", mirror.id())
}

/// Where a mirror is this frame, in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MirrorPlace {
    /// The middle of the mirror's line.
    pub centre: Vec2,
    /// Unit vector along the line (the entity's local +x).
    pub along: Vec2,
    /// Unit vector the mirror faces (the entity's local +y).
    pub front: Vec2,
    pub rotation: f32,
    pub width: f32,
    pub depth: f32,
}

impl MirrorPlace {
    pub fn of(mirror: &Mirror, position: Vec2, rotation: f32) -> Self {
        Self::at(position, rotation, mirror.width, mirror.depth)
    }

    /// A line `width` long at `position`, turned by `rotation`, looking
    /// `depth` in front of it (a portal's too, `ecs::portal`).
    pub fn at(position: Vec2, rotation: f32, width: f32, depth: f32) -> Self {
        let along = Vec2::from_angle(rotation);
        Self { centre: position, along, front: along.perp(), rotation, width: width.max(0.0), depth: depth.max(0.0) }
    }

    /// The camera that sees what the mirror reflects: centred on the region
    /// in front, turned with the mirror, so the top of its picture is the far
    /// side of that region and the bottom is the line. `key` is the mirror's
    /// [`mirror_key`]: its own pass leaves its own reflection out.
    pub fn camera(&self, key: u32) -> Camera2D {
        let mut camera = Camera2D::new(self.width.max(1.0), self.depth.max(1.0));
        camera.position = self.centre + self.front * self.depth * 0.5;
        camera.rotation = self.rotation;
        camera.pass = CameraPass::Mirror(key);
        camera
    }

    /// The reflection's quad behind the line and its texture coordinates:
    /// the line's two ends take the bottom of the picture (the line), the far
    /// corners its top (the far side of the region in front) - which is the
    /// reflection.
    pub fn reflection(&self) -> ([Vec2; 4], [[f32; 3]; 4]) {
        let half = self.along * self.width * 0.5;
        let back = self.front * self.depth;
        (
            [self.centre - half, self.centre + half, self.centre + half - back, self.centre - half - back],
            [[0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
        )
    }

    /// The box round both regions: what it reflects and where it shows it.
    fn bounds(&self) -> (Vec2, Vec2) {
        let half = self.along * self.width * 0.5;
        let reach = self.front * self.depth;
        let corners = [self.centre - half - reach, self.centre + half - reach, self.centre - half + reach, self.centre + half + reach];
        let lo = corners.iter().fold(Vec2::splat(f32::MAX), |a, c| a.min(*c));
        let hi = corners.iter().fold(Vec2::splat(f32::MIN), |a, c| a.max(*c));
        (lo, hi)
    }
}

/// Where `entity`'s mirror is this frame, by its world transform.
pub fn place_of(world: &World, entity: Entity) -> Option<(Mirror, MirrorPlace)> {
    let mirror = (*world.get::<&Mirror>(entity).ok()?).clone();
    let (p, r) = match (world.get::<&WorldTransform>(entity), world.get::<&Transform>(entity)) {
        (Ok(w), _) => (w.position, w.rotation),
        (_, Ok(t)) => (t.position, t.rotation),
        _ => return None,
    };
    let place = MirrorPlace::of(&mirror, p, r);
    Some((mirror, place))
}

thread_local! {
    /// The mirrors with a view this frame - the only ones whose texture
    /// exists, so the only ones the sprite pass draws.
    static ACTIVE: RefCell<HashSet<Entity>> = RefCell::new(HashSet::new());
}

/// Whether `mirror` has a view this frame (from [`mirror_views`]).
pub fn is_active(mirror: Entity) -> bool {
    ACTIVE.with(|a| a.borrow().contains(&mirror))
}

/// A view per enabled mirror near `main`'s view - the main camera's view
/// grown by the deepest mirror, so a mirror seen only in another mirror
/// still draws. Every host appends these to its views each frame.
pub fn mirror_views(world: &World, main: &Camera2D) -> Vec<crate::ecs::views::ViewCamera> {
    let mirrors: Vec<(Entity, Mirror, MirrorPlace)> = world
        .query::<&Mirror>()
        .iter()
        .filter(|(_, m)| m.enabled && m.width > 0.0 && m.depth > 0.0)
        .filter_map(|(e, _)| place_of(world, e).map(|(m, p)| (e, m, p)))
        .collect();
    let reach = mirrors.iter().fold(0.0f32, |a, (_, _, p)| a.max(p.depth));
    let corners = main.view_corners();
    let lo = corners.iter().fold(Vec2::splat(f32::MAX), |a, c| a.min(*c)) - Vec2::splat(reach);
    let hi = corners.iter().fold(Vec2::splat(f32::MIN), |a, c| a.max(*c)) + Vec2::splat(reach);
    let mut active = HashSet::new();
    let mut views: Vec<crate::ecs::views::ViewCamera> = mirrors
        .into_iter()
        .filter(|(_, _, p)| {
            let (a, b) = p.bounds();
            a.x <= hi.x && b.x >= lo.x && a.y <= hi.y && b.y >= lo.y
        })
        .map(|(e, m, p)| {
            active.insert(e);
            let texels = |world_units: f32| ((world_units * m.resolution.max(0.01)).round() as u32).clamp(1, MAX_MIRROR_TEXELS);
            crate::ecs::views::ViewCamera {
                target: target_id(e),
                size: (texels(p.width), texels(p.depth)),
                camera: p.camera(mirror_key(e)),
                transparent: true,
            }
        })
        .collect();
    ACTIVE.with(|a| *a.borrow_mut() = active);
    views.sort_by(|a, b| a.target.cmp(&b.target));
    views
}

/// Forget this frame's mirrors (a test or a tool drawing without a host).
pub fn clear_active() {
    ACTIVE.with(|a| a.borrow_mut().clear());
}

/// `Mirror.set(e, field, value)`: write any field by name - `enabled` to
/// turn a mirror off, `tint` to cloud it, `depth` to reach further.
pub fn set_mirror_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let Ok(mut m) = world.get::<&mut Mirror>(entity) else {
        return Err("no mirror on that entity".into());
    };
    let mut whole = serde_json::to_value(&*m).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) && field != "sort_layer" {
        return Err(format!("'{field}' is not a mirror field"));
    }
    let value = match (field, value) {
        ("z_order", serde_json::Value::Number(n)) => serde_json::Value::from(n.as_f64().unwrap_or(0.0) as i64),
        _ => value.clone(),
    };
    map.insert(field.to_string(), value);
    *m = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    Ok(())
}
