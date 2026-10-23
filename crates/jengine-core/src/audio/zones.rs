//! Audio zones (Oct 2026; Unity's Reverb Zones, and the underwater muffle
//! every game with water wants): a box that gives a bus effects while the
//! listener is inside it - a cave's echo, the dull hush under the surface.
//!
//! The listener is the one positional audio uses ([`super::spatial::listener`]).
//! A zone comes in over `fade` world units inside its edge, so walking
//! through a doorway turns the echo up rather than switching it on; the
//! low-pass slides in hertz on a log scale, which is how a muffle is heard.
//! Several zones on one bus combine as [`BusEffects::combine`] does - the
//! lowest cutoff, the wettest reverb.
//!
//! Hosts need nothing new: [`apply`] runs inside the spatial step every host
//! already makes each frame, and the mixer's bus plan carries the result to
//! the backend through `sync_mixer`.

use std::collections::HashMap;

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::mixer::{AudioMixer, BusEffects, SFX_BUS};
use crate::ecs::components::{Transform, WorldTransform};

fn default_size() -> [f32; 2] {
    [400.0, 300.0]
}
fn default_bus() -> String {
    SFX_BUS.to_string()
}
fn default_room() -> f32 {
    0.5
}
fn default_fade() -> f32 {
    80.0
}
fn default_echo_time() -> f32 {
    0.35
}
fn default_echo_feedback() -> f32 {
    0.4
}

/// `audio_zone`: while the listener is inside this box (`size`, turned and
/// scaled with the entity), `bus` gets these effects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioZone {
    #[serde(default = "default_size")]
    pub size: [f32; 2],
    /// The bus whose sounds it changes - `master` for everything.
    #[serde(default = "default_bus")]
    pub bus: String,
    /// Low-pass cutoff in hertz at the zone's heart (0: none).
    #[serde(default)]
    pub lowpass: f32,
    /// Reverb mix at its heart, 0..1.
    #[serde(default)]
    pub reverb: f32,
    /// The reverb's room, 0..1.
    #[serde(default = "default_room")]
    pub room: f32,
    /// Echo mix at its heart, 0..1 - a canyon, a well.
    #[serde(default)]
    pub echo: f32,
    /// Seconds between echoes.
    #[serde(default = "default_echo_time")]
    pub echo_time: f32,
    /// How much of each echo returns, 0..0.95.
    #[serde(default = "default_echo_feedback")]
    pub echo_feedback: f32,
    /// How far inside its edge it reaches full strength, in world units.
    #[serde(default = "default_fade")]
    pub fade: f32,
}

impl Default for AudioZone {
    fn default() -> Self {
        Self {
            size: default_size(),
            bus: default_bus(),
            lowpass: 0.0,
            reverb: 0.0,
            room: default_room(),
            echo: 0.0,
            echo_time: default_echo_time(),
            echo_feedback: default_echo_feedback(),
            fade: default_fade(),
        }
    }
}

impl AudioZone {
    /// How far into the zone `p` is, 0 outside or at the edge to 1 at
    /// `fade` in and beyond.
    pub fn weight(&self, centre: Vec2, rotation: f32, scale: Vec2, p: Vec2) -> f32 {
        let local = glam::Mat2::from_angle(-rotation) * (p - centre);
        let half = Vec2::from(self.size) * scale.abs() * 0.5;
        let depth = (half.x - local.x.abs()).min(half.y - local.y.abs());
        if depth <= 0.0 {
            return 0.0;
        }
        if self.fade <= 0.0 { 1.0 } else { (depth / self.fade).min(1.0) }
    }

    /// Its effects at `weight`: the cutoff slid from open on a log scale,
    /// the reverb and echo mixes scaled.
    pub fn effects_at(&self, weight: f32) -> BusEffects {
        let w = weight.clamp(0.0, 1.0);
        let lowpass = match (BusEffects { lowpass: self.lowpass, ..Default::default() }).cutoff() {
            Some(hz) if w >= 1.0 => hz,
            Some(hz) if w > 0.0 => (BusEffects::OPEN.ln() + (hz.ln() - BusEffects::OPEN.ln()) * w).exp(),
            _ => 0.0,
        };
        BusEffects {
            lowpass,
            reverb: self.reverb.clamp(0.0, 1.0) * w,
            room: self.room,
            echo: self.echo.clamp(0.0, 1.0) * w,
            echo_time: self.echo_time,
            echo_feedback: self.echo_feedback,
            ..Default::default()
        }
    }
}

