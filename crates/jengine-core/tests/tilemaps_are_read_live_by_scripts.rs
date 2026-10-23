//! A script reads a tilemap as it is *now*, straight from the world.
//!
//! `get_tile` and friends used to answer from a copy: every frame the script
//! system deep-cloned every `Tilemap` in the world into the bridge snapshot
//! (and, separately, into a legacy `ScriptState` snapshot nothing read). A
//! builder game's million tiles were memcpy'd twice a frame so that a script
//! could look at one of them. The accessors now read through `world_ptr`
//! like every other getter, so this test is the one that would notice if a
//! frame stopped lending the world - the answers would come back `-1`.
//!
//! The script writes what it read into a custom component, which is how the
//! assertion sees it. It also edits a tile and reads the edit back the next
//! frame: `set_tile` is a command applied after the frame, and a copy taken
//! before the apply would keep answering the old value.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;
use jengine_core::tilemap::{Tilemap, TilemapLayer, TilesetDef};

const SRC: &str = r#"
local frame = 0
function update(entity, dt)
    frame = frame + 1
    if frame == 1 then
        set_custom(entity, "Read", "at_2_0", get_tile(entity, "ground", 2, 0))
        set_custom(entity, "Read", "at_0_0", get_tile(entity, "ground", 0, 0))
        set_custom(entity, "Read", "outside", get_tile(entity, "ground", 9, 9))
        local tx, ty = world_to_tile(entity, 100 + 40, 100 - 8)
        set_custom(entity, "Read", "tx", tx)
        set_custom(entity, "Read", "ty", ty)
        set_tile(entity, "ground", 1, 0, 5)
    elseif frame == 2 then
        set_custom(entity, "Read", "edited", get_tile(entity, "ground", 1, 0))
    end
end
"#;

fn map() -> Tilemap {
    Tilemap {
        tile_size: [32.0, 32.0],
        layers: vec![TilemapLayer {
            name: "ground".into(),
            tiles: vec![0, 0, 3, 7],
            width: 4,
            height: 1,
            visible: true,
            collision: false,
            ..Default::default()
        }],
        tilesets: vec![TilesetDef {
            texture_id: "tiles.png".into(),
            tile_width: 32,
            tile_height: 32,
            columns: 2,
            tile_count: 4,
            first_gid: 1,
            image_width: 64,
            image_height: 64,
            ..Default::default()
        }],
        source: None,
        z_order: 0,
        sort_layer: None,
        ..Default::default()
    }
}

#[test]
fn a_script_reads_and_edits_the_tilemap_in_the_world() {
    let dir = std::env::temp_dir().join(format!(
        "jengine_live_tilemap_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("reader.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let e = world.spawn((
        Name("Map".into()),
        Transform { position: glam::Vec2::new(100.0, 100.0), ..Default::default() },
        map(),
        CustomComponents::default(),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_string_lossy().into(),
            cull_distance: None,
        },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);

    let custom = world.get::<&CustomComponents>(e).unwrap();
    let read = custom.0.get("Read").expect("the script wrote its readings");
    let n = |k: &str| read.get(k).and_then(|v| v.as_f64()).unwrap_or(f64::NAN);
    assert_eq!(n("at_2_0"), 3.0, "a tile read live from the world: {read}");
    assert_eq!(n("at_0_0"), 0.0, "an empty cell reads as 0");
    assert_eq!(n("outside"), -1.0, "out of bounds reads as -1");
    // The map hangs from its transform at (100, 100) and rows run down, so
    // world (140, 92) is tile (1, 0).
    assert_eq!((n("tx"), n("ty")), (1.0, 0.0), "world_to_tile uses the entity's live position");
    assert_eq!(n("edited"), 5.0, "the edit made last frame is what this frame reads");
    assert_eq!(
        world.get::<&Tilemap>(e).unwrap().layers[0].get_tile(1, 0),
        Some(5),
        "and the world holds the edit, not a copy of it"
    );
}
