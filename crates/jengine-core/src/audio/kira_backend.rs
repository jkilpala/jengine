//! Kira-based native audio backend.
//!
//! Uses kira 0.12 for high-quality audio playback on desktop platforms.

use std::collections::HashMap;
use std::io::Cursor;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::Result;
use kira::effect::compressor::{CompressorBuilder, CompressorHandle};
use kira::effect::eq_filter::{EqFilterBuilder, EqFilterHandle, EqFilterKind};
use kira::effect::filter::{FilterBuilder, FilterHandle};
use kira::effect::reverb::{ReverbBuilder, ReverbHandle};
use kira::effect::{Effect, EffectBuilder};
use kira::info::Info;
use kira::{Decibels, Frame};
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings};
use kira::track::{MainTrackBuilder, TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Mix, Tween};

use super::mixer::{BusEffects, BusPlan, MASTER_BUS};
use super::{AudioBackend, ClipHandle, PlaySettings, SoundHandle};

/// One mixer bus as kira tracks see it: its own sub-track (none for
/// `master`, which is the main track) and the effects every bus carries
/// ([`BusEffects`]'s order). They are built neutral - everything dry or flat -
/// because kira adds effects only when a track is built.
struct KiraBus {
    track: Option<TrackHandle>,
    parent: Option<String>,
    fx: BusChain,
}

/// A bus's effect handles.
struct BusChain {
    eq: [EqFilterHandle; 3],
    distortion: Arc<AtomicU32>,
    filter: FilterHandle,
    compressor: CompressorHandle,
    echo: Arc<EchoParams>,
    reverb: ReverbHandle,
}

/// Add a bus's effects to a track builder - a main track's or a
/// sub-track's, which are different types with the same `add_effect`.
macro_rules! bus_chain {
    ($builder:expr) => {{
        let b = $builder;
        let eq = [
            b.add_effect(EqFilterBuilder::new(EqFilterKind::LowShelf, BusEffects::EQ_LOW_HZ as f64, Decibels(0.0), 0.7)),
            b.add_effect(EqFilterBuilder::new(EqFilterKind::Bell, BusEffects::EQ_MID_HZ as f64, Decibels(0.0), BusEffects::EQ_MID_Q as f64)),
            b.add_effect(EqFilterBuilder::new(EqFilterKind::HighShelf, BusEffects::EQ_HIGH_HZ as f64, Decibels(0.0), 0.7)),
        ];
        let distortion = b.add_effect(DriveBuilder);
        let filter = b.add_effect(neutral_filter());
        let compressor = b.add_effect(CompressorBuilder::new().mix(Mix::DRY));
        let echo = b.add_effect(EchoBuilder);
        let reverb = b.add_effect(neutral_reverb());
        BusChain { eq, distortion, filter, compressor, echo, reverb }
    }};
}

/// The echo's settings, shared with the audio thread: kira's own delay fixes
/// its time when the track is built, and a bus's echo time is a setting a
/// script changes. f32 bits in atomics, read once per block.
#[derive(Debug)]
struct EchoParams {
    mix: AtomicU32,
    time: AtomicU32,
    feedback: AtomicU32,
}

impl EchoParams {
    fn set(&self, fx: &BusEffects) {
        self.mix.store(fx.echo.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.time.store(fx.echo_delay().to_bits(), Ordering::Relaxed);
        self.feedback.store(fx.echo_return().to_bits(), Ordering::Relaxed);
    }

    fn get(&self) -> (f32, f32, f32) {
        let f = |a: &AtomicU32| f32::from_bits(a.load(Ordering::Relaxed));
        (f(&self.mix), f(&self.time), f(&self.feedback))
    }
}

/// The distortion: [`super::mixer::distort_sample`] on every sample, its
/// amount shared with the audio thread as f32 bits and glided toward.
struct DriveBuilder;

impl EffectBuilder for DriveBuilder {
    type Handle = Arc<AtomicU32>;

