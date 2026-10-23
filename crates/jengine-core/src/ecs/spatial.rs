//! A uniform spatial grid for "what is near this point" queries.
//!
//! # Why this exists
//!
//! Several systems need the same thing — the entities close to a position —
//! and each grew its own answer:
//!
//! * [`crate::ecs::systems::apply_separation`] compared every separating body
//!   to every other, `O(n²)`. Correct for the ten enemies it was written for,
//!   hopeless at a thousand.
//! * `update_trigger_areas` builds a candidate list over every entity with a
//!   `Transform` and scans all of it per trigger.
//! * `jengine-swarm` wrote a good CSR grid of its own, because neither of the
//!   above was usable from a package.
//!
//! Three implementations of one idea, of which two were wrong at scale. This
//! is the shared one, generalised from the swarm's: it derives its bounds from
//! the points rather than taking a fixed play area, so it works for a flock, a
//! crowd of enemies, or whatever a trigger is testing against.
//!
//! # Shape
//!
//! Counting sort into a CSR layout — counts, prefix sums, then a single index
//! array — rather than a `HashMap<cell, Vec<usize>>`. One allocation per build
//! instead of one per occupied cell, and the neighbour scan walks a contiguous
//! slice.
//!
//! Rebuilt rather than incrementally maintained: the points move every frame,
//! so a rebuild touches the memory an update would and needs no bookkeeping
//! that can go stale.

use glam::Vec2;

/// Largest grid dimension on either axis.
///
/// A tiny cell size over a large spread would otherwise allocate an enormous,
/// almost-empty grid — worse than the wider scan it saves. Past this the cell
/// size is grown to fit instead.
const MAX_DIM: i32 = 512;

/// A uniform grid over a set of points, built fresh each query.
pub struct UniformGrid {
    cols: i32,
    rows: i32,
    cell: f32,
    min: Vec2,
    /// Start offset of each cell's slice in `indices`, length `cols*rows + 1`.
    starts: Vec<u32>,
    /// Point indices, grouped by cell.
    indices: Vec<u32>,
}

impl UniformGrid {
    /// Build a grid over `points`, with cells at least `cell` across.
    ///
    /// `cell` should be the query radius: with cells that size, everything
    /// within the radius of a point is in the 3×3 block around it. A smaller
    /// cell would miss neighbours; a larger one only costs a wider scan.
    ///
    /// Bounds come from the points, so a cluster in one corner of the world
    /// does not pay for the empty rest of it.
    pub fn build(points: &[Vec2], cell: f32) -> Self {
        if points.is_empty() {
            return Self {
                cols: 1,
                rows: 1,
                cell: cell.max(1e-3),
                min: Vec2::ZERO,
                starts: vec![0, 0],
                indices: Vec::new(),
            };
        }

        let (mut min, mut max) = (points[0], points[0]);
        for p in &points[1..] {
            min = min.min(*p);
            max = max.max(*p);
        }
        let extent = (max - min).max(Vec2::splat(1e-3));

        // Grow the cell rather than the grid if the spread would need more
        // than `MAX_DIM` cells: a huge sparse grid costs more to allocate and
        // clear than the extra points a wider cell makes us test.
        let mut cell = cell.max(1e-3);
        let needed = (extent / cell).ceil();
        let worst = needed.x.max(needed.y);
        if worst > MAX_DIM as f32 {
            cell *= worst / MAX_DIM as f32;
        }

        let cols = ((extent.x / cell).ceil() as i32 + 1).clamp(1, MAX_DIM);
        let rows = ((extent.y / cell).ceil() as i32 + 1).clamp(1, MAX_DIM);
        let n_cells = (cols * rows) as usize;

        let cell_of = |p: Vec2| -> usize {
            let x = (((p.x - min.x) / cell) as i32).clamp(0, cols - 1);
            let y = (((p.y - min.y) / cell) as i32).clamp(0, rows - 1);
            (y * cols + x) as usize
        };

        let mut counts = vec![0u32; n_cells];
        for p in points {
            counts[cell_of(*p)] += 1;
        }
        let mut starts = vec![0u32; n_cells + 1];
        let mut running = 0u32;
        for i in 0..n_cells {
            starts[i] = running;
            running += counts[i];
        }
        starts[n_cells] = running;

        let mut cursor = starts.clone();
        let mut indices = vec![0u32; points.len()];
        for (i, p) in points.iter().enumerate() {
            let c = cell_of(*p);
            indices[cursor[c] as usize] = i as u32;
            cursor[c] += 1;
        }

        Self { cols, rows, cell, min, starts, indices }
    }

