//! Audio mixer — backlog 2.4.
//!
//! A small named-bus tree that multiplies volumes from leaf to root before
//! a sound is handed to the backend. The default tree (`master`, with
//! `sfx` and `music` as children) covers the existing engine surface;
//! scripts and tooling can add custom buses with [`AudioMixer::create_bus`].
//!
//! The mixer does NOT own playback; it only computes effective volumes
//! and tracks which playing sound is on which bus so that volume changes
//! mid-playback can be re-applied to in-flight handles by the runtime
//! audio command loop.
//!
//! ## Volume model
//!
//! A sound's output amplitude is **its own volume x its bus's effective
//! volume**. The sound's own volume is what the script asked for
//! (`play_sound(clip, 0.3)`, `set_music_volume(0.6)`) and is kept on its
//! track; a bus change re-applies the product and never overwrites it. The
//! mixer used to re-apply the bare bus volume instead, so moving the master
//! slider snapped a quiet sound or a 0.3 music track to full, and a
//! `set_music_volume` made after that ignored the bus entirely.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// Default bus names. These are pre-populated by [`AudioMixer::default`]
/// and must always exist — [`AudioMixer::remove_bus`] refuses to drop
/// the master bus.
pub const MASTER_BUS: &str = "master";
pub const SFX_BUS: &str = "sfx";
pub const MUSIC_BUS: &str = "music";

/// What a bus's sounds pass through on the way out (Oct 2026; Godot's bus
/// effects, Unity's mixer effects): a three-band EQ, a soft-clip distortion,
/// a low-pass filter, a compressor, an echo and a reverb, in that order.
/// Each backend gives every bus a real track for them - a kira sub-track, a
/// Web Audio node chain - nested as the buses are, so an effect on `master`
/// reaches everything. The numbers each effect turns into (gains, drive,
/// threshold) are worked out here, once, so the two backends agree.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BusEffects {
    /// Low-pass cutoff in hertz: sounds above it are muffled. 0 (or
    /// [`BusEffects::OPEN`] and up) is off.
    #[serde(default)]
    pub lowpass: f32,
    /// How much reverb is mixed in, 0..1 (0 is off).
    #[serde(default)]
    pub reverb: f32,
    /// How big the reverb's room is, 0..1: a closet to a cathedral.
    #[serde(default = "default_room")]
    pub room: f32,
    /// How much echo is heard, 0..1 (0 is off).
    #[serde(default)]
    pub echo: f32,
    /// Seconds between one echo and the next, 0.02..2.
    #[serde(default = "default_echo_time")]
    pub echo_time: f32,
    /// How much of each echo comes back as the next, 0..0.95.
    #[serde(default = "default_echo_feedback")]
    pub echo_feedback: f32,
    /// Soft-clip distortion, 0..1 (0 is off): a crackling radio at 0.6.
    #[serde(default)]
    pub distortion: f32,
    /// Compression, 0..1 (0 is off): loud things pulled down toward quiet
    /// ones, and the whole made up again.
    #[serde(default)]
    pub compress: f32,
    /// EQ gains in decibels, -24..24 (0 is flat): a low shelf at
    /// [`BusEffects::EQ_LOW_HZ`], a bell at [`BusEffects::EQ_MID_HZ`], a high
    /// shelf at [`BusEffects::EQ_HIGH_HZ`].
    #[serde(default)]
    pub eq_low: f32,
    #[serde(default)]
    pub eq_mid: f32,
    #[serde(default)]
    pub eq_high: f32,
}

fn default_room() -> f32 {
    0.5
}

fn default_echo_time() -> f32 {
    0.35
}

fn default_echo_feedback() -> f32 {
    0.4
}

impl Default for BusEffects {
    fn default() -> Self {
        Self {
            lowpass: 0.0,
            reverb: 0.0,
            room: default_room(),
            echo: 0.0,
            echo_time: default_echo_time(),
            echo_feedback: default_echo_feedback(),
            distortion: 0.0,
            compress: 0.0,
            eq_low: 0.0,
            eq_mid: 0.0,
            eq_high: 0.0,
        }
    }
}

/// [`BusEffects::distort`] for an amount, without a `BusEffects` - what the
/// native effect calls per sample.
pub fn distort_sample(x: f32, amount: f32) -> f32 {
    let a = amount.clamp(0.0, 1.0);
    if a <= 0.0 {
        return x;
    }
    let k = 1.0 + 30.0 * a;
    a * (k * x).tanh() / k.tanh() + (1.0 - a) * x
}

