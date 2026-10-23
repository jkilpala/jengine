//! Per-tile shapes (Oct 2026; Godot's TileSet physics and occlusion layers,
//! Tiled's tile collision editor): what part of a tile is solid, and what
//! part casts a shadow.
//!
//! Before this every tile of a colliding layer was a whole box and no tile
//! cast a shadow at all, so a slope was a staircase, a half-height ledge was
//! a full block, and a lit cave needed its rock outlined again by hand in
//! `light_occluder`s. A tileset now names shapes for the tiles that need
//! them ([`TilesetDef::shapes`]); a tile it does not name is the whole cell,
//! as before - so no map that exists changed.
//!
//! Shadows come from a `tilemap_occluder` naming the layers that cast them.
//! A tile's occluder is its collision shape unless it names one of its own,
//! and the cells' outlines are **merged** before the lights see them
//! ([`occluder_loops`]): edges two cells share cancel, edges that run on in
//! a straight line join. One box per tile would be four edges a tile, and a
//! wall of a hundred tiles would fill the lights' whole pool; merged it is
//! the four edges of its outline.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{Tilemap, TilesetDef};

/// One tile's shapes ([`TilesetDef::shapes`]).
///
/// Points are in the tile's **pixels**, `(0, 0)` its top-left and y down -
/// the way the art is drawn and the way Tiled stores a tile's collision
/// objects - and are mapped onto the cell through the grid, so on an
/// isometric map the tile's square is the diamond. A hexagonal map's cells
/// are hexagons and keep them: its tiles are whole or nothing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TileShape {
    /// The tile, local to the tileset (from 0).
    pub tile: u32,
    /// What is solid, as a **convex** outline. Absent: the whole cell.
    /// Empty: nothing - a tile in a solid layer that is walked through
    /// (grass, a sign).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision: Option<Vec<[f32; 2]>>,
    /// What casts a shadow, in a layer a `tilemap_occluder` names. Absent:
    /// the collision outline (the whole cell when that is absent too).
    /// Empty: nothing - glass, a grating. May be concave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occluder: Option<Vec<[f32; 2]>>,
}

/// What a cell holds for collision or shadow: the whole cell, nothing, or
/// an outline in grid coordinates (`(col + x / tile_width, row + y /
/// tile_height)`).
#[derive(Debug, Clone, PartialEq)]
pub enum CellShape {
    Full,
    Empty,
    Outline(Vec<(f32, f32)>),
}

impl TilesetDef {
    /// The shapes named for `local`, if any.
    pub fn shape_of(&self, local: u32) -> Option<&TileShape> {
        self.shapes.iter().find(|s| s.tile == local)
    }
}

impl Tilemap {
    /// Whether any tileset names a shape - the fast path for every map made
    /// before shapes existed.
    pub fn has_tile_shapes(&self) -> bool {
        self.tilesets.iter().any(|t| !t.shapes.is_empty())
    }

