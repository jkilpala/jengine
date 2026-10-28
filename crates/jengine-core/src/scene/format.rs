//! Scene data format — JSON-serializable scene representation.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::animation::SpriteAnimator;
use crate::ecs::components::*;
use crate::package::component_registry::ComponentRegistry;
use crate::particles::ParticleEmitter;
use crate::renderer::postprocess::PostProcessSettings;
use crate::tilemap::Tilemap;
use crate::ui::UIImage;

/// A complete scene — serializable to/from JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneData {
    /// Scene format version, preserved so a **v2** scene survives a
    /// round-trip through this v1 struct.
    ///
    /// The editor loads every scene as `SceneData` and re-serializes it to
    /// push into the viewport. Without this field the version was silently
    /// dropped, so a v2 scene arrived looking like v1 — and `load_scene_auto`
    /// then took the `ComponentBundle` path, where a package component is an
    /// unknown key that lands in `CustomComponents` instead of becoming a real
    /// component. The package's system found nothing and did nothing, on an
    /// otherwise healthy-looking scene.
    ///
    /// `skip_serializing_if` keeps v1 files byte-identical rather than
    /// stamping `"version": 1` across every scene in the repo.
    #[serde(default = "default_scene_version", skip_serializing_if = "is_v1")]
    pub version: u32,
    pub name: String,
    pub entities: Vec<EntityData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_process: Option<PostProcessSettings>,
    /// Optional `.sortlayers` asset ID that becomes the active set when
    /// this scene loads. `None` keeps raw `z_order` values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layers: Option<String>,
    /// Every other top-level key, carried through untouched: `audio_refs`,
    /// `atlas_refs`, `paths`, `texture_refs` and whatever comes next.
    ///
    /// The editor loads a scene as this struct and saves it back, and serde
    /// dropped any key without a field. So saving a scene in the editor
    /// deleted its `audio_refs` (68 scenes have one), `atlas_refs` (52) and
    /// `paths` from disk, and Play - which pushes the editor's re-serialized
    /// copy to the viewport - ran without them: test-lab scene 64's patrols
    /// stood still in the editor because their routes never arrived
    /// (BUGS #157). A new scene-level key needs no field here to survive.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_scene_version() -> u32 {
    1
}

fn is_v1(v: &u32) -> bool {
    *v <= 1
}

impl Default for SceneData {
    fn default() -> Self {
        Self {
            version: 1,
            name: "Untitled Scene".to_string(),
            entities: Vec::new(),
            post_process: None,
            sort_layers: None,
            extra: Default::default(),
        }
    }
}

impl SceneData {
    /// [`ComponentBundle::normalize`] every entity, parts included.
    pub fn normalize(&mut self, registry: &ComponentRegistry) {
        fn walk(entity: &mut EntityData, registry: &ComponentRegistry) {
            entity.components.normalize(registry);
            for part in &mut entity.children {
                walk(part, registry);
            }
        }
        for entity in &mut self.entities {
            walk(entity, registry);
        }
    }
}

/// A single entity's serializable representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityData {
    pub name: String,
    /// Parent entity name. `None` means top-level (no parent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub components: ComponentBundle,
    /// The entities hung under this one (Oct 2026; Godot's nested scenes,
    /// Unity's prefab hierarchies): a prefab's parts - a cart's wheels, a
    /// catapult's arm - each its own `EntityData` with children of its own,
    /// placed in this one's frame. A child may be a `prefab_instance`, so a
    /// prefab can be built of prefabs. A scene flattens them into entities
    /// `parent`ed by name when it loads (`scene::prefab`); a script's
    /// `spawn_prefab` spawns the whole tree in every runtime
    /// (`scene::prefab::spawn_children`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<EntityData>,
}

/// An entity's components as a scene or prefab file holds them: each one's
/// JSON under its scene key (`"sprite"`, `"collider"`, a package's, a custom
/// component's), decoded through the component registry when the entity is
/// spawned ([`ComponentBundle::add_to_builder`]).
///
/// Until Oct 2026 this was a struct with a typed field per built-in
/// component - 176 of them, beside a registry that knew every one too - and
/// a hand-written line per field in the loader, the saver and the spawner:
/// a second copy of every component's serde code in every game (the struct
/// alone was 0.84 MB of the web module, `docs/build-sizes.md`), in which a
/// package's component could only be custom data. The JSON on disk is
/// unchanged. Read a component with [`ComponentBundle::get`].
///
/// Parsing a bundle checks nothing about the components in it - that happens
/// when one is decoded, and a component that will not decode is left out and
/// logged. A tool that wants a file's errors before anything spawns calls
/// [`ComponentBundle::check`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ComponentBundle(pub serde_json::Map<String, serde_json::Value>);

impl ComponentBundle {
    pub fn new() -> Self {
        Self::default()
    }

    /// A component's JSON, by its scene key.
    pub fn value(&self, key: &str) -> Option<&serde_json::Value> {
        self.0.get(key)
    }

    pub fn value_mut(&mut self, key: &str) -> Option<&mut serde_json::Value> {
        self.0.get_mut(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// The component under `key`, decoded as a `T` - `None` when the bundle
    /// has none, or when its JSON is not a `T` (which is logged).
    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        let value = self.0.get(key)?;
        // An owned `Value`, as the registry decodes: one copy of each
        // component's deserializer, not one per kind of reader.
        match serde_json::from_value(value.clone()) {
            Ok(component) => Some(component),
            Err(e) => {
                log::warn!("component '{key}' could not be read: {e}");
                None
            }
        }
    }

    /// Set the component under `key`.
    pub fn set<T: Serialize>(&mut self, key: &str, component: &T) {
        match serde_json::to_value(component) {
            Ok(value) => {
                self.0.insert(key.to_string(), value);
            }
            Err(e) => log::warn!("component '{key}' could not be written: {e}"),
        }
    }

    /// The bundle with the component under `key` set -
    /// `ComponentBundle::new().with("sprite", &sprite)`.
    pub fn with<T: Serialize>(mut self, key: &str, component: &T) -> Self {
        self.set(key, component);
        self
    }

    pub fn insert_value(&mut self, key: impl Into<String>, value: serde_json::Value) -> Option<serde_json::Value> {
        self.0.insert(key.into(), value)
    }

    pub fn remove(&mut self, key: &str) -> Option<serde_json::Value> {
        self.0.remove(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &serde_json::Value)> {
        self.0.iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Add every component `registry` knows to `builder`, decoded, and return
    /// the keys it does not know - custom data, for a `CustomComponents`.
    pub fn add_to_builder(
        &self,
        builder: &mut hecs::EntityBuilder,
        registry: &ComponentRegistry,
    ) -> HashMap<String, serde_json::Value> {
        registry.add_components(builder, &self.0)
    }

    /// Rewrite every component `registry` knows as it decodes and encodes
    /// again - its type's own form, defaults filled in except where the type
    /// skips them - and leave the rest as written. What
    /// a tool showing or saving a scene field by field wants: the editor reads
    /// and writes scenes through it, which is what it got when the bundle was
    /// a typed struct. A component that does not decode is left as it was.
    pub fn normalize(&mut self, registry: &ComponentRegistry) {
        for (key, value) in self.0.iter_mut() {
            if let Some(desc) = registry.get(key)
                && let Some(component) = (desc.deserialize_fn)(value)
            {
                *value = (desc.serialize_fn)(component.as_ref());
            }
        }
    }

    /// Every component `registry` knows whose JSON it refuses, with serde's
    /// reason - what a shipped-scene guard asks, since parsing alone no
    /// longer looks inside components.
    pub fn check(&self, registry: &ComponentRegistry) -> Vec<(String, String)> {
        self.0
            .iter()
            .filter_map(|(key, value)| {
                let desc = registry.get(key)?;
                (desc.check_fn)(value).err().map(|why| (key.clone(), why))
            })
            .collect()
    }
}

/// The keys of what is drawn or placed: an entity with one of them and no
/// `transform` is given a default one when a v1 scene loads.
const NEEDS_TRANSFORM: [&str; 6] = ["sprite", "sprite_animator", "anim_state_machine", "tilemap", "camera", "vector_path"];

/// Warn about custom components authored under a `"custom"` wrapper.
///
/// `ComponentBundle.custom` is `#[serde(flatten)]`, so it swallows every key
/// that is not a known built-in. Authoring
///
/// ```json
/// "components": { "custom": { "Stats": { "hp": 100 } } }
/// ```
///
/// therefore produces one custom component *named* `custom`, whose value is
/// an object — not a `Stats` component. `get_custom(e, "Stats", "hp")` then
/// returns nil forever.
///
/// This is silent, survives a save/load round trip unchanged, and is usually
/// masked by an `or 0` fallback in the script that reads it. Scene 37 in the
/// test lab shipped that way and rendered zeros for months. The names have to
/// sit directly under `components`.
fn warn_about_nested_custom(
    entity_name: &str,
    custom: &HashMap<String, serde_json::Value>,
) {
    let Some(nested) = custom.get("custom") else {
        return;
    };
    // Only complain when it looks like the mistake: an object of objects is a
    // wrapper, while a scalar is someone who genuinely wanted a component
    // called "custom" and is entitled to it.
    let inner_names: Vec<&str> = nested
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(_, v)| v.is_object())
                .map(|(k, _)| k.as_str())
                .collect()
        })
        .unwrap_or_default();
    if inner_names.is_empty() {
        return;
    }
    log::warn!(
        "[scene] entity '{entity_name}' has a custom component named 'custom' wrapping \
         {inner_names:?}. `custom` is not a component name - it is the catch-all field, \
         so these arrived as one component called 'custom' and `get_custom(e, \"{}\", ...)` \
         will return nil. Move them directly under `components`.",
        inner_names[0]
    );
}

