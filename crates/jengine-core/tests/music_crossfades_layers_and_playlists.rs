//! **Music crossfades, plays in stems and runs a playlist, and a sound a
//! script played can still be reached** (Oct 2026). `Audio.crossfade_music`,
//! `play_music_layers` / `set_music_layer`, `set_playlist` / `next_track`,
//! `set_music_pitch`, `seek_music` and `set_sound_pitch` go to one music
//! director (`audio::music`) that every host drives once a frame - where
//! music used to be one track swapped hard, in three hand-kept copies.
//!
//! And `play_sound` hands back an id that reaches its sound: until this the
//! id was the script's own counter, dropped on the way to the backend
//! natively and returned as 0 in the browsers, so `Audio.stop_sound(id)` and
//! `set_sound_volume(id)` had never stopped or turned down anything they
//! named. Found by the showcase's Night Drive, whose engine note is a sound
//! whose pitch follows the speed.
//!
//! `audio::music`'s own tests cover the director; this plays the script
//! surface natively through `apply_audio_commands` and a stand-in backend,
//! and checks every host drives the director and aliases sound ids.

#[cfg(feature = "native")]
use std::collections::HashMap;

fn read(p: &str) -> String {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(p)).unwrap().replace("\r\n", "\n")
}

/// Stands in for kira: what plays, at what volume and pitch, and where.
#[cfg(feature = "native")]
#[derive(Default)]
struct Deck {
    clips: HashMap<String, u64>,
    names: HashMap<u64, String>,
    /// sound -> (clip, volume, pitch, position)
    live: HashMap<u64, (String, f32, f32, f64)>,
    next: u64,
}

#[cfg(feature = "native")]
mod deck {
    use super::Deck;
    use jengine_core::audio::{AudioBackend, ClipHandle, PlaySettings, SoundHandle};
    impl AudioBackend for Deck {
        fn load_clip(&mut self, id: &str, _data: &[u8]) -> anyhow::Result<ClipHandle> {
            let h = self.clips.len() as u64 + 1;
            self.clips.insert(id.to_string(), h);
            self.names.insert(h, id.to_string());
            Ok(ClipHandle(h))
        }
        fn get_clip_by_name(&self, name: &str) -> Option<ClipHandle> {
            self.clips.get(name).copied().map(ClipHandle)
        }
        fn play(&mut self, clip: ClipHandle, s: PlaySettings) -> anyhow::Result<SoundHandle> {
            // Ids well clear of a script's, so an unaliased id would miss.
            self.next += 1;
            let id = 500 + self.next;
            self.live.insert(id, (self.names[&clip.0].clone(), s.volume, s.pitch, 0.0));
            Ok(SoundHandle(id))
        }
        fn stop(&mut self, h: SoundHandle) {
            self.live.remove(&h.0);
        }
        fn pause(&mut self, _h: SoundHandle) {}
        fn resume(&mut self, _h: SoundHandle) {}
        fn set_volume(&mut self, h: SoundHandle, v: f32) {
            if let Some(s) = self.live.get_mut(&h.0) {
                s.1 = v;
            }
        }
        fn set_pitch(&mut self, h: SoundHandle, p: f32) {
            if let Some(s) = self.live.get_mut(&h.0) {
                s.2 = p;
            }
        }
        fn seek(&mut self, h: SoundHandle, to: f64) {
            if let Some(s) = self.live.get_mut(&h.0) {
                s.3 = to;
            }
        }
        fn position(&self, h: SoundHandle) -> Option<f64> {
            self.live.get(&h.0).map(|s| s.3)
        }
        fn clip_duration(&self, _clip: ClipHandle) -> Option<f64> {
            Some(60.0)
        }
        fn set_global_volume(&mut self, _v: f32) {}
        fn is_playing(&self, h: SoundHandle) -> bool {
            self.live.contains_key(&h.0)
        }
        fn update(&mut self) {}
    }
}

