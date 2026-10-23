//! **The mix's spectrum, and notes made from numbers** (`Audio.get_spectrum`,
//! `Audio.get_level`, `Audio.synth`; Oct 2026; `audio::analysis`). Before
//! this a game could not see its music - no beat to pulse a light by - and
//! every sound had to be a file. Every runtime feeds one ring (a kira effect
//! on the master track natively, an `AnalyserNode` in the browsers) and one
//! analysis reads it, so a band means the same everywhere; a voice is
//! rendered here and handed to the backend by `music::drive`, which every
//! host already calls. Found by the showcase's Equaliser.

use std::sync::Mutex;

use jengine_core::audio::analysis::{self, Voice, Wave};
use jengine_core::audio::music::{self, MusicOut};

/// The ring and the queue are process-wide; the tests take turns.
static TURN: Mutex<()> = Mutex::new(());

fn sine(freq: f32, amp: f32, rate: u32, n: usize) -> Vec<f32> {
    (0..n).map(|i| (i as f32 / rate as f32 * freq * std::f32::consts::TAU).sin() * amp).collect()
}

#[test]
fn a_tone_lights_its_own_band() {
    let rate = 48_000;
    let mags = analysis::magnitudes(&sine(1000.0, 0.8, rate, analysis::WINDOW));
    let peak = mags.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap();
    let hz = peak.0 as f32 * rate as f32 / analysis::WINDOW as f32;
    assert!((hz - 1000.0).abs() < 30.0, "the peak is at the tone: {hz} Hz");
    assert!((peak.1 - 0.8).abs() < 0.15, "and about its amplitude: {}", peak.1);
    let bands = analysis::bands_of(&mags, rate, 16);
    let loudest = bands.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
    // 16 bands, log-spaced 40 Hz..16 kHz: 1 kHz falls in band 8.
    assert_eq!(loudest, 8, "1 kHz is the ninth of sixteen bands: {bands:?}");
    assert!(bands[0] < 0.4 && bands[15] < 0.4, "the ends stay low: {bands:?}");
}

#[test]
fn the_ring_answers_level_and_spectrum() {
    let _t = TURN.lock().unwrap_or_else(|p| p.into_inner());
    analysis::clear();
    assert_eq!(analysis::level(), 0.0, "silence");
    assert_eq!(analysis::spectrum(8), vec![0.0; 8]);
    analysis::feed(sine(220.0, 0.5, 48_000, 4096), 48_000);
    let level = analysis::level();
    assert!((level - 0.5 / 2f32.sqrt()).abs() < 0.02, "a sine's RMS is its amplitude over root two: {level}");
    let s = analysis::spectrum(8);
    assert_eq!(s.len(), 8);
    assert!(s.iter().any(|&v| v > 0.8), "a loud tone fills its band: {s:?}");
    // The browsers hand a whole window at a time.
    analysis::set_window(&vec![0.0; analysis::WINDOW], 44_100);
    assert_eq!(analysis::level(), 0.0);
    analysis::clear();
}

#[test]
fn a_voice_is_its_envelope_and_its_wave() {
    let v = Voice { wave: Wave::Square, freq: 100.0, duration: 0.2, attack: 0.05, decay: 0.05, sustain: 0.5, release: 0.1, volume: 1.0, ..Default::default() };
    assert_eq!(v.envelope(0.025), 0.5, "half way up the attack");
    assert_eq!(v.envelope(0.05), 1.0, "the top of the attack");
    assert!((v.envelope(0.15) - 0.5).abs() < 1e-6, "held at the sustain");
    assert!((v.envelope(0.25) - 0.25).abs() < 1e-6, "half way through the release");
    assert_eq!(v.envelope(0.31), 0.0, "and then nothing");
    let s = v.render(1000);
    assert_eq!(s.len(), 300, "duration and release, at 1000 a second");
    assert!(s.iter().all(|x| x.abs() <= 1.0));
    assert!((s[150].abs() - 0.5).abs() < 1e-3, "a square wave at the sustain level: {}", s[150]);
    assert_eq!(analysis::note_hz(69.0), 440.0);
    assert!((analysis::note_hz(81.0) - 880.0).abs() < 1e-3, "an octave up doubles it");
}

