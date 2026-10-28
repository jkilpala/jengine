//! Tilemap system — Tiled JSON (.tmj) import, tile layer rendering, and tile manipulation.

pub mod asset;
pub mod autotile;
pub mod pathfinding;
pub mod scene_tiles;
pub mod shapes;
pub mod tiled;

pub use pathfinding::{PathMode, find_path, find_path_graph, find_path_on_layer, find_path_with};
pub use shapes::{CellShape, TileShape, occluder_loops};

use serde::{Deserialize, Serialize};

/// Where a tilemap's cell (0, 0) hangs: its **world** position - carried by
/// its parent when it has one (Oct 2026). Every reader of a tilemap's place
/// goes through this: the tile pass and the y-sorted tiles, the colliders,
/// the ray snapshot, navigation, and the scripts' `world_to_tile` /
/// `tile_to_world` in every runtime - so what is drawn, what is solid and
/// what is picked agree for a parented map too. They all read the local
/// `Transform.position` before, so a minimap parented to the camera, or a map
/// moved by a parent with the things standing on it, drew and collided at
/// its local offset about the origin (four Strategy Classics games met it).
///
/// Scale is **not** applied: a tile's size is `tile_size`, which the packages
/// and the scripts' own arithmetic read too. A tilemap whose world scale is
/// not 1 says so once in the log; set `tile_size`, or zoom the camera.
pub fn world_origin(
    world: &hecs::World,
    entity: hecs::Entity,
    local: &crate::ecs::components::Transform,
) -> glam::Vec2 {
    let placed = world.get::<&crate::ecs::components::WorldTransform>(entity).ok();
    let scale = placed.as_ref().map_or(local.scale, |w| w.scale);
    if (scale.x - 1.0).abs() > 1e-4 || (scale.y - 1.0).abs() > 1e-4 {
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            log::warn!(
                "a tilemap's transform scale ({}, {}) is ignored - a tile is `tile_size` big;                  set tile_size or zoom the camera instead",
                scale.x, scale.y
            )
        });
    }
    crate::ecs::hierarchy::world_position(local, placed.as_deref())
}

fn is_zero_px(v: &u32) -> bool {
    *v == 0
}

/// Definition of a tileset — maps tile IDs to atlas UV regions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TilesetDef {
    /// Texture asset path for this tileset.
    pub texture_id: String,
    /// Width of a single tile in pixels.
    pub tile_width: u32,
    /// Height of a single tile in pixels.
    pub tile_height: u32,
    /// Number of tile columns in the tileset image.
    pub columns: u32,
    /// Total number of tiles in this tileset.
    pub tile_count: u32,
    /// First global tile ID for this tileset (from Tiled's firstgid).
    pub first_gid: u32,
    /// Full image width in pixels (for UV calculation).
    #[serde(default)]
    pub image_width: u32,
    /// Full image height in pixels (for UV calculation).
    #[serde(default)]
    pub image_height: u32,
    /// Pixels round the sheet's edge before the first tile (Tiled's
    /// `margin`, Oct 2026).
    #[serde(default, skip_serializing_if = "is_zero_px")]
    pub margin: u32,
    /// Pixels between neighbouring tiles on the sheet (Tiled's `spacing`).
    /// Read by nothing before Oct 2026, so a Tiled sheet with spacing drew
    /// each tile shifted. It is also how a linearly filtered tileset avoids
    /// bleeding its neighbours into a tile's edge: leave a gutter and fill it
    /// with the tile's own edge pixels (Burrow Pump).
    #[serde(default, skip_serializing_if = "is_zero_px")]
    pub spacing: u32,
    /// Tiles that animate (Oct 2026): wherever a cell names `tile`, it is
    /// drawn as its frames in turn - water, a torch, a waterfall. Tiled's
    /// tile animations, which import as these. The cells keep the tile they
    /// name, so `get_tile`, the colliders and pathfinding never see a frame.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<TileAnimation>,
    /// Tiles that are not the whole cell (Oct 2026): a slope, a ledge, a
    /// tile walked through, glass that casts no shadow ([`TileShape`]). A
    /// tile not named here is solid and casts a shadow over its whole cell.
    /// Tiled's tile collision objects import as these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<TileShape>,
    /// Fields a game reads off a tile (Oct 2026; Godot's custom data
    /// layers, Tiled's tile properties): any JSON by local tile id -
    /// `{ "3": { "kind": "lava", "damage": 5 } }`. `Tilemap.get_tile_data`
    /// reads them; they are the cell's tile's, never a drawn variant's.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub data: std::collections::BTreeMap<String, serde_json::Value>,
    /// Tiles drawn as one of several (Oct 2026; Godot's alternative tiles
    /// with a probability, Unity's Random Tile): wherever a cell names
    /// `tile`, it is drawn as one of its `variants`, picked by weight with a
    /// hash of the cell's place - the same cell always alike, no two meadows
    /// alike ([`TileRandom`]). The cell keeps the tile it names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub random: Vec<TileRandom>,
    /// Tiles that are things (Oct 2026; Godot's scene tiles): a cell naming
    /// `tile` becomes an instance of `prefab` at its centre when the scene
    /// loads ([`scene_tiles`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spawns: Vec<TileSpawn>,
}

/// A tile drawn as one of several ([`TilesetDef::random`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TileRandom {
    /// The local tile a cell names.
    pub tile: u32,
    /// `[tile, weight]` pairs it is drawn as; a weight of 0 never.
    pub variants: Vec<(u32, f32)>,
}

/// A tile that becomes a prefab instance when its scene loads
/// ([`TilesetDef::spawns`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TileSpawn {
    /// The local tile.
    pub tile: u32,
    /// The prefab, by name.
    pub prefab: String,
    /// Keep the tile in its cell, under the thing; by default the cell is
    /// emptied.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep: bool,
}

/// One animated tile of a tileset ([`TilesetDef::animations`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TileAnimation {
    /// The tile (local to the tileset, from 0) that animates.
    pub tile: u32,
    /// What it shows, in order, then round again.
    pub frames: Vec<TileFrame>,
    /// Start each cell at its own point in the loop, so a sea of the same
    /// water tile does not flip in lockstep. Off is Tiled's behaviour: every
    /// cell shows the same frame.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub scatter: bool,
}

/// One frame of a [`TileAnimation`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct TileFrame {
    /// The tile shown (local to the tileset).
    pub tile: u32,
    /// For how long, in seconds.
    pub duration: f32,
}

