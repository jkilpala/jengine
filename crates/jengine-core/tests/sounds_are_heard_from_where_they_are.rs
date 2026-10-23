//! **Positional audio** (Oct 2026): an `AudioSource` with `spatial` is louder
//! near the listener and panned to its side, every frame as either moves, and
//! `Audio.play_sound_at` plays a one-shot from a point. Before it every sound
//! played centred at full volume wherever it was - and a looping source went
//! on playing after its entity was gone. Found by the showcase's Night Market.
//!
//! The curve is unit-tested in `audio::spatial`; this runs the tracker
//! through the native host's `apply_spatial_audio` against a recording
//! backend, the autoplay start, a real Luau script, and checks every host
//! is wired.

use std::collections::HashMap;

use glam::Vec2;
use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::audio::spatial::{AudioListener, SpatialAudio};
use jengine_core::audio::{AudioBackend, AudioMixer, ClipHandle, PlaySettings, SoundHandle};
use jengine_core::ecs::behaviors::collect_autoplay_audio;
use jengine_core::ecs::components::{AudioSource, Camera2DComponent, Transform};
use jengine_core::ecs::systems::{apply_spatial_audio, play_autoplay_audio};

/// Stands in for kira: remembers each sound's volume and pan.
#[derive(Default)]
struct Recorder {
    clips: HashMap<String, u64>,
    live: HashMap<u64, (f32, f32)>,
    next: u64,
}

impl AudioBackend for Recorder {
    fn load_clip(&mut self, id: &str, _data: &[u8]) -> anyhow::Result<ClipHandle> {
        let h = self.clips.len() as u64 + 1;
        self.clips.insert(id.to_string(), h);
        Ok(ClipHandle(h))
    }
    fn get_clip_by_name(&self, name: &str) -> Option<ClipHandle> {
        self.clips.get(name).copied().map(ClipHandle)
    }
    fn play(&mut self, _clip: ClipHandle, s: PlaySettings) -> anyhow::Result<SoundHandle> {
        self.next += 1;
        self.live.insert(self.next, (s.volume, s.pan));
        Ok(SoundHandle(self.next))
    }
    fn stop(&mut self, h: SoundHandle) {
        self.live.remove(&h.0);
    }
    fn pause(&mut self, _h: SoundHandle) {}
    fn resume(&mut self, _h: SoundHandle) {}
    fn set_volume(&mut self, h: SoundHandle, volume: f32) {
        if let Some(e) = self.live.get_mut(&h.0) {
            e.0 = volume;
        }
    }
    fn set_pan(&mut self, h: SoundHandle, pan: f32) {
        if let Some(e) = self.live.get_mut(&h.0) {
            e.1 = pan;
        }
    }
    fn set_global_volume(&mut self, _volume: f32) {}
    fn is_playing(&self, h: SoundHandle) -> bool {
        self.live.contains_key(&h.0)
    }
    fn update(&mut self) {}
}

struct Rig {
    world: World,
    audio: Recorder,
    assets: AssetManager,
    mixer: AudioMixer,
    spatial: SpatialAudio,
}

impl Rig {
    fn new() -> Self {
        let mut assets = AssetManager::new();
        for clip in ["amb/fire.ogg", "amb/hum.ogg"] {
            assets.insert_audio(clip.to_string(), vec![0]);
        }
        let mut world = World::new();
        // The listener: the active camera at the origin.
        world.spawn((Transform::default(), Camera2DComponent { is_active: true, ..Default::default() }));
        Self { world, audio: Recorder::default(), assets, mixer: AudioMixer::new(), spatial: SpatialAudio::new() }
    }

    fn source(&mut self, x: f32, spatial: bool, clip: &str) -> hecs::Entity {
        self.world.spawn((
            Transform { position: Vec2::new(x, 0.0), ..Default::default() },
            AudioSource {
                clip_id: clip.into(),
                looping: true,
                auto_play: true,
                spatial,
                min_distance: 100.0,
                max_distance: 500.0,
                pan_width: 200.0,
                ..Default::default()
            },
        ))
    }

    /// A host's frame: start what wants starting, then mix.
    fn frame(&mut self) {
        let requests = collect_autoplay_audio(&mut self.world);
        play_autoplay_audio(&requests, &mut self.audio, &self.assets, &mut self.mixer, &mut self.spatial);
        apply_spatial_audio(&self.world, &mut self.audio, &mut self.mixer, &mut self.spatial);
    }