/// A bus compressor's settings, from [`BusEffects::compressor`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompressorSettings {
    /// Level above which it acts, in decibels (0 is full scale).
    pub threshold_db: f32,
    /// How much a level over the threshold is reduced: 4 means 4 dB over
    /// comes out 1 dB over.
    pub ratio: f32,
    pub attack: f32,
    pub release: f32,
    /// Gain added after, in decibels, so compressing does not only quieten.
    pub makeup_db: f32,
}

impl BusEffects {
    /// A cutoff at or above this lets everything through.
    pub const OPEN: f32 = 20_000.0;
    /// The longest echo, in seconds - what a backend's delay line holds.
    pub const MAX_ECHO_TIME: f32 = 2.0;
    pub const EQ_LOW_HZ: f32 = 250.0;
    pub const EQ_MID_HZ: f32 = 1000.0;
    /// The mid bell's width (Q).
    pub const EQ_MID_Q: f32 = 0.8;
    pub const EQ_HIGH_HZ: f32 = 4000.0;
    /// How far each EQ band reaches, in decibels either way.
    pub const EQ_RANGE: f32 = 24.0;

    /// The echo's dry and wet gains: an equal-power crossfade by `echo`, as
    /// kira's effects mix.
    pub fn echo_gains(&self) -> (f32, f32) {
        let m = self.echo.clamp(0.0, 1.0);
        ((1.0 - m).sqrt(), m.sqrt())
    }

    /// Seconds between echoes, within what a delay line holds.
    pub fn echo_delay(&self) -> f32 {
        self.echo_time.clamp(0.02, Self::MAX_ECHO_TIME)
    }

    /// How much of each echo returns, as an amplitude.
    pub fn echo_return(&self) -> f32 {
        self.echo_feedback.clamp(0.0, 0.95)
    }

    /// What the distortion does to one sample: driven into a `tanh` that
    /// keeps full scale at full scale (`tanh(k x) / tanh(k)`, `k` up to 31),
    /// mixed with the clean sample by `distortion` - linearly, since the two
    /// are the same signal and an equal-power mix of them would overshoot.
    /// The native effect runs this per sample and the browsers' wave shaper
    /// is drawn from it, so the two crackle alike.
    pub fn distort(&self, x: f32) -> f32 {
        distort_sample(x, self.distortion)
    }

    /// The compressor at `compress`: a threshold falling to -30 dB and a
    /// ratio rising to 8:1, with half the reduction made up again.
    pub fn compressor(&self) -> CompressorSettings {
        let c = self.compress.clamp(0.0, 1.0);
        let threshold_db = -30.0 * c;
        let ratio = 1.0 + 7.0 * c;
        CompressorSettings {
            threshold_db,
            ratio,
            attack: 0.005,
            release: 0.15,
            makeup_db: -threshold_db * (1.0 - 1.0 / ratio) * 0.5,
        }
    }

    /// The three EQ gains in decibels, within range.
    pub fn eq_gains(&self) -> [f32; 3] {
        let r = Self::EQ_RANGE;
        [self.eq_low.clamp(-r, r), self.eq_mid.clamp(-r, r), self.eq_high.clamp(-r, r)]
    }

    /// The cutoff, if the filter is on.
    pub fn cutoff(&self) -> Option<f32> {
        (self.lowpass > 0.0 && self.lowpass < Self::OPEN).then_some(self.lowpass.max(20.0))
    }

    /// Two sets of effects at once - a script's and a zone's: the lower
    /// cutoff, the wetter reverb and its room, the wetter echo and its
    /// timing, the stronger distortion and compression, the EQs added.
    pub fn combine(self, other: BusEffects) -> BusEffects {
        let lowpass = match (self.cutoff(), other.cutoff()) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => 0.0,
        };
        let (reverb, room) = if other.reverb > self.reverb { (other.reverb, other.room) } else { (self.reverb, self.room) };
        let echo = if other.echo > self.echo { other } else { self };
        let r = Self::EQ_RANGE;
        BusEffects {
            lowpass,
            reverb,
            room,
            echo: echo.echo,
            echo_time: echo.echo_time,
            echo_feedback: echo.echo_feedback,
            distortion: self.distortion.max(other.distortion),
            compress: self.compress.max(other.compress),
            eq_low: (self.eq_low + other.eq_low).clamp(-r, r),
            eq_mid: (self.eq_mid + other.eq_mid).clamp(-r, r),
            eq_high: (self.eq_high + other.eq_high).clamp(-r, r),
        }
    }

    /// Whether `other` sounds different enough to send to a backend: a
    /// cutoff a percent off, a mix half a percent, an EQ a tenth of a
    /// decibel.
    pub fn differs(&self, other: &BusEffects) -> bool {
        let cut = match (self.cutoff(), other.cutoff()) {
            (Some(a), Some(b)) => (a.ln() - b.ln()).abs() > 0.01,
            (None, None) => false,
            _ => true,
        };
        let near = |a: f32, b: f32, by: f32| (a - b).abs() <= by;
        cut || !near(self.reverb, other.reverb, 0.005)
            || !near(self.room, other.room, 0.01)
            || !near(self.echo, other.echo, 0.005)
            || !near(self.echo_time, other.echo_time, 0.002)
            || !near(self.echo_feedback, other.echo_feedback, 0.005)
            || !near(self.distortion, other.distortion, 0.005)
            || !near(self.compress, other.compress, 0.005)
            || !near(self.eq_low, other.eq_low, 0.1)
            || !near(self.eq_mid, other.eq_mid, 0.1)
            || !near(self.eq_high, other.eq_high, 0.1)
    }
}

