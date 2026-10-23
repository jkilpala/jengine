//! Type-erased component registry for the package system.
//!
//! Allows packages to register ECS component types at startup without
//! hardcoding them in `ComponentBundle`. Each component is stored as a
//! `ComponentDescriptor` with function pointers for serialization,
//! deserialization, and ECS spawning.

use std::any::{Any, TypeId};
use std::collections::HashMap;

use serde::{de::DeserializeOwned, Serialize};

// ──────────────────────────── Component Trait Bound ────────────────────────────

/// Trait alias for the bounds required on registerable components.
///
/// Components must be serializable, cloneable, and ECS‑compatible.
pub trait Component: Serialize + DeserializeOwned + Clone + Send + Sync + Any + 'static {}
impl<T: Serialize + DeserializeOwned + Clone + Send + Sync + Any + 'static> Component for T {}

// ──────────────────────────── ComponentDescriptor ────────────────────────────

/// Type-erased descriptor for a registered component.
///
/// Stores function pointers that operate on `Box<dyn Any>` so the registry
/// can serialize, deserialize, and spawn components without knowing their
/// concrete Rust type.
#[derive(Clone)]
pub struct ComponentDescriptor {
    /// Unique name used as the JSON key in scene files (e.g. "transform", "rigid_body").
    pub name: String,

    /// Human-readable display name for the editor (e.g. "Transform", "Rigid Body 2D").
    pub display_name: String,

    /// Rust `TypeId` of the concrete component type.
    pub type_id: TypeId,

    /// Category for the editor "Add Component" menu (e.g. "Core", "Physics", "Audio").
    pub category: String,

    /// Serialize a `&dyn Any` (which is `&T`) to a JSON value.
    pub serialize_fn: fn(&dyn Any) -> serde_json::Value,

    /// Deserialize a JSON value to a `Box<dyn Any>` (which is `Box<T>`).
    /// Returns `None` if deserialization fails.
    pub deserialize_fn: fn(&serde_json::Value) -> Option<Box<dyn Any>>,

    /// Add this component (from `Box<dyn Any>`) to a hecs `EntityBuilder`.
    pub spawn_fn: fn(&mut hecs::EntityBuilder, Box<dyn Any>),

    /// Query a single entity for this component and return it as `Box<dyn Any>`,
    /// or `None` if the entity doesn't have the component.
    pub query_fn: fn(&hecs::World, hecs::Entity) -> Option<Box<dyn Any>>,

    /// Whether a JSON value deserializes as this component, and serde's
    /// message when it does not. The editor schema reads enum options out of
    /// that message (`package::editor_registry::derived_component_info`).
    pub check_fn: fn(&serde_json::Value) -> Result<(), String>,

    /// The component's `Default`, when it has one and was registered with
    /// [`ComponentRegistry::register_defaulted`] - what the editor adds, and
    /// what its inspector fields are derived from. Without it the editor
    /// tries `{}`, which a component with a required field refuses.
    pub default_fn: Option<fn() -> Box<dyn Any>>,

    /// Every field the component deserializes, by its scene key - the ones
    /// its default leaves out when empty too (`package::field_names`).
    /// `None` for a component that is not a plain struct.
    pub fields_fn: fn() -> Option<&'static [&'static str]>,

    /// Insert (or replace) this component on an existing entity. Used by
    /// the bridge `SetComponentField` path so script writes against
    /// package-registered components land on the live world.
    pub insert_fn: fn(&mut hecs::World, hecs::Entity, Box<dyn Any>),
}

// ──────────────────────────── ComponentRegistry ────────────────────────────

/// Registry of all known component types.
///
/// Populated at startup by `register_core_components()` and by each package's
/// `register_components()` method.
#[derive(Clone)]
pub struct ComponentRegistry {
    /// name → descriptor
    descriptors: HashMap<String, ComponentDescriptor>,
    /// TypeId → name (reverse lookup for serialization)
    type_to_name: HashMap<TypeId, String>,
}

