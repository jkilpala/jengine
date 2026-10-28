//! **Smooth terrain** (Oct 2026): ground drawn and collided as the contour of a
//! density grid - marching squares - so a cave's walls are slopes rather than
//! stairs of tiles or pixels, and digging rounds them off rather than biting a
//! square out.
//!
//! A [`SmoothTerrain`] holds a value at every corner of a grid of `cols` x
//! `rows` cells, `cell` world units each, centred on its entity; a corner at
//! or above `iso` is solid. Absent an authored `density`, it is generated from
//! fractal noise (`seed`, `noise_scale`, `fill`) with a solid border.
//! `Terrain.edit(e, x, y, radius, amount)` digs (positive) or builds
//! (negative) round a world point. Each cell's solid part is a convex polygon
//! cut where the density crosses `iso` (linearly interpolated, so the
//! contour is smooth), and three things are made from those polygons, cached
//! under the terrain's revision:
//!
//! - the **fill** - every polygon fanned into triangles, textured in tiles of
//!   `tile` cells (the UVs local to a tile, since every sampler clamps);
//! - the **edge** - a strip `edge_width` deep inside every contour segment,
//!   moss on rock, `edge_texture` across it;
//! - the **collider** - full cells merged along each row into boxes, the
//!   rest one convex polygon each (`physics::terrain_colliders`), rebuilt on
//!   the same body when the terrain changes, so what rested on dug ground
//!   falls.
//!
//! The renderer draws fill and edge as two `InterleavedItem::Skinned`s, as a
//! spline shape's are; the browsers' ray snapshot builds the same colliders.
//! So no host was wired.

use std::sync::{Arc, Mutex};

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

/// The ground: a density grid drawn and collided as its contour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SmoothTerrain {
    pub cols: u32,
    pub rows: u32,
    /// World units a cell.
    pub cell: f32,
    /// A corner at or above this is solid.
    pub iso: f32,
    /// Generated when `density` is empty: fractal noise of this seed...
    pub seed: u32,
    /// ...whose features are this many world units across...
    pub noise_scale: f32,
    /// ...shifted by this (more is more solid; 0.5 about half), with a solid
    /// border round the edge when `border`.
    pub fill: f32,
    pub border: bool,
    /// The value at every corner, `(cols + 1) * (rows + 1)`, row by row from
    /// the bottom. Saved, so a dug cave stays dug.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub density: Vec<f32>,
    pub fill_texture: String,
    /// Cells a fill tile spans.
    pub tile: u32,
    pub fill_color: [f32; 4],
    pub edge_texture: String,
    /// How deep the edge strip reaches inside the contour, world units.
    pub edge_width: f32,
    pub edge_color: [f32; 4],
    pub collider: bool,
    pub friction: f32,
    pub z_order: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    /// Bumped by every edit (and drawn fresh from a counter the first time),
    /// so the mesh, the collider and the ray snapshot know to rebuild.
    #[serde(skip)]
    pub rev: u64,
    #[serde(skip)]
    pub cache: TerrainCache,
}

impl Default for SmoothTerrain {
    fn default() -> Self {
        Self {
            cols: 64,
            rows: 36,
            cell: 20.0,
            iso: 0.5,
            seed: 1,
            noise_scale: 260.0,
            fill: 0.5,
            border: true,
            density: Vec::new(),
            fill_texture: String::new(),
            tile: 8,
            fill_color: [1.0, 1.0, 1.0, 1.0],
            edge_texture: String::new(),
            edge_width: 12.0,
            edge_color: [1.0, 1.0, 1.0, 1.0],
            collider: true,
            friction: 0.8,
            z_order: 0,
            sort_layer: None,
            rev: 0,
            cache: TerrainCache::default(),
        }
    }
}

/// The built mesh and collider pieces, under the revision they were built at.
#[derive(Default)]
pub struct TerrainCache(Mutex<Option<(u64, Arc<TerrainMesh>)>>);

impl Clone for TerrainCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for TerrainCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TerrainCache")
    }
}

