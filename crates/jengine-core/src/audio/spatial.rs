//! **Positional audio** (Oct 2026): a sound in the world is louder near the
//! listener and panned to its side - Unity's spatial blend, Godot's
//! `AudioStreamPlayer2D`. Before it every sound played centred at its own
//! volume wherever it was, so a waterfall across the map was as loud as one
//! underfoot, and a looping `AudioSource` kept playing after its entity was
//! gone.
//!
//! Three parts, all here so every runtime does the same arithmetic:
//!
//! - [`attenuate`]: full volume within `min_distance`, silent past
//!   `max_distance`, eased between; panned by how far left or right the
//!   source is, across `pan_width`.
//! - [`listener`]: an entity with an [`AudioListener`] - the player, usually -
//!   or else the active camera.
//! - [`SpatialAudio`]: what a host keeps, one per world, to remember which
//!   playing sound belongs to which `AudioSource`. Each frame it answers
//!   what every source should play at now, and which sounds to stop because
//!   their entity has gone. Native applies that through its `AudioBackend`
//!   (`ecs::systems::apply_spatial_audio`), the browsers through `WebAudio`.

use std::collections::HashMap;

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::ecs::components::{AudioSource, Transform, WorldTransform};

/// Marks the entity sounds are heard from (Unity's `AudioListener`). Without
/// one the active camera is the listener.
///
/// With `room_reverb` naming a bus, the listener also **hears the room it is
/// in**: rays cast round it against the colliders (`audio::zones`) say how
/// enclosed it is and how far the walls are, and that bus is given a reverb
/// to match - none in the open, a short one in a closet, a long wet one in a
/// great hall.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioListener {
    /// The bus the room's reverb goes on (empty: none).
    pub room_reverb: String,
    /// How far a ray looks for a wall, in world units: a room bigger than
    /// this sounds like the open.
    pub room_reach: f32,
    /// The reverb's mix in a room closed all round, 0..1.
    pub room_mix: f32,
}

impl Default for AudioListener {
    fn default() -> Self {
        Self { room_reverb: String::new(), room_reach: 1200.0, room_mix: 0.5 }
    }
}

/// How far a positional sound carries, in world units: what `AudioSource`
/// carries for its own sound, and what `Audio.play_sound_at` uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Falloff {
    pub min_distance: f32,
    pub max_distance: f32,
    pub pan_width: f32,
}

impl Default for Falloff {
    fn default() -> Self {
        Self { min_distance: DEFAULT_MIN_DISTANCE, max_distance: DEFAULT_MAX_DISTANCE, pan_width: DEFAULT_PAN_WIDTH }
    }
}

/// Full volume within this of the listener.
pub const DEFAULT_MIN_DISTANCE: f32 = 120.0;
/// Silent past this.
pub const DEFAULT_MAX_DISTANCE: f32 = 1000.0;
/// A source this far to one side is panned all the way to it.
pub const DEFAULT_PAN_WIDTH: f32 = 600.0;

/// `(gain, pan)` for a source at `source` heard at `listener`: gain 1 within
/// `min_distance`, 0 from `max_distance`, and between them `(1 - t)^2` of
/// the way across - a curve that falls quickly near the source and tails off,
/// as a sound does. Pan is the sideways offset over `pan_width`, -1 hard left
/// to 1 hard right.
pub fn attenuate(listener: Vec2, source: Vec2, f: &Falloff) -> (f32, f32) {
    let d = listener.distance(source);
    let (min, max) = (f.min_distance.max(0.0), f.max_distance.max(f.min_distance.max(0.0) + 1e-3));
    let gain = if d <= min {
        1.0
    } else if d >= max {
        0.0
    } else {
        let t = (d - min) / (max - min);
        (1.0 - t) * (1.0 - t)
    };
    let pan = if f.pan_width > 0.0 { ((source.x - listener.x) / f.pan_width).clamp(-1.0, 1.0) } else { 0.0 };
    (gain, pan)
}

/// Where sounds are heard from: the first entity with an [`AudioListener`],
/// else the active camera, else the origin - by world transform.
pub fn listener(world: &hecs::World) -> Vec2 {
    let at = |t: &Transform, w: Option<&WorldTransform>| w.map(|w| w.position).unwrap_or(t.position);
    if let Some((_, (t, w))) = world.query::<(&Transform, Option<&WorldTransform>)>().with::<&AudioListener>().iter().next() {
        return at(t, w);
    }
    crate::ecs::camera_rig::main_camera(world)
        .and_then(|e| {
            let mut q = world.query_one::<(&Transform, Option<&WorldTransform>)>(e).ok()?;
            q.get().map(|(t, w)| at(t, w))
        })
        .unwrap_or(Vec2::ZERO)
}

/// The entity with the [`AudioListener`], if one has it.
pub fn listener_entity(world: &hecs::World) -> Option<hecs::Entity> {
    world.query::<()>().with::<&AudioListener>().iter().next().map(|(e, _)| e)
}

/// How many walls are between the listener and a source, and so its gain
/// and cutoff: each wall takes `occlusion` of what is left, and the first
/// sets the cutoff to `muffle`, each further one halving it (0: open).
pub fn occlude(walls: usize, occlusion: f32, muffle: f32) -> (f32, f32) {
    if walls == 0 {
        return (1.0, 0.0);
    }
    let gain = (1.0 - occlusion.clamp(0.0, 1.0)).powi(walls as i32);
    let cutoff = if muffle > 0.0 { muffle / 2f32.powi(walls as i32 - 1) } else { 0.0 };
    (gain, cutoff)
}