impl TilesetDef {
    /// The tile cell `(col, row)` shows for `local` at `time` seconds into
    /// the map's clock ([`Tilemap::anim_time`]): `local` itself unless it
    /// animates. The one place a frame is chosen - the tilemap pass and a
    /// y-sorted layer both draw through it.
    /// What a cell naming `local` is drawn as: a random variant picked by
    /// its place ([`TilesetDef::variant_at`]), then that tile's animation
    /// frame. The one rule every tile drawer goes through.
    pub fn drawn(&self, local: u32, time: f64, col: i64, row: i64) -> u32 {
        self.frame_at(self.variant_at(local, col, row), time, col, row)
    }

    /// The variant of `local` a cell at `col, row` shows ([`TileRandom`]):
    /// weighted by a hash of the place, so it never changes for that cell.
    pub fn variant_at(&self, local: u32, col: i64, row: i64) -> u32 {
        let Some(r) = self.random.iter().find(|r| r.tile == local) else { return local };
        let total: f32 = r.variants.iter().map(|v| v.1.max(0.0)).sum();
        if total <= 0.0 {
            return local;
        }
        let h = (col.wrapping_mul(0x8DA6_B343) ^ row.wrapping_mul(0xD816_3841) ^ (local as i64).wrapping_mul(0xCB1A_B31F)) as u64;
        let h = h.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40;
        let mut pick = (h as f32 / (1u64 << 24) as f32) * total;
        for (tile, w) in &r.variants {
            let w = w.max(0.0);
            if pick < w {
                return *tile;
            }
            pick -= w;
        }
        r.variants.iter().rev().find(|v| v.1 > 0.0).map(|v| v.0).unwrap_or(local)
    }

    /// The custom data of a local tile ([`TilesetDef::data`]).
    pub fn data_of(&self, local: u32) -> Option<&serde_json::Value> {
        self.data.get(&local.to_string())
    }

    pub fn frame_at(&self, local: u32, time: f64, col: i64, row: i64) -> u32 {
        let Some(anim) = self.animations.iter().find(|a| a.tile == local) else { return local };
        let total: f64 = anim.frames.iter().map(|f| f.duration.max(0.0) as f64).sum();
        if anim.frames.is_empty() || total <= 0.0 {
            return local;
        }
        let mut t = time;
        if anim.scatter {
            // A fixed offset per cell: a hash of its place, as a share of the loop.
            let h = (col.wrapping_mul(73_856_093) ^ row.wrapping_mul(19_349_663)) as u64;
            let h = h.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40;
            t += (h as f64 / (1u64 << 24) as f64) * total;
        }
        let mut t = t.rem_euclid(total);
        for f in &anim.frames {
            let d = f.duration.max(0.0) as f64;
            if t < d {
                return f.tile;
            }
            t -= d;
        }
        anim.frames[anim.frames.len() - 1].tile
    }
}

impl TilesetDef {
    /// Compute the UV rect [u_min, v_min, u_max, v_max] for a local tile index (0-based).
    pub fn tile_uv(&self, local_id: u32) -> Option<[f32; 4]> {
        if local_id >= self.tile_count || self.columns == 0 || self.image_width == 0 || self.image_height == 0 {
            return None;
        }
        let col = local_id % self.columns;
        let row = local_id / self.columns;
        let x = self.margin + col * (self.tile_width + self.spacing);
        let y = self.margin + row * (self.tile_height + self.spacing);
        let u_min = x as f32 / self.image_width as f32;
        let v_min = y as f32 / self.image_height as f32;
        let u_max = (x + self.tile_width) as f32 / self.image_width as f32;
        let v_max = (y + self.tile_height) as f32 / self.image_height as f32;
        Some([u_min, v_min, u_max, v_max])
    }
}

/// How many tiles across a collider chunk is.
///
/// A colliding tilemap's body is built and rebuilt **per chunk**, so an edit
/// costs one chunk rather than the whole map. 32 x 32 is 1024 tiles: small
/// enough that one pick swing on a million-tile world rebuilds a thousandth
/// of it, large enough that a flat floor is still a handful of merged boxes
/// rather than one per tile.
pub const CHUNK_TILES: u32 = 32;

/// Per-layer mutation counters, so [`crate::physics::PhysicsWorld`] can tell
/// what changed without reading every tile.
///
/// `total` is bumped by every [`TilemapLayer::set_tile`] that actually changes
/// something, and the counter for the containing chunk with it. A collider
/// sync compares `total` first - **one integer per layer per frame** - and
/// only looks at the chunks when it differs. That is the whole point: the
/// sync used to hash every tile of every colliding layer every frame just to
/// discover that nothing had moved.
///
/// Neither counter is serialized. A freshly loaded map starts at zero and so
/// does a physics world with no record of it, so the first sync builds
/// everything either way.
///
/// `id` is what stops a *replaced* layer from passing for the old one. A
/// counter alone is not enough: assign `tm.layers = vec![fresh_layer()]` and
/// the new layer's total is 0, which is exactly what the physics record last
/// saw - same width, same height, same name, so it would keep the colliders it
/// built for the map that is gone. Every `LayerRevisions::default()` takes the
/// next id instead, so a layer built from scratch never collides with one that
/// was already there. `Clone` keeps the id, which is right: a cloned layer has
/// the cloned tiles too.
#[derive(Debug, Clone)]
pub struct LayerRevisions {
    id: u64,
    total: u64,
    chunks: Vec<u32>,
}

impl Default for LayerRevisions {
    fn default() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self { id: NEXT.fetch_add(1, Ordering::Relaxed), total: 0, chunks: Vec::new() }
    }
}

impl LayerRevisions {
    /// Which layer these counters belong to - unique to the layer as it was
    /// constructed, so a `total` of 0 on a brand new layer cannot be mistaken
    /// for a `total` of 0 on the one it replaced.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// How many times the layer has changed. Compared first, because for an
    /// untouched layer it is the only thing a collider sync has to read.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// How many times chunk `index` has changed. Zero for a chunk that has
    /// never been written, which is also what an unallocated counter reads
    /// as - a layer nobody has edited has no per-chunk storage at all.
    pub fn chunk(&self, index: usize) -> u32 {
        self.chunks.get(index).copied().unwrap_or(0)
    }
}