/// One collider piece, in the terrain's frame.
#[derive(Debug, Clone, PartialEq)]
pub enum TerrainPiece {
    /// A run of full cells.
    Box { centre: Vec2, half: Vec2 },
    /// A cell's solid part.
    Convex(Vec<Vec2>),
}

/// What a terrain is drawn and collided as, in its own frame.
#[derive(Debug, Default)]
pub struct TerrainMesh {
    /// Triangles: (position, uv) per vertex, three a triangle.
    pub fill: Vec<(Vec2, [f32; 2])>,
    pub edge: Vec<(Vec2, [f32; 2])>,
    pub pieces: Vec<TerrainPiece>,
    /// The contour, segment by segment, solid on each segment's left.
    pub contour: Vec<(Vec2, Vec2)>,
}

static NEXT_REV: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn fresh_rev() -> u64 {
    NEXT_REV.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl SmoothTerrain {
    fn corners(&self) -> (usize, usize) {
        (self.cols.max(1) as usize + 1, self.rows.max(1) as usize + 1)
    }

    /// The bottom-left corner of the grid, in the entity's frame.
    pub fn origin(&self) -> Vec2 {
        Vec2::new(-(self.cols.max(1) as f32) * self.cell * 0.5, -(self.rows.max(1) as f32) * self.cell * 0.5)
    }

    /// Make sure there is a density grid (generating it) and a revision.
    pub fn ensure(&mut self) {
        let (w, h) = self.corners();
        if self.density.len() != w * h {
            self.density = self.generate();
            self.rev = 0;
        }
        if self.rev == 0 {
            self.rev = fresh_rev();
        }
    }

    /// The density grid from noise, with a solid border.
    pub fn generate(&self) -> Vec<f32> {
        let (w, h) = self.corners();
        let origin = self.origin();
        let scale = self.noise_scale.max(1.0) as f64;
        let mut out = Vec::with_capacity(w * h);
        for j in 0..h {
            for i in 0..w {
                let p = origin + Vec2::new(i as f32, j as f32) * self.cell;
                let (x, y) = (p.x as f64 / scale, p.y as f64 / scale);
                let n = crate::noise::perlin(x, y, self.seed) * 0.65
                    + crate::noise::perlin(x * 2.1, y * 2.1, self.seed.wrapping_add(17)) * 0.35;
                let mut v = (0.5 + n as f32 * 0.9 + (self.fill - 0.5)).clamp(0.0, 1.0);
                if self.border {
                    let edge = i.min(w - 1 - i).min(j).min(h - 1 - j);
                    if edge < 2 {
                        v = 1.0;
                    }
                }
                out.push(v);
            }
        }
        out
    }

    /// Dig (`amount` > 0) or build (< 0) round `local` (the entity's frame):
    /// each corner within `radius` moves toward empty or solid by `amount`,
    /// fully at the middle and not at all at the rim.
    pub fn edit_local(&mut self, local: Vec2, radius: f32, amount: f32) {
        self.ensure();
        let (w, h) = self.corners();
        let origin = self.origin();
        let r = radius.max(1e-3);
        let lo = ((local - origin - Vec2::splat(r)) / self.cell).floor();
        let hi = ((local - origin + Vec2::splat(r)) / self.cell).ceil();
        let mut changed = false;
        for j in (lo.y.max(0.0) as usize)..=(hi.y.max(0.0) as usize).min(h - 1) {
            for i in (lo.x.max(0.0) as usize)..=(hi.x.max(0.0) as usize).min(w - 1) {
                let p = origin + Vec2::new(i as f32, j as f32) * self.cell;
                let d = p.distance(local);
                if d >= r {
                    continue;
                }
                let k = 1.0 - crate::detmath::DetMath::dpowi(d / r, 2);
                let v = &mut self.density[j * w + i];
                let n = (*v - amount * k).clamp(0.0, 1.0);
                if n != *v {
                    *v = n;
                    changed = true;
                }
            }
        }
        if changed {
            self.rev = fresh_rev();
        }
    }

    /// The density at `local`, between the four corners round it (0 outside).
    pub fn density_at(&self, local: Vec2) -> f32 {
        let (w, h) = self.corners();
        if self.density.len() != w * h {
            return 0.0;
        }
        let g = (local - self.origin()) / self.cell;
        if g.x < 0.0 || g.y < 0.0 || g.x > (w - 1) as f32 || g.y > (h - 1) as f32 {
            return 0.0;
        }
        let (i, j) = ((g.x.floor() as usize).min(w - 2), (g.y.floor() as usize).min(h - 2));
        let (fx, fy) = (g.x - i as f32, g.y - j as f32);
        let at = |i: usize, j: usize| self.density[j * w + i];
        let a = at(i, j) + (at(i + 1, j) - at(i, j)) * fx;
        let b = at(i, j + 1) + (at(i + 1, j + 1) - at(i, j + 1)) * fx;
        a + (b - a) * fy
    }

    /// The mesh, built once per revision.
    pub fn mesh(&self) -> Arc<TerrainMesh> {
        let key = self.rev ^ ((self.cols as u64) << 40) ^ ((self.rows as u64) << 52);
        let mut cache = self.cache.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((k, m)) = cache.as_ref()
            && *k == key
        {
            return Arc::clone(m);
        }
        let m = Arc::new(self.build());
        *cache = Some((key, Arc::clone(&m)));
        m
    }

    /// One cell's solid part, as convex polygons (counter-clockwise), and the
    /// contour segments through it (solid on the left).
    fn cell_polygons(&self, i: usize, j: usize) -> (Vec<Vec<Vec2>>, Vec<(Vec2, Vec2)>) {
        let (w, _) = self.corners();
        let o = self.origin();
        let c = self.cell;
        let corner = |a: usize, b: usize| (o + Vec2::new(a as f32, b as f32) * c, self.density[b * w + a]);
        // Bottom-left, bottom-right, top-right, top-left: counter-clockwise.
        let ring = [corner(i, j), corner(i + 1, j), corner(i + 1, j + 1), corner(i, j + 1)];
        let iso = self.iso;
        let inside: Vec<bool> = ring.iter().map(|(_, v)| *v >= iso).collect();
        let n_in = inside.iter().filter(|b| **b).count();
        if n_in == 0 {
            return (Vec::new(), Vec::new());
        }
        let cross = |a: (Vec2, f32), b: (Vec2, f32)| {
            let t = ((iso - a.1) / (b.1 - a.1)).clamp(0.0, 1.0);
            a.0 + (b.0 - a.0) * t
        };
        // A saddle whose middle is empty is two corners apart.
        let saddle = n_in == 2 && inside[0] == inside[2];
        let middle = ring.iter().map(|(_, v)| v).sum::<f32>() / 4.0;
        if saddle && middle < iso {
            let mut polys = Vec::new();
            let mut segs = Vec::new();
            for k in 0..4 {
                if !inside[k] {
                    continue;
                }
                let prev = ring[(k + 3) % 4];
                let next = ring[(k + 1) % 4];
                let a = cross(ring[k], next);
                let b = cross(ring[k], prev);
                polys.push(vec![ring[k].0, a, b]);
                segs.push((a, b));
            }
            return (polys, segs);
        }
        // Walk the ring: every solid corner, and a crossing where it changes.
        let mut poly = Vec::new();
        let mut segs = Vec::new();
        let mut leaving: Option<Vec2> = None;
        let mut first_entry: Option<Vec2> = None;
        for k in 0..4 {
            let next = (k + 1) % 4;
            if inside[k] {
                poly.push(ring[k].0);
            }
            if inside[k] != inside[next] {
                let p = cross(ring[k], ring[next]);
                poly.push(p);
                if inside[k] {
                    leaving = Some(p);
                } else {
                    // Entering again: the contour ran from where it left.
                    match leaving.take() {
                        Some(a) => segs.push((a, p)),
                        None => first_entry = Some(p),
                    }
                }
            }
        }
        if let (Some(a), Some(b)) = (leaving, first_entry) {
            segs.push((a, b));
        }
        (vec![poly], segs)
    }

    fn build(&self) -> TerrainMesh {
        let mut mesh = TerrainMesh::default();
        let (w, h) = self.corners();
        if self.density.len() != w * h {
            return mesh;
        }
        let tile = self.tile.max(1) as usize;
        let tile_world = tile as f32 * self.cell;
        let origin = self.origin();
        let (cols, rows) = (w - 1, h - 1);
        for j in 0..rows {
            let mut run: Option<usize> = None;
            for i in 0..=cols {
                let full = i < cols && (0..4).all(|k| {
                    let (a, b) = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)][k];
                    self.density[b * w + a] >= self.iso
                });
                // Boxes for the collider: runs of full cells along the row.
                match (full, run) {
                    (true, None) => run = Some(i),
                    (false, Some(s)) => {
                        let lo = origin + Vec2::new(s as f32, j as f32) * self.cell;
                        let hi = origin + Vec2::new(i as f32, (j + 1) as f32) * self.cell;
                        mesh.pieces.push(TerrainPiece::Box { centre: (lo + hi) * 0.5, half: (hi - lo) * 0.5 });
                        run = None;
                    }
                    _ => {}
                }
                if i == cols {
                    break;
                }
                let (polys, segs) = self.cell_polygons(i, j);
                // The tile this cell is in: UVs are local to it.
                let t0 = origin + Vec2::new((i / tile * tile) as f32, (j / tile * tile) as f32) * self.cell;
                let uv = |p: Vec2| {
                    let q = (p - t0) / tile_world;
                    [q.x.clamp(0.0, 1.0), 1.0 - q.y.clamp(0.0, 1.0)]
                };
                for poly in polys.iter().filter_map(|p| clean(p)) {
                    let poly = &poly;
                    for k in 1..poly.len().saturating_sub(1) {
                        for p in [poly[0], poly[k], poly[k + 1]] {
                            mesh.fill.push((p, uv(p)));
                        }
                    }
                    if !full {
                        mesh.pieces.push(TerrainPiece::Convex(poly.clone()));
                    }
                }
                for (a, b) in segs {
                    mesh.contour.push((a, b));
                }
            }
        }
        // The edge strip: each contour segment pushed inward (its left).
        let depth = self.edge_width.max(0.0);
        if depth > 0.0 {
            for &(a, b) in &mesh.contour {
                let d = b - a;
                if d.length_squared() < 1e-8 {
                    continue;
                }
                let n = Vec2::new(-d.y, d.x).normalize() * depth;
                let (a2, b2) = (a + n, b + n);
                for (p, uv) in [(a, [0.0, 0.0]), (b, [1.0, 0.0]), (b2, [1.0, 1.0]), (a, [0.0, 0.0]), (b2, [1.0, 1.0]), (a2, [0.0, 1.0])] {
                    mesh.edge.push((p, uv));
                }
            }
        }
        mesh
    }
}

