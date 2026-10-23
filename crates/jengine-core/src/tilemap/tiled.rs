//! Tiled JSON (.tmj) parser — imports orthogonal, isometric and hexagonal Tiled maps.

use serde::Deserialize;

use super::{Tilemap, TilemapLayer, TilesetDef};

/// Top-level Tiled JSON map.
#[derive(Debug, Deserialize)]
pub struct TiledMap {
    pub width: u32,
    pub height: u32,
    pub tilewidth: u32,
    pub tileheight: u32,
    /// `"orthogonal"`, `"isometric"` or `"hexagonal"`; `"staggered"`
    /// (staggered isometric) is read as orthogonal.
    #[serde(default)]
    pub orientation: String,
    /// A hexagonal map's flat-side length, in pixels.
    #[serde(default)]
    pub hexsidelength: f32,
    /// `"x"` or `"y"` (the default).
    #[serde(default)]
    pub staggeraxis: String,
    /// `"odd"` (the default) or `"even"`.
    #[serde(default)]
    pub staggerindex: String,
    #[serde(default)]
    pub layers: Vec<TiledLayer>,
    #[serde(default)]
    pub tilesets: Vec<TiledTileset>,
}

/// A layer in the Tiled JSON map.
#[derive(Debug, Deserialize)]
pub struct TiledLayer {
    pub name: String,
    #[serde(rename = "type")]
    pub layer_type: String,
    #[serde(default)]
    pub data: Vec<u32>,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Where Tiled draws the layer, in pixels with y down.
    #[serde(default)]
    pub offsetx: f32,
    #[serde(default)]
    pub offsety: f32,
}

fn default_true() -> bool {
    true
}

/// A tileset reference in the Tiled JSON map.
#[derive(Debug, Deserialize)]
pub struct TiledTileset {
    pub firstgid: u32,
    #[serde(default)]
    pub tilecount: u32,
    #[serde(default)]
    pub tilewidth: u32,
    #[serde(default)]
    pub tileheight: u32,
    #[serde(default)]
    pub columns: u32,
    /// Tileset image path (relative to .tmj file).
    #[serde(default)]
    pub image: String,
    /// Full tileset image width in pixels.
    #[serde(default)]
    pub imagewidth: u32,
    /// Full tileset image height in pixels.
    #[serde(default)]
    pub imageheight: u32,
    /// Pixels round the image's edge, and between tiles.
    #[serde(default)]
    pub margin: u32,
    #[serde(default)]
    pub spacing: u32,
    /// Per-tile data; `animation` and the collision `objectgroup` are read.
    #[serde(default)]
    pub tiles: Vec<TiledTile>,
}

/// A tileset's per-tile entry.
#[derive(Debug, Deserialize)]
pub struct TiledTile {
    pub id: u32,
    /// Frames, durations in milliseconds.
    #[serde(default)]
    pub animation: Vec<TiledFrame>,
    /// The tile's collision objects (Tiled's tile collision editor): the
    /// first rectangle or polygon becomes its [`super::TileShape`].
    #[serde(default)]
    pub objectgroup: Option<TiledObjectGroup>,
    /// Its custom properties: `[{ name, type, value }]`, which import as
    /// the tile's data ([`super::TilesetDef::data`]).
    #[serde(default)]
    pub properties: Vec<TiledProperty>,
}

/// One custom property of a Tiled tile.
#[derive(Debug, Deserialize)]
pub struct TiledProperty {
    pub name: String,
    #[serde(default)]
    pub value: serde_json::Value,
}

/// A tile's collision objects.
#[derive(Debug, Deserialize)]
pub struct TiledObjectGroup {
    #[serde(default)]
    pub objects: Vec<TiledObject>,
}

/// One collision object, in the tile's pixels: a rectangle at `x, y` of
/// `width` x `height`, or a `polygon` of points relative to `x, y`.
#[derive(Debug, Deserialize)]
pub struct TiledObject {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub width: f32,
    #[serde(default)]
    pub height: f32,
    #[serde(default)]
    pub polygon: Option<Vec<TiledPoint>>,
}

/// A polygon point.
#[derive(Debug, Deserialize)]
pub struct TiledPoint {
    pub x: f32,
    pub y: f32,
}