/// Save a hecs World to `SceneData`: every component the component registry
/// knows (the published one, or core's), by its scene key, with each
/// entity's custom components beside them.
pub fn save_scene(world: &hecs::World) -> SceneData {
    crate::package::component_registry::with_registry(|registry| save_scene_with(world, registry))
}

/// [`save_scene`] through a given registry.
pub fn save_scene_with(world: &hecs::World, registry: &ComponentRegistry) -> SceneData {
    let mut entities = Vec::new();
    for (entity, _) in world.query::<()>().iter() {
        // Transient health-bar sprites and name tags are managed by
        // `sync_health_bars` / `sync_name_tags` at runtime; never persist
        // them to the scene.
        if world.get::<&HealthBarView>(entity).is_ok() || world.get::<&NameTagView>(entity).is_ok() {
            continue;
        }
        let name = world
            .get::<&Name>(entity)
            .map(|n| n.0.clone())
            .unwrap_or_else(|_| format!("Entity_{}", entity.id()));
        let mut components = ComponentBundle::new();
        for (key, value) in registry.serialize_entity(world, entity) {
            components.insert_value(key, value);
        }
        // No tags is no `tags` key.
        if components.value("tags").and_then(|t| t.as_array()).is_some_and(|t| t.is_empty()) {
            components.remove("tags");
        }
        if let Ok(custom) = world.get::<&CustomComponents>(entity) {
            for (key, value) in &custom.0 {
                if !components.contains(key) {
                    components.insert_value(key.clone(), value.clone());
                }
            }
        }
        let parent = world
            .get::<&Parent>(entity)
            .ok()
            .and_then(|p| world.get::<&Name>(p.0).ok().map(|n| n.0.clone()));
        entities.push(EntityData { name, parent, components, children: Vec::new() });
    }

    SceneData {
        version: 1,
        name: "Scene".to_string(),
        entities,
        post_process: None,
        sort_layers: None,
        extra: Default::default(),
    }
}

/// Load SceneData into a hecs World (clears existing entities first).
pub fn load_scene(data: &SceneData, world: &mut hecs::World) {
    let _ = load_scene_with_mapping(data, world);
}

/// Identical to [`load_scene`], but also returns a `Vec<hecs::Entity>`
/// where index `i` is the entity spawned for `data.entities[i]`. Used
/// by surfaces that need a stable scene-index → world-entity map
/// (e.g. the editor viewport's behavior-graph mirror, where SceneData
/// remains the source of truth for rendering and a parallel hecs World
/// is maintained only so `BehaviorGraphSystem` has somewhere to run).
pub fn load_scene_with_mapping(
    data: &SceneData,
    world: &mut hecs::World,
) -> Vec<hecs::Entity> {
    crate::package::component_registry::with_registry(|registry| load_scene_with_registry(data, world, registry))
}

/// Load a v1 scene through `registry` (clears the world first), returning
/// the entity each of `data.entities` became. Every component the registry
/// knows is decoded - a package's too - and the rest is custom data.
pub fn load_scene_with_registry(
    data: &SceneData,
    world: &mut hecs::World,
    registry: &ComponentRegistry,
) -> Vec<hecs::Entity> {
    world.clear();
    let mut mapping = Vec::with_capacity(data.entities.len());

    for entity_data in &data.entities {
        let components = &entity_data.components;
        let mut builder = hecs::EntityBuilder::new();
        builder.add(Name(entity_data.name.clone()));
        if !components.contains("transform") && NEEDS_TRANSFORM.iter().any(|k| components.contains(k)) {
            builder.add(Transform::default());
        }
        let custom = components.add_to_builder(&mut builder, registry);
        if !custom.is_empty() {
            warn_about_nested_custom(&entity_data.name, &custom);
            builder.add(CustomComponents(custom));
        }
        mapping.push(world.spawn(builder.build()));
    }
    for (i, entity_data) in data.entities.iter().enumerate() {
        let Some(parent_name) = entity_data.parent.as_deref() else { continue };
        // Find first entity in the scene with this name. Names are not
        // guaranteed unique; first-match keeps behavior deterministic.
        let parent_entity = data
            .entities
            .iter()
            .position(|e| e.name == parent_name)
            .map(|idx| mapping[idx]);
        match parent_entity {
            Some(p) if p != mapping[i] => {
                let _ = world.insert_one(mapping[i], Parent(p));
            }
            Some(_) => {
                log::warn!(
                    "Scene entity '{}' lists itself as parent — ignoring.",
                    entity_data.name
                );
            }
            None => {
                log::warn!(
                    "Scene entity '{}' has unknown parent '{}' — ignoring.",
                    entity_data.name,
                    parent_name
                );
            }
        }
    }
    mapping
}

// ──────────────────────────── V2: Registry-Based Scene Format ────────────────────────────

/// V2 scene data — components stored as a dynamic JSON map instead of `ComponentBundle`.
///
/// The JSON shape is identical to v1 but components are serialized/deserialized
/// through the `ComponentRegistry`, so package-provided components are supported
/// without modifying this code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneDataV2 {
    /// Format version — always 2 for registry-based scenes.
    #[serde(default = "default_version")]
    pub version: u32,
    pub name: String,
    pub entities: Vec<EntityDataV2>,
    /// Optional `.sortlayers` asset ID that becomes the active set when
    /// this scene loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layers: Option<String>,
}

fn default_version() -> u32 {
    2
}

impl Default for SceneDataV2 {
    fn default() -> Self {
        Self {
            version: 2,
            name: "Untitled Scene".to_string(),
            entities: Vec::new(),
            sort_layers: None,
        }
    }
}

/// V2 entity data — component map is fully dynamic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityDataV2 {
    pub name: String,
    /// The entity this one hangs from, by name - the same field, and the
    /// same first-match rule, as a v1 scene's. Until Sep 2026 a v2 entity
    /// had no way to say it, and `save_scene_v2` dropped every `Parent`: a
    /// package game could not author a rig, and the editor's Play snapshot
    /// flattened the scene on Stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The parent's position in `entities`, which a save writes when the
    /// parent's name is shared - two legs both called `Leg` - so a foot
    /// cannot land on its namesake's leg. Preferred over `parent` when valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_index: Option<usize>,
    /// All components as `{ "component_name": { ... }, ... }`.
    pub components: HashMap<String, serde_json::Value>,
}

impl EntityDataV2 {
    pub fn new(name: String, components: HashMap<String, serde_json::Value>) -> Self {
        Self { name, parent: None, parent_index: None, components }
    }
}

/// Save a hecs World to `SceneDataV2` using the component registry.
///
/// Registered components are serialized via their `serialize_fn`.
/// `CustomComponents` data (script-created dynamic components) is flattened
/// into the same map. `Name` is extracted to the entity-level `name` field.
pub fn save_scene_v2(world: &hecs::World, registry: &ComponentRegistry) -> SceneDataV2 {
    save_scene_v2_inner(world, registry, false)
}