/// A single tile layer in the tilemap.
///
/// Build one with `..Default::default()` after the fields you mean, so a
/// field added later does not break the literal (`visible` defaults to true).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TilemapLayer {
    /// Layer name (from Tiled).
    pub name: String,
    /// Flattened tile data: the global tile id of each cell, **`0` for an
    /// empty cell**. Every tileset's `first_gid` is 1 or more, so 0 can never
    /// name a tile - it is the "empty" of every tilemap format there is. This
    /// used to be `Vec<Option<u32>>`: eight bytes a cell to say what a zero
    /// already said, and two spellings of "nothing here" for every reader to
    /// agree on. Scene files still write `null` for an empty cell
    /// (`tile_serde`), so no asset and nothing in the editor changed.
    ///
    /// **Write through [`Self::set_tile`] or [`Self::tiles_mut`].** Writing
    /// this field directly bumps no revision, so a colliding tilemap keeps
    /// the colliders it had - the map draws the edit and collides as it was.
    #[serde(with = "tile_serde")]
    pub tiles: Vec<u32>,
    /// Layer width in tiles.
    pub width: u32,
    /// Layer height in tiles.
    pub height: u32,
    /// Whether this layer is visible.
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Whether this layer is a collision layer (not rendered in-game).
    #[serde(default)]
    pub collision: bool,
    /// Where the layer is **drawn**, moved by this many world units (Tiled's
    /// layer offset, with y up) - an isometric hill's second storey is a
    /// layer lifted by a block's height. Only the drawing moves: the cells,
    /// `world_to_tile`, the colliders and the sort below stay on the grid.
    #[serde(default, skip_serializing_if = "is_zero2")]
    pub offset: [f32; 2],
    /// Draw this layer's tiles **in the sprite pass**, each sorted with the
    /// `y_sort` sprites of the tilemap's z by its cell - so a villager walks
    /// behind a house tile and in front of it, as Godot's y-sorted tile
    /// layers do. A plain layer is drawn in the tilemap pass, under every
    /// sprite. An isometric tile sorts by its cell's centre, an orthogonal
    /// one by its cell's bottom edge.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub y_sort: bool,
    /// What has changed since the colliders were built. See [`LayerRevisions`].
    #[serde(skip)]
    pub rev: LayerRevisions,
}

impl Default for TilemapLayer {
    fn default() -> Self {
        Self {
            name: String::new(),
            tiles: Vec::new(),
            width: 0,
            height: 0,
            visible: true,
            collision: false,
            offset: [0.0, 0.0],
            y_sort: false,
            rev: LayerRevisions::default(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn is_zero2(v: &[f32; 2]) -> bool {
    v[0] == 0.0 && v[1] == 0.0
}

/// How a tilemap's cells sit in the world (Tiled's `orientation`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TileOrientation {
    /// Square cells in rows and columns - every tilemap before Oct 2026.
    #[default]
    Orthogonal,
    /// Diamond cells, `tile_size` wide and tall: a column runs down to the
    /// right and a row down to the left, and the entity's transform is the
    /// **top vertex of cell (0, 0)**. A tile whose image is taller than the
    /// diamond (a block, a house, a tree) stands on its cell, bottom-aligned,
    /// at its image's proportions.
    Isometric,
    /// Hexagons (Oct 2026), `tile_size` the hexagon's bounding box and
    /// `hex_side` the length of its flat sides, as Tiled has them. With
    /// `stagger_axis` y the hexagons point up and every other row is pushed
    /// half a hexagon right; with x they are flat-topped and every other
    /// column is pushed half a hexagon down (`stagger_index` says which: the
    /// odd ones or the even ones). The entity's transform is the top-left of
    /// cell (0, 0)'s bounding box. A tile image taller than the hexagon
    /// stands on its cell, bottom-aligned, as on an isometric map.
    Hexagonal,
}

/// Which way a hexagonal map's rows or columns are pushed half a cell
/// (Tiled's `staggeraxis`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StaggerAxis {
    /// Pointy-topped hexagons in rows; every other row is pushed right.
    #[default]
    Y,
    /// Flat-topped hexagons in columns; every other column is pushed down.
    X,
}

/// Which rows (or columns) of a hexagonal map are pushed (Tiled's
/// `staggerindex`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StaggerIndex {
    #[default]
    Odd,
    Even,
}

impl StaggerAxis {
    fn is_y(&self) -> bool {
        *self == StaggerAxis::Y
    }
}

impl StaggerIndex {
    fn is_odd(&self) -> bool {
        *self == StaggerIndex::Odd
    }
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

impl TileOrientation {
    fn is_orthogonal(&self) -> bool {
        *self == TileOrientation::Orthogonal
    }
}

impl TilemapLayer {
    /// Get the tile at grid position (x, y). `None` when out of bounds, or
    /// when there is no tile there.
    ///
    /// **A gid of `0` is not a tile.** Every tileset's `first_gid` is 1 or
    /// more, so 0 can never name one - it is the "empty" of every tilemap
    /// format there is, and `Some(0)` is what a hand-written map or a
    /// script means by "nothing here".
    ///
    /// That used to be true of the renderer only, which resolves gid 0 to
    /// no tileset and draws nothing, while `sync_tilemap_colliders` asked
    /// `is_some()` and got `true`. A map written with 0s for its empty
    /// cells therefore **drew as open air and collided as solid rock** -
    /// an invisible wall over the whole level. BUGS #141.
    pub fn get_tile(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = (y * self.width + x) as usize;
        self.tiles.get(idx).copied().filter(|gid| *gid != 0)
    }

    /// Set the tile at grid position (x, y). `None` clears it - and so
    /// does `Some(0)`, because 0 is not a tile (see [`Self::get_tile`]).
    /// Storing the `None` rather than keeping a `Some(0)` around means the
    /// map that gets saved says what it means.
    ///
    /// This is the only runtime path that changes a tile, which is what
    /// makes [`Self::rev`] trustworthy - and why a write that does not
    /// change anything bumps nothing. Painting the tile that is already
    /// there must not cost a collider rebuild, because a tool that paints
    /// while the mouse is down sends one every frame.
    pub fn set_tile(&mut self, x: u32, y: u32, tile_id: Option<u32>) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = (y * self.width + x) as usize;
        let Some(slot) = self.tiles.get_mut(idx) else {
            return;
        };
        // `None` and `Some(0)` both mean "clear it", and 0 is what an empty
        // cell is stored as.
        let next = tile_id.unwrap_or(0);
        let changed = *slot != next;
        *slot = next;
        if changed {
            self.touch_tile(x, y);
        }
    }

    /// The tile data, for code that fills a whole layer at once - a generator,
    /// a loader - marking every chunk as changed.
    ///
    /// Cheaper than a `set_tile` per cell, and unlike writing [`Self::tiles`]
    /// directly it leaves the colliders correct.
    pub fn tiles_mut(&mut self) -> &mut Vec<u32> {
        self.touch_all();
        &mut self.tiles
    }

