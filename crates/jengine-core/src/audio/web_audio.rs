#![allow(deprecated)]
//! Slice 8 — minimal Web Audio backend for the editor viewport.
//!
//! Routes the 9 script-emitted audio commands (`ps`, `pm`, `sts`,
//! `stm`, `pam`, `rem`, `ssv`, `smv`, `smav`) to a browser
//! `AudioContext`. Replaces the original "preview no-op" stubs so
//! Slice 8's scene 16 actually plays sound inside the editor.
//!
//! Design choices kept deliberately small:
//! - Clips are decoded asynchronously via `decode_audio_data` and
//!   stored as `AudioBuffer`s keyed by their asset path.
//! - One global master `GainNode` feeds `ctx.destination`; every sound -
//!   music included - has its own `GainNode` in front of it, addressed by the
//!   id `play_*` returns. Those ids are what the mixer tracks, so a bus change
//!   reaches a playing sound here exactly as it does natively: see
//!   [`WebAudioBackend::sync_mixer`] and the `*_on_bus` helpers, which both
//!   wasm hosts call rather than doing the volume arithmetic themselves.
//! - One-shot sounds get a per-call `GainNode` (volume) and an
//!   `AudioBufferSourceNode` (pitch via `playbackRate`). The browser
//!   GCs the nodes once they fire `onended`.
//! - Pause/resume on music tracks the elapsed offset against the
//!   `AudioContext`'s `current_time` and rebuilds the source on
//!   resume (Web Audio sources are one-shot by spec, so this is the
//!   standard workaround).
//! - One-shots are retained in `oneshots` so `stop_all_sounds` /
//!   `pause_all_sounds` / `resume_all_sounds` and `stop_sound_by_clip`
//!   can reach them. They used to be dropped straight after `start()`,
//!   which made a long one-shot uncancellable in the editor and on web
//!   while `stop_all_sounds` mapped to `stop_music()` alone.
//! - `stop_sound(handle)` stays a no-op: `play_sound` is a *deferred*
//!   command, so it has not run by the time the script returns and
//!   there is no handle to hand back. Stopping by clip id is the
//!   usable form, and is what `stop_sound_by_clip` offers.
//!
//! Errors are surfaced through `app.log_buffer` by the caller; this
//! module just returns `Result<_, String>` and lets the host log.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioBuffer, AudioBufferSourceNode, AudioContext, AudioContextState, AudioNode, BiquadFilterNode, BiquadFilterType,
    ConvolverNode, DelayNode, DynamicsCompressorNode, GainNode, OverSampleType, StereoPannerNode, WaveShaperNode,
};

use super::mixer::{BusEffects, BusPlan};
use super::{AudioMixer, MASTER_BUS, MUSIC_BUS};

/// One mixer bus as Web Audio nodes, in [`BusEffects`]'s order: `input`
/// through three EQ bands, a wave shaper (the distortion), a low-pass
/// filter and a compressor with its makeup gain; then the echo (a dry gain
/// and a delay line with its feedback loop and wet gain, summed at
/// `echoed`); then split into a dry gain and a reverb (a convolver with a
/// generated impulse, then a wet gain), joined again at `output`, which
/// feeds the parent bus's input - or, for `master`, the master gain. Every
/// number comes from `BusEffects`' own methods, which kira's chain reads
/// too, so the two backends agree on what `echo = 0.3` sounds like.
struct WebBus {
    input: GainNode,
    eq: [BiquadFilterNode; 3],
    shaper: WaveShaperNode,
    /// The distortion the shaper's curve was drawn for; NaN before one is.
    shaped: f32,
    filter: BiquadFilterNode,
    compressor: DynamicsCompressorNode,
    makeup: GainNode,
    echo_dry: GainNode,
    delay: DelayNode,
    echo_return: GainNode,
    echo_wet: GainNode,
    dry: GainNode,
    convolver: ConvolverNode,
    wet: GainNode,
    output: GainNode,
    parent: Option<String>,
    /// The room the convolver's impulse was made for; NaN before one is.
    room: f32,
}

/// A reverb's impulse response: two channels of noise, fading over
/// 0.4 to 3.6 seconds as `room` grows, and losing its highs as it fades -
/// which is what a room does to an echo.
fn reverb_impulse(ctx: &AudioContext, room: f32) -> Option<AudioBuffer> {
    let rate = ctx.sample_rate();
    let room = room.clamp(0.0, 1.0);
    let len = (rate * (0.4 + 3.2 * room)).max(1.0) as u32;
    let buf = ctx.create_buffer(2, len, rate).ok()?;
    let mut seed: u32 = 0x9E37_79B9;
    for ch in 0..2 {
        let mut data = vec![0f32; len as usize];
        let mut low = 0.0f32;
        for (i, v) in data.iter_mut().enumerate() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
            let t = i as f32 / len as f32;
            low += (0.9 - 0.75 * t * (0.4 + 0.6 * room)) * (noise - low);
            *v = low * (1.0 - t).powf(2.0 + 2.0 * (1.0 - room));
        }
        buf.copy_to_channel(&mut data, ch).ok()?;
    }
    Some(buf)
}

/// Decode state for a single clip. We start `Loading` the moment the
/// editor calls `viewport_load_audio` and flip to `Ready` once the
/// async decode resolves. `Failed` means we logged once and won't
/// retry on subsequent plays.
enum Clip {
    Loading,
    Ready(AudioBuffer),
    Failed,
}