/// What a backend has to do to its bus tracks, from
/// [`AudioMixer::take_bus_plan`]: every bus listed with its parent and its
/// effects, parents before children. `tree` is set when buses were added or
/// removed, and then every bus is listed - a backend drops what is not.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BusPlan {
    pub tree: bool,
    pub buses: Vec<(String, Option<String>, BusEffects)>,
}

fn one() -> f32 {
    1.0
}

/// One bus of a project's mixer (`settings.audio_mixer` in `project.jengine`,
/// the editor's Project Settings -> Audio): its parent, volume, mute and
/// effects. See [`MixerLayout`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BusLayout {
    pub name: String,
    /// The bus it feeds; none for `master`, and `master` when absent for any
    /// other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default = "one")]
    pub volume: f32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
    #[serde(default)]
    pub effects: BusEffects,
}

/// A project's mixer (Oct 2026; Godot's audio bus layout, Unity's
/// AudioMixer asset): the buses a game starts with, before any script
/// touches them - parents first. Until then a bus existed only once a
/// script had made it with `Audio.create_bus`, and its effects only once a
/// script had set them, so a mix could not be authored, only coded.
///
/// Every host reads it from `project.jengine` where it reads the rest of
/// its settings and hands it to [`AudioMixer::apply_layout`]
/// (`mixer_layout_reaches_every_runtime`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MixerLayout {
    #[serde(default)]
    pub buses: Vec<BusLayout>,
}

impl MixerLayout {
    pub fn is_empty(&self) -> bool {
        self.buses.is_empty()
    }

    /// `settings.audio_mixer` of a parsed `project.jengine`, if it has one.
    pub fn from_project(project: &serde_json::Value) -> Option<Self> {
        Self::from_settings(project.get("settings")?)
    }

    /// `audio_mixer` of a project's `settings` object, if it has one.
    pub fn from_settings(settings: &serde_json::Value) -> Option<Self> {
        let v = settings.get("audio_mixer")?;
        match serde_json::from_value::<MixerLayout>(v.clone()) {
            Ok(layout) => Some(layout),
            Err(e) => {
                log::warn!("[audio] settings.audio_mixer is not a mixer layout: {e}");
                None
            }
        }
    }
}

/// A single mixer bus. Volumes are linear amplitudes in `0.0..=1.0`;
/// the backend converts to decibels at apply time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioBus {
    pub name: String,
    /// Linear amplitude (0.0 = silent, 1.0 = unity).
    pub volume: f32,
    /// Parent bus name; `None` for the master root.
    pub parent: Option<String>,
    /// If true, the bus and all children silence.
    #[serde(default)]
    pub muted: bool,
    /// If true, all non-solo buses silence (any solo on the tree
    /// activates the global solo-isolate behaviour).
    #[serde(default)]
    pub solo: bool,
    /// Its effects as a script set them (`Audio.set_bus_effect`); an
    /// `audio_zone` the listener is in adds to them.
    #[serde(default)]
    pub effects: BusEffects,
}

impl AudioBus {
    pub fn new(name: impl Into<String>, parent: Option<&str>, volume: f32) -> Self {
        Self {
            name: name.into(),
            volume: volume.clamp(0.0, 1.0),
            parent: parent.map(str::to_string),
            muted: false,
            solo: false,
            effects: BusEffects::default(),
        }
    }
}

/// One playing sound as the mixer sees it.
#[derive(Debug, Clone)]
struct Track {
    bus: String,
    /// The sound's own volume, before the bus chain is applied.
    volume: f32,
}