#[cfg(feature = "native")]

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
fn a_script_drives_stems_a_crossfade_and_a_sound_by_its_id() {
    use hecs::World;
    use jengine_core::asset::AssetManager;
    use jengine_core::audio::music;
    use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
    use jengine_core::ecs::systems::{ScriptSystem, apply_audio_commands};
    use jengine_core::input::InputState;
    const SRC: &str = r#"
local frame = 0
local engine
function update(entity, dt)
    frame = frame + 1
    if frame == 1 then
        Audio.play_music_layers({ "m/drums.ogg", "m/bass.ogg", "m/lead.ogg" }, { 1, 0, 0 })
        engine = Audio.play_sound("sfx/engine.ogg", 0.5)
        Entity.set_custom(entity, "Probe", "engine", engine)
    elseif frame == 2 then
        Audio.set_music_layer(2, 0.8, 0)
        Audio.set_sound_pitch(engine, 1.75)
        Audio.set_sound_volume(engine, 0.25)
        Audio.seek_music(12)
        Entity.set_custom(entity, "Probe", "clip", Audio.get_music_clip())
    elseif frame == 3 then
        Entity.set_custom(entity, "Probe", "bass", Audio.get_music_layer(2))
        Audio.crossfade_music("m/calm.ogg", 0)
        Audio.stop_sound(engine)
    end
end
"#;
    let dir = std::env::temp_dir().join(format!("jengine_music_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("drive.lua");
    std::fs::write(&path, SRC).unwrap();
    let mut world = World::new();
    let e = world.spawn((
        Name("Probe".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut assets = AssetManager::new();
    for c in ["m/drums.ogg", "m/bass.ogg", "m/lead.ogg", "m/calm.ogg", "sfx/engine.ogg"] {
        assets.insert_audio(c.to_string(), vec![0]);
    }
    let mut deck = Deck::default();
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    // A frame as the runner runs it: scripts, their audio, then the music.
    let mut frame = |world: &mut World, deck: &mut Deck, t: f64| {
        system.run(world, 1.0 / 60.0, &input, None, None);
        let cmds = std::mem::take(&mut system.api.commands);
        apply_audio_commands(&cmds, deck, &assets, &mut system.api);
        music::drive_at(t, &mut music::NativeOut { audio: deck, assets: &assets, mixer: &mut system.api.services.mixer });
    };
    let by_clip = |deck: &Deck, clip: &str| deck.live.values().find(|s| s.0 == clip).cloned();

    frame(&mut world, &mut deck, 0.0);
    for stem in ["m/drums.ogg", "m/bass.ogg", "m/lead.ogg"] {
        assert!(by_clip(&deck, stem).is_some(), "{stem} did not start with the others");
    }
    assert_eq!(by_clip(&deck, "m/bass.ogg").unwrap().1, 0.0, "a stem starts at its own volume");
    let engine = by_clip(&deck, "sfx/engine.ogg").expect("the engine note plays");
    assert_eq!(engine.1, 0.5);

    frame(&mut world, &mut deck, 0.1);
    assert!((by_clip(&deck, "m/bass.ogg").unwrap().1 - 0.8).abs() < 1e-4, "set_music_layer reached the bass");
    let engine = by_clip(&deck, "sfx/engine.ogg").unwrap();
    assert_eq!(engine.2, 1.75, "set_sound_pitch reached the sound its id names");
    assert_eq!(engine.1, 0.25, "and so did set_sound_volume");
    assert!(deck.live.values().filter(|s| s.0.starts_with("m/")).all(|s| s.3 == 12.0), "every stem was seeked");

    frame(&mut world, &mut deck, 0.2);
    assert!(by_clip(&deck, "sfx/engine.ogg").is_none(), "stop_sound stopped the sound its id names");
    assert_eq!(deck.live.values().map(|s| s.0.as_str()).collect::<Vec<_>>(), vec!["m/calm.ogg"], "a cut, with no fade");

    let get = |k: &str| world.get::<&CustomComponents>(e).unwrap().0.get("Probe").and_then(|p| p.get(k)).cloned().unwrap();
    assert_eq!(get("clip"), serde_json::json!("m/drums.ogg"), "the first stem names the music");
    assert!((get("bass").as_f64().unwrap() - 0.8).abs() < 1e-6);
}

#[test]
fn every_host_drives_the_music_and_aliases_sound_ids() {
    let runner = read("crates/jengine-runtime/src/wgpu_scene_runner.rs");
    assert!(runner.contains("audio::music::drive(&mut jengine_core::audio::music::NativeOut"), "native never drives the music");
    for (host, src) in [
        ("web", with_dispatch(&read("crates/jengine-web/src/script_host.rs"))),
        ("editor", with_dispatch(&read("crates/jengine-editor-viewport/src/lib.rs"))),
    ] {
        assert!(src.contains("audio::music::drive(&mut jengine_core::audio::web_audio::WebOut"), "{host} never drives the music");
        assert_eq!(src.matches("audio::music::alias(sid, id)").count(), 2, "{host} does not alias play_sound and play_sound_at");
        assert!(src.contains("\"sts\" => {") && src.contains("\"ssv\" => {"), "{host} drops stop_sound / set_sound_volume");
        assert!(src.matches("audio::music::resolve(").count() >= 2, "{host} does not resolve a script's sound id");
        assert!(!src.contains("play_music_on_bus") && !src.contains("web_audio.stop_music"), "{host} keeps a second music path");
    }
    assert!(read("crates/jengine-editor-viewport/src/lib.rs").matches("audio::music::reset()").count() >= 2, "Play and Stop do not silence the music");
    let systems = read("crates/jengine-core/src/ecs/systems.rs");
    assert!(systems.contains("crate::audio::music::alias(*sound_id, h)"), "native does not alias play_sound");
    // The script surface.
    let preamble = read("crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs");
    let luau = read("crates/jengine-core/src/scripting/bridge/adapters/luau.rs");
    for (flat, op) in [
        ("audio_crossfade_music", "crossfade"),
        ("audio_play_music_layers", "layers"),
        ("audio_set_music_layer", "layer"),
        ("audio_set_playlist", "playlist"),
        ("audio_next_track", "next"),
        ("audio_set_music_pitch", "pitch"),
        ("audio_seek_music", "seek"),
        ("audio_set_sound_pitch", "sound_pitch"),
        ("audio_get_music_clip", "clip"),
        ("audio_get_music_layer", "layer_volume"),
    ] {
        let at = preamble.find(&format!("function {flat}(")).unwrap_or_else(|| panic!("no preamble {flat}"));
        let line = &preamble[at..at + preamble[at..].find('\n').unwrap()];
        assert!(line.contains(&format!("__music('{op}'")), "{flat} does not ask the hook for '{op}'");
        assert!(luau.contains(&format!("globals.set(\"{flat}\", music(\"{op}\")?)")), "{flat} is not bound natively to '{op}'");
    }
    assert!(preamble.contains("function play_sound(") && preamble.contains("sid: sid"), "the browsers' play_sound hands back no id");
}
