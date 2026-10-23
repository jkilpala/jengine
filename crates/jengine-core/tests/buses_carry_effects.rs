//! Bus effects (`audio::BusEffects`): every mixer bus can low-pass and
//! reverberate what plays on it, set by a script (`Audio.set_bus_effect`) or
//! by an `audio_zone` the listener stands in. The mixer works out what each
//! bus should sound like and hands the backend a plan through `sync_mixer`;
//! the sounds play on their bus's track.

use glam::Vec2;
use hecs::World;
use jengine_core::audio::zones::{AudioZone, zone_effects};
use jengine_core::audio::{
    AudioBackend, AudioMixer, BusEffects, BusPlan, ClipHandle, MASTER_BUS, MUSIC_BUS, PlaySettings, SFX_BUS, SoundHandle,
    sync_mixer,
};
use jengine_core::ecs::components::Transform;

#[derive(Default)]
struct Recorder {
    plans: Vec<BusPlan>,
    played_on: Vec<String>,
}

impl AudioBackend for Recorder {
    fn load_clip(&mut self, _id: &str, _data: &[u8]) -> anyhow::Result<ClipHandle> {
        Ok(ClipHandle(1))
    }
    fn get_clip_by_name(&self, _name: &str) -> Option<ClipHandle> {
        Some(ClipHandle(1))
    }
    fn play(&mut self, _clip: ClipHandle, s: PlaySettings) -> anyhow::Result<SoundHandle> {
        self.played_on.push(s.bus);
        Ok(SoundHandle(self.played_on.len() as u64))
    }
    fn stop(&mut self, _h: SoundHandle) {}
    fn pause(&mut self, _h: SoundHandle) {}
    fn resume(&mut self, _h: SoundHandle) {}
    fn set_volume(&mut self, _h: SoundHandle, _v: f32) {}
    fn set_global_volume(&mut self, _v: f32) {}
    fn is_playing(&self, _h: SoundHandle) -> bool {
        true
    }
    fn apply_buses(&mut self, plan: &BusPlan) {
        self.plans.push(plan.clone());
    }
    fn update(&mut self) {}
}

fn listed(plan: &BusPlan) -> Vec<&str> {
    plan.buses.iter().map(|(n, _, _)| n.as_str()).collect()
}


/// The shared command arms both browser hosts call (`adapters::js::host_dispatch`).
const DISPATCH: &str = include_str!("../src/scripting/bridge/adapters/js/host_dispatch.rs");