/// Snapshot a world for a scene push / pop, skipping bulk-spawned entities.
///
/// # Why this is not just `save_scene_v2`
///
/// A snapshot is taken on `push_scene`, on `pop_scene` and at the start of a
/// play session, so its cost is a hitch in the middle of the game rather than
/// something a save dialog pays. It is also the one case where preserving
/// *everything* is not obviously right: entities marked [`BulkEntity`] are
/// spawned in bulk by something that owns them — a flock, a particle-like
/// swarm — and that owner respawns them. Writing 16,000 boids into a snapshot
/// so they can be restored identically costs real time and restores state
/// nobody addressed individually in the first place.
///
/// So this skips them and `save_scene_v2` does not. A genuine save must keep
/// every entity, including any a scene chose to author with the marker; only
/// the transient in-play snapshot may drop them.
///
/// `BulkEntity` already means "spawned in bulk, not addressed individually" —
/// it governs the script mirror and the editor's Scene panel for the same
/// reason. This is the third thing it decides, and a game that wants its
/// bulk-spawned entities restored across a scene push simply does not set it.
pub fn snapshot_scene_v2(world: &hecs::World, registry: &ComponentRegistry) -> SceneDataV2 {
    save_scene_v2_inner(world, registry, true)
}

fn save_scene_v2_inner(
    world: &hecs::World,
    registry: &ComponentRegistry,
    skip_bulk: bool,
) -> SceneDataV2 {
    use crate::ecs::components::BulkEntity;
    let mut entities = Vec::new();
    let mut saved: Vec<hecs::Entity> = Vec::new();

    for (entity, _) in world.query::<()>().iter() {
        if skip_bulk && world.get::<&BulkEntity>(entity).is_ok() {
            continue;
        }
        saved.push(entity);
        let name = world
            .get::<&Name>(entity)
            .map(|n| n.0.clone())
            .unwrap_or_else(|_| format!("Entity_{}", entity.id()));

        let mut components = registry.serialize_entity(world, entity);

        // Flatten CustomComponents entries into the component map
        if let Ok(custom) = world.get::<&CustomComponents>(entity) {
            for (k, v) in &custom.0 {
                // Don't overwrite registered components with custom data
                if !components.contains_key(k) {
                    components.insert(k.clone(), v.clone());
                }
            }
        }

        entities.push(EntityDataV2::new(name, components));
    }

    // Parents, once every entity has its place in the list. By name, and by
    // index as well wherever the name alone would pick the wrong entity.
    let index_of: HashMap<hecs::Entity, usize> = saved.iter().enumerate().map(|(i, e)| (*e, i)).collect();
    let names: Vec<String> = entities.iter().map(|e| e.name.clone()).collect();
    let mut name_count: HashMap<&str, usize> = HashMap::new();
    for n in &names {
        *name_count.entry(n.as_str()).or_default() += 1;
    }
    for (i, entity) in saved.iter().enumerate() {
        let Ok(p) = world.get::<&Parent>(*entity) else { continue };
        // A parent that is not being saved (a bulk entity in a snapshot,
        // say) has nothing to point at.
        let Some(&pi) = index_of.get(&p.0) else { continue };
        entities[i].parent = Some(names[pi].clone());
        if name_count.get(names[pi].as_str()).copied().unwrap_or(0) > 1 {
            entities[i].parent_index = Some(pi);
        }
    }

    SceneDataV2 {
        version: 2,
        name: "Scene".to_string(),
        entities,
        sort_layers: None,
    }
}

/// Load `SceneDataV2` into a hecs World using the component registry.
///
/// Registered components are deserialized via their `deserialize_fn`.
/// Unknown keys (not in the registry) are collected into a `CustomComponents`
/// component on the entity.
pub fn load_scene_v2(data: &SceneDataV2, world: &mut hecs::World, registry: &ComponentRegistry) {
    world.clear();

    for entity_data in &data.entities {
        let mut builder = hecs::EntityBuilder::new();
        builder.add(Name(entity_data.name.clone()));

        let unknown = registry.deserialize_into_builder(&mut builder, &entity_data.components);

        // Store unrecognized keys as CustomComponents
        if !unknown.is_empty() {
            builder.add(CustomComponents(unknown));
        }

        world.spawn(builder.build());
    }
    resolve_v2_parents(data, world);
}

/// Give every entity of a freshly loaded v2 scene the `Parent` it names.
/// `load_scene_v2` spawns into an empty world in list order, so the scene's
/// entities are the world's, in that order.
fn resolve_v2_parents(data: &SceneDataV2, world: &mut hecs::World) {
    if data.entities.iter().all(|e| e.parent.is_none() && e.parent_index.is_none()) {
        return;
    }
    let mut spawned: Vec<hecs::Entity> = world.iter().map(|e| e.entity()).collect();
    spawned.sort_by_key(|e| e.id());
    if spawned.len() != data.entities.len() {
        log::warn!("v2 scene: {} entities spawned for {} listed - parents not resolved", spawned.len(), data.entities.len());
        return;
    }
    for (i, e) in data.entities.iter().enumerate() {
        let by_index = e.parent_index.filter(|&pi| pi < spawned.len() && pi != i);
        let target = by_index.or_else(|| {
            let name = e.parent.as_deref()?;
            let found = data.entities.iter().position(|o| o.name == name);
            if found.is_none() {
                log::warn!("Scene entity '{}' has unknown parent '{}' - ignoring.", e.name, name);
            }
            found
        });
        match target {
            Some(pi) if pi != i => {
                let _ = world.insert_one(spawned[i], Parent(spawned[pi]));
            }
            Some(_) => log::warn!("Scene entity '{}' lists itself as parent - ignoring.", e.name),
            None => {}
        }
    }
}

/// Convert a v1 `SceneData` (with `ComponentBundle`) to v2 format.
///
/// Useful for migrating existing scene files. The conversion loads the v1 scene
/// into a temporary world, then re-saves it using the registry.
pub fn upgrade_scene_v1_to_v2(
    v1: &SceneData,
    registry: &ComponentRegistry,
) -> SceneDataV2 {
    let mut world = hecs::World::new();
    load_scene_with_registry(v1, &mut world, registry);
    let mut v2 = save_scene_v2(&world, registry);
    v2.name = v1.name.clone();
    v2
}

/// Load a scene from JSON, auto-detecting v1 or v2 format.
///
/// If the JSON has `"version": 2`, it's loaded as v2. Otherwise it's treated
/// as v1 and loaded through the same registry, so a package's components in
/// a v1 scene are components, not custom data.
pub fn load_scene_auto(
    json: &str,
    world: &mut hecs::World,
    registry: &ComponentRegistry,
) -> Result<(), serde_json::Error> {
    load_scene_value(serde_json::from_str(json)?, world, registry)
}

/// A scene's top-level fields without its entities - its `audio_refs`,
/// `atlas_refs`, `sort_layers`, `paths`, `post_process`, `name` - which is all
/// a reader of scene-wide settings looks at. Cheap to copy where the entity
/// list is not, so a runtime keeps this and hands the scene itself to
/// [`load_scene_value`].
pub fn scene_header(scene: &serde_json::Value) -> serde_json::Value {
    match scene.as_object() {
        Some(obj) => serde_json::Value::Object(
            obj.iter().filter(|(k, _)| k.as_str() != "entities").map(|(k, v)| (k.clone(), v.clone())).collect(),
        ),
        None => serde_json::Value::Null,
    }
}

/// [`load_scene_auto`] for a scene already parsed. Takes the value whole -
/// scene types are only ever read from an owned value (`from_json_text`) - so
/// a runtime reads what else it wants from the scene (`scene_metadata`, its
/// `audio_refs`, ...) first and hands it over last.
pub fn load_scene_value(
    raw: serde_json::Value,
    world: &mut hecs::World,
    registry: &ComponentRegistry,
) -> Result<(), serde_json::Error> {
    let version = raw.get("version").and_then(|v| v.as_u64()).unwrap_or(1);

    if version >= 2 {
        let data: SceneDataV2 = serde_json::from_value(raw)?;
        load_scene_v2(&data, world, registry);
    } else {
        let data: SceneData = serde_json::from_value(raw)?;
        load_scene_with_registry(&data, world, registry);
    }
    Ok(())
}

