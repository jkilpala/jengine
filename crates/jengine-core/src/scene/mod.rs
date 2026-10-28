//! Scene serialization — save and load scenes as JSON.

pub mod format;
pub mod manager;
pub mod prefab;
pub mod sources;
/// Scenes loaded in the background, and laid into the running one.
pub mod streaming;

pub use format::*;
pub use manager::*;
pub use prefab::{
    add_prefabs, expand_prefab_instances, expand_prefab_instances_in_json, expand_prefab_instances_in_value, parse_prefab,
    resolve_prefab_variants, ExpandReport,
};

thread_local! {
    static CURRENT_NAME: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Record the running scene's name - what `Scene.get_name()` answers. Every
/// host calls this where it replaces its world with a scene (`name_of` reads
/// it from the scene's JSON). Nothing recorded one until Oct 2026, so the
/// getter answered "" in every runtime.
pub fn set_current_name(name: &str) {
    CURRENT_NAME.with(|n| {
        let mut n = n.borrow_mut();
        n.clear();
        n.push_str(name);
    });
}

/// The running scene's name, as [`set_current_name`] left it.
pub fn current_name() -> String {
    CURRENT_NAME.with(|n| n.borrow().clone())
}

/// Parse scene data - an `EntityData`, a `SceneData`, anything holding a
/// `ComponentBundle` - from JSON text, by way of a `serde_json::Value`.
///
/// Never `serde_json::from_str` such a type directly. Serde's code is
/// generated per reader, and the scene loaders already read a `Value`, so a
/// type parsed straight from text compiles a second deserializer for every
/// component the engine has: one `from_str::<Vec<EntityData>>` in the
/// browser hosts cost the web module 616 KB (238 KB gzipped) until Oct 2026.
/// `docs/build-sizes.md`; `scene_types_are_parsed_through_a_value` is the
/// guard.
pub fn from_json_text<T: serde::de::DeserializeOwned>(text: &str) -> serde_json::Result<T> {
    serde_json::from_value(serde_json::from_str(text)?)
}

/// A scene's `"name"`, read from its JSON without parsing the rest of it into
/// a world.
pub fn name_of(json: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Named {
        name: Option<String>,
    }
    serde_json::from_str::<Named>(json).ok()?.name
}

/// [`name_of`] for a scene already parsed.
pub fn name_in(scene: &serde_json::Value) -> Option<String> {
    scene.get("name").and_then(|n| n.as_str()).map(str::to_string)
}