/// A host's source as far as its commands go: its own, plus the shared arms -
/// but only if it calls them, so a host that dropped the call fails.
fn with_dispatch(src: &str) -> String {
    if src.contains("host_dispatch::handle_host_command(") {
        format!("{src}
{DISPATCH}")
    } else {
        src.to_string()
    }
}

#[test]
fn the_backend_hears_the_tree_then_only_what_changed() {
    let mut mixer = AudioMixer::new();
    let mut rec = Recorder::default();
    sync_mixer(&mut rec, &mut mixer);
    let first = rec.plans.last().expect("a first plan").clone();
    assert!(first.tree);
    assert_eq!(listed(&first)[0], MASTER_BUS, "parents come first");
    assert!(listed(&first).contains(&SFX_BUS) && listed(&first).contains(&MUSIC_BUS));

    // Nothing changed: nothing sent.
    sync_mixer(&mut rec, &mut mixer);
    assert_eq!(rec.plans.len(), 1);

    mixer.set_effect(SFX_BUS, "lowpass", 800.0).unwrap();
    mixer.set_effect(SFX_BUS, "reverb", 0.3).unwrap();
    sync_mixer(&mut rec, &mut mixer);
    let plan = rec.plans.last().unwrap();
    assert!(!plan.tree);
    assert_eq!(listed(plan), vec![SFX_BUS]);
    assert_eq!(plan.buses[0].2.cutoff(), Some(800.0));
    assert!((plan.buses[0].2.reverb - 0.3).abs() < 1e-6);

    // A tiny change is not worth a message; a new bus re-sends the tree.
    mixer.set_effect(SFX_BUS, "reverb", 0.301).unwrap();
    sync_mixer(&mut rec, &mut mixer);
    assert_eq!(rec.plans.len(), 2);
    assert!(mixer.create_bus("voice", Some(SFX_BUS), 1.0));
    sync_mixer(&mut rec, &mut mixer);
    let plan = rec.plans.last().unwrap();
    assert!(plan.tree);
    let names = listed(plan);
    assert!(names.iter().position(|n| *n == SFX_BUS) < names.iter().position(|n| *n == "voice"));

    assert!(mixer.set_effect("nope", "lowpass", 1.0).is_err());
    assert!(mixer.set_effect(SFX_BUS, "chorus", 1.0).is_err());
}

#[test]
fn sounds_play_on_their_bus() {
    let mut rec = Recorder::default();
    let mut mixer = AudioMixer::new();
    let assets = jengine_core::asset::AssetManager::new();
    jengine_core::ecs::systems::play_sfx(&mut rec, &assets, &mut mixer, "hit.wav", 1.0);
    assert_eq!(rec.played_on, vec![SFX_BUS.to_string()]);
}

fn zone_world(zone: AudioZone, listener: Vec2) -> World {
    let mut world = World::new();
    world.spawn((Transform::default(), zone));
    world.spawn((
        Transform { position: listener, ..Default::default() },
        jengine_core::audio::spatial::AudioListener::default(),
    ));
    world
}

#[test]
fn a_zone_gives_its_bus_effects_while_the_listener_is_in_it() {
    let cave = AudioZone { size: [400.0, 400.0], reverb: 0.6, room: 0.9, lowpass: 0.0, fade: 100.0, ..Default::default() };
    // Outside: nothing.
    let out = zone_effects(&zone_world(cave.clone(), Vec2::new(300.0, 0.0)));
    assert!(out.is_empty());
    // At the edge's fade: half.
    let half = zone_effects(&zone_world(cave.clone(), Vec2::new(150.0, 0.0)));
    assert!((half[SFX_BUS].reverb - 0.3).abs() < 1e-4, "{half:?}");
    // At the heart: all of it.
    let full = zone_effects(&zone_world(cave.clone(), Vec2::ZERO));
    assert!((full[SFX_BUS].reverb - 0.6).abs() < 1e-5);

    // Through the mixer: the zone and the script's own setting combine, and
    // walking out sends the bus back to dry.
    let mut mixer = AudioMixer::new();
    let mut rec = Recorder::default();
    sync_mixer(&mut rec, &mut mixer);
    mixer.set_effect(SFX_BUS, "lowpass", 3000.0).unwrap();
    let water = AudioZone { size: [400.0, 400.0], lowpass: 600.0, fade: 0.0, ..Default::default() };
    jengine_core::audio::zones::apply(&zone_world(water.clone(), Vec2::ZERO), &mut mixer);
    let fx = mixer.effective_effects(SFX_BUS);
    assert_eq!(fx.cutoff(), Some(600.0), "the lower cutoff wins");
    sync_mixer(&mut rec, &mut mixer);
    assert_eq!(rec.plans.last().unwrap().buses[0].2.cutoff(), Some(600.0));
    jengine_core::audio::zones::apply(&zone_world(water, Vec2::new(900.0, 0.0)), &mut mixer);
    sync_mixer(&mut rec, &mut mixer);
    assert_eq!(rec.plans.last().unwrap().buses[0].2.cutoff(), Some(3000.0), "back to the script's own");
}

#[test]
fn effects_combine_and_compare() {
    let a = BusEffects { lowpass: 2000.0, reverb: 0.2, room: 0.3, ..Default::default() };
    let b = BusEffects { lowpass: 0.0, reverb: 0.5, room: 0.9, ..Default::default() };
    let c = a.combine(b);
    assert_eq!((c.cutoff(), c.reverb, c.room), (Some(2000.0), 0.5, 0.9));
    assert_eq!(BusEffects { lowpass: 25_000.0, ..Default::default() }.cutoff(), None, "past hearing is off");
    assert!(!a.differs(&BusEffects { lowpass: 2005.0, ..a }));
    assert!(a.differs(&BusEffects { lowpass: 2100.0, ..a }));
}

#[test]
fn the_kira_backend_builds_its_bus_tracks() {
    // Needs an output device; a machine without one has nothing to test.
    let Ok(mut kira) = jengine_core::audio::kira_backend::KiraAudioBackend::new() else {
        eprintln!("no audio device - skipping");
        return;
    };
    let mut mixer = AudioMixer::new();
    mixer.create_bus("voice", Some(SFX_BUS), 1.0);
    mixer.set_effect(SFX_BUS, "reverb", 0.5).unwrap();
    mixer.set_effect(MASTER_BUS, "lowpass", 900.0).unwrap();
    sync_mixer(&mut kira, &mut mixer);
    // A plain sound on a bus with effects plays and can be stopped.
    let wav = {
        let mut bytes = Vec::new();
        let n = 4410u32;
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + n * 2).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&44100u32.to_le_bytes());
        bytes.extend_from_slice(&(44100u32 * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(n * 2).to_le_bytes());
        bytes.extend(std::iter::repeat_n(0u8, (n * 2) as usize));
        bytes
    };
    let clip = kira.load_clip("silence", &wav).expect("decodes");
    for bus in ["voice", SFX_BUS, MUSIC_BUS, ""] {
        let h = kira
            .play(clip, PlaySettings { volume: 0.0, bus: bus.to_string(), ..Default::default() })
            .expect("plays on its bus");
        kira.stop(h);
    }
    // Removing a bus re-sends the tree; the backend drops its track.
    mixer.remove_bus("voice");
    sync_mixer(&mut kira, &mut mixer);
    let h = kira.play(clip, PlaySettings { volume: 0.0, bus: "voice".into(), ..Default::default() }).unwrap();
    kira.stop(h);
}

