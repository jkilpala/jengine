//! 2.6 — A* pathfinding over a [`Tilemap`] layer.
//!
//! Walkability: a tile is walkable when its GID is `0` (empty) by default.
//! Out-of-bounds tiles are never walkable. Callers that need a custom rule
//! (cost properties, blocked tiles inverted, etc.) can use
//! [`find_path_with`] which takes a `walkable: Fn((i32, i32)) -> bool`
//! closure and uses the same A* core.

use super::{Tilemap, TilemapLayer};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// Movement model accepted by the pathfinder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMode {
    /// 4-direction (N/S/E/W). Heuristic + step cost use Manhattan distance.
    Manhattan,
    /// 8-direction. Diagonals cost `~1.414` (we use `1.0` and `1.5` in
    /// integer-friendly units of `2`/`3`) and use the octile heuristic.
    /// A diagonal step is allowed only when **both** flanking orthogonal
    /// neighbours are walkable, so a path never clips the corner of a wall.
    Diagonal,
}

impl PathMode {
    /// Parse a script-facing string. Accepts `"manhattan"` / `"4"` for
    /// [`PathMode::Manhattan`] and `"diagonal"` / `"8"` / `"octile"` for
    /// [`PathMode::Diagonal`]. Any other value falls back to Manhattan.
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "diagonal" | "8" | "octile" => Self::Diagonal,
            _ => Self::Manhattan,
        }
    }
}