    /// A number that changes when any tileset's shapes do. The colliders
    /// and the merged occluder outlines key on it beside the tiles'.
    pub fn shapes_key(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| {
            h ^= v;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        for ts in &self.tilesets {
            mix(ts.shapes.len() as u64);
            mix(ts.tile_width as u64 | (ts.tile_height as u64) << 32);
            for s in &ts.shapes {
                mix(s.tile as u64);
                for (tag, pts) in [(1u64, &s.collision), (2, &s.occluder)] {
                    match pts {
                        None => mix(tag << 8),
                        Some(p) => {
                            mix(tag << 8 | 1 | (p.len() as u64) << 16);
                            for q in p {
                                mix(q[0].to_bits() as u64 | (q[1].to_bits() as u64) << 32);
                            }
                        }
                    }
                }
            }
        }
        h
    }

    /// What tile `gid` at `(col, row)` holds for collision.
    pub fn collision_shape(&self, gid: u32, col: u32, row: u32) -> CellShape {
        self.cell_shape(gid, col, row, false)
    }

    /// What tile `gid` at `(col, row)` holds for shadows.
    pub fn occluder_shape(&self, gid: u32, col: u32, row: u32) -> CellShape {
        self.cell_shape(gid, col, row, true)
    }

    fn cell_shape(&self, gid: u32, col: u32, row: u32, occluder: bool) -> CellShape {
        let Some((ti, local)) = self.resolve_gid(gid) else { return CellShape::Full };
        let ts = &self.tilesets[ti];
        let Some(shape) = ts.shape_of(local) else { return CellShape::Full };
        let pts = if occluder { shape.occluder.as_ref().or(shape.collision.as_ref()) } else { shape.collision.as_ref() };
        match pts {
            None => CellShape::Full,
            Some(p) if p.len() < 3 => CellShape::Empty,
            // A hexagon's cell is its own shape; a square of pixels does not
            // map onto it.
            Some(_) if self.is_hex() => CellShape::Full,
            Some(p) => {
                let (tw, th) = (ts.tile_width.max(1) as f32, ts.tile_height.max(1) as f32);
                CellShape::Outline(p.iter().map(|q| (col as f32 + q[0] / tw, row as f32 + q[1] / th)).collect())
            }
        }
    }
}

/// Quantum of the merge, in world units: corners closer than this are one.
const SNAP: f32 = 1.0 / 64.0;

type Key = (i64, i64);

fn key(x: f32, y: f32) -> Key {
    ((x / SNAP).round() as i64, (y / SNAP).round() as i64)
}

/// The outlines of everything the tiles of `layers` cast a shadow with,
/// relative to the tilemap's entity (as [`Tilemap::grid_to_local`]), merged:
/// the edges two cells share cancel out, an edge that meets the side of a
/// bigger one is cut where they meet so the shared part cancels too, and
/// edges running on in a straight line become one. What is left is a loop
/// round each solid mass and round each hole in one, wound the same way.
///
/// Where two named layers both hold a tile at one cell the first one listed
/// decides that cell's shape.
pub fn occluder_loops(tm: &Tilemap, layers: &[usize]) -> Vec<Vec<[f32; 2]>> {
    let mut taken = std::collections::HashSet::new();
    let mut polys: Vec<Vec<Key>> = Vec::new();
    for &li in layers {
        let Some(l) = tm.layers.get(li) else { continue };
        for row in 0..l.height {
            for col in 0..l.width {
                let Some(gid) = l.get_tile(col, row) else { continue };
                if !taken.insert((col, row)) {
                    continue;
                }
                let local: Vec<(f32, f32)> = match tm.occluder_shape(gid, col, row) {
                    CellShape::Empty => continue,
                    CellShape::Full if tm.is_hex() => tm.cell_outline(col as i32, row as i32),
                    CellShape::Full => {
                        let (c, r) = (col as f32, row as f32);
                        [(c, r), (c + 1.0, r), (c + 1.0, r + 1.0), (c, r + 1.0)]
                            .iter()
                            .map(|&(gc, gr)| tm.grid_to_local(gc, gr))
                            .collect()
                    }
                    CellShape::Outline(p) => p.iter().map(|&(gc, gr)| tm.grid_to_local(gc, gr)).collect(),
                };
                let mut k: Vec<Key> = local.iter().map(|&(x, y)| key(x, y)).collect();
                k.dedup();
                if k.len() > 1 && k.first() == k.last() {
                    k.pop();
                }
                if k.len() < 3 {
                    continue;
                }
                // One winding for every cell, so a shared edge runs one way
                // in one cell and the other way in its neighbour.
                if signed_area(&k) < 0 {
                    k.reverse();
                }
                polys.push(k);
            }
        }
    }
    merge_outlines(&polys)
        .into_iter()
        .map(|l| l.into_iter().map(|(x, y)| [x as f32 * SNAP, y as f32 * SNAP]).collect())
        .collect()
}

/// A `tilemap_occluder`'s merged outlines, kept between frames and keyed by
/// what they are made of (the tiles, the shapes and which layers), so a map
/// that does not change is merged once. Never saved; a clone starts empty.
#[derive(Default)]
pub struct OutlineCache(std::sync::Mutex<Option<(u64, std::sync::Arc<Vec<Vec<[f32; 2]>>>)>>);

impl Clone for OutlineCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for OutlineCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OutlineCache")
    }
}

