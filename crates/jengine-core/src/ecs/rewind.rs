//! Rewinding the world (Oct 2026; Braid, Prince of Persia's sands): every
//! `rewindable` entity keeps a history of how it was, and one `rewind` clock
//! decides whether the world is being recorded, played backwards or held.
//!
//! A [`Rewindable`] records, once a frame while its clock records, where it
//! is (its transform), how fast it is going (its `velocity`, which on a
//! dynamic body is the body's), its sprite's colour and picture (`uv_rect`,
//! so an animation's frame comes back with it), its sprite animator's place
//! in its clip, and its particle emitter's rate and whether it is on - `seconds`
//! of it, the oldest let go. When the [`Rewind`] clock is set to `rewind`, its
//! cursor walks back through the recording at `speed` and every rewindable is
//! put where it was then (blended between the two samples either side); at
//! the start of the recording it stops there, and `hold` keeps the world still
//! at the cursor. Set back to `record`, the world goes on **from the cursor**:
//! what came after it is forgotten, a body is handed the velocity it had then,
//! and an animator plays on from its frame.
//!
//! Bodies are put back by writing their transforms, which the physics
//! teleports them to, with no velocity while the clock is not recording, so
//! gravity does not pull them out of the past between frames. A particle
//! emitter is held while rewinding (it makes no new particles). A thing that
//! did not exist yet at the cursor - spawned after the recording began - is
//! hidden until the world catches up with its birth; scripts' own state is
//! theirs to rewind (a script reads the clock's `mode`).
//!
//! It runs inside `tick_motion`, before the physics step, so no host was
//! wired.

use std::collections::VecDeque;

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::animation::SpriteAnimator;
use crate::ecs::components::{SpriteComponent, Transform, Velocity2D};
use crate::particles::ParticleEmitter;

/// What the clock is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RewindMode {
    /// Time runs forwards and is written down.
    #[default]
    Record,
    /// The cursor walks back through what was written down.
    Rewind,
    /// The world is kept as it was at the cursor.
    Hold,
}

/// `rewind`: the clock every `rewindable` records and rewinds by. One a
/// world - the first found is the clock.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Rewind {
    pub mode: RewindMode,
    /// How fast the cursor walks back: 1 is as fast as time went forwards.
    pub speed: f32,
    /// Runtime state - never saved.
    #[serde(skip)]
    pub state: RewindClock,
}

impl Default for Rewind {
    fn default() -> Self {
        Self { mode: RewindMode::Record, speed: 1.0, state: RewindClock::default() }
    }
}

/// Where the clock is.
#[derive(Debug, Clone, Default)]
pub struct RewindClock {
    /// The time the world is at - the recording's latest moment while
    /// recording, the cursor otherwise.
    pub now: f64,
    /// The mode last frame, to see a switch.
    pub last: RewindMode,
}

/// One moment of a rewindable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub t: f64,
    pub position: Vec2,
    pub rotation: f32,
    pub scale: Vec2,
    pub velocity: Option<[f32; 2]>,
    pub color: Option<[f32; 4]>,
    pub uv_rect: Option<[f32; 4]>,
    /// The animator's frame and time into it.
    pub frame: Option<(usize, f32)>,
    /// The emitter's rate and whether it is on.
    pub emitter: Option<(f32, bool)>,
}

/// `rewindable`: an entity whose history is kept for its world's `rewind`
/// clock.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Rewindable {
    /// How much of its history is kept.
    pub seconds: f32,
    /// Runtime state - never saved.
    #[serde(skip)]
    pub state: RewindHistory,
}

impl Default for Rewindable {
    fn default() -> Self {
        Self { seconds: 10.0, state: RewindHistory::default() }
    }
}

/// A rewindable's recording.
#[derive(Debug, Clone, Default)]
pub struct RewindHistory {
    pub samples: VecDeque<Sample>,
    /// When it was first recorded - before it, it did not exist.
    pub born: Option<f64>,
    /// What it was before being hidden or held, to give back.
    hidden_color: Option<[f32; 4]>,
    was_playing: Option<bool>,
    was_active: Option<bool>,
}