    fn build(self) -> (Box<dyn Effect>, Self::Handle) {
        let amount = Arc::new(AtomicU32::new(0f32.to_bits()));
        (Box::new(Drive { amount: amount.clone(), now: 0.0 }), amount)
    }
}

struct Drive {
    amount: Arc<AtomicU32>,
    now: f32,
}

impl Effect for Drive {
    fn process(&mut self, input: &mut [Frame], _dt: f64, _info: &Info) {
        let want = f32::from_bits(self.amount.load(Ordering::Relaxed));
        if want <= 0.0 && self.now <= 1e-4 {
            self.now = 0.0;
            return;
        }
        for frame in input.iter_mut() {
            self.now += (want - self.now) / 1000.0;
            frame.left = super::mixer::distort_sample(frame.left, self.now);
            frame.right = super::mixer::distort_sample(frame.right, self.now);
        }
    }
}

/// The spectrum's tap (`audio::analysis`): the master track's output,
/// copied as mono into the analyser's ring. Last on the track, so it hears
/// the mix as the speakers do.
struct TapBuilder;

impl EffectBuilder for TapBuilder {
    type Handle = ();

    fn build(self) -> (Box<dyn Effect>, Self::Handle) {
        (Box::new(Tap), ())
    }
}

struct Tap;

impl Effect for Tap {
    fn process(&mut self, input: &mut [Frame], dt: f64, _info: &Info) {
        let rate = if dt > 0.0 { (1.0 / dt).round() as u32 } else { 48_000 };
        super::analysis::feed(input.iter().map(|f| (f.left + f.right) * 0.5), rate);
    }
}

struct EchoBuilder;

impl EffectBuilder for EchoBuilder {
    type Handle = Arc<EchoParams>;

    fn build(self) -> (Box<dyn Effect>, Self::Handle) {
        let fx = BusEffects::default();
        let params = Arc::new(EchoParams {
            mix: AtomicU32::new(0f32.to_bits()),
            time: AtomicU32::new(fx.echo_delay().to_bits()),
            feedback: AtomicU32::new(fx.echo_return().to_bits()),
        });
        (Box::new(Echo::new(params.clone())), params)
    }
}

/// A delay line whose time can change: what comes in is heard again
/// `time` later, `feedback` of it again after that, mixed in by equal power.
/// The settings glide sample by sample, so a change does not click.
struct Echo {
    params: Arc<EchoParams>,
    line: Vec<Frame>,
    at: usize,
    rate: f32,
    mix: f32,
    time: f32,
    feedback: f32,
}

impl Echo {
    fn new(params: Arc<EchoParams>) -> Self {
        let (mix, time, feedback) = params.get();
        Self { params, line: Vec::new(), at: 0, rate: 0.0, mix, time, feedback }
    }

    fn size_for(&mut self, sample_rate: u32) {
        self.rate = sample_rate as f32;
        let len = (BusEffects::MAX_ECHO_TIME * self.rate) as usize + 2;
        self.line = vec![Frame::ZERO; len];
        self.at = 0;
    }

    /// The delay line over one block - the effect's whole work, apart from
    /// kira's `Info`, so a test can run it.
    fn run(&mut self, input: &mut [Frame]) {
        let len = self.line.len();
        if len < 3 {
            return;
        }
        let (mix, time, feedback) = self.params.get();
        // About 20 ms to settle at 48 kHz.
        let k = 1.0 / 1000.0;
        for frame in input.iter_mut() {
            self.mix += (mix - self.mix) * k;
            self.time += (time - self.time) * k;
            self.feedback += (feedback - self.feedback) * k;
            let lag = ((self.time * self.rate) as usize).clamp(1, len - 1);
            let heard = self.line[(self.at + len - lag) % len];
            self.line[self.at] = *frame + heard * self.feedback;
            self.at = (self.at + 1) % len;
            let m = self.mix.clamp(0.0, 1.0);
            *frame = *frame * (1.0 - m).sqrt() + heard * m.sqrt();
        }
    }
}

impl Effect for Echo {
    fn init(&mut self, sample_rate: u32, _internal_buffer_size: usize) {
        self.size_for(sample_rate);
    }

    fn on_change_sample_rate(&mut self, sample_rate: u32) {
        self.size_for(sample_rate);
    }