impl OutlineCache {
    /// [`occluder_loops`] for the layers named `names`, from the cache when
    /// nothing they are made of has changed.
    pub fn loops(&self, tm: &Tilemap, names: &[String]) -> std::sync::Arc<Vec<Vec<[f32; 2]>>> {
        let layers: Vec<usize> =
            names.iter().filter_map(|n| tm.layers.iter().position(|l| &l.name == n)).collect();
        let mut k = tm.revision_key() ^ tm.shapes_key().rotate_left(17);
        for &i in &layers {
            k = (k ^ i as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
        let mut cache = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((key, loops)) = cache.as_ref()
            && *key == k
        {
            return std::sync::Arc::clone(loops);
        }
        let loops = std::sync::Arc::new(occluder_loops(tm, &layers));
        *cache = Some((k, std::sync::Arc::clone(&loops)));
        loops
    }
}

fn signed_area(p: &[Key]) -> i128 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a.0 as i128 * b.1 as i128 - b.0 as i128 * a.1 as i128
        })
        .sum()
}

fn cross(o: Key, a: Key, b: Key) -> i128 {
    (a.0 - o.0) as i128 * (b.1 - o.1) as i128 - (a.1 - o.1) as i128 * (b.0 - o.0) as i128
}

fn dot(o: Key, a: Key, b: Key) -> i128 {
    (a.0 - o.0) as i128 * (b.0 - o.0) as i128 + (a.1 - o.1) as i128 * (b.1 - o.1) as i128
}

