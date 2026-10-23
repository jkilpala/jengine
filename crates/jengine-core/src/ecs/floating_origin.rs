//! A world that never gets far from its origin (Oct 2026; Unity's floating
//! origin, KSP's "Krakensbane"): a 32-bit float has about seven digits, so a
//! thing a million units out is placed to a tenth of a unit and one ten
//! million out to a whole one - it shakes, and so does everything near it.
//! The fix is not more bits but less distance: when the entity carrying a
//! [`FloatingOrigin`] strays more than `threshold` from the origin along an
//! axis, the **whole world is moved back** along that axis by where it is,
//! so it stands at the origin again and everything keeps its place relative
//! to it. Each axis on its own: a car driving along a side-scroller's road
//! is moved back in x and never in y, so the ground stays at the height it
//! was authored at and a script holding a height need not follow it.
//!
//! What is moved, in the world: every entity without a parent (children ride
//! along), and the world positions components keep - trail, cloth and rope
//! points, fluid particles, a rewindable root's history. Outside the world,
//! whoever keeps world positions of their own follows one counter with
//! [`follow`]: the particles in flight, the renderer's camera (so a follow
//! does not sweep across the jump), the physics bodies (moved, not
//! teleported - velocities and contacts are kept, and a kinematic body is
//! not given the jump as a speed) and the motion blur's history. The total
//! moved so far is kept as `f64` in the entity's custom `FloatingOrigin`
//! (`x`, `y`), so a script can still say how far it has really come, and
//! `on_origin_shift` is pushed to its script after each move.
//!
//! Ticked last in `tick_motion`, so no host was wired.

use std::cell::Cell;

use glam::{DVec2, Vec2};
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{CustomComponents, Parent, Trail, Transform};

/// `floating_origin`: keep the world near the origin round this entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FloatingOrigin {
    /// How far it may stray before the world is moved back.
    pub threshold: f32,
    pub enabled: bool,
    /// How far the world has been moved, all told - runtime state.
    #[serde(skip)]
    pub moved: DVec2,
}

impl Default for FloatingOrigin {
    fn default() -> Self {
        Self { threshold: 5000.0, enabled: true, moved: DVec2::ZERO }
    }
}

thread_local! {
    /// Every shift so far, added up (in f64 so it never drifts) - read by
    /// whoever keeps world positions outside the world.
    static ORIGIN: Cell<(f64, f64)> = const { Cell::new((0.0, 0.0)) };
}

/// The total shift so far.
pub fn origin() -> (f64, f64) {
    ORIGIN.with(|o| o.get())
}

/// How far the world has moved back since this reader last looked - what
/// to take off the world positions it keeps. A reader that has never looked
/// (`None`, a fresh one) starts from now and moves nothing.
pub fn follow(seen: &mut Option<(f64, f64)>) -> Vec2 {
    let now = origin();
    let by = match *seen {
        Some(s) => Vec2::new((now.0 - s.0) as f32, (now.1 - s.1) as f32),
        None => Vec2::ZERO,
    };
    *seen = Some(now);
    by
}

/// Move the world back by `by`: every root's transform and the world
/// positions components keep. Exposed for tests and tools; the component
/// calls it.
pub fn shift_world(world: &mut World, by: Vec2) {
    for (_, t) in world.query_mut::<&mut Transform>().without::<&Parent>() {
        t.position -= by;
    }
    for (_, trail) in world.query_mut::<&mut Trail>() {
        for p in &mut trail.points {
            p.position -= by;
        }
    }
    for (_, c) in world.query_mut::<&mut crate::ecs::cloth::Cloth>() {
        for p in &mut c.points {
            *p -= by;
        }
    }
    for (_, r) in world.query_mut::<&mut crate::ecs::cloth::Rope>() {
        for p in &mut r.points {
            *p -= by;
        }
    }
    for (_, f) in world.query_mut::<&mut crate::ecs::fluid::Fluid>() {
        for p in &mut f.particles {
            p.position -= by;
        }
    }
    for (_, r) in world.query_mut::<&mut crate::ecs::rewind::Rewindable>().without::<&Parent>() {
        for s in &mut r.state.samples {
            s.position -= by;
        }
    }
    ORIGIN.with(|o| {
        let (x, y) = o.get();
        o.set((x + by.x as f64, y + by.y as f64));
    });
}