    /// Mark the whole layer as changed. Call after writing [`Self::tiles`]
    /// directly; the colliders are rebuilt on the next sync.
    pub fn touch_all(&mut self) {
        self.rev.total = self.rev.total.wrapping_add(1);
        let n = self.chunk_count();
        self.rev.chunks.resize(n, 0);
        for c in &mut self.rev.chunks {
            *c = c.wrapping_add(1);
        }
    }

    fn touch_tile(&mut self, x: u32, y: u32) {
        self.rev.total = self.rev.total.wrapping_add(1);
        let n = self.chunk_count();
        if self.rev.chunks.len() != n {
            self.rev.chunks.resize(n, 0);
        }
        let index = (y / CHUNK_TILES * self.chunk_cols() + x / CHUNK_TILES) as usize;
        if let Some(c) = self.rev.chunks.get_mut(index) {
            *c = c.wrapping_add(1);
        }
    }

    /// How many [`CHUNK_TILES`]-wide chunk columns the layer has.
    pub fn chunk_cols(&self) -> u32 {
        self.width.div_ceil(CHUNK_TILES)
    }

    /// How many [`CHUNK_TILES`]-tall chunk rows the layer has.
    pub fn chunk_rows(&self) -> u32 {
        self.height.div_ceil(CHUNK_TILES)
    }

    /// How many chunks the layer is divided into, row-major.
    pub fn chunk_count(&self) -> usize {
        (self.chunk_cols() * self.chunk_rows()) as usize
    }

    /// The tile rectangle `(x, y, width, height)` of chunk `index`, clipped
    /// to the layer - the last column and row are usually short.
    pub fn chunk_rect(&self, index: usize) -> (u32, u32, u32, u32) {
        let cols = self.chunk_cols().max(1);
        let x = (index as u32 % cols) * CHUNK_TILES;
        let y = (index as u32 / cols) * CHUNK_TILES;
        (x, y, self.width.saturating_sub(x).min(CHUNK_TILES), self.height.saturating_sub(y).min(CHUNK_TILES))
    }
}

/// What a script means when it says "nothing here", whichever way it
/// spelt it.
///
/// Three spellings reach the runtimes and all three mean the same thing:
///
/// - **`nil` / `null` / absent.** What the docs have always promised and
///   what a script naturally writes.
/// - **A negative number.** What native and the QuickJS host accepted,
///   because their bindings took a plain `i32`.
/// - **`0`.** What every tilemap format on earth spells empty as, and
///   what a generator filling an array starts with. See [`TilemapLayer::get_tile`].
///
/// They did not all work everywhere. Native's binding took `i32`, so a
/// `nil` from a script was a **hard error every frame it was called** -
/// while the same script cleared tiles quite happily in the editor, whose
/// host read `null` and passed a negative straight through `as u32` into
/// a gid of four billion. Each runtime broke on the other's spelling, and
/// no ratchet test could see it because the function's *name* is bound in
/// both. Every caller now funnels through here.
pub fn script_tile_id(v: Option<f64>) -> Option<u32> {
    v.filter(|n| *n >= 1.0).map(|n| n as u32)
}

/// Tilemap component — a complete tile-based map attached to an entity.
///
/// Build one with `..Default::default()` after the fields you mean, so a
/// field added later does not break the literal.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tilemap {
    /// Width and height of a tile in world units - of the diamond, in an
    /// isometric map.
    pub tile_size: [f32; 2],
    /// Square cells or diamonds ([`TileOrientation`]); orthogonal unless said.
    #[serde(default, skip_serializing_if = "TileOrientation::is_orthogonal")]
    pub orientation: TileOrientation,
    /// A hexagonal map's flat sides, in world units: the upright ones of a
    /// pointy-topped hexagon (stagger y), the level ones of a flat-topped one
    /// (stagger x). Half the height (or width) makes a regular hexagon.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub hex_side: f32,
    /// Which way a hexagonal map is staggered ([`StaggerAxis`]).
    #[serde(default, skip_serializing_if = "StaggerAxis::is_y")]
    pub stagger_axis: StaggerAxis,
    /// Which rows (or columns) of a hexagonal map are pushed ([`StaggerIndex`]).
    #[serde(default, skip_serializing_if = "StaggerIndex::is_odd")]
    pub stagger_index: StaggerIndex,
    /// Tile layers, rendered bottom to top.
    pub layers: Vec<TilemapLayer>,
    /// Tileset definitions used by this map.
    pub tilesets: Vec<TilesetDef>,
    /// Optional path to a `.tilemap` asset file (relative to project assets).
    /// When present, layers/tilesets are loaded from this file at scene load time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Draw order — higher values draw on top among other tilemaps in
    /// the same pass.
    #[serde(default)]
    pub z_order: i32,
    /// Optional sort-layer name (see `SortLayers`). Affects ordering
    /// among other tilemap entities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    /// Seconds the map's tile animations have run
    /// ([`TilesetDef::animations`]), advanced by `tick_motion` - every host's
    /// shared step - so they run in Play and stand still while editing.
    /// Never saved.
    #[serde(skip)]
    pub anim_time: f64,
}

/// Advance every animated tilemap's clock ([`Tilemap::anim_time`]). Called by
/// `behaviors::tick_motion`.
pub fn tick_tile_animations(world: &mut hecs::World, dt: f32) {
    for (_, tm) in world.query_mut::<&mut Tilemap>() {
        if tm.tilesets.iter().any(|t| !t.animations.is_empty()) {
            tm.anim_time += dt.max(0.0) as f64;
        }
    }
}

impl Tilemap {
    /// A number that changes whenever the map's tiles, layers or tile size
    /// do, and only then.
    ///
    /// Folded from what the layers already count: each layer's
    /// [`LayerRevisions::id`] and [`LayerRevisions::total`], its size, and
    /// the tile size - so it reads a handful of integers, never a tile.
    /// Anything that mirrors a tilemap somewhere expensive (the browser
    /// hosts publish one to JavaScript for `get_tile`) keys its copy on
    /// this and rebuilds only when the key moves. A layer replaced rather
    /// than edited has a new id, so it cannot pass for the old one; a tile
    /// written through anything but `set_tile` / `tiles_mut` / `touch_all`
    /// bumps nothing and is invisible here, as it is to the colliders.
    pub fn revision_key(&self) -> u64 {
        // FNV-1a over the counters. Not a security hash; a few integers.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| {
            h ^= v;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        mix(self.tile_size[0].to_bits() as u64);
        mix(self.tile_size[1].to_bits() as u64);
        mix(self.orientation as u64);
        mix(self.hex_side.to_bits() as u64);
        mix(((self.stagger_axis as u64) << 1) | self.stagger_index as u64);
        mix(self.layers.len() as u64);
        for layer in &self.layers {
            mix(layer.rev.id());
            mix(layer.rev.total());
            mix(layer.width as u64);
            mix(layer.height as u64);
        }
        h
    }