/// The boundary of the union of `polys` (all wound alike, touching only
/// along edges - tiles do not overlap), as loops of corners.
fn merge_outlines(polys: &[Vec<Key>]) -> Vec<Vec<Key>> {
    // Every corner, bucketed, so an edge finds the corners lying on it
    // without looking at the whole map.
    let span = polys
        .iter()
        .flat_map(|p| p.windows(2).map(|w| (w[1].0 - w[0].0).abs().max((w[1].1 - w[0].1).abs())))
        .max()
        .unwrap_or(1)
        .max(1);
    let bucket = |k: Key| (k.0.div_euclid(span), k.1.div_euclid(span));
    let mut corners: HashMap<(i64, i64), Vec<Key>> = HashMap::new();
    for p in polys {
        for &k in p {
            corners.entry(bucket(k)).or_default().push(k);
        }
    }
    for v in corners.values_mut() {
        v.sort_unstable();
        v.dedup();
    }

    // Directed edges, each cut at every corner lying inside it, with each
    // piece cancelled by the same piece running the other way.
    let mut edges: HashMap<(Key, Key), i32> = HashMap::new();
    for p in polys {
        let n = p.len();
        for i in 0..n {
            let (a, b) = (p[i], p[(i + 1) % n]);
            let (ba, bb) = (bucket(a), bucket(b));
            let mut on: Vec<Key> = Vec::new();
            for bx in ba.0.min(bb.0) - 1..=ba.0.max(bb.0) + 1 {
                for by in ba.1.min(bb.1) - 1..=ba.1.max(bb.1) + 1 {
                    for &c in corners.get(&(bx, by)).map(Vec::as_slice).unwrap_or(&[]) {
                        if c != a && c != b && cross(a, b, c) == 0 && dot(a, b, c) > 0 && dot(b, a, c) > 0 {
                            on.push(c);
                        }
                    }
                }
            }
            on.sort_by_key(|&c| dot(a, b, c));
            let mut from = a;
            for to in on.into_iter().chain(std::iter::once(b)) {
                match edges.get_mut(&(to, from)) {
                    Some(n) if *n > 0 => *n -= 1,
                    _ => *edges.entry((from, to)).or_insert(0) += 1,
                }
                from = to;
            }
        }
    }

    // Chain what is left into loops.
    let mut out_of: HashMap<Key, Vec<Key>> = HashMap::new();
    let mut starts: Vec<(Key, Key)> = Vec::new();
    for (&(a, b), &n) in &edges {
        for _ in 0..n.max(0) {
            out_of.entry(a).or_default().push(b);
            starts.push((a, b));
        }
    }
    // Deterministic: the same map always gives the same loops in the same order.
    starts.sort_unstable();
    for v in out_of.values_mut() {
        v.sort_unstable();
    }
    let mut loops = Vec::new();
    for (a, _) in starts {
        if out_of.get(&a).is_none_or(Vec::is_empty) {
            continue;
        }
        let mut ring = vec![a];
        let mut at = a;
        loop {
            let Some(next) = out_of.get_mut(&at).and_then(|v| v.pop()) else { break };
            if next == a {
                break;
            }
            ring.push(next);
            at = next;
        }
        // Straight runs become one edge.
        let mut i = 0;
        while ring.len() >= 3 && i < ring.len() {
            let n = ring.len();
            let (p, c, q) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
            if cross(p, c, q) == 0 && dot(c, p, q) < 0 {
                ring.remove(i);
                if i > 0 {
                    i -= 1;
                }
            } else {
                i += 1;
            }
        }
        if ring.len() >= 3 {
            loops.push(ring);
        }
    }
    loops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tilemap::{TilemapLayer, TilesetDef};

    fn map(rows: &[&str], shapes: Vec<TileShape>) -> Tilemap {
        let (w, h) = (rows[0].len() as u32, rows.len() as u32);
        let mut layer = TilemapLayer { name: "rock".into(), width: w, height: h, ..Default::default() };
        layer.tiles = vec![0; (w * h) as usize];
        for (r, line) in rows.iter().enumerate() {
            for (c, ch) in line.chars().enumerate() {
                let gid = match ch {
                    '#' => 1, // tile 0: no shape, the whole cell
                    '/' => 2, // tile 1: a slope
                    '_' => 3, // tile 2: a half-height ledge
                    'o' => 4, // tile 3: solid glass, casts nothing
                    _ => 0,
                };
                layer.tiles[r * w as usize + c] = gid;
            }
        }
        layer.touch_all();
        Tilemap {
            tile_size: [32.0, 32.0],
            layers: vec![layer],
            tilesets: vec![TilesetDef {
                tile_width: 32,
                tile_height: 32,
                columns: 4,
                tile_count: 4,
                first_gid: 1,
                shapes,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn slope_and_ledge() -> Vec<TileShape> {
        vec![
            // A slope rising to the right: bottom-left, bottom-right, top-right.
            TileShape { tile: 1, collision: Some(vec![[0.0, 32.0], [32.0, 32.0], [32.0, 0.0]]), occluder: None },
            // A half-height ledge along the bottom.
            TileShape { tile: 2, collision: Some(vec![[0.0, 16.0], [32.0, 16.0], [32.0, 32.0], [0.0, 32.0]]), occluder: None },
            // A see-through tile: solid, casts nothing.
            TileShape { tile: 3, collision: None, occluder: Some(vec![]) },
        ]
    }

    #[test]
    fn a_solid_block_of_tiles_is_one_loop_of_four_edges() {
        let tm = map(&["###", "###"], vec![]);
        let loops = occluder_loops(&tm, &[0]);
        assert_eq!(loops.len(), 1, "{loops:?}");
        assert_eq!(loops[0].len(), 4, "the outline, not 24 tile edges: {loops:?}");
        let xs: Vec<f32> = loops[0].iter().map(|p| p[0]).collect();
        let ys: Vec<f32> = loops[0].iter().map(|p| p[1]).collect();
        assert_eq!(xs.iter().cloned().fold(f32::MAX, f32::min), 0.0);
        assert_eq!(xs.iter().cloned().fold(f32::MIN, f32::max), 96.0);
        assert_eq!(ys.iter().cloned().fold(f32::MAX, f32::min), -64.0, "rows run down");
        assert_eq!(ys.iter().cloned().fold(f32::MIN, f32::max), 0.0);
    }

    #[test]
    fn a_ring_of_rock_is_an_outer_loop_and_a_hole() {
        let tm = map(&["####", "#..#", "####"], vec![]);
        let loops = occluder_loops(&tm, &[0]);
        assert_eq!(loops.len(), 2, "{loops:?}");
        let mut sizes: Vec<usize> = loops.iter().map(Vec::len).collect();
        sizes.sort();
        assert_eq!(sizes, vec![4, 4]);
    }

    #[test]
    fn a_slope_and_a_ledge_keep_their_shape_and_merge_with_the_rock() {
        // A slope tile (/) beside full rock: the slope's hypotenuse stays,
        // its bottom and right side join the rock's.
        let tm = map(&["..##", "./##"], slope_and_ledge());
        let loops = occluder_loops(&tm, &[0]);
        assert_eq!(loops.len(), 1, "{loops:?}");
        let l = &loops[0];
        // Corners: the slope's foot, the rock's bottom-right, top-right,
        // top-left, and the slope's top where it meets the rock.
        assert_eq!(l.len(), 5, "{l:?}");
        assert!(l.contains(&[32.0, -64.0]), "the slope's foot: {l:?}");
        // A half-height ledge (_) against a full tile: the full tile's side
        // is cut where the ledge's top meets it, and the shared part cancels.
        let tm = map(&["..#", "._#"], slope_and_ledge());
        let loops = occluder_loops(&tm, &[0]);
        assert_eq!(loops.len(), 1, "{loops:?}");
        assert!(loops[0].contains(&[32.0, -48.0]) && loops[0].contains(&[64.0, -48.0]), "{:?}", loops[0]);
        assert_eq!(loops[0].len(), 6, "{:?}", loops[0]);
    }

    #[test]
    fn a_tile_can_be_solid_and_cast_nothing_or_be_walked_through() {
        let tm = map(&["o#"], slope_and_ledge());
        let loops = occluder_loops(&tm, &[0]);
        assert_eq!(loops.len(), 1);
        assert!(loops[0].iter().all(|p| p[0] >= 32.0), "the glass casts nothing: {:?}", loops[0]);
        assert_eq!(tm.collision_shape(4, 0, 0), CellShape::Full, "but it is solid");
        let mut walk = map(&["#"], vec![TileShape { tile: 0, collision: Some(vec![]), occluder: None }]);
        assert_eq!(walk.collision_shape(1, 0, 0), CellShape::Empty);
        assert!(occluder_loops(&walk, &[0]).is_empty(), "the occluder follows the collision shape");
        walk.tilesets[0].shapes[0].occluder = Some(vec![[0.0, 0.0], [32.0, 0.0], [32.0, 32.0], [0.0, 32.0]]);
        assert_eq!(occluder_loops(&walk, &[0]).len(), 1, "unless it names its own");
    }

    #[test]
    fn shapes_change_the_key_and_tiles_that_touch_at_a_corner_make_two_loops() {
        let a = map(&["#."], vec![]);
        let b = map(&["#."], slope_and_ledge());
        assert_ne!(a.shapes_key(), b.shapes_key());
        assert!(!a.has_tile_shapes() && b.has_tile_shapes());
        let tm = map(&["#.", ".#"], vec![]);
        let loops = occluder_loops(&tm, &[0]);
        let edges: usize = loops.iter().map(Vec::len).sum();
        assert_eq!(edges, 8, "two squares touching at a corner: {loops:?}");
    }
}