/// What every zone the listener is in gives each bus now.
pub fn zone_effects(world: &hecs::World) -> HashMap<String, BusEffects> {
    let mut out: HashMap<String, BusEffects> = HashMap::new();
    let mut q = world.query::<(&AudioZone, &Transform, Option<&WorldTransform>)>();
    let mut zones = q.iter().peekable();
    if zones.peek().is_none() {
        return out;
    }
    let ear = super::spatial::listener(world);
    for (_, (zone, t, w)) in zones {
        let (centre, rotation, scale) = match w {
            Some(w) => (w.position, w.rotation, w.scale),
            None => (t.position, t.rotation, t.scale),
        };
        let weight = zone.weight(centre, rotation, scale, ear);
        if weight <= 0.0 {
            continue;
        }
        let fx = zone.effects_at(weight);
        out.entry(zone.bus.clone()).and_modify(|e| *e = e.combine(fx)).or_insert(fx);
    }
    out
}

/// The reverb the listener's room gives its `room_reverb` bus: rays cast
/// round it against the colliders, the share that meet a wall within
/// `room_reach` how enclosed it is (the mix, squared so a doorway lets most
/// of it out) and their mean length how big (the room).
pub fn room_effects(world: &hecs::World) -> Option<(String, BusEffects)> {
    let e = super::spatial::listener_entity(world)?;
    let l = world.get::<&super::spatial::AudioListener>(e).ok()?;
    if l.room_reverb.is_empty() {
        return None;
    }
    let ear = super::spatial::listener(world);
    let outlines = crate::ecs::outlines::ColliderOutlines::gather(world);
    const RAYS: usize = 16;
    let reach = l.room_reach.max(1.0);
    let (mut hits, mut total) = (0usize, 0.0f32);
    for k in 0..RAYS {
        let a = k as f32 / RAYS as f32 * std::f32::consts::TAU;
        if let Some(d) = outlines.ray_reach(ear, Vec2::new(a.cos(), a.sin()), reach) {
            hits += 1;
            total += d;
        }
    }
    if hits == 0 {
        return None;
    }
    let enclosed = hits as f32 / RAYS as f32;
    let fx = BusEffects {
        reverb: l.room_mix.clamp(0.0, 1.0) * enclosed * enclosed,
        room: (total / hits as f32 / reach).clamp(0.15, 1.0),
        ..Default::default()
    };
    Some((l.room_reverb.clone(), fx))
}

/// Hand the mixer this frame's zone effects. Called from the spatial step
/// of every host (`systems::apply_spatial_audio`,
/// `WebAudioBackend::apply_spatial`).
pub fn apply(world: &hecs::World, mixer: &mut AudioMixer) {
    let mut fx = zone_effects(world);
    // The listener's room is one zone more.
    if let Some((bus, room)) = room_effects(world) {
        fx.entry(bus).and_modify(|e| *e = e.combine(room)).or_insert(room);
    }
    mixer.set_zone_effects(fx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zone_fades_in_from_its_edge() {
        let z = AudioZone { size: [200.0, 200.0], fade: 50.0, reverb: 0.8, lowpass: 500.0, ..Default::default() };
        let w = |x: f32| z.weight(Vec2::ZERO, 0.0, Vec2::ONE, Vec2::new(x, 0.0));
        assert_eq!(w(150.0), 0.0);
        assert!((w(75.0) - 0.5).abs() < 1e-5);
        assert_eq!(w(0.0), 1.0);
        let half = z.effects_at(0.5);
        assert!((half.reverb - 0.4).abs() < 1e-5);
        // Halfway on a log scale between 20 kHz and 500 Hz.
        assert!((half.lowpass - (20_000.0f32 * 500.0).sqrt()).abs() < 1.0, "{}", half.lowpass);
        assert_eq!(z.effects_at(0.0).cutoff(), None);
    }
}
