//! What a sound actually plays at, once buses are involved.
//!
//! The model: output = the sound's own volume x its bus's effective volume.
//! Before it was written down, native got it wrong three ways -
//!
//! * a bus change re-applied the *bare* bus volume to every playing sound, so
//!   moving the master slider snapped a 0.3 music track to full;
//! * `set_music_volume` / `set_sound_volume` ignored the bus, so they un-muted
//!   a muted one;
//! * bus changes were only pushed out on frames that also issued an audio
//!   command, and finished sounds were never forgotten, so the mixer's track
//!   list grew by one per `play_sound` for the whole session.
//!
//! And autoplay `AudioSource`s ignored `bus_id`, and a looping one was played
//! as music - which stopped the music that was already playing.

use std::collections::HashMap;

use jengine_core::asset::AssetManager;
use jengine_core::audio::{
    AudioBackend, ClipHandle, MASTER_BUS, MUSIC_BUS, PlaySettings, SFX_BUS, SoundHandle, sync_mixer,
};
use jengine_core::ecs::behaviors::AudioPlayRequest;
use jengine_core::ecs::systems::{apply_audio_commands, play_autoplay_audio};
use jengine_core::scripting::api::{ScriptCommand, ScriptState};

/// Stands in for kira, which needs an output device. Remembers the volume each
/// sound was last given, which is the number these tests are about.
#[derive(Default)]
struct Recorder {
    clips: HashMap<String, u64>,
    /// sound id -> (volume, looping)
    live: HashMap<u64, (f32, bool)>,
    next: u64,
}

impl Recorder {
    fn volume(&self, h: u64) -> f32 {
        self.live[&h].0
    }
    /// A one-shot reaching its end.
    fn finish(&mut self, h: u64) {
        self.live.remove(&h);
    }
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
        self.live.insert(self.next, (s.volume, s.looping));
        Ok(SoundHandle(self.next))
    }
    fn stop(&mut self, h: SoundHandle) {
        self.live.remove(&h.0);
    }
    fn pause(&mut self, _h: SoundHandle) {}
    fn resume(&mut self, _h: SoundHandle) {}
    fn set_volume(&mut self, h: SoundHandle, volume: f32) {
        if let Some(entry) = self.live.get_mut(&h.0) {
            entry.0 = volume;
        }
    }
    fn set_global_volume(&mut self, _volume: f32) {}
    fn is_playing(&self, h: SoundHandle) -> bool {
        self.live.contains_key(&h.0)
    }
    fn stop_all(&mut self) {
        self.live.clear();
    }
    fn update(&mut self) {}
}

struct Rig {
    audio: Recorder,
    assets: AssetManager,
    api: ScriptState,
}

impl Rig {
    fn new() -> Self {
        let mut assets = AssetManager::new();
        for clip in ["sfx/hit.wav", "music/a.ogg", "music/b.ogg", "amb/torch.ogg"] {
            assets.insert_audio(clip.to_string(), vec![0]);
        }
        Self { audio: Recorder::default(), assets, api: ScriptState::new() }
    }

    fn run(&mut self, cmds: Vec<ScriptCommand>) {
        apply_audio_commands(&cmds, &mut self.audio, &self.assets, &mut self.api);
        self.end_frame();
    }

    /// What the runner does after `audio.update()`.
    fn end_frame(&mut self) {
        sync_mixer(&mut self.audio, &mut self.api.services.mixer);
        // The music director, as the runner drives it.
        jengine_core::audio::music::drive(&mut jengine_core::audio::music::NativeOut {
            audio: &mut self.audio,
            assets: &self.assets,
            mixer: &mut self.api.services.mixer,
        });
    }

    fn music(&self) -> u64 {
        jengine_core::audio::music::with(|d| d.playing_ids()).first().copied().expect("music is playing")
    }
}

fn play_music(clip: &str, volume: f32) -> ScriptCommand {
    ScriptCommand::PlayMusic { clip_id: clip.into(), volume, looping: true }
}