pub struct WebAudioBackend {
    /// Lazy: only constructed on the first audio call so we don't
    /// trigger an autoplay-blocked AudioContext at viewport_init time.
    ctx: Option<AudioContext>,
    master_gain: Option<GainNode>,
    /// The spectrum's tap: an analyser on the master gain (`tap`).
    analyser: Option<web_sys::AnalyserNode>,
    master_vol: f32,
    /// The page's say over the sound, apart from the game's own: a web
    /// portal muting it (its audio setting, an ad) or pausing the game.
    /// Muting holds the master at 0 without touching `master_vol`; pausing
    /// suspends the whole context, music and one-shots alike, and keeps the
    /// autoplay resume from starting it again. See `docs/web-platforms.md`.
    host_muted: bool,
    host_paused: bool,
    /// Id for the next sound. Shared by one-shots and music, so one id names
    /// one sound whichever kind it is.
    next_id: u64,
    clips: Rc<RefCell<HashMap<String, Clip>>>,
    /// Currently-playing one-shots, so they can be stopped.
    ///
    /// Web Audio gives no "has this source finished?" flag, and wiring an
    /// `onended` closure per sound would mean a `Closure` allocation and a
    /// leak-or-forget decision on every shot. The end time is known up front
    /// from the buffer duration and playback rate, so expiry is arithmetic
    /// against `ctx.current_time()` instead.
    oneshots: Vec<OneShot>,
    /// The mixer's buses - parent and effects - as the last bus plan left
    /// them (`apply_buses`); nodes are built from it when first played on.
    bus_model: HashMap<String, (Option<String>, BusEffects)>,
    bus_nodes: HashMap<String, WebBus>,
    /// Where a transcoded clip's loop starts, in seconds, when not at its
    /// top (`audio::transcode` - a `.song` with an intro).
    loop_starts: HashMap<String, f64>,
}

struct OneShot {
    id: u64,
    clip: String,
    source: AudioBufferSourceNode,
    gain: GainNode,
    /// Between `gain` and the master, once the sound has been off-centre -
    /// made at play time or by the first [`WebAudioBackend::set_pan`].
    panner: Option<StereoPannerNode>,
    /// Between `gain` and the panner (or the master), once the sound has been
    /// muffled - made by the first [`WebAudioBackend::set_lowpass`].
    lowpass: Option<web_sys::BiquadFilterNode>,
    /// Where it goes out: its bus's input (or the master gain).
    dest: AudioNode,
    /// Playback rate (pitch), needed again when a paused shot is rebuilt.
    rate: f64,
    looping: bool,
    /// Where a loop goes back to, seconds; 0 for the top.
    loop_start: f64,
    /// Buffer length in seconds.
    duration: f64,
    /// `AudioContext` time the current source started, and the buffer offset
    /// it started from - together they give the playhead at any moment.
    started_at: f64,
    start_offset: f64,
    /// `AudioContext` time this shot is expected to finish at; infinite for a
    /// looping shot.
    ends_at: f64,
    /// Buffer offset to resume from, while paused.
    paused_offset: Option<f64>,
}

impl OneShot {
    /// Where in the buffer the playhead is at context time `now`.
    fn playhead(&self, now: f64) -> f64 {
        let pos = self.start_offset + (now - self.started_at).max(0.0) * self.rate;
        if self.looping && self.duration > 0.0 {
            super::transcode::loop_position(pos, self.loop_start, self.duration)
        } else {
            pos.min(self.duration)
        }
    }
}

impl WebAudioBackend {
    pub fn new() -> Self {
        Self {
            ctx: None,
            master_gain: None,
            analyser: None,
            master_vol: 1.0,
            host_muted: false,
            host_paused: false,
            next_id: 1,
            clips: Rc::new(RefCell::new(HashMap::new())),
            oneshots: Vec::new(),
            bus_model: HashMap::new(),
            bus_nodes: HashMap::new(),
            loop_starts: HashMap::new(),
        }
    }

    /// Bring the bus model in line with the mixer's plan, and set the
    /// effects of every bus whose nodes exist. The browsers' half of
    /// `AudioBackend::apply_buses`.
    pub fn apply_buses(&mut self, plan: &BusPlan) {
        if plan.tree {
            let wanted: HashMap<&str, Option<&str>> =
                plan.buses.iter().map(|(n, p, _)| (n.as_str(), p.as_deref())).collect();
            self.bus_model.retain(|n, _| wanted.contains_key(n.as_str()));
            let stale: Vec<String> = self
                .bus_nodes
                .iter()
                .filter(|(n, b)| wanted.get(n.as_str()).is_none_or(|p| *p != b.parent.as_deref()))
                .map(|(n, _)| n.clone())
                .collect();
            for n in stale {
                if let Some(b) = self.bus_nodes.remove(&n) {
                    let _ = b.output.disconnect();
                }
            }
        }
        for (name, parent, fx) in &plan.buses {
            self.bus_model.insert(name.clone(), (parent.clone(), *fx));
            self.set_bus_effects(name);
        }
    }