impl ComponentRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            descriptors: HashMap::new(),
            type_to_name: HashMap::new(),
        }
    }

    /// Register a component type.
    ///
    /// - `name`: JSON key used in scene files (e.g. "transform")
    /// - `display_name`: human-readable name for the editor (e.g. "Transform")
    /// - `category`: editor category (e.g. "Core", "Physics")
    pub fn register<T: Component>(&mut self, name: &str, display_name: &str, category: &str) {
        let type_id = TypeId::of::<T>();

        let descriptor = ComponentDescriptor {
            name: name.to_string(),
            display_name: display_name.to_string(),
            type_id,
            category: category.to_string(),

            serialize_fn: |any| {
                let component = any.downcast_ref::<T>().expect("type mismatch in serialize_fn");
                serde_json::to_value(component).unwrap_or(serde_json::Value::Null)
            },

            deserialize_fn: |value| {
                serde_json::from_value::<T>(value.clone())
                    .ok()
                    .map(|c| Box::new(c) as Box<dyn Any>)
            },

            check_fn: |value| serde_json::from_value::<T>(value.clone()).map(|_| ()).map_err(|e| e.to_string()),

            default_fn: None,

            fields_fn: crate::package::field_names::field_names::<T>,

            spawn_fn: |builder, any| {
                if let Ok(component) = any.downcast::<T>() {
                    builder.add(*component);
                }
            },

            query_fn: |world, entity| {
                world
                    .get::<&T>(entity)
                    .ok()
                    .map(|c| Box::new((*c).clone()) as Box<dyn Any>)
            },

            insert_fn: |world, entity, any| {
                if let Ok(component) = any.downcast::<T>() {
                    let _ = world.insert_one(entity, *component);
                }
            },
        };

        log::trace!("Component registered: {} ({})", name, display_name);
        self.type_to_name.insert(type_id, name.to_string());
        self.descriptors.insert(name.to_string(), descriptor);
    }

    /// [`Self::register`], and record the component's `Default` for the
    /// editor (see [`ComponentDescriptor::default_fn`]).
    pub fn register_defaulted<T: Component + Default>(&mut self, name: &str, display_name: &str, category: &str) {
        self.register::<T>(name, display_name, category);
        if let Some(d) = self.descriptors.get_mut(name) {
            d.default_fn = Some(|| Box::new(T::default()) as Box<dyn Any>);
        }
    }

    /// Look up a descriptor by JSON key name.
    pub fn get(&self, name: &str) -> Option<&ComponentDescriptor> {
        self.descriptors.get(name)
    }

    /// Look up the JSON key name for a Rust type.
    pub fn name_of<T: 'static>(&self) -> Option<&str> {
        self.type_to_name.get(&TypeId::of::<T>()).map(|s| s.as_str())
    }

    /// Look up the JSON key name by TypeId.
    pub fn name_of_type_id(&self, type_id: &TypeId) -> Option<&str> {
        self.type_to_name.get(type_id).map(|s| s.as_str())
    }

    /// Iterate over all registered descriptors.
    pub fn iter(&self) -> impl Iterator<Item = &ComponentDescriptor> {
        self.descriptors.values()
    }

    /// Get all descriptors grouped by category.
    pub fn by_category(&self) -> HashMap<&str, Vec<&ComponentDescriptor>> {
        let mut map: HashMap<&str, Vec<&ComponentDescriptor>> = HashMap::new();
        for desc in self.descriptors.values() {
            map.entry(desc.category.as_str()).or_default().push(desc);
        }
        map
    }

    /// Number of registered component types.
    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }

    // ──────────────────── Scene Serialization Helpers ────────────────────

    /// Serialize all registered components on an entity to a JSON map.
    ///
    /// Returns only the components that exist on the entity. Unknown components
    /// (not in the registry) are not included.
    pub fn serialize_entity(
        &self,
        world: &hecs::World,
        entity: hecs::Entity,
    ) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();

        // Ask the entity what it has, rather than asking the world 67 times
        // "does this entity have you?".
        //
        // The old loop probed *every registered descriptor* per entity, so
        // cost followed the size of the registry rather than the size of the
        // entity. Snapshotting 16,000 two-component entities took 66 ms -
        // 4.1 us each - and `push_scene` / `pop_scene` pay that as a visible
        // hitch mid-game.
        //
        // Falls back to the exhaustive probe if the entity is not in the
        // world, which `component_types` cannot answer for.
        let Ok(entity_ref) = world.entity(entity) else {
            return map;
        };
        for type_id in entity_ref.component_types() {
            let Some(name) = self.type_to_name.get(&type_id) else {
                continue;
            };
            let Some(desc) = self.descriptors.get(name) else {
                continue;
            };
            if let Some(any_component) = (desc.query_fn)(world, entity) {
                let value = (desc.serialize_fn)(any_component.as_ref());
                if !value.is_null() {
                    map.insert(name.clone(), value);
                }
            }
        }

        map
    }

    /// Deserialize components from a JSON map and add them to an `EntityBuilder`.
    ///
    /// Unknown keys (not in the registry) are collected into the returned map
    /// so the caller can handle them (e.g. store as `CustomComponents`).
    pub fn deserialize_into_builder(
        &self,
        builder: &mut hecs::EntityBuilder,
        components: &HashMap<String, serde_json::Value>,
    ) -> HashMap<String, serde_json::Value> {
        let mut unknown = HashMap::new();

        for (name, value) in components {
            if let Some(desc) = self.descriptors.get(name) {
                if let Some(any_component) = (desc.deserialize_fn)(value) {
                    (desc.spawn_fn)(builder, any_component);
                } else {
                    log::warn!(
                        "Failed to deserialize component '{}': {:?}",
                        name,
                        value
                    );
                }
            } else {
                unknown.insert(name.clone(), value.clone());
            }
        }

        unknown
    }
}