    fn process(&mut self, input: &mut [Frame], _dt: f64, _info: &Info) {
        self.run(input);
    }
}

#[cfg(test)]
mod echo_tests {
    use super::*;

    #[test]
    fn an_echo_comes_back_after_its_time_and_fades() {
        let (_, params) = {
            let fx = BusEffects { echo: 1.0, echo_time: 0.1, echo_feedback: 0.5, ..Default::default() };
            let (effect, params) = EchoBuilder.build();
            drop(effect);
            params.set(&fx);
            ((), params)
        };
        let mut echo = Echo::new(params);
        echo.size_for(100); // 100 samples a second: the echo is 10 later
        let mut block = vec![Frame::ZERO; 60];
        block[0] = Frame::from_mono(1.0);
        echo.run(&mut block);
        let heard: Vec<f32> = block.iter().map(|f| f.left).collect();
        assert!(heard[0].abs() < 1e-6, "fully wet: the clap itself is not heard");
        assert!((heard[10] - 1.0).abs() < 1e-3, "heard again 0.1 s later: {heard:?}");
        assert!((heard[20] - 0.5).abs() < 1e-3, "and again at half: {}", heard[20]);
        assert!((heard[30] - 0.25).abs() < 1e-3);
        assert!(heard.iter().enumerate().all(|(i, v)| i % 10 == 0 || v.abs() < 1e-6), "nothing between");
    }
}

/// The effects a bus's track starts with: a low-pass and a reverb, dry.
fn neutral_filter() -> FilterBuilder {
    FilterBuilder::new().cutoff(BusEffects::OPEN as f64).mix(Mix::DRY)
}

fn neutral_reverb() -> ReverbBuilder {
    ReverbBuilder::new().mix(Mix::DRY)
}

/// How quickly an effect moves to a new setting: short enough to follow a
/// listener walking through a doorway, long enough not to click.
fn glide() -> Tween {
    Tween { duration: std::time::Duration::from_millis(60), ..Default::default() }
}

impl BusChain {
    fn set(&mut self, fx: &BusEffects) {
        for (band, gain) in self.eq.iter_mut().zip(fx.eq_gains()) {
            band.set_gain(Decibels(gain), glide());
        }
        self.distortion.store(fx.distortion.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        match fx.cutoff() {
            Some(hz) => {
                self.filter.set_cutoff(hz as f64, glide());
                self.filter.set_mix(Mix::WET, glide());
            }
            None => {
                self.filter.set_cutoff(BusEffects::OPEN as f64, glide());
                self.filter.set_mix(Mix::DRY, glide());
            }
        }
        let c = fx.compressor();
        self.compressor.set_threshold(c.threshold_db as f64, glide());
        self.compressor.set_ratio(c.ratio as f64, glide());
        self.compressor.set_attack_duration(std::time::Duration::from_secs_f32(c.attack), glide());
        self.compressor.set_release_duration(std::time::Duration::from_secs_f32(c.release), glide());
        self.compressor.set_makeup_gain(Decibels(c.makeup_db), glide());
        self.compressor.set_mix(if fx.compress > 0.0 { Mix::WET } else { Mix::DRY }, glide());
        self.echo.set(fx);
        // A bigger room rings longer and duller.
        let room = fx.room.clamp(0.0, 1.0) as f64;
        self.reverb.set_feedback(0.55 + 0.42 * room, glide());
        self.reverb.set_damping(0.6 - 0.45 * room, glide());
        self.reverb.set_mix(Mix(fx.reverb.clamp(0.0, 1.0)), glide());
    }
}

/// Convert a linear amplitude (0.0–1.0) to kira Decibels.
fn amplitude_to_db(amplitude: f32) -> kira::Decibels {
    if amplitude <= 0.0 {
        kira::Decibels(-100.0)
    } else {
        kira::Decibels(20.0 * amplitude.log10())
    }
}

/// Native audio backend backed by kira.
pub struct KiraAudioBackend {
    manager: AudioManager<DefaultBackend>,
    /// Loaded clip data, keyed by ClipHandle.
    clips: HashMap<u64, StaticSoundData>,
    /// Named clip lookup: clip_id string → handle.
    clip_names: HashMap<String, u64>,
    /// Currently playing sound handles.
    sounds: HashMap<u64, StaticSoundHandle>,
    /// Which clip each live sound came from, so `stop_by_clip` can address
    /// every instance of one sound by name.
    sound_clips: HashMap<u64, u64>,
    next_clip_id: u64,
    next_sound_id: u64,
    /// Sounds `suspend` paused, for `unsuspend` to resume - and only those.
    suspended: Vec<u64>,
    /// The mixer's buses as tracks, by name (`apply_buses`).
    buses: HashMap<String, KiraBus>,
    /// Each clip's name by handle, for its variation (`audio::variation`).
    clip_ids: HashMap<u64, String>,
    /// A filtered sound's own track and low-pass (`PlaySettings::filter`),
    /// kept while it plays.
    filtered: HashMap<u64, (TrackHandle, FilterHandle)>,
    /// Where a transcoded clip's loop starts, in seconds, when not at its
    /// top (`audio::transcode` - a `.song` with an intro).
    clip_loops: HashMap<u64, f64>,
}

impl KiraAudioBackend {
    /// Create a new kira audio backend.
    pub fn new() -> Result<Self> {
        // The main track is `master`; its effects have to be there from the
        // start.
        let mut main = MainTrackBuilder::new();
        let fx = bus_chain!(&mut main);
        main.add_effect(TapBuilder);
        let settings = AudioManagerSettings { main_track_builder: main, ..Default::default() };
        let manager = AudioManager::<DefaultBackend>::new(settings)
            .map_err(|e| anyhow::anyhow!("Failed to create audio manager: {}", e))?;
        let mut buses = HashMap::new();
        buses.insert(MASTER_BUS.to_string(), KiraBus { track: None, parent: None, fx });

        Ok(Self {
            manager,
            clips: HashMap::new(),
            clip_names: HashMap::new(),
            sounds: HashMap::new(),
            sound_clips: HashMap::new(),
            next_clip_id: 1,
            next_sound_id: 1,
            suspended: Vec::new(),
            buses,
            clip_ids: HashMap::new(),
            filtered: HashMap::new(),
            clip_loops: HashMap::new(),
        })
    }
}

impl AudioBackend for KiraAudioBackend {
    fn load_clip(&mut self, id: &str, data: &[u8]) -> Result<ClipHandle> {
        // Check if already loaded
        if let Some(&handle_id) = self.clip_names.get(id) {
            return Ok(ClipHandle(handle_id));
        }

        let handle_id = self.next_clip_id;
        // A clip a package renders (a `.song`) arrives as PCM, frames
        // straight into kira with no file in between.
        let sound_data = match super::transcode::transcode(id, data) {
            Some(rendered) => {
                let t = rendered.map_err(|e| anyhow::anyhow!("{e}"))?;
                if let Some(at) = t.loop_start_seconds() {
                    self.clip_loops.insert(handle_id, at);
                }
                let frames: Arc<[Frame]> = t.left.iter().zip(&t.right).map(|(&l, &r)| Frame { left: l, right: r }).collect();
                StaticSoundData { sample_rate: t.rate, frames, settings: StaticSoundSettings::new(), slice: None }
            }
            None => StaticSoundData::from_cursor(Cursor::new(data.to_vec()))
                .map_err(|e| anyhow::anyhow!("Failed to decode audio '{}': {}", id, e))?,
        };
        self.next_clip_id += 1;
        self.clips.insert(handle_id, sound_data);
        self.clip_names.insert(id.to_string(), handle_id);
        self.clip_ids.insert(handle_id, id.to_string());

        log::info!("Loaded audio clip '{}' (handle {})", id, handle_id);
        Ok(ClipHandle(handle_id))
    }