    /// Resolve a global tile ID to (tileset_index, local_tile_id).
    /// The custom data of the tile `gid` names (`TilesetDef::data`).
    pub fn tile_data(&self, gid: u32) -> Option<&serde_json::Value> {
        let (ts, local) = self.resolve_gid(gid)?;
        self.tilesets[ts].data_of(local)
    }

    pub fn resolve_gid(&self, gid: u32) -> Option<(usize, u32)> {
        // Tilesets should be checked in reverse order (highest firstgid first)
        let mut best: Option<(usize, u32)> = None;
        for (i, ts) in self.tilesets.iter().enumerate() {
            if gid >= ts.first_gid {
                let local = gid - ts.first_gid;
                if local < ts.tile_count {
                    match best {
                        Some((_, prev_local)) if local < prev_local => {
                            best = Some((i, local));
                        }
                        None => {
                            best = Some((i, local));
                        }
                        _ => {}
                    }
                }
            }
        }
        best
    }

    /// Find a layer by name.
    pub fn find_layer(&self, name: &str) -> Option<&TilemapLayer> {
        self.layers.iter().find(|l| l.name == name)
    }

    /// Find a mutable layer by name.
    pub fn find_layer_mut(&mut self, name: &str) -> Option<&mut TilemapLayer> {
        self.layers.iter_mut().find(|l| l.name == name)
    }

    /// Where grid point `(col, row)` is, relative to the entity - fractional,
    /// so `(c + 0.5, r + 0.5)` is a cell's centre and `(c, r)` its top-left
    /// corner (its top vertex, in an isometric map). **The one place a
    /// tilemap's shape is decided**: drawing, picking, autotiling and the
    /// colliders all go through it and [`Self::local_to_grid`].
    ///
    /// A hexagonal grid is not a linear map of its indices, since every other
    /// row (or column) is pushed: there a point is its cell's centre plus the
    /// fraction across the cell times a step, so `(c + 0.5, r + 0.5)` is still
    /// the centre and the floor of [`Self::local_to_grid`] still the cell.
    pub fn grid_to_local(&self, col: f32, row: f32) -> (f32, f32) {
        let [tw, th] = self.tile_size;
        match self.orientation {
            TileOrientation::Orthogonal => (col * tw, -row * th),
            TileOrientation::Isometric => ((col - row) * tw * 0.5, -(col + row) * th * 0.5),
            TileOrientation::Hexagonal => {
                let (ci, ri) = (col.floor(), row.floor());
                let (cx, cy) = self.cell_centre(ci as i32, ri as i32);
                let (cs, rs) = self.hex_steps();
                (cx + (col - ci - 0.5) * cs, cy - (row - ri - 0.5) * rs)
            }
        }
    }

    /// The grid point at `(x, y)` relative to the entity: the inverse of
    /// [`Self::grid_to_local`], fractional. Floor it for the cell.
    pub fn local_to_grid(&self, x: f32, y: f32) -> (f32, f32) {
        let [tw, th] = self.tile_size;
        if tw <= 0.0 || th <= 0.0 {
            return (0.0, 0.0);
        }
        match self.orientation {
            TileOrientation::Orthogonal => (x / tw, -y / th),
            TileOrientation::Isometric => (x / tw - y / th, -y / th - x / tw),
            TileOrientation::Hexagonal => {
                let (ci, ri) = self.cell_at(x, y);
                let (cx, cy) = self.cell_centre(ci, ri);
                let (cs, rs) = self.hex_steps();
                let fc = ((x - cx) / cs + 0.5).clamp(0.0, 0.999);
                let fr = (-(y - cy) / rs + 0.5).clamp(0.0, 0.999);
                (ci as f32 + fc, ri as f32 + fr)
            }
        }
    }

    /// Convert world coordinates to tile grid coordinates, given the entity's position.
    pub fn world_to_tile(&self, entity_x: f32, entity_y: f32, world_x: f32, world_y: f32) -> (i32, i32) {
        // Y is inverted: world +Y is up, but tile row 0 is at the top.
        let (c, r) = self.local_to_grid(world_x - entity_x, world_y - entity_y);
        (c.floor() as i32, r.floor() as i32)
    }

    /// Convert tile grid coordinates to world coordinates (returns center of tile).
    pub fn tile_to_world(&self, entity_x: f32, entity_y: f32, tile_x: i32, tile_y: i32) -> (f32, f32) {
        let (x, y) = self.grid_to_local(tile_x as f32 + 0.5, tile_y as f32 + 0.5);
        (entity_x + x, entity_y + y)
    }

    /// Whether this is a hexagonal map.
    pub fn is_hex(&self) -> bool {
        self.orientation == TileOrientation::Hexagonal
    }

    /// A hexagonal map's step from one column to the next and from one row
    /// to the next, and whether `index` (a row for stagger y, a column for x)
    /// is a pushed one.
    fn hex_steps(&self) -> (f32, f32) {
        let [tw, th] = self.tile_size;
        let s = self.hex_side.clamp(0.0, if self.stagger_axis == StaggerAxis::Y { th } else { tw });
        match self.stagger_axis {
            StaggerAxis::Y => (tw, (th + s) * 0.5),
            StaggerAxis::X => ((tw + s) * 0.5, th),
        }
    }

    fn hex_pushed(&self, index: i32) -> bool {
        (index.rem_euclid(2) == 1) == (self.stagger_index == StaggerIndex::Odd)
    }

    /// The centre of cell `(col, row)`, relative to the entity. Cells off the
    /// map have centres too - the grid goes on.
    pub fn cell_centre(&self, col: i32, row: i32) -> (f32, f32) {
        let [tw, th] = self.tile_size;
        match self.orientation {
            TileOrientation::Hexagonal => {
                let (cs, rs) = self.hex_steps();
                match self.stagger_axis {
                    StaggerAxis::Y => {
                        let push = if self.hex_pushed(row) { tw * 0.5 } else { 0.0 };
                        (col as f32 * cs + tw * 0.5 + push, -(row as f32 * rs + th * 0.5))
                    }
                    StaggerAxis::X => {
                        let push = if self.hex_pushed(col) { th * 0.5 } else { 0.0 };
                        (col as f32 * cs + tw * 0.5, -(row as f32 * rs + th * 0.5 + push))
                    }
                }
            }
            _ => self.grid_to_local(col as f32 + 0.5, row as f32 + 0.5),
        }
    }