/// The gain and pan a sound played at `(x, y)` starts with - what
/// `Audio.play_sound_at` uses, with the default [`Falloff`].
pub fn at_point(world: &hecs::World, x: f32, y: f32) -> (f32, f32) {
    attenuate(listener(world), Vec2::new(x, y), &Falloff::default())
}

/// A source's falloff, from its component.
pub fn falloff_of(src: &AudioSource) -> Falloff {
    Falloff { min_distance: src.min_distance, max_distance: src.max_distance, pan_width: src.pan_width }
}

/// The gain and pan a source starts with: as it sits now if it is spatial,
/// else none.
pub fn source_start(world: &hecs::World, entity: hecs::Entity) -> (f32, f32) {
    let Ok(src) = world.get::<&AudioSource>(entity) else { return (1.0, 0.0) };
    if !src.spatial {
        return (1.0, 0.0);
    }
    let Some(pos) = entity_position(world, entity) else { return (1.0, 0.0) };
    attenuate(listener(world), pos, &falloff_of(&src))
}

fn entity_position(world: &hecs::World, entity: hecs::Entity) -> Option<Vec2> {
    if let Ok(w) = world.get::<&WorldTransform>(entity) {
        return Some(w.position);
    }
    world.get::<&Transform>(entity).ok().map(|t| t.position)
}

/// One playing source's sound, to set this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceMix {
    pub handle: u64,
    /// The sound's own volume (the source's, times its distance gain) - what
    /// the mixer's track is set to, before its bus.
    pub volume: f32,
    pub pan: f32,
    /// Its low-pass cutoff in hertz, walls being between it and the
    /// listener (0: open).
    pub cutoff: f32,
}

/// What a host does with its `AudioSource` sounds this frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpatialFrame {
    pub mix: Vec<SourceMix>,
    pub stop: Vec<u64>,
}

/// The sounds `AudioSource`s are playing, by entity. One per world, kept by
/// the host.
#[derive(Debug, Default)]
pub struct SpatialAudio {
    playing: HashMap<hecs::Entity, (u64, String)>,
}

impl SpatialAudio {
    pub fn new() -> Self {
        Self::default()
    }

    /// `entity`'s source started `handle` playing `clip`. Returns a handle to
    /// stop: the sound the same entity id was playing before, which after a
    /// scene swap reusing the id is the old scene's loop.
    pub fn started(&mut self, entity: hecs::Entity, handle: u64, clip: &str) -> Option<u64> {
        self.playing.insert(entity, (handle, clip.to_string())).map(|(h, _)| h).filter(|h| *h != handle)
    }

    /// How many sounds are tracked. For tests.
    pub fn len(&self) -> usize {
        self.playing.len()
    }

    pub fn is_empty(&self) -> bool {
        self.playing.is_empty()
    }

    /// This frame: each spatial source's volume and pan, and the sounds to
    /// stop - their entity gone, its source removed or pointed at another
    /// clip. A sound the backend says has ended (`is_playing` false) is
    /// forgotten.
    pub fn update(&mut self, world: &hecs::World, is_playing: impl Fn(u64) -> bool) -> SpatialFrame {
        let mut frame = SpatialFrame::default();
        let ear = listener(world);
        // Walls are looked for only when some source listens for them.
        let walls = self
            .playing
            .keys()
            .any(|e| world.get::<&AudioSource>(*e).is_ok_and(|s| s.spatial && (s.occlusion > 0.0 || s.muffle > 0.0)))
            .then(|| crate::ecs::outlines::ColliderOutlines::gather(world));
        let ear_entity = listener_entity(world);
        self.playing.retain(|entity, (handle, clip)| {
            if !is_playing(*handle) {
                return false;
            }
            let src = world.get::<&AudioSource>(*entity).ok().filter(|s| s.clip_id == *clip);
            let Some(src) = src else {
                frame.stop.push(*handle);
                return false;
            };
            if src.spatial
                && let Some(pos) = entity_position(world, *entity)
            {
                let (gain, pan) = attenuate(ear, pos, &falloff_of(&src));
                let (wall_gain, cutoff) = match &walls {
                    Some(o) if src.occlusion > 0.0 || src.muffle > 0.0 => {
                        let skip: Vec<hecs::Entity> = std::iter::once(*entity).chain(ear_entity).collect();
                        occlude(o.walls_between(ear, pos, &skip), src.occlusion, src.muffle)
                    }
                    _ => (1.0, 0.0),
                };
                frame.mix.push(SourceMix { handle: *handle, volume: src.volume * gain * wall_gain, pan, cutoff });
            }
            true
        });
        frame.mix.sort_by_key(|m| m.handle);
        frame.stop.sort_unstable();
        frame
    }

    /// Every tracked sound, forgotten - after the host stopped them all.
    pub fn clear(&mut self) {
        self.playing.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_is_loud_far_is_silent_and_the_side_is_heard() {
        let f = Falloff { min_distance: 100.0, max_distance: 500.0, pan_width: 200.0 };
        let ear = Vec2::ZERO;
        assert_eq!(attenuate(ear, Vec2::new(50.0, 0.0), &f), (1.0, 0.25));
        assert_eq!(attenuate(ear, Vec2::new(0.0, 600.0), &f).0, 0.0);
        let (mid, _) = attenuate(ear, Vec2::new(0.0, 300.0), &f);
        assert!((mid - 0.25).abs() < 1e-6, "halfway out, a quarter: {mid}");
        assert_eq!(attenuate(ear, Vec2::new(-900.0, 0.0), &f).1, -1.0);
        assert_eq!(attenuate(ear, Vec2::new(0.0, -50.0), &f).1, 0.0, "straight below is centred");
    }
}