    fn get_clip_by_name(&self, name: &str) -> Option<ClipHandle> {
        self.clip_names.get(name).map(|id| ClipHandle(*id))
    }

    fn play(&mut self, clip: ClipHandle, settings: PlaySettings) -> Result<SoundHandle> {
        let sound_data = self
            .clips
            .get(&clip.0)
            .ok_or_else(|| anyhow::anyhow!("Audio clip handle {} not found", clip.0))?
            .clone();

        // Build settings
        let mut kira_settings = StaticSoundSettings::new();
        if settings.looping {
            kira_settings = match self.clip_loops.get(&clip.0) {
                Some(&at) => kira_settings.loop_region(at..),
                None => kira_settings.loop_region(..),
            };
        }
        // A one-shot of a clip with a variation never plays the same twice.
        let (pitch, volume) = match self.clip_ids.get(&clip.0) {
            Some(name) if !settings.looping && settings.bus != super::MUSIC_BUS => {
                super::variation::apply(name, settings.pitch, settings.volume)
            }
            _ => (settings.pitch, settings.volume),
        };
        kira_settings = kira_settings
            .volume(amplitude_to_db(volume))
            .playback_rate(pitch as f64)
            .panning(kira::Panning(settings.pan.clamp(-1.0, 1.0)));

        let sound_data = sound_data.with_settings(kira_settings);

        // On its bus's track, so the bus's effects reach it - or on a track
        // of its own under that one, with a low-pass to muffle it.
        let sound_id = self.next_sound_id;
        let on_track = self.buses.get_mut(&settings.bus).and_then(|b| b.track.as_mut());
        let handle = if settings.filter {
            let mut builder = TrackBuilder::new();
            let filter = builder.add_effect(neutral_filter());
            let mut track = match on_track {
                Some(t) => t.add_sub_track(builder),
                None => self.manager.add_sub_track(builder),
            }
            .map_err(|e| anyhow::anyhow!("Failed to add a sound's track: {}", e))?;
            let handle = track.play(sound_data).map_err(|e| anyhow::anyhow!("Failed to play sound: {}", e))?;
            self.filtered.insert(sound_id, (track, filter));
            handle
        } else {
            match on_track {
                Some(track) => track.play(sound_data),
                None => self.manager.play(sound_data),
            }
            .map_err(|e| anyhow::anyhow!("Failed to play sound: {}", e))?
        };

        self.next_sound_id += 1;
        self.sounds.insert(sound_id, handle);
        // Remembered so `stop_by_clip` can find every instance of a clip.
        self.sound_clips.insert(sound_id, clip.0);

        Ok(SoundHandle(sound_id))
    }