    fn only(&self) -> (f32, f32) {
        assert_eq!(self.audio.live.len(), 1, "{:?}", self.audio.live);
        *self.audio.live.values().next().unwrap()
    }
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
fn a_source_fades_and_pans_as_it_moves_and_stops_when_it_goes() {
    let mut rig = Rig::new();
    let fire = rig.source(300.0, true, "amb/fire.ogg");
    rig.frame();
    // Started at its place: half way through its fade, panned hard right.
    let (v, p) = rig.only();
    assert!((v - 0.25).abs() < 1e-4 && p == 1.0, "starts where it is: {v} {p}");
    // Brought in close on the left: full volume, panned left.
    rig.world.get::<&mut Transform>(fire).unwrap().position = Vec2::new(-50.0, 0.0);
    rig.frame();
    assert_eq!(rig.only(), (1.0, -0.25));
    // Out of earshot: silent, still playing.
    rig.world.get::<&mut Transform>(fire).unwrap().position = Vec2::new(0.0, 900.0);
    rig.frame();
    assert_eq!(rig.only().0, 0.0);
    // The listener can be an entity instead of the camera.
    rig.world.spawn((Transform { position: Vec2::new(0.0, 880.0), ..Default::default() }, AudioListener::default()));
    rig.frame();
    assert_eq!(rig.only().0, 1.0, "next to the player now");
    // Gone: its sound stops.
    rig.world.despawn(fire).unwrap();
    rig.frame();
    assert!(rig.audio.live.is_empty(), "the despawned source's loop stopped");
}

#[test]
fn a_plain_source_is_left_alone_but_still_stops_with_its_entity() {
    let mut rig = Rig::new();
    let hum = rig.source(800.0, false, "amb/hum.ogg");
    rig.frame();
    assert_eq!(rig.only(), (1.0, 0.0), "not spatial: full volume, centred, however far");
    rig.frame();
    assert_eq!(rig.only(), (1.0, 0.0));
    rig.world.despawn(hum).unwrap();
    rig.frame();
    assert!(rig.audio.live.is_empty(), "a looping torch used to play on after its torch was gone");
}

#[test]
fn a_reused_entity_id_stops_the_old_scenes_loop() {
    // A scene swap builds a new world, whose entities reuse the old ids.
    let mut rig = Rig::new();
    let e = rig.source(0.0, true, "amb/fire.ogg");
    rig.frame();
    let old = *rig.audio.live.keys().next().unwrap();
    let mut fresh = World::new();
    fresh.spawn((Transform::default(), Camera2DComponent { is_active: true, ..Default::default() }));
    rig.world = fresh;
    let again = rig.source(0.0, true, "amb/fire.ogg");
    assert_eq!(again.id(), e.id(), "the test needs the id reused");
    rig.frame();
    assert!(!rig.audio.live.contains_key(&old), "the old scene's loop was stopped");
    assert_eq!(rig.audio.live.len(), 1);
}

#[cfg(feature = "native")]
#[test]
fn a_script_plays_a_sound_from_a_point() {
    use jengine_core::ecs::components::{ScriptComponent, ScriptLanguage};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::scripting::api::ScriptCommand;
    const SRC: &str = r#"
function update(entity, dt)
    Audio.play_sound_at("amb/fire.ogg", -300, 0, 0.8)
    Audio.play_sound_at("amb/fire.ogg", 2000, 0)
end
"#;
    let mut world = World::new();
    world.spawn((Transform::default(), Camera2DComponent { is_active: true, ..Default::default() }));
    let dir = std::env::temp_dir().join(format!("jengine_spatial_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.lua");
    std::fs::write(&path, SRC).unwrap();
    world.spawn((
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    system.run(&mut world, 1.0 / 60.0, &jengine_core::input::InputState::new(), None, None);
    let played: Vec<(f32, f32)> = system
        .api
        .commands
        .iter()
        .filter_map(|c| match c {
            ScriptCommand::PlaySound { volume, pan, .. } => Some((*volume, *pan)),
            _ => None,
        })
        .collect();
    assert_eq!(played.len(), 2, "{:?}", system.api.commands);
    // 300 left with the default falloff (120 / 1000 / 600): (1 - 180/880)^2, half left.
    let want = (1.0f32 - 180.0 / 880.0).powi(2) * 0.8;
    assert!((played[0].0 - want).abs() < 1e-4 && (played[0].1 + 0.5).abs() < 1e-4, "{played:?}");
    assert_eq!(played[1].0, 0.0, "out of earshot");
}

#[test]
fn every_host_tracks_and_mixes_its_sources() {
    for (name, src, mix) in [
        ("native", include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs"), "apply_spatial_audio("),
        ("editor", include_str!("../../jengine-editor-viewport/src/lib.rs"), ".apply_spatial("),
        ("web", include_str!("../../jengine-web/src/lib.rs"), ".apply_spatial("),
    ] {
        assert!(src.contains(mix), "{name} never mixes its spatial sources - they play centred at full volume");
        // The browsers start their autoplay sources through one function,
        // `WebAudioBackend::start_autoplay`, which records them (below).
        assert!(
            src.contains("spatial_audio.started(")
                || src.contains("&mut state.spatial_audio")
                || src.contains(".start_autoplay(&mut app.world, &mut app.mixer, &mut app.spatial_audio"),
            "{name} never records which sound is whose - a despawned source plays on"
        );
    }
    let web_audio = include_str!("../src/audio/web_audio.rs");
    let at = web_audio.find("pub fn start_autoplay(").expect("start_autoplay");
    assert!(web_audio[at..].contains("spatial.started("), "start_autoplay never records which sound is whose");
    for (name, src) in [
        ("editor", include_str!("../../jengine-editor-viewport/src/lib.rs")),
        ("web", include_str!("../../jengine-web/src/script_host.rs")),
    ] {
        assert!(with_dispatch(src).contains("\"psa\" =>"), "{name} drops `play_sound_at`");
    }
}
