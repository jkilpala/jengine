//! jEngine Audio Package — sound effects and music powered by kira.
//!
//! Registers the `AudioSource` component and provides the `AudioBackend`
//! trait + `KiraAudioBackend` implementation.

use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::editor_registry::EditorRegistry;
use jengine_core::package::script_api::{ScriptApiRegistry, ScriptArg, ScriptValueType};
use jengine_core::package::system_schedule::{SystemPhase, SystemSchedule};
use jengine_core::package::{JEnginePackage, PackageMetadata};

// Re-export audio types for consumers
pub use jengine_core::audio::{AudioBackend, ClipHandle, NullAudioBackend, PlaySettings, SoundHandle};
pub use jengine_core::audio::kira_backend::KiraAudioBackend;
pub use jengine_core::ecs::components::AudioSource;

/// Audio package — registers audio components and script API.
pub struct AudioPackage;

impl JEnginePackage for AudioPackage {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "jengine-audio".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "Sound effects and music powered by kira".to_string(),
            author: "jEngine Team".to_string(),
            engine_version: Some("0.1.0".to_string()),
            dependencies: vec![],
        }
    }

    fn register_components(&self, registry: &mut ComponentRegistry) {
        registry.register::<AudioSource>("audio_source", "Audio Source", "Audio");
    }

    fn register_systems(&self, schedule: &mut SystemSchedule) {
        // Per-frame audio backend tick (kira internal garbage collection).
        // Audio command processing (play/stop/pause) is handled in the game loop
        // because it requires access to the AssetManager and EngineApi.
        schedule.add_system("audio_update", SystemPhase::PostPhysics, 100, |ctx| {
            let (_, _, resources) = ctx.split();
            if let Some(audio) = resources.get_mut::<Box<dyn AudioBackend>>() {
                audio.update();
            }
        });
    }

    fn register_script_api(&self, api: &mut ScriptApiRegistry) {
        let pkg = "jengine-audio";

        api.add_function(
            "audio_play",
            pkg,
            "Play a loaded audio clip",
            vec![
                ScriptArg { name: "clip_id".into(), arg_type: ScriptValueType::String },
                ScriptArg { name: "volume".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "pitch".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "looping".into(), arg_type: ScriptValueType::Bool },
            ],
            ScriptValueType::Int, // SoundHandle
        );
        api.add_function(
            "audio_stop",
            pkg,
            "Stop a playing sound",
            vec![ScriptArg { name: "handle".into(), arg_type: ScriptValueType::Int }],
            ScriptValueType::Nil,
        );
        api.add_function(
            "audio_pause",
            pkg,
            "Pause a playing sound",
            vec![ScriptArg { name: "handle".into(), arg_type: ScriptValueType::Int }],
            ScriptValueType::Nil,
        );
        api.add_function(
            "audio_resume",
            pkg,
            "Resume a paused sound",
            vec![ScriptArg { name: "handle".into(), arg_type: ScriptValueType::Int }],
            ScriptValueType::Nil,
        );
        api.add_function(
            "audio_set_volume",
            pkg,
            "Set volume of a playing sound (0.0–1.0)",
            vec![
                ScriptArg { name: "handle".into(), arg_type: ScriptValueType::Int },
                ScriptArg { name: "volume".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "audio_set_global_volume",
            pkg,
            "Set the global master volume (0.0–1.0)",
            vec![ScriptArg { name: "volume".into(), arg_type: ScriptValueType::Float }],
            ScriptValueType::Nil,
        );
    }

    fn register_editor(&self, _editor: &mut EditorRegistry) {
        // Audio inspector fields are handled by the core editor schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_package_metadata() {
        let pkg = AudioPackage;
        let meta = pkg.metadata();
        assert_eq!(meta.name, "jengine-audio");
        assert!(!meta.version.is_empty());
    }

    #[test]
    fn audio_package_registers_components() {
        let pkg = AudioPackage;
        let mut registry = ComponentRegistry::new();
        pkg.register_components(&mut registry);
        assert!(registry.get("audio_source").is_some());
    }

    #[test]
    fn audio_package_registers_script_api() {
        let pkg = AudioPackage;
        let mut api = ScriptApiRegistry::new();
        pkg.register_script_api(&mut api);
        assert!(api.len() >= 6);
    }
}
