//! **Autotiling** (Oct 2026): a tilemap layer painted in *terrains* - grass,
//! sand, water - that draws its own edges and corners, as Godot's terrains
//! and Unity's rule tiles do.
//!
//! An `autotile` component on a tilemap's entity names the layers it applies
//! to, and for each the terrains in order of priority, lowest first. Such a
//! layer's cells hold **terrain numbers** - 1 for the first terrain, 2 for
//! the next, 0 for none - so painting is `Tilemap.set_tile(map, layer, x, y,
//! 2)`, and a `tilemap_collider` on the layer collides with every painted
//! cell, as it would with tiles.
//!
//! It is drawn on a **dual grid**: a grid offset half a tile, each of whose
//! cells sits on a corner shared by four painted cells and picks its tile
//! from which of those four are a terrain - sixteen tiles a terrain, one per
//! corner pattern ([`corner_mask`]), the one with no corner never drawn. The
//! terrains stack: a higher one covers a lower one's corners, so grass over
//! sand over water is three passes of the same rule, and any terrain can
//! meet any other with no tiles drawn for the pair. Nothing is stored or
//! ticked - the renderer works it out from the cells it can see - so it
//! draws in the editor while editing, and an edit shows the frame it lands.
//!
//! A terrain's sixteen tiles are consecutive gids from its `first_gid`, in
//! [`corner_mask`] order: bit 1 the top-left corner, 2 top-right, 4
//! bottom-left, 8 bottom-right. Put one layer's terrains in one tileset, so
//! they draw in priority order.
//!
//! An **isometric** map autotiles the same way: the dual cell is a diamond
//! centred on the vertex four diamonds share, and its corners are the top
//! (bit 1, cell `(dx - 1, dy - 1)`), right (2), left (4) and bottom (8)
//! vertices - the same cells, turned.

use serde::{Deserialize, Serialize};

use super::TilemapLayer;

/// One autotiled layer: its name, and each terrain's first gid.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AutotileLayer {
    /// The layer of the entity's `tilemap` painted in terrain numbers.
    pub layer: String,
    /// Terrain `n`'s sixteen tiles start at `terrains[n - 1]`, lowest
    /// priority first.
    pub terrains: Vec<u32>,
}

/// The `autotile` component: which layers of the entity's tilemap are
/// painted in terrains, and how each draws.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Autotile {
    pub layers: Vec<AutotileLayer>,
}

impl Autotile {
    /// The rule for the layer named `name`, if it is autotiled.
    pub fn for_layer(&self, name: &str) -> Option<&AutotileLayer> {
        self.layers.iter().find(|l| l.layer == name)
    }
}

/// The terrain number painted at `(x, y)`, 0 off the map or where none is.
fn terrain_at(layer: &TilemapLayer, x: i64, y: i64) -> u32 {
    if x < 0 || y < 0 {
        return 0;
    }
    layer.get_tile(x as u32, y as u32).unwrap_or(0)
}

/// Which corners of dual cell `(dx, dy)` are terrain `t` or above: bit 1 its
/// top-left (painted cell `(dx - 1, dy - 1)`), 2 top-right, 4 bottom-left,
/// 8 bottom-right. Rows run down, as a tilemap's do.
pub fn corner_mask(layer: &TilemapLayer, dx: i64, dy: i64, t: u32) -> u32 {
    let on = |x: i64, y: i64| u32::from(t > 0 && terrain_at(layer, x, y) >= t);
    on(dx - 1, dy - 1) | on(dx, dy - 1) << 1 | on(dx - 1, dy) << 2 | on(dx, dy) << 3
}

/// The tiles of dual cell `(dx, dy)`, bottom terrain first: the gids to draw
/// there, each centred on the corner the cell sits on.
pub fn dual_cell_gids<'a>(layer: &'a TilemapLayer, rule: &'a AutotileLayer, dx: i64, dy: i64) -> impl Iterator<Item = u32> + 'a {
    rule.terrains.iter().enumerate().filter_map(move |(i, &first)| {
        let mask = corner_mask(layer, dx, dy, i as u32 + 1);
        (mask > 0).then_some(first + mask)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(rows: &[&str]) -> TilemapLayer {
        let h = rows.len() as u32;
        let w = rows[0].len() as u32;
        let tiles = rows.iter().flat_map(|r| r.bytes().map(|b| (b - b'0') as u32)).collect();
        TilemapLayer {
            name: "ground".into(),
            tiles,
            width: w,
            height: h,
            visible: true,
            collision: false,
            ..Default::default()
        }
    }

    #[test]
    fn a_lone_cell_has_four_corners_around_it() {
        let l = layer(&["000", "010", "000"]);
        // The four dual cells on the painted cell's corners.
        assert_eq!(corner_mask(&l, 1, 1, 1), 8, "its top-left corner: only the bottom-right is painted");
        assert_eq!(corner_mask(&l, 2, 1, 1), 4);
        assert_eq!(corner_mask(&l, 1, 2, 1), 2);
        assert_eq!(corner_mask(&l, 2, 2, 1), 1);
        assert_eq!(corner_mask(&l, 0, 0, 1), 0, "away from it, nothing");
    }

    #[test]
    fn a_filled_area_is_whole_inside_and_edged_outside() {
        let l = layer(&["1111", "1111", "1111"]);
        assert_eq!(corner_mask(&l, 2, 1, 1), 15, "inside: every corner");
        assert_eq!(corner_mask(&l, 2, 0, 1), 4 | 8, "along the top edge: the bottom two");
        assert_eq!(corner_mask(&l, 0, 0, 1), 8, "the outer corner");
    }

    #[test]
    fn a_higher_terrain_covers_a_lower_ones_corners() {
        let l = layer(&["12", "11"]);
        // Dual cell (1, 1) sits where all four meet.
        assert_eq!(corner_mask(&l, 1, 1, 1), 15, "every corner is terrain 1 or above");
        assert_eq!(corner_mask(&l, 1, 1, 2), 2, "only the top-right is terrain 2");
        let rule = AutotileLayer { layer: "ground".into(), terrains: vec![100, 200] };
        let gids: Vec<u32> = dual_cell_gids(&l, &rule, 1, 1).collect();
        assert_eq!(gids, vec![115, 202], "terrain 1 whole, terrain 2's top-right corner over it");
    }

    #[test]
    fn the_component_reads_from_a_scene() {
        let a: Autotile = serde_json::from_str(r#"{ "layers": [{ "layer": "ground", "terrains": [1, 17, 33] }] }"#).unwrap();
        assert_eq!(a.for_layer("ground").unwrap().terrains, vec![1, 17, 33]);
        assert!(a.for_layer("walls").is_none());
    }
}
