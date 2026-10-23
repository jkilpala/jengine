//! Tile data (Oct 2026): a tileset can give its tiles custom fields
//! (`data`, which Tiled's tile properties import as), draw a tile as one of
//! several weighted variants picked by the cell's place (`random`), and turn
//! a tile into a prefab instance when its scene loads (`spawns`) - expanded
//! by `expand_prefab_instances_in_json`, which every runtime runs on a scene
//! before loading it.

use std::collections::HashMap;

use jengine_core::scene::expand_prefab_instances_in_json;
use jengine_core::scene::format::EntityData;
use jengine_core::tilemap::{TileRandom, Tilemap, TilesetDef, tiled::parse_tiled_json};

fn tileset() -> TilesetDef {
    let mut ts = TilesetDef {
        texture_id: "isle.png".into(),
        tile_width: 32,
        tile_height: 32,
        columns: 8,
        tile_count: 32,
        first_gid: 1,
        image_width: 256,
        image_height: 128,
        ..Default::default()
    };
    ts.data.insert("4".into(), serde_json::json!({ "kind": "lava", "damage": 5 }));
    ts.random.push(TileRandom { tile: 0, variants: vec![(0, 6.0), (1, 2.0), (2, 1.0), (3, 0.0)] });
    ts
}

#[test]
fn a_tile_carries_its_data_by_the_tile_a_cell_names() {
    let map = Tilemap { tilesets: vec![tileset()], ..Default::default() };
    assert_eq!(map.tile_data(5).unwrap()["kind"], "lava", "gid 5 is local tile 4");
    assert_eq!(map.tile_data(5).unwrap()["damage"], 5);
    assert!(map.tile_data(1).is_none(), "a tile with none");
}

#[test]
fn tiled_tile_properties_import_as_data() {
    let tmj = r#"{ "width": 2, "height": 1, "tilewidth": 32, "tileheight": 32, "orientation": "orthogonal",
      "layers": [ { "type": "tilelayer", "name": "g", "width": 2, "height": 1, "data": [1, 3] } ],
      "tilesets": [ { "firstgid": 1, "image": "isle.png", "tilewidth": 32, "tileheight": 32, "columns": 4,
                      "tilecount": 8, "imagewidth": 128, "imageheight": 64,
                      "tiles": [ { "id": 2, "properties": [ { "name": "kind", "type": "string", "value": "chest" },
                                                          { "name": "gold", "type": "int", "value": 30 } ] } ] } ] }"#;
    let map = parse_tiled_json(tmj).expect("imports");
    assert_eq!(map.tile_data(3).unwrap(), &serde_json::json!({ "kind": "chest", "gold": 30 }));
}

#[test]
fn random_tiles_vary_by_place_by_weight_and_never_change() {
    let ts = tileset();
    let mut counts = [0usize; 4];
    for row in 0..60 {
        for col in 0..60 {
            let v = ts.variant_at(0, col, row);
            counts[v as usize] += 1;
            assert_eq!(v, ts.variant_at(0, col, row), "a cell always draws the same variant");
        }
    }
    // 3600 cells weighted 6 : 2 : 1 : 0.
    assert_eq!(counts[3], 0, "a weight of 0 is never drawn");
    let share = |n: usize| n as f32 / 3600.0;
    assert!((share(counts[0]) - 6.0 / 9.0).abs() < 0.05, "{counts:?}");
    assert!((share(counts[1]) - 2.0 / 9.0).abs() < 0.05, "{counts:?}");
    assert!((share(counts[2]) - 1.0 / 9.0).abs() < 0.04, "{counts:?}");
    assert_eq!(ts.variant_at(7, 3, 3), 7, "a tile with no variants draws as itself");
    assert_eq!(ts.drawn(4, 0.0, 1, 1), 4);
}

#[test]
fn a_spawning_tile_loads_as_its_prefab() {
    let scene = r#"{ "name": "isle", "entities": [
      { "name": "Map", "components": {
        "transform": { "position": [0, 0], "rotation": 0, "scale": [1, 1] },
        "tilemap": { "tile_size": [32, 32],
          "tilesets": [ { "texture_id": "t.png", "tile_width": 32, "tile_height": 32, "columns": 4, "tile_count": 8,
                          "first_gid": 1, "image_width": 128, "image_height": 64,
                          "spawns": [ { "tile": 5, "prefab": "Chest" } ] } ],
          "layers": [ { "name": "ground", "width": 2, "height": 1, "tiles": [1, 6] } ] } } } ] }"#;
    let chest: EntityData = serde_json::from_str(
        r#"{ "name": "Chest", "components": {
             "transform": { "position": [0, 0], "rotation": 0, "scale": [1, 1] },
             "sprite": { "texture_id": "chest.png", "color": [1, 1, 1, 1], "z_order": 4, "size": [28, 24] } } }"#,
    )
    .unwrap();
    let prefabs: HashMap<String, EntityData> = HashMap::from([("Chest".to_string(), chest)]);
    let out: serde_json::Value = serde_json::from_str(&expand_prefab_instances_in_json(scene, &prefabs, "test")).unwrap();
    let ents = out["entities"].as_array().unwrap();
    let spawned = ents.iter().find(|e| e["name"].as_str().unwrap().starts_with("Map/Chest")).expect("the chest");
    assert_eq!(spawned["components"]["sprite"]["texture_id"], "chest.png", "expanded from its prefab");
    assert_eq!(spawned["components"]["transform"]["position"], serde_json::json!([48.0, -16.0]), "at its cell's centre");
    let map: Tilemap = serde_json::from_value(ents[0]["components"]["tilemap"].clone()).unwrap();
    assert_eq!(map.layers[0].get_tile(1, 0), None, "its cell emptied");
    assert_eq!(map.layers[0].get_tile(0, 0), Some(1), "and only its cell");
}