/// Mixer state.
///
/// Lives on [`crate::scripting::api::ScriptServices`]. The bridge
/// translates script-side bus changes into `BridgeCommand::Audio*`
/// variants that `route_bridge_commands` applies directly here.
/// [`AudioMixer::dirty_buses`] is drained by the audio command loop
/// each frame to re-apply effective volume on in-flight tracks.
#[derive(Debug, Clone)]
pub struct AudioMixer {
    buses: HashMap<String, AudioBus>,
    /// Every sound a runtime is still able to address, keyed by its
    /// `SoundHandle.0`. Music registers under [`MUSIC_BUS`]; one-shots default
    /// to [`SFX_BUS`] unless an `AudioSource.bus_id` overrides it.
    ///
    /// Runtimes prune finished sounds with [`AudioMixer::retain_tracks`];
    /// without that every `play_sound` of the session stayed here.
    tracks: HashMap<u64, Track>,
    /// Buses whose effective volume changed since the last drain. The
    /// audio command loop calls [`AudioMixer::take_dirty`] each frame.
    dirty_buses: HashSet<String>,
    /// What the `audio_zone`s the listener is in add to each bus this frame.
    zone_effects: HashMap<String, BusEffects>,
    /// The effects each bus's backend track was last given.
    applied: HashMap<String, BusEffects>,
    /// Buses were added or removed since the backend last heard.
    tree_changed: bool,
}

impl Default for AudioMixer {
    fn default() -> Self {
        let mut buses = HashMap::new();
        buses.insert(MASTER_BUS.to_string(), AudioBus::new(MASTER_BUS, None, 1.0));
        buses.insert(SFX_BUS.to_string(), AudioBus::new(SFX_BUS, Some(MASTER_BUS), 1.0));
        buses.insert(
            MUSIC_BUS.to_string(),
            AudioBus::new(MUSIC_BUS, Some(MASTER_BUS), 1.0),
        );
        Self {
            buses,
            tracks: HashMap::new(),
            dirty_buses: HashSet::new(),
            zone_effects: HashMap::new(),
            applied: HashMap::new(),
            tree_changed: true,
        }
    }
}

