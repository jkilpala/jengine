//! Scene management Tauri commands.

use jengine_core::scene::format::{ComponentBundle, EntityData, SceneData};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[tauri::command]
pub fn create_new_scene() -> SceneData {
    SceneData {
        version: 1,
        name: "Untitled Scene".to_string(),
        entities: Vec::new(),
        post_process: None,
        sort_layers: None,
        extra: Default::default(),
    }
}

/// Loaded and saved with every known component's fields filled in
/// (`SceneData::normalize`) - what the inspector reads, and what a save has
/// always written, since the editor read scenes through a typed struct until
/// Oct 2026.
#[tauri::command]
pub fn load_scene(path: String) -> Result<SceneData, String> {
    let contents = std::fs::read_to_string(&path).map_err(|e| format!("Failed to read file: {e}"))?;
    let mut scene: SceneData =
        serde_json::from_str(&contents).map_err(|e| format!("Invalid scene JSON: {e}"))?;
    jengine_core::package::component_registry::with_registry(|r| scene.normalize(r));
    Ok(scene)
}

#[tauri::command]
pub fn save_scene(path: String, mut scene: SceneData) -> Result<(), String> {
    jengine_core::package::component_registry::with_registry(|r| scene.normalize(r));
    let json =
        serde_json::to_string_pretty(&scene).map_err(|e| format!("Failed to serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write file: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn add_entity(mut scene: SceneData, name: String) -> SceneData {
    scene.entities.push(EntityData {
        children: Vec::new(),
        name,
        parent: None,
        components: ComponentBundle::default(),
    });
    scene
}

#[tauri::command]
pub fn remove_entity(mut scene: SceneData, index: usize) -> SceneData {
    if index < scene.entities.len() {
        scene.entities.remove(index);
    }
    scene
}

#[tauri::command]
pub fn update_entity_name(mut scene: SceneData, index: usize, name: String) -> SceneData {
    if let Some(entity) = scene.entities.get_mut(index) {
        entity.name = name;
    }
    scene
}

/// A script source loaded from disk, ready for the viewport WASM play mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptSourceEntry {
    pub entity_index: usize,
    pub entity_name: String,
    pub source: String,
    pub language: String,
    pub source_path: String,
}

/// Result of loading script sources — includes entries that loaded and warnings for failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptLoadResult {
    pub entries: Vec<ScriptSourceEntry>,
    pub warnings: Vec<String>,
}

/// Load all script sources referenced in the scene from the project's assets directory.
/// Missing or unreadable scripts produce warnings but don't fail the entire operation.
#[tauri::command]
pub fn load_script_sources(
    project_path: String,
    scene: SceneData,
) -> ScriptLoadResult {
    let assets_dir = PathBuf::from(&project_path).join("assets");
    let mut entries = Vec::new();
    let mut warnings = Vec::new();

    for (i, entity) in scene.entities.iter().enumerate() {
        if let Some(script) = entity.components.get::<jengine_core::ecs::components::ScriptComponent>("script") {
            let script_path = assets_dir.join(&script.source_path);
            match std::fs::read_to_string(&script_path) {
                Ok(source) => {
                    let language = match script.language {
                        jengine_core::ecs::components::ScriptLanguage::Lua => "Lua",
                        jengine_core::ecs::components::ScriptLanguage::JavaScript => "JavaScript",
                    };
                    entries.push(ScriptSourceEntry {
                        entity_index: i,
                        entity_name: entity.name.clone(),
                        source,
                        language: language.to_string(),
                        source_path: script.source_path.clone(),
                    });
                }
                Err(e) => {
                    warnings.push(format!(
                        "Script '{}' for entity '{}': {}",
                        script.source_path, entity.name, e
                    ));
                }
            }
        }
    }

    ScriptLoadResult { entries, warnings }
}