    fn stop(&mut self, handle: SoundHandle) {
        if let Some(mut sound) = self.sounds.remove(&handle.0) {
            sound.stop(Tween::default());
        }
    }

    fn pause(&mut self, handle: SoundHandle) {
        if let Some(sound) = self.sounds.get_mut(&handle.0) {
            sound.pause(Tween::default());
        }
    }

    fn resume(&mut self, handle: SoundHandle) {
        if let Some(sound) = self.sounds.get_mut(&handle.0) {
            sound.resume(Tween::default());
        }
    }

    fn set_volume(&mut self, handle: SoundHandle, volume: f32) {
        if let Some(sound) = self.sounds.get_mut(&handle.0) {
            sound.set_volume(
                amplitude_to_db(volume),
                Tween::default(),
            );
        }
    }

    fn set_pan(&mut self, handle: SoundHandle, pan: f32) {
        if let Some(sound) = self.sounds.get_mut(&handle.0) {
            sound.set_panning(kira::Panning(pan.clamp(-1.0, 1.0)), Tween::default());
        }
    }

    fn set_lowpass(&mut self, handle: SoundHandle, hz: f32) {
        if let Some((_, filter)) = self.filtered.get_mut(&handle.0) {
            if hz > 0.0 && hz < BusEffects::OPEN {
                filter.set_cutoff(hz as f64, glide());
                filter.set_mix(Mix::WET, glide());
            } else {
                filter.set_mix(Mix::DRY, glide());
            }
        }
    }

    fn set_pitch(&mut self, handle: SoundHandle, pitch: f32) {
        if let Some(sound) = self.sounds.get_mut(&handle.0) {
            sound.set_playback_rate(pitch.max(0.01) as f64, Tween::default());
        }
    }

    fn seek(&mut self, handle: SoundHandle, seconds: f64) {
        if let Some(sound) = self.sounds.get_mut(&handle.0) {
            sound.seek_to(seconds.max(0.0));
        }
    }