/// Move the world back along each axis the tracked entity has strayed past
/// the threshold on. Last in `tick_motion`.
pub fn tick_floating_origin(world: &mut World) {
    let Some((e, at, f)) = world
        .query::<(&FloatingOrigin, &Transform)>()
        .without::<&Parent>()
        .iter()
        .next()
        .map(|(e, (f, t))| (e, t.position, f.clone()))
    else {
        return;
    };
    let axis = |v: f32| if v.abs() > f.threshold { v } else { 0.0 };
    let by = Vec2::new(axis(at.x), axis(at.y));
    if !f.enabled || by == Vec2::ZERO {
        return;
    }
    shift_world(world, by);
    let moved = f.moved + by.as_dvec2();
    if let Ok(mut fo) = world.get::<&mut FloatingOrigin>(e) {
        fo.moved = moved;
    }
    let custom = serde_json::json!({ "x": moved.x, "y": moved.y });
    let custom = match world.get::<&mut CustomComponents>(e) {
        Ok(mut c) => {
            c.0.insert("FloatingOrigin".into(), custom);
            None
        }
        Err(_) => Some(custom),
    };
    if let Some(custom) = custom {
        let mut map = std::collections::HashMap::new();
        map.insert("FloatingOrigin".into(), custom);
        let _ = world.insert_one(e, CustomComponents(map));
    }
    crate::ecs::callbacks::push(e, "on_origin_shift");
}

/// `FloatingOrigin.set(e, field, value)`: `threshold` or `enabled`.
pub fn set_origin_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut f = world.get::<&mut FloatingOrigin>(entity).map_err(|_| "no floating_origin on that entity".to_string())?;
    match field {
        "threshold" => f.threshold = value.as_f64().ok_or("threshold takes a number")? as f32,
        "enabled" => f.enabled = value.as_bool().ok_or("enabled takes true or false")?,
        _ => return Err(format!("'{field}' is not a floating_origin field (threshold, enabled)")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straying_past_the_threshold_moves_the_world_back() {
        let mut world = World::new();
        let car = world.spawn((Transform { position: Vec2::new(4000.0, 10.0), ..Default::default() }, FloatingOrigin::default()));
        let tree = world.spawn((Transform { position: Vec2::new(4500.0, 0.0), ..Default::default() },));
        let wheel = world.spawn((Transform { position: Vec2::new(30.0, -20.0), ..Default::default() }, Parent(car)));
        let mut reader = None;
        assert_eq!(follow(&mut reader), Vec2::ZERO, "a fresh reader starts from now");
        tick_floating_origin(&mut world);
        assert_eq!(world.get::<&Transform>(car).unwrap().position.x, 4000.0, "within the threshold: nothing moves");
        world.get::<&mut Transform>(car).unwrap().position.x = 6000.0;
        crate::ecs::callbacks::take();
        tick_floating_origin(&mut world);
        assert_eq!(
            world.get::<&Transform>(car).unwrap().position,
            Vec2::new(0.0, 10.0),
            "back at the origin in x - and only x, which is the axis that strayed"
        );
        assert_eq!(world.get::<&Transform>(tree).unwrap().position, Vec2::new(-1500.0, 0.0), "and the tree kept its place by it");
        assert_eq!(world.get::<&Transform>(wheel).unwrap().position, Vec2::new(30.0, -20.0), "a child rides along");
        assert_eq!(world.get::<&CustomComponents>(car).unwrap().0["FloatingOrigin"]["x"], 6000.0);
        assert_eq!(follow(&mut reader), Vec2::new(6000.0, 0.0), "the readers outside the world are told");
        assert_eq!(follow(&mut reader), Vec2::ZERO, "once");
        // Far out on both axes: moved back on both.
        world.get::<&mut Transform>(car).unwrap().position = Vec2::new(-7000.0, 9000.0);
        tick_floating_origin(&mut world);
        assert_eq!(world.get::<&Transform>(car).unwrap().position, Vec2::ZERO);
        assert_eq!(world.get::<&CustomComponents>(car).unwrap().0["FloatingOrigin"]["y"], 9000.0);
        assert!(crate::ecs::callbacks::take().iter().any(|c| c.handler == "on_origin_shift"));
    }
}
