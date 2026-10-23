//! A clock of its own (Oct 2026): `Time.set_scale` slows the whole world -
//! every host multiplies the frame's `dt` by it - and a [`TimeScale`] lets
//! one entity keep a different pace inside it. Bullet time is the world at
//! a quarter and the hero at full speed: the hero carries a `time_scale`
//! with `ignore_world`, so its velocity, sprite animation, skeleton,
//! particles and trail run on real time while everything round it crawls.
//!
//! `scale` multiplies the entity's time on top of that (2 is twice as fast,
//! 0 stops it). A world paused with `Time.set_scale(0)` stops even an entity
//! that ignores the world - a pause is a pause. Scripts are handed the
//! world's `dt`; one that moves its entity at its own pace divides by
//! `Time.get_scale()` itself. Physics bodies share one rapier step and
//! cannot run at their own pace.

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

/// `time_scale`: this entity's own pace.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TimeScale {
    /// How fast its time runs against the world's (or real time's, with
    /// `ignore_world`).
    pub scale: f32,
    /// Run on real time: untouched by `Time.set_scale` (but for a pause).
    pub ignore_world: bool,
}

impl Default for TimeScale {
    fn default() -> Self {
        Self { scale: 1.0, ignore_world: false }
    }
}

/// What to multiply the world's `dt` by for an entity with `ts` - 1 for
/// one without.
pub fn factor(ts: Option<&TimeScale>) -> f32 {
    let Some(ts) = ts else { return 1.0 };
    let free = if ts.ignore_world {
        let world = crate::time::world_scale();
        if world > 1e-6 { 1.0 / world } else { 0.0 }
    } else {
        1.0
    };
    ts.scale.max(0.0) * free
}

/// [`factor`] for an entity in `world`.
pub fn factor_of(world: &World, entity: Entity) -> f32 {
    factor(world.get::<&TimeScale>(entity).ok().as_deref())
}

/// `TimeScale.set(e, field, value)`: `scale` or `ignore_world`. Adds the
/// component when the entity has none.
pub fn set_time_scale_field(world: &mut World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut ts = world.get::<&TimeScale>(entity).map(|t| *t).unwrap_or_default();
    match field {
        "scale" => ts.scale = value.as_f64().ok_or("scale takes a number")? as f32,
        "ignore_world" => ts.ignore_world = value.as_bool().ok_or("ignore_world takes true or false")?,
        _ => return Err(format!("'{field}' is not a time_scale field (scale, ignore_world)")),
    }
    world.insert_one(entity, ts).map_err(|e| e.to_string())
}