    /// Cell `(col, row)`'s outline relative to the entity, anticlockwise: a
    /// square, a diamond, or a hexagon.
    pub fn cell_outline(&self, col: i32, row: i32) -> Vec<(f32, f32)> {
        let [tw, th] = self.tile_size;
        if self.orientation == TileOrientation::Hexagonal {
            let (cx, cy) = self.cell_centre(col, row);
            let (hw, hh) = (tw * 0.5, th * 0.5);
            let pts: [(f32, f32); 6] = match self.stagger_axis {
                StaggerAxis::Y => {
                    let s = self.hex_side.clamp(0.0, th) * 0.5;
                    [(0.0, hh), (-hw, s), (-hw, -s), (0.0, -hh), (hw, -s), (hw, s)]
                }
                StaggerAxis::X => {
                    let s = self.hex_side.clamp(0.0, tw) * 0.5;
                    [(s, hh), (-s, hh), (-hw, 0.0), (-s, -hh), (s, -hh), (hw, 0.0)]
                }
            };
            return pts.iter().map(|(x, y)| (cx + x, cy + y)).collect();
        }
        let (c, r) = (col as f32, row as f32);
        // Anticlockwise with y up: the grid's rows run down.
        [(c, r), (c, r + 1.0), (c + 1.0, r + 1.0), (c + 1.0, r)].iter().map(|&(a, b)| self.grid_to_local(a, b)).collect()
    }

    /// The cell holding the point `(x, y)` relative to the entity - off the
    /// map too. On a hexagonal map, the hexagon it is inside (the nearest
    /// centre if it is on an edge).
    pub fn cell_at(&self, x: f32, y: f32) -> (i32, i32) {
        if self.orientation != TileOrientation::Hexagonal {
            let (c, r) = self.local_to_grid(x, y);
            return (c.floor() as i32, r.floor() as i32);
        }
        let [tw, th] = self.tile_size;
        if tw <= 0.0 || th <= 0.0 {
            return (0, 0);
        }
        let (cs, rs) = self.hex_steps();
        // A first guess from the steps, then the hexagons round it.
        let (gc, gr) = ((x / cs).floor() as i32, (-y / rs).floor() as i32);
        let mut best = (gc, gr);
        let mut best_d = f32::MAX;
        for r in gr - 1..=gr + 1 {
            for c in gc - 1..=gc + 1 {
                if point_in_convex(&self.cell_outline(c, r), (x, y)) {
                    return (c, r);
                }
                let (cx, cy) = self.cell_centre(c, r);
                let d = crate::detmath::DetMath::dpowi(cx - x, 2) + crate::detmath::DetMath::dpowi(cy - y, 2);
                if d < best_d {
                    best_d = d;
                    best = (c, r);
                }
            }
        }
        best
    }

    /// The cells next to `(col, row)`: four on a square or isometric map,
    /// six on a hexagonal one. Off-map cells included; the caller bounds them.
    pub fn neighbours(&self, col: i32, row: i32) -> Vec<(i32, i32)> {
        if self.orientation != TileOrientation::Hexagonal {
            return vec![(col + 1, row), (col - 1, row), (col, row + 1), (col, row - 1)];
        }
        let (c, r) = (col, row);
        match self.stagger_axis {
            StaggerAxis::Y => {
                // A pushed row's neighbours above and below are at its own
                // column and the next; an unpushed row's at the one before.
                let d = if self.hex_pushed(r) { 0 } else { -1 };
                vec![(c + 1, r), (c - 1, r), (c + d, r - 1), (c + d + 1, r - 1), (c + d, r + 1), (c + d + 1, r + 1)]
            }
            StaggerAxis::X => {
                let d = if self.hex_pushed(c) { 0 } else { -1 };
                vec![(c, r + 1), (c, r - 1), (c - 1, r + d), (c - 1, r + d + 1), (c + 1, r + d), (c + 1, r + d + 1)]
            }
        }
    }

    /// The map's size in cells: its largest layer's.
    pub fn size_in_cells(&self) -> (u32, u32) {
        self.layers.iter().fold((0, 0), |(w, h), l| (w.max(l.width), h.max(l.height)))
    }

    /// [`Self::neighbours`] that are on the map - what `Tilemap.neighbors`
    /// answers a script.
    pub fn neighbours_on_map(&self, col: i32, row: i32) -> Vec<(i32, i32)> {
        let (w, h) = self.size_in_cells();
        self.neighbours(col, row)
            .into_iter()
            .filter(|&(c, r)| c >= 0 && r >= 0 && (c as u32) < w && (r as u32) < h)
            .collect()
    }

    /// How many steps from one cell to another: through [`Self::neighbours`],
    /// ignoring what is on the cells. Manhattan on a square or isometric map.
    pub fn cell_distance(&self, a: (i32, i32), b: (i32, i32)) -> u32 {
        if self.orientation != TileOrientation::Hexagonal {
            return a.0.abs_diff(b.0) + a.1.abs_diff(b.1);
        }
        let (aq, ar) = self.hex_axial(a);
        let (bq, br) = self.hex_axial(b);
        let (dq, dr) = (aq - bq, ar - br);
        ((dq.abs() + dr.abs() + (dq + dr).abs()) / 2) as u32
    }

    /// A hexagonal cell in axial coordinates, where a step to any of its six
    /// neighbours changes `q`, `r` and `q + r` by at most one each.
    fn hex_axial(&self, (c, r): (i32, i32)) -> (i32, i32) {
        // Offset to axial: undo the push of every other row (or column).
        // Pushing the odd ones, a row's push is `r` rounded down to even,
        // halved; pushing the even ones, rounded up.
        let shift = |i: i32| -> i32 {
            if self.stagger_index == StaggerIndex::Odd {
                (i - i.rem_euclid(2)).div_euclid(2)
            } else {
                (i + i.rem_euclid(2)).div_euclid(2)
            }
        };
        match self.stagger_axis {
            StaggerAxis::Y => (c - shift(r), r),
            StaggerAxis::X => (c, r - shift(c)),
        }
    }