    /// Call `f` with the index of every point in the 3×3 cell block around
    /// `p`, until `f` returns `false`.
    ///
    /// Candidates, not results: the caller still checks the real distance.
    /// The early exit matters for callers that cap how many neighbours they
    /// consider — stopping has to skip the *walk*, since in a dense cell the
    /// walk is the cost.
    #[inline]
    pub fn for_each_near(&self, p: Vec2, mut f: impl FnMut(u32) -> bool) {
        let cx = (((p.x - self.min.x) / self.cell) as i32).clamp(0, self.cols - 1);
        let cy = (((p.y - self.min.y) / self.cell) as i32).clamp(0, self.rows - 1);
        for gy in (cy - 1).max(0)..=(cy + 1).min(self.rows - 1) {
            for gx in (cx - 1).max(0)..=(cx + 1).min(self.cols - 1) {
                let c = (gy * self.cols + gx) as usize;
                let (a, b) = (self.starts[c] as usize, self.starts[c + 1] as usize);
                for &idx in &self.indices[a..b] {
                    if !f(idx) {
                        return;
                    }
                }
            }
        }
    }

    /// The point indices grouped by cell — the permutation the grid sorted
    /// them into.
    ///
    /// `order()[slot]` is the caller's original index for the point in `slot`.
    /// Permuting the caller's own arrays by this and then scanning by *slot*
    /// is what turns a neighbour walk from a series of random reads into a
    /// walk over contiguous memory; see [`Self::for_each_near_run`].
    pub fn order(&self) -> &[u32] {
        &self.indices
    }

    /// Call `f` with each contiguous **slot range** in the 3×3 cell block
    /// around `p`, until `f` returns `false`.
    ///
    /// The cache-friendly sibling of [`Self::for_each_near`]. That one hands
    /// back original indices, so a caller reading its own arrays does a random
    /// access per candidate — at a couple of dozen neighbours per point that
    /// is where the time goes, not in the arithmetic. This hands back the
    /// ranges instead, so a caller that has permuted its data by [`Self::order`]
    /// reads each cell as a contiguous slice.
    ///
    /// Ranges are half-open `(start, end)` and index the permuted arrays, not
    /// the caller's originals. One cell per call, so a 3×3 block yields up to
    /// nine ranges. Empty cells are skipped, so a caller never sees
    /// `start == end`.
    ///
    /// **Tried and rejected:** cells are numbered row-major and the counting
    /// sort groups them in that order, so the three cells of one row of the
    /// block are one unbroken span of `indices` and could be handed over as a
    /// single range — three calls per block instead of nine, each three times
    /// as long. It visits exactly the same points in exactly the same order,
    /// and measured *slower*: 8.06 ms against 7.81 at 16,000 boids and 3.89
    /// against 3.77 at 8,000, repeatably. The short per-cell loops apparently
    /// suit the optimiser better than the long ones. Left as it is; the
    /// coalescing is not worth re-deriving.
    #[inline]
    pub fn for_each_near_run(&self, p: Vec2, mut f: impl FnMut(usize, usize) -> bool) {
        let cx = (((p.x - self.min.x) / self.cell) as i32).clamp(0, self.cols - 1);
        let cy = (((p.y - self.min.y) / self.cell) as i32).clamp(0, self.rows - 1);
        // Centre cell first, then the ring. Order is irrelevant to a caller
        // that visits everything, but decides *which* candidates a capped
        // caller keeps — and the centre cell holds the nearest ones. Starting
        // at the corner meant a capped walk kept whichever neighbours happened
        // to lie up and to the left, which is both a worse answer and a slower
        // one: the cap fills sooner in the denser centre.
        let centre = (cy * self.cols + cx) as usize;
        let (a, b) = (self.starts[centre] as usize, self.starts[centre + 1] as usize);
        if a != b && !f(a, b) {
            return;
        }
        for gy in (cy - 1).max(0)..=(cy + 1).min(self.rows - 1) {
            for gx in (cx - 1).max(0)..=(cx + 1).min(self.cols - 1) {
                let c = (gy * self.cols + gx) as usize;
                if c == centre {
                    continue;
                }
                let (a, b) = (self.starts[c] as usize, self.starts[c + 1] as usize);
                if a != b && !f(a, b) {
                    return;
                }
            }
        }
    }

    /// Number of points indexed.
    pub fn len(&self) -> usize {
        self.indices.len()
    }

