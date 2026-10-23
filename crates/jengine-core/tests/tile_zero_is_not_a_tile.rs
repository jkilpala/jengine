//! A tile id of `0` is empty, to everything that asks.
//!
//! It was empty to the renderer and solid to the physics: the renderer
//! resolves gid 0 against `first_gid` (always 1 or more) and draws
//! nothing, while `sync_tilemap_colliders` asked `get_tile(..).is_some()`
//! and got `true`. A level written with 0s for its empty cells drew as
//! open air and collided as solid rock - an invisible wall over the whole
//! map, which reads as a physics bug and is a data one.
//!
//! Found by a player of a run-and-gun platformer: "you can jump up, and
//! when you get on top of something you cannot run forward". They were
//! inside the level. BUGS #141.

use jengine_core::tilemap::{Tilemap, TilemapLayer, TilesetDef, script_tile_id};

fn layer(tiles: Vec<u32>) -> TilemapLayer {
    TilemapLayer {
        name: "ground".into(),
        tiles,
        width: 4,
        height: 1,
        visible: true,
        collision: false,
        ..Default::default()
    }
}

#[test]
fn zero_reads_as_empty() {
    // `Some(0)` is what a hand-written map, a generator, or a script means
    // by "nothing here" - every tilemap format in the world spells empty
    // that way, which is why `first_gid` starts at 1.
    let l = layer(vec![0, 0, 1, 7]);
    assert_eq!(l.get_tile(0, 0), None, "a zero is not a tile");
    assert_eq!(l.get_tile(1, 0), None);
    assert_eq!(l.get_tile(2, 0), Some(1), "a real gid still is one");
    assert_eq!(l.get_tile(3, 0), Some(7));
}

#[test]
fn writing_zero_clears_the_cell_rather_than_storing_a_phantom() {
    // A script's `set_tile(e, layer, x, y, 0)` means "clear it". Keeping
    // a `Some(0)` would save a map that claims a tile is there.
    let mut l = layer(vec![3; 4]);
    l.set_tile(0, 0, Some(0));
    l.set_tile(1, 0, None);
    assert_eq!(l.tiles[0], 0, "the stored value, not just what is read back");
    assert_eq!(l.tiles[1], 0);
    assert_eq!(l.get_tile(0, 0), None);
    assert_eq!(l.get_tile(2, 0), Some(3), "and it touched nothing else");
}

#[test]
fn an_empty_cell_is_empty_to_both_halves() {
    // The bug in one assertion. An EMPTY cell - `None` or a zero - must
    // be skipped by the renderer and left out by the collider alike.
    // (A gid that names a tile the tileset has no art for is a different
    // thing, and deliberately still solid; see the next test.)
    let tilemap = Tilemap {
        tile_size: [32.0, 32.0],
        layers: vec![layer(vec![0, 0, 1, 99])],
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
    };
    let l = &tilemap.layers[0];
    for (x, what) in [(0u32, "a zero"), (1, "a None")] {
        // `is_some()` is exactly the collider's own rule.
        assert!(!l.get_tile(x, 0).is_some(), "{what} must not be solid");
        assert!(
            l.get_tile(x, 0).and_then(|gid| tilemap.resolve_gid(gid)).is_none(),
            "{what} must not draw either"
        );
    }
    assert_eq!(l.get_tile(2, 0), Some(1), "and a real tile is untouched");
}

#[test]
fn collision_without_art_is_still_allowed() {
    // The other half of the pair, recorded rather than silently changed:
    // a gid the tileset has no art for is a tile the map deliberately
    // asked for, and a level may legitimately want collision with
    // nothing drawn. Only ZERO means "nothing here".
    let tilemap = Tilemap {
        tile_size: [32.0, 32.0],
        layers: vec![layer(vec![99, 0, 0, 0])],
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
    };
    assert_eq!(tilemap.layers[0].get_tile(0, 0), Some(99), "still a tile");
    assert!(tilemap.resolve_gid(99).is_none(), "it just has no art here");
}

#[test]
fn a_map_of_zeroes_is_an_empty_map() {
    // The shape the arcade's level generator actually wrote: a full grid
    // of 0s with a few real tiles in it. Before the fix, every one of
    // those 0s was a collider.
    let mut tiles = vec![0u32; 4];
    tiles[2] = 1;
    let l = layer(tiles);
    let solid: Vec<u32> = (0..4).filter(|x| l.get_tile(*x, 0).is_some()).collect();
    assert_eq!(solid, vec![2], "only the real tile is solid");
}

#[test]
fn every_way_a_script_spells_empty_means_empty() {
    // Three spellings reach the runtimes, and each host used to accept a
    // different subset. `nil` is what the docs promise; a negative is
    // what native and the QuickJS host took, because their bindings were
    // plain `i32`; and `0` is what every tilemap format on earth means
    // by empty.
    assert_eq!(script_tile_id(None), None, "nil / null / absent");
    assert_eq!(script_tile_id(Some(-1.0)), None, "a negative");
    assert_eq!(script_tile_id(Some(0.0)), None, "a zero");
    assert_eq!(script_tile_id(Some(f64::NAN)), None, "and a nonsense number");
    assert_eq!(script_tile_id(Some(1.0)), Some(1), "gid 1 is the first real tile");
    assert_eq!(script_tile_id(Some(17.0)), Some(17));
    // A fractional gid is somebody's arithmetic, not a request for tile
    // 4.7. Truncating is the only answer that cannot surprise.
    assert_eq!(script_tile_id(Some(4.7)), Some(4));
}

#[test]
fn a_huge_negative_does_not_wrap_round_into_a_real_tile() {
    // The browser hosts did `as u32` on whatever came in, so the `-1`
    // that every other runtime reads as "clear it" arrived as a gid of
    // 4,294,967,295 - a tile the tileset has no art for, and therefore
    // one that draws as nothing and **collides as rock**. Exactly the
    // shape of BUGS #141, arriving by a different door.
    assert_eq!(script_tile_id(Some(-1.0)), None);
    assert_eq!(script_tile_id(Some(-999_999.0)), None);
}