#[test]
fn a_script_table_reads_as_a_voice() {
    let v = Voice::from_json(&serde_json::json!({
        "wave": "saw", "note": 57, "slide": 110, "duration": 0.4, "attack": 0.02,
        "decay": 0.1, "sustain": 0.3, "release": 0.5, "volume": 0.6, "bus": "music"
    }));
    assert_eq!(v.wave, Wave::Saw);
    assert!((v.freq - 220.0).abs() < 1e-3, "MIDI 57 is A3: {}", v.freq);
    assert_eq!((v.freq_end, v.duration, v.sustain, v.volume, v.bus.as_str()), (Some(110.0), 0.4, 0.3, 0.6, "music"));
    assert_eq!(Voice::from_json(&serde_json::Value::Null), Voice::default());
    assert_eq!(v.clip_name(), v.clone().clip_name(), "the same note is the same clip");
    assert_ne!(v.clip_name(), Voice::default().clip_name());
    let wav = analysis::wav_bytes(&[0.0, 1.0, -1.0], 8000);
    assert_eq!((&wav[0..4], &wav[8..12], wav.len()), (&b"RIFF"[..], &b"WAVE"[..], 44 + 6));
}

#[derive(Default)]
struct Out {
    played: Vec<(String, usize, u32, String)>,
    tapped: usize,
}

impl MusicOut for Out {
    fn start(&mut self, _: &str, _: f32, _: bool, _: f32, _: f64) -> Option<u64> {
        None
    }
    fn set_volume(&mut self, _: u64, _: f32) {}
    fn set_pitch(&mut self, _: u64, _: f32) {}
    fn seek(&mut self, _: u64, _: f64) {}
    fn stop(&mut self, _: u64) {}
    fn pause(&mut self, _: u64) {}
    fn resume(&mut self, _: u64) {}
    fn position(&self, _: u64) -> Option<f64> {
        None
    }
    fn duration(&self, _: &str) -> Option<f64> {
        None
    }
    fn is_playing(&self, _: u64) -> bool {
        false
    }
    fn play_voice(&mut self, name: &str, samples: &[f32], rate: u32, bus: &str) -> Option<u64> {
        self.played.push((name.to_string(), samples.len(), rate, bus.to_string()));
        Some(1)
    }
    fn feed_tap(&mut self) {
        self.tapped += 1;
    }
}

#[test]
fn the_music_step_plays_queued_notes_and_taps_the_mix() {
    let _t = TURN.lock().unwrap_or_else(|p| p.into_inner());
    analysis::take();
    analysis::request(Voice { duration: 0.1, release: 0.1, ..Default::default() });
    analysis::request(Voice { bus: "music".into(), ..Default::default() });
    let mut out = Out::default();
    music::drive_at(0.0, &mut out);
    assert_eq!(out.tapped, 1, "the tap is fed once a frame");
    assert_eq!(out.played.len(), 2, "both notes played: {:?}", out.played);
    assert_eq!((out.played[0].1, out.played[0].2, out.played[0].3.as_str()), (8820, analysis::SYNTH_RATE, "sfx"));
    assert_eq!(out.played[1].3, "music");
    music::drive_at(0.1, &mut out);
    assert_eq!(out.played.len(), 2, "and only once");
}

/// Every op the browser preamble sends is one the hook answers.
#[test]
fn the_browser_hook_answers_every_op_the_preamble_sends() {
    let preamble = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
    let hook = include_str!("../src/scripting/bridge/adapters/js/analysis.rs");
    let ops: Vec<&str> = preamble.split("__analysis('").skip(1).map(|s| s.split('\'').next().unwrap()).collect();
    assert_eq!(ops.len(), 3, "the scan found {ops:?}");
    for op in ops {
        assert!(hook.contains(&format!("\"{op}\" =>")), "the preamble sends '{op}' and the hook has no arm for it");
    }
}

#[cfg(feature = "native")]
#[test]
fn a_native_script_reads_the_spectrum_and_plays_a_note() {
    use hecs::World;
    use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::input::InputState;
    let _t = TURN.lock().unwrap_or_else(|p| p.into_inner());
    analysis::clear();
    analysis::take();
    analysis::feed(sine(440.0, 0.6, 48_000, 4096), 48_000);
    let src = r#"
local done = false
function update(entity, dt)
    if done then return end
    done = true
    local s = Audio.get_spectrum(12)
    Entity.set_custom(entity, "Probe", "bands", #s)
    Entity.set_custom(entity, "Probe", "level", Audio.get_level())
    Audio.synth({ wave = "triangle", note = 60, duration = 0.2 })
end
"#;
    let dir = std::env::temp_dir().join(format!("jengine_synth_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("synth.lua");
    std::fs::write(&path, src).unwrap();
    let mut world = World::new();
    let e = world.spawn((
        Name("Probe".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..2 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    let c = world.get::<&CustomComponents>(e).unwrap();
    let probe = c.0.get("Probe").and_then(|p| p.as_object()).expect("the script wrote its probe");
    assert_eq!(probe["bands"].as_f64(), Some(12.0));
    assert!(probe["level"].as_f64().unwrap() > 0.3);
    let queued = analysis::take();
    assert_eq!(queued.len(), 1, "the note is queued for the music step");
    assert_eq!(queued[0].wave, Wave::Triangle);
    analysis::clear();
}