/// Scene-level metadata that lives outside the per-entity data.
///
/// Callers that need to apply scene-wide settings (e.g. the active
/// `.sortlayers` set) can read this without re-deserializing the whole
/// entity list. Populated for both v1 and v2 scene JSON.
// `Eq` is gone since `paths` arrived: `Path2D` holds floats, and comparing
// waypoint coordinates for exact equality is not a meaningful thing to want.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneMetadata {
    /// Asset id of the `.sortlayers` set this scene wants active.
    pub sort_layers: Option<String>,
    /// Named waypoint paths for `PatrolPathRef` entities in this scene.
    ///
    /// # Why these live in the scene
    ///
    /// `Path2D` and `Path2DLibrary` and the patrol system have existed, with
    /// tests, since the component was added — but nothing ever filled the
    /// library, so `PatrolPathRef` moved nothing in any runtime (BUGS #7). The
    /// missing piece was never the algorithm, it was a way to get points in.
    ///
    /// A patrol route is level geometry: it is placed against the walls of one
    /// scene and means nothing in another. Putting it here rather than in a
    /// separate `.path` asset means every runtime gets it from the scene
    /// parsing it already does — no third loader, and no push API for the
    /// editor, which has no filesystem of its own. Many entities in a scene can
    /// still share one route, which is the reason `Path2D` is a shared asset
    /// rather than a `Vec<Vec2>` on the component.
    ///
    /// If routes ever need to be shared *across* scenes, a `.path` asset
    /// referenced by id is the natural next step — `PatrolPathRef.path_id` is
    /// already a string key and would not change.
    pub paths: std::collections::HashMap<String, crate::ecs::path::Path2D>,
}

/// Parse only the scene-level metadata fields from a scene JSON string.
/// Cheap; does not allocate the entity list. Works for v1 and v2.
pub fn read_scene_metadata(json: &str) -> Result<SceneMetadata, serde_json::Error> {
    Ok(scene_metadata(&serde_json::from_str(json)?))
}

/// [`read_scene_metadata`] for a scene already parsed.
pub fn scene_metadata(raw: &serde_json::Value) -> SceneMetadata {
    let sort_layers = raw
        .get("sort_layers")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    // A malformed path is dropped rather than failing the whole scene load:
    // the rest of the level is still playable, and the patrol system already
    // skips an id it cannot resolve.
    let mut paths = std::collections::HashMap::new();
    if let Some(map) = raw.get("paths").and_then(|v| v.as_object()) {
        for (id, value) in map {
            match serde_json::from_value::<crate::ecs::path::Path2D>(value.clone()) {
                Ok(path) => {
                    paths.insert(id.clone(), path);
                }
                Err(e) => log::warn!("scene path '{id}' is malformed and was skipped: {e}"),
            }
        }
    }
    SceneMetadata { sort_layers, paths }
}

/// Every texture a set of prefabs will need once something spawns one.
///
/// A runtime preloads textures by walking the **world**, which is the right
/// answer for everything the scene names and the wrong answer for
/// everything it does not: a prefab's sheet is referenced by nothing until
/// a script spawns from it, mid-frame, far too late to fetch an image. The
/// result is a prefab that spawns, lives, moves and draws nothing at all,
/// with no warning anywhere - the missing-texture warning only fires for
/// ids something asked for.
///
/// The web runtime had its own copy of this from the start; native did not,
/// which is how a game's RTS came to have an invisible army (BUGS #137).
pub fn prefab_texture_ids<'a>(
    prefabs: impl IntoIterator<Item = &'a EntityData>,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    // A prefab's parts (`EntityData::children`) need theirs too: a wagon's
    // barrels are named by nothing but the wagon.
    let mut todo: Vec<&EntityData> = prefabs.into_iter().collect();
    while let Some(entity) = todo.pop() {
        todo.extend(entity.children.iter());
        let Ok(value) = serde_json::to_value(&entity.components) else {
            continue;
        };
        for key in ["sprite", "mesh", "trail", "line", "particle_emitter", "lightmap", "ui_image", "sprite_stack", "mode7"] {
            if let Some(id) = value
                .get(key)
                .and_then(|s| s.get("texture_id"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                out.insert(id.to_string());
            }
        }
        // Running water's picture (`ecs::flow`).
        if let Some(id) = value.get("flow_path").and_then(|t| t.get("texture")).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            out.insert(id.to_string());
        }
        // A toast stack's card and icons (`ui::toast`).
        if let Some(t) = value.get("toast_stack") {
            if let Some(id) = t.get("texture").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                out.insert(id.to_string());
            }
            for id in t.get("icons").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str()) {
                out.insert(id.to_string());
            }
        }
        // A speech bubble's pictures (`ui::speech_bubble`).
        for key in ["texture", "tail_texture"] {
            if let Some(id) = value.get("speech_bubble").and_then(|t| t.get(key)).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                out.insert(id.to_string());
            }
        }
        // An off-screen marker's arrow and icon (`ecs::offscreen_marker`).
        for key in ["arrow_texture", "icon_texture"] {
            if let Some(id) = value.get("offscreen_marker").and_then(|t| t.get(key)).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                out.insert(id.to_string());
            }
        }
        // What a sliceable shows inside (`ecs::slice`), once cut.
        if let Some(id) = value.get("sliceable").and_then(|t| t.get("inside_texture")).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            out.insert(id.to_string());
        }
        // A tilted sprite's back (`ecs::sprite_tilt`), shown only once it is
        // turned over.
        if let Some(id) = value.get("sprite_tilt").and_then(|t| t.get("back_texture")).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            out.insert(id.to_string());
        }
        // A sprite library's parts (`ecs::sprite_library`).
        if let Some(cats) = value.get("sprite_library").and_then(|l| l.get("categories")).and_then(|c| c.as_object()) {
            for entry in cats.values().filter_map(|c| c.as_object()).flat_map(|c| c.values()) {
                if let Some(id) = entry.get("texture_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    out.insert(id.to_string());
                }
            }
        }
        // A light's cookie (`PointLight2D::cookie`), named by nothing else.
        for key in ["point_light", "spot_light"] {
            if let Some(id) = value
                .get(key)
                .and_then(|s| s.get("cookie"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                out.insert(id.to_string());
            }
        }
        // A sprite animator can swap the texture per frame.
        if let Some(clips) = value
            .get("sprite_animator")
            .and_then(|a| a.get("clips"))
            .and_then(|c| c.as_object())
        {
            for clip in clips.values() {
                let Some(frames) = clip.get("frames").and_then(|f| f.as_array()) else {
                    continue;
                };
                for frame in frames {
                    if let Some(id) = frame
                        .get("texture_id")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        out.insert(id.to_string());
                    }
                }
            }
        }
        // And a prefab can carry a whole tilemap.
        if let Some(sets) = value
            .get("tilemap")
            .and_then(|t| t.get("tilesets"))
            .and_then(|t| t.as_array())
        {
            for set in sets {
                if let Some(id) = set
                    .get("texture_id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    out.insert(id.to_string());
                }
            }
        }
    }
    out
}

/// The material files a prefab draws with once something spawns one -
/// built-in ids (`default_sprite`, `additive`, ...) name no file and are left
/// out.
///
/// The same blind spot [`prefab_texture_ids`] closes, for materials: the
/// editor pushed only the materials scene entities named, so a graph material
/// that only a spawned prefab used never reached its viewport, and the sprite
/// drew with the default shader - the showcase's Spellcraft shockwaves (a
/// graph over a white quad) flashed as white boxes in the editor while
/// native, which loads materials on demand, drew them. The browser hosts
/// fetch these as soon as the prefabs arrive.
pub fn prefab_material_ids<'a>(
    prefabs: impl IntoIterator<Item = &'a EntityData>,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for entity in prefabs {
        let Ok(value) = serde_json::to_value(&entity.components) else {
            continue;
        };
        if let Some(id) = value
            .get("material")
            .and_then(|m| m.get("material_id"))
            .and_then(|v| v.as_str())
            .filter(|s| !crate::renderer::material::is_builtin_material(s))
        {
            out.insert(id.to_string());
        }
    }
    out
}