    /// A bus's input node, building its chain (and its parents') the first
    /// time. `None` for a bus the mixer does not have, or before the
    /// context exists.
    fn bus_input(&mut self, name: &str) -> Option<AudioNode> {
        if let Some(b) = self.bus_nodes.get(name) {
            return Some(b.input.clone().into());
        }
        let parent = self.bus_model.get(name)?.0.clone();
        let dest: AudioNode = match parent.as_deref() {
            Some(p) => self.bus_input(p).or_else(|| self.master_gain.clone().map(Into::into))?,
            None => self.master_gain.clone()?.into(),
        };
        let ctx = self.ctx.as_ref()?;
        let input = ctx.create_gain().ok()?;
        let band = |kind: BiquadFilterType, hz: f32, q: f32| -> Option<BiquadFilterNode> {
            let b = ctx.create_biquad_filter().ok()?;
            b.set_type(kind);
            b.frequency().set_value(hz);
            b.q().set_value(q);
            b.gain().set_value(0.0);
            Some(b)
        };
        let eq = [
            band(BiquadFilterType::Lowshelf, BusEffects::EQ_LOW_HZ, 0.7)?,
            band(BiquadFilterType::Peaking, BusEffects::EQ_MID_HZ, BusEffects::EQ_MID_Q)?,
            band(BiquadFilterType::Highshelf, BusEffects::EQ_HIGH_HZ, 0.7)?,
        ];
        let shaper = ctx.create_wave_shaper().ok()?;
        shaper.set_oversample(OverSampleType::N2x);
        let filter = ctx.create_biquad_filter().ok()?;
        filter.set_type(BiquadFilterType::Lowpass);
        filter.frequency().set_value((ctx.sample_rate() * 0.5).min(BusEffects::OPEN));
        let compressor = ctx.create_dynamics_compressor().ok()?;
        compressor.threshold().set_value(0.0);
        compressor.ratio().set_value(1.0);
        compressor.knee().set_value(6.0);
        let makeup = ctx.create_gain().ok()?;
        let echo_dry = ctx.create_gain().ok()?;
        let delay = ctx.create_delay_with_max_delay_time(BusEffects::MAX_ECHO_TIME as f64).ok()?;
        let echo_return = ctx.create_gain().ok()?;
        echo_return.gain().set_value(0.0);
        let echo_wet = ctx.create_gain().ok()?;
        echo_wet.gain().set_value(0.0);
        let echoed = ctx.create_gain().ok()?;
        let dry = ctx.create_gain().ok()?;
        let convolver = ctx.create_convolver().ok()?;
        let wet = ctx.create_gain().ok()?;
        wet.gain().set_value(0.0);
        let output = ctx.create_gain().ok()?;
        let wired = input.connect_with_audio_node(&eq[0]).is_ok()
            && eq[0].connect_with_audio_node(&eq[1]).is_ok()
            && eq[1].connect_with_audio_node(&eq[2]).is_ok()
            && eq[2].connect_with_audio_node(&shaper).is_ok()
            && shaper.connect_with_audio_node(&filter).is_ok()
            && filter.connect_with_audio_node(&compressor).is_ok()
            && compressor.connect_with_audio_node(&makeup).is_ok()
            && makeup.connect_with_audio_node(&echo_dry).is_ok()
            && echo_dry.connect_with_audio_node(&echoed).is_ok()
            && makeup.connect_with_audio_node(&delay).is_ok()
            && delay.connect_with_audio_node(&echo_return).is_ok()
            && echo_return.connect_with_audio_node(&delay).is_ok()
            && delay.connect_with_audio_node(&echo_wet).is_ok()
            && echo_wet.connect_with_audio_node(&echoed).is_ok()
            && echoed.connect_with_audio_node(&dry).is_ok()
            && dry.connect_with_audio_node(&output).is_ok()
            && echoed.connect_with_audio_node(&convolver).is_ok()
            && convolver.connect_with_audio_node(&wet).is_ok()
            && wet.connect_with_audio_node(&output).is_ok()
            && output.connect_with_audio_node(&dest).is_ok();
        if !wired {
            log::warn!("[audio] could not wire bus '{name}'");
            return None;
        }
        let node: AudioNode = input.clone().into();
        self.bus_nodes.insert(
            name.to_string(),
            WebBus {
                input,
                eq,
                shaper,
                shaped: f32::NAN,
                filter,
                compressor,
                makeup,
                echo_dry,
                delay,
                echo_return,
                echo_wet,
                dry,
                convolver,
                wet,
                output,
                parent,
                room: f32::NAN,
            },
        );
        self.set_bus_effects(name);
        Some(node)
    }

    /// Glide a built bus's nodes to its model's effects.
    fn set_bus_effects(&mut self, name: &str) {
        let (Some(ctx), Some(bus), Some((_, fx))) =
            (self.ctx.as_ref(), self.bus_nodes.get_mut(name), self.bus_model.get(name))
        else {
            return;
        };
        let now = ctx.current_time();
        for (band, gain) in bus.eq.iter().zip(fx.eq_gains()) {
            let _ = band.gain().set_target_at_time(gain, now, 0.02);
        }
        if fx.distortion <= 0.0 {
            if !bus.shaped.is_nan() {
                bus.shaper.set_curve(None);
                bus.shaped = f32::NAN;
            }
        } else if bus.shaped.is_nan() || (bus.shaped - fx.distortion).abs() > 0.005 {
            // The curve is `BusEffects::distort` over -1..1, which is what
            // kira's soft clip does to each sample.
            let n = 1025;
            let mut curve: Vec<f32> =
                (0..n).map(|i| fx.distort(i as f32 / (n - 1) as f32 * 2.0 - 1.0)).collect();
            bus.shaper.set_curve(Some(&mut curve));
            bus.shaped = fx.distortion;
        }
        let open = (ctx.sample_rate() * 0.5).min(BusEffects::OPEN);
        let hz = fx.cutoff().unwrap_or(open).min(open);
        let _ = bus.filter.frequency().set_target_at_time(hz, now, 0.02);
        let c = fx.compressor();
        let _ = bus.compressor.threshold().set_target_at_time(c.threshold_db, now, 0.02);
        let _ = bus.compressor.ratio().set_target_at_time(c.ratio, now, 0.02);
        bus.compressor.attack().set_value(c.attack);
        bus.compressor.release().set_value(c.release);
        let _ = bus.makeup.gain().set_target_at_time(10f32.powf(c.makeup_db / 20.0), now, 0.02);
        let (echo_dry, echo_wet) = fx.echo_gains();
        let _ = bus.echo_dry.gain().set_target_at_time(echo_dry, now, 0.02);
        let _ = bus.echo_wet.gain().set_target_at_time(echo_wet, now, 0.02);
        let _ = bus.echo_return.gain().set_target_at_time(fx.echo_return(), now, 0.02);
        let _ = bus.delay.delay_time().set_target_at_time(fx.echo_delay(), now, 0.02);
        let mix = fx.reverb.clamp(0.0, 1.0);
        let _ = bus.dry.gain().set_target_at_time(1.0 - mix, now, 0.02);
        let _ = bus.wet.gain().set_target_at_time(mix, now, 0.02);
        if mix > 0.0 && (bus.room.is_nan() || (bus.room - fx.room).abs() > 0.02) {
            if let Some(buf) = reverb_impulse(ctx, fx.room) {
                bus.convolver.set_buffer(Some(&buf));
                bus.room = fx.room;
            }
        }
    }