#[cfg(feature = "native")]
#[test]
fn a_script_sets_a_bus_effect() {
    use jengine_core::ecs::components::{ScriptComponent, ScriptLanguage};
    use jengine_core::ecs::systems::ScriptSystem;
    let mut world = World::new();
    let dir = std::env::temp_dir().join(format!("jengine_busfx_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.lua");
    std::fs::write(&path, "function update(e, dt)\n  Audio.set_bus_effect(\"sfx\", \"lowpass\", 700)\n  Audio.set_bus_effect(\"music\", \"reverb\", 0.4)\nend\n").unwrap();
    world.spawn((
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    system.run(&mut world, 1.0 / 60.0, &jengine_core::input::InputState::new(), None, None);
    let mixer = &system.api.services.mixer;
    assert_eq!(mixer.effects(SFX_BUS).cutoff(), Some(700.0));
    assert!((mixer.effects(MUSIC_BUS).reverb - 0.4).abs() < 1e-6);
}

#[test]
fn every_runtime_hands_its_backend_the_bus_plan() {
    let web = include_str!("../src/audio/web_audio.rs");
    assert!(web.contains("mixer.take_bus_plan()"), "the browsers never build their bus chains");
    assert!(web.contains("super::zones::apply(world, mixer)"), "the browsers ignore audio zones");
    let systems = include_str!("../src/ecs/systems.rs");
    assert!(systems.contains("crate::audio::zones::apply(world, mixer)"), "native ignores audio zones");
    let core = include_str!("../src/audio/mod.rs");
    assert!(core.contains("audio.apply_buses(&plan)"));
    for (name, src) in [
        ("editor", include_str!("../../jengine-editor-viewport/src/lib.rs")),
        ("web", include_str!("../../jengine-web/src/script_host.rs")),
    ] {
        assert!(with_dispatch(src).contains("\"fx\" =>"), "{name} drops Audio.set_bus_effect");
    }
}