/// A cell polygon fit to collide: points closer than a thousandth of a unit
/// merged (a contour through a corner repeats it), and nothing of almost no
/// area - a convex hull of a line is no shape, and parry refuses it.
fn clean(poly: &[Vec2]) -> Option<Vec<Vec2>> {
    let mut out: Vec<Vec2> = Vec::with_capacity(poly.len());
    for &p in poly {
        if out.last().is_none_or(|q| q.distance_squared(p) > 1e-6) {
            out.push(p);
        }
    }
    while out.len() > 1 && out[0].distance_squared(out[out.len() - 1]) <= 1e-6 {
        out.pop();
    }
    if out.len() < 3 {
        return None;
    }
    let area: f32 = (0..out.len()).map(|k| out[k].perp_dot(out[(k + 1) % out.len()])).sum::<f32>() * 0.5;
    (area > 0.25).then_some(out)
}

/// Give every terrain its density and revision (the first frame). Called
/// from `tick_motion`, so the renderer and the physics find them made.
pub fn tick_smooth_terrain(world: &mut World) {
    for (_, t) in world.query_mut::<&mut SmoothTerrain>() {
        t.ensure();
    }
}

/// `Terrain.edit(e, x, y, radius, amount)`: dig or build round a world point.
pub fn edit(world: &World, entity: Entity, x: f32, y: f32, radius: f32, amount: f32) -> Result<(), String> {
    use crate::ecs::components::{Transform, WorldTransform};
    let wt = {
        let t = world.get::<&Transform>(entity).map_err(|_| "no transform".to_string())?;
        world.get::<&WorldTransform>(entity).ok().map(|w| (*w).clone()).unwrap_or_else(|| WorldTransform::from(&*t))
    };
    let mut terrain = world.get::<&mut SmoothTerrain>(entity).map_err(|_| "no smooth_terrain on that entity".to_string())?;
    // Into the terrain's frame: placed, turned, scaled back.
    let d = Vec2::new(x, y) - wt.position;
    let (sn, cs) = (-wt.rotation).sin_cos();
    let local = Vec2::new(d.x * cs - d.y * sn, d.x * sn + d.y * cs) / wt.scale.abs().max(Vec2::splat(1e-6));
    terrain.edit_local(local, radius / wt.scale.abs().max_element().max(1e-6), amount);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(cols: u32, rows: u32, solid_below: f32) -> SmoothTerrain {
        let mut t = SmoothTerrain { cols, rows, cell: 10.0, border: false, ..Default::default() };
        let (w, h) = (cols as usize + 1, rows as usize + 1);
        let o = t.origin();
        t.density = (0..h)
            .flat_map(|j| (0..w).map(move |i| (i, j)))
            .map(|(_, j)| {
                let y = o.y + j as f32 * 10.0;
                // A ramp across the surface, so the contour cuts the cells.
                (0.5 + (solid_below - y) / 20.0).clamp(0.0, 1.0)
            })
            .collect();
        t.ensure();
        t
    }

    #[test]
    fn a_flat_ground_is_drawn_up_to_its_surface() {
        let t = flat(10, 10, 3.0);
        let m = t.mesh();
        assert!(!m.fill.is_empty());
        let top = m.fill.iter().map(|(p, _)| p.y).fold(f32::MIN, f32::max);
        assert!((top - 3.0).abs() < 1e-3, "the fill stops at the surface: {top}");
        // Contour segments lie on it and the solid is below (on their left
        // going right to left).
        assert!(m.contour.iter().all(|(a, b)| (a.y - 3.0).abs() < 1e-3 && (b.y - 3.0).abs() < 1e-3));
        assert!(m.contour.iter().all(|(a, b)| b.x < a.x), "solid on the left: segments run right to left");
    }

    #[test]
    fn digging_takes_ground_away_and_is_a_new_revision() {
        let mut t = flat(20, 20, 30.0);
        let before = t.rev;
        let area = |m: &TerrainMesh| {
            m.fill.chunks(3).map(|c| ((c[1].0 - c[0].0).perp_dot(c[2].0 - c[0].0)).abs() * 0.5).sum::<f32>()
        };
        let a0 = area(&t.mesh());
        t.edit_local(Vec2::new(0.0, 20.0), 40.0, 1.0);
        assert_ne!(t.rev, before);
        let a1 = area(&t.mesh());
        assert!(a1 < a0 - 1000.0, "a hole dug: {a0} -> {a1}");
        assert!(t.density_at(Vec2::new(0.0, 20.0)) < 0.5, "empty where it was dug");
        t.edit_local(Vec2::new(0.0, 20.0), 40.0, -2.0);
        assert!(t.density_at(Vec2::new(0.0, 20.0)) >= 0.5, "and built back");
    }

    #[test]
    fn every_cell_piece_is_convex() {
        let t = SmoothTerrain { cols: 30, rows: 20, seed: 7, ..Default::default() };
        let mut t = t;
        t.ensure();
        let m = t.mesh();
        let mut n = 0;
        for piece in &m.pieces {
            if let TerrainPiece::Convex(poly) = piece {
                n += 1;
                for k in 0..poly.len() {
                    let (a, b, c) = (poly[k], poly[(k + 1) % poly.len()], poly[(k + 2) % poly.len()]);
                    assert!((b - a).perp_dot(c - b) >= -1e-3, "a reflex corner in {poly:?}");
                }
            }
        }
        assert!(n > 10, "a noise cave has cut cells: {n}");
    }
}