/// Every texture a loaded world draws with, for a runtime to fetch before
/// the first frame.
///
/// Native and the web runtime each kept their own copy of this walk, and
/// the two had drifted: the web's knew nothing of particle, animation-frame
/// or tileset textures, and **neither knew `UIImage`** - so an entity-UI
/// icon on a sheet no sprite shared drew nothing outside the editor (which
/// pulls whatever the renderer misses). One list now, beside
/// [`prefab_texture_ids`], which is the same question asked of prefabs.
pub fn world_texture_ids(world: &hecs::World) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let mut add = |id: &str| {
        if !id.is_empty() {
            out.insert(id.to_string());
        }
    };
    for (_, s) in world.query::<&SpriteComponent>().iter() {
        add(&s.texture_id);
    }
    // A mesh names a texture the way a sprite does.
    for (_, m) in world.query::<&Mesh2D>().iter() {
        add(&m.texture_id);
    }
    // Every part a sprite library can swap in: nothing names one until it
    // is worn, which is too late to read a file.
    #[cfg(feature = "sprite_library")]
    for (_, l) in world.query::<&crate::ecs::sprite_library::SpriteLibrary>().iter() {
        for id in l.texture_ids() {
            add(id);
        }
    }
    // A Mode 7 plane's ground (`ecs::mode7`).
    #[cfg(feature = "mode7")]
    for (_, m) in world.query::<&crate::ecs::mode7::Mode7>().iter() {
        add(&m.texture_id);
    }
    // A sprite stack's strip of slices (`ecs::sprite_stack`).
    #[cfg(feature = "sprite_stack")]
    for (_, s) in world.query::<&crate::ecs::sprite_stack::SpriteStack>().iter() {
        add(&s.texture_id);
    }
    // Running water's picture (`ecs::flow`).
    #[cfg(feature = "flow")]
    for (_, f) in world.query::<&crate::ecs::flow::FlowPath>().iter() {
        add(&f.texture);
    }
    // A toast stack's card and the icons its toasts will show (`ui::toast`).
    #[cfg(feature = "toast")]
    for (_, s) in world.query::<&crate::ui::toast::ToastStack>().iter() {
        add(&s.texture);
        for icon in &s.icons {
            add(icon);
        }
    }
    // A speech bubble's pictures (`ui::speech_bubble`): drawn only once
    // someone speaks.
    #[cfg(feature = "speech_bubble")]
    for (_, b) in world.query::<&crate::ui::speech_bubble::SpeechBubble>().iter() {
        add(&b.texture);
        add(&b.tail_texture);
    }
    // An off-screen marker's arrow and icon (`ecs::offscreen_marker`): it
    // swaps between them.
    #[cfg(feature = "offscreen_marker")]
    for (_, m) in world.query::<&crate::ecs::offscreen_marker::OffscreenMarker>().iter() {
        add(&m.arrow_texture);
        add(&m.icon_texture);
    }
    // What a sliceable shows inside (`ecs::slice`): drawn only once cut.
    #[cfg(feature = "slice")]
    for (_, s) in world.query::<&crate::ecs::slice::Sliceable>().iter() {
        add(&s.inside_texture);
    }
    // A tilted sprite's back (`ecs::sprite_tilt`): nothing names it until
    // the sprite is turned over.
    #[cfg(feature = "sprite_tilt")]
    for (_, t) in world.query::<&crate::ecs::sprite_tilt::SpriteTilt>().iter() {
        add(&t.back_texture);
    }
    // A conversation's portraits (`ecs::dialogue`), read from its table.
    crate::feature!("dialogue", {
        for (_, d) in world.query::<&crate::ecs::dialogue::Dialogue>().iter() {
            for id in crate::ecs::dialogue::portrait_ids(d) {
                add(&id);
            }
        }
    });
    // A spline shape's fill and edge (`renderer::spline_shape`).
    #[cfg(feature = "spline_shape")]
    for (_, s) in world.query::<&crate::renderer::spline_shape::SplineShape>().iter() {
        add(&s.fill_texture);
        add(&s.edge_texture);
    }
    // A lit material's normal map: without it the lit pass shades against
    // the flat fallback and looks like a plain sprite.
    for (_, m) in world.query::<&MaterialComponent>().iter() {
        if let Some(id) = m.normal_map_id.as_deref() {
            add(id);
        }
    }
    // A lightmap is drawn into the light buffer, so nothing else names it
    // (BUGS #95).
    // A colour grade's strip: nothing draws it, so nothing else names it.
    for (_, l) in world.query::<&crate::ecs::components::ColorLut>().iter() {
        add(&l.texture_id);
    }
    // A falling-sand grid's seed picture (`ecs::pixel_sim`), read for its
    // pixels and drawn by nothing.
    #[cfg(feature = "pixel_sim")]
    for (_, p) in world.query::<&crate::ecs::pixel_sim::PixelSim>().iter() {
        add(&p.seed_texture);
    }
    for (_, l) in world.query::<&LightmapComponent>().iter() {
        add(&l.texture_id);
    }
    // A light's cookie is drawn through by the light, so nothing else names
    // it either.
    for (_, l) in world.query::<&crate::ecs::components::PointLight2D>().iter() {
        add(&l.cookie);
    }
    for (_, l) in world.query::<&crate::ecs::components::SpotLight2D>().iter() {
        add(&l.cookie);
    }
    // A rich text's inline icons (`renderer::rich_text`), world and UI.
    for (_, t) in world.query::<&TextComponent>().iter() {
        if t.rich && t.text.contains("[icon=") {
            for icon in crate::renderer::rich_text::parse(&t.text, 1).icons {
                add(&t.text[icon.id]);
            }
        }
    }
    for (_, t) in world.query::<&crate::ui::UIText>().iter() {
        if t.rich && t.text.contains("[icon=") {
            for icon in crate::renderer::rich_text::parse(&t.text, 1).icons {
                add(&t.text[icon.id]);
            }
        }
    }
    // A texture mask is never drawn, only read (`renderer::mask`).
    for (_, m) in world.query::<&crate::ecs::components::SpriteMask>().iter() {
        if let Some(id) = m.shape.texture_id() {
            add(id);
        }
    }
    for (_, e) in world.query::<&ParticleEmitter>().iter() {
        if let Some(id) = e.texture_id.as_deref() {
            add(id);
        }
    }
    // A clip frame can swap the sprite's texture as it plays.
    for (_, a) in world.query::<&SpriteAnimator>().iter() {
        for clip in a.clips.values() {
            for frame in &clip.frames {
                if let Some(id) = frame.texture_id.as_deref() {
                    add(id);
                }
            }
        }
    }
    for (_, t) in world.query::<&Tilemap>().iter() {
        for set in &t.tilesets {
            add(&set.texture_id);
        }
    }
    for (_, i) in world.query::<&UIImage>().iter() {
        add(&i.texture_id);
    }
    // A trail stretches its texture along the ribbon.
    for (_, t) in world.query::<&crate::ecs::components::Trail>().iter() {
        add(&t.texture_id);
    }
    // So does a line (`renderer::line2d`).
    #[cfg(feature = "line2d")]
    for (_, l) in world.query::<&crate::renderer::line2d::Line2D>().iter() {
        add(&l.texture_id);
    }
    // A video's frames are written, not loaded (`video`).
    #[cfg(feature = "video")]
    for id in crate::video::video_texture_ids(world) {
        out.remove(&id);
    }
    // A camera's target is drawn, not loaded (`ecs::views`).
    for id in crate::ecs::views::render_target_ids(world) {
        out.remove(&id);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    /// A trail's texture is fetched like a sprite's, in a scene and in a
    /// prefab - and so is a mesh's in a prefab. Neither collector knew
    /// `trail`, so the showcase's Trails drew no ribbon at all: the texture
    /// was never loaded and a draw with no texture is skipped.
    #[test]
    fn trails_and_meshes_name_textures_to_preload() {
        let mut world = hecs::World::new();
        world.spawn((crate::ecs::components::Trail { texture_id: "fx/ribbon.png".into(), ..Default::default() },));
        assert!(world_texture_ids(&world).contains("fx/ribbon.png"));

        let prefab: EntityData = serde_json::from_value(serde_json::json!({
            "name": "Meteor",
            "components": { "trail": { "texture_id": "fx/streak.png" } }
        }))
        .unwrap();
        assert!(prefab_texture_ids([&prefab]).contains("fx/streak.png"));
    }

    /// An entity-UI icon is fetched like a sprite: nothing else in a scene
    /// has to name its sheet.
    #[test]
    fn a_ui_image_names_a_texture_to_preload() {
        let mut world = hecs::World::new();
        world.spawn((crate::ui::UIElement::default(), UIImage { texture_id: "ui/icons.png".into(), ..Default::default() }));
        world.spawn((SpriteComponent { texture_id: "a.png".into(), ..Default::default() },));
        let ids = world_texture_ids(&world);
        assert!(ids.contains("ui/icons.png"), "{ids:?}");
        assert!(ids.contains("a.png"));

        let prefab: EntityData = serde_json::from_value(serde_json::json!({
            "name": "Icon",
            "components": { "ui_element": {"anchor": "TopLeft", "offset": [0.0, 0.0], "size": [32.0, 32.0]},
                            "ui_image": {"texture_id": "ui/prefab.png"} }
        }))
        .unwrap();
        assert!(prefab_texture_ids([&prefab]).contains("ui/prefab.png"));
    }

    #[test]
    fn scene_roundtrip() {
        let mut world = hecs::World::new();

        // Spawn a few entities with different component combinations
        world.spawn((
            Name("Player".to_string()),
            Transform {
                position: Vec2::new(10.0, 20.0),
                rotation: 1.5,
                scale: Vec2::new(2.0, 3.0),
            },
            SpriteComponent {
                texture_id: "player.png".into(),
                color: [1.0, 0.5, 0.0, 1.0],
                z_order: 5,
                size: Some([32.0, 64.0]),
                uv_rect: None,
                anchor: [0.5, 0.5],
                fill: None,
                sort_layer: None,
                region: None,
                region_resolved: false,
                corners: None,
                ..SpriteComponent::default()
            },
        ));
        world.spawn((
            Name("Background".to_string()),
            Transform::default(),
            SpriteComponent {
                texture_id: "bg.png".into(),
                color: [1.0, 1.0, 1.0, 1.0],
                z_order: -10,
                size: None,
                uv_rect: None,
                anchor: [0.5, 0.5],
                fill: None,
                sort_layer: None,
                region: None,
                region_resolved: false,
                corners: None,
                ..SpriteComponent::default()
            },
        ));
        world.spawn((
            Name("ScriptOnly".to_string()),
            ScriptComponent {
                language: ScriptLanguage::Lua,
                source_path: "scripts/enemy.lua".to_string(),
                cull_distance: None,
            },
        ));

        // Save
        let scene_data = save_scene(&world);
        let json = serde_json::to_string_pretty(&scene_data).unwrap();

        // Verify JSON is valid
        let parsed: SceneData = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.entities.len(), 3);

        // Load into a fresh world
        let mut world2 = hecs::World::new();
        load_scene(&parsed, &mut world2);

        // Verify roundtrip
        let scene_data2 = save_scene(&world2);
        assert_eq!(scene_data2.entities.len(), 3);

        // Find the Player entity and check its data
        let player = scene_data2
            .entities
            .iter()
            .find(|e| e.name == "Player")
            .unwrap();
        let t = player.components.get::<crate::ecs::components::Transform>("transform").unwrap();
        assert!((t.position.x - 10.0).abs() < f32::EPSILON);
        assert!((t.position.y - 20.0).abs() < f32::EPSILON);
        assert!((t.rotation - 1.5).abs() < f32::EPSILON);

        let s = player.components.get::<crate::ecs::components::SpriteComponent>("sprite").unwrap();
        assert_eq!(s.texture_id, "player.png");
        assert_eq!(s.z_order, 5);

        // Find the ScriptOnly entity
        let script_entity = scene_data2
            .entities
            .iter()
            .find(|e| e.name == "ScriptOnly")
            .unwrap();
        assert!(script_entity.components.get::<crate::ecs::components::Transform>("transform").is_none());
        assert!(script_entity.components.get::<crate::ecs::components::SpriteComponent>("sprite").is_none());
        let sc = script_entity.components.get::<crate::ecs::components::ScriptComponent>("script").unwrap();
        assert_eq!(sc.source_path, "scripts/enemy.lua");
    }

    /// The editor reads and saves scenes normalized, so an authored
    /// component reaches the inspector as it did when the bundle was typed -
    /// and custom data and a malformed component pass through untouched.
    #[test]
    fn normalize_fills_in_known_components_and_leaves_the_rest() {
        let mut registry = ComponentRegistry::new();
        crate::package::core_registration::register_core_components(&mut registry);
        let mut bundle: ComponentBundle = serde_json::from_value(serde_json::json!({
            "sprite": { "texture_id": "a.png", "color": [1, 1, 1, 1], "z_order": 2 },
            "collider": { "shape": "not a shape" },
            "Stats": { "hp": 3 }
        }))
        .unwrap();
        bundle.normalize(&registry);
        // Exactly what the typed struct wrote: the component decoded and
        // encoded again (a field its type skips at its default stays out).
        let typed: SpriteComponent = serde_json::from_value(serde_json::json!({ "texture_id": "a.png", "color": [1, 1, 1, 1], "z_order": 2 })).unwrap();
        assert_eq!(bundle.value("sprite").unwrap(), &serde_json::to_value(&typed).unwrap());
        assert_eq!(bundle.value("collider").unwrap(), &serde_json::json!({ "shape": "not a shape" }));
        assert_eq!(bundle.value("Stats").unwrap(), &serde_json::json!({ "hp": 3 }));
        assert_eq!(bundle.check(&registry).len(), 1, "the collider is still reported");
    }

    #[test]
    fn scene_json_format() {
        let scene = SceneData {
            version: 1,
            name: "Test".to_string(),
            post_process: None,
            extra: Default::default(),
            entities: vec![EntityData {
                children: Vec::new(),
                name: "Entity1".to_string(),
                parent: None,
                components: ComponentBundle::new().with("transform", &Transform::default()),
            }],
            sort_layers: None,
        };

        let json = serde_json::to_string_pretty(&scene).unwrap();
        // Verify empty components are omitted (skip_serializing_if)
        assert!(!json.contains("\"sprite\""));
        assert!(!json.contains("\"script\""));
        assert!(json.contains("\"transform\""));
    }

    #[test]
    fn scene_v2_roundtrip() {
        use crate::package::core_registration::register_core_components;
        use crate::package::component_registry::ComponentRegistry;

        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        let mut world = hecs::World::new();

        world.spawn((
            Name("Player".to_string()),
            Transform {
                position: Vec2::new(10.0, 20.0),
                rotation: 1.5,
                scale: Vec2::new(2.0, 3.0),
            },
            SpriteComponent {
                texture_id: "player.png".into(),
                color: [1.0, 0.5, 0.0, 1.0],
                z_order: 5,
                size: Some([32.0, 64.0]),
                uv_rect: None,
                anchor: [0.5, 0.5],
                fill: None,
                sort_layer: None,
                region: None,
                region_resolved: false,
                corners: None,
                ..SpriteComponent::default()
            },
        ));
        world.spawn((
            Name("ScriptOnly".to_string()),
            ScriptComponent {
                language: ScriptLanguage::Lua,
                source_path: "scripts/enemy.lua".to_string(),
                cull_distance: None,
            },
        ));

        // Save via registry
        let scene_v2 = save_scene_v2(&world, &registry);
        assert_eq!(scene_v2.version, 2);
        assert_eq!(scene_v2.entities.len(), 2);

        let json = serde_json::to_string_pretty(&scene_v2).unwrap();
        assert!(json.contains("\"version\": 2"));

        // Load into fresh world via registry
        let parsed: SceneDataV2 = serde_json::from_str(&json).unwrap();
        let mut world2 = hecs::World::new();
        load_scene_v2(&parsed, &mut world2, &registry);

        // Re-save and verify
        let scene_v2_2 = save_scene_v2(&world2, &registry);
        assert_eq!(scene_v2_2.entities.len(), 2);

        let player = scene_v2_2
            .entities
            .iter()
            .find(|e| e.name == "Player")
            .unwrap();
        let t: Transform =
            serde_json::from_value(player.components["transform"].clone()).unwrap();
        assert!((t.position.x - 10.0).abs() < f32::EPSILON);
        assert!((t.rotation - 1.5).abs() < f32::EPSILON);

        let s: SpriteComponent =
            serde_json::from_value(player.components["sprite"].clone()).unwrap();
        assert_eq!(s.texture_id, "player.png");
        assert_eq!(s.z_order, 5);
    }

    #[test]
    fn scene_v1_to_v2_upgrade() {
        use crate::package::core_registration::register_core_components;
        use crate::package::component_registry::ComponentRegistry;

        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        // Create a v1 scene
        let v1 = SceneData {
            version: 1,
            name: "Legacy Level".to_string(),
            post_process: None,
            extra: Default::default(),
            entities: vec![EntityData {
                children: Vec::new(),
                name: "OldEntity".to_string(),
                parent: None,
                components: ComponentBundle::new().with("transform", &Transform {
                        position: Vec2::new(5.0, 10.0),
                        rotation: 0.0,
                        scale: Vec2::ONE,
                    }).with("sprite", &SpriteComponent::default()),
            }],
            sort_layers: None,
        };

        // Upgrade
        let v2 = upgrade_scene_v1_to_v2(&v1, &registry);
        assert_eq!(v2.version, 2);
        assert_eq!(v2.name, "Legacy Level");
        assert_eq!(v2.entities.len(), 1);
        assert!(v2.entities[0].components.contains_key("transform"));
        assert!(v2.entities[0].components.contains_key("sprite"));
    }

    #[test]
    fn scene_load_auto_v1() {
        use crate::package::core_registration::register_core_components;
        use crate::package::component_registry::ComponentRegistry;

        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        // v1 JSON (no "version" field)
        let json = r#"{
            "name": "V1 Scene",
            "entities": [{
                "name": "A",
                "components": {
                    "transform": {"position": [1.0, 2.0], "rotation": 0.0, "scale": [1.0, 1.0]}
                }
            }]
        }"#;

        let mut world = hecs::World::new();
        load_scene_auto(json, &mut world, &registry).unwrap();
        assert_eq!(world.len(), 1);
    }

    #[test]
    fn scene_load_auto_v2() {
        use crate::package::core_registration::register_core_components;
        use crate::package::component_registry::ComponentRegistry;

        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        // v2 JSON (has "version": 2)
        let json = r#"{
            "version": 2,
            "name": "V2 Scene",
            "entities": [{
                "name": "B",
                "components": {
                    "transform": {"position": [3.0, 4.0], "rotation": 0.0, "scale": [1.0, 1.0]}
                }
            }]
        }"#;

        let mut world = hecs::World::new();
        load_scene_auto(json, &mut world, &registry).unwrap();
        assert_eq!(world.len(), 1);

        // Verify the transform was loaded correctly
        for (_, transform) in world.query::<&Transform>().iter() {
            assert!((transform.position.x - 3.0).abs() < f32::EPSILON);
            assert!((transform.position.y - 4.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn a_custom_wrapper_is_detected_as_the_nesting_mistake() {
        // The shape scene 37 shipped with: an object of objects under
        // "custom". `warn_about_nested_custom` is what now says so.
        let mut custom = HashMap::new();
        custom.insert(
            "custom".to_string(),
            serde_json::json!({ "Stats": { "hp": 100 }, "Loot": { "gold": 5 } }),
        );
        // The function only logs, so this asserts the classification it uses:
        // an object of objects under the `custom` key is the mistake.
        let nested = custom.get("custom").unwrap();
        let inner: Vec<&str> = nested
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, v)| v.is_object())
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(inner.len(), 2, "both wrapped components are named");
        warn_about_nested_custom("Box1", &custom);
    }

    #[test]
    fn a_scalar_component_named_custom_is_left_alone() {
        // Someone may legitimately want a component called "custom"; only the
        // object-of-objects shape is the wrapper mistake.
        let mut custom = HashMap::new();
        custom.insert("custom".to_string(), serde_json::json!(42));
        warn_about_nested_custom("E", &custom);

        let mut flat = HashMap::new();
        flat.insert("custom".to_string(), serde_json::json!({ "hp": 1 }));
        // `hp` is a scalar, so this is a real component with real fields.
        warn_about_nested_custom("E", &flat);
    }

    #[test]
    fn correctly_authored_components_do_not_warn() {
        let mut custom = HashMap::new();
        custom.insert("Stats".to_string(), serde_json::json!({ "hp": 100 }));
        warn_about_nested_custom("E", &custom);
    }

    #[test]
    fn the_wrapper_mistake_loads_as_one_component_named_custom() {
        // The behaviour the warning exists to explain, pinned so nobody
        // "fixes" the loader into silently unwrapping it - that would change
        // what an existing scene means.
        let scene = r#"{"name":"p","entities":[{"name":"E","components":{
            "custom": { "Stats": { "hp": 100 } }
        }}]}"#;
        let mut reg = ComponentRegistry::new();
        crate::package::core_registration::register_core_components(&mut reg);
        let mut world = hecs::World::new();
        load_scene_auto(scene, &mut world, &reg).unwrap();
        let mut q = world.query::<&CustomComponents>();
        let (_, c) = q.iter().next().unwrap();
        assert!(c.0.contains_key("custom"));
        assert!(
            !c.0.contains_key("Stats"),
            "the wrapper is not unwrapped - that is why it needs a warning"
        );
    }

    #[test]
    fn scene_v2_custom_components_preserved() {
        use crate::package::core_registration::register_core_components;
        use crate::package::component_registry::ComponentRegistry;

        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        // Create entity with a custom component
        let mut world = hecs::World::new();
        let mut customs = HashMap::new();
        customs.insert(
            "health".to_string(),
            serde_json::json!({"current": 100, "max": 100}),
        );
        world.spawn((
            Name("Player".to_string()),
            Transform::default(),
            CustomComponents(customs),
        ));

        // Save → the custom component should appear in the map
        let v2 = save_scene_v2(&world, &registry);
        let player = &v2.entities[0];
        assert!(player.components.contains_key("transform"));
        assert!(player.components.contains_key("health"));

        // Load back → custom component should go into CustomComponents
        let mut world2 = hecs::World::new();
        load_scene_v2(&v2, &mut world2, &registry);

        for (_, (_, custom)) in world2.query::<(&Transform, &CustomComponents)>().iter() {
            assert!(custom.0.contains_key("health"));
            assert_eq!(custom.0["health"]["current"], 100);
        }
    }

    #[test]
    fn behavior_graph_component_round_trips_through_scene_save_load() {
        // Slice 7.6 — `BehaviorGraphComponent` survives save → JSON →
        // load with `graph_path` + `overrides` intact, and lands on
        // the entity ready for `BehaviorGraphSystem::sync_instances`.
        use serde_json::json;

        let mut world = hecs::World::new();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("speed".to_string(), json!(150.0));
        overrides.insert(
            "target".to_string(),
            json!({ "x": 1.0, "y": -2.0 }),
        );
        world.spawn((
            Name("Hero".to_string()),
            Transform::default(),
            BehaviorGraphComponent {
                graph_path: "graphs/player.behavior".to_string(),
                overrides: overrides.clone(),
            },
        ));

        let scene = save_scene(&world);
        let json_text = serde_json::to_string_pretty(&scene).expect("serialize");
        assert!(
            json_text.contains("behavior_graph"),
            "behavior_graph field must serialize, got:\n{json_text}"
        );
        assert!(
            json_text.contains("graphs/player.behavior"),
            "graph_path must serialize, got:\n{json_text}"
        );

        let parsed: SceneData = serde_json::from_str(&json_text).expect("deserialize");
        let mut loaded = hecs::World::new();
        load_scene(&parsed, &mut loaded);

        let mut found = 0usize;
        for (_e, bg) in loaded.query::<&BehaviorGraphComponent>().iter() {
            assert_eq!(bg.graph_path, "graphs/player.behavior");
            assert_eq!(bg.overrides.len(), 2);
            assert_eq!(bg.overrides.get("speed"), Some(&json!(150.0)));
            assert_eq!(
                bg.overrides.get("target"),
                Some(&json!({ "x": 1.0, "y": -2.0 })),
            );
            found += 1;
        }
        assert_eq!(found, 1, "expected exactly one BehaviorGraphComponent on load");
    }

    #[test]
    fn behavior_graph_component_omitted_when_absent() {
        // Slice 7.6 — entities without a BehaviorGraphComponent must
        // not emit a `behavior_graph: null` field (skip_serializing_if
        // contract; keeps existing `.scene` files diff-clean).
        let mut world = hecs::World::new();
        world.spawn((Name("Plain".to_string()), Transform::default()));

        let scene = save_scene(&world);
        let json_text = serde_json::to_string(&scene).expect("serialize");
        assert!(
            !json_text.contains("behavior_graph"),
            "absent component must not appear in JSON, got:\n{json_text}"
        );
    }

    // ── Sort layers 1.4 — regression tests ──

    #[test]
    fn sort_layer_round_trips_through_sprite_save_load() {
        // SpriteComponent.sort_layer must survive a save → JSON → load
        // cycle and land back on the entity. Guards against accidental
        // omission from save_scene/load_scene path or a stale serde
        // attribute.
        let mut world = hecs::World::new();
        world.spawn((
            Name("Hud".to_string()),
            Transform::default(),
            SpriteComponent {
                texture_id: "hud.png".into(),
                color: [1.0, 1.0, 1.0, 1.0],
                z_order: 3,
                size: None,
                uv_rect: None,
                anchor: [0.5, 0.5],
                fill: None,
                sort_layer: Some("ui".to_string()),
                region: None,
                region_resolved: false,
                corners: None,
                ..SpriteComponent::default()
            },
        ));

        let scene = save_scene(&world);
        let json = serde_json::to_string(&scene).expect("serialize");
        assert!(
            json.contains("\"sort_layer\""),
            "sort_layer must appear in JSON, got: {json}"
        );

        let parsed: SceneData = serde_json::from_str(&json).expect("parse");
        let sprite = parsed.entities[0]
            .components.get::<crate::ecs::components::SpriteComponent>("sprite")
            .expect("sprite missing after load");
        assert_eq!(sprite.sort_layer.as_deref(), Some("ui"));
    }

    #[test]
    fn sprite_without_sort_layer_omits_field_in_json() {
        // `sort_layer: None` must not pollute existing scenes with a
        // `"sort_layer": null` row (skip_serializing_if contract).
        let mut world = hecs::World::new();
        world.spawn((
            Name("Plain".to_string()),
            Transform::default(),
            SpriteComponent {
                texture_id: "p.png".into(),
                color: [1.0, 1.0, 1.0, 1.0],
                z_order: 0,
                size: None,
                uv_rect: None,
                anchor: [0.5, 0.5],
                fill: None,
                sort_layer: None,
                region: None,
                region_resolved: false,
                corners: None,
                ..SpriteComponent::default()
            },
        ));
        let scene = save_scene(&world);
        let json = serde_json::to_string(&scene).expect("serialize");
        assert!(
            !json.contains("sort_layer"),
            "absent sort_layer must not appear in JSON, got: {json}"
        );
    }

    #[test]
    fn scene_sort_layers_field_round_trips() {
        // `SceneData.sort_layers` (the active-set reference) must
        // survive save/load and also be discoverable via the
        // lightweight `read_scene_metadata` helper used by the runtime
        // before deserializing the full scene.
        let scene = SceneData {
            version: 1,
            name: "Level".into(),
            entities: vec![],
            post_process: None,
            sort_layers: Some("sortlayers/main.sortlayers".into()),
            extra: Default::default(),
        };
        let json = serde_json::to_string(&scene).expect("serialize");
        let parsed: SceneData = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed.sort_layers.as_deref(), Some("sortlayers/main.sortlayers"));

        let meta = read_scene_metadata(&json).expect("metadata");
        assert_eq!(meta.sort_layers.as_deref(), Some("sortlayers/main.sortlayers"));
    }

    #[test]
    fn scene_without_sort_layers_omits_field_in_json() {
        // Same diff-clean contract as components.
        let scene = SceneData::default();
        let json = serde_json::to_string(&scene).expect("serialize");
        assert!(
            !json.contains("sort_layers"),
            "absent sort_layers must not appear in JSON, got: {json}"
        );
    }

    #[test]
    fn named_sort_layer_outranks_higher_raw_z_order() {
        // Render-time interleave guarantee: a sprite tagged with a
        // foreground layer (`base_z = 1000`) must sort above a sprite
        // with no layer and a high raw `z_order = 999`. This is the
        // core promise of the sort_layers system — without it scenes
        // would have to rely on fragile magic z numbers.
        use crate::asset::sort_layers::{SortLayer, SortLayers};

        let layers = SortLayers::from_layers(vec![
            SortLayer { name: "bg".into(), base_z: -1000 },
            SortLayer { name: "default".into(), base_z: 0 },
            SortLayer { name: "fg".into(), base_z: 1000 },
        ]);

        // Three sprites; ordered low → high by effective z.
        let untagged_high = SpriteComponent {
            texture_id: "a".into(),
            color: [1.0; 4],
            z_order: 999,
            size: None,
            uv_rect: None,
            anchor: [0.5, 0.5],
            fill: None,
            sort_layer: None,
            region: None,
            region_resolved: false,
            corners: None,
            ..SpriteComponent::default()
        };
        let fg_low = SpriteComponent {
            texture_id: "b".into(),
            color: [1.0; 4],
            z_order: 0,
            size: None,
            uv_rect: None,
            anchor: [0.5, 0.5],
            fill: None,
            sort_layer: Some("fg".into()),
            region: None,
            region_resolved: false,
            corners: None,
            ..SpriteComponent::default()
        };
        let bg_high = SpriteComponent {
            texture_id: "c".into(),
            color: [1.0; 4],
            z_order: 999,
            size: None,
            uv_rect: None,
            anchor: [0.5, 0.5],
            fill: None,
            sort_layer: Some("bg".into()),
            region: None,
            region_resolved: false,
            corners: None,
            ..SpriteComponent::default()
        };

        let mut items = [&untagged_high, &fg_low, &bg_high];
        items.sort_by_key(|s| layers.resolve(s.sort_layer.as_deref(), s.z_order));
        let order: Vec<&str> =
            items.iter().map(|s| s.texture_id.as_str()).collect();
        assert_eq!(
            order,
            vec!["c", "a", "b"],
            "expected bg < untagged < fg (resolved z = -1 / 999 / 1000)"
        );
    }

    #[test]
    fn unknown_sort_layer_falls_back_to_z_order_only() {
        // Defensive: a scene that references a layer name not defined
        // in the active `.sortlayers` (e.g. user renamed it) must not
        // panic and must degrade to the raw `z_order` so the entity is
        // still visible somewhere sensible.
        use crate::asset::sort_layers::{SortLayer, SortLayers};
        let layers = SortLayers::from_layers(vec![SortLayer {
            name: "ui".into(),
            base_z: 5000,
        }]);
        let s = SpriteComponent {
            texture_id: "x".into(),
            color: [1.0; 4],
            z_order: 42,
            size: None,
            uv_rect: None,
            anchor: [0.5, 0.5],
            fill: None,
            sort_layer: Some("typo".into()),
            region: None,
            region_resolved: false,
            corners: None,
            ..SpriteComponent::default()
        };
        assert_eq!(layers.resolve(s.sort_layer.as_deref(), s.z_order), 42);
    }

    #[test]
    fn light_occluder_round_trips_through_scene_save_load() {
        // LightOccluder must survive a save → JSON → load cycle and
        // land back on the entity with its size/enabled intact.
        let mut world = hecs::World::new();
        world.spawn((
            Name("Wall".to_string()),
            Transform::default(),
            LightOccluder {
                enabled: true,
                size: [48.0, 12.0],
                from_sprite: true,
                polygon: vec![[0.0, 0.0], [16.0, 0.0], [8.0, 16.0]],
                shadow_tint: [0.8, 0.1, 0.1],
                shadow_opacity: 0.6,
                self_shadow: true,
                shadow_length: 120.0,
                shadows_casters: false,
                ..Default::default()
            },
        ));

        let scene = save_scene(&world);
        let json = serde_json::to_string(&scene).expect("serialize");
        assert!(
            json.contains("\"light_occluder\""),
            "light_occluder must appear in JSON, got: {json}"
        );

        let parsed: SceneData = serde_json::from_str(&json).expect("parse");
        let occ = parsed.entities[0]
            .components.get::<crate::ecs::components::LightOccluder>("light_occluder")
            .expect("light_occluder missing after load");
        assert!(occ.enabled);
        assert_eq!(occ.size, [48.0, 12.0]);
        assert!(occ.from_sprite);
        assert_eq!(occ.polygon, vec![[0.0, 0.0], [16.0, 0.0], [8.0, 16.0]]);
        assert_eq!(occ.shadow_tint, [0.8, 0.1, 0.1]);
        assert_eq!(occ.shadow_opacity, 0.6);
        assert!(occ.self_shadow, "self_shadow must survive a round trip");
        assert_eq!(occ.shadow_length, 120.0, "shadow_length must survive a round trip");
    }
}
