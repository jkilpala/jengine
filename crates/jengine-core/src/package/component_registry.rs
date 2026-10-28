//! Type-erased component registry for the package system.
//!
//! The one list of components: scenes (v1 and v2), prefabs and every spawn
//! decode a component by its scene key through it, and packages register
//! theirs here at startup. Each component is stored as a
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

    /// Clone a `&dyn Any` (which is `&T`). What lets a prefab be decoded
    /// once and spawned many times ([`Prepared`]).
    pub clone_fn: fn(&dyn Any) -> Box<dyn Any + Send + Sync>,
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

/// Write each number that is exactly an `f32` the way the `f32` prints -
/// `0.1`, not `0.10000000149011612`, which is what `to_value` makes of
/// `0.1f32` (it widens to `f64` first). Components are nearly all `f32`s and
/// a saved scene is read by people and diffed by git; either spelling reads
/// back as the same `f32`. Until Oct 2026 only v2 saves went through here; v1
/// ones, and so every save from the editor, serialized the typed struct
/// straight to text and never widened.
fn tidy_floats(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Number(n) if n.is_f64() => {
            let Some(wide) = n.as_f64() else { return };
            let narrow = wide as f32;
            if narrow.is_finite()
                && narrow as f64 == wide
                && let Ok(short) = narrow.to_string().parse::<f64>()
                && let Some(tidy) = serde_json::Number::from_f64(short)
            {
                *n = tidy;
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(tidy_floats),
        serde_json::Value::Object(fields) => fields.values_mut().for_each(tidy_floats),
        _ => {}
    }
}

/// A component from its JSON - the one copy of `from_value::<T>` both
/// `deserialize_fn` and `check_fn` call. Inlined into each closure it was
/// two copies of every component's deserializer (`docs/build-sizes.md`).
#[inline(never)]
fn decode<T: Component>(value: &serde_json::Value) -> Result<T, serde_json::Error> {
    serde_json::from_value::<T>(value.clone())
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
                let mut value = serde_json::to_value(component).unwrap_or(serde_json::Value::Null);
                tidy_floats(&mut value);
                value
            },

            deserialize_fn: |value| decode::<T>(value).ok().map(|c| Box::new(c) as Box<dyn Any>),

            check_fn: |value| decode::<T>(value).map(|_| ()).map_err(|e| e.to_string()),

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

            clone_fn: |any| Box::new(any.downcast_ref::<T>().expect("type mismatch in clone_fn").clone()),
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
                // Kept even when it is `null`: that is what a unit component
                // (`BulkEntity`) serializes to, and leaving it out dropped the
                // component from every save and the editor's Play snapshot.
                map.insert(name.clone(), (desc.serialize_fn)(any_component.as_ref()));
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
        self.add_components(builder, components)
    }

    /// [`Self::deserialize_into_builder`] over any map of scene keys to JSON
    /// (a `scene::format::ComponentBundle`, a `serde_json::Map`, ...). A
    /// component that does not deserialize is left out and named in the log
    /// with serde's reason.
    pub fn add_components<'a>(
        &self,
        builder: &mut hecs::EntityBuilder,
        components: impl IntoIterator<Item = (&'a String, &'a serde_json::Value)>,
    ) -> HashMap<String, serde_json::Value> {
        let mut unknown = HashMap::new();
        for (name, value) in components {
            match self.descriptors.get(name) {
                Some(desc) => match (desc.deserialize_fn)(value) {
                    Some(any_component) => (desc.spawn_fn)(builder, any_component),
                    None => warn_undecodable(desc, value),
                },
                None => {
                    note_left_out(name);
                    unknown.insert(name.clone(), value.clone());
                }
            }
        }
        unknown
    }

    /// Decode components once, to add them to any number of entities
    /// ([`Prepared::add_to`]) - what a prefab a script spawns by the hundred
    /// wants: decoding a bullet's JSON costs about four times cloning it
    /// (`jengine-benchmark-cpu`'s `spawn`).
    pub fn prepare<'a>(&self, components: impl IntoIterator<Item = (&'a String, &'a serde_json::Value)>) -> Prepared {
        let mut prepared = Prepared { parts: Vec::new(), unknown: HashMap::new() };
        for (name, value) in components {
            match self.descriptors.get(name) {
                Some(desc) => match (desc.deserialize_fn)(value) {
                    Some(any_component) => prepared.parts.push(PreparedPart {
                        type_id: desc.type_id,
                        spawn: desc.spawn_fn,
                        clone: desc.clone_fn,
                        component: (desc.clone_fn)(any_component.as_ref()),
                    }),
                    None => warn_undecodable(desc, value),
                },
                None => {
                    note_left_out(name);
                    prepared.unknown.insert(name.clone(), value.clone());
                }
            }
        }
        prepared
    }
}

/// A key no descriptor knows that is an engine feature's component, in a
/// build without that feature: say so, rather than let it pass as custom data
/// (`features::missing` says it once).
fn note_left_out(key: &str) {
    if let Some(f) = crate::features::of_component(key)
        && !crate::features::enabled(f.name)
    {
        crate::features::missing(f.name, &format!("the `{key}` component"));
    }
}