    /// Where cell `(col, row)` drawn with a tile of `tileset` goes, relative
    /// to the entity: the quad's centre and size, and the height it sorts by
    /// when its layer is `y_sort`. Orthogonal tiles fill their cell. An
    /// isometric tile is the diamond's width and its image's proportions,
    /// standing on the cell's bottom vertex - so a block or a house rises
    /// above its cell - and sorts by the cell's centre.
    pub fn cell_quad(&self, tileset: &TilesetDef, col: u32, row: u32) -> ([f32; 2], [f32; 2], f32) {
        let [tw, th] = self.tile_size;
        let (c, r) = (col as f32, row as f32);
        match self.orientation {
            TileOrientation::Orthogonal => {
                // The cell - unless the picture is taller than the cell is,
                // when it keeps its proportions and stands on the cell's
                // bottom edge, as an isometric or hexagonal tile does: a tree
                // or a mountain two cells tall over a square map (Oct 2026).
                // It was squashed into the cell, so Six Banners drew its tops
                // as a second, lifted layer.
                let (x, y) = self.grid_to_local(c + 0.5, r + 0.5);
                let foot = y - th * 0.5;
                let h = Self::picture_height(tileset, tw, th);
                if h > th + 1e-3 {
                    ([x, foot + h * 0.5], [tw, h], foot)
                } else {
                    ([x, y], [tw, th], foot)
                }
            }
            TileOrientation::Hexagonal => {
                // The hexagon's box, or the image's proportions standing on
                // the box's bottom edge.
                let h = Self::picture_height(tileset, tw, th);
                let (cx, cy) = self.cell_centre(col as i32, row as i32);
                ([cx, cy - th * 0.5 + h * 0.5], [tw, h], cy)
            }
            TileOrientation::Isometric => {
                let h = Self::picture_height(tileset, tw, th);
                let (bx, by) = self.grid_to_local(c + 1.0, r + 1.0);
                let (_, cy) = self.grid_to_local(c + 0.5, r + 0.5);
                ([bx, by + h * 0.5], [tw, h], cy)
            }
        }
    }

    /// A tileset's picture drawn `tw` wide: how tall it stands.
    fn picture_height(tileset: &TilesetDef, tw: f32, th: f32) -> f32 {
        if tileset.tile_width > 0 {
            tw * tileset.tile_height as f32 / tileset.tile_width as f32
        } else {
            th
        }
    }

    /// How far above its cell a tile can be drawn: the tallest tileset's
    /// image over the cell (the diamond, on an isometric map). Zero when no
    /// picture is taller than its cell.
    pub fn overhang(&self) -> f32 {
        let [tw, th] = self.tile_size;
        self.tilesets
            .iter()
            .filter(|t| t.tile_width > 0)
            .map(|t| tw * t.tile_height as f32 / t.tile_width as f32 - th)
            .fold(0.0, f32::max)
    }

    /// The cells of a `width` x `height` layer that may draw into the box
    /// `lo..hi` (relative to the entity, offset already taken off): the
    /// column range and the row range, both half-open and clipped to the
    /// layer. An isometric map widens the box by a cell, and downwards by
    /// [`Self::overhang`], since a tall tile below the box reaches up into it.
    pub fn cells_in_box(&self, width: u32, height: u32, lo: [f32; 2], hi: [f32; 2]) -> (std::ops::Range<u32>, std::ops::Range<u32>) {
        let [tw, th] = self.tile_size;
        let (lo, hi) = match self.orientation {
            // A tall picture below the box reaches up into it.
            TileOrientation::Orthogonal => ([lo[0], lo[1] - self.overhang()], hi),
            TileOrientation::Isometric | TileOrientation::Hexagonal => {
                ([lo[0] - tw, lo[1] - th - self.overhang()], [hi[0] + tw, hi[1] + th])
            }
        };
        let corners = [(lo[0], lo[1]), (hi[0], lo[1]), (lo[0], hi[1]), (hi[0], hi[1])];
        let (mut c0, mut c1, mut r0, mut r1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for (x, y) in corners {
            let (c, r) = self.local_to_grid(x, y);
            c0 = c0.min(c);
            c1 = c1.max(c);
            r0 = r0.min(r);
            r1 = r1.max(r);
        }
        let clip = |v: f32, n: u32| (v.max(0.0) as u32).min(n);
        (clip(c0.floor(), width)..clip(c1.ceil(), width), clip(r0.floor(), height)..clip(r1.ceil(), height))
    }
}

/// Whether `p` is inside (or on) the convex polygon `poly`, either winding.
fn point_in_convex(poly: &[(f32, f32)], p: (f32, f32)) -> bool {
    let mut sign = 0.0f32;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        if cross.abs() <= 1e-4 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tileset_uv_calculation() {
        let ts = TilesetDef {
            texture_id: "tiles.png".into(),
            tile_width: 16,
            tile_height: 16,
            columns: 4,
            tile_count: 8,
            first_gid: 1,
            image_width: 64,
            image_height: 32,
            ..Default::default()
        };
        // Tile 0 (top-left)
        let uv = ts.tile_uv(0).unwrap();
        assert_eq!(uv, [0.0, 0.0, 0.25, 0.5]);
        // Tile 3 (end of first row)
        let uv = ts.tile_uv(3).unwrap();
        assert_eq!(uv, [0.75, 0.0, 1.0, 0.5]);
        // Tile 4 (start of second row)
        let uv = ts.tile_uv(4).unwrap();
        assert_eq!(uv, [0.0, 0.5, 0.25, 1.0]);
        // Out of range
        assert!(ts.tile_uv(8).is_none());
    }

    #[test]
    fn tileset_uv_skips_margin_and_spacing() {
        // 2 x 2 tiles of 16 px, a 1 px margin and 2 px between them: 37 px.
        let ts = TilesetDef {
            tile_width: 16,
            tile_height: 16,
            columns: 2,
            tile_count: 4,
            image_width: 37,
            image_height: 37,
            margin: 1,
            spacing: 3,
            ..Default::default()
        };
        let px = |uv: [f32; 4]| uv.map(|v| (v * 37.0).round() as u32);
        assert_eq!(px(ts.tile_uv(0).unwrap()), [1, 1, 17, 17]);
        assert_eq!(px(ts.tile_uv(1).unwrap()), [20, 1, 36, 17]);
        assert_eq!(px(ts.tile_uv(3).unwrap()), [20, 20, 36, 36]);
        // Absent, both are zero and not written back out.
        let json = serde_json::to_string(&TilesetDef { tile_width: 8, ..Default::default() }).unwrap();
        assert!(!json.contains("margin") && !json.contains("spacing"), "{json}");
    }

    #[test]
    fn layer_get_set() {
        let mut layer = TilemapLayer {
            name: "ground".into(),
            tiles: vec![1, 2, 0, 4],
            width: 2,
            height: 2,
            visible: true,
            collision: false,
            ..Default::default()
        };
        assert_eq!(layer.get_tile(0, 0), Some(1));
        assert_eq!(layer.get_tile(1, 0), Some(2));
        assert_eq!(layer.get_tile(0, 1), None);
        assert_eq!(layer.get_tile(1, 1), Some(4));
        // Out of bounds
        assert_eq!(layer.get_tile(2, 0), None);

        layer.set_tile(0, 1, Some(99));
        assert_eq!(layer.get_tile(0, 1), Some(99));
        layer.set_tile(0, 1, None);
        assert_eq!(layer.get_tile(0, 1), None);
    }

    #[test]
    fn resolve_gid() {
        let tilemap = Tilemap {
            tile_size: [16.0, 16.0],
            layers: vec![],
            tilesets: vec![
                TilesetDef {
                    texture_id: "ground.png".into(),
                    tile_width: 16,
                    tile_height: 16,
                    columns: 4,
                    tile_count: 16,
                    first_gid: 1,
                    image_width: 64,
                    image_height: 64,
                    ..Default::default()
                },
                TilesetDef {
                    texture_id: "objects.png".into(),
                    tile_width: 16,
                    tile_height: 16,
                    columns: 4,
                    tile_count: 8,
                    first_gid: 17,
                    image_width: 64,
                    image_height: 32,
                    ..Default::default()
                },
            ],
            source: None,
            z_order: 0,
            sort_layer: None,
            ..Default::default()
        };
        // GID 1 = tileset 0, local 0
        assert_eq!(tilemap.resolve_gid(1), Some((0, 0)));
        // GID 16 = tileset 0, local 15
        assert_eq!(tilemap.resolve_gid(16), Some((0, 15)));
        // GID 17 = tileset 1, local 0
        assert_eq!(tilemap.resolve_gid(17), Some((1, 0)));
        // GID 0 = empty (no match)
        assert_eq!(tilemap.resolve_gid(0), None);
    }

    #[test]
    fn world_tile_conversion() {
        let tilemap = Tilemap {
            tile_size: [16.0, 16.0],
            layers: vec![],
            tilesets: vec![],
            source: None,
            z_order: 0,
            sort_layer: None,
            ..Default::default()
        };
        // Entity at origin
        let (tx, ty) = tilemap.world_to_tile(0.0, 0.0, 8.0, -8.0);
        assert_eq!(tx, 0);
        assert_eq!(ty, 0);

        let (wx, wy) = tilemap.tile_to_world(0.0, 0.0, 0, 0);
        assert_eq!(wx, 8.0);
        assert_eq!(wy, -8.0);
    }
}

#[cfg(test)]
mod revision_key_tests {
    use super::*;