impl AudioMixer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn buses(&self) -> &HashMap<String, AudioBus> {
        &self.buses
    }

    pub fn get(&self, name: &str) -> Option<&AudioBus> {
        self.buses.get(name)
    }

    /// Create a new bus. Returns `false` if the name already exists or
    /// the parent is unknown. Volume is clamped to `0.0..=1.0`.
    pub fn create_bus(&mut self, name: &str, parent: Option<&str>, volume: f32) -> bool {
        if name.is_empty() || self.buses.contains_key(name) {
            return false;
        }
        if let Some(p) = parent
            && !self.buses.contains_key(p)
        {
            return false;
        }
        self.buses
            .insert(name.to_string(), AudioBus::new(name, parent, volume));
        // No tracks yet, but mark dirty so a future track on this bus
        // picks up the right effective volume on its first apply.
        self.dirty_buses.insert(name.to_string());
        self.tree_changed = true;
        true
    }

    /// Remove a bus. Cannot remove [`MASTER_BUS`]. Children of the
    /// removed bus are re-parented to its parent (or `None` if removing
    /// a top-level bus other than master).
    pub fn remove_bus(&mut self, name: &str) -> bool {
        if name == MASTER_BUS {
            return false;
        }
        let Some(removed) = self.buses.remove(name) else {
            return false;
        };
        // Reparent children.
        for bus in self.buses.values_mut() {
            if bus.parent.as_deref() == Some(name) {
                bus.parent = removed.parent.clone();
                self.dirty_buses.insert(bus.name.clone());
            }
        }
        // Tracks on the removed bus fall back to master.
        for track in self.tracks.values_mut() {
            if track.bus == name {
                MASTER_BUS.clone_into(&mut track.bus);
            }
        }
        self.dirty_buses.insert(MASTER_BUS.to_string());
        self.tree_changed = true;
        true
    }

    /// Set the mixer to a project's layout ([`MixerLayout`]): each bus made if
    /// it is missing (under `master` when its parent is unknown), then given
    /// the layout's volume, mute and effects - every effect through
    /// [`Self::set_effect`], so a layout is clamped as a script's values are.
    /// Buses the layout does not name are left alone. Applying it again (a
    /// new play session in the editor) puts back what scripts changed.
    pub fn apply_layout(&mut self, layout: &MixerLayout) {
        for bus in &layout.buses {
            if bus.name.is_empty() {
                continue;
            }
            if !self.buses.contains_key(&bus.name) {
                let parent = bus.parent.as_deref().filter(|p| self.buses.contains_key(*p)).unwrap_or(MASTER_BUS);
                if bus.parent.as_deref().is_some_and(|p| p != parent) {
                    log::warn!("[audio] bus '{}' names parent '{}', which is not a bus; under master", bus.name, bus.parent.as_deref().unwrap_or(""));
                }
                self.create_bus(&bus.name, Some(parent), bus.volume);
            }
            self.set_volume(&bus.name, bus.volume);
            self.set_muted(&bus.name, bus.muted);
            let fx = bus.effects;
            for (effect, value) in [
                ("lowpass", fx.lowpass),
                ("reverb", fx.reverb),
                ("room", fx.room),
                ("echo", fx.echo),
                ("echo_time", fx.echo_time),
                ("echo_feedback", fx.echo_feedback),
                ("distortion", fx.distortion),
                ("compress", fx.compress),
                ("eq_low", fx.eq_low),
                ("eq_mid", fx.eq_mid),
                ("eq_high", fx.eq_high),
            ] {
                let _ = self.set_effect(&bus.name, effect, value);
            }
        }
    }

    /// `Audio.set_bus_effect(bus, effect, value)`: `lowpass` (hertz, 0 off),
    /// `reverb` / `room` (0..1), `echo` (0..1), `echo_time` (seconds),
    /// `echo_feedback` (0..0.95), `distortion` / `compress` (0..1) or
    /// `eq_low` / `eq_mid` / `eq_high` (decibels).
    pub fn set_effect(&mut self, bus: &str, effect: &str, value: f32) -> Result<(), String> {
        let b = self.buses.get_mut(bus).ok_or_else(|| format!("there is no audio bus '{bus}'"))?;
        let v = if value.is_finite() { value } else { 0.0 };
        let r = BusEffects::EQ_RANGE;
        let fx = &mut b.effects;
        match effect {
            "lowpass" => fx.lowpass = v.max(0.0),
            "reverb" => fx.reverb = v.clamp(0.0, 1.0),
            "room" => fx.room = v.clamp(0.0, 1.0),
            "echo" => fx.echo = v.clamp(0.0, 1.0),
            "echo_time" => fx.echo_time = v.clamp(0.02, BusEffects::MAX_ECHO_TIME),
            "echo_feedback" => fx.echo_feedback = v.clamp(0.0, 0.95),
            "distortion" => fx.distortion = v.clamp(0.0, 1.0),
            "compress" => fx.compress = v.clamp(0.0, 1.0),
            "eq_low" => fx.eq_low = v.clamp(-r, r),
            "eq_mid" => fx.eq_mid = v.clamp(-r, r),
            "eq_high" => fx.eq_high = v.clamp(-r, r),
            other => {
                return Err(format!(
                    "'{other}' is not a bus effect (lowpass, reverb, room, echo, echo_time, echo_feedback, \
                     distortion, compress, eq_low, eq_mid, eq_high)"
                ));
            }
        }
        Ok(())
    }

    /// A bus's effects as a script set them.
    pub fn effects(&self, bus: &str) -> BusEffects {
        self.buses.get(bus).map(|b| b.effects).unwrap_or_default()
    }

    /// What the zones add this frame, replacing last frame's.
    pub fn set_zone_effects(&mut self, zones: HashMap<String, BusEffects>) {
        self.zone_effects = zones;
    }

    /// A bus's effects as they sound: its own and its zones' together.
    pub fn effective_effects(&self, bus: &str) -> BusEffects {
        let own = self.effects(bus);
        match self.zone_effects.get(bus) {
            Some(z) => own.combine(*z),
            None => own,
        }
    }

    fn depth(&self, name: &str) -> usize {
        let mut d = 0;
        let mut at = self.buses.get(name).and_then(|b| b.parent.clone());
        while let Some(p) = at {
            d += 1;
            if d > 64 {
                break;
            }
            at = self.buses.get(&p).and_then(|b| b.parent.clone());
        }
        d
    }

    /// What the backend's bus tracks need, if anything changed since the
    /// last call: see [`BusPlan`].
    pub fn take_bus_plan(&mut self) -> Option<BusPlan> {
        let tree = std::mem::take(&mut self.tree_changed);
        let mut names: Vec<String> = self.buses.keys().cloned().collect();
        names.sort_by_key(|n| (self.depth(n), n.clone()));
        let mut buses = Vec::new();
        for name in names {
            let fx = self.effective_effects(&name);
            let changed = self.applied.get(&name).is_none_or(|a| a.differs(&fx));
            if tree || changed {
                let parent = self.buses.get(&name).and_then(|b| b.parent.clone());
                buses.push((name.clone(), parent, fx));
                self.applied.insert(name, fx);
            }
        }
        if tree {
            let live: HashSet<String> = self.buses.keys().cloned().collect();
            self.applied.retain(|n, _| live.contains(n));
        }
        (tree || !buses.is_empty()).then_some(BusPlan { tree, buses })
    }

    pub fn set_volume(&mut self, name: &str, volume: f32) {
        if let Some(b) = self.buses.get_mut(name) {
            let v = volume.clamp(0.0, 1.0);
            if (b.volume - v).abs() > f32::EPSILON {
                b.volume = v;
                self.mark_subtree_dirty(name);
            }
        }
    }

    pub fn get_volume(&self, name: &str) -> Option<f32> {
        self.buses.get(name).map(|b| b.volume)
    }

    pub fn set_muted(&mut self, name: &str, muted: bool) {
        if let Some(b) = self.buses.get_mut(name)
            && b.muted != muted
        {
            b.muted = muted;
            self.mark_subtree_dirty(name);
        }
    }

    pub fn is_muted(&self, name: &str) -> bool {
        self.buses.get(name).is_some_and(|b| b.muted)
    }

    pub fn set_solo(&mut self, name: &str, solo: bool) {
        if let Some(b) = self.buses.get_mut(name)
            && b.solo != solo
        {
            b.solo = solo;
            // Solo affects the whole tree, not just the subtree.
            for n in self.buses.keys().cloned().collect::<Vec<_>>() {
                self.dirty_buses.insert(n);
            }
        }
    }

    pub fn is_solo(&self, name: &str) -> bool {
        self.buses.get(name).is_some_and(|b| b.solo)
    }

    /// Effective amplitude for `bus`. Returns 0 if the bus or any
    /// ancestor is muted, if any other bus is soloed and this one
    /// isn't on the solo path, or if the bus is unknown.
    pub fn effective_volume(&self, bus: &str) -> f32 {
        let Some(start) = self.buses.get(bus) else {
            return 0.0;
        };
        // Solo-isolate: when any bus has `solo == true`, only the solo
        // chain (bus + its ancestors) and any descendant of a solo bus
        // play. Implemented as: silence if there's any solo on the tree
        // AND the queried bus is not on the solo-allow-list.
        let any_solo = self.buses.values().any(|b| b.solo);
        if any_solo && !self.is_in_solo_path(bus) {
            return 0.0;
        }
        let mut acc = 1.0_f32;
        let mut cursor: Option<&AudioBus> = Some(start);
        while let Some(b) = cursor {
            if b.muted {
                return 0.0;
            }
            acc *= b.volume;
            cursor = b.parent.as_deref().and_then(|p| self.buses.get(p));
        }
        acc.clamp(0.0, 1.0)
    }

    /// True if `bus` is one of the solo'd buses, an ancestor of one,
    /// or a descendant of one. Used by solo-isolate routing.
    fn is_in_solo_path(&self, bus: &str) -> bool {
        // Ancestor chain.
        let mut cursor = self.buses.get(bus);
        while let Some(b) = cursor {
            if b.solo {
                return true;
            }
            cursor = b.parent.as_deref().and_then(|p| self.buses.get(p));
        }
        // Descendant: walk all solo'd buses and check if `bus` is in
        // their ancestor chain via parent links.
        for solo_bus in self.buses.values().filter(|b| b.solo) {
            let mut cur = self.buses.get(&solo_bus.name);
            while let Some(b) = cur {
                if b.name == bus {
                    return true;
                }
                cur = b.parent.as_deref().and_then(|p| self.buses.get(p));
            }
        }
        false
    }

    /// Mark `bus` and every descendant dirty.
    fn mark_subtree_dirty(&mut self, bus: &str) {
        let descendants: Vec<String> = self
            .buses
            .values()
            .filter(|b| self.is_descendant_of(&b.name, bus))
            .map(|b| b.name.clone())
            .collect();
        self.dirty_buses.insert(bus.to_string());
        for d in descendants {
            self.dirty_buses.insert(d);
        }
    }

    fn is_descendant_of(&self, child: &str, ancestor: &str) -> bool {
        if child == ancestor {
            return false;
        }
        let mut cursor = self.buses.get(child);
        while let Some(b) = cursor {
            if b.parent.as_deref() == Some(ancestor) {
                return true;
            }
            cursor = b.parent.as_deref().and_then(|p| self.buses.get(p));
        }
        false
    }

    /// The name to route a sound through: `requested` if that bus exists,
    /// otherwise `default`.
    ///
    /// An unknown bus has an effective volume of zero, so a typo in an
    /// `AudioSource.bus_id` - or a bus the script has not created yet - would
    /// silence the sound with no clue why. Falling back keeps it audible and
    /// says so.
    pub fn route(&self, requested: Option<&str>, default: &str) -> String {
        match requested {
            Some(bus) if self.buses.contains_key(bus) => bus.to_string(),
            Some(bus) => {
                log::warn!("[audio] bus '{bus}' does not exist - playing on '{default}'");
                default.to_string()
            }
            None => default.to_string(),
        }
    }

    /// Output amplitude for a sound of `volume` on `bus`.
    pub fn output_volume(&self, bus: &str, volume: f32) -> f32 {
        volume.max(0.0) * self.effective_volume(bus)
    }

    /// Register a playing sound on `bus` at its own `volume`, returning the
    /// amplitude to hand the backend.
    ///
    /// Registering an existing handle replaces it - a host that reuses one
    /// handle for "the music" re-registers it on every `play_music`.
    pub fn register_track(&mut self, sound_handle: u64, bus: impl Into<String>, volume: f32) -> f32 {
        let bus = bus.into();
        let out = self.output_volume(&bus, volume);
        self.tracks.insert(sound_handle, Track { bus, volume });
        out
    }

    /// Drop the registration for a sound (call on stop or natural end).
    pub fn unregister_track(&mut self, sound_handle: u64) {
        self.tracks.remove(&sound_handle);
    }

    /// Look up the bus a sound was registered on.
    pub fn track_bus(&self, sound_handle: u64) -> Option<&str> {
        self.tracks.get(&sound_handle).map(|t| t.bus.as_str())
    }

    /// A sound's own volume, before the bus chain.
    pub fn track_volume(&self, sound_handle: u64) -> Option<f32> {
        self.tracks.get(&sound_handle).map(|t| t.volume)
    }

    /// Change a sound's own volume, returning the amplitude to hand the
    /// backend, or `None` if the sound is not tracked.
    pub fn set_track_volume(&mut self, sound_handle: u64, volume: f32) -> Option<f32> {
        let bus = {
            let track = self.tracks.get_mut(&sound_handle)?;
            track.volume = volume;
            track.bus.clone()
        };
        Some(self.output_volume(&bus, volume))
    }

    /// Keep only the tracks for which `keep(handle)` is true - runtimes pass
    /// "is the backend still playing this".
    pub fn retain_tracks(&mut self, mut keep: impl FnMut(u64) -> bool) {
        self.tracks.retain(|handle, _| keep(*handle));
    }

    /// Drain the dirty set. Returns the buses whose effective volume
    /// may have changed since the last call. Prefer
    /// [`AudioMixer::take_volume_updates`], which also works out what each
    /// affected sound should now play at.
    pub fn take_dirty(&mut self) -> HashSet<String> {
        std::mem::take(&mut self.dirty_buses)
    }

    /// Drain the dirty set and return `(sound_handle, amplitude)` for every
    /// tracked sound on an affected bus - exactly the `set_volume` calls a
    /// runtime has to make.
    pub fn take_volume_updates(&mut self) -> Vec<(u64, f32)> {
        if self.dirty_buses.is_empty() {
            return Vec::new();
        }
        let dirty = self.take_dirty();
        self.tracks
            .iter()
            .filter(|(_, t)| dirty.contains(&t.bus))
            .map(|(h, t)| (*h, self.output_volume(&t.bus, t.volume)))
            .collect()
    }

    /// All tracked `(sound_handle, bus_name)` pairs.
    pub fn tracks(&self) -> impl Iterator<Item = (u64, &str)> {
        self.tracks.iter().map(|(h, t)| (*h, t.bus.as_str()))
    }

    /// Forget every tracked sound, e.g. after the backend stopped them all.
    pub fn clear_tracks(&mut self) {
        self.tracks.clear();
    }

    /// A snapshot of every bus, used to expose read-only state to
    /// scripts each frame. Cheap — bus tree is small.
    pub fn snapshot(&self) -> HashMap<String, AudioBus> {
        self.buses.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tree_has_master_sfx_music() {
        let m = AudioMixer::new();
        assert!(m.get(MASTER_BUS).is_some());
        assert_eq!(m.get(SFX_BUS).unwrap().parent.as_deref(), Some(MASTER_BUS));
        assert_eq!(m.get(MUSIC_BUS).unwrap().parent.as_deref(), Some(MASTER_BUS));
    }

    #[test]
    fn effective_volume_multiplies_through_chain() {
        let mut m = AudioMixer::new();
        m.set_volume(MASTER_BUS, 0.5);
        m.set_volume(SFX_BUS, 0.5);
        // 0.5 * 0.5 = 0.25
        assert!((m.effective_volume(SFX_BUS) - 0.25).abs() < 1e-6);
        // music is on master only.
        assert!((m.effective_volume(MUSIC_BUS) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mute_silences_subtree() {
        let mut m = AudioMixer::new();
        m.set_muted(MASTER_BUS, true);
        assert_eq!(m.effective_volume(SFX_BUS), 0.0);
        assert_eq!(m.effective_volume(MUSIC_BUS), 0.0);
        m.set_muted(MASTER_BUS, false);
        assert!(m.effective_volume(SFX_BUS) > 0.0);
    }

    #[test]
    fn solo_isolates_to_chain() {
        let mut m = AudioMixer::new();
        m.set_solo(SFX_BUS, true);
        // sfx and master (ancestor of soloed bus) play.
        assert!(m.effective_volume(SFX_BUS) > 0.0);
        assert!(m.effective_volume(MASTER_BUS) > 0.0);
        // music is silenced.
        assert_eq!(m.effective_volume(MUSIC_BUS), 0.0);
    }

    #[test]
    fn create_and_remove_custom_bus() {
        let mut m = AudioMixer::new();
        assert!(m.create_bus("voice", Some(MASTER_BUS), 0.8));
        // duplicate fails
        assert!(!m.create_bus("voice", Some(MASTER_BUS), 1.0));
        // unknown parent fails
        assert!(!m.create_bus("ambient", Some("nowhere"), 1.0));
        // can't remove master
        assert!(!m.remove_bus(MASTER_BUS));
        // remove reparents children & moves tracks to master
        m.register_track(42, "voice", 1.0);
        assert!(m.remove_bus("voice"));
        assert_eq!(m.track_bus(42), Some(MASTER_BUS));
    }

    #[test]
    fn volume_change_marks_subtree_dirty() {
        let mut m = AudioMixer::new();
        let _ = m.take_dirty(); // drain initial dirties
        m.set_volume(MASTER_BUS, 0.5);
        let d = m.take_dirty();
        assert!(d.contains(MASTER_BUS));
        assert!(d.contains(SFX_BUS));
        assert!(d.contains(MUSIC_BUS));
        // second drain is empty
        assert!(m.take_dirty().is_empty());
    }

    #[test]
    fn unknown_bus_is_silent_but_safe() {
        let m = AudioMixer::new();
        assert_eq!(m.effective_volume("does-not-exist"), 0.0);
        assert!(m.get_volume("does-not-exist").is_none());
    }

    #[test]
    fn track_register_unregister() {
        let mut m = AudioMixer::new();
        m.register_track(1, SFX_BUS, 1.0);
        m.register_track(2, MUSIC_BUS, 1.0);
        assert_eq!(m.track_bus(1), Some(SFX_BUS));
        m.unregister_track(1);
        assert!(m.track_bus(1).is_none());
        assert_eq!(m.track_bus(2), Some(MUSIC_BUS));
    }

    #[test]
    fn a_bus_change_keeps_each_sounds_own_volume() {
        // The bug: bus changes re-applied the bare bus volume, so a sound
        // started at 0.3 jumped to full the moment any slider moved.
        let mut m = AudioMixer::new();
        assert!((m.register_track(1, MUSIC_BUS, 0.3) - 0.3).abs() < 1e-6);
        let _ = m.take_volume_updates();

        m.set_volume(MASTER_BUS, 0.5);
        let updates = m.take_volume_updates();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, 1);
        assert!((updates[0].1 - 0.15).abs() < 1e-6, "got {}", updates[0].1);
        assert_eq!(m.track_volume(1), Some(0.3), "the sound's own volume was overwritten");
    }

    #[test]
    fn setting_a_sounds_volume_still_goes_through_its_bus() {
        let mut m = AudioMixer::new();
        m.set_volume(MUSIC_BUS, 0.5);
        m.register_track(7, MUSIC_BUS, 1.0);
        assert_eq!(m.set_track_volume(7, 0.6), Some(0.3));
        m.set_muted(MUSIC_BUS, true);
        assert_eq!(m.set_track_volume(7, 0.9), Some(0.0), "a muted bus must stay muted");
        assert_eq!(m.set_track_volume(99, 0.5), None, "untracked sounds report None");
    }

    #[test]
    fn updates_only_cover_sounds_on_affected_buses() {
        let mut m = AudioMixer::new();
        m.register_track(1, SFX_BUS, 1.0);
        m.register_track(2, MUSIC_BUS, 1.0);
        let _ = m.take_volume_updates();
        m.set_volume(SFX_BUS, 0.25);
        assert_eq!(m.take_volume_updates(), vec![(1, 0.25)]);
        assert!(m.take_volume_updates().is_empty(), "updates drain");
    }

    #[test]
    fn finished_sounds_can_be_pruned() {
        let mut m = AudioMixer::new();
        for h in 0..100 {
            m.register_track(h, SFX_BUS, 1.0);
        }
        m.retain_tracks(|h| h >= 95);
        assert_eq!(m.tracks().count(), 5);
    }

    #[test]
    fn an_unknown_bus_falls_back_rather_than_silencing() {
        let mut m = AudioMixer::new();
        assert_eq!(m.route(Some("nope"), SFX_BUS), SFX_BUS);
        assert_eq!(m.route(None, MUSIC_BUS), MUSIC_BUS);
        assert!(m.create_bus("voice", Some(MASTER_BUS), 1.0));
        assert_eq!(m.route(Some("voice"), SFX_BUS), "voice");
    }
}
