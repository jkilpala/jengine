//! jEngine Particles Package — 2D particle emitters and simulation.
//!
//! Registers the `ParticleEmitter` component and provides the
//! `ParticleSystem` simulation engine.

use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::editor_registry::EditorRegistry;
use jengine_core::package::script_api::{ScriptApiRegistry, ScriptArg, ScriptValueType};
use jengine_core::package::system_schedule::SystemSchedule;
use jengine_core::package::{JEnginePackage, PackageMetadata};

// Re-export particle types for consumers
pub use jengine_core::particles::{ParticleDrawData, ParticleEmitter, ParticleSystem};

/// Particles package — registers particle components and script API.
pub struct ParticlesPackage;

impl JEnginePackage for ParticlesPackage {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "jengine-particles".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "2D particle emitters and simulation".to_string(),
            author: "jEngine Team".to_string(),
            engine_version: Some("0.1.0".to_string()),
            dependencies: vec![],
        }
    }

    fn register_components(&self, registry: &mut ComponentRegistry) {
        registry.register::<ParticleEmitter>("particle_emitter", "Particle Emitter", "Particles");
    }

    fn register_systems(&self, _schedule: &mut SystemSchedule) {
        // Particle simulation is tightly integrated with the scripting system
        // (ParticleSystem lives in EngineApi). Rendering is done in the Render phase
        // by the core game loop. Future: decouple ParticleSystem -> ResourceMap.
    }

    fn register_script_api(&self, api: &mut ScriptApiRegistry) {
        let pkg = "jengine-particles";

        api.add_function(
            "particle_set_active",
            pkg,
            "Enable or disable a particle emitter",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "active".into(), arg_type: ScriptValueType::Bool },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "particle_burst",
            pkg,
            "Emit a burst of particles from an emitter",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "count".into(), arg_type: ScriptValueType::Int },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "particle_set_emission_rate",
            pkg,
            "Set the emission rate (particles per second)",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "rate".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "particle_set_speed",
            pkg,
            "Set particle speed range",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "min".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "max".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "particle_set_lifetime",
            pkg,
            "Set particle lifetime range",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "min".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "max".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "particle_set_color",
            pkg,
            "Set particle start/end colors",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "sr".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "sg".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "sb".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "sa".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "er".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "eg".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "eb".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "ea".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "particle_set_size",
            pkg,
            "Set particle start/end sizes",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "start".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "end".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "particle_set_gravity_modifier",
            pkg,
            "Set the gravity modifier for particles",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "modifier".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
    }

    fn register_editor(&self, _editor: &mut EditorRegistry) {
        // Particle inspector fields are handled by the core editor schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particles_package_metadata() {
        let pkg = ParticlesPackage;
        let meta = pkg.metadata();
        assert_eq!(meta.name, "jengine-particles");
        assert!(!meta.version.is_empty());
    }

    #[test]
    fn particles_package_registers_components() {
        let pkg = ParticlesPackage;
        let mut registry = ComponentRegistry::new();
        pkg.register_components(&mut registry);
        assert!(registry.get("particle_emitter").is_some());
    }

    #[test]
    fn particles_package_registers_script_api() {
        let pkg = ParticlesPackage;
        let mut api = ScriptApiRegistry::new();
        pkg.register_script_api(&mut api);
        assert!(api.len() >= 8);
    }
}