/// Name a component that would not deserialize, with serde's reason.
fn warn_undecodable(desc: &ComponentDescriptor, value: &serde_json::Value) {
    let why = (desc.check_fn)(value).err().unwrap_or_default();
    log::warn!("component '{}' could not be read and was left out: {why} (in {value})", desc.name);
}

/// Components decoded by [`ComponentRegistry::prepare`], added to an entity
/// by cloning rather than by decoding JSON again.
pub struct Prepared {
    parts: Vec<PreparedPart>,
    /// The keys no descriptor knows - custom data, carried as written.
    pub unknown: HashMap<String, serde_json::Value>,
}

struct PreparedPart {
    type_id: TypeId,
    spawn: fn(&mut hecs::EntityBuilder, Box<dyn Any>),
    clone: fn(&dyn Any) -> Box<dyn Any + Send + Sync>,
    component: Box<dyn Any + Send + Sync>,
}

impl Prepared {
    /// Add a copy of every decoded component to `builder`. The unknown keys
    /// are the caller's (`CustomComponents`, usually).
    pub fn add_to(&self, builder: &mut hecs::EntityBuilder) {
        for part in &self.parts {
            (part.spawn)(builder, (part.clone)(part.component.as_ref()));
        }
    }

    /// Whether a component of type `T` is among the decoded ones.
    pub fn has<T: 'static>(&self) -> bool {
        self.parts.iter().any(|p| p.type_id == TypeId::of::<T>())
    }
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared")
            .field("components", &self.parts.len())
            .field("unknown", &self.unknown.keys().collect::<Vec<_>>())
            .finish()
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


/// Put `value` at the dotted `path` inside `root`. A number written over an
/// integer is rounded, so a track can animate a count or a `z_order`.
pub fn set_path(root: &mut serde_json::Value, path: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut at = root;
    let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err("an empty field".into());
    }
    for (n, part) in parts.iter().enumerate() {
        let next = match at {
            serde_json::Value::Object(map) => map.get_mut(*part),
            serde_json::Value::Array(list) => part.parse::<usize>().ok().and_then(|i| list.get_mut(i)),
            _ => None,
        };
        let Some(next) = next else {
            return Err(format!("no field '{}' in '{path}'", parts[..=n].join(".")));
        };
        at = next;
    }
    *at = match (&*at, value) {
        (serde_json::Value::Number(old), serde_json::Value::Number(new)) if !old.is_f64() => {
            let r = new.as_f64().unwrap_or(0.0).round();
            if old.is_u64() && r >= 0.0 { (r as u64).into() } else { (r as i64).into() }
        }
        _ => value.clone(),
    };
    Ok(())
}

/// Write one field of `component` on `entity` through the registry's serde -
/// what a timeline's track and a behaviour tree's `write` leaf do.
pub fn write_field(world: &mut hecs::World, entity: hecs::Entity, component: &str, path: &str, value: &serde_json::Value) -> Result<(), String> {
    with_registry(|registry| {
        let desc = registry.get(component).ok_or_else(|| format!("no component '{component}'"))?;
        let current = (desc.query_fn)(world, entity).ok_or_else(|| format!("the entity has no '{component}'"))?;
        let mut json = (desc.serialize_fn)(current.as_ref());
        set_path(&mut json, path, value)?;
        let new = (desc.deserialize_fn)(&json).ok_or_else(|| format!("'{component}.{path}' cannot take {value}"))?;
        (desc.insert_fn)(world, entity, new);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Dust {
        speed: f32,
        spread: [f32; 2],
        seed: f64,
        count: u32,
    }

    #[test]
    fn f32s_save_as_they_print_and_read_back_the_same() {
        let mut registry = ComponentRegistry::new();
        registry.register::<Dust>("dust", "Dust", "Test");
        let desc = registry.get("dust").unwrap();
        let dust = Dust { speed: 0.1, spread: [0.3, 12.7], seed: 0.1, count: 3 };
        let value = (desc.serialize_fn)(&dust);
        let text = value.to_string();
        assert!(text.contains("\"speed\":0.1") && text.contains("[0.3,12.7]"), "{text}");
        assert!(text.contains("\"count\":3"), "integers stay integers: {text}");
        let back = (desc.deserialize_fn)(&value).unwrap();
        assert_eq!(back.downcast_ref::<Dust>(), Some(&dust), "the same values come back");
    }

    #[test]
    fn a_prepared_component_is_cloned_into_every_entity() {
        let mut registry = ComponentRegistry::new();
        registry.register::<Dust>("dust", "Dust", "Test");
        let json = serde_json::json!({ "dust": { "speed": 2.0, "spread": [1.0, 1.0], "seed": 0.0, "count": 1 }, "Notes": 4 });
        let prepared = registry.prepare(json.as_object().unwrap());
        assert!(prepared.has::<Dust>());
        assert_eq!(prepared.unknown.len(), 1, "the custom key is carried");
        let mut world = hecs::World::new();
        for _ in 0..3 {
            let mut builder = hecs::EntityBuilder::new();
            prepared.add_to(&mut builder);
            world.spawn(builder.build());
        }
        assert_eq!(world.query::<&Dust>().iter().count(), 3);
    }
}