    /// Lazily create the `AudioContext` + master gain. Returns
    /// references for use by the caller. Browsers block context
    /// creation until a user gesture has occurred; the first
    /// keypress that triggers an audio command happens after that
    /// gesture, so this path is safe in practice.
    fn ensure_ctx(&mut self) -> Result<(&AudioContext, &GainNode), String> {
        if self.ctx.is_none() {
            let ctx = AudioContext::new()
                .map_err(|e| format!("AudioContext::new failed: {e:?}"))?;
            let gain = ctx
                .create_gain()
                .map_err(|e| format!("create_gain failed: {e:?}"))?;
            gain.gain().set_value(self.effective_master());
            if self.host_paused {
                let _ = ctx.suspend();
            }
            gain.connect_with_audio_node(&ctx.destination())
                .map_err(|e| format!("connect master failed: {e:?}"))?;
            // The spectrum hears the master's output, beside the speakers.
            if let Ok(a) = ctx.create_analyser() {
                a.set_fft_size(super::analysis::WINDOW as u32);
                let _ = gain.connect_with_audio_node(&a);
                self.analyser = Some(a);
            }
            self.ctx = Some(ctx);
            self.master_gain = Some(gain);
        }
        Ok((self.ctx.as_ref().unwrap(), self.master_gain.as_ref().unwrap()))
    }

    /// Hand the spectrum the analyser's latest window (`audio::analysis`).
    pub fn tap(&mut self) {
        let (Some(a), Some(ctx)) = (&self.analyser, &self.ctx) else { return };
        let mut buf = vec![0.0f32; super::analysis::WINDOW];
        a.get_float_time_domain_data(&mut buf);
        super::analysis::set_window(&buf, ctx.sample_rate() as u32);
    }

    /// A clip made of samples here and now - a synthesised note - ready at
    /// once, where `load_clip` decodes a file on a promise.
    pub fn insert_samples(&mut self, name: &str, samples: &[f32], rate: u32) -> Result<(), String> {
        if self.clips.borrow().contains_key(name) {
            return Ok(());
        }
        let (ctx, _) = self.ensure_ctx()?;
        let buf = ctx
            .create_buffer(1, samples.len().max(1) as u32, rate as f32)
            .map_err(|e| format!("create_buffer failed: {e:?}"))?;
        buf.copy_to_channel(samples, 0).map_err(|e| format!("copy_to_channel failed: {e:?}"))?;
        self.clips.borrow_mut().insert(name.to_string(), Clip::Ready(buf));
        Ok(())
    }

