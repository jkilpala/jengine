//! jEngine Physics Package — 2D rigid body physics powered by rapier2d.
//!
//! Registers physics components (`RigidBody2D`, `Collider2D`, `Joint2D`)
//! and provides the `PhysicsWorld` simulation wrapper.

use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::editor_registry::EditorRegistry;
use jengine_core::package::script_api::{ScriptApiRegistry, ScriptArg, ScriptValueType};
use jengine_core::package::system_schedule::{SystemPhase, SystemSchedule};
use jengine_core::package::{FrameContactEvents, JEnginePackage, PackageMetadata};

// Re-export physics types for consumers
pub use jengine_core::ecs::components::{
    BodyType, CharacterController2D, Collider2D, ColliderShape, Joint2D, JointType, RigidBody2D,
};
pub use jengine_core::physics::{ContactEvent, ContactType, PhysicsWorld, RayHit};

/// Physics package — registers physics components and script API.
pub struct PhysicsPackage;

impl JEnginePackage for PhysicsPackage {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "jengine-physics".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "2D rigid body physics powered by rapier2d".to_string(),
            author: "jEngine Team".to_string(),
            engine_version: Some("0.1.0".to_string()),
            dependencies: vec![],
        }
    }

    fn register_components(&self, registry: &mut ComponentRegistry) {
        registry.register::<RigidBody2D>("rigid_body", "Rigid Body 2D", "Physics");
        registry.register::<Collider2D>("collider", "Collider 2D", "Physics");
        registry.register::<Joint2D>("joint", "Joint 2D", "Physics");
        registry.register::<CharacterController2D>("character_controller", "Character Controller 2D", "Physics");
    }

    fn register_systems(&self, schedule: &mut SystemSchedule) {
        schedule.add_system("physics_step", SystemPhase::Physics, 0, |ctx| {
            let (world, dt, resources) = ctx.split();

            // Take physics from resources to get owned access alongside world
            if let Some(mut physics) = resources.remove::<PhysicsWorld>() {
                // Sync ECS bodies to physics world
                jengine_core::ecs::systems::ensure_physics_bodies(&*world, &mut physics);
                physics.sync_kinematic_from_ecs(&*world);
                physics.move_characters(world, dt);

                // Script commands that touch physics are applied by the host
                // (`apply_physics_commands` in the native runner, an inline arm
                // in the viewport); a `FrameScriptCommands` resource used to be
                // read here, and no host ever inserted one (BUGS #144).

                // Step simulation
                let contacts = physics.step(dt);

                // Sync physics results back to ECS
                physics.sync_transforms_to_ecs(world);

                // Store contact events for the game loop to retrieve
                resources.insert(FrameContactEvents(contacts));

                // Put physics back
                resources.insert(physics);
            }
        });
    }

    fn register_script_api(&self, api: &mut ScriptApiRegistry) {
        let pkg = "jengine-physics";

        api.add_function(
            "physics_apply_force",
            pkg,
            "Apply a force to an entity's physics body",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "fx".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "fy".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "physics_apply_impulse",
            pkg,
            "Apply an impulse to an entity's physics body",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "ix".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "iy".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "physics_set_velocity",
            pkg,
            "Set the linear velocity of an entity's physics body",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "vx".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "vy".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "physics_get_velocity",
            pkg,
            "Get the linear velocity of an entity's physics body",
            vec![ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId }],
            ScriptValueType::Vec2,
        );
        api.add_function(
            "physics_set_angular_velocity",
            pkg,
            "Set the angular velocity of an entity's physics body",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "omega".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "physics_get_angular_velocity",
            pkg,
            "Get the angular velocity of an entity's physics body",
            vec![ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId }],
            ScriptValueType::Float,
        );
        api.add_function(
            "physics_set_gravity",
            pkg,
            "Set the physics world gravity",
            vec![
                ScriptArg { name: "gx".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "gy".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "physics_get_gravity",
            pkg,
            "Get the physics world gravity",
            vec![],
            ScriptValueType::Vec2,
        );
        api.add_function(
            "physics_set_body_type",
            pkg,
            "Change the body type of an entity",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "body_type".into(), arg_type: ScriptValueType::String },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "physics_raycast",
            pkg,
            "Cast a ray and return the first hit",
            vec![
                ScriptArg { name: "ox".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "oy".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "dx".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "dy".into(), arg_type: ScriptValueType::Float },
                ScriptArg { name: "max_dist".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil, // Returns table or nil
        );
        api.add_function(
            "physics_add_joint",
            pkg,
            "Add a joint between two entities",
            vec![
                ScriptArg { name: "entity_a".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "entity_b".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "joint_type".into(), arg_type: ScriptValueType::String },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "physics_remove_joint",
            pkg,
            "Remove a joint from an entity",
            vec![ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId }],
            ScriptValueType::Nil,
        );
        api.add_function(
            "physics_set_motor_speed",
            pkg,
            "Set motor speed on a joint",
            vec![
                ScriptArg { name: "entity_id".into(), arg_type: ScriptValueType::EntityId },
                ScriptArg { name: "speed".into(), arg_type: ScriptValueType::Float },
            ],
            ScriptValueType::Nil,
        );
        api.add_function(
            "get_contacts",
            pkg,
            "Get contact events from the current physics step",
            vec![],
            ScriptValueType::Nil, // Returns table
        );
    }

    fn register_editor(&self, _editor: &mut EditorRegistry) {
        // Physics inspector fields are handled by the core editor schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physics_package_metadata() {
        let pkg = PhysicsPackage;
        let meta = pkg.metadata();
        assert_eq!(meta.name, "jengine-physics");
        assert!(!meta.version.is_empty());
    }

    #[test]
    fn physics_package_registers_components() {
        let pkg = PhysicsPackage;
        let mut registry = ComponentRegistry::new();
        pkg.register_components(&mut registry);
        assert!(registry.get("rigid_body").is_some());
        assert!(registry.get("collider").is_some());
        assert!(registry.get("joint").is_some());
    }

    #[test]
    fn physics_package_registers_script_api() {
        let pkg = PhysicsPackage;
        let mut api = ScriptApiRegistry::new();
        pkg.register_script_api(&mut api);
        assert!(api.len() >= 14);
    }
}