impl RewindHistory {
    /// The moment at `t`, blended between the samples either side (`None`
    /// before the first sample).
    pub fn at(&self, t: f64) -> Option<Sample> {
        let first = self.samples.front()?;
        if t <= first.t {
            return Some(*first);
        }
        // The samples are in time order: find the pair round `t`.
        let k = self.samples.partition_point(|s| s.t <= t);
        if k >= self.samples.len() {
            return self.samples.back().copied();
        }
        let (a, b) = (self.samples[k - 1], self.samples[k]);
        let f = if b.t > a.t { ((t - a.t) / (b.t - a.t)) as f32 } else { 0.0 };
        let mix = |x: f32, y: f32| x + (y - x) * f;
        let turn = {
            let mut d = b.rotation - a.rotation;
            d = (d + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            a.rotation + d * f
        };
        Some(Sample {
            t,
            position: a.position.lerp(b.position, f),
            rotation: turn,
            scale: a.scale.lerp(b.scale, f),
            velocity: a.velocity.zip(b.velocity).map(|(p, q)| [mix(p[0], q[0]), mix(p[1], q[1])]),
            color: a.color.zip(b.color).map(|(p, q)| std::array::from_fn(|i| mix(p[i], q[i]))),
            // A picture is a frame or the next one, not a blend of the two.
            uv_rect: if f < 0.5 { a.uv_rect } else { b.uv_rect },
            frame: if f < 0.5 { a.frame } else { b.frame },
            emitter: if f < 0.5 { a.emitter } else { b.emitter },
        })
    }
}

/// The world's clock: its entity, mode and speed.
fn clock(world: &World) -> Option<(Entity, RewindMode, f32)> {
    world.query::<&Rewind>().iter().next().map(|(e, r)| (e, r.mode, r.speed))
}

/// Record, rewind or hold every rewindable. Inside `tick_motion`.
pub fn tick_rewind(world: &mut World, dt: f32) {
    let Some((clock_entity, mode, speed)) = clock(world) else { return };
    let _span = crate::profiling::trace::span("rewind");
    let (now, switched_to_record) = {
        let mut r = world.get::<&mut Rewind>(clock_entity).expect("the clock");
        let last = r.state.last;
        r.state.last = mode;
        match mode {
            RewindMode::Record => r.state.now += dt.max(0.0) as f64,
            RewindMode::Rewind => r.state.now -= (dt.max(0.0) * speed.max(0.0)) as f64,
            RewindMode::Hold => {}
        }
        (r.state.now, mode == RewindMode::Record && last != RewindMode::Record)
    };
    // The cursor stops at the oldest moment anything still remembers.
    let now = if mode == RewindMode::Rewind {
        let oldest = world
            .query::<&Rewindable>()
            .iter()
            .filter_map(|(_, r)| r.state.samples.front().map(|s| s.t))
            .fold(f64::INFINITY, f64::min);
        if now < oldest && oldest.is_finite() {
            world.get::<&mut Rewind>(clock_entity).expect("the clock").state.now = oldest;
            oldest
        } else {
            now
        }
    } else {
        now
    };

    for (_, (rw, t, mut vel, mut sprite, mut anim, mut emitter)) in world
        .query::<(
            &mut Rewindable,
            &mut Transform,
            Option<&mut Velocity2D>,
            Option<&mut SpriteComponent>,
            Option<&mut SpriteAnimator>,
            Option<&mut ParticleEmitter>,
        )>()
        .iter()
    {
        let h = &mut rw.state;
        if mode == RewindMode::Record {
            if switched_to_record {
                // The world goes on from the cursor: forget what came after,
                // and hand back what moves it.
                while h.samples.back().is_some_and(|s| s.t > now) {
                    h.samples.pop_back();
                }
                if let (Some(v), Some(s)) = (vel.as_deref_mut(), h.samples.back())
                    && let Some(sv) = s.velocity
                {
                    v.x = sv[0];
                    v.y = sv[1];
                }
                if h.born.is_some_and(|b| b > now) {
                    h.born = Some(now);
                }
                unhide(h, sprite.as_deref_mut(), anim.as_deref_mut(), emitter.as_deref_mut());
            }
            h.born.get_or_insert(now);
            h.samples.push_back(Sample {
                t: now,
                position: t.position,
                rotation: t.rotation,
                scale: t.scale,
                velocity: vel.as_deref().map(|v| [v.x, v.y]),
                color: sprite.as_deref().map(|s| h.hidden_color.unwrap_or(s.color)),
                uv_rect: sprite.as_deref().and_then(|s| s.uv_rect),
                frame: anim.as_deref().map(|a| (a.current_frame, a.elapsed)),
                emitter: emitter.as_deref().map(|e| (e.emission_rate, h.was_active.unwrap_or(e.active))),
            });
            let keep = rw.seconds.max(0.0) as f64;
            while h.samples.front().is_some_and(|s| s.t < now - keep) {
                h.samples.pop_front();
            }
            continue;
        }
        // Rewinding or held: put it where it was.
        if h.born.is_some_and(|b| now < b) {
            // Not born yet: hidden.
            if let Some(s) = sprite {
                if h.hidden_color.is_none() {
                    h.hidden_color = Some(s.color);
                }
                s.color[3] = 0.0;
            }
            continue;
        }
        let Some(s) = h.at(now) else { continue };
        t.position = s.position;
        t.rotation = s.rotation;
        t.scale = s.scale;
        if let Some(v) = vel {
            v.x = 0.0;
            v.y = 0.0;
        }
        if let Some(sp) = sprite {
            if let Some(c) = s.color {
                sp.color = c;
                h.hidden_color = None;
            }
            if s.uv_rect.is_some() {
                sp.uv_rect = s.uv_rect;
            }
        }
        if let Some(a) = anim {
            h.was_playing.get_or_insert(a.playing);
            a.playing = false;
            if let Some((frame, elapsed)) = s.frame {
                a.current_frame = frame;
                a.elapsed = elapsed;
            }
        }
        if let Some(e) = emitter {
            h.was_active.get_or_insert(e.active);
            e.active = false;
            if let Some((rate, _)) = s.emitter {
                e.emission_rate = rate;
            }
        }
    }
}

/// Give back what rewinding took away: an animator's playing, an emitter's
/// being on (as it was at the cursor), a hidden sprite's colour.
fn unhide(
    h: &mut RewindHistory,
    sprite: Option<&mut SpriteComponent>,
    anim: Option<&mut SpriteAnimator>,
    emitter: Option<&mut ParticleEmitter>,
) {
    if let (Some(s), Some(c)) = (sprite, h.hidden_color.take()) {
        s.color = c;
    }
    if let (Some(a), Some(p)) = (anim, h.was_playing.take()) {
        a.playing = p;
    }
    if let Some(e) = emitter {
        let at = h.samples.back().and_then(|s| s.emitter).map(|(_, on)| on);
        if let Some(on) = at.or(h.was_active) {
            e.active = on;
        }
        h.was_active = None;
    }
}

/// `Rewind.set(e, field, value)`: the clock's `mode` (`"record"`,
/// `"rewind"`, `"hold"`) or `speed`.
pub fn set_rewind_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut r = world.get::<&mut Rewind>(entity).map_err(|_| "no rewind clock on that entity".to_string())?;
    match field {
        "mode" => {
            r.mode = serde_json::from_value(value.clone()).map_err(|_| format!("'{value}' is not record, rewind or hold"))?;
        }
        "speed" => r.speed = value.as_f64().ok_or("speed takes a number")? as f32,
        _ => return Err(format!("'{field}' is not a rewind field (mode, speed)")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> (World, Entity, Entity) {
        let mut w = World::new();
        let clock = w.spawn((Rewind::default(),));
        let thing = w.spawn((Transform::default(), Velocity2D::default(), Rewindable::default()));
        (w, clock, thing)
    }

    fn step(w: &mut World, e: Entity, dt: f32) {
        // A thing moving right at 60 units a second while time runs.
        if w.get::<&Rewind>(e).unwrap().mode == RewindMode::Record {
            for (_, (t, v)) in w.query_mut::<(&mut Transform, &mut Velocity2D)>() {
                v.x = 60.0;
                t.position.x += v.x * dt;
            }
        }
        tick_rewind(w, dt);
    }

    #[test]
    fn rewinding_walks_back_through_the_recording_and_stops_at_its_start() {
        let (mut w, clock, thing) = world();
        for _ in 0..60 {
            step(&mut w, clock, 1.0 / 60.0);
        }
        let x = w.get::<&Transform>(thing).unwrap().position.x;
        assert!((x - 60.0).abs() < 1e-3);
        set_rewind_field(&w, clock, "mode", &serde_json::json!("rewind")).unwrap();
        for _ in 0..30 {
            step(&mut w, clock, 1.0 / 60.0);
        }
        let back = w.get::<&Transform>(thing).unwrap().position.x;
        assert!((back - 30.0).abs() < 1.5, "half a second back: {back}");
        assert_eq!(w.get::<&Velocity2D>(thing).unwrap().x, 0.0, "held still while rewinding");
        for _ in 0..120 {
            step(&mut w, clock, 1.0 / 60.0);
        }
        let start = w.get::<&Transform>(thing).unwrap().position.x;
        assert!(start.abs() < 1.5, "at the start of the recording: {start}");
    }

    #[test]
    fn recording_again_goes_on_from_the_cursor() {
        let (mut w, clock, thing) = world();
        for _ in 0..60 {
            step(&mut w, clock, 1.0 / 60.0);
        }
        set_rewind_field(&w, clock, "mode", &serde_json::json!("rewind")).unwrap();
        for _ in 0..30 {
            step(&mut w, clock, 1.0 / 60.0);
        }
        set_rewind_field(&w, clock, "mode", &serde_json::json!("record")).unwrap();
        step(&mut w, clock, 1.0 / 60.0);
        let h = &w.get::<&Rewindable>(thing).unwrap().state;
        let last = h.samples.back().unwrap();
        assert!(h.samples.iter().all(|s| s.t <= last.t), "nothing after the cursor is kept");
        assert!(h.samples.len() < 40, "the half second after the cursor was forgotten: {}", h.samples.len());
        assert_eq!(w.get::<&Velocity2D>(thing).unwrap().x, 60.0, "it moves again");
    }

    #[test]
    fn a_thing_born_after_the_cursor_is_hidden_until_then() {
        let (mut w, clock, _) = world();
        for _ in 0..30 {
            step(&mut w, clock, 1.0 / 60.0);
        }
        let late = w.spawn((Transform::default(), SpriteComponent::default(), Rewindable::default()));
        for _ in 0..30 {
            step(&mut w, clock, 1.0 / 60.0);
        }
        set_rewind_field(&w, clock, "mode", &serde_json::json!("rewind")).unwrap();
        for _ in 0..45 {
            step(&mut w, clock, 1.0 / 60.0);
        }
        assert_eq!(w.get::<&SpriteComponent>(late).unwrap().color[3], 0.0, "not born yet");
        set_rewind_field(&w, clock, "mode", &serde_json::json!("record")).unwrap();
        step(&mut w, clock, 1.0 / 60.0);
        assert_eq!(w.get::<&SpriteComponent>(late).unwrap().color[3], 1.0, "given back");
    }

    #[test]
    fn a_turn_blends_the_short_way_round() {
        let mut h = RewindHistory::default();
        let s = |t: f64, r: f32| Sample {
            t,
            position: Vec2::ZERO,
            rotation: r,
            scale: Vec2::ONE,
            velocity: None,
            color: None,
            uv_rect: None,
            frame: None,
            emitter: None,
        };
        h.samples.push_back(s(0.0, 3.0));
        h.samples.push_back(s(1.0, -3.0));
        let mid = h.at(0.5).unwrap().rotation;
        assert!(mid.abs() > 3.0, "through pi, not through zero: {mid}");
        assert!(set_rewind_field(&World::new(), Entity::DANGLING, "mode", &serde_json::json!("x")).is_err());
    }
}