    /// Kick off an async decode for `bytes` and store the resulting
    /// `AudioBuffer` under `name`. Safe to call from the editor
    /// before play mode begins. If the context can't be created yet
    /// (no user gesture) we defer the decode until the first play
    /// call by stashing `Loading` and the bytes are dropped — the
    /// editor re-uploads on play if needed. For simplicity we just
    /// require the editor to call this from within play mode (which
    /// `Toolbar.handlePlay` already gates on a user click).
    pub fn load_clip(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        // Already loaded / loading — don't redecode.
        if self.clips.borrow().contains_key(name) {
            return Ok(());
        }
        let (ctx, _) = self.ensure_ctx()?;
        // A clip a package renders (a `.song`) is PCM already: a buffer made
        // here and ready at once, where a file decodes on a promise.
        if let Some(rendered) = super::transcode::transcode(name, bytes) {
            let t = rendered?;
            let n = t.frames();
            let buf = ctx
                .create_buffer(2, n.max(1) as u32, t.rate as f32)
                .map_err(|e| format!("create_buffer failed for '{name}': {e:?}"))?;
            buf.copy_to_channel(&t.left[..n], 0).map_err(|e| format!("copy_to_channel failed: {e:?}"))?;
            buf.copy_to_channel(&t.right[..n], 1).map_err(|e| format!("copy_to_channel failed: {e:?}"))?;
            if let Some(at) = t.loop_start_seconds() {
                self.loop_starts.insert(name.to_string(), at);
            }
            self.clips.borrow_mut().insert(name.to_string(), Clip::Ready(buf));
            return Ok(());
        }
        // Copy bytes into a fresh ArrayBuffer. `decode_audio_data`
        // detaches its input per spec, so we can't share with WASM.
        let u8a = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
        u8a.copy_from(bytes);
        let array_buffer = u8a.buffer();
        let promise = ctx
            .decode_audio_data(&array_buffer)
            .map_err(|e| format!("decode_audio_data failed for '{name}': {e:?}"))?;

        self.clips.borrow_mut().insert(name.to_string(), Clip::Loading);
        let clips_for_decode = self.clips.clone();
        let name_owned = name.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            match JsFuture::from(promise).await {
                Ok(buffer) => {
                    match buffer.dyn_into::<AudioBuffer>() {
                        Ok(buf) => {
                            clips_for_decode
                                .borrow_mut()
                                .insert(name_owned, Clip::Ready(buf));
                        }
                        Err(_) => {
                            clips_for_decode
                                .borrow_mut()
                                .insert(name_owned, Clip::Failed);
                        }
                    }
                }
                Err(_) => {
                    clips_for_decode
                        .borrow_mut()
                        .insert(name_owned, Clip::Failed);
                }
            }
        });
        Ok(())
    }

    /// Fetch a decoded `AudioBuffer` if ready. Returns `None` for
    /// `Loading` / `Failed` / missing — caller logs appropriately.
    fn get_ready(&self, name: &str) -> Option<AudioBuffer> {
        let map = self.clips.borrow();
        match map.get(name) {
            Some(Clip::Ready(buf)) => Some(buf.clone()),
            _ => None,
        }
    }

    /// Resume the context if the browser started it suspended.
    ///
    /// Autoplay policy: an `AudioContext` created without user activation
    /// starts `suspended`, and a suspended context plays *nothing* while
    /// every call still reports success - the failure is completely silent.
    /// The editor never hit this because its context is created from inside
    /// the Play click; a deployed build creates one during a script-driven
    /// scene load, which is not a gesture task.
    ///
    /// Resuming needs sticky activation rather than a live gesture handler,
    /// so calling this before playback (and again from the first input
    /// event) is enough once the player has interacted with the page at all.
    pub fn resume_if_suspended(&mut self) {
        if self.host_paused {
            return;
        }
        if let Some(ctx) = self.ctx.as_ref()
            && ctx.state() == AudioContextState::Suspended
        {
            // Resolves asynchronously; nothing here depends on it, and a
            // rejection just means activation has not happened yet.
            let _ = ctx.resume();
        }
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Centred, non-looping one-shot.
    pub fn play_sound(&mut self, clip: &str, volume: f32, pitch: f32) -> Result<u64, String> {
        self.play_sound_panned(clip, volume, pitch, 0.0, false)
    }

    /// [`Self::play_sound_routed`] on the master bus.
    pub fn play_sound_panned(
        &mut self,
        clip: &str,
        volume: f32,
        pitch: f32,
        pan: f32,
        looping: bool,
    ) -> Result<u64, String> {
        self.play_sound_routed(clip, volume, pitch, pan, looping, MASTER_BUS)
    }

    /// One-shot at a stereo position through `bus`'s effects: `pan` is -1.0
    /// hard left to 1.0 hard right, the same convention kira uses natively.
    /// `volume` is the final gain - see [`Self::play_sound_on_bus`] for the
    /// mixer-aware form. Returns the sound's id.
    pub fn play_sound_routed(
        &mut self,
        clip: &str,
        volume: f32,
        pitch: f32,
        pan: f32,
        looping: bool,
        bus: &str,
    ) -> Result<u64, String> {
        self.play_routed_at(clip, volume, pitch, pan, looping, bus, 0.0)
    }

    /// [`Self::play_sound_routed`], starting `offset` seconds into the clip -
    /// how music resumes and seeks.
    #[allow(clippy::too_many_arguments)]
    pub fn play_routed_at(
        &mut self,
        clip: &str,
        volume: f32,
        pitch: f32,
        pan: f32,
        looping: bool,
        bus: &str,
        offset: f64,
    ) -> Result<u64, String> {
        self.resume_if_suspended();
        let buf = match self.get_ready(clip) {
            Some(b) => b,
            None => return Err(format!("clip '{clip}' not ready")),
        };
        self.ensure_ctx()?;
        let dest: AudioNode = match self.bus_input(bus) {
            Some(n) => n,
            None => self.master_gain.clone().ok_or("no master gain")?.into(),
        };
        // A one-shot of a clip with a variation never plays the same twice;
        // music and loops play as written.
        let (pitch, volume) = if looping || bus == MUSIC_BUS {
            (pitch, volume)
        } else {
            super::variation::apply(clip, pitch, volume)
        };
        let master = &dest;
        let ctx = self.ctx.as_ref().ok_or("no audio context")?;
        let source = ctx
            .create_buffer_source()
            .map_err(|e| format!("create_buffer_source: {e:?}"))?;
        source.set_buffer(Some(&buf));
        source.set_loop(looping);
        let loop_start = if looping { self.loop_starts.get(clip).copied().unwrap_or(0.0) } else { 0.0 };
        if loop_start > 0.0 {
            source.set_loop_start(loop_start);
            source.set_loop_end(buf.duration());
        }
        source.playback_rate().set_value(pitch.max(0.01));
        let gain = ctx.create_gain().map_err(|e| format!("create_gain: {e:?}"))?;
        gain.gain().set_value(volume.clamp(0.0, 4.0));
        source
            .connect_with_audio_node(&gain)
            .map_err(|e| format!("connect source->gain: {e:?}"))?;
        // A panner is only inserted when the sound is actually off-centre:
        // the node is cheap but not free, and most sounds are centred.
        let mut kept_panner = None;
        if pan.abs() > f32::EPSILON {
            let panner = ctx
                .create_stereo_panner()
                .map_err(|e| format!("create_stereo_panner: {e:?}"))?;
            panner.pan().set_value(pan.clamp(-1.0, 1.0));
            gain.connect_with_audio_node(&panner)
                .map_err(|e| format!("connect gain->panner: {e:?}"))?;
            panner
                .connect_with_audio_node(master)
                .map_err(|e| format!("connect panner->master: {e:?}"))?;
            kept_panner = Some(panner);
        } else {
            gain.connect_with_audio_node(master)
                .map_err(|e| format!("connect gain->master: {e:?}"))?;
        }
        let offset = if looping && buf.duration() > 0.0 {
            super::transcode::loop_position(offset, loop_start, buf.duration())
        } else {
            offset.max(0.0)
        };
        source
            .start_with_when_and_grain_offset(0.0, offset)
            .map_err(|e| format!("source.start: {e:?}"))?;
        // Retain it so stop/pause/resume-all can reach it. Pruned first so a
        // scene firing shots every frame does not grow the list without bound.
        let rate = pitch.max(0.01) as f64;
        let now = ctx.current_time();
        let duration = buf.duration();
        let ends_at = if looping { f64::INFINITY } else { now + (duration - offset).max(0.0) / rate };
        self.prune_finished_oneshots();
        let id = self.take_id();
        self.oneshots.push(OneShot {
            id,
            clip: clip.to_string(),
            source,
            gain,
            panner: kept_panner,
            lowpass: None,
            dest,
            rate,
            looping,
            loop_start,
            duration,
            started_at: now,
            start_offset: offset,
            ends_at,
            paused_offset: None,
        });
        Ok(id)
    }

    /// Drop one-shots whose expected end time has passed.
    ///
    /// Paused shots are kept regardless: their clock is stopped, so `ends_at`
    /// means nothing until they resume.
    fn prune_finished_oneshots(&mut self) {
        let Some(now) = self.ctx.as_ref().map(|c| c.current_time()) else {
            return;
        };
        self.oneshots
            .retain(|o| o.paused_offset.is_some() || o.ends_at > now);
    }

    /// Stop every playing one-shot. Music is stopped separately by the caller,
    /// which lets `stop_all_sounds` and `stop_music` stay independent.
    pub fn stop_all_oneshots(&mut self) {
        for shot in self.oneshots.drain(..) {
            let _ = shot.source.stop();
        }
    }

    /// Stop every playing instance of one clip, addressed by its asset path.
    ///
    /// The usable form of "stop this sound": `play_sound` is deferred and
    /// cannot return a handle, so there is nothing else for a script to name.
    pub fn stop_sound_by_clip(&mut self, clip: &str) -> usize {
        let mut stopped = 0;
        self.oneshots.retain(|shot| {
            if shot.clip == clip {
                let _ = shot.source.stop();
                stopped += 1;
                false
            } else {
                true
            }
        });
        stopped
    }

    /// Pause every playing one-shot.
    ///
    /// A Web Audio source cannot be paused and restarted, so this stops it and
    /// records the remaining tail; `resume_all_oneshots` builds a fresh source
    /// and plays that tail as an offset — the same trick `pause_music` uses.
    pub fn pause_all_oneshots(&mut self) {
        self.prune_finished_oneshots();
        let Some(now) = self.ctx.as_ref().map(|c| c.current_time()) else {
            return;
        };
        for shot in &mut self.oneshots {
            if shot.paused_offset.is_none() {
                shot.paused_offset = Some(shot.playhead(now));
                let _ = shot.source.stop();
            }
        }
    }

    /// Resume every paused one-shot from where it stopped.
    ///
    /// The rebuilt source used to get neither the shot's pitch nor a loop
    /// flag, and the resume offset was worked out in wall-clock time rather
    /// than buffer time, so a pitched shot resumed at the wrong point and at
    /// normal speed.
    pub fn resume_all_oneshots(&mut self) {
        let Some(ctx) = self.ctx.clone() else { return };
        let clips = self.clips.clone();
        let clips = clips.borrow();
        let mut resumed: Vec<OneShot> = Vec::new();
        for shot in self.oneshots.drain(..) {
            match shot.paused_offset {
                None => resumed.push(shot),
                // Start where it stopped rather than replaying from the top.
                Some(offset) => resumed.extend(Self::restarted(&ctx, &clips, shot, offset)),
            }
        }
        self.oneshots = resumed;
    }

    /// A shot's source rebuilt to play from `offset`, at its own rate: a Web
    /// Audio source plays once, so resuming and seeking both make a new one.
    fn restarted(ctx: &AudioContext, clips: &HashMap<String, Clip>, shot: OneShot, offset: f64) -> Option<OneShot> {
        let buf = match clips.get(&shot.clip) {
            Some(Clip::Ready(b)) => b.clone(),
            _ => return None,
        };
        let source = ctx.create_buffer_source().ok()?;
        source.set_buffer(Some(&buf));
        source.set_loop(shot.looping);
        if shot.looping && shot.loop_start > 0.0 {
            source.set_loop_start(shot.loop_start);
            source.set_loop_end(shot.duration);
        }
        source.playback_rate().set_value(shot.rate as f32);
        source.connect_with_audio_node(&shot.gain).ok()?;
        source.start_with_when_and_grain_offset(0.0, offset).ok()?;
        let now = ctx.current_time();
        let ends_at = if shot.looping { f64::INFINITY } else { now + (shot.duration - offset).max(0.0) / shot.rate };
        Some(OneShot { source, started_at: now, start_offset: offset, ends_at, paused_offset: None, ..shot })
    }

    /// Pause one sound, keeping where it was.
    pub fn pause_id(&mut self, id: u64) {
        let Some(now) = self.ctx.as_ref().map(|c| c.current_time()) else { return };
        if let Some(shot) = self.oneshots.iter_mut().find(|o| o.id == id && o.paused_offset.is_none()) {
            shot.paused_offset = Some(shot.playhead(now));
            let _ = shot.source.stop();
        }
    }

    /// Resume one paused sound from where it was.
    pub fn resume_id(&mut self, id: u64) {
        let Some(ctx) = self.ctx.clone() else { return };
        let Some(i) = self.oneshots.iter().position(|o| o.id == id && o.paused_offset.is_some()) else { return };
        let shot = self.oneshots.remove(i);
        let offset = shot.paused_offset.unwrap_or(0.0);
        let clips = self.clips.clone();
        if let Some(shot) = Self::restarted(&ctx, &clips.borrow(), shot, offset) {
            self.oneshots.push(shot);
        }
    }

    /// Move one sound's playhead to `seconds` (wrapped, for a loop).
    pub fn seek_id(&mut self, id: u64, seconds: f64) {
        let Some(ctx) = self.ctx.clone() else { return };
        let Some(i) = self.oneshots.iter().position(|o| o.id == id) else { return };
        let d = self.oneshots[i].duration;
        let to = if self.oneshots[i].looping && d > 0.0 {
            super::transcode::loop_position(seconds, self.oneshots[i].loop_start, d)
        } else {
            seconds.clamp(0.0, d)
        };
        if self.oneshots[i].paused_offset.is_some() {
            self.oneshots[i].paused_offset = Some(to);
            return;
        }
        let shot = self.oneshots.remove(i);
        let _ = shot.source.stop();
        let clips = self.clips.clone();
        if let Some(shot) = Self::restarted(&ctx, &clips.borrow(), shot, to) {
            self.oneshots.push(shot);
        }
    }

    /// Change one sound's playback rate, keeping its playhead where it is.
    pub fn set_rate(&mut self, id: u64, pitch: f32) {
        let Some(now) = self.ctx.as_ref().map(|c| c.current_time()) else { return };
        let Some(shot) = self.oneshots.iter_mut().find(|o| o.id == id) else { return };
        let rate = pitch.max(0.01) as f64;
        if shot.paused_offset.is_none() {
            shot.start_offset = shot.playhead(now);
            shot.started_at = now;
            shot.source.playback_rate().set_value(rate as f32);
            shot.ends_at = if shot.looping { f64::INFINITY } else { now + (shot.duration - shot.start_offset).max(0.0) / rate };
        }
        shot.rate = rate;
    }

    /// Seconds into one sound, playing or paused.
    pub fn position_of(&self, id: u64) -> Option<f64> {
        let shot = self.oneshots.iter().find(|o| o.id == id)?;
        let now = self.ctx.as_ref()?.current_time();
        Some(shot.paused_offset.unwrap_or_else(|| shot.playhead(now)))
    }

    /// How long a decoded clip is.
    pub fn clip_duration(&self, clip: &str) -> Option<f64> {
        match self.clips.borrow().get(clip) {
            Some(Clip::Ready(b)) => Some(b.duration()),
            _ => None,
        }
    }

    /// Set the final gain of one sound, music or one-shot.
    pub fn set_volume(&mut self, id: u64, volume: f32) {
        let v = volume.clamp(0.0, 4.0);
        if let Some(shot) = self.oneshots.iter().find(|o| o.id == id) {
            shot.gain.gain().set_value(v);
        }
    }

    /// Move a playing one-shot across the stereo field (-1 left, 1 right).
    /// A sound that started centred has no panner yet: one is put between
    /// its gain and the master the first time it moves.
    /// A sound's own low-pass cutoff in hertz (0: open) - the walls between
    /// a spatial source and the listener. The filter is put in after the
    /// sound's gain the first time it is muffled, and opened up (not taken
    /// out) after.
    pub fn set_lowpass(&mut self, id: u64, hz: f32) {
        let Some(ctx) = self.ctx.as_ref() else { return };
        let Some(shot) = self.oneshots.iter_mut().find(|o| o.id == id) else { return };
        let open = !(hz > 0.0 && hz < super::BusEffects::OPEN);
        let cutoff = if open { super::BusEffects::OPEN } else { hz };
        if let Some(f) = shot.lowpass.as_ref() {
            let _ = f.frequency().set_target_at_time(cutoff, ctx.current_time(), 0.05);
            return;
        }
        if open {
            return;
        }
        let Ok(filter) = ctx.create_biquad_filter() else { return };
        filter.set_type(web_sys::BiquadFilterType::Lowpass);
        filter.frequency().set_value(cutoff);
        let next: AudioNode = match shot.panner.as_ref() {
            Some(p) => p.clone().into(),
            None => shot.dest.clone(),
        };
        let _ = shot.gain.disconnect();
        if shot.gain.connect_with_audio_node(&filter).is_ok() && filter.connect_with_audio_node(&next).is_ok() {
            shot.lowpass = Some(filter);
        } else {
            let _ = shot.gain.connect_with_audio_node(&next);
        }
    }

    pub fn set_pan(&mut self, id: u64, pan: f32) {
        let pan = pan.clamp(-1.0, 1.0);
        let Some(ctx) = self.ctx.as_ref() else { return };
        let Some(shot) = self.oneshots.iter_mut().find(|o| o.id == id) else { return };
        let master = shot.dest.clone();
        let master = &master;
        if let Some(p) = shot.panner.as_ref() {
            p.pan().set_value(pan);
            return;
        }
        if pan.abs() <= f32::EPSILON {
            return;
        }
        let Ok(panner) = ctx.create_stereo_panner() else { return };
        panner.pan().set_value(pan);
        // After the low-pass, if the sound has one.
        let from: AudioNode = match shot.lowpass.as_ref() {
            Some(f) => f.clone().into(),
            None => shot.gain.clone().into(),
        };
        let _ = from.disconnect();
        if from.connect_with_audio_node(&panner).is_ok() && panner.connect_with_audio_node(master).is_ok() {
            shot.panner = Some(panner);
        } else {
            let _ = from.connect_with_audio_node(master);
        }
    }

    /// Start the world's `auto_play` sources (`behaviors::collect_autoplay_audio`),
    /// each on its `bus_id` - a looping source loops on its own sound rather
    /// than replacing the music, as natively. A clip still **decoding** is
    /// tried again next frame: decoding is asynchronous here, and the source
    /// used to be marked started before the play was tried, so a scene's
    /// ambience was lost whenever its first frame beat the decoder. A clip
    /// that is missing or failed is warned about once and left.
    pub fn start_autoplay(
        &mut self,
        world: &mut hecs::World,
        mixer: &mut AudioMixer,
        spatial: &mut super::spatial::SpatialAudio,
        warn: &mut dyn FnMut(String),
    ) {
        for r in crate::ecs::behaviors::collect_autoplay_audio(world) {
            let bus = mixer.route(r.bus_id.as_deref(), super::SFX_BUS);
            match self.play_sound_on_bus(mixer, &bus, &r.clip_id, r.volume, r.pitch, r.pan, r.looping) {
                Ok(id) => {
                    if let Some(old) = spatial.started(r.entity, id, &r.clip_id) {
                        self.stop_id(old);
                        mixer.unregister_track(old);
                    }
                }
                Err(_) if self.is_loading(&r.clip_id) => crate::ecs::behaviors::retry_autoplay(world, r.entity),
                Err(e) => warn(format!("autoplay {}: {e}", r.clip_id)),
            }
        }
    }

    /// Whether `clip` has been handed over and is still decoding.
    pub fn is_loading(&self, clip: &str) -> bool {
        matches!(self.clips.borrow().get(clip), Some(Clip::Loading))
    }

    /// Positional audio's frame for the browsers (`audio::spatial`): the twin
    /// of the native `ecs::systems::apply_spatial_audio`, on this backend.
    pub fn apply_spatial(
        &mut self,
        world: &hecs::World,
        mixer: &mut AudioMixer,
        spatial: &mut super::spatial::SpatialAudio,
    ) {
        // Zones first: they need no positional sound to be playing.
        super::zones::apply(world, mixer);
        if spatial.is_empty() {
            return;
        }
        let frame = spatial.update(world, |h| self.is_playing(h));
        for h in frame.stop {
            self.stop_id(h);
            mixer.unregister_track(h);
        }
        for m in frame.mix {
            if let Some(out) = mixer.set_track_volume(m.handle, m.volume) {
                self.set_volume(m.handle, out);
            }
            self.set_pan(m.handle, m.pan);
            self.set_lowpass(m.handle, m.cutoff);
        }
    }

    /// Stop one one-shot by id.
    pub fn stop_id(&mut self, id: u64) {
        if let Some(i) = self.oneshots.iter().position(|o| o.id == id) {
            let shot = self.oneshots.remove(i);
            let _ = shot.source.stop();
        }
    }

    /// Whether `id` is a sound that is still playing or paused.
    pub fn is_playing(&self, id: u64) -> bool {
        let now = self.ctx.as_ref().map(|c| c.current_time()).unwrap_or(0.0);
        self.oneshots
            .iter()
            .any(|o| o.id == id && (o.paused_offset.is_some() || o.ends_at > now))
    }

    // -- Mixer-aware entry points --------------------------------------
    //
    // Both wasm hosts call these instead of multiplying bus volumes
    // themselves. Each used to scale a sound by its bus once, at play time,
    // and never again - so a bus slider did nothing to anything already
    // playing - and `set_music_volume` wrote a raw gain that ignored the bus
    // and dropped the volume `play_music` had been given.

    /// Play a one-shot on `bus` at its own `volume`, registered with `mixer`.
    #[allow(clippy::too_many_arguments)]
    pub fn play_sound_on_bus(
        &mut self,
        mixer: &mut AudioMixer,
        bus: &str,
        clip: &str,
        volume: f32,
        pitch: f32,
        pan: f32,
        looping: bool,
    ) -> Result<u64, String> {
        let out = mixer.output_volume(bus, volume);
        let id = self.play_sound_routed(clip, out, pitch, pan, looping, bus)?;
        mixer.register_track(id, bus, volume);
        Ok(id)
    }

    /// Once a frame: forget finished sounds and push bus changes to the rest.
    /// The browser twin of [`crate::audio::sync_mixer`].
    pub fn sync_mixer(&mut self, mixer: &mut AudioMixer) {
        mixer.retain_tracks(|id| self.is_playing(id));
        for (id, volume) in mixer.take_volume_updates() {
            self.set_volume(id, volume);
        }
        // Bus tracks and their effects (`BusEffects`), as natively.
        if let Some(plan) = mixer.take_bus_plan() {
            self.apply_buses(&plan);
        }
    }

    pub fn set_master_volume(&mut self, volume: f32) {
        self.master_vol = volume.clamp(0.0, 4.0);
        if let Some(g) = self.master_gain.as_ref() {
            g.gain().set_value(self.effective_master());
        }
    }

    /// The master as it sounds: the game's volume, unless the page muted it.
    fn effective_master(&self) -> f32 {
        if self.host_muted { 0.0 } else { self.master_vol }
    }

    /// Mute or unmute for the page (a portal), leaving the game's own master
    /// volume as the game set it.
    pub fn set_host_muted(&mut self, muted: bool) {
        self.host_muted = muted;
        if let Some(g) = self.master_gain.as_ref() {
            g.gain().set_value(self.effective_master());
        }
    }

    /// Stop or restart every sound for the page (a portal pausing the game).
    /// The context's clock stops with it, so a one-shot's expiry - arithmetic
    /// against `current_time` - holds still too.
    pub fn set_host_paused(&mut self, paused: bool) {
        self.host_paused = paused;
        let Some(ctx) = self.ctx.as_ref() else { return };
        if paused {
            let _ = ctx.suspend();
        } else if ctx.state() == AudioContextState::Suspended {
            let _ = ctx.resume();
        }
    }
}

