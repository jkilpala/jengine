//! A project's mixer (`settings.audio_mixer`, `audio::MixerLayout`): the
//! buses a game starts with and their effects, authored in Project Settings
//! -> Audio instead of created by a script. Checked here as the mixer applies
//! it, and that every host applies it - natively and on the web where the
//! project's settings are read, and in the editor viewport through
//! `viewport_set_audio_mixer`, which the editor calls when the settings
//! change and as each play session starts.

use jengine_core::audio::{AudioMixer, MixerLayout};
use serde_json::json;

fn layout() -> MixerLayout {
    serde_json::from_value(json!({ "buses": [
        { "name": "master", "volume": 0.9 },
        { "name": "music", "volume": 0.5, "effects": { "lowpass": 1200.0 } },
        { "name": "ambience", "parent": "music", "volume": 0.4, "effects": { "reverb": 0.6, "room": 0.8 } },
        { "name": "voice", "parent": "nowhere", "muted": true, "effects": { "eq_mid": 99.0 } }
    ]}))
    .expect("a layout")
}

#[test]
fn a_layout_makes_and_sets_the_buses() {
    let mut mixer = AudioMixer::new();
    mixer.apply_layout(&layout());
    assert_eq!(mixer.get_volume("master"), Some(0.9));
    assert_eq!(mixer.get_volume("music"), Some(0.5));
    assert_eq!(mixer.effects("music").lowpass, 1200.0);
    let ambience = mixer.get("ambience").expect("made");
    assert_eq!(ambience.parent.as_deref(), Some("music"));
    assert_eq!(mixer.effects("ambience").reverb, 0.6);
    // An unknown parent is master's child; an effect past its range is
    // clamped as a script's would be.
    assert_eq!(mixer.get("voice").and_then(|b| b.parent.as_deref()), Some("master"));
    assert!(mixer.is_muted("voice"));
    assert!(mixer.effects("voice").eq_mid <= 24.0);
    // Applying it again puts back what a script changed.
    mixer.set_volume("music", 0.1);
    mixer.set_effect("ambience", "reverb", 0.0).unwrap();
    mixer.apply_layout(&layout());
    assert_eq!(mixer.get_volume("music"), Some(0.5));
    assert_eq!(mixer.effects("ambience").reverb, 0.6);
}

#[test]
fn a_project_names_its_layout_in_settings() {
    let project = json!({ "settings": { "audio_mixer": { "buses": [{ "name": "ui" }] } } });
    let l = MixerLayout::from_project(&project).expect("read");
    assert_eq!(l.buses[0].name, "ui");
    assert_eq!(l.buses[0].volume, 1.0, "a bus is at full volume unless it says");
    assert!(MixerLayout::from_project(&json!({ "settings": {} })).is_none());
    assert!(MixerLayout::from_settings(&json!({ "audio_mixer": "nonsense" })).is_none());
}

fn source(rel: &str) -> String {
    std::fs::read_to_string(format!("{}/../../{rel}", env!("CARGO_MANIFEST_DIR"))).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn mixer_layout_reaches_every_runtime() {
    let native = source("crates/jengine-runtime/src/wgpu_scene_runner.rs");
    assert!(native.contains("services.mixer.apply_layout("), "native applies the project's mixer");
    let web = source("crates/jengine-web/src/lib.rs");
    assert!(web.contains("audio_mixer: jengine_core::audio::MixerLayout"), "web reads settings.audio_mixer");
    assert!(web.contains("apply_layout(&s.audio_mixer)"), "web applies it");
    let viewport = source("crates/jengine-editor-viewport/src/lib.rs");
    assert!(viewport.contains("pub fn viewport_set_audio_mixer"), "the viewport takes a layout");
    let editor = source("editor/src/lib/Viewport.svelte");
    assert!(
        editor.matches("viewport_set_audio_mixer(").count() >= 2,
        "the editor hands the viewport the layout on a settings change and at each play"
    );
    let settings = source("editor/src-tauri/src/commands/project.rs");
    assert!(
        settings.contains("audio_mixer"),
        "the editor's typed ProjectSettings keeps audio_mixer - a field it lacks is erased on save"
    );
}
