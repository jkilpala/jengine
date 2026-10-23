//! **Wave function collapse** (Oct 2026; `ecs::wfc`): a `wfc` generator,
//! authored as a scene author writes it, fills the regions a script asks for
//! with tiles whose touching edges agree - each new region joining the
//! tiles already beside it. Found by the showcase's The Endless Dungeon.
//! The algorithm is tested in the module.

use hecs::World;
use jengine_core::ecs::wfc::{WfcGenerator, queue_region, tick_wfc};
use jengine_core::tilemap::{Tilemap, TilemapLayer};

/// A generator as a scene authors it: pipes, gid 1 + the open sides.
fn generator() -> WfcGenerator {
    let tiles: Vec<serde_json::Value> = (0..16)
        .map(|b: u32| {
            serde_json::json!({
                "gid": 1 + b,
                "edges": (0..4).map(|d| if b & (1 << d) != 0 { "o" } else { "x" }).collect::<Vec<_>>(),
                "weight": if b == 0 { 0.1 } else { 1.0 },
            })
        })
        .collect();
    serde_json::from_value(serde_json::json!({
        "layer": "ground", "tiles": tiles, "cells_per_second": 0.0, "seed": 5, "outside": "x"
    }))
    .expect("a wfc component deserializes")
}

fn open(gid: u32, side: u32) -> bool {
    (gid - 1) & (1 << side) != 0
}

#[test]
fn a_long_map_is_filled_stretch_by_stretch_and_every_seam_fits() {
    let (w, h) = (48u32, 9u32);
    let mut world = World::new();
    let e = world.spawn((
        generator(),
        Tilemap {
            layers: vec![TilemapLayer {
                name: "ground".into(),
                width: w,
                height: h,
                tiles: vec![0; (w * h) as usize],
                ..Default::default()
            }],
            ..Default::default()
        },
    ));
    // Eight columns at a time, as the showcase asks for them.
    for k in 0..6 {
        queue_region(&world, e, k * 8, 0, 8, h as i32).unwrap();
        tick_wfc(&mut world, 0.016);
    }
    let g = world.get::<&WfcGenerator>(e).unwrap();
    assert_eq!((g.state.finished, g.state.failed), (6, 0));
    let map = world.get::<&Tilemap>(e).unwrap();
    let l = map.find_layer("ground").unwrap();
    for y in 0..h {
        for x in 0..w {
            let t = l.get_tile(x, y).expect("every cell decided");
            if x + 1 < w {
                assert_eq!(open(t, 1), open(l.get_tile(x + 1, y).unwrap(), 3), "({x}, {y}) and its right");
            } else {
                assert!(!open(t, 1), "nothing opens off the right edge");
            }
            if y + 1 < h {
                assert_eq!(open(t, 2), open(l.get_tile(x, y + 1).unwrap(), 0), "({x}, {y}) and below");
            }
        }
    }
}
