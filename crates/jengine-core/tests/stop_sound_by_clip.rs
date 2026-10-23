//! BUGS #20 — one-shot sounds are stoppable.
//!
//! `stop_all_sounds` mapped to `stop_music()` in the editor and on web, and
//! `stop_sound(handle)` was unusable everywhere: `play_sound` is a *deferred*
//! command, so it has not run by the time the script returns and there is no
//! handle to give back. Stopping by clip id is the addressable form.

use jengine_core::audio::{AudioBackend, NullAudioBackend, PlaySettings};

/// A backend that records what it was told, standing in for kira in a headless
/// test — the real one needs an output device.
#[derive(Default)]
struct RecordingBackend {
    clips: std::collections::HashMap<String, u64>,
    live: Vec<(u64, u64)>, // (sound_id, clip_handle)
    next: u64,
}

impl AudioBackend for RecordingBackend {
    fn load_clip(&mut self, id: &str, _data: &[u8]) -> anyhow::Result<jengine_core::audio::ClipHandle> {
        let h = self.clips.len() as u64 + 1;
        self.clips.insert(id.to_string(), h);
        Ok(jengine_core::audio::ClipHandle(h))
    }
    fn get_clip_by_name(&self, name: &str) -> Option<jengine_core::audio::ClipHandle> {
        self.clips.get(name).copied().map(jengine_core::audio::ClipHandle)
    }
    fn play(
        &mut self,
        clip: jengine_core::audio::ClipHandle,
        _settings: PlaySettings,
    ) -> anyhow::Result<jengine_core::audio::SoundHandle> {
        self.next += 1;
        self.live.push((self.next, clip.0));
        Ok(jengine_core::audio::SoundHandle(self.next))
    }
    fn stop(&mut self, handle: jengine_core::audio::SoundHandle) {
        self.live.retain(|(id, _)| *id != handle.0);
    }
    fn pause(&mut self, _handle: jengine_core::audio::SoundHandle) {}
    fn resume(&mut self, _handle: jengine_core::audio::SoundHandle) {}
    fn set_volume(&mut self, _handle: jengine_core::audio::SoundHandle, _volume: f32) {}
    fn stop_all(&mut self) {
        self.live.clear();
    }
    fn stop_by_clip(&mut self, clip: jengine_core::audio::ClipHandle) -> usize {
        let before = self.live.len();
        self.live.retain(|(_, c)| *c != clip.0);
        before - self.live.len()
    }
    fn set_global_volume(&mut self, _volume: f32) {}
    fn is_playing(&self, handle: jengine_core::audio::SoundHandle) -> bool {
        self.live.iter().any(|(id, _)| *id == handle.0)
    }
    fn update(&mut self) {}
}

fn play(b: &mut RecordingBackend, clip: &str) {
    let h = b.get_clip_by_name(clip).expect("clip loaded");
    b.play(h, PlaySettings::default()).expect("play");
}

#[test]
fn stops_every_instance_of_one_clip_and_leaves_the_others() {
    let mut b = RecordingBackend::default();
    b.load_clip("sounds/hum.wav", &[]).unwrap();
    b.load_clip("sounds/hit.wav", &[]).unwrap();

    play(&mut b, "sounds/hum.wav");
    play(&mut b, "sounds/hum.wav");
    play(&mut b, "sounds/hit.wav");
    assert_eq!(b.live.len(), 3);

    // Both hums stop; the hit keeps playing. Stopping by clip has to reach
    // *every* instance — a looping ambience started twice would otherwise
    // leave one running with nothing able to name it.
    assert_eq!(b.stop_by_clip(b.get_clip_by_name("sounds/hum.wav").unwrap()), 2);
    assert_eq!(b.live.len(), 1);
    assert_eq!(b.live[0].1, b.get_clip_by_name("sounds/hit.wav").unwrap().0);
}

#[test]
fn stopping_an_unplayed_clip_reports_zero_rather_than_failing() {
    let mut b = RecordingBackend::default();
    b.load_clip("sounds/hum.wav", &[]).unwrap();
    assert_eq!(b.stop_by_clip(b.get_clip_by_name("sounds/hum.wav").unwrap()), 0);
}

#[test]
fn backends_without_source_retention_default_to_a_no_op() {
    // The trait default keeps a backend that cannot retain sources compiling
    // and honest: it reports zero stopped rather than pretending.
    let mut null = NullAudioBackend;
    assert_eq!(null.stop_by_clip(jengine_core::audio::ClipHandle(1)), 0);
}

#[test]
fn stop_all_clears_every_live_sound() {
    let mut b = RecordingBackend::default();
    b.load_clip("sounds/hum.wav", &[]).unwrap();
    play(&mut b, "sounds/hum.wav");
    play(&mut b, "sounds/hum.wav");
    b.stop_all();
    assert!(b.live.is_empty());
}