/// The browsers' backend as the music director's output
/// ([`super::music::MusicOut`]): voices are sounds on the music bus, through
/// the mixer as every sound is.
pub struct WebOut<'a> {
    pub audio: &'a mut WebAudioBackend,
    pub mixer: &'a mut AudioMixer,
}

impl super::music::MusicOut for WebOut<'_> {
    fn play_voice(&mut self, name: &str, samples: &[f32], rate: u32, bus: &str) -> Option<u64> {
        if let Err(e) = self.audio.insert_samples(name, samples, rate) {
            log::warn!("[synth] {e}");
            return None;
        }
        let out = self.mixer.output_volume(bus, 1.0);
        self.audio.play_routed_at(name, out, 1.0, 0.0, false, bus, 0.0).ok()
    }
    fn feed_tap(&mut self) {
        self.audio.tap();
    }
    fn start(&mut self, clip: &str, volume: f32, looping: bool, pitch: f32, offset: f64) -> Option<u64> {
        let out = self.mixer.output_volume(MUSIC_BUS, volume);
        let id = self.audio.play_routed_at(clip, out, pitch, 0.0, looping, MUSIC_BUS, offset).ok()?;
        self.mixer.register_track(id, MUSIC_BUS, volume);
        Some(id)
    }
    fn set_volume(&mut self, id: u64, volume: f32) {
        if let Some(out) = self.mixer.set_track_volume(id, volume) {
            self.audio.set_volume(id, out);
        }
    }
    fn set_pitch(&mut self, id: u64, pitch: f32) {
        self.audio.set_rate(id, pitch);
    }
    fn seek(&mut self, id: u64, seconds: f64) {
        self.audio.seek_id(id, seconds);
    }
    fn stop(&mut self, id: u64) {
        self.audio.stop_id(id);
        self.mixer.unregister_track(id);
    }
    fn pause(&mut self, id: u64) {
        self.audio.pause_id(id);
    }
    fn resume(&mut self, id: u64) {
        self.audio.resume_id(id);
    }
    fn position(&self, id: u64) -> Option<f64> {
        self.audio.position_of(id)
    }
    fn duration(&self, clip: &str) -> Option<f64> {
        self.audio.clip_duration(clip)
    }
    fn is_playing(&self, id: u64) -> bool {
        self.audio.is_playing(id)
    }
}
