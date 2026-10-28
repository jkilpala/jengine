//! Tiles that are things (Oct 2026; Godot's scene tiles, Unity's GameObject
//! brush): a tileset's [`TileSpawn`](super::TileSpawn)s turn every cell
//! naming a tile into an instance of a prefab at the cell's centre - a chest,
//! a torch, an enemy painted with the tilemap brush - and empty the cell
//! unless the spawn `keep`s it.
//!
//! Done **when a scene loads**, as a scene's `prefab_instance`s are: every
//! runtime runs [`crate::scene::expand_prefab_instances_in_json`] on a scene
//! before loading it, and that calls [`expand_scene_tiles`] first, so the
//! spawned things are ordinary prefab instances by the time anything reads
//! the scene and no host was wired. A tile painted later with `set_tile`
//! stays a tile. The cell's centre is the map's own transform plus the
//! layer's offset - a map hung under a parent places them as if it were not.

use crate::scene::{EntityData, SceneData};

/// Add a prefab instance for every spawning cell of every tilemap in
/// `scene`, and empty the cells that do not `keep` their tile. The number of
/// things spawned.
pub fn expand_scene_tiles(scene: &mut SceneData) -> usize {
    let mut added: Vec<EntityData> = Vec::new();
    for entity in scene.entities.iter_mut() {
        // Asked of the JSON first: a big map is decoded only if one of its
        // tilesets spawns anything.
        if !spawns_anything(entity.components.value("tilemap")) {
            continue;
        }
        let Some(mut map) = entity.components.get::<crate::tilemap::Tilemap>("tilemap") else { continue };
        let origin = entity
            .components
            .get::<crate::ecs::components::Transform>("transform")
            .map(|t| t.position)
            .unwrap_or_default();
        let mut emptied = false;
        let mut cells: Vec<(usize, u32, u32, String, bool)> = Vec::new();
        for (li, layer) in map.layers.iter().enumerate() {
            for row in 0..layer.height {
                for col in 0..layer.width {
                    let Some(gid) = layer.get_tile(col, row) else { continue };
                    let Some((ts, local)) = map.resolve_gid(gid) else { continue };
                    if let Some(spawn) = map.tilesets[ts].spawns.iter().find(|s| s.tile == local) {
                        cells.push((li, col, row, spawn.prefab.clone(), spawn.keep));
                    }
                }
            }
        }
        for (li, col, row, prefab, keep) in cells {
            let offset = map.layers[li].offset;
            let (x, y) = map.grid_to_local(col as f32 + 0.5, row as f32 + 0.5);
            let pos = [origin.x + offset[0] + x, origin.y + offset[1] + y];
            let mut overrides = std::collections::HashMap::new();
            overrides.insert("transform.position".to_string(), serde_json::json!(pos));
            let mut data = EntityData {
                children: Vec::new(),
                name: format!("{}/{prefab}@{col},{row}", entity.name),
                parent: None,
                components: Default::default(),
            };
            data.components
                .set("prefab_instance", &crate::ecs::components::PrefabInstance { prefab_id: prefab, overrides });
            added.push(data);
            if !keep {
                map.layers[li].set_tile(col, row, None);
                emptied = true;
            }
        }
        if emptied {
            entity.components.set("tilemap", &map);
        }
    }
    let n = added.len();
    scene.entities.extend(added);
    n
}

/// Whether a tilemap's JSON has a tileset with a non-empty `spawns`.
fn spawns_anything(tilemap: Option<&serde_json::Value>) -> bool {
    tilemap
        .and_then(|m| m.get("tilesets"))
        .and_then(|t| t.as_array())
        .is_some_and(|sets| sets.iter().any(|s| s.get("spawns").and_then(|v| v.as_array()).is_some_and(|a| !a.is_empty())))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE: &str = r#"{ "name": "isle", "entities": [
      { "name": "Map", "components": {
        "transform": { "position": [100, 200], "rotation": 0, "scale": [1, 1] },
        "tilemap": { "tile_size": [32, 32],
          "tilesets": [ { "texture_id": "t.png", "tile_width": 32, "tile_height": 32, "columns": 4, "tile_count": 8,
                          "first_gid": 1, "image_width": 128, "image_height": 64,
                          "spawns": [ { "tile": 5, "prefab": "Chest" }, { "tile": 6, "prefab": "Torch", "keep": true } ] } ],
          "layers": [ { "name": "ground", "width": 3, "height": 2, "tiles": [1, 6, 1, 1, 7, 1] } ] } } } ] }"#;

    #[test]
    fn a_spawning_tile_becomes_a_prefab_instance_at_its_cell() {
        let mut scene: SceneData = serde_json::from_str(SCENE).unwrap();
        assert_eq!(expand_scene_tiles(&mut scene), 2);
        let chest = scene.entities.iter().find(|e| e.name.starts_with("Map/Chest")).expect("a chest");
        let inst = chest.components.get::<crate::ecs::components::PrefabInstance>("prefab_instance").unwrap();
        assert_eq!(inst.prefab_id, "Chest");
        // Cell (1, 0): its centre is 48 right of the map's corner and 16 down.
        assert_eq!(inst.overrides["transform.position"], serde_json::json!([148.0, 184.0]));
        let torch = scene.entities.iter().find(|e| e.name.starts_with("Map/Torch")).expect("a torch");
        assert_eq!(torch.components.get::<crate::ecs::components::PrefabInstance>("prefab_instance").unwrap().overrides["transform.position"],
                   serde_json::json!([148.0, 152.0]));
        let layer = &scene.entities[0].components.get::<crate::tilemap::Tilemap>("tilemap").unwrap().layers[0];
        assert_eq!(layer.get_tile(1, 0), None, "the chest's cell is emptied");
        assert_eq!(layer.get_tile(1, 1), Some(7), "a kept tile stays");
        assert_eq!(expand_scene_tiles(&mut scene), 1, "only the kept tile is left to spawn");
    }
}