#[derive(Eq, PartialEq)]
struct Node {
    /// `f = g + h`, scaled to integers (units of `1` for Manhattan, `2`
    /// for Diagonal so the diagonal cost `~1.414 ≈ 3/2` stays integer
    /// when we use `2`/`3` for ortho/diag).
    f: u32,
    /// Tile coordinates.
    x: i32,
    y: i32,
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap; invert so the lowest `f` pops first.
        other.f.cmp(&self.f)
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Find a path through `layer` from `start` to `goal`. Returns `None`
/// when either endpoint is unwalkable, out of bounds, or no path exists.
///
/// The returned vector starts with `start` and ends with `goal`.
pub fn find_path_on_layer(
    layer: &TilemapLayer,
    start: (i32, i32),
    goal: (i32, i32),
    mode: PathMode,
) -> Option<Vec<(i32, i32)>> {
    let (w, h) = (layer.width as i32, layer.height as i32);
    let in_bounds = |(x, y): (i32, i32)| x >= 0 && y >= 0 && x < w && y < h;
    let walkable = |p: (i32, i32)| -> bool {
        if !in_bounds(p) {
            return false;
        }
        layer.get_tile(p.0 as u32, p.1 as u32).is_none()
    };
    find_path_with(start, goal, mode, walkable)
}

/// Find a path on `tilemap`'s layer named `layer_name`. Convenience
/// wrapper over [`find_path_on_layer`] - except on a hexagonal map, whose
/// cells have six neighbours that depend on the row: there `mode` is
/// ignored and every step is to one of [`Tilemap::neighbours`].
pub fn find_path(
    tilemap: &Tilemap,
    layer_name: &str,
    start: (i32, i32),
    goal: (i32, i32),
    mode: PathMode,
) -> Option<Vec<(i32, i32)>> {
    let layer = tilemap.find_layer(layer_name)?;
    if tilemap.is_hex() {
        let (w, h) = (layer.width as i32, layer.height as i32);
        let walkable = |(x, y): (i32, i32)| {
            x >= 0 && y >= 0 && x < w && y < h && layer.get_tile(x as u32, y as u32).is_none()
        };
        return find_path_graph(
            start,
            goal,
            walkable,
            |(x, y)| tilemap.neighbours(x, y),
            |p| tilemap.cell_distance(p, goal),
        );
    }
    find_path_on_layer(layer, start, goal, mode)
}

/// A* over any grid whose cells know their own neighbours: every step costs
/// one and `distance` (to the goal) must never overestimate. How a
/// hexagonal map is searched - six neighbours, whose places depend on the
/// row - with the same A* as the square modes.
pub fn find_path_graph(
    start: (i32, i32),
    goal: (i32, i32),
    walkable: impl Fn((i32, i32)) -> bool,
    neighbours: impl Fn((i32, i32)) -> Vec<(i32, i32)>,
    distance: impl Fn((i32, i32)) -> u32,
) -> Option<Vec<(i32, i32)>> {
    if !walkable(start) || !walkable(goal) {
        return None;
    }
    if start == goal {
        return Some(vec![start]);
    }
    let mut open: BinaryHeap<Node> = BinaryHeap::new();
    let mut g_score: HashMap<(i32, i32), u32> = HashMap::new();
    let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    g_score.insert(start, 0);
    open.push(Node { f: distance(start), x: start.0, y: start.1 });
    while let Some(Node { x, y, .. }) = open.pop() {
        let cur = (x, y);
        if cur == goal {
            let mut path = vec![cur];
            let mut node = cur;
            while let Some(&prev) = came_from.get(&node) {
                path.push(prev);
                node = prev;
            }
            path.reverse();
            return Some(path);
        }
        let Some(&cur_g) = g_score.get(&cur) else { continue };
        for next in neighbours(cur) {
            if !walkable(next) {
                continue;
            }
            let tentative = cur_g + 1;
            if tentative < *g_score.get(&next).unwrap_or(&u32::MAX) {
                came_from.insert(next, cur);
                g_score.insert(next, tentative);
                open.push(Node { f: tentative + distance(next), x: next.0, y: next.1 });
            }
        }
    }
    None
}

/// Generic A* over an arbitrary walkability predicate. The predicate
/// must return `false` for out-of-bounds positions (the algorithm has no
/// implicit bounds).
pub fn find_path_with<F>(
    start: (i32, i32),
    goal: (i32, i32),
    mode: PathMode,
    walkable: F,
) -> Option<Vec<(i32, i32)>>
where
    F: Fn((i32, i32)) -> bool,
{
    if !walkable(start) || !walkable(goal) {
        return None;
    }
    if start == goal {
        return Some(vec![start]);
    }

    // Step cost scaling so diagonal moves stay integer-comparable:
    //   Manhattan: ortho = 1                       — heuristic = |dx|+|dy|
    //   Diagonal : ortho = 2, diagonal = 3 (~√2*2) — heuristic = octile
    let (ortho_cost, diag_cost) = match mode {
        PathMode::Manhattan => (1u32, 0u32),
        PathMode::Diagonal => (2u32, 3u32),
    };

    let heuristic = |p: (i32, i32)| -> u32 {
        let dx = (p.0 - goal.0).unsigned_abs();
        let dy = (p.1 - goal.1).unsigned_abs();
        match mode {
            PathMode::Manhattan => dx + dy,
            PathMode::Diagonal => {
                let (min, max) = if dx < dy { (dx, dy) } else { (dy, dx) };
                // 3*min + 2*(max-min) = 2*max + min
                2 * max + min
            }
        }
    };

    let mut open: BinaryHeap<Node> = BinaryHeap::new();
    let mut g_score: HashMap<(i32, i32), u32> = HashMap::new();
    let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();

    g_score.insert(start, 0);
    open.push(Node {
        f: heuristic(start),
        x: start.0,
        y: start.1,
    });

    let ortho: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    let diag: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

    while let Some(Node { x, y, .. }) = open.pop() {
        let cur = (x, y);
        if cur == goal {
            // Reconstruct.
            let mut path = vec![cur];
            let mut node = cur;
            while let Some(&prev) = came_from.get(&node) {
                path.push(prev);
                node = prev;
            }
            path.reverse();
            return Some(path);
        }

        let cur_g = match g_score.get(&cur) {
            Some(&g) => g,
            None => continue,
        };

        for &(dx, dy) in &ortho {
            let next = (cur.0 + dx, cur.1 + dy);
            if !walkable(next) {
                continue;
            }
            let tentative = cur_g + ortho_cost;
            if tentative < *g_score.get(&next).unwrap_or(&u32::MAX) {
                came_from.insert(next, cur);
                g_score.insert(next, tentative);
                open.push(Node {
                    f: tentative + heuristic(next),
                    x: next.0,
                    y: next.1,
                });
            }
        }

        if mode == PathMode::Diagonal {
            for &(dx, dy) in &diag {
                let next = (cur.0 + dx, cur.1 + dy);
                if !walkable(next) {
                    continue;
                }
                // No corner cutting. This used to block the step only when
                // *both* flanks were walls, so a diagonal could still shave the
                // corner of a single wall tile - a path an agent the size of a
                // tile cannot follow without walking into the wall.
                let flank_a = (cur.0 + dx, cur.1);
                let flank_b = (cur.0, cur.1 + dy);
                if !walkable(flank_a) || !walkable(flank_b) {
                    continue;
                }
                let tentative = cur_g + diag_cost;
                if tentative < *g_score.get(&next).unwrap_or(&u32::MAX) {
                    came_from.insert(next, cur);
                    g_score.insert(next, tentative);
                    open.push(Node {
                        f: tentative + heuristic(next),
                        x: next.0,
                        y: next.1,
                    });
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tilemap::TilemapLayer;

    /// Build a layer from an ASCII grid: '.' = walkable, '#' = blocked.
    fn layer_from_ascii(rows: &[&str]) -> TilemapLayer {
        let height = rows.len() as u32;
        let width = rows.first().map(|r| r.len()).unwrap_or(0) as u32;
        let mut tiles = Vec::with_capacity((width * height) as usize);
        for row in rows {
            assert_eq!(row.len() as u32, width, "ragged ascii rows");
            for c in row.chars() {
                tiles.push(if c == '#' { 1 } else { 0 });
            }
        }
        TilemapLayer {
            name: "test".into(),
            tiles,
            width,
            height,
            visible: true,
            collision: false,
            ..Default::default()
        }
    }

    #[test]
    fn manhattan_finds_shortest_open_path() {
        let layer = layer_from_ascii(&["....", "....", "....", "...."]);
        let path = find_path_on_layer(&layer, (0, 0), (3, 0), PathMode::Manhattan).unwrap();
        // Straight horizontal — exactly 4 tiles.
        assert_eq!(path.len(), 4);
        assert_eq!(path.first(), Some(&(0, 0)));
        assert_eq!(path.last(), Some(&(3, 0)));
    }

    #[test]
    fn manhattan_routes_around_walls() {
        let layer = layer_from_ascii(&[
            "....",
            ".##.",
            ".##.",
            "....",
        ]);
        let path = find_path_on_layer(&layer, (0, 0), (3, 3), PathMode::Manhattan).unwrap();
        // 7 steps (6 moves) is the optimum for a 4x4 ring around the 2x2 block.
        assert_eq!(path.len(), 7);
        for tile in &path {
            assert!(layer.get_tile(tile.0 as u32, tile.1 as u32).is_none(), "stepped on wall {tile:?}");
        }
    }

    #[test]
    fn no_path_returns_none() {
        let layer = layer_from_ascii(&[
            "..#..",
            "..#..",
            "..#..",
            "..#..",
            "..#..",
        ]);
        assert!(find_path_on_layer(&layer, (0, 0), (4, 0), PathMode::Manhattan).is_none());
    }

    #[test]
    fn diagonal_is_shorter_than_manhattan() {
        let layer = layer_from_ascii(&["....", "....", "....", "...."]);
        let m = find_path_on_layer(&layer, (0, 0), (3, 3), PathMode::Manhattan).unwrap();
        let d = find_path_on_layer(&layer, (0, 0), (3, 3), PathMode::Diagonal).unwrap();
        assert_eq!(m.len(), 7); // 6 ortho moves
        assert_eq!(d.len(), 4); // 3 diagonal moves
    }

    #[test]
    fn diagonal_blocks_corner_cutting() {
        // Two walls form a tight diagonal — diagonal step from (0,0) to
        // (1,1) must be blocked because both flanks are walls.
        let layer = layer_from_ascii(&[".#", "#."]);
        assert!(find_path_on_layer(&layer, (0, 0), (1, 1), PathMode::Diagonal).is_none());
    }

    #[test]
    fn diagonal_does_not_clip_a_single_wall_corner() {
        // (0,0) -> (1,1) with one wall at (1,0): the diagonal would shave its
        // corner, so the path must go round through (0,1).
        let layer = layer_from_ascii(&[".#", ".."]);
        let path = find_path_on_layer(&layer, (0, 0), (1, 1), PathMode::Diagonal).unwrap();
        assert_eq!(path, vec![(0, 0), (0, 1), (1, 1)]);
    }

    #[test]
    fn endpoints_must_be_walkable() {
        let layer = layer_from_ascii(&["..", ".#"]);
        assert!(find_path_on_layer(&layer, (0, 0), (1, 1), PathMode::Manhattan).is_none());
        assert!(find_path_on_layer(&layer, (1, 1), (0, 0), PathMode::Manhattan).is_none());
    }

    #[test]
    fn start_equals_goal_is_trivial() {
        let layer = layer_from_ascii(&[".."]);
        let path = find_path_on_layer(&layer, (0, 0), (0, 0), PathMode::Manhattan).unwrap();
        assert_eq!(path, vec![(0, 0)]);
    }

    #[test]
    fn path_mode_from_str_lossy() {
        assert_eq!(PathMode::from_str_lossy("manhattan"), PathMode::Manhattan);
        assert_eq!(PathMode::from_str_lossy("MANHATTAN"), PathMode::Manhattan);
        assert_eq!(PathMode::from_str_lossy("diagonal"), PathMode::Diagonal);
        assert_eq!(PathMode::from_str_lossy("octile"), PathMode::Diagonal);
        assert_eq!(PathMode::from_str_lossy("8"), PathMode::Diagonal);
        assert_eq!(PathMode::from_str_lossy("bogus"), PathMode::Manhattan); // default
    }
}