// ──────────────────────────── The process-wide registry ────────────────────────────

/// The components the packages loaded in this process registered, for the
/// native script-command apply loop.
///
/// Scenes load through a registry the runtime holds, but script spawns apply
/// in `ecs::systems`, which has none: a prefab's package components (a
/// `shooter_body`) fell into `CustomComponents` there and the package never
/// saw them, while the editor spawned the real thing (BUGS #109). Published
/// once at startup, like `script_commands::install_global`, rather than
/// threaded through a hot loop that needs it only for spawns.
static GLOBAL: std::sync::RwLock<Option<ComponentRegistry>> = std::sync::RwLock::new(None);

/// Publish the registry for script spawns in this process. Call after
/// `PackageHost::initialize`; a second call replaces the first.
pub fn install_global(registry: ComponentRegistry) {
    match GLOBAL.write() {
        Ok(mut slot) => *slot = Some(registry),
        Err(e) => log::error!("[package] could not publish the component registry: {e}"),
    }
}

/// Split `custom` into the components the published registry knows, added to
/// `builder`, and the rest, returned. With nothing published, all of it is
/// returned.
pub fn deserialize_global(
    builder: &mut hecs::EntityBuilder,
    custom: &HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    match GLOBAL.read() {
        Ok(slot) => match slot.as_ref() {
            Some(registry) => registry.deserialize_into_builder(builder, custom),
            None => custom.clone(),
        },
        Err(_) => custom.clone(),
    }
}

/// Run `f` with the published registry - or, where no host publishes one
/// (the browser hosts), the core components' own, built once. What a system
/// that reaches components by their scene key uses (`ecs::timeline`), so it
/// works in every runtime: everywhere for core components, and for a
/// package's where the registry is published.
pub fn with_registry<R>(f: impl FnOnce(&ComponentRegistry) -> R) -> R {
    if let Ok(slot) = GLOBAL.read()
        && let Some(registry) = slot.as_ref()
    {
        return f(registry);
    }
    static CORE: std::sync::OnceLock<ComponentRegistry> = std::sync::OnceLock::new();
    f(CORE.get_or_init(|| {
        let mut registry = ComponentRegistry::new();
        crate::package::core_registration::register_core_components(&mut registry);
        registry
    }))
}

impl Default for ComponentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