    fn clip_duration(&self, clip: ClipHandle) -> Option<f64> {
        self.clips.get(&clip.0).map(|d| d.duration().as_secs_f64())
    }

    fn set_global_volume(&mut self, volume: f32) {
        self.manager.main_track().set_volume(
            amplitude_to_db(volume),
            Tween::default(),
        );
    }

    fn is_playing(&self, handle: SoundHandle) -> bool {
        // `update` drops finished sounds from the map, so presence is the
        // answer once it has run this frame.
        self.sounds.contains_key(&handle.0)
    }

    fn stop_all(&mut self) {
        for (_id, mut sound) in self.sounds.drain() {
            sound.stop(Tween::default());
        }
        self.sound_clips.clear();
    }

    fn stop_by_clip(&mut self, clip: ClipHandle) -> usize {
        let ids: Vec<u64> = self
            .sound_clips
            .iter()
            .filter(|(_, c)| **c == clip.0)
            .map(|(id, _)| *id)
            .collect();
        let mut stopped = 0;
        for id in ids {
            if let Some(mut sound) = self.sounds.remove(&id) {
                sound.stop(Tween::default());
                stopped += 1;
            }
            self.sound_clips.remove(&id);
        }
        stopped
    }

    fn pause_all(&mut self) {
        for sound in self.sounds.values_mut() {
            sound.pause(Tween::default());
        }
    }

    fn resume_all(&mut self) {
        for sound in self.sounds.values_mut() {
            sound.resume(Tween::default());
        }
    }

    fn suspend(&mut self) {
        self.suspended.clear();
        for (id, sound) in self.sounds.iter_mut() {
            if matches!(sound.state(), kira::sound::PlaybackState::Playing) {
                sound.pause(Tween::default());
                self.suspended.push(*id);
            }
        }
    }

    fn unsuspend(&mut self) {
        for id in self.suspended.drain(..) {
            if let Some(sound) = self.sounds.get_mut(&id) {
                sound.resume(Tween::default());
            }
        }
    }

    fn apply_buses(&mut self, plan: &BusPlan) {
        if plan.tree {
            // A bus that is gone, or now hangs from another parent, is
            // dropped (and rebuilt below) - kira cannot move a track.
            let wanted: HashMap<&str, Option<&str>> =
                plan.buses.iter().map(|(n, p, _)| (n.as_str(), p.as_deref())).collect();
            self.buses.retain(|name, bus| {
                name == MASTER_BUS || wanted.get(name.as_str()).is_some_and(|p| *p == bus.parent.as_deref())
            });
        }
        for (name, parent, fx) in &plan.buses {
            if !self.buses.contains_key(name) {
                let mut builder = TrackBuilder::new();
                let fx = bus_chain!(&mut builder);
                let under = parent.as_deref().and_then(|p| self.buses.get_mut(p)).and_then(|b| b.track.as_mut());
                let track = match under {
                    Some(t) => t.add_sub_track(builder),
                    None => self.manager.add_sub_track(builder),
                };
                match track {
                    Ok(track) => {
                        self.buses.insert(name.clone(), KiraBus { track: Some(track), parent: parent.clone(), fx });
                    }
                    Err(e) => {
                        log::warn!("[audio] could not make a track for bus '{name}': {e}");
                        continue;
                    }
                }
            }
            if let Some(bus) = self.buses.get_mut(name) {
                bus.fx.set(fx);
            }
        }
    }

    fn position(&self, handle: SoundHandle) -> Option<f64> {
        self.sounds.get(&handle.0).map(|s| s.position())
    }

    fn update(&mut self) {
        // Clean up finished sounds
        self.sounds.retain(|_id, handle| {
            !matches!(
                handle.state(),
                kira::sound::PlaybackState::Stopped
            )
        });
        // Keep the clip map in step, or it grows for the life of the process.
        self.sound_clips.retain(|id, _| self.sounds.contains_key(id));
        // A filtered sound's track goes with it.
        self.filtered.retain(|id, _| self.sounds.contains_key(id));
    }
}