    fn map() -> Tilemap {
        Tilemap {
            tile_size: [16.0, 16.0],
            layers: vec![TilemapLayer {
                name: "g".into(),
                tiles: vec![0; 16],
                width: 4,
                height: 4,
                visible: true,
                collision: false,
                ..Default::default()
            }],
            tilesets: Vec::new(),
            source: None,
            z_order: 0,
            sort_layer: None,
            ..Default::default()
        }
    }

    #[test]
    fn the_key_holds_still_until_something_changes() {
        let mut m = map();
        let k0 = m.revision_key();
        assert_eq!(k0, m.revision_key(), "reading it changes nothing");
        m.layers[0].set_tile(1, 1, Some(3));
        let k1 = m.revision_key();
        assert_ne!(k0, k1, "an edit moves it");
        m.layers[0].set_tile(1, 1, Some(3));
        assert_eq!(k1, m.revision_key(), "writing the same tile again is not a change");
        m.tile_size = [32.0, 32.0];
        assert_ne!(k1, m.revision_key(), "so does the tile size");
    }

    #[test]
    fn a_replaced_layer_cannot_pass_for_the_one_it_replaced() {
        let mut m = map();
        let k0 = m.revision_key();
        let fresh = TilemapLayer { tiles: vec![0; 16], ..m.layers[0].clone() };
        // A clone keeps the id, so this alone is the same map to the key.
        m.layers[0] = fresh;
        assert_eq!(k0, m.revision_key());
        m.layers = vec![TilemapLayer {
            name: "g".into(),
            tiles: vec![0; 16],
            width: 4,
            height: 4,
            visible: true,
            collision: false,
            ..Default::default()
        }];
        assert_ne!(k0, m.revision_key(), "a layer built from scratch has a new id");
    }
}

/// Scene files spell an empty cell `null`, and the editor writes and compares
/// `null` too - so the `Vec<u32>` a layer stores (0 for empty) reads and
/// writes that shape unchanged. A `0` in a file reads as empty as well.
mod tile_serde {
    use serde::de::{SeqAccess, Visitor};
    use serde::ser::SerializeSeq;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(tiles: &[u32], s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(tiles.len()))?;
        for &gid in tiles {
            if gid == 0 {
                seq.serialize_element(&None::<u32>)?;
            } else {
                seq.serialize_element(&gid)?;
            }
        }
        seq.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u32>, D::Error> {
        struct TilesVisitor;
        impl<'de> Visitor<'de> for TilesVisitor {
            type Value = Vec<u32>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a sequence of tile ids, null for an empty cell")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u32>, A::Error> {
                let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(cell) = seq.next_element::<Option<u32>>()? {
                    out.push(cell.unwrap_or(0));
                }
                Ok(out)
            }
        }
        d.deserialize_seq(TilesVisitor)
    }
}

#[cfg(test)]
mod tile_serde_tests {
    use super::*;

    #[test]
    fn an_empty_cell_round_trips_as_null_and_a_zero_reads_as_empty() {
        let layer = TilemapLayer {
            name: "g".into(),
            tiles: vec![0, 7, 0],
            width: 3,
            height: 1,
            visible: true,
            collision: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&layer).unwrap();
        assert!(json.contains(r#""tiles":[null,7,null]"#), "the file shape is unchanged: {json}");
        let back: TilemapLayer = serde_json::from_str(r#"{"name":"g","tiles":[null,7,0],"width":3,"height":1}"#).unwrap();
        assert_eq!(back.tiles, vec![0, 7, 0]);
    }
}