fn play_sound(volume: f32) -> ScriptCommand {
    ScriptCommand::PlaySound { clip_id: "sfx/hit.wav".into(), volume, pitch: 1.0, pan: 0.0, sound_id: 0, looping: false }
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn a_bus_change_scales_a_sound_instead_of_replacing_its_volume() {
    let mut rig = Rig::new();
    rig.run(vec![play_music("music/a.ogg", 0.3)]);
    let m = rig.music();
    assert!(approx(rig.audio.volume(m), 0.3));

    rig.api.services.mixer.set_volume(MASTER_BUS, 0.5);
    rig.end_frame();
    assert!(
        approx(rig.audio.volume(m), 0.15),
        "master 0.5 x music 0.3 should be 0.15, got {}",
        rig.audio.volume(m)
    );

    // And back up: the 0.3 was not lost along the way.
    rig.api.services.mixer.set_volume(MASTER_BUS, 1.0);
    rig.end_frame();
    assert!(approx(rig.audio.volume(m), 0.3), "got {}", rig.audio.volume(m));
}

#[test]
fn a_bus_change_applies_on_a_frame_with_no_audio_commands() {
    let mut rig = Rig::new();
    rig.run(vec![play_sound(1.0)]);
    let h = rig.api.services.last_sound_handle;

    // A settings menu moving the sfx slider - no play/stop this frame.
    rig.api.services.mixer.set_volume(SFX_BUS, 0.2);
    rig.run(vec![]);
    assert!(approx(rig.audio.volume(h), 0.2), "got {}", rig.audio.volume(h));
}

#[test]
fn set_music_volume_does_not_unmute_a_muted_bus() {
    let mut rig = Rig::new();
    rig.run(vec![play_music("music/a.ogg", 1.0)]);
    rig.api.services.mixer.set_muted(MUSIC_BUS, true);
    rig.end_frame();
    assert_eq!(rig.audio.volume(rig.music()), 0.0);

    rig.run(vec![ScriptCommand::SetMusicVolume { volume: 0.6 }]);
    assert_eq!(rig.audio.volume(rig.music()), 0.0, "set_music_volume bypassed the mute");

    rig.api.services.mixer.set_muted(MUSIC_BUS, false);
    rig.end_frame();
    assert!(approx(rig.audio.volume(rig.music()), 0.6), "the 0.6 was not kept");
}

#[test]
fn set_sound_volume_goes_through_the_bus() {
    let mut rig = Rig::new();
    rig.api.services.mixer.set_volume(SFX_BUS, 0.5);
    rig.run(vec![play_sound(1.0)]);
    let h = rig.api.services.last_sound_handle;
    assert!(approx(rig.audio.volume(h), 0.5));

    rig.run(vec![ScriptCommand::SetSoundVolume { handle_id: h, volume: 0.4 }]);
    assert!(approx(rig.audio.volume(h), 0.2), "got {}", rig.audio.volume(h));
}

#[test]
fn finished_sounds_are_forgotten() {
    let mut rig = Rig::new();
    for _ in 0..50 {
        rig.run(vec![play_sound(1.0)]);
    }
    assert_eq!(rig.api.services.mixer.tracks().count(), 50);

    let ids: Vec<u64> = rig.audio.live.keys().copied().collect();
    for id in ids {
        rig.audio.finish(id);
    }
    rig.end_frame();
    assert_eq!(rig.api.services.mixer.tracks().count(), 0, "finished sounds stayed tracked");
}

#[test]
fn replacing_or_stopping_music_forgets_the_old_track() {
    let mut rig = Rig::new();
    rig.run(vec![play_music("music/a.ogg", 1.0)]);
    rig.run(vec![play_music("music/b.ogg", 1.0)]);
    assert_eq!(rig.api.services.mixer.tracks().count(), 1);
    rig.run(vec![ScriptCommand::StopMusic]);
    assert_eq!(rig.api.services.mixer.tracks().count(), 0);

    rig.run(vec![play_music("music/a.ogg", 1.0), play_sound(1.0)]);
    rig.run(vec![ScriptCommand::StopAllSounds]);
    assert_eq!(rig.api.services.mixer.tracks().count(), 0);
    assert!(jengine_core::audio::music::with(|d| d.playing_ids()).is_empty());
}

#[test]
fn autoplay_honours_bus_id() {
    let mut rig = Rig::new();
    assert!(rig.api.services.mixer.create_bus("ambience", Some(MASTER_BUS), 0.25));
    let req = AudioPlayRequest {
        clip_id: "amb/torch.ogg".into(),
        volume: 0.8,
        pitch: 1.0,
        looping: false,
        bus_id: Some("ambience".into()),
        pan: 0.0,
        entity: hecs::Entity::DANGLING,
        filter: false,
    };
    play_autoplay_audio(&[req], &mut rig.audio, &rig.assets, &mut rig.api.services.mixer, &mut Default::default());
    let h = *rig.audio.live.keys().next().unwrap();
    assert!(approx(rig.audio.volume(h), 0.2), "0.8 on a 0.25 bus, got {}", rig.audio.volume(h));
    assert_eq!(rig.api.services.mixer.track_bus(h), Some("ambience"));

    // A bus that does not exist falls back to sfx instead of silence.
    let req = AudioPlayRequest {
        clip_id: "sfx/hit.wav".into(),
        volume: 1.0,
        pitch: 1.0,
        looping: false,
        bus_id: Some("typo".into()),
        pan: 0.0,
        entity: hecs::Entity::DANGLING,
        filter: false,
    };
    play_autoplay_audio(&[req], &mut rig.audio, &rig.assets, &mut rig.api.services.mixer, &mut Default::default());
    let newest = rig.audio.next;
    assert_eq!(rig.api.services.mixer.track_bus(newest), Some(SFX_BUS));
    assert!(approx(rig.audio.volume(newest), 1.0));
}

#[test]
fn a_looping_audio_source_does_not_replace_the_music() {
    let mut rig = Rig::new();
    rig.run(vec![play_music("music/a.ogg", 1.0)]);
    let music = rig.music();

    let torch = AudioPlayRequest {
        clip_id: "amb/torch.ogg".into(),
        volume: 1.0,
        pitch: 1.0,
        looping: true,
        bus_id: None,
        pan: 0.0,
        entity: hecs::Entity::DANGLING,
        filter: false,
    };
    play_autoplay_audio(&[torch], &mut rig.audio, &rig.assets, &mut rig.api.services.mixer, &mut Default::default());
    rig.end_frame();

    assert!(rig.audio.is_playing(SoundHandle(music)), "the torch loop stopped the music");
    assert_eq!(rig.music(), music);
    let torch_handle = rig.audio.next;
    assert!(rig.audio.live[&torch_handle].1, "the source should loop on its own");
}