impl TiledTile {
    /// The tile's collision outline, if its object group has one Tiled can
    /// say as a polygon or a rectangle.
    fn collision_outline(&self) -> Option<Vec<[f32; 2]>> {
        self.objectgroup.as_ref()?.objects.iter().find_map(|o| match &o.polygon {
            Some(p) if p.len() >= 3 => Some(p.iter().map(|q| [o.x + q.x, o.y + q.y]).collect()),
            Some(_) => None,
            None if o.width > 0.0 && o.height > 0.0 => Some(vec![
                [o.x, o.y],
                [o.x + o.width, o.y],
                [o.x + o.width, o.y + o.height],
                [o.x, o.y + o.height],
            ]),
            None => None,
        })
    }
}

/// One frame of a Tiled tile animation.
#[derive(Debug, Deserialize)]
pub struct TiledFrame {
    pub tileid: u32,
    pub duration: u32,
}

/// Parse a Tiled JSON (.tmj) string into the engine's Tilemap format.
///
/// Only supports orthogonal tile layers. Object layers, image layers,
/// and group layers are silently skipped.
pub fn parse_tiled_json(json: &str) -> anyhow::Result<Tilemap> {
    let tiled: TiledMap = serde_json::from_str(json)?;

    let tilesets: Vec<TilesetDef> = tiled.tilesets.iter().map(|ts| {
        // Extract just the filename from the image path
        let texture_id = ts.image
            .rsplit('/')
            .next()
            .unwrap_or(&ts.image)
            .to_string();

        // Compute image dimensions from tile dimensions if not specified
        let image_width = if ts.imagewidth > 0 {
            ts.imagewidth
        } else if ts.columns > 0 {
            2 * ts.margin + ts.columns * ts.tilewidth + (ts.columns - 1) * ts.spacing
        } else {
            0
        };
        let rows = if ts.columns > 0 && ts.tilecount > 0 {
            ts.tilecount.div_ceil(ts.columns)
        } else {
            0
        };
        let image_height = if ts.imageheight > 0 {
            ts.imageheight
        } else if rows > 0 {
            2 * ts.margin + rows * ts.tileheight + (rows - 1) * ts.spacing
        } else {
            0
        };

        TilesetDef {
            texture_id,
            tile_width: ts.tilewidth,
            tile_height: ts.tileheight,
            columns: ts.columns,
            tile_count: ts.tilecount,
            first_gid: ts.firstgid,
            image_width,
            image_height,
            margin: ts.margin,
            spacing: ts.spacing,
            animations: ts
                .tiles
                .iter()
                .filter(|t| !t.animation.is_empty())
                .map(|t| super::TileAnimation {
                    tile: t.id,
                    frames: t
                        .animation
                        .iter()
                        .map(|f| super::TileFrame { tile: f.tileid, duration: f.duration as f32 / 1000.0 })
                        .collect(),
                    scatter: false,
                })
                .collect(),
            // Tiled's tile properties are the tile's data.
            data: ts
                .tiles
                .iter()
                .filter(|t| !t.properties.is_empty())
                .map(|t| {
                    let fields: serde_json::Map<String, serde_json::Value> =
                        t.properties.iter().map(|p| (p.name.clone(), p.value.clone())).collect();
                    (t.id.to_string(), serde_json::Value::Object(fields))
                })
                .collect(),
            random: Vec::new(),
            spawns: Vec::new(),
            shapes: ts
                .tiles
                .iter()
                .filter_map(|t| {
                    let outline = t.collision_outline()?;
                    Some(super::TileShape { tile: t.id, collision: Some(outline), occluder: None })
                })
                .collect(),
        }
    }).collect();

    let layers: Vec<TilemapLayer> = tiled.layers.iter()
        .filter(|l| l.layer_type == "tilelayer")
        .map(|l| {
            // Tiled spells an empty cell 0, which is what the layer stores.
            let tiles = l.data.clone();

            TilemapLayer {
                name: l.name.clone(),
                tiles,
                width: l.width,
                height: l.height,
                visible: l.visible,
                collision: l.name.to_lowercase().contains("collision"),
                offset: [l.offsetx, -l.offsety],
                ..Default::default()
            }
        })
        .collect();

    let orientation = match tiled.orientation.as_str() {
        "isometric" => super::TileOrientation::Isometric,
        "hexagonal" => super::TileOrientation::Hexagonal,
        _ => super::TileOrientation::Orthogonal,
    };
    Ok(Tilemap {
        tile_size: [tiled.tilewidth as f32, tiled.tileheight as f32],
        orientation,
        hex_side: if orientation == super::TileOrientation::Hexagonal { tiled.hexsidelength } else { 0.0 },
        stagger_axis: if tiled.staggeraxis == "x" { super::StaggerAxis::X } else { super::StaggerAxis::Y },
        stagger_index: if tiled.staggerindex == "even" { super::StaggerIndex::Even } else { super::StaggerIndex::Odd },
        layers,
        tilesets,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_tiled_map() {
        let json = r#"{
            "width": 3,
            "height": 2,
            "tilewidth": 16,
            "tileheight": 16,
            "layers": [
                {
                    "name": "ground",
                    "type": "tilelayer",
                    "data": [1, 2, 3, 0, 1, 2],
                    "width": 3,
                    "height": 2,
                    "visible": true
                }
            ],
            "tilesets": [
                {
                    "firstgid": 1,
                    "tilecount": 4,
                    "tilewidth": 16,
                    "tileheight": 16,
                    "columns": 2,
                    "image": "../sprites/tiles.png",
                    "imagewidth": 32,
                    "imageheight": 32
                }
            ]
        }"#;

        let tilemap = parse_tiled_json(json).unwrap();
        assert_eq!(tilemap.tile_size, [16.0, 16.0]);
        assert_eq!(tilemap.layers.len(), 1);
        assert_eq!(tilemap.tilesets.len(), 1);

        let layer = &tilemap.layers[0];
        assert_eq!(layer.name, "ground");
        assert_eq!(layer.width, 3);
        assert_eq!(layer.height, 2);
        assert_eq!(layer.get_tile(0, 0), Some(1));
        assert_eq!(layer.get_tile(1, 0), Some(2));
        assert_eq!(layer.get_tile(2, 0), Some(3));
        assert_eq!(layer.get_tile(0, 1), None); // GID 0 = empty

        let ts = &tilemap.tilesets[0];
        assert_eq!(ts.texture_id, "tiles.png");
        assert_eq!(ts.first_gid, 1);
        assert_eq!(ts.tile_count, 4);
    }

    #[test]
    fn a_tileset_keeps_its_margin_and_spacing() {
        // Tiled writes both; a sheet laid out with them drew every tile
        // shifted by the spacing it ignored.
        let json = r#"{
            "width": 1, "height": 1, "tilewidth": 16, "tileheight": 16,
            "layers": [ { "name": "g", "type": "tilelayer", "data": [4], "width": 1, "height": 1, "visible": true } ],
            "tilesets": [ { "firstgid": 1, "tilecount": 4, "tilewidth": 16, "tileheight": 16, "columns": 2,
                            "margin": 1, "spacing": 3, "image": "t.png" } ]
        }"#;
        let ts = &parse_tiled_json(json).unwrap().tilesets[0];
        assert_eq!((ts.margin, ts.spacing), (1, 3));
        assert_eq!((ts.image_width, ts.image_height), (37, 37), "the size worked out with both");
        let uv = ts.tile_uv(3).unwrap().map(|v| (v * 37.0).round() as u32);
        assert_eq!(uv, [20, 20, 36, 36]);
    }

    #[test]
    fn parse_skips_object_layers() {
        let json = r#"{
            "width": 2,
            "height": 2,
            "tilewidth": 16,
            "tileheight": 16,
            "layers": [
                {
                    "name": "ground",
                    "type": "tilelayer",
                    "data": [1, 1, 1, 1],
                    "width": 2,
                    "height": 2,
                    "visible": true
                },
                {
                    "name": "objects",
                    "type": "objectgroup",
                    "visible": true
                }
            ],
            "tilesets": [
                {
                    "firstgid": 1,
                    "tilecount": 4,
                    "tilewidth": 16,
                    "tileheight": 16,
                    "columns": 2,
                    "image": "tiles.png",
                    "imagewidth": 32,
                    "imageheight": 32
                }
            ]
        }"#;

        let tilemap = parse_tiled_json(json).unwrap();
        // Only tile layers are parsed
        assert_eq!(tilemap.layers.len(), 1);
        assert_eq!(tilemap.layers[0].name, "ground");
    }
}