    /// Whether the grid indexes nothing.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scatter(n: usize, spread: f32) -> Vec<Vec2> {
        (0..n)
            .map(|i| {
                let a = i as f32 * 2.399_963;
                let r = ((i % 97) as f32 / 97.0) * spread;
                Vec2::new(a.cos() * r, a.sin() * r)
            })
            .collect()
    }

    /// The grid is an optimisation, not a behaviour change.
    #[test]
    fn finds_the_same_neighbours_as_brute_force() {
        let points = scatter(600, 500.0);
        let radius = 40.0f32;
        let grid = UniformGrid::build(&points, radius);

        for (i, p) in points.iter().enumerate() {
            let mut got: Vec<usize> = Vec::new();
            grid.for_each_near(*p, |j| {
                let j = j as usize;
                if j != i && points[j].distance_squared(*p) <= radius * radius {
                    got.push(j);
                }
                true
            });
            let mut want: Vec<usize> = (0..points.len())
                .filter(|&j| j != i && points[j].distance_squared(*p) <= radius * radius)
                .collect();
            got.sort_unstable();
            want.sort_unstable();
            assert_eq!(got, want, "grid and brute force disagree for point {i}");
        }
    }

    #[test]
    fn an_empty_set_is_safe_to_query() {
        let grid = UniformGrid::build(&[], 10.0);
        assert!(grid.is_empty());
        let mut hits = 0;
        grid.for_each_near(Vec2::new(5.0, 5.0), |_| {
            hits += 1;
            true
        });
        assert_eq!(hits, 0);
    }

    #[test]
    fn coincident_points_all_land_in_the_same_cell() {
        // Degenerate but real: a spawner that puts everything at the origin
        // before the first tick spreads them.
        let points = vec![Vec2::ZERO; 50];
        let grid = UniformGrid::build(&points, 10.0);
        let mut seen = 0;
        grid.for_each_near(Vec2::ZERO, |_| {
            seen += 1;
            true
        });
        assert_eq!(seen, 50);
    }

    #[test]
    fn a_huge_spread_grows_the_cell_instead_of_the_grid() {
        // A 1-unit cell over a million-unit spread would need a million cells
        // per axis. The cell grows instead, so the build stays bounded.
        let points = vec![Vec2::ZERO, Vec2::splat(1_000_000.0)];
        let grid = UniformGrid::build(&points, 1.0);
        assert!(grid.cols <= MAX_DIM && grid.rows <= MAX_DIM);
        // And it still finds the neighbour it should.
        let mut found = false;
        grid.for_each_near(Vec2::ZERO, |i| {
            if i == 0 {
                found = true;
            }
            true
        });
        assert!(found);
    }

    /// The permuted walk must see exactly what the index walk sees — it is
    /// the same query with a different memory layout, not a different one.
    #[test]
    fn the_run_walk_and_the_index_walk_agree() {
        let points = scatter(400, 300.0);
        let grid = UniformGrid::build(&points, 35.0);
        let order = grid.order();
        let permuted: Vec<Vec2> = order.iter().map(|&i| points[i as usize]).collect();

        for p in &points {
            let mut by_index: Vec<u32> = Vec::new();
            grid.for_each_near(*p, |i| {
                by_index.push(i);
                true
            });

            let mut by_run: Vec<u32> = Vec::new();
            grid.for_each_near_run(*p, |a, b| {
                for slot in a..b {
                    // The permuted array is what a caller would actually read;
                    // check it lines up with the original the slot names.
                    assert_eq!(permuted[slot], points[order[slot] as usize]);
                    by_run.push(order[slot]);
                }
                true
            });

            by_index.sort_unstable();
            by_run.sort_unstable();
            assert_eq!(by_index, by_run);
        }
    }

    #[test]
    fn order_is_a_permutation_of_every_point() {
        let points = scatter(250, 400.0);
        let grid = UniformGrid::build(&points, 40.0);
        let mut seen: Vec<u32> = grid.order().to_vec();
        seen.sort_unstable();
        assert_eq!(seen, (0..points.len() as u32).collect::<Vec<_>>());
    }

    #[test]
    fn the_run_walk_stops_early_too() {
        // A filled 2D block, so the walk has three rows to stop between —
        // points on a single row could not tell an early exit from there
        // simply being nothing more to visit.
        let points: Vec<Vec2> = (0..81)
            .map(|i| Vec2::new((i % 9) as f32 * 12.0, (i / 9) as f32 * 12.0))
            .collect();
        let grid = UniformGrid::build(&points, 12.0);
        let mut all = 0;
        grid.for_each_near_run(Vec2::new(48.0, 48.0), |_, _| {
            all += 1;
            true
        });
        assert_eq!(all, 9, "a filled 3x3 block should be nine cell-runs");

        let mut ranges = 0;
        grid.for_each_near_run(Vec2::new(48.0, 48.0), |_, _| {
            ranges += 1;
            false
        });
        assert_eq!(ranges, 1, "returning false did not stop after the first run");
    }

    /// Empty cells are skipped rather than reported as zero-length ranges, so
    /// a caller never has to guard against `a == b`.
    #[test]
    fn empty_cells_are_not_reported() {
        let points = vec![Vec2::ZERO, Vec2::new(1.0, 0.0)];
        let grid = UniformGrid::build(&points, 5.0);
        grid.for_each_near_run(Vec2::ZERO, |a, b| {
            assert!(a < b, "an empty range was handed to the caller");
            true
        });
    }

    #[test]
    fn the_early_exit_stops_the_walk() {
        let points = vec![Vec2::ZERO; 100];
        let grid = UniformGrid::build(&points, 10.0);
        let mut seen = 0;
        grid.for_each_near(Vec2::ZERO, |_| {
            seen += 1;
            seen < 5
        });
        assert_eq!(seen, 5, "returning false did not stop the walk");
    }
}
