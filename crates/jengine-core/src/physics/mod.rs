//! Physics system — 2D rigid body physics powered by rapier2d.
//!
//! Wraps the rapier2d pipeline behind a simplified `PhysicsWorld` API.
//! Entity ↔ body mappings are maintained so the ECS and physics world stay in sync.

#[cfg(feature = "physics")]
use crate::detmath::DetMath;
#[cfg(feature = "physics")]
use std::collections::{HashMap, HashSet};

#[cfg(feature = "physics")]
use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(feature = "physics")]
pub mod query;
#[cfg(feature = "physics")]
pub mod ray;

// Re-export component types
pub use crate::ecs::components::{BodyType, Collider2D, ColliderShape, Joint2D, JointType, RigidBody2D};

/// A collision contact event between two entities.
#[derive(Debug, Clone)]
pub struct ContactEvent {
    pub entity_a: u64,
    pub entity_b: u64,
    pub event_type: ContactType,
    /// How hard they hit: the total impulse the solver applied across the
    /// contact this step. Zero for a `Stopped` event, and for a rest
    /// contact that is carrying no load.
    ///
    /// Scripts had no way to ask this at all before Sep 2026 - a collision
    /// said *that* something was hit, never how hard - so a game that
    /// wanted a block to break on a heavy landing and survive a nudge had
    /// to guess from velocities it could only read a frame late.
    pub impulse: f32,
    /// Where they touched, in world units: the deepest contact point.
    pub point: (f32, f32),
}

impl ContactEvent {
    /// A contact with no impulse or position, for tests and for the
    /// runtimes that synthesise events.
    pub fn simple(entity_a: u64, entity_b: u64, event_type: ContactType) -> Self {
        Self { entity_a, entity_b, event_type, impulse: 0.0, point: (0.0, 0.0) }
    }
}

/// Whether a contact started or stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContactType {
    Started,
    Stopped,
}

/// A hash of everything [`collider_builder_for`] builds a shape from, so a
/// caller holding the built shape can tell when it must build it again.
///
/// Building is not free for every shape: a `ConvexDecomposition` (what a
/// `sprite_collider` writes) runs a convex decomposition each time, a few
/// milliseconds a shape - which the browsers' ray snapshot used to pay for
/// every collider, every frame.
#[cfg(feature = "physics")]
pub(crate) fn shape_key(shape: &ColliderShape) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    let floats = |fs: &[f32], mix: &mut dyn FnMut(u64)| {
        mix(fs.len() as u64);
        for f in fs {
            mix(f.to_bits() as u64);
        }
    };
    match shape {
        ColliderShape::Box { width, height } => {
            mix(1);
            floats(&[*width, *height], &mut mix);
        }
        ColliderShape::Circle { radius } => {
            mix(2);
            floats(&[*radius], &mut mix);
        }
        ColliderShape::Capsule { half_height, radius } => {
            mix(3);
            floats(&[*half_height, *radius], &mut mix);
        }
        ColliderShape::Segment { a, b } => {
            mix(4);
            floats(&[a[0], a[1], b[0], b[1]], &mut mix);
        }
        ColliderShape::Triangle { a, b, c } => {
            mix(5);
            floats(&[a[0], a[1], b[0], b[1], c[0], c[1]], &mut mix);
        }
        ColliderShape::ConvexPolygon { vertices } => {
            mix(6);
            floats(vertices.as_flattened(), &mut mix);
        }
        ColliderShape::Polyline { vertices } => {
            mix(7);
            floats(vertices.as_flattened(), &mut mix);
        }
        ColliderShape::HeightField { heights, scale } => {
            mix(8);
            floats(heights, &mut mix);
            floats(scale, &mut mix);
        }
        ColliderShape::ConvexDecomposition { vertices, loops } => {
            mix(9);
            floats(vertices.as_flattened(), &mut mix);
            mix(loops.len() as u64);
            for l in loops {
                mix(*l as u64);
            }
        }
    }
    h
}

/// Build the rapier collider for a [`ColliderShape`].
///
/// Extracted from `add_collider` so the **same** shape construction is used
/// by anything else that needs rapier geometry from an authored component -
/// `ray::RaySnapshot`, which answers `raycast` in the browser runtimes, is
/// the reason it exists. Two places building shapes from the same enum is
/// two places for them to disagree, and a raycast that disagreed with the
/// simulation about where a wall is would be worse than no raycast.
#[cfg(feature = "physics")]
pub(crate) fn collider_builder_for(
    shape: &ColliderShape,
    entity_id: u64,
) -> rapier2d::prelude::ColliderBuilder {
    use rapier2d::prelude::ColliderBuilder;
    match shape {
        ColliderShape::Box { width, height } => {
            ColliderBuilder::cuboid(width / 2.0, height / 2.0)
        }
        ColliderShape::Circle { radius } => ColliderBuilder::ball(*radius),
        ColliderShape::Capsule {
            half_height,
            radius,
        } => ColliderBuilder::capsule_y(*half_height, *radius),
        ColliderShape::Segment { a, b } => ColliderBuilder::segment(
            rapier2d::na::Point2::new(a[0], a[1]),
            rapier2d::na::Point2::new(b[0], b[1]),
        ),
        ColliderShape::Triangle { a, b, c } => ColliderBuilder::triangle(
            rapier2d::na::Point2::new(a[0], a[1]),
            rapier2d::na::Point2::new(b[0], b[1]),
            rapier2d::na::Point2::new(c[0], c[1]),
        ),
        ColliderShape::ConvexPolygon { vertices } => {
            let points: Vec<rapier2d::na::Point2<f32>> =
                vertices.iter().map(|v| rapier2d::na::Point2::new(v[0], v[1])).collect();
            match ColliderBuilder::convex_hull(&points) {
                Some(builder) => builder,
                None => {
                    log::warn!("ConvexPolygon hull failed for entity {} — falling back to unit box", entity_id);
                    ColliderBuilder::cuboid(0.5, 0.5)
                }
            }
        }
        ColliderShape::Polyline { vertices } => {
            let points: Vec<rapier2d::na::Point2<f32>> =
                vertices.iter().map(|v| rapier2d::na::Point2::new(v[0], v[1])).collect();
            ColliderBuilder::polyline(points, None)
        }
        ColliderShape::HeightField { heights, scale } => {
            let dvec = rapier2d::na::DVector::from_vec(heights.clone());
            ColliderBuilder::heightfield(dvec, rapier2d::na::Vector2::new(scale[0], scale[1]))
        }
        ColliderShape::ConvexDecomposition { vertices, loops } => {
            let points: Vec<rapier2d::na::Point2<f32>> =
                vertices.iter().map(|v| rapier2d::na::Point2::new(v[0], v[1])).collect();
            // Edge indices: each loop's consecutive vertex pairs, closed.
            let mut indices: Vec<[u32; 2]> = Vec::with_capacity(vertices.len());
            let mut base = 0u32;
            for l in crate::ecs::components::decomposition_loops(vertices, loops) {
                let n = l.len() as u32;
                indices.extend((0..n).map(|i| [base + i, base + (i + 1) % n]));
                base += n;
            }
            ColliderBuilder::convex_decomposition(&points, &indices)
        }
    }
}

/// What a prediction found (`PhysicsWorld::predict`, read by
/// `ecs::trajectory`): the body's places, the first dynamic body it
/// touched, where it was then, and that body's places after.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Prediction {
    pub path: Vec<[f32; 2]>,
    /// The struck body's physics id.
    pub hit: Option<u64>,
    pub hit_at: Option<[f32; 2]>,
    pub hit_path: Vec<[f32; 2]>,
    /// Steps run: fewer than asked when everything came to rest.
    pub steps: u32,
}

/// One body as the physics had it (`PhysicsWorld::body_state`, saved and
/// restored by `ecs::save_state`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyState {
    pub position: [f32; 2],
    pub rotation: f32,
    /// The rotation as rapier holds it, `(cos, sin)`: what a restore puts
    /// back. Rebuilt from `rotation` it is not always the same bits - an
    /// angle read through `atan2` and turned back through `sin_cos` can land
    /// a step away - and a world put back a step away is a different game
    /// from then on.
    pub turn: [f32; 2],
    pub velocity: [f32; 2],
    pub spin: f32,
}

/// Result of a raycast query.
#[derive(Debug, Clone)]
pub struct RayHit {
    pub entity_id: u64,
    pub point_x: f32,
    pub point_y: f32,
    pub normal_x: f32,
    pub normal_y: f32,
    pub distance: f32,
}

/// Damping factor for joint velocity motors.
///
/// Rapier's `motor_velocity(target, factor)` takes a *damping* factor, not a
/// torque: the constraint's softness is `1 / (dt * factor)`, so a large factor
/// tracks the target speed tightly and the motor's max force is what bounds
/// it. Joints used to pass the authored `motor_max_torque` / `motor_max_force`
/// here, which left the real cap unset (unlimited) and made the "torque" a
/// stiffness knob.
#[cfg(feature = "physics")]
const MOTOR_DAMPING: f32 = 1000.0;

/// `GenericJoint::user_data` of a wheel joint, so `set_motor_speed` drives
/// its rotation rather than its suspension.
#[cfg(feature = "physics")]
const WHEEL_JOINT: u128 = 0x5748_4545_4c00;

/// A spring of `frequency` (Hz) and `damping_ratio` as rapier's
/// acceleration-based motor wants it: stiffness `w^2`, damping `2 z w`.
#[cfg(feature = "physics")]
fn spring_coefficients(frequency: f32, damping_ratio: f32) -> (f32, f32) {
    let w = std::f32::consts::TAU * frequency.max(0.0);
    (w * w, 2.0 * damping_ratio.max(0.0) * w)
}

/// A target joint's kinematic anchor and what it follows.
#[cfg(feature = "physics")]
#[derive(Debug, Clone, Copy)]
struct TargetAnchor {
    entity: u64,
    body: rapier2d::prelude::RigidBodyHandle,
    follow: Option<u64>,
    at: [f32; 2],
}

/// `Collider::user_data` bit marking a one-way platform (`Collider2D::one_way`).
#[cfg(feature = "physics")]
const ONE_WAY: u128 = 1;

/// The static body built for one colliding tilemap, and what it was built
/// from.
///
/// Everything here except the per-chunk tile state is compared *whole* on
/// every sync - it is a handful of integers - and a difference rebuilds the
/// body from scratch. The tiles are the part that can be a million cells, so
/// they are tracked per chunk instead.
#[cfg(feature = "physics")]
struct TilemapBody {
    owner: hecs::Entity,
    body: rapier2d::prelude::RigidBodyHandle,
    /// The transform position, as bits: an exact comparison, no float lint.
    origin: (u32, u32),
    tile_size: (u32, u32),
    /// The grid's shape it was built for - square, diamonds or hexagons,
    /// and a hexagon's side and stagger (`grid_shape_key`).
    shape: u64,
    layers: Vec<TilemapLayerBody>,
    boxes: usize,
}

/// The static body built from a spline shape's fill
/// (`renderer::spline_shape`), and what it was built from.
#[cfg(all(feature = "physics", feature = "spline_shape"))]
struct SplineBody {
    owner: hecs::Entity,
    body: rapier2d::prelude::RigidBodyHandle,
    key: u64,
    pose: [u32; 5],
}

/// The static body built from a carvable sprite's outline (`ecs::carve`),
/// and what it was built from.
#[cfg(feature = "physics")]
struct CarvedBody {
    owner: hecs::Entity,
    body: rapier2d::prelude::RigidBodyHandle,
    rev: u64,
    /// The world transform it was built at, as bits.
    pose: [u32; 5],
}

/// One colliding layer's contribution to a [`TilemapBody`], chunk by chunk.
#[cfg(feature = "physics")]
struct TilemapLayerBody {
    index: usize,
    one_way: bool,
    width: u32,
    height: u32,
    /// The identity of the layer's revision block. A layer *replaced* rather
    /// than edited has the same name, size and a total of zero, so without
    /// this the record would keep colliders for a map that no longer exists.
    ///
    /// [`LayerRevisions::id`]: crate::tilemap::LayerRevisions::id
    rev_id: u64,
    /// The layer's [`LayerRevisions::total`] when it was last checked.
    /// `None` before the first build, so a fresh record builds everything.
    ///
    /// [`LayerRevisions::total`]: crate::tilemap::LayerRevisions::total
    revision: Option<u64>,
    /// Per chunk: the chunk revision its colliders were built at, and the
    /// colliders themselves.
    chunks: Vec<(Option<u32>, Vec<rapier2d::prelude::ColliderHandle>)>,
}

/// The runs of filled cells a tilemap's colliders are built from: merged
/// boxes ([`merge_tiles`]) on a square or isometric grid, and every cell on
/// its own on a hexagonal one - hexagons merge into nothing convex.
#[cfg(feature = "physics")]
pub(crate) fn tile_runs(
    tm: &crate::tilemap::Tilemap,
    width: u32,
    height: u32,
    filled: impl Fn(u32, u32) -> bool,
    rows_only: bool,
) -> Vec<(u32, u32, u32, u32)> {
    if !tm.is_hex() {
        return merge_tiles(width, height, filled, rows_only);
    }
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|&(x, y)| filled(x, y))
        .map(|(x, y)| (x, y, 1, 1))
        .collect()
}

/// What a tilemap's colliders depend on besides its tiles: the grid's shape.
#[cfg(feature = "physics")]
fn grid_shape_key(tm: &crate::tilemap::Tilemap) -> u64 {
    ((tm.orientation as u64) | ((tm.stagger_axis as u64) << 2) | ((tm.stagger_index as u64) << 3) | ((tm.hex_side.to_bits() as u64) << 8))
        ^ tm.shapes_key().rotate_left(41)
}

/// The colliders of the cells `w` x `h` from `(x0, y0)` of a tilemap layer,
/// in the map's local space: whole cells merged into runs
/// ([`tile_runs`] / [`tile_run_collider`]), and each tile whose tileset
/// gives it a shape of its own ([`crate::tilemap::TileShape`]) as that
/// convex outline. The physics sync builds a chunk with it and the
/// browser's ray snapshot a whole layer, so the two cannot disagree.
#[cfg(feature = "physics")]
pub(crate) fn tile_area_colliders(
    tm: &crate::tilemap::Tilemap,
    l: &crate::tilemap::TilemapLayer,
    (x0, y0, w, h): (u32, u32, u32, u32),
    rows_only: bool,
) -> Vec<rapier2d::prelude::ColliderBuilder> {
    use crate::tilemap::CellShape;
    use rapier2d::prelude::*;
    let shaped = tm.has_tile_shapes();
    let whole = |x: u32, y: u32| {
        l.get_tile(x0 + x, y0 + y)
            .is_some_and(|g| !shaped || tm.collision_shape(g, x0 + x, y0 + y) == CellShape::Full)
    };
    let mut out: Vec<ColliderBuilder> = tile_runs(tm, w, h, whole, rows_only)
        .into_iter()
        .filter_map(|(c, r, cw, ch)| tile_run_collider(tm, x0 + c, y0 + r, cw, ch))
        .collect();
    if shaped {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let Some(g) = l.get_tile(x, y) else { continue };
                if let CellShape::Outline(p) = tm.collision_shape(g, x, y) {
                    let pts: Vec<Point<Real>> = p
                        .iter()
                        .map(|&(gc, gr)| {
                            let (lx, ly) = tm.grid_to_local(gc, gr);
                            point![lx, ly]
                        })
                        .collect();
                    if let Some(b) = ColliderBuilder::convex_hull(&pts) {
                        out.push(b);
                    }
                }
            }
        }
    }
    out
}

/// The collider of a run of tilemap cells - `cols` x `rows` from `(col,
/// row)` - in the map's local space: a box on a square grid, a parallelogram
/// of diamonds on an isometric one (`Tilemap::grid_to_local`), and on a
/// hexagonal one the cell's hexagon (hexagons are never merged, so a run is
/// one cell). The physics sync and the browser's ray snapshot both build
/// tiles with it.
#[cfg(feature = "physics")]
pub(crate) fn tile_run_collider(
    tm: &crate::tilemap::Tilemap,
    col: u32,
    row: u32,
    cols: u32,
    rows: u32,
) -> Option<rapier2d::prelude::ColliderBuilder> {
    use rapier2d::prelude::*;
    let (c0, r0) = (col as f32, row as f32);
    let (c1, r1) = (c0 + cols as f32, r0 + rows as f32);
    if tm.is_hex() {
        let corners: Vec<Point<Real>> =
            tm.cell_outline(col as i32, row as i32).into_iter().map(|(x, y)| point![x, y]).collect();
        return ColliderBuilder::convex_hull(&corners);
    }
    if tm.orientation == crate::tilemap::TileOrientation::Isometric {
        let corners: Vec<Point<Real>> = [(c0, r0), (c1, r0), (c1, r1), (c0, r1)]
            .iter()
            .map(|&(gc, gr)| {
                let (x, y) = tm.grid_to_local(gc, gr);
                point![x, y]
            })
            .collect();
        ColliderBuilder::convex_hull(&corners)
    } else {
        let [tw, th] = tm.tile_size;
        let (bw, bh) = (cols as f32 * tw, rows as f32 * th);
        let (cx, cy) = (c0 * tw + bw / 2.0, -(r0 * th + bh / 2.0));
        Some(ColliderBuilder::cuboid(bw / 2.0, bh / 2.0).translation(vector![cx, cy]))
    }
}

/// The colliders of a spline shape (`renderer::spline_shape`), in its
/// entity's frame with `scale` applied: a convex triangle per triangle of its
/// fill. The physics sync and the browser's ray snapshot both build it.
#[cfg(feature = "spline_shape")]
#[cfg(feature = "physics")]
pub(crate) fn spline_colliders(
    shape: &crate::renderer::spline_shape::SplineShape,
    scale: glam::Vec2,
) -> Vec<rapier2d::prelude::ColliderBuilder> {
    use rapier2d::prelude::*;
    let mesh = shape.mesh();
    mesh.triangles
        .iter()
        .filter_map(|t| {
            let pts: Vec<_> = t.iter().map(|&i| mesh.polygon[i] * scale).map(|p| point![p.x, p.y]).collect();
            ColliderBuilder::convex_hull(&pts)
        })
        .collect()
}

/// The colliders of a smooth terrain (`ecs::smooth_terrain`), in its
/// entity's frame with `scale` applied: a box per run of full cells and a
/// convex polygon per cut cell. The physics sync and the browser's ray
/// snapshot both build it.
#[cfg(all(feature = "physics", feature = "smooth_terrain"))]
pub(crate) fn terrain_colliders(
    terrain: &crate::ecs::smooth_terrain::SmoothTerrain,
    scale: glam::Vec2,
) -> Vec<rapier2d::prelude::ColliderBuilder> {
    use crate::ecs::smooth_terrain::TerrainPiece;
    use rapier2d::prelude::*;
    let mesh = terrain.mesh();
    mesh.pieces
        .iter()
        .filter_map(|piece| match piece {
            TerrainPiece::Box { centre, half } => {
                let (c, h) = (*centre * scale, (*half * scale).abs());
                (h.x > 1e-4 && h.y > 1e-4).then(|| ColliderBuilder::cuboid(h.x, h.y).translation(vector![c.x, c.y]))
            }
            TerrainPiece::Convex(poly) => {
                let pts: Vec<_> = poly.iter().map(|p| *p * scale).map(|p| point![p.x, p.y]).collect();
                ColliderBuilder::convex_hull(&pts)
            }
        })
        .collect()
}

/// The colliders of a carvable sprite's solid texels (`ecs::carve`), in its
/// entity's frame with `scale` applied: a convex piece per cell along the
/// outline and boxes inside. The physics sync and the browser's ray snapshot
/// both build carved land with it.
#[cfg(feature = "physics")]
pub(crate) fn carved_colliders(
    c: &crate::ecs::carve::Carvable,
    sprite: &crate::ecs::components::SpriteComponent,
    scale: glam::Vec2,
) -> Vec<rapier2d::prelude::ColliderBuilder> {
    use rapier2d::prelude::*;
    let frame = crate::ecs::carve::TexelFrame::of(sprite, c.state.width, c.state.height);
    let (boxes, polygons) =
        crate::ecs::carve::solid_pieces(c, &frame, |w, h, filled| merge_tiles(w, h, filled, false));
    let mut out = Vec::with_capacity(boxes.len() + polygons.len());
    for poly in polygons {
        let points: Vec<_> = poly.into_iter().map(|p| point![p.x * scale.x, p.y * scale.y]).collect();
        if let Some(b) = ColliderBuilder::convex_hull(&points) {
            out.push(b);
        }
    }
    for (centre, half) in boxes {
        let (hx, hy) = (half.x * scale.x.abs(), half.y * scale.y.abs());
        out.push(ColliderBuilder::cuboid(hx, hy).translation(vector![centre.x * scale.x, centre.y * scale.y]));
    }
    out
}

/// Merge a layer's filled cells into rectangles `(col, row, cols, rows)`:
/// runs along each row, then runs stacked with the same span below them.
/// A flat floor becomes one box, so a character sliding along it meets no
/// seams to catch on. `rows_only` keeps each row separate.
pub fn merge_tiles(width: u32, height: u32, filled: impl Fn(u32, u32) -> bool, rows_only: bool) -> Vec<(u32, u32, u32, u32)> {
    let mut open: Vec<(u32, u32, u32, u32)> = Vec::new();
    let mut done = Vec::new();
    for row in 0..height {
        let mut runs = Vec::new();
        let mut col = 0;
        while col < width {
            if filled(col, row) {
                let start = col;
                while col < width && filled(col, row) {
                    col += 1;
                }
                runs.push((start, col - start));
            } else {
                col += 1;
            }
        }
        let mut next = Vec::new();
        for (start, len) in runs {
            if !rows_only
                && let Some(i) = open.iter().position(|r| r.0 == start && r.2 == len && r.1 + r.3 == row)
            {
                let mut r = open.swap_remove(i);
                r.3 += 1;
                next.push(r);
            } else {
                next.push((start, row, len, 1));
            }
        }
        done.append(&mut open);
        open = next;
    }
    done.append(&mut open);
    done
}

/// The physics world — wraps rapier2d state and provides a simplified API.
///
/// Each frame: [`crate::ecs::systems::ensure_physics_bodies`] to create bodies
/// for new entities and remove those of despawned ones, `step()`, then
/// `sync_transforms_to_ecs()` to write results back.
#[cfg(feature = "physics")]
pub struct PhysicsWorld {
    rigid_body_set: rapier2d::prelude::RigidBodySet,
    collider_set: rapier2d::prelude::ColliderSet,
    physics_pipeline: rapier2d::prelude::PhysicsPipeline,
    island_manager: rapier2d::prelude::IslandManager,
    broad_phase: rapier2d::prelude::DefaultBroadPhase,
    narrow_phase: rapier2d::prelude::NarrowPhase,
    impulse_joint_set: rapier2d::prelude::ImpulseJointSet,
    multibody_joint_set: rapier2d::prelude::MultibodyJointSet,
    ccd_solver: rapier2d::prelude::CCDSolver,
    query_pipeline: rapier2d::prelude::QueryPipeline,
    integration_parameters: rapier2d::prelude::IntegrationParameters,
    gravity: rapier2d::prelude::Vector<f32>,

    // Entity ↔ body/collider mappings
    entity_to_body: HashMap<u64, rapier2d::prelude::RigidBodyHandle>,
    body_to_entity: HashMap<rapier2d::prelude::RigidBodyHandle, u64>,
    entity_to_collider: HashMap<u64, rapier2d::prelude::ColliderHandle>,
    /// Which traced shape (`SpriteCollider::key`) each sprite-shaped
    /// collider was built from, so a changed sprite rebuilds it.
    sprite_shapes: HashMap<u64, u64>,
    /// Mass authored on `RigidBody2D`, for bodies whose collider has not been
    /// added yet.
    ///
    /// Mass belongs to the body, but rapier derives it from the *collider* —
    /// so the value has to survive from `add_body` to the `add_collider` call
    /// that follows it.
    requested_mass: HashMap<u64, f32>,
    /// Joints created with each entity as the first body. A list: a script
    /// can `add_joint` the same entity to several targets, and a single slot
    /// let the second overwrite the first - which kept simulating with nothing
    /// able to remove it.
    entity_to_joint: HashMap<u64, Vec<rapier2d::prelude::ImpulseJointHandle>>,
    /// The ECS entity each body was created for, generation included.
    ///
    /// Bodies are keyed by `entity.id()` - the index alone - because that is
    /// the number scripts and every host command pass around. hecs hands a
    /// despawned entity's index to the next spawn, so the index cannot tell
    /// the dead owner from its successor; this can. Without it a despawned
    /// entity's body stayed in the simulation forever and the next entity to
    /// reuse the index was driven by it. Only bodies created through
    /// [`Self::add_body_for_entity`] have an entry.
    owners: HashMap<u64, hecs::Entity>,

    // Contact tracking for enter/exit detection. A frame's contact events
    // come out sorted by their entity ids (`end_frame`), on every run and
    // platform: read straight out of a `HashSet`, whose seed is random per
    // process, scripts heard `on_collision_enter` in a new order each run -
    // which a game that reacts to the first hit of a frame turns into a
    // different game. The sets themselves only answer "is this pair in it",
    // so they are hash sets; kept as `BTreeSet`s they cost a tenth of a
    // frame over 400 resting boxes (`jengine-benchmark-cpu`).
    prev_contacts: FxHashSet<(u64, u64)>,
    /// Pairs that appeared last step and have not been reported yet.
    ///
    /// A contact is reported one step after rapier first sees it, because
    /// the solver writes the impulse *during* the step that resolves the
    /// pair: read on the step the pair appears, every impact is zero. One
    /// frame of delay buys a collision event that can say how hard it was.
    pending_contacts: FxHashSet<(u64, u64)>,

    /// Where each kinematic body's transform was last frame, and how far it
    /// moved since: what a character standing on it is carried by.
    kinematic_last: HashMap<u64, (f32, f32)>,
    kinematic_delta: HashMap<u64, (f32, f32)>,
    /// The floating origin the bodies last followed ([`Self::follow_origin`]).
    origin_seen: Option<(f64, f64)>,
    /// Colliders removed since the last step. The query pipeline hears of a
    /// removal only when the next step runs, so until then it still holds
    /// their leaves - and a refit before that step reads every leaf it holds.
    /// [`Self::move_characters`] hands these over with its refit.
    removed_since_step: Vec<rapier2d::prelude::ColliderHandle>,
    /// The static body built from each colliding tilemap, and the chunks of
    /// tiles it was built from.
    tilemap_bodies: HashMap<u64, TilemapBody>,
    /// The static body built from each carvable sprite's outline.
    carved_bodies: HashMap<u64, CarvedBody>,
    /// Smooth terrains' bodies (`ecs::smooth_terrain`), as carved ones are.
    #[cfg(feature = "smooth_terrain")]
    terrain_bodies: HashMap<u64, CarvedBody>,
    /// The static body built from each spline shape with a collider.
    #[cfg(feature = "spline_shape")]
    spline_bodies: HashMap<u64, SplineBody>,
    /// The area effects as `sync_area_effects` last read them.
    areas: Vec<crate::ecs::area_effect::PlacedArea>,
    /// Whether the last step left area forces on bodies, so the step after
    /// the last area goes away still clears them.
    area_forces_live: bool,
    /// Chunks rebuilt over this world's life. See
    /// [`PhysicsWorld::tilemap_chunk_rebuilds`].
    tilemap_chunk_rebuilds: u64,

    /// Where each dynamic body was before the most recent step: `(x, y,
    /// angle)`. [`Self::sync_transforms_to_ecs`] blends from here to the
    /// body's current pose by how far the accumulator has run past that step,
    /// so a 60 Hz simulation drawn at 165 Hz moves every frame instead of in
    /// 60 Hz jumps. Refilled at the start of every sub-step.
    prev_poses: HashMap<rapier2d::prelude::RigidBodyHandle, (f32, f32, f32)>,
    /// Forces `apply_force` collected this frame, applied by the next
    /// [`Self::step`] as a push lasting that frame's `dt`. See `apply_force`.
    frame_forces: HashMap<rapier2d::prelude::RigidBodyHandle, rapier2d::na::Vector2<f32>>,
    /// The transform this world last wrote to each dynamic body's entity.
    /// A transform that no longer matches it was moved by someone else - a
    /// script's `set_position` - and [`Self::sync_kinematic_from_ecs`]
    /// teleports the body there instead of overwriting the move.
    written: HashMap<rapier2d::prelude::RigidBodyHandle, (f32, f32, f32)>,
    /// The velocity last written into each dynamic body's `Velocity2D`, so a
    /// component changed since - by a package, which has no physics world to
    /// call - is told apart and handed to the body, as a moved transform is.
    written_velocity: HashMap<rapier2d::prelude::RigidBodyHandle, (f32, f32)>,
    /// Set by [`Self::step`], consumed by [`Self::blend_transforms_for_render`]:
    /// a frame that did not step draws the stepped pose as it is.
    blend_pending: bool,
    /// Set when [`Self::blend_transforms_for_render`] wrote a blended pose,
    /// consumed by [`Self::restore_stepped_poses`] at the next frame's start.
    blended: bool,
    /// Accumulator for fixed-timestep physics stepping.
    accumulator: f32,
    /// Fixed timestep (default 1/60 s).
    fixed_dt: f32,
    /// The rate `fixed_dt` was derived from, kept so `step_hz` reports what
    /// the project asked for rather than `1.0 / (1.0 / hz)`.
    step_hz: f32,
    /// Joints that snap past a force or a torque (`Joint2D::break_force` /
    /// `break_torque`): the entity that made each, its two limits, and the
    /// steps it has lived (see [`BREAK_GRACE_SECONDS`]).
    breakable: HashMap<rapier2d::prelude::ImpulseJointHandle, (u64, f32, f32, u32)>,
    /// Target joints (`JointType::Target`) and the kinematic anchor each one
    /// pulls toward.
    target_joints: HashMap<rapier2d::prelude::ImpulseJointHandle, TargetAnchor>,
    /// Joints broken since the last [`Self::sync_transforms_to_ecs`], which
    /// removes their components and tells their scripts.
    broken_joints: Vec<(u64, f32, f32)>,
    /// The bodies of each limp ragdoll, by its root's id (`ecs::ragdoll`).
    ragdolls: HashMap<u64, RagdollBodies>,
    /// Each limp bone's body, by the bone's id: what `apply_impulse` and
    /// `set_velocity` reach a bone through.
    ragdoll_parts: HashMap<u64, rapier2d::prelude::RigidBodyHandle>,
    /// Impulses and velocities given this frame to entities with no body
    /// yet, by the id the script used (an entity id, or a `spawn_prefab`
    /// handle). A script that spawns a ball and fires it, or sets a skeleton
    /// limp and kicks a bone, in one frame aims at a body built after its
    /// command ran - natively the physics commands go first, before the
    /// spawn is even applied - so a body being built takes these
    /// ([`Self::apply_pending`]); each step clears what is left.
    pending_impulses: HashMap<u64, rapier2d::na::Vector2<f32>>,
    pending_velocity: HashMap<u64, rapier2d::na::Vector2<f32>>,
    /// The frame's contacts so far, between [`Self::begin_frame`] and
    /// [`Self::end_frame`].
    frame_contacts: FrameContacts,
}

/// Pairs seen in any of a frame's sub-steps, and the hardest impulse each
/// produced.
#[cfg(feature = "physics")]
#[derive(Default)]
struct FrameContacts {
    current: FxHashSet<(u64, u64)>,
    strength: FxHashMap<(u64, u64), (f32, (f32, f32))>,
    stepped: u32,
}

/// How long a new breakable joint is left alone. Rapier's joints are soft
/// and start from no impulse, so a weight hung on a new chain drops, is
/// caught and bounces: the load swings to half again its weight in the first
/// tenth of a second and settles over half a second
/// (`a_new_chain_settles_before_it_is_judged`). Judged at once, a chain built
/// to hold its weight snapped as the scene began.
#[cfg(feature = "physics")]
pub const BREAK_GRACE_SECONDS: f32 = 0.5;

/// The bodies of one limp skeleton, parents first.
#[cfg(feature = "physics")]
struct RagdollBodies {
    #[cfg_attr(not(feature = "ragdoll"), allow(dead_code))]
    root: hecs::Entity,
    parts: Vec<RagdollPart>,
}

#[cfg(feature = "physics")]
struct RagdollPart {
    entity: hecs::Entity,
    body: rapier2d::prelude::RigidBodyHandle,
    /// The parent bone's index in `parts`.
    parent: Option<usize>,
    /// The bone's world scale when it went limp, kept as it was.
    scale: glam::Vec2,
}

/// A ragdoll part's collider `user_data` carries this bit, and its ragdoll's
/// root id plus one above bit 8: [`RagdollHooks`] keeps the parts of one
/// ragdoll from colliding with each other.
#[cfg(feature = "physics")]
const RAGDOLL_PART: u128 = 2;

/// A conveyor's collider `user_data` carries this bit, and its speed's f32
/// bits in the top 32 (`Conveyor`).
#[cfg(feature = "physics")]
const CONVEYOR: u128 = 4;

/// The step's contact hooks. The filter: no contact between two parts of
/// one ragdoll (a shoulder's capsule overlaps the chest at rest). The
/// modifier: a conveyor's contacts slide along it at its speed. Each is
/// asked only about pairs where a collider opted in.
#[cfg(feature = "physics")]
struct StepHooks;

#[cfg(feature = "physics")]
impl rapier2d::prelude::PhysicsHooks for StepHooks {
    fn modify_solver_contacts(&self, context: &mut rapier2d::prelude::ContactModificationContext) {
        let belt = |h| {
            let c: &rapier2d::prelude::Collider = context.colliders.get(h)?;
            (c.user_data & CONVEYOR != 0)
                .then(|| (f32::from_bits((c.user_data >> 96) as u32), c.position().rotation.angle()))
        };
        // The tangent velocity is the first collider's surface against the
        // second's (measured: a crate on a belt that is the second is
        // carried backwards otherwise), so a belt that is the second slides
        // the other way round.
        let (speed, angle, sign) = match (belt(context.collider1), belt(context.collider2)) {
            (Some((s, a)), _) => (s, a, 1.0),
            (None, Some((s, a))) => (s, a, -1.0),
            _ => return,
        };
        let v = rapier2d::na::Vector2::new(angle.dcos(), angle.dsin()) * (speed * sign);
        for c in context.solver_contacts.iter_mut() {
            c.tangent_velocity = v;
        }
    }

    fn filter_contact_pair(
        &self,
        context: &rapier2d::prelude::PairFilterContext,
    ) -> Option<rapier2d::prelude::SolverFlags> {
        let tag = |h| context.colliders.get(h).map_or(0, |c: &rapier2d::prelude::Collider| c.user_data);
        let (a, b) = (tag(context.collider1), tag(context.collider2));
        if a & RAGDOLL_PART != 0 && b & RAGDOLL_PART != 0 && a >> 8 == b >> 8 {
            None
        } else {
            Some(rapier2d::prelude::SolverFlags::COMPUTE_IMPULSES)
        }
    }
}

#[cfg(feature = "physics")]
impl PhysicsWorld {
    /// Create a new physics world with the given gravity and length unit.
    ///
    /// `gravity_x` / `gravity_y` are in **m/s²** (Earth ≈ -9.81 for Y).
    /// `length_unit` tells Rapier how many game-units equal 1 meter.
    /// Gravity is internally scaled by `length_unit` so that physics behaves
    /// correctly regardless of coordinate scale.
    pub fn new(gravity_x: f32, gravity_y: f32, length_unit: f32) -> Self {
        let integration_parameters = rapier2d::prelude::IntegrationParameters {
            length_unit,
            ..Default::default()
        };
        Self {
            rigid_body_set: rapier2d::prelude::RigidBodySet::new(),
            collider_set: rapier2d::prelude::ColliderSet::new(),
            physics_pipeline: rapier2d::prelude::PhysicsPipeline::new(),
            island_manager: rapier2d::prelude::IslandManager::new(),
            broad_phase: rapier2d::prelude::DefaultBroadPhase::new(),
            narrow_phase: rapier2d::prelude::NarrowPhase::new(),
            impulse_joint_set: rapier2d::prelude::ImpulseJointSet::new(),
            multibody_joint_set: rapier2d::prelude::MultibodyJointSet::new(),
            ccd_solver: rapier2d::prelude::CCDSolver::new(),
            query_pipeline: rapier2d::prelude::QueryPipeline::new(),
            integration_parameters,
            gravity: rapier2d::na::Vector2::new(gravity_x * length_unit, gravity_y * length_unit),
            entity_to_body: HashMap::new(),
            body_to_entity: HashMap::new(),
            entity_to_collider: HashMap::new(),
            sprite_shapes: HashMap::new(),
            requested_mass: HashMap::new(),
            entity_to_joint: HashMap::new(),
            owners: HashMap::new(),
            prev_contacts: FxHashSet::default(),
            pending_contacts: FxHashSet::default(),
            kinematic_last: HashMap::new(),
            kinematic_delta: HashMap::new(),
            origin_seen: None,
            removed_since_step: Vec::new(),
            tilemap_bodies: HashMap::new(),
            carved_bodies: HashMap::new(),
            #[cfg(feature = "smooth_terrain")]
            terrain_bodies: HashMap::new(),
            #[cfg(feature = "spline_shape")]
            spline_bodies: HashMap::new(),
            areas: Vec::new(),
            area_forces_live: false,
            tilemap_chunk_rebuilds: 0,
            prev_poses: HashMap::new(),
            frame_forces: HashMap::new(),
            written: HashMap::new(),
            written_velocity: HashMap::new(),
            blend_pending: false,
            blended: false,
            accumulator: 0.0,
            fixed_dt: 1.0 / 60.0,
            step_hz: 60.0,
            breakable: HashMap::new(),
            target_joints: HashMap::new(),
            broken_joints: Vec::new(),
            ragdolls: HashMap::new(),
            ragdoll_parts: HashMap::new(),
            pending_impulses: HashMap::new(),
            pending_velocity: HashMap::new(),
            frame_contacts: FrameContacts::default(),
        }
    }

    /// How many fixed steps the simulation runs per second. 60 unless the
    /// project chose otherwise (`physics.step_hz` in `project.jengine`).
    pub fn step_hz(&self) -> f32 {
        self.step_hz
    }

    /// Choose the fixed step rate. Clamped to 10..=1000 Hz: below that a
    /// frame's four-step cap cannot keep up with real time, above it the
    /// solver's cost per frame is the whole budget. The accumulator and the
    /// blend (`blend_transforms_for_render`) follow the new step size.
    pub fn set_step_hz(&mut self, hz: f32) {
        let hz = if hz.is_finite() { hz.clamp(10.0, 1000.0) } else { 60.0 };
        self.step_hz = hz;
        self.fixed_dt = 1.0 / hz;
        self.accumulator = self.accumulator.min(self.fixed_dt * 4.0);
    }

    /// The same choice as seconds per step, which is how `project.jengine`
    /// spells it (`physics.fixed_timestep`, the field the editor's Project
    /// Settings edits). Zero, negative or non-finite means the default.
    pub fn set_fixed_timestep(&mut self, seconds: f32) {
        let hz = if seconds.is_finite() && seconds > 0.0 { 1.0 / seconds } else { 60.0 };
        // `0.016666` reads back as 60.0002 Hz and `1.0 / 240.0` as 239.99998;
        // a rate that is a whole number to within float noise is that number.
        let rounded = hz.round();
        let hz = if (hz - rounded).abs() < 0.01 { rounded } else { hz };
        self.set_step_hz(hz);
    }

    /// Make every `Conveyor`'s collider slide what rests on it, at the
    /// component's speed now - a changed speed or a rebuilt collider is
    /// picked up here. One compare per conveyor when nothing changed.
    pub fn sync_conveyors(&mut self, world: &hecs::World) {
        use rapier2d::prelude::ActiveHooks;
        for (entity, conveyor) in world.query::<&crate::ecs::components::Conveyor>().iter() {
            let Some(&handle) = self.entity_to_collider.get(&(entity.id() as u64)) else { continue };
            let Some(c) = self.collider_set.get_mut(handle) else { continue };
            let want = (c.user_data & ((1u128 << 96) - 1)) | CONVEYOR | ((conveyor.speed.to_bits() as u128) << 96);
            if c.user_data != want {
                c.user_data = want;
                c.set_active_hooks(c.active_hooks() | ActiveHooks::MODIFY_SOLVER_CONTACTS);
            }
        }
    }

    /// A `RigidBody2D`'s settings, changed after its body was built, given
    /// to the body: gravity scale, both dampings, fixed rotation and CCD.
    /// They were read once, by [`Self::add_body`], so a package switching a
    /// vehicle's angular damping between "on the ground" and "in the air"
    /// changed a number nothing read again. Compared against the body
    /// itself, so a frame with nothing changed writes nothing.
    pub fn sync_body_settings(&mut self, world: &hecs::World) {
        use rapier2d::prelude::LockedAxes;
        for (entity, rb) in world.query::<&RigidBody2D>().iter() {
            let Some(&handle) = self.entity_to_body.get(&(entity.id() as u64)) else { continue };
            let Some(body) = self.rigid_body_set.get_mut(handle) else { continue };
            if body.gravity_scale() != rb.gravity_scale {
                body.set_gravity_scale(rb.gravity_scale, true);
            }
            if body.linear_damping() != rb.linear_damping {
                body.set_linear_damping(rb.linear_damping);
            }
            if body.angular_damping() != rb.angular_damping {
                body.set_angular_damping(rb.angular_damping);
            }
            let locked = if rb.fixed_rotation { LockedAxes::ROTATION_LOCKED } else { LockedAxes::empty() };
            if body.locked_axes() != locked {
                body.set_locked_axes(locked, true);
            }
            if body.is_ccd_enabled() != rb.ccd {
                body.enable_ccd(rb.ccd);
            }
        }
    }

    /// Whether a collider has been attached under `entity_id`.
    pub fn has_collider(&self, entity_id: u64) -> bool {
        self.entity_to_collider.contains_key(&entity_id)
    }

    /// Take `entity_id`'s collider off its body, leaving the body - so the
    /// next `ensure_physics_bodies` attaches the collider the entity has now.
    pub fn remove_collider(&mut self, entity_id: u64) {
        self.sprite_shapes.remove(&entity_id);
        if let Some(handle) = self.entity_to_collider.remove(&entity_id) {
            self.removed_since_step.push(handle);
            self.collider_set.remove(handle, &mut self.island_manager, &mut self.rigid_body_set, true);
        }
    }

    /// The `SpriteCollider::key` of the traced shape `entity_id`'s collider
    /// was built from, if it was one.
    pub fn sprite_shape_key(&self, entity_id: u64) -> Option<u64> {
        self.sprite_shapes.get(&entity_id).copied()
    }

    /// Record that `entity_id`'s collider is the traced shape `key`.
    pub fn set_sprite_shape_key(&mut self, entity_id: u64, key: u64) {
        self.sprite_shapes.insert(entity_id, key);
    }

    /// Check if an entity already has a physics body.
    pub fn has_body(&self, entity_id: u64) -> bool {
        self.entity_to_body.contains_key(&entity_id)
    }

    /// Add a rigid body for an entity with the given component and initial position/rotation.
    pub fn add_body(
        &mut self,
        entity_id: u64,
        rb: &RigidBody2D,
        pos_x: f32,
        pos_y: f32,
        rotation: f32,
    ) {
        use rapier2d::prelude::*;

        // A second body under the same id would orphan the first: the maps
        // would point at the new handle and the old body would simulate, and
        // collide, with nothing able to reach it.
        if self.entity_to_body.contains_key(&entity_id) {
            self.remove_body(entity_id);
        }

        let body_type = match rb.body_type {
            BodyType::Dynamic => RigidBodyType::Dynamic,
            BodyType::Static => RigidBodyType::Fixed,
            BodyType::Kinematic => RigidBodyType::KinematicPositionBased,
        };

        let rigid_body = RigidBodyBuilder::new(body_type)
            .translation(rapier2d::na::Vector2::new(pos_x, pos_y))
            .rotation(rotation)
            .gravity_scale(rb.gravity_scale)
            .linear_damping(rb.linear_damping)
            .angular_damping(rb.angular_damping)
            .locked_axes(if rb.fixed_rotation {
                LockedAxes::ROTATION_LOCKED
            } else {
                LockedAxes::empty()
            })
            // The one line that makes the `CCDSolver` this world has
            // always owned do anything at all. Rapier sweeps only bodies
            // that opt in, so before this the solver ran on an empty set
            // every step and a fast body went through walls.
            .ccd_enabled(rb.ccd)
            .can_sleep(rb.can_sleep)
            .build();

        let handle = self.rigid_body_set.insert(rigid_body);
        self.entity_to_body.insert(entity_id, handle);
        self.body_to_entity.insert(handle, entity_id);
        // `RigidBody2D.mass` used to be read by nothing at all: rapier derives
        // mass from collider density x area, so a 40x40 crate authored
        // `mass: 1.0` actually weighed about 1600 and an impulse sized for it
        // did nothing visible (BUGS #50). Stashed here because the collider is
        // where rapier wants it, and that arrives on the next call.
        if rb.mass > 0.0 {
            self.requested_mass.insert(entity_id, rb.mass);
        } else {
            self.requested_mass.remove(&entity_id);
        }
    }

    /// [`Self::add_body`] for an ECS entity, remembering which entity owns it
    /// so [`Self::remove_stale_bodies`] can tell when it is gone.
    ///
    /// This is the path the runtimes take (through
    /// [`crate::ecs::systems::ensure_physics_bodies`]); plain `add_body` is for
    /// callers with no world, and its bodies are never swept.
    pub fn add_body_for_entity(
        &mut self,
        entity: hecs::Entity,
        rb: &RigidBody2D,
        pos_x: f32,
        pos_y: f32,
        rotation: f32,
    ) {
        let entity_id = entity.id() as u64;
        self.add_body(entity_id, rb, pos_x, pos_y, rotation);
        self.owners.insert(entity_id, entity);
    }

    /// Remove every body whose owning entity has been despawned or no longer
    /// carries a [`RigidBody2D`]. Returns how many were removed.
    ///
    /// Despawning is done all over the engine - script commands, lifetimes,
    /// hitboxes, bounds, behavior graphs, three hosts - and none of it knows
    /// about physics, so rather than chase every site this reconciles once a
    /// frame, before bodies are created. Comparing the whole `Entity` rather
    /// than the index is what makes a reused index read as a new owner.
    pub fn remove_stale_bodies(&mut self, world: &hecs::World) -> usize {
        // Almost always nothing is stale; the `Vec` this collected every
        // frame is only built when something is.
        let mut stale: Vec<u64> = Vec::new();
        for (id, owner) in &self.owners {
            if !world.satisfies::<&RigidBody2D>(*owner).unwrap_or(false) {
                stale.push(*id);
            }
        }
        // In id order, not the map's: which slot rapier frees first decides
        // where later bodies go, and with them the solver's order.
        stale.sort_unstable();
        for id in &stale {
            self.remove_body(*id);
        }
        stale.len()
    }

    /// Whether `entity` - generation included - is the one that owns the body
    /// stored under its index. Bodies added without an owner match any entity.
    fn owned_by(&self, entity_id: u64, entity: hecs::Entity) -> bool {
        self.owners.get(&entity_id).is_none_or(|owner| *owner == entity)
    }

    /// Add a collider for an entity, attached to its existing body.
    pub fn add_collider(&mut self, entity_id: u64, collider: &Collider2D) {
        use rapier2d::prelude::{ActiveEvents, Group, InteractionGroups};

        let Some(&body_handle) = self.entity_to_body.get(&entity_id) else {
            log::warn!("Cannot add collider — entity {} has no body", entity_id);
            return;
        };

        let shape = collider_builder_for(&collider.shape, entity_id);

        let built = shape
            .translation(rapier2d::na::Vector2::new(collider.offset[0], collider.offset[1]))
            .sensor(collider.is_sensor)
            .friction(collider.friction)
            .restitution(collider.restitution);
        // An authored `RigidBody2D.mass` wins over the collider's density.
        //
        // `ColliderBuilder::mass` scales the shape's computed mass properties
        // to that total, so angular inertia stays proportional to the shape —
        // which `additional_mass` on the body would not do, and a body with
        // zero inertia spins freely from the lightest touch.
        //
        // `mass <= 0` means "let the shape decide", which is rapier's own
        // model and the way to author a body whose weight should follow its
        // size.
        let built = match self.requested_mass.get(&entity_id).copied() {
            Some(m) if m > 0.0 => built.mass(m),
            _ => built.density(collider.density),
        };
        let built = built
            .collision_groups(InteractionGroups::new(
                Group::from_bits_truncate(collider.collision_membership as u32),
                Group::from_bits_truncate(collider.collision_filter as u32),
            ))
            .active_events(ActiveEvents::COLLISION_EVENTS)
            .user_data(if collider.one_way { ONE_WAY } else { 0 })
            .build();

        let handle =
            self.collider_set
                .insert_with_parent(built, body_handle, &mut self.rigid_body_set);
        self.entity_to_collider.insert(entity_id, handle);
    }

    /// Add a joint between two entities. A target joint pulls toward
    /// `entity_b` rather than joining it, so `entity_b` needs no body there.
    pub fn add_joint(&mut self, entity_a: u64, entity_b: u64, joint: &Joint2D) {
        use rapier2d::prelude::{
            FixedJointBuilder, GenericJoint, GenericJointBuilder, JointAxesMask, JointAxis,
            PrismaticJointBuilder, RevoluteJointBuilder, RopeJointBuilder, SpringJointBuilder,
        };

        if let JointType::Target { target, .. } = &joint.joint_type {
            self.add_target_joint(entity_a, joint, Some(entity_b), *target);
            return;
        }
        let Some(&body_a) = self.entity_to_body.get(&entity_a) else {
            log::warn!("Cannot add joint — entity {} has no body", entity_a);
            return;
        };
        let Some(&body_b) = self.entity_to_body.get(&entity_b) else {
            log::warn!("Cannot add joint — entity {} has no body", entity_b);
            return;
        };

        let point = |p: &[f32; 2]| rapier2d::na::Point2::new(p[0], p[1]);
        let unlimited = |m: f32| if m > 0.0 { m } else { f32::MAX };
        // Most joints put this entity first. A wheel puts the chassis first,
        // so its suspension axis is in the chassis's frame - the wheel turns.
        let mut chassis_first = false;
        let joint_params: GenericJoint = match &joint.joint_type {
            JointType::Wheel {
                anchor,
                anchor_b,
                axis,
                frequency,
                damping_ratio,
                limits,
                motor_speed,
                motor_max_torque,
            } => {
                chassis_first = true;
                let axis = rapier2d::na::Unit::new_normalize(rapier2d::na::Vector2::new(axis[0], axis[1]));
                let (k, c) = spring_coefficients(*frequency, *damping_ratio);
                let mut j = GenericJointBuilder::new(JointAxesMask::LIN_Y)
                    .local_axis1(axis)
                    .local_axis2(axis)
                    .local_anchor1(point(anchor_b))
                    .local_anchor2(point(anchor))
                    .motor_position(JointAxis::LinX, 0.0, k, c)
                    .user_data(WHEEL_JOINT);
                if let Some([min, max]) = limits {
                    j = j.limits(JointAxis::LinX, [*min, *max]);
                }
                if *motor_speed != 0.0 || *motor_max_torque != 0.0 {
                    j = j
                        // Negated: the chassis is first here, and a revolute
                        // wheel (wheel first) rolls right on a positive speed.
                        .motor_velocity(JointAxis::AngX, -*motor_speed, MOTOR_DAMPING)
                        .motor_max_force(JointAxis::AngX, *motor_max_torque);
                }
                j.build()
            }
            JointType::Motor { linear_offset, angular_offset, max_force, max_torque, frequency, damping_ratio } => {
                // The target first: the offset is in its frame.
                chassis_first = true;
                let (k, c) = spring_coefficients(*frequency, *damping_ratio);
                let (f, t) = (unlimited(*max_force), unlimited(*max_torque));
                GenericJointBuilder::new(JointAxesMask::empty())
                    .local_anchor1(point(linear_offset))
                    .motor_position(JointAxis::LinX, 0.0, k, c)
                    .motor_max_force(JointAxis::LinX, f)
                    .motor_position(JointAxis::LinY, 0.0, k, c)
                    .motor_max_force(JointAxis::LinY, f)
                    .motor_position(JointAxis::AngX, *angular_offset, k, c)
                    .motor_max_force(JointAxis::AngX, t)
                    .build()
            }
            JointType::Friction { anchor_a, anchor_b, max_force, max_torque } => {
                // Every axis free, each with a motor that wants no relative
                // motion and is allowed only so much force to get it.
                GenericJointBuilder::new(JointAxesMask::empty())
                    .local_anchor1(point(anchor_a))
                    .local_anchor2(point(anchor_b))
                    .motor_velocity(JointAxis::LinX, 0.0, MOTOR_DAMPING)
                    .motor_max_force(JointAxis::LinX, max_force.max(0.0))
                    .motor_velocity(JointAxis::LinY, 0.0, MOTOR_DAMPING)
                    .motor_max_force(JointAxis::LinY, max_force.max(0.0))
                    .motor_velocity(JointAxis::AngX, 0.0, MOTOR_DAMPING)
                    .motor_max_force(JointAxis::AngX, max_torque.max(0.0))
                    .build()
            }
            JointType::Target { .. } => unreachable!("handled above"),
            JointType::Revolute { anchor, anchor_b, limits, motor_speed, motor_max_torque } => {
                let mut j = RevoluteJointBuilder::new()
                    .local_anchor1(rapier2d::na::Point2::new(anchor[0], anchor[1]))
                    .local_anchor2(rapier2d::na::Point2::new(anchor_b[0], anchor_b[1]));
                if let Some([min, max]) = limits {
                    j = j.limits([*min, *max]);
                }
                if *motor_speed != 0.0 || *motor_max_torque != 0.0 {
                    j = j
                        .motor_velocity(*motor_speed, MOTOR_DAMPING)
                        .motor_max_force(*motor_max_torque);
                }
                j.build().into()
            }
            JointType::Fixed { anchor_a, anchor_b } => {
                FixedJointBuilder::new()
                    .local_anchor1(rapier2d::na::Point2::new(anchor_a[0], anchor_a[1]))
                    .local_anchor2(rapier2d::na::Point2::new(anchor_b[0], anchor_b[1]))
                    .build()
                    .into()
            }
            JointType::Prismatic { anchor, anchor_b, axis, limits, motor_speed, motor_max_force } => {
                let unit_axis = rapier2d::na::Unit::new_normalize(
                    rapier2d::na::Vector2::new(axis[0], axis[1]),
                );
                let mut j = PrismaticJointBuilder::new(unit_axis)
                    .local_anchor1(rapier2d::na::Point2::new(anchor[0], anchor[1]))
                    .local_anchor2(rapier2d::na::Point2::new(anchor_b[0], anchor_b[1]));
                if let Some([min, max]) = limits {
                    j = j.limits([*min, *max]);
                }
                if *motor_speed != 0.0 || *motor_max_force != 0.0 {
                    j = j
                        .motor_velocity(*motor_speed, MOTOR_DAMPING)
                        .motor_max_force(*motor_max_force);
                }
                j.build().into()
            }
            JointType::Rope { anchor_a, anchor_b, max_distance } => {
                RopeJointBuilder::new(*max_distance)
                    .local_anchor1(rapier2d::na::Point2::new(anchor_a[0], anchor_a[1]))
                    .local_anchor2(rapier2d::na::Point2::new(anchor_b[0], anchor_b[1]))
                    .build()
                    .into()
            }
            JointType::Spring { anchor_a, anchor_b, rest_length, stiffness, damping } => {
                SpringJointBuilder::new(*rest_length, *stiffness, *damping)
                    .local_anchor1(rapier2d::na::Point2::new(anchor_a[0], anchor_a[1]))
                    .local_anchor2(rapier2d::na::Point2::new(anchor_b[0], anchor_b[1]))
                    .build()
                    .into()
            }
        };

        let mut generic = joint_params;
        if joint.collide_connected {
            generic.set_contacts_enabled(true);
        } else {
            generic.set_contacts_enabled(false);
        }

        let (first, second) = if chassis_first { (body_b, body_a) } else { (body_a, body_b) };
        let handle = self.impulse_joint_set.insert(first, second, generic, true);
        self.entity_to_joint.entry(entity_a).or_default().push(handle);
        if joint.break_force > 0.0 || joint.break_torque > 0.0 {
            self.breakable.insert(handle, (entity_a, joint.break_force, joint.break_torque, 0));
        }
    }

    /// A target joint (`JointType::Target`): the entity's `anchor` point is
    /// pulled toward a kinematic body of the joint's own, which
    /// [`Self::sync_joint_targets`] moves to the entity named by `follow`
    /// (or to `at`) before each step.
    pub fn add_target_joint(&mut self, entity: u64, joint: &Joint2D, follow: Option<u64>, at: [f32; 2]) {
        use rapier2d::prelude::{GenericJoint, GenericJointBuilder, JointAxesMask, JointAxis, RigidBodyBuilder};
        let JointType::Target { anchor, frequency, damping_ratio, max_force, .. } = &joint.joint_type else {
            return;
        };
        let Some(&body) = self.entity_to_body.get(&entity) else {
            return;
        };
        // Start on the pulled point, so nothing is yanked before the first
        // sync moves the anchor where it belongs.
        let local = rapier2d::na::Point2::new(anchor[0], anchor[1]);
        let start = self.rigid_body_set.get(body).map(|b| b.position() * local).map(|p| [p.x, p.y]).unwrap_or(at);
        let anchor_body = self.rigid_body_set.insert(
            RigidBodyBuilder::kinematic_position_based()
                .translation(rapier2d::na::Vector2::new(start[0], start[1]))
                .build(),
        );
        let (k, c) = spring_coefficients(*frequency, *damping_ratio);
        let max = if *max_force > 0.0 { *max_force } else { f32::MAX };
        let mut generic: GenericJoint = GenericJointBuilder::new(JointAxesMask::empty())
            .local_anchor2(local)
            .motor_position(JointAxis::LinX, 0.0, k, c)
            .motor_max_force(JointAxis::LinX, max)
            .motor_position(JointAxis::LinY, 0.0, k, c)
            .motor_max_force(JointAxis::LinY, max)
            .build();
        generic.set_contacts_enabled(false);
        let handle = self.impulse_joint_set.insert(anchor_body, body, generic, true);
        self.entity_to_joint.entry(entity).or_default().push(handle);
        self.target_joints.insert(handle, TargetAnchor { entity, body: anchor_body, follow, at });
        if joint.break_force > 0.0 {
            self.breakable.insert(handle, (entity, joint.break_force, joint.break_torque, 0));
        }
    }

    /// The entities target joints follow, for [`Self::sync_joint_targets`]'s
    /// lookup.
    pub fn target_follows(&self) -> Vec<u64> {
        self.target_joints.values().filter_map(|t| t.follow).collect()
    }

    /// Move every target joint's anchor to where it pulls: the followed
    /// entity's position as `position_of` gives it, else the joint's fixed
    /// point. A body asleep wakes when its target moves. Anchors whose joint
    /// is gone (removed, broken, or its body removed) are dropped.
    pub fn sync_joint_targets(&mut self, position_of: impl Fn(u64) -> Option<[f32; 2]>) {
        let joints = &self.impulse_joint_set;
        let gone: Vec<_> = self.target_joints.keys().filter(|h| joints.get(**h).is_none()).copied().collect();
        for h in gone {
            if let Some(t) = self.target_joints.remove(&h) {
                self.rigid_body_set.remove(
                    t.body,
                    &mut self.island_manager,
                    &mut self.collider_set,
                    &mut self.impulse_joint_set,
                    &mut self.multibody_joint_set,
                    true,
                );
            }
        }
        for t in self.target_joints.values() {
            let at = t.follow.and_then(&position_of).unwrap_or(t.at);
            let Some(anchor) = self.rigid_body_set.get_mut(t.body) else { continue };
            let to = rapier2d::na::Vector2::new(at[0], at[1]);
            let moved = (anchor.translation() - to).norm_squared() > 1e-6;
            anchor.set_next_kinematic_translation(to);
            if moved && let Some(&b) = self.entity_to_body.get(&t.entity) && let Some(body) = self.rigid_body_set.get_mut(b) {
                body.wake_up(true);
            }
        }
    }

    /// The load on the entity's joints over the last step: the largest force
    /// and the largest torque any of them carried, in the units
    /// `break_force` and `break_torque` are given in. `None` with no joint.
    pub fn joint_load(&self, entity_id: u64) -> Option<(f32, f32)> {
        let handles = self.entity_to_joint.get(&entity_id)?;
        let mut load = (0.0f32, 0.0f32);
        for h in handles {
            if let Some(j) = self.impulse_joint_set.get(*h) {
                let (f, t) = self.joint_load_of(j);
                load = (load.0.max(f), load.1.max(t));
            }
        }
        Some(load)
    }

    /// One joint's force and torque over the last step.
    ///
    /// Rapier keeps the impulse each constraint row applied in the step's
    /// **last solver substep**, so it is a force over that substep's length -
    /// over the whole step it read a quarter of the weight hanging from it.
    /// The force is the pin's two linear rows: exact for a hinge swinging
    /// free, a rope or a spring (`a_joint_carries_the_weight_hanging_from_it`).
    /// The torque is what a hinge's **limit and motor** carry, exact at a
    /// limit (`a_hinge_at_its_limit_carries_the_torque`). Rapier orthogonalises
    /// a joint's rows before solving, so a locked angle's own row is not a
    /// torque (a fixed joint holding a bar out reads 0 there) and a hinge
    /// resting on its limit carries part of its weight in the limit row -
    /// which is why the two are kept apart rather than summed.
    fn joint_load_of(&self, j: &rapier2d::prelude::ImpulseJoint) -> (f32, f32) {
        use rapier2d::prelude::JointAxis;
        let substeps = self.integration_parameters.num_solver_iterations.get() as f32;
        let dt = (self.fixed_dt / substeps).max(1e-6);
        let force = (j.impulses.x * j.impulses.x + j.impulses.y * j.impulses.y).sqrt() / dt;
        let ang = JointAxis::AngX as usize;
        let torque = (j.data.limits[ang].impulse.abs() + j.data.motors[ang].impulse.abs()) / dt;
        (force, torque)
    }

    /// Remove every breakable joint loaded past its limit by the step just
    /// run, and note it for [`Self::sync_transforms_to_ecs`].
    fn break_joints(&mut self) {
        let mut snapped = Vec::new();
        let grace = (BREAK_GRACE_SECONDS / self.fixed_dt.max(1e-6)).ceil() as u32;
        let mut breakable = std::mem::take(&mut self.breakable);
        for (&h, entry) in breakable.iter_mut() {
            let (owner, force, torque, age) = *entry;
            if age < grace {
                entry.3 += 1;
                continue;
            }
            let Some(j) = self.impulse_joint_set.get(h) else { continue };
            let (f, t) = self.joint_load_of(j);
            if (force > 0.0 && f > force) || (torque > 0.0 && t > torque) {
                snapped.push((h, owner, force, torque));
            }
        }
        self.breakable = breakable;
        // In handle order, not the map's: the order joints come out of
        // rapier, and `on_joint_break` reaches scripts, is the same everywhere.
        snapped.sort_unstable_by_key(|s| s.0.0.into_raw_parts());
        for (h, owner, force, torque) in snapped {
            self.breakable.remove(&h);
            self.impulse_joint_set.remove(h, true);
            if let Some(list) = self.entity_to_joint.get_mut(&owner) {
                list.retain(|x| *x != h);
                if list.is_empty() {
                    self.entity_to_joint.remove(&owner);
                }
            }
            self.broken_joints.push((owner, force, torque));
        }
    }

    /// Remove every joint the entity created.
    pub fn remove_joint(&mut self, entity_id: u64) {
        for handle in self.entity_to_joint.remove(&entity_id).unwrap_or_default() {
            self.impulse_joint_set.remove(handle, true);
            self.breakable.remove(&handle);
            if let Some(t) = self.target_joints.remove(&handle) {
                self.rigid_body_set.remove(
                    t.body,
                    &mut self.island_manager,
                    &mut self.collider_set,
                    &mut self.impulse_joint_set,
                    &mut self.multibody_joint_set,
                    true,
                );
            }
        }
    }

    /// Check if an entity already has a joint.
    pub fn has_joint(&self, entity_id: u64) -> bool {
        self.entity_to_joint.get(&entity_id).is_some_and(|j| !j.is_empty())
    }

    /// How many joints the entity created. For tests and diagnostics.
    pub fn joint_count(&self, entity_id: u64) -> usize {
        self.entity_to_joint.get(&entity_id).map_or(0, Vec::len)
    }

    /// Set the motor target velocity on the entity's joints (revolute or
    /// prismatic), keeping each motor's max force.
    ///
    /// The motor goes on whichever axis the joint leaves free: rotation for a
    /// revolute joint, the slide axis for a prismatic one. A fixed, rope or
    /// spring joint has no free axis to drive and is left alone.
    ///
    /// This used to motorise both axes on every call (its `max_force > 0`
    /// test is always true - rapier defaults the field to `f32::MAX`) and pass
    /// that max force as the damping factor. It happened to work, because
    /// rapier ignores a motor on a locked axis, but it only did so by accident.
    pub fn set_motor_speed(&mut self, entity_id: u64, speed: f32) {
        use rapier2d::prelude::{JointAxesMask, JointAxis};

        let handles = self.entity_to_joint.get(&entity_id).cloned().unwrap_or_default();
        if handles.is_empty() {
            log::warn!("set_motor_speed: entity {} has no joint", entity_id);
            return;
        }
        for handle in handles {
            let Some(joint) = self.impulse_joint_set.get_mut(handle) else {
                continue;
            };
            let data = &mut joint.data;
            // A wheel's motor turns it; its free slide is the suspension.
            let axis = if data.user_data == WHEEL_JOINT {
                JointAxis::AngX
            } else if !data.locked_axes.contains(JointAxesMask::ANG_X)
                && data.locked_axes.contains(JointAxesMask::LIN_AXES)
            {
                JointAxis::AngX
            } else if !data.locked_axes.contains(JointAxesMask::LIN_X)
                && data.locked_axes.contains(JointAxesMask::ANG_X)
            {
                JointAxis::LinX
            } else {
                continue;
            };
            // A wheel's chassis is its first body, so its sign is flipped to
            // match a revolute wheel's (see `add_joint`).
            let speed = if data.user_data == WHEEL_JOINT { -speed } else { speed };
            data.set_motor_velocity(axis, speed, MOTOR_DAMPING);
        }
    }

    /// Remove a body (and its collider and joint) for an entity.
    pub fn remove_body(&mut self, entity_id: u64) {
        self.remove_joint(entity_id);
        self.requested_mass.remove(&entity_id);
        self.sprite_shapes.remove(&entity_id);
        self.owners.remove(&entity_id);
        if let Some(collider_handle) = self.entity_to_collider.remove(&entity_id) {
            self.removed_since_step.push(collider_handle);
            self.collider_set.remove(
                collider_handle,
                &mut self.island_manager,
                &mut self.rigid_body_set,
                true,
            );
        }
        if let Some(body_handle) = self.entity_to_body.remove(&entity_id) {
            self.body_to_entity.remove(&body_handle);
            self.written.remove(&body_handle);
            self.written_velocity.remove(&body_handle);
            self.frame_forces.remove(&body_handle);
            if let Some(body) = self.rigid_body_set.get(body_handle) {
                self.removed_since_step.extend_from_slice(body.colliders());
            }
            self.rigid_body_set.remove(
                body_handle,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
        }
        // Removing a body also removes every joint attached to it, including
        // ones another entity created with this one as the target. Their
        // entries would otherwise claim a joint exists, and
        // `ensure_physics_bodies` would never rebuild it for a respawned target.
        let joints = &self.impulse_joint_set;
        self.entity_to_joint.retain(|_, handles| {
            handles.retain(|h| joints.get(*h).is_some());
            !handles.is_empty()
        });
        self.breakable.retain(|h, _| joints.get(*h).is_some());
    }

    /// Step the physics simulation by `dt` seconds using a fixed-timestep accumulator.
    /// This ensures deterministic, smooth physics regardless of frame rate.
    /// Returns contact events.
    ///
    /// The same as [`Self::begin_frame`], [`Self::step_once`] as many times as
    /// it says, then [`Self::end_frame`] - which is how a host runs
    /// `fixed_update` scripts between the steps.
    pub fn step(&mut self, dt: f32) -> Vec<ContactEvent> {
        let steps = self.begin_frame(dt);
        for _ in 0..steps {
            self.step_once();
        }
        self.end_frame()
    }

    /// Start a frame of `dt`: the frame's forces, the accumulator, and how
    /// many fixed steps are due - which [`Self::step_once`] then runs, one
    /// call each, before [`Self::end_frame`].
    pub fn begin_frame(&mut self, dt: f32) -> u32 {
        self.integration_parameters.dt = self.fixed_dt;
        // This frame's `apply_force` calls, as the velocity change a force
        // held for the frame produces - exact whether the frame runs zero
        // fixed steps or four.
        self.flush_forces(dt);
        self.accumulator += dt;
        self.blend_pending = true;
        // Pushes for bodies that never came (see `pending_impulses`).
        self.pending_impulses.clear();
        self.pending_velocity.clear();

        // Cap accumulator to avoid spiral of death (max 4 steps per frame)
        let max_accumulation = self.fixed_dt * 4.0;
        if self.accumulator > max_accumulation {
            self.accumulator = max_accumulation;
        }
        self.frame_contacts = FrameContacts::default();
        // Counted exactly as the steps will consume it, so a host that runs
        // this many `step_once`s steps exactly what `step` used to.
        let (mut left, mut steps) = (self.accumulator, 0u32);
        while left >= self.fixed_dt {
            left -= self.fixed_dt;
            steps += 1;
        }
        steps
    }

    /// Turn the `apply_force` calls collected so far into the velocity change
    /// a force held for `dt` produces. [`Self::begin_frame`] does it for the
    /// frame's; a host running `fixed_update` does it after each call with
    /// the step's `dt`, so a force set there pushes for that step.
    pub fn flush_forces(&mut self, dt: f32) {
        for (handle, force) in self.frame_forces.drain() {
            if let Some(body) = self.rigid_body_set.get_mut(handle) {
                body.apply_impulse(force * dt.max(0.0), true);
            }
        }
    }

    /// One fixed step of [`Self::fixed_dt`]. See [`Self::begin_frame`].
    pub fn step_once(&mut self) {
        let _span = crate::profiling::trace::span("physics.step");
        self.frame_contacts.stepped += 1;
        // Remember where every dynamic body is before this step, so the
        // sync can blend toward where the step puts it.
        self.prev_poses.clear();
        for (handle, body) in self.rigid_body_set.iter() {
            if body.is_dynamic() {
                let p = body.translation();
                self.prev_poses.insert(handle, (p.x, p.y, body.rotation().angle()));
            }
        }
        // Area effects push what is in them every step, at the step's
        // rate - water, wind, a pull (`ecs::area_effect`).
        if !self.areas.is_empty() || self.area_forces_live {
            self.apply_area_effects(self.fixed_dt);
            self.area_forces_live = !self.areas.is_empty();
        }
        self.physics_pipeline.step(
            &self.gravity,
            &self.integration_parameters,
            &mut self.island_manager,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.rigid_body_set,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            &mut self.ccd_solver,
            Some(&mut self.query_pipeline),
            &StepHooks,
            &(),
        );
        // The step has told the query pipeline about every removal. An
        // index kept past this point could name a newer collider.
        self.removed_since_step.clear();
        self.accumulator -= self.fixed_dt;
        // Joints loaded past their limit by this step let go now, so the
        // next step already runs without them.
        if !self.breakable.is_empty() {
            self.break_joints();
        }
        // Read this sub-step's contacts now: the narrow phase only holds
        // the latest step, so a pair that began and ended inside one
        // frame's several steps used to be invisible, and "the frame's
        // impact is the hardest of them" was a comment about a maximum
        // that was never taken.
        let mut frame = std::mem::take(&mut self.frame_contacts);
        self.collect_contacts(&mut frame.current, &mut frame.strength);
        self.frame_contacts = frame;
    }

    /// The fixed step, in seconds.
    pub fn fixed_dt(&self) -> f32 {
        self.fixed_dt
    }

    /// Finish a frame: the contact events its steps produced.
    pub fn end_frame(&mut self) -> Vec<ContactEvent> {
        let FrameContacts { current: current_contacts, strength, stepped } = std::mem::take(&mut self.frame_contacts);
        // A pair is reported on its *second* step, with the impulse the
        // solver applied to resolve it. On the step rapier first sees the
        // pair that number is always zero, so an event read there could only
        // ever say "something touched".
        let mut reported: FxHashSet<(u64, u64)> = FxHashSet::default();

        // A frame that ran no step learned nothing about contacts. At 165 Hz
        // most frames are like that, and diffing their empty set against the
        // last frame's reported every resting contact as ended - and, on the
        // next step, as starting again with the resting load for an
        // "impact". Scene 84's landings all read ~20 in the editor, and a
        // pending first touch looked like a one-step graze with no impulse
        // (BUGS #159). Report nothing and keep the tracking as it was.
        if stepped == 0 {
            return Vec::new();
        }

        // Detect contact events by comparing current contacts with previous
        // frame. Each kind is sorted by its pair before it is reported:
        // starts, then ends, then one-step grazes.
        let mut started = Vec::new();
        for &key in &current_contacts {
            if self.prev_contacts.contains(&key) {
                reported.insert(key);
            } else if self.pending_contacts.contains(&key) {
                started.push(key);
                reported.insert(key);
            }
        }
        // Brand-new pairs wait a step.
        let next_pending: FxHashSet<(u64, u64)> =
            current_contacts.iter().copied().filter(|k| !reported.contains(k)).collect();
        // Contacts that ended.
        let mut stopped: Vec<(u64, u64)> =
            self.prev_contacts.iter().copied().filter(|k| !current_contacts.contains(k)).collect();
        // A graze that lasted a single step is still a contact: report it
        // with what little it was, rather than losing it.
        let mut grazed: Vec<(u64, u64)> =
            self.pending_contacts.iter().copied().filter(|k| !current_contacts.contains(k)).collect();
        started.sort_unstable();
        stopped.sort_unstable();
        grazed.sort_unstable();

        let mut events = Vec::with_capacity(started.len() + stopped.len() + grazed.len() * 2);
        for key in started {
            let (impulse, point) = strength.get(&key).copied().unwrap_or((0.0, (0.0, 0.0)));
            events.push(ContactEvent {
                entity_a: key.0,
                entity_b: key.1,
                event_type: ContactType::Started,
                impulse,
                point,
            });
        }
        for key in stopped {
            events.push(ContactEvent::simple(key.0, key.1, ContactType::Stopped));
        }
        for key in grazed {
            events.push(ContactEvent::simple(key.0, key.1, ContactType::Started));
            events.push(ContactEvent::simple(key.0, key.1, ContactType::Stopped));
        }

        self.prev_contacts = reported;
        self.pending_contacts = next_pending;
        events
    }

    /// Add the narrow phase's active pairs to `current`, keeping in
    /// `strength` the hardest impulse (and its deepest point) seen for each
    /// pair so far this frame.
    fn collect_contacts(
        &self,
        current: &mut FxHashSet<(u64, u64)>,
        strength: &mut FxHashMap<(u64, u64), (f32, (f32, f32))>,
    ) {
        for pair in self.narrow_phase.contact_pairs() {
            if pair.has_any_active_contact {
                let entity_a = self.collider_to_entity(pair.collider1);
                let entity_b = self.collider_to_entity(pair.collider2);
                if let (Some(a), Some(b)) = (entity_a, entity_b) {
                    let key = if a < b { (a, b) } else { (b, a) };
                    current.insert(key);
                    let mut impulse = 0.0f32;
                    let mut deepest = (f32::MAX, (0.0f32, 0.0f32));
                    for manifold in &pair.manifolds {
                        for point in &manifold.points {
                            impulse += point.data.impulse;
                            if point.dist < deepest.0 {
                                // `local_p1` is on the first collider; put it
                                // in world space through that collider's own
                                // position.
                                let at = self
                                    .collider_set
                                    .get(pair.collider1)
                                    .map(|c| c.position() * point.local_p1)
                                    .map(|p| (p.x, p.y))
                                    .unwrap_or((0.0, 0.0));
                                deepest = (point.dist, at);
                            }
                        }
                    }
                    let at = if deepest.0 < f32::MAX { deepest.1 } else { (0.0, 0.0) };
                    let slot = strength.entry(key).or_insert((0.0, at));
                    // Several steps can run in one frame; the frame's impact
                    // is the hardest of them, not the last.
                    if impulse >= slot.0 {
                        *slot = (impulse, at);
                    }
                }
            }
        }
    }

    /// Write physics body positions back to the ECS world (for dynamic bodies).
    ///
    /// Walks the **bodies**, not the world: this used to visit every
    /// `Transform` in the world with a hash probe each to find the handful
    /// that have a body, `O(entities)` a frame for `O(bodies)` of work.
    ///
    /// This is the **stepped** pose - what scripts and packages act on. The
    /// smooth pose a frame is drawn with is written separately, just before
    /// rendering, by [`Self::blend_transforms_for_render`].
    pub fn sync_transforms_to_ecs(&mut self, world: &mut hecs::World) {
        let _span = crate::profiling::trace::span("physics.sync_back");
        let mut fallback = None;
        for (&entity_id, &body_handle) in &self.entity_to_body {
            let Some(body) = self.rigid_body_set.get(body_handle) else { continue };
            if !body.is_dynamic() {
                continue;
            }
            let Some(entity) = self.entity_for_body(entity_id, world, &mut fallback) else { continue };
            let Ok(mut transform) = world.get::<&mut crate::ecs::components::Transform>(entity) else { continue };
            let p = body.translation();
            transform.position.x = p.x;
            transform.position.y = p.y;
            transform.rotation = body.rotation().angle();
            self.written.insert(body_handle, (p.x, p.y, transform.rotation));
            drop(transform);
            // And how fast it is going, so `Velocity2D` reads the body rather
            // than the seed it was built with - a package has no physics world
            // to ask (BUGS #132). The motion step leaves a dynamic body alone,
            // so writing this does not move it a second time.
            if let Ok(mut v) = world.get::<&mut crate::ecs::components::Velocity2D>(entity) {
                let lv = body.linvel();
                v.x = lv.x;
                v.y = lv.y;
                self.written_velocity.insert(body_handle, (lv.x, lv.y));
            }
        }
        self.write_ragdolls(world, 1.0);
        // Broken joints: the component goes (or `ensure_physics_bodies`
        // would build the joint again next frame) and the script hears it.
        for (owner, force, torque) in std::mem::take(&mut self.broken_joints) {
            let Some(entity) = self.entity_for_body(owner, world, &mut fallback) else { continue };
            let made_by_component = world
                .get::<&crate::ecs::components::Joint2D>(entity)
                .is_ok_and(|j| j.break_force == force && j.break_torque == torque);
            if made_by_component {
                let _ = world.remove_one::<crate::ecs::components::Joint2D>(entity);
            }
            crate::ecs::callbacks::push(entity, "on_joint_break");
        }
    }

    /// Write each limp skeleton's bones from their bodies, parents first, as
    /// local transforms under whatever each hangs from - `alpha` of the way
    /// from the pose before the last step, as dynamic bodies are drawn.
    fn write_ragdolls(&self, world: &mut hecs::World, alpha: f32) {
        use crate::ecs::components::{Parent, Transform, WorldTransform};
        for rd in self.ragdolls.values() {
            let mut poses: Vec<WorldTransform> = Vec::with_capacity(rd.parts.len());
            for part in &rd.parts {
                let Some(body) = self.rigid_body_set.get(part.body) else {
                    poses.push(WorldTransform::default());
                    continue;
                };
                let p = body.translation();
                let a = body.rotation().angle();
                let (x, y, r) = match self.prev_poses.get(&part.body) {
                    Some(&(px, py, pa)) if alpha < 1.0 => {
                        let da = (a - pa + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                            - std::f32::consts::PI;
                        (px + (p.x - px) * alpha, py + (p.y - py) * alpha, pa + da * alpha)
                    }
                    _ => (p.x, p.y, a),
                };
                let pose =
                    WorldTransform { position: glam::Vec2::new(x, y), rotation: r, scale: part.scale, shear: 0.0 };
                let above = match part.parent {
                    Some(i) => Some(poses[i].clone()),
                    None => world
                        .get::<&Parent>(part.entity)
                        .ok()
                        .map(|p| p.0)
                        .and_then(|e| crate::ecs::hierarchy::world_transform_of(world, e)),
                };
                if let Ok(mut t) = world.get::<&mut Transform>(part.entity) {
                    let (lp, lr) = match &above {
                        Some(w) => crate::ecs::hierarchy::local_under(w, pose.position, pose.rotation),
                        None => (pose.position, pose.rotation),
                    };
                    t.position = lp;
                    t.rotation = lr;
                }
                poses.push(pose);
            }
        }
    }

    /// Build the bodies of every ragdoll gone limp and drop those of every
    /// one stood up again (or gone). Called by
    /// [`crate::ecs::systems::ensure_physics_bodies`]. See `ecs::ragdoll`.
    #[cfg(feature = "ragdoll")]
    pub fn sync_ragdolls(&mut self, world: &hecs::World) {
        use crate::ecs::ragdoll::Ragdoll;
        let want: Vec<(u64, hecs::Entity, Ragdoll)> = world
            .query::<&Ragdoll>()
            .iter()
            .filter(|(_, r)| r.active)
            .map(|(e, r)| (e.id() as u64, e, r.clone()))
            .collect();
        if want.is_empty() && self.ragdolls.is_empty() {
            return;
        }
        let stale: Vec<u64> = self
            .ragdolls
            .iter()
            .filter(|(id, rd)| !want.iter().any(|(w, e, _)| w == *id && *e == rd.root))
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            self.drop_ragdoll(id);
        }
        for (id, root, rd) in want {
            if !self.ragdolls.contains_key(&id) {
                self.build_ragdoll(world, id, root, &rd);
            }
        }
    }

    /// How many bodies the skeleton under `root_id` is limp as; `None` when
    /// it is not limp. For tests and diagnostics.
    pub fn ragdoll_part_count(&self, root_id: u64) -> Option<usize> {
        self.ragdolls.get(&root_id).map(|r| r.parts.len())
    }

    #[cfg(feature = "ragdoll")]
    fn build_ragdoll(&mut self, world: &hecs::World, id: u64, root: hecs::Entity, rd: &crate::ecs::ragdoll::Ragdoll) {
        use rapier2d::prelude::{ActiveEvents, ActiveHooks, ColliderBuilder, RevoluteJointBuilder, RigidBodyBuilder};
        let bones = crate::ecs::ragdoll::bones_of(world, root);
        let tag = ((id as u128 + 1) << 8) | RAGDOLL_PART;
        let radius = (rd.thickness * 0.5).max(0.5);
        let mut parts: Vec<RagdollPart> = Vec::with_capacity(bones.len());
        for b in &bones {
            let body = RigidBodyBuilder::dynamic()
                .translation(rapier2d::na::Vector2::new(b.world.position.x, b.world.position.y))
                .rotation(b.world.rotation)
                .linear_damping(rd.damping.max(0.0))
                .angular_damping(rd.damping.max(0.0))
                .build();
            let handle = self.rigid_body_set.insert(body);
            // A bone with a negative world scale.x points back along its
            // rotation, as IK and jiggle have it.
            let reach = b.length * b.world.scale.x;
            let shape = if reach.abs() > 1e-3 {
                ColliderBuilder::capsule_from_endpoints(
                    rapier2d::na::Point2::new(0.0, 0.0),
                    rapier2d::na::Point2::new(reach, 0.0),
                    radius,
                )
            } else {
                ColliderBuilder::ball(radius)
            };
            let collider = shape
                .density(rd.density.max(1e-4))
                .friction(rd.friction.max(0.0))
                .user_data(tag)
                .active_hooks(ActiveHooks::FILTER_CONTACT_PAIRS)
                .active_events(ActiveEvents::COLLISION_EVENTS)
                .build();
            self.collider_set.insert_with_parent(collider, handle, &mut self.rigid_body_set);
            let bone_id = b.entity.id() as u64;
            self.body_to_entity.insert(handle, bone_id);
            self.ragdoll_parts.insert(bone_id, handle);
            self.apply_pending(bone_id);
            parts.push(RagdollPart { entity: b.entity, body: handle, parent: b.parent, scale: b.world.scale });
        }
        // Each bone hangs from its parent where it starts, by a hinge whose
        // rest is the angle it has now, so it bends `limit` either way from
        // the pose the skeleton fell in.
        for (i, b) in bones.iter().enumerate() {
            let Some(pi) = b.parent else { continue };
            let up = &bones[pi].world;
            let d = b.world.position - up.position;
            let (sin, cos) = up.rotation.dsin_cos();
            let anchor = rapier2d::na::Point2::new(d.x * cos + d.y * sin, -d.x * sin + d.y * cos);
            let rest = (b.world.rotation - up.rotation + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            let mut hinge = RevoluteJointBuilder::new()
                .local_anchor1(anchor)
                .local_anchor2(rapier2d::na::Point2::new(0.0, 0.0))
                .contacts_enabled(false);
            if rd.limit < std::f32::consts::PI {
                let l = rd.limit.max(0.0);
                hinge = hinge.limits([-l, l]);
            }
            let mut joint = hinge.build();
            joint.data.local_frame1.rotation = rapier2d::na::UnitComplex::new(rest);
            self.impulse_joint_set.insert(parts[pi].body, parts[i].body, joint, true);
        }
        self.ragdolls.insert(id, RagdollBodies { root, parts });
    }

    #[cfg(feature = "ragdoll")]
    fn drop_ragdoll(&mut self, id: u64) {
        let Some(rd) = self.ragdolls.remove(&id) else { return };
        for part in rd.parts {
            if let Some(body) = self.rigid_body_set.get(part.body) {
                self.removed_since_step.extend_from_slice(body.colliders());
            }
            self.rigid_body_set.remove(
                part.body,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
            self.body_to_entity.remove(&part.body);
            self.prev_poses.remove(&part.body);
            self.ragdoll_parts.remove(&(part.entity.id() as u64));
        }
    }

    /// Give a body just built what a script aimed at it before it existed
    /// (see `pending_impulses`): looked up by the entity's id and by the
    /// `spawn_prefab` handle a script knows it by. Call once the body has its
    /// collider - rapier's mass comes from the collider, and an impulse on a
    /// body without one moves nothing.
    pub fn apply_pending(&mut self, entity_id: u64) {
        if self.pending_impulses.is_empty() && self.pending_velocity.is_empty() {
            return;
        }
        let Some(handle) = self.body_or_part(entity_id) else { return };
        let keys = [Some(entity_id), crate::scripting::spawn_aliases::handle_of(entity_id)];
        for key in keys.into_iter().flatten() {
            let velocity = self.pending_velocity.remove(&key);
            let impulse = self.pending_impulses.remove(&key);
            if let Some(body) = self.rigid_body_set.get_mut(handle) {
                if let Some(v) = velocity {
                    body.set_linvel(v, true);
                }
                if let Some(i) = impulse {
                    body.apply_impulse(i, true);
                }
            }
        }
    }

    /// The body an entity's id names: its own, or a limp bone's.
    fn body_or_part(&self, entity_id: u64) -> Option<rapier2d::prelude::RigidBodyHandle> {
        self.entity_to_body.get(&entity_id).or_else(|| self.ragdoll_parts.get(&entity_id)).copied()
    }

    /// Write each dynamic body's transform **for drawing**: blended between
    /// where it was before the latest step and where that step put it, by
    /// how far the accumulator has run since.
    ///
    /// The simulation steps at `fixed_dt` and the display does not: at
    /// 165 Hz a frame alternates between zero and one step, so drawing the
    /// last stepped pose moved a falling body in 60 Hz jumps while the frame
    /// counter read a perfect 165 - the "steady frame rate, juddering motion"
    /// report `time::FramePacer` cannot touch, because the simulation is not
    /// on the display's grid at all.
    ///
    /// Call it **after** scripts and packages have run and **before** the
    /// hierarchy resolve, so what they read (`sync_transforms_to_ecs`, the
    /// stepped pose) is not the blend - which lags the simulation by up to
    /// one step, and a package that decides "on the ground" from a wheel's
    /// transform decides it a step late. The next frame's sync overwrites
    /// the blend before anything reads it. A frame that did not step (the
    /// editor not in play) blends nothing.
    pub fn blend_transforms_for_render(&mut self, world: &mut hecs::World) {
        if !self.blend_pending {
            return;
        }
        self.blend_pending = false;
        self.blended = true;
        let alpha = if self.fixed_dt > 0.0 { (self.accumulator / self.fixed_dt).clamp(0.0, 1.0) } else { 1.0 };
        let mut fallback = None;
        for (&entity_id, &body_handle) in &self.entity_to_body {
            let Some(body) = self.rigid_body_set.get(body_handle) else { continue };
            if !body.is_dynamic() {
                continue;
            }
            let Some(&(px, py, pa)) = self.prev_poses.get(&body_handle) else { continue };
            let Some(entity) = self.entity_for_body(entity_id, world, &mut fallback) else { continue };
            let Ok(mut transform) = world.get::<&mut crate::ecs::components::Transform>(entity) else { continue };
            // Moved by a script since this world last wrote it: leave the
            // move for `sync_kinematic_from_ecs` to teleport the body to.
            // Blending over it here - after scripts, which is where every
            // host calls this - erased every `set_position` on a dynamic
            // body before the teleport could see it (BUGS #158).
            if let Some(&(wx, wy, wr)) = self.written.get(&body_handle)
                && ((transform.position.x - wx).abs() > 1e-3
                    || (transform.position.y - wy).abs() > 1e-3
                    || (transform.rotation - wr).abs() > 1e-4)
            {
                continue;
            }
            let p = body.translation();
            let cur = (p.x, p.y, body.rotation().angle());
            // Shortest way round for the angle, so a body turning through
            // +/- pi does not spin the long way for a frame.
            let mut da = cur.2 - pa;
            if da > std::f32::consts::PI {
                da -= std::f32::consts::TAU;
            } else if da < -std::f32::consts::PI {
                da += std::f32::consts::TAU;
            }
            transform.position.x = px + (cur.0 - px) * alpha;
            transform.position.y = py + (cur.1 - py) * alpha;
            transform.rotation = pa + da * alpha;
            self.written
                .insert(body_handle, (transform.position.x, transform.position.y, transform.rotation));
        }
        self.write_ragdolls(world, alpha);
    }

    /// Undo [`Self::blend_transforms_for_render`]: every dynamic body's
    /// transform put back at the pose it was stepped to, so whatever reads
    /// the world before this frame's step - behaviour graphs, the motion
    /// step, packages' `PreUpdate` and `ScriptExec` systems - reads the
    /// physics, not the picture.
    ///
    /// Every host calls it at the start of its frame (before the behaviour
    /// graphs). Without it the blend was overwritten only by the next step's
    /// sync - and a frame at 165 Hz runs no step most of the time, so a
    /// package working out a pitch rate from frame-to-frame transform changes
    /// read hundreds of radians a second: a hill-driving game's jeep flipped onto its
    /// roof natively while every headless test, one step a frame, drove fine.
    /// A transform moved since the blend wrote it (a script's
    /// `set_position`) is left to [`Self::sync_kinematic_from_ecs`].
    pub fn restore_stepped_poses(&mut self, world: &mut hecs::World) {
        if !self.blended {
            return;
        }
        self.blended = false;
        let mut fallback = None;
        for (&entity_id, &body_handle) in &self.entity_to_body {
            let Some(body) = self.rigid_body_set.get(body_handle) else { continue };
            if !body.is_dynamic() {
                continue;
            }
            let Some(&(wx, wy, wr)) = self.written.get(&body_handle) else { continue };
            let Some(entity) = self.entity_for_body(entity_id, world, &mut fallback) else { continue };
            let Ok(mut transform) = world.get::<&mut crate::ecs::components::Transform>(entity) else { continue };
            if (transform.position.x - wx).abs() > 1e-3
                || (transform.position.y - wy).abs() > 1e-3
                || (transform.rotation - wr).abs() > 1e-4
            {
                continue;
            }
            let p = body.translation();
            transform.position.x = p.x;
            transform.position.y = p.y;
            transform.rotation = body.rotation().angle();
            self.written.insert(body_handle, (p.x, p.y, transform.rotation));
        }
        self.write_ragdolls(world, 1.0);
    }

    /// Move every body back by however far the world was moved back since
    /// the last call (`ecs::floating_origin`). Moved, not teleported: a
    /// dynamic body keeps its velocity and contacts, and a kinematic one is
    /// placed rather than driven there, so the jump is never read as its
    /// speed (nor as a platform's, carrying what stands on it). Called first
    /// by `ensure_physics_bodies` and [`Self::sync_kinematic_from_ecs`], so
    /// whichever runs first in a frame does it.
    pub fn follow_origin(&mut self) {
        let by = crate::ecs::floating_origin::follow(&mut self.origin_seen);
        if by == glam::Vec2::ZERO {
            return;
        }
        let d = rapier2d::na::Vector2::new(by.x, by.y);
        for (_, body) in self.rigid_body_set.iter_mut() {
            let mut iso = *body.position();
            iso.translation.vector -= d;
            body.set_position(iso, false);
            if body.is_kinematic() {
                body.set_next_kinematic_position(iso);
            }
        }
        // Every collider too, attached or not: a body moved by
        // `set_position` takes its colliders along only when rapier steps a
        // body it thinks moved, and a fixed one's never did - the ground
        // stayed where it was and what stood on it fell.
        for (_, c) in self.collider_set.iter_mut() {
            let t = c.translation() - d;
            c.set_translation(t);
        }
        for v in self.written.values_mut() {
            v.0 -= by.x;
            v.1 -= by.y;
        }
        for v in self.prev_poses.values_mut() {
            v.0 -= by.x;
            v.1 -= by.y;
        }
        for v in self.kinematic_last.values_mut() {
            v.0 -= by.x;
            v.1 -= by.y;
        }
    }

    /// Sync kinematic body positions from ECS transforms into rapier.
    ///
    /// Also notes how far each one moved since last frame, which is what
    /// [`Self::move_characters`] carries a character standing on it by.
    /// Walks the bodies, like [`Self::sync_transforms_to_ecs`].
    pub fn sync_kinematic_from_ecs(&mut self, world: &hecs::World) {
        self.follow_origin();
        let mut seen = HashSet::new();
        let mut fallback = None;
        let handles: Vec<(u64, rapier2d::prelude::RigidBodyHandle)> =
            self.entity_to_body.iter().map(|(&id, &h)| (id, h)).collect();
        for (entity_id, body_handle) in handles {
            let Some(entity) = self.entity_for_body(entity_id, world, &mut fallback) else { continue };
            let Ok(transform) = world.get::<&crate::ecs::components::Transform>(entity) else { continue };
            let Some(body) = self.rigid_body_set.get_mut(body_handle) else { continue };
            if body.is_dynamic() {
                // Moved by someone other than this world since it last wrote
                // the transform - a script's `set_position` / `set_rotation`,
                // a package placing a body. Teleport the body there. Before
                // this the next sync simply wrote the body's own pose back,
                // so `set_position` on any dynamic body did nothing in any
                // runtime (test-lab scene 84's "drop again", BUGS #158).
                // Velocity is kept, as moving a transform keeps it anywhere
                // else; a script wanting a dead stop sets it.
                let now = (transform.position.x, transform.position.y, transform.rotation);
                if let Some(&(wx, wy, wr)) = self.written.get(&body_handle) {
                    let moved = (now.0 - wx).abs() > 1e-3 || (now.1 - wy).abs() > 1e-3;
                    let turned = (now.2 - wr).abs() > 1e-4;
                    if moved || turned {
                        body.set_position(
                            rapier2d::prelude::Isometry::new(rapier2d::na::Vector2::new(now.0, now.1), now.2),
                            true,
                        );
                        // Nothing to blend from: the body did not travel there.
                        self.prev_poses.insert(body_handle, now);
                        self.written.insert(body_handle, now);
                    }
                }
                // Its `Velocity2D` changed by someone else since this world
                // wrote it - a package setting the speed, which has no other
                // way to: the body takes it. A script's `set_velocity` goes
                // to rapier directly and is written back before this reads.
                if let Ok(v) = world.get::<&crate::ecs::components::Velocity2D>(entity)
                    && let Some(&(wx, wy)) = self.written_velocity.get(&body_handle)
                    && ((v.x - wx).abs() > 1e-3 || (v.y - wy).abs() > 1e-3)
                {
                    body.set_linvel(rapier2d::na::Vector2::new(v.x, v.y), true);
                    self.written_velocity.insert(body_handle, (v.x, v.y));
                }
                continue;
            }
            if !body.is_kinematic() {
                continue;
            }
            let now = (transform.position.x, transform.position.y);
            let last = self.kinematic_last.insert(entity_id, now).unwrap_or(now);
            self.kinematic_delta.insert(entity_id, (now.0 - last.0, now.1 - last.1));
            seen.insert(entity_id);
            body.set_next_kinematic_position(rapier2d::prelude::Isometry::new(
                rapier2d::na::Vector2::new(transform.position.x, transform.position.y),
                transform.rotation,
            ));
        }
        self.kinematic_last.retain(|id, _| seen.contains(id));
        self.kinematic_delta.retain(|id, _| seen.contains(id));
    }

    /// The live entity a body belongs to: its recorded owner, or - for a
    /// body added by bare id, which only tests do - whichever entity has
    /// that index, found through an index built once per call and only
    /// when needed.
    /// The entity a body was built for, by the id scripts know it by.
    pub fn owner_of(&self, entity_id: u64) -> Option<hecs::Entity> {
        self.owners.get(&entity_id).copied()
    }

    fn entity_for_body(
        &self,
        entity_id: u64,
        world: &hecs::World,
        fallback: &mut Option<HashMap<u64, hecs::Entity>>,
    ) -> Option<hecs::Entity> {
        if let Some(&owner) = self.owners.get(&entity_id) {
            return Some(owner);
        }
        let index = fallback
            .get_or_insert_with(|| world.iter().map(|e| (e.entity().id() as u64, e.entity())).collect());
        index.get(&entity_id).copied()
    }

    /// Build, rebuild or drop the static body of every colliding tilemap.
    ///
    /// Called by [`crate::ecs::systems::ensure_physics_bodies`], so every
    /// runtime does it. A tilemap collides when it has a `collision` layer or
    /// a [`TilemapCollider`] naming layers.
    ///
    /// # What this costs
    ///
    /// An idle frame reads **one integer per colliding layer** - the layer's
    /// [`LayerRevisions::total`] - plus the position and tile size. It does
    /// not look at a single tile. It used to hash every tile of every
    /// colliding layer *every frame*, which for a million-tile world was a
    /// per-frame walk of a million cells to discover that nothing had moved.
    ///
    /// An edited frame rebuilds the [`CHUNK_TILES`]-square chunks whose own
    /// counters moved, and nothing else. One pick swing used to remove the
    /// body and rebuild every collider on the map.
    ///
    /// Measured on a 1000 x 1000 map of 15,149 merged boxes, release build
    /// (`the_cost_of_a_million_tiles`, which is `#[ignore]`d because a timing
    /// is not an assertion):
    ///
    /// | | before | after |
    /// |---|---|---|
    /// | idle frame | 966 µs (hash every tile) | **884 ns** |
    /// | one-tile edit | 966 µs + a full 11.6 ms rebuild | **1.1 µs** |
    ///
    /// The first build is still 11.6 ms, and still happens once.
    ///
    /// The trade is that merged boxes stop at chunk borders, so a floor
    /// spanning four chunks is four boxes rather than one. Seams between
    /// abutting cuboids already existed wherever runs did not line up
    /// (`merge_tiles` only stacks rows with identical spans), and
    /// `a_character_walks_across_chunk_seams` is the test that says a
    /// character does not catch on them.
    ///
    /// [`TilemapCollider`]: crate::ecs::components::TilemapCollider
    /// [`LayerRevisions::total`]: crate::tilemap::LayerRevisions::total
    /// [`CHUNK_TILES`]: crate::tilemap::CHUNK_TILES
    pub fn sync_tilemap_colliders(&mut self, world: &hecs::World) {
        let _span = crate::profiling::trace::span("physics.tilemaps");
        use crate::ecs::components::{TilemapCollider, Transform};
        use crate::tilemap::Tilemap;
        use rapier2d::prelude::*;

        let mut seen = HashSet::new();
        for (entity, (tm, tr)) in world.query::<(&Tilemap, &Transform)>().iter() {
            let id = entity.id() as u64;
            let spec = world.get::<&TilemapCollider>(entity).ok().map(|c| (*c).clone());
            // (layer index, one-way) for each colliding layer.
            let layers: Vec<(usize, bool)> = tm
                .layers
                .iter()
                .enumerate()
                .filter_map(|(i, l)| {
                    let one_way = spec.as_ref().is_some_and(|s| s.one_way.contains(&l.name));
                    let solid = l.collision || spec.as_ref().is_some_and(|s| s.solid.contains(&l.name));
                    (one_way || solid).then_some((i, one_way && !l.collision))
                })
                .collect();
            if layers.is_empty() {
                continue;
            }
            let (tw, th) = (tm.tile_size[0], tm.tile_size[1]);
            if tw <= 0.0 || th <= 0.0 {
                continue;
            }
            seen.insert(id);
            // A parented map is solid where its parent carries it.
            let placed = crate::tilemap::world_origin(world, entity, tr);
            let origin = (placed.x.to_bits(), placed.y.to_bits());
            let tile_size = (tw.to_bits(), th.to_bits());
            let shape = grid_shape_key(tm);

            // Can the body we already have describe this tilemap at all? Its
            // shape - where it is, how big a tile is, which layers collide
            // and how large they are - is a handful of integers, compared
            // whole. Only the tiles are tracked per chunk.
            let reusable = self.tilemap_bodies.get(&id).is_some_and(|b| {
                b.owner == entity
                    && b.origin == origin
                    && b.tile_size == tile_size
                    && b.shape == shape
                    && b.layers.len() == layers.len()
                    && b.layers.iter().zip(&layers).all(|(lb, (i, one_way))| {
                        lb.index == *i
                            && lb.one_way == *one_way
                            && lb.width == tm.layers[*i].width
                            && lb.height == tm.layers[*i].height
                            && lb.rev_id == tm.layers[*i].rev.id()
                    })
            });
            // Taken out of the map for the duration, so the rebuild below can
            // borrow the collider set and the body set at the same time.
            let mut record = if reusable {
                self.tilemap_bodies.remove(&id).expect("reusable implies a record")
            } else {
                self.remove_tilemap_body(id);
                let body = self.rigid_body_set.insert(
                    RigidBodyBuilder::fixed().translation(vector![placed.x, placed.y]).build(),
                );
                self.body_to_entity.insert(body, id);
                TilemapBody {
                    owner: entity,
                    body,
                    origin,
                    tile_size,
                    shape,
                    boxes: 0,
                    layers: layers
                        .iter()
                        .map(|(i, one_way)| TilemapLayerBody {
                            index: *i,
                            one_way: *one_way,
                            width: tm.layers[*i].width,
                            height: tm.layers[*i].height,
                            rev_id: tm.layers[*i].rev.id(),
                            revision: None,
                            chunks: (0..tm.layers[*i].chunk_count()).map(|_| (None, Vec::new())).collect(),
                        })
                        .collect(),
                }
            };

            let mut rebuilt_any = false;
            for lb in &mut record.layers {
                let l = &tm.layers[lb.index];
                // The whole idle case, and the reason this function is cheap.
                if lb.revision == Some(l.rev.total()) {
                    continue;
                }
                lb.revision = Some(l.rev.total());
                for ci in 0..lb.chunks.len() {
                    let want = l.rev.chunk(ci);
                    if lb.chunks[ci].0 == Some(want) {
                        continue;
                    }
                    // Wake whatever was resting on the old colliders: a body
                    // asleep on a tile that just went away must fall.
                    for h in std::mem::take(&mut lb.chunks[ci].1) {
                        self.removed_since_step.push(h);
                        self.collider_set.remove(h, &mut self.island_manager, &mut self.rigid_body_set, true);
                    }
                    for builder in tile_area_colliders(tm, l, l.chunk_rect(ci), lb.one_way) {
                        let col = builder
                            .user_data(if lb.one_way { ONE_WAY } else { 0 })
                            .active_events(ActiveEvents::COLLISION_EVENTS)
                            .build();
                        let h = self.collider_set.insert_with_parent(col, record.body, &mut self.rigid_body_set);
                        lb.chunks[ci].1.push(h);
                    }
                    lb.chunks[ci].0 = Some(want);
                    self.tilemap_chunk_rebuilds += 1;
                    rebuilt_any = true;
                }
            }
            // Only when something moved, so an idle frame really does read one
            // integer per layer and nothing else - counting the boxes is a walk
            // of every chunk, which is small but is not nothing.
            if rebuilt_any {
                record.boxes =
                    record.layers.iter().map(|lb| lb.chunks.iter().map(|c| c.1.len()).sum::<usize>()).sum();
            }
            self.tilemap_bodies.insert(id, record);
        }
        let gone: Vec<u64> = self.tilemap_bodies.keys().filter(|id| !seen.contains(id)).copied().collect();
        for id in gone {
            self.remove_tilemap_body(id);
        }
    }

    /// How many boxes the tilemap's body is made of, if it has one. For
    /// tests and diagnostics.
    pub fn tilemap_box_count(&self, entity_id: u64) -> Option<usize> {
        self.tilemap_bodies.get(&entity_id).map(|b| b.boxes)
    }

    /// How many tilemap collider **chunks** have been rebuilt since this world
    /// was created.
    ///
    /// The cost of [`Self::sync_tilemap_colliders`] is this number times the
    /// tiles in a chunk, and nothing else can see it: a test that a one-tile
    /// edit is cheap has to count rebuilds, because the resulting colliders
    /// are identical either way. An idle frame must not move it at all.
    pub fn tilemap_chunk_rebuilds(&self) -> u64 {
        self.tilemap_chunk_rebuilds
    }

    /// Build - or rebuild, when its mask or its place changed - the static
    /// body of every carvable sprite with `collider` on: its solid texels as
    /// small convex pieces along the outline and boxes inside
    /// (`ecs::carve::solid_pieces`). A rebuild swaps the
    /// colliders on the same body and wakes what touched them, so a body
    /// resting on a pixel that was carved away falls.
    pub fn sync_carved_colliders(&mut self, world: &hecs::World) {
        use crate::ecs::carve::Carvable;
        use crate::ecs::components::{SpriteComponent, Transform, WorldTransform};
        use rapier2d::prelude::*;

        let mut seen = HashSet::new();
        for (entity, (c, s, t, w)) in
            world.query::<(&Carvable, &SpriteComponent, &Transform, Option<&WorldTransform>)>().iter()
        {
            if !c.collider || c.state.texture.is_none() {
                continue;
            }
            let id = entity.id() as u64;
            seen.insert(id);
            let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
            let pose = [
                wt.position.x.to_bits(),
                wt.position.y.to_bits(),
                wt.rotation.to_bits(),
                wt.scale.x.to_bits(),
                wt.scale.y.to_bits(),
            ];
            let known = self.carved_bodies.get(&id).filter(|b| b.owner == entity).map(|b| (b.body, b.rev, b.pose));
            if known.is_some_and(|(_, rev, p)| rev == c.state.rev && p == pose) {
                continue;
            }
            let body = match known {
                Some((body, _, _)) => {
                    // Drop the old outline, waking whatever rested on it.
                    let old: Vec<ColliderHandle> =
                        self.rigid_body_set.get(body).map(|b| b.colliders().to_vec()).unwrap_or_default();
                    for h in old {
                        self.removed_since_step.push(h);
                        self.collider_set.remove(h, &mut self.island_manager, &mut self.rigid_body_set, true);
                    }
                    if let Some(b) = self.rigid_body_set.get_mut(body) {
                        b.set_position(Isometry::new(vector![wt.position.x, wt.position.y], wt.rotation), true);
                    }
                    body
                }
                None => {
                    self.remove_carved_body(id);
                    let body = self.rigid_body_set.insert(
                        RigidBodyBuilder::fixed()
                            .translation(vector![wt.position.x, wt.position.y])
                            .rotation(wt.rotation)
                            .build(),
                    );
                    self.body_to_entity.insert(body, id);
                    body
                }
            };
            for builder in carved_colliders(c, s, wt.scale) {
                let col = builder.friction(c.friction).active_events(ActiveEvents::COLLISION_EVENTS).build();
                self.collider_set.insert_with_parent(col, body, &mut self.rigid_body_set);
            }
            self.carved_bodies.insert(id, CarvedBody { owner: entity, body, rev: c.state.rev, pose });
        }
        let gone: Vec<u64> = self.carved_bodies.keys().filter(|id| !seen.contains(id)).copied().collect();
        for id in gone {
            self.remove_carved_body(id);
        }
    }

    /// Read every [`crate::ecs::area_effect::AreaEffect`] in the world -
    /// where its box is this frame, turned and scaled with its entity - for
    /// the steps that follow to apply. `ensure_physics_bodies` calls it.
    pub fn sync_area_effects(&mut self, world: &hecs::World) {
        self.areas = crate::ecs::area_effect::placed_areas(world);
    }

    /// One fixed step of every area effect on every dynamic body it holds:
    /// its acceleration as an impulse of mass times acceleration times `dt`,
    /// and its drag taken off the body's velocity.
    fn apply_area_effects(&mut self, dt: f32) {
        use rapier2d::na::Vector2;
        let gravity = glam::Vec2::new(self.gravity.x, self.gravity.y);
        for (_, body) in self.rigid_body_set.iter_mut() {
            if !body.is_dynamic() {
                continue;
            }
            // Last step's push is cleared whether or not there is one now:
            // a body that floated out of the water must stop rising.
            body.reset_forces(false);
            body.reset_torques(false);
            let com = body.center_of_mass();
            let centre = glam::Vec2::new(com.x, com.y);
            let (mut force, mut torque) = (glam::Vec2::ZERO, 0.0f32);
            let (mut drag, mut spin_drag, mut weight) = (0.0f32, 0.0f32, 0.0f32);
            for h in body.colliders() {
                let Some(c) = self.collider_set.get(*h) else { continue };
                if c.is_sensor() {
                    continue;
                }
                let outline = collider_outline(c);
                let mass = c.mass();
                weight += mass;
                for area in &self.areas {
                    let Some(p) = crate::ecs::area_effect::push(area, &outline, centre, gravity) else { continue };
                    force += p.accel * mass;
                    let lift = p.lift * mass;
                    force += lift;
                    torque += (p.lift_at - centre).perp_dot(lift);
                    drag += p.drag * mass;
                    spin_drag += p.angular_drag * mass;
                }
            }
            if weight <= 0.0 || (force == glam::Vec2::ZERO && torque == 0.0 && drag == 0.0 && spin_drag == 0.0) {
                continue;
            }
            // Forces, not impulses: rapier spreads gravity over the step's
            // solver substeps and an impulse lands whole at its start, so an
            // impulse cancelling gravity exactly still moved a body - water
            // of buoyancy one lifted what it should have held still, and a
            // float sat a tenth of its height low. The force slot is ours
            // alone (`apply_force` goes through impulses).
            body.add_force(Vector2::new(force.x, force.y), true);
            body.add_torque(torque, true);
            // Drag weighed by each collider's share of the mass.
            let (drag, spin_drag) = (drag / weight, spin_drag / weight);
            if drag > 0.0 {
                let v = *body.linvel() * (1.0 - drag * dt).max(0.0);
                body.set_linvel(v, true);
            }
            if spin_drag > 0.0 {
                let w = body.angvel() * (1.0 - spin_drag * dt).max(0.0);
                body.set_angvel(w, true);
            }
        }
    }

    /// Build - or rebuild, when it was dug or built or moved - the static
    /// body of every smooth terrain with `collider` on
    /// (`terrain_colliders`). A rebuild swaps the colliders on the same body
    /// and wakes what touched them, so a body resting on dug ground falls.
    #[cfg(feature = "smooth_terrain")]
    pub fn sync_smooth_terrain(&mut self, world: &hecs::World) {
        use crate::ecs::components::{Transform, WorldTransform};
        use crate::ecs::smooth_terrain::SmoothTerrain;
        use rapier2d::prelude::*;

        let mut seen = HashSet::new();
        for (entity, (terrain, t, w)) in world.query::<(&SmoothTerrain, &Transform, Option<&WorldTransform>)>().iter() {
            if !terrain.collider || terrain.rev == 0 {
                continue;
            }
            let id = entity.id() as u64;
            seen.insert(id);
            let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
            let pose = [
                wt.position.x.to_bits(),
                wt.position.y.to_bits(),
                wt.rotation.to_bits(),
                wt.scale.x.to_bits(),
                wt.scale.y.to_bits(),
            ];
            let known = self.terrain_bodies.get(&id).filter(|b| b.owner == entity).map(|b| (b.body, b.rev, b.pose));
            if known.is_some_and(|(_, rev, p)| rev == terrain.rev && p == pose) {
                continue;
            }
            let body = match known {
                Some((body, _, _)) => {
                    let old: Vec<ColliderHandle> =
                        self.rigid_body_set.get(body).map(|b| b.colliders().to_vec()).unwrap_or_default();
                    for h in old {
                        self.removed_since_step.push(h);
                        self.collider_set.remove(h, &mut self.island_manager, &mut self.rigid_body_set, true);
                    }
                    if let Some(b) = self.rigid_body_set.get_mut(body) {
                        b.set_position(Isometry::new(vector![wt.position.x, wt.position.y], wt.rotation), true);
                    }
                    body
                }
                None => {
                    self.remove_terrain_body(id);
                    let body = self.rigid_body_set.insert(
                        RigidBodyBuilder::fixed()
                            .translation(vector![wt.position.x, wt.position.y])
                            .rotation(wt.rotation)
                            .build(),
                    );
                    self.body_to_entity.insert(body, id);
                    body
                }
            };
            for builder in terrain_colliders(terrain, wt.scale) {
                let col = builder.friction(terrain.friction).active_events(ActiveEvents::COLLISION_EVENTS).build();
                self.collider_set.insert_with_parent(col, body, &mut self.rigid_body_set);
            }
            self.terrain_bodies.insert(id, CarvedBody { owner: entity, body, rev: terrain.rev, pose });
        }
        let gone: Vec<u64> = self.terrain_bodies.keys().filter(|id| !seen.contains(id)).copied().collect();
        for id in gone {
            self.remove_terrain_body(id);
        }
    }

    #[cfg(feature = "smooth_terrain")]
    fn remove_terrain_body(&mut self, entity_id: u64) {
        if let Some(cb) = self.terrain_bodies.remove(&entity_id) {
            self.body_to_entity.remove(&cb.body);
            if let Some(body) = self.rigid_body_set.get(cb.body) {
                self.removed_since_step.extend_from_slice(body.colliders());
            }
            self.rigid_body_set.remove(
                cb.body,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
        }
    }

    /// Build - or rebuild, when it or its place changed - the static body of
    /// every spline shape with `collider` on: a convex triangle per triangle
    /// of its fill (`spline_colliders`), solid all the way through.
    #[cfg(feature = "spline_shape")]
    pub fn sync_spline_colliders(&mut self, world: &hecs::World) {
        use crate::ecs::components::{Transform, WorldTransform};
        use crate::renderer::spline_shape::SplineShape;
        use rapier2d::prelude::*;

        let mut seen = HashSet::new();
        for (entity, (shape, t, w)) in world.query::<(&SplineShape, &Transform, Option<&WorldTransform>)>().iter() {
            if !shape.collider {
                continue;
            }
            let id = entity.id() as u64;
            seen.insert(id);
            let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
            let pose = [
                wt.position.x.to_bits(),
                wt.position.y.to_bits(),
                wt.rotation.to_bits(),
                wt.scale.x.to_bits(),
                wt.scale.y.to_bits(),
            ];
            let key = shape.key() ^ (shape.friction.to_bits() as u64).rotate_left(17);
            if self.spline_bodies.get(&id).is_some_and(|b| b.owner == entity && b.key == key && b.pose == pose) {
                continue;
            }
            self.remove_spline_body(id);
            let body = self.rigid_body_set.insert(
                RigidBodyBuilder::fixed().translation(vector![wt.position.x, wt.position.y]).rotation(wt.rotation).build(),
            );
            self.body_to_entity.insert(body, id);
            for builder in spline_colliders(shape, wt.scale) {
                let col = builder.friction(shape.friction).active_events(ActiveEvents::COLLISION_EVENTS).build();
                self.collider_set.insert_with_parent(col, body, &mut self.rigid_body_set);
            }
            self.spline_bodies.insert(id, SplineBody { owner: entity, body, key, pose });
        }
        let gone: Vec<u64> = self.spline_bodies.keys().filter(|id| !seen.contains(id)).copied().collect();
        for id in gone {
            self.remove_spline_body(id);
        }
    }

    #[cfg(feature = "spline_shape")]
    fn remove_spline_body(&mut self, entity_id: u64) {
        if let Some(sb) = self.spline_bodies.remove(&entity_id) {
            self.body_to_entity.remove(&sb.body);
            if let Some(body) = self.rigid_body_set.get(sb.body) {
                self.removed_since_step.extend_from_slice(body.colliders());
            }
            self.rigid_body_set.remove(
                sb.body,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
        }
    }

    fn remove_carved_body(&mut self, entity_id: u64) {
        if let Some(cb) = self.carved_bodies.remove(&entity_id) {
            self.body_to_entity.remove(&cb.body);
            if let Some(body) = self.rigid_body_set.get(cb.body) {
                self.removed_since_step.extend_from_slice(body.colliders());
            }
            self.rigid_body_set.remove(
                cb.body,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
        }
    }

    fn remove_tilemap_body(&mut self, entity_id: u64) {
        if let Some(tb) = self.tilemap_bodies.remove(&entity_id) {
            self.body_to_entity.remove(&tb.body);
            if let Some(body) = self.rigid_body_set.get(tb.body) {
                self.removed_since_step.extend_from_slice(body.colliders());
            }
            self.rigid_body_set.remove(
                tb.body,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
        }
    }

    /// Move every [`CharacterController2D`] by the velocity it asks for, with
    /// rapier's kinematic character controller, and write back where it ended
    /// up and what it touched.
    ///
    /// Call once a frame after [`Self::sync_kinematic_from_ecs`] and before
    /// [`Self::step`] - every runtime does - so the step carries the body to
    /// its new place and pushes what it walked into. It moves by the frame's
    /// `dt`, not the fixed physics step: a character moving in 60 Hz jumps
    /// looks wrong on a faster screen, where nothing else does.
    ///
    /// [`CharacterController2D`]: crate::ecs::components::CharacterController2D
    pub fn move_characters(&mut self, world: &mut hecs::World, dt: f32) {
        let _span = crate::profiling::trace::span("physics.characters");
        use crate::ecs::components::{CharacterController2D, Transform};
        use rapier2d::control::{CharacterLength, KinematicCharacterController};
        use rapier2d::prelude::*;

        if dt <= 0.0 {
            return;
        }
        let entities: Vec<hecs::Entity> =
            world.query::<(&CharacterController2D, &Transform)>().iter().map(|(e, _)| e).collect();
        if entities.is_empty() {
            return;
        }
        // The query pipeline is built during `step`, so it still has every
        // moving platform where it was last step. A character casts against
        // it, so a lift rising into one would pass through it and leave it
        // behind. Put the moved kinematic colliders where they are about to
        // be, and refresh the tree over those.
        let moved: Vec<RigidBodyHandle> = self
            .kinematic_delta
            .iter()
            .filter(|(_, (dx, dy))| dx.abs() > 1e-6 || dy.abs() > 1e-6)
            .filter_map(|(id, _)| self.entity_to_body.get(id).copied())
            .collect();
        if !moved.is_empty() {
            let mut touched: Vec<ColliderHandle> = Vec::new();
            for bh in moved {
                let Some(body) = self.rigid_body_set.get(bh) else { continue };
                let next = *body.next_position();
                for ch in body.colliders().to_vec() {
                    if let Some(c) = self.collider_set.get_mut(ch) {
                        let local = c.position_wrt_parent().copied().unwrap_or_else(Isometry::identity);
                        c.set_position(next * local);
                        touched.push(ch);
                    }
                }
            }
            // With the removals since the last step: the refit reads every leaf
            // the pipeline holds, and one whose collider is gone panics.
            let removed = std::mem::take(&mut self.removed_since_step);
            self.query_pipeline.update_incremental(&self.collider_set, &touched, &removed, true);
        }
        for entity in entities {
            let id = entity.id() as u64;
            if !self.owned_by(id, entity) {
                continue;
            }
            let (Some(&bh), Some(&ch)) = (self.entity_to_body.get(&id), self.entity_to_collider.get(&id)) else {
                continue;
            };
            let Some(cc) = world.get::<&CharacterController2D>(entity).ok().map(|c| (*c).clone()) else { continue };
            let Some(body) = self.rigid_body_set.get(bh) else { continue };
            if !body.is_kinematic() {
                log::debug!("character_controller on entity {id} needs a kinematic rigid_body");
                continue;
            }
            // A platform it stands on carries it: rapier's controller drags
            // a character along with the kinematic ground under it, in both
            // directions, as long as the query pipeline above knows where
            // that ground has got to.
            let mut start = *body.next_position();
            // A platform it stands on carries it. Rapier's controller drags a
            // character along with the kinematic ground under it and lifts it
            // with a rising one, but it cannot take one *down*: a platform
            // dropping faster than the character falls leaves it in the air,
            // and it hops down the whole way. So the downward part is carried
            // here, by moving the character before it moves itself - added to
            // what it asks for instead, a platform would be in its way.
            let dropped = if cc.ride_platforms && cc.grounded {
                let dy = cc
                    .ground_entity
                    .and_then(|g| self.kinematic_delta.get(&g))
                    .map_or(0.0, |(_, dy)| dy.min(0.0));
                // A drop of several times its own height in one frame is not a
                // platform it could have stayed on but one sent somewhere else -
                // a crumbling platform parked out of the level, a lift reset to
                // its start. Carried, the rider went with it (a platformer's hero
                // died below the level whenever a platform gave way under him).
                let tall = self.collider_set.get(ch).map_or(0.0, |c| {
                    let a = c.shape().compute_local_aabb();
                    a.maxs.y - a.mins.y
                });
                if -dy > 4.0 * tall.max(1.0) { 0.0 } else { dy }
            } else {
                0.0
            };
            start.translation.y += dropped;
            let Some(collider) = self.collider_set.get(ch) else { continue };
            let local = collider.position_wrt_parent().copied().unwrap_or_else(Isometry::identity);
            let shape_pos = start * local;
            let shape = collider.shared_shape().clone();

            let desired = vector![cc.velocity[0], cc.velocity[1]] * dt;
            let kcc = KinematicCharacterController {
                up: Vector::y_axis(),
                offset: CharacterLength::Absolute(cc.skin.max(0.01)),
                slide: true,
                // Stepping is done below rather than by rapier's autostep,
                // which refuses whenever the character is already resting
                // against the ledge - so whether it stepped depended on how
                // the frames happened to fall.
                autostep: None,
                max_slope_climb_angle: cc.max_slope_deg.to_radians(),
                min_slope_slide_angle: cc.min_slide_deg.to_radians(),
                snap_to_ground: (cc.snap_to_ground > 0.0).then_some(CharacterLength::Absolute(cc.snap_to_ground)),
                ..Default::default()
            };
            // A one-way platform blocks only a character coming down onto it
            // from above: not one rising through it, not one whose feet were
            // already below its top, and not one asking to drop through.
            let feet = shape.compute_aabb(&shape_pos).mins.y;
            let rising = desired.y > 0.0;
            let tolerance = cc.skin + 1.0;
            let blocks = |_: ColliderHandle, c: &Collider| -> bool {
                if c.user_data & ONE_WAY == 0 {
                    return true;
                }
                !cc.drop_through && !rising && feet >= c.compute_aabb().maxs.y - tolerance
            };
            // The character's own collision layers decide what stops it, as
            // they do for every other body: a hero whose filter leaves out
            // the enemy layer walks through enemies (and is told it touched
            // them below) instead of being blocked by them.
            let groups = collider.collision_groups();
            let filter = QueryFilter::default()
                .exclude_rigid_body(bh)
                .exclude_sensors()
                .groups(groups)
                .predicate(&blocks);
            let mut collisions = Vec::new();
            let mut mv = kcc.move_shape(
                dt,
                &self.rigid_body_set,
                &self.collider_set,
                &self.query_pipeline,
                shape.as_ref(),
                &shape_pos,
                desired,
                filter,
                |c| collisions.push(c),
            );

            // A ledge no taller than `step_height`: lift, move on, and settle
            // onto its top - if there is headroom, room to stand, and a floor.
            let blocked = desired.x.abs() > 1e-4 && mv.translation.x.abs() < desired.x.abs() * 0.5;
            if cc.step_height > 0.0 && blocked && (mv.grounded || cc.grounded) {
                let plain = KinematicCharacterController { snap_to_ground: None, ..kcc };
                let up = vector![0.0, cc.step_height];
                let here = Translation::from(mv.translation) * shape_pos;
                let headroom = self.query_pipeline.cast_shape(
                    &self.rigid_body_set,
                    &self.collider_set,
                    &here,
                    &Vector::y_axis(),
                    shape.as_ref(),
                    rapier2d::parry::query::details::ShapeCastOptions {
                        max_time_of_impact: cc.step_height,
                        target_distance: cc.skin,
                        stop_at_penetration: false,
                        compute_impact_geometry_on_penetration: true,
                    },
                    filter,
                );
                if headroom.is_none() {
                    let lifted = Translation::from(up) * here;
                    let rest = vector![desired.x - mv.translation.x, 0.0];
                    let side = plain.move_shape(
                        dt, &self.rigid_body_set, &self.collider_set, &self.query_pipeline, shape.as_ref(), &lifted,
                        rest, filter, |_| {},
                    );
                    if side.translation.x.abs() > rest.x.abs() * 0.5 {
                        let drop = cc.step_height + cc.skin * 2.0;
                        let mut landing = Vec::new();
                        let down = plain.move_shape(
                            dt, &self.rigid_body_set, &self.collider_set, &self.query_pipeline, shape.as_ref(),
                            &(Translation::from(side.translation) * lifted), vector![0.0, -drop], filter,
                            |c| landing.push(c),
                        );
                        let floor = cc.max_slope_deg.to_radians().dcos();
                        if landing.iter().any(|c| c.hit.normal1.y >= floor) {
                            mv.translation += up + side.translation + down.translation;
                            mv.grounded = true;
                            collisions.extend(landing);
                        }
                    }
                }
            }
            if cc.push_mass > 0.0 {
                kcc.solve_character_collision_impulses(
                    dt,
                    &mut self.rigid_body_set,
                    &self.collider_set,
                    &self.query_pipeline,
                    shape.as_ref(),
                    cc.push_mass,
                    &collisions,
                    filter,
                );
            }

            // What it hit, by the surface normal: ground within the climbable
            // slope, a ceiling overhead, a wall to either side.
            let floor = cc.max_slope_deg.to_radians().dcos();
            let mut ground: Option<(u64, [f32; 2])> = None;
            let (mut wall, mut ceiling) = (0, false);
            let mut touching: Vec<u64> = Vec::new();
            for c in &collisions {
                let n = c.hit.normal1;
                let other = self.collider_to_entity(c.handle);
                if let Some(o) = other {
                    touching.push(o);
                }
                if n.y >= floor {
                    if let Some(o) = other {
                        ground = Some((o, [n.x, n.y]));
                    }
                } else if n.y < -0.5 {
                    ceiling |= rising;
                } else if n.x.abs() > 0.5 {
                    wall = if n.x > 0.0 { -1 } else { 1 };
                }
            }
            let end = Translation::from(mv.translation) * shape_pos;
            if mv.grounded && ground.is_none() {
                // Grounded without a hit this frame (standing still): look down.
                let options = rapier2d::parry::query::details::ShapeCastOptions {
                    max_time_of_impact: cc.skin + cc.snap_to_ground.max(2.0),
                    target_distance: 0.0,
                    stop_at_penetration: true,
                    compute_impact_geometry_on_penetration: true,
                };
                if let Some((h, hit)) = self.query_pipeline.cast_shape(
                    &self.rigid_body_set,
                    &self.collider_set,
                    &end,
                    &-Vector::y_axis(),
                    shape.as_ref(),
                    options,
                    filter,
                ) && let Some(o) = self.collider_to_entity(h)
                {
                    ground = Some((o, [hit.normal1.x, hit.normal1.y]));
                }
            }
            // Everything overlapping it, whatever the layers: pickups and
            // hazards are usually on layers that do not stop anything.
            let sensors = QueryFilter::default().exclude_rigid_body(bh);
            self.query_pipeline.intersections_with_shape(
                &self.rigid_body_set,
                &self.collider_set,
                &end,
                shape.as_ref(),
                sensors,
                |h| {
                    if let Some(o) = self.collider_to_entity(h) {
                        touching.push(o);
                    }
                    true
                },
            );
            touching.sort_unstable();
            touching.dedup();

            let pos = start.translation.vector + mv.translation;
            if let Some(body) = self.rigid_body_set.get_mut(bh) {
                body.set_next_kinematic_position(Isometry::new(pos, start.rotation.angle()));
            }
            if let Ok(mut t) = world.get::<&mut Transform>(entity) {
                t.position.x = pos.x;
                t.position.y = pos.y;
            }
            // The body is where it was put, not where it was last frame: the
            // next frame's platform delta must not count this move.
            if let Ok(mut out) = world.get::<&mut CharacterController2D>(entity) {
                out.grounded = mv.grounded || ground.is_some() && !rising;
                out.ground_normal = ground.map_or([0.0, 1.0], |g| g.1);
                out.ground_entity = if out.grounded { ground.map(|g| g.0) } else { None };
                out.wall = wall;
                out.ceiling = ceiling;
                out.sliding = mv.is_sliding_down_slope;
                out.moved = [mv.translation.x / dt, mv.translation.y / dt];
                out.touching = touching;
            }
            self.kinematic_last.insert(id, (pos.x, pos.y));
        }
    }

    /// Apply a force to an entity's body (applied over time, resets each step).
    /// Push a body with a force for **this frame** - call it every frame
    /// the force should act, as a held thruster or a key does.
    ///
    /// It used to hand the force to rapier's `add_force`, which keeps a force
    /// on the body until `reset_forces` - and nothing ever reset it. So each
    /// frame's call stacked on the last and none ever went away: holding a
    /// key ramped the push without bound, releasing it changed nothing, and
    /// test-lab scene 56's ball accelerated until it tunnelled through a wall
    /// (BUGS #156). Now the frame's forces are summed here and applied by
    /// [`Self::step`] as `force * dt`, which also keeps a 165 Hz frame (that
    /// often runs no fixed step) from piling several frames into one step.
    pub fn apply_force(&mut self, entity_id: u64, fx: f32, fy: f32) {
        if let Some(&handle) = self.entity_to_body.get(&entity_id)
            && self.rigid_body_set.get(handle).is_some()
        {
            *self
                .frame_forces
                .entry(handle)
                .or_insert_with(rapier2d::na::Vector2::zeros) += rapier2d::na::Vector2::new(fx, fy);
        }
    }

    /// Apply an impulse to an entity's body (instant velocity change).
    pub fn apply_impulse(&mut self, entity_id: u64, ix: f32, iy: f32) {
        let impulse = rapier2d::na::Vector2::new(ix, iy);
        match self.body_or_part(entity_id) {
            Some(handle) => {
                if let Some(body) = self.rigid_body_set.get_mut(handle) {
                    body.apply_impulse(impulse, true);
                }
            }
            // Perhaps a bone going limp, or a body spawned, this frame: kept
            // until its body is built.
            None => *self.pending_impulses.entry(entity_id).or_insert_with(rapier2d::na::Vector2::zeros) += impulse,
        }
    }

    /// Set the linear velocity of an entity's body directly.
    /// Where `entity_id`'s body goes if it is set moving at `velocity` now:
    /// a copy of the whole world - every body, collider, joint and the
    /// contacts already touching - stepped `steps` times by the same
    /// pipeline, parameters and hooks the real step uses, so the answer is
    /// the step's own and not a ray's guess. Its place every `every` steps
    /// (and its first and last), the first other dynamic body it touches
    /// and where, and that body's path from then on. Area effects are not
    /// applied (`ecs::trajectory`). `None` for a body that is not dynamic
    /// or does not exist.
    pub fn predict(&self, entity_id: u64, velocity: [f32; 2], steps: u32, every: u32) -> Option<Prediction> {
        use rapier2d::prelude::*;
        let handle = *self.entity_to_body.get(&entity_id)?;
        if !self.rigid_body_set.get(handle)?.is_dynamic() {
            return None;
        }
        let mut bodies = self.rigid_body_set.clone();
        let mut colliders = self.collider_set.clone();
        let mut islands = self.island_manager.clone();
        let mut broad = self.broad_phase.clone();
        let mut narrow = self.narrow_phase.clone();
        let mut joints = self.impulse_joint_set.clone();
        let mut multibody = self.multibody_joint_set.clone();
        let mut ccd = self.ccd_solver.clone();
        let mut pipeline = PhysicsPipeline::new();
        bodies.get_mut(handle)?.set_linvel(vector![velocity[0], velocity[1]], true);
        let at = |bodies: &RigidBodySet, h: RigidBodyHandle| {
            let p = bodies[h].translation();
            [p.x, p.y]
        };
        let own: Vec<ColliderHandle> = bodies[handle].colliders().to_vec();
        let every = every.max(1);
        let mut out = Prediction { path: vec![at(&bodies, handle)], ..Default::default() };
        let mut struck: Option<RigidBodyHandle> = None;
        for step in 1..=steps {
            pipeline.step(
                &self.gravity,
                &self.integration_parameters,
                &mut islands,
                &mut broad,
                &mut narrow,
                &mut bodies,
                &mut colliders,
                &mut joints,
                &mut multibody,
                &mut ccd,
                None,
                &StepHooks,
                &(),
            );
            out.steps = step;
            if struck.is_none() {
                'find: for c in &own {
                    for pair in narrow.contact_pairs_with(*c) {
                        if !pair.has_any_active_contact {
                            continue;
                        }
                        let other = if pair.collider1 == *c { pair.collider2 } else { pair.collider1 };
                        let Some(parent) = colliders.get(other).and_then(|o| o.parent()) else { continue };
                        if parent != handle && bodies.get(parent).is_some_and(|b| b.is_dynamic()) {
                            struck = Some(parent);
                            out.hit = self.body_to_entity.get(&parent).copied();
                            out.hit_at = Some(at(&bodies, handle));
                            out.hit_path.push(at(&bodies, parent));
                            break 'find;
                        }
                    }
                }
            }
            let still = |h: RigidBodyHandle| bodies.get(h).is_none_or(|b| b.is_sleeping() || b.linvel().norm() < 0.5);
            let done = step == steps || (still(handle) && struck.is_none_or(still));
            if step % every == 0 || done {
                out.path.push(at(&bodies, handle));
                if let Some(h) = struck {
                    out.hit_path.push(at(&bodies, h));
                }
            }
            if done {
                break;
            }
        }
        Some(out)
    }

    /// A body's pose, velocity and spin, for a save state.
    pub fn body_state(&self, entity_id: u64) -> Option<BodyState> {
        let h = self.entity_to_body.get(&entity_id)?;
        let b = self.rigid_body_set.get(*h)?;
        let (p, v) = (b.translation(), b.linvel());
        Some(BodyState {
            position: [p.x, p.y],
            rotation: b.rotation().angle(),
            turn: [b.rotation().re, b.rotation().im],
            velocity: [v.x, v.y],
            spin: b.angvel(),
        })
    }

    /// Put a body back as a save state had it, and tell the sync so it is
    /// not taken for an edit made outside (nor blended from where it was).
    pub fn restore_body(&mut self, entity_id: u64, s: BodyState) {
        let Some(&h) = self.entity_to_body.get(&entity_id) else { return };
        let Some(b) = self.rigid_body_set.get_mut(h) else { return };
        let turn = rapier2d::na::UnitComplex::new_unchecked(rapier2d::na::Complex::new(s.turn[0], s.turn[1]));
        b.set_position(
            rapier2d::na::Isometry2::from_parts(rapier2d::na::Translation2::new(s.position[0], s.position[1]), turn),
            true,
        );
        if b.is_dynamic() {
            b.set_linvel(rapier2d::na::Vector2::new(s.velocity[0], s.velocity[1]), true);
            b.set_angvel(s.spin, true);
        }
        let pose = (s.position[0], s.position[1], s.rotation);
        self.written.insert(h, pose);
        self.prev_poses.insert(h, pose);
        self.written_velocity.insert(h, (s.velocity[0], s.velocity[1]));
    }

    /// A body's mass (its colliders' and any it was given).
    pub fn mass_of(&self, entity_id: u64) -> Option<f32> {
        let h = self.entity_to_body.get(&entity_id)?;
        self.rigid_body_set.get(*h).map(|b| b.mass())
    }

    pub fn set_velocity(&mut self, entity_id: u64, vx: f32, vy: f32) {
        let v = rapier2d::na::Vector2::new(vx, vy);
        match self.body_or_part(entity_id) {
            Some(handle) => {
                if let Some(body) = self.rigid_body_set.get_mut(handle) {
                    body.set_linvel(v, true);
                }
            }
            // Perhaps spawned this frame: kept until its body is built.
            None => {
                self.pending_velocity.insert(entity_id, v);
            }
        }
    }

    /// Get the linear velocity of an entity's body.
    pub fn get_velocity(&self, entity_id: u64) -> Option<(f32, f32)> {
        let handle = self.body_or_part(entity_id)?;
        let body = self.rigid_body_set.get(handle)?;
        let v = body.linvel();
        Some((v.x, v.y))
    }

    /// Where an entity's body is right now.
    ///
    /// The counterpart to [`Self::get_velocity`], which existed on its
    /// own for a long time: anything outside this module could ask how
    /// fast a body was going but not where it had got to, short of
    /// syncing the whole world back into the ECS.
    pub fn get_position(&self, entity_id: u64) -> Option<(f32, f32)> {
        let &handle = self.entity_to_body.get(&entity_id)?;
        let body = self.rigid_body_set.get(handle)?;
        let t = body.translation();
        Some((t.x, t.y))
    }

    /// Get the rotation of a body, in radians.
    pub fn get_rotation(&self, entity_id: u64) -> Option<f32> {
        let handle = self.entity_to_body.get(&entity_id)?;
        Some(self.rigid_body_set.get(*handle)?.rotation().angle())
    }

    /// Set the angular velocity of an entity's body.
    pub fn set_angular_velocity(&mut self, entity_id: u64, omega: f32) {
        if let Some(&handle) = self.entity_to_body.get(&entity_id)
            && let Some(body) = self.rigid_body_set.get_mut(handle)
        {
            body.set_angvel(omega, true);
        }
    }

    /// Get the angular velocity of an entity's body.
    pub fn get_angular_velocity(&self, entity_id: u64) -> Option<f32> {
        let &handle = self.entity_to_body.get(&entity_id)?;
        let body = self.rigid_body_set.get(handle)?;
        Some(body.angvel())
    }

    /// Set the world gravity (in m/s², scaled by `length_unit`).
    ///
    /// Wakes every dynamic body. Rapier skips sleeping bodies entirely, so a
    /// crate resting on the floor would never notice that gravity had changed —
    /// it stayed put until something else poked it, which is why this looked
    /// like "gravity only works after you press impulse". Everything that
    /// addresses one body already passes `wake_up: true`; changing the world
    /// out from under all of them is the case that had nobody to wake it.
    pub fn set_gravity(&mut self, gx: f32, gy: f32) {
        let lu = self.integration_parameters.length_unit;
        self.gravity = rapier2d::na::Vector2::new(gx * lu, gy * lu);
        // Handles first, then `get_mut` on each. `iter_mut` hands out `&mut
        // RigidBody` without telling the set the body was touched, so the
        // island manager never learns the body woke and can be left describing
        // a sleeping island that contains awake bodies. `get_mut` is the path
        // every other command here uses, and it does that bookkeeping.
        let dynamic: Vec<_> = self
            .rigid_body_set
            .iter()
            // Static and kinematic bodies are unaffected by gravity, and waking
            // them would only cost the solver work.
            .filter(|(_, body)| body.is_dynamic())
            .map(|(handle, _)| handle)
            .collect();
        for handle in dynamic {
            if let Some(body) = self.rigid_body_set.get_mut(handle) {
                body.wake_up(true);
            }
        }
    }

    /// Get the world gravity (in m/s², unscaled from internal game-units).
    pub fn get_gravity(&self) -> (f32, f32) {
        let lu = self.integration_parameters.length_unit;
        (self.gravity.x / lu, self.gravity.y / lu)
    }

    /// Change the body type of an entity's body at runtime.
    /// The body type the simulation currently holds for `entity_id`, so the
    /// sync can tell an edited `RigidBody2D` from the one it built.
    pub fn body_type_of(&self, entity_id: u64) -> Option<BodyType> {
        use rapier2d::prelude::RigidBodyType;
        let body = self.rigid_body_set.get(*self.entity_to_body.get(&entity_id)?)?;
        Some(match body.body_type() {
            RigidBodyType::Dynamic => BodyType::Dynamic,
            RigidBodyType::Fixed => BodyType::Static,
            RigidBodyType::KinematicPositionBased | RigidBodyType::KinematicVelocityBased => BodyType::Kinematic,
        })
    }

    pub fn set_body_type(&mut self, entity_id: u64, body_type: BodyType) {
        use rapier2d::prelude::RigidBodyType;
        if let Some(&handle) = self.entity_to_body.get(&entity_id)
            && let Some(body) = self.rigid_body_set.get_mut(handle)
        {
            let rt = match body_type {
                BodyType::Dynamic => RigidBodyType::Dynamic,
                BodyType::Static => RigidBodyType::Fixed,
                BodyType::Kinematic => RigidBodyType::KinematicPositionBased,
            };
            body.set_body_type(rt, true);
        }
    }

    /// Cast a ray and return the first hit entity.
    pub fn raycast(
        &self,
        origin_x: f32,
        origin_y: f32,
        dir_x: f32,
        dir_y: f32,
        max_dist: f32,
    ) -> Option<RayHit> {
        use rapier2d::prelude::{QueryFilter, Ray};

        // A unit direction, so `max_dist` and the reported `distance` are in
        // world units. Rapier measures time of impact in multiples of the
        // direction's length: `raycast(x, y, 10, 0, 100)` used to reach 1000
        // units and report a hit at 400 as distance 40.
        let dir = rapier2d::na::Vector2::new(dir_x, dir_y);
        let len = dir.norm();
        if len <= f32::EPSILON || !len.is_finite() {
            return None;
        }
        let ray = Ray::new(rapier2d::na::Point2::new(origin_x, origin_y), dir / len);

        let filter = QueryFilter::default();
        let result = self.query_pipeline.cast_ray_and_get_normal(
            &self.rigid_body_set,
            &self.collider_set,
            &ray,
            max_dist,
            true,
            filter,
        );

        result.map(|(handle, intersection)| {
            let entity_id = self.collider_to_entity(handle).unwrap_or(0);
            let point = ray.point_at(intersection.time_of_impact);
            RayHit {
                entity_id,
                point_x: point.x,
                point_y: point.y,
                normal_x: intersection.normal.x,
                normal_y: intersection.normal.y,
                distance: intersection.time_of_impact,
            }
        })
    }

    /// Every collider with the entity that owns it, for the shape queries.
    fn owned_shapes(&self) -> impl Iterator<Item = (u64, &dyn rapier2d::prelude::Shape, &rapier2d::prelude::Isometry<f32>)> {
        self.collider_set
            .iter()
            .filter_map(|(h, c)| Some((self.collider_to_entity(h)?, c.shape(), c.position())))
    }

    /// Every entity a collider of which overlaps `q` (`physics::query`).
    pub fn overlap(&self, q: &query::QueryShape) -> Vec<u64> {
        query::overlaps(self.owned_shapes(), q)
    }

    /// A circle swept along a line: `raycast` with a width (`physics::query`).
    pub fn circle_cast(&self, ox: f32, oy: f32, dx: f32, dy: f32, radius: f32, max_dist: f32) -> Option<RayHit> {
        query::circle_cast(self.owned_shapes(), ox, oy, dx, dy, radius, max_dist)
    }

    /// Look up which entity owns a collider.
    fn collider_to_entity(&self, collider_handle: rapier2d::prelude::ColliderHandle) -> Option<u64> {
        let collider = self.collider_set.get(collider_handle)?;
        let body_handle = collider.parent()?;
        self.body_to_entity.get(&body_handle).copied()
    }
}

/// Stub PhysicsWorld when the physics feature is disabled.
/// A collider's outline in world points, for clipping against an area
/// effect's box: its corners for a box or a polygon, a polygon for a ball or
/// a capsule, and its bounds for anything else.
#[cfg(feature = "physics")]
fn collider_outline(c: &rapier2d::prelude::Collider) -> Vec<glam::Vec2> {
    use rapier2d::na::Point2;
    let iso = c.position();
    let world = |p: Point2<f32>| {
        let q = iso * p;
        glam::Vec2::new(q.x, q.y)
    };
    let shape = c.shape();
    if let Some(b) = shape.as_cuboid() {
        let h = b.half_extents;
        return [(-h.x, -h.y), (h.x, -h.y), (h.x, h.y), (-h.x, h.y)].iter().map(|(x, y)| world(Point2::new(*x, *y))).collect();
    }
    if let Some(p) = shape.as_convex_polygon() {
        return p.points().iter().map(|p| world(*p)).collect();
    }
    if let Some(b) = shape.as_ball() {
        let o = world(Point2::origin());
        return crate::ecs::area_effect::circle_outline(o, b.radius, 16);
    }
    if let Some(cap) = shape.as_capsule() {
        // Two half circles joined: eight points round each end.
        let (a, b, r) = (cap.segment.a, cap.segment.b, cap.radius);
        let (wa, wb) = (world(a), world(b));
        let along = (wb - wa).normalize_or(glam::Vec2::Y);
        let mut out = Vec::with_capacity(18);
        for i in 0..=8 {
            let t = std::f32::consts::PI * i as f32 / 8.0;
            out.push(wb + glam::Vec2::from_angle(t - std::f32::consts::FRAC_PI_2).rotate(along) * r);
        }
        for i in 0..=8 {
            let t = std::f32::consts::PI * i as f32 / 8.0;
            out.push(wa + glam::Vec2::from_angle(t + std::f32::consts::FRAC_PI_2).rotate(along) * r);
        }
        return out;
    }
    let b = c.compute_aabb();
    vec![
        glam::Vec2::new(b.mins.x, b.mins.y),
        glam::Vec2::new(b.maxs.x, b.mins.y),
        glam::Vec2::new(b.maxs.x, b.maxs.y),
        glam::Vec2::new(b.mins.x, b.maxs.y),
    ]
}

#[cfg(not(feature = "physics"))]
pub struct PhysicsWorld;

#[cfg(not(feature = "physics"))]
impl PhysicsWorld {
    pub fn new(_gravity_x: f32, _gravity_y: f32, _length_unit: f32) -> Self {
        Self
    }
    pub fn step(&mut self, _dt: f32) -> Vec<ContactEvent> { Vec::new() }
    pub fn begin_frame(&mut self, _dt: f32) -> u32 { 0 }
    pub fn step_once(&mut self) {}
    pub fn flush_forces(&mut self, _dt: f32) {}
    pub fn end_frame(&mut self) -> Vec<ContactEvent> { Vec::new() }
    pub fn fixed_dt(&self) -> f32 { 1.0 / 60.0 }
    pub fn sync_carved_colliders(&mut self, _world: &hecs::World) {}
    #[cfg(feature = "spline_shape")]
    pub fn sync_spline_colliders(&mut self, _world: &hecs::World) {}
    pub fn sync_smooth_terrain(&mut self, _world: &hecs::World) {}
    pub fn sync_area_effects(&mut self, _world: &hecs::World) {}
    #[cfg(feature = "ragdoll")]
    pub fn sync_ragdolls(&mut self, _world: &hecs::World) {}
    pub fn apply_pending(&mut self, _entity_id: u64) {}
    pub fn set_gravity(&mut self, _gx: f32, _gy: f32) {}
    pub fn apply_force(&mut self, _entity_id: u64, _fx: f32, _fy: f32) {}
    pub fn apply_impulse(&mut self, _entity_id: u64, _ix: f32, _iy: f32) {}
    pub fn set_velocity(&mut self, _entity_id: u64, _vx: f32, _vy: f32) {}
    pub fn mass_of(&self, _entity_id: u64) -> Option<f32> { None }
    pub fn body_state(&self, _entity_id: u64) -> Option<BodyState> { None }
    pub fn restore_body(&mut self, _entity_id: u64, _s: BodyState) {}
    pub fn body_type_of(&self, _entity_id: u64) -> Option<crate::ecs::components::BodyType> { None }
    pub fn predict(&self, _entity_id: u64, _velocity: [f32; 2], _steps: u32, _every: u32) -> Option<Prediction> { None }
    pub fn set_angular_velocity(&mut self, _entity_id: u64, _omega: f32) {}
    pub fn get_position(&self, _entity_id: u64) -> Option<(f32, f32)> { None }
    pub fn get_rotation(&self, _entity_id: u64) -> Option<f32> { None }
    pub fn get_velocity(&self, _entity_id: u64) -> Option<(f32, f32)> { None }
    pub fn get_angular_velocity(&self, _entity_id: u64) -> Option<f32> { None }
    pub fn remove_body(&mut self, _entity_id: u64) {}
    pub fn remove_collider(&mut self, _entity_id: u64) {}
    pub fn sync_conveyors(&mut self, _world: &hecs::World) {}
    pub fn sync_body_settings(&mut self, _world: &hecs::World) {}
    pub fn sprite_shape_key(&self, _entity_id: u64) -> Option<u64> { None }
    pub fn set_sprite_shape_key(&mut self, _entity_id: u64, _key: u64) {}
    pub fn set_body_type(&mut self, _entity_id: u64, _body_type: crate::ecs::components::BodyType) {}
    pub fn owner_of(&self, _entity_id: u64) -> Option<hecs::Entity> {
        None
    }
    pub fn raycast(&self, _ox: f32, _oy: f32, _dx: f32, _dy: f32, _max_dist: f32) -> Option<RayHit> { None }
    pub fn add_joint(&mut self, _entity_a: u64, _entity_b: u64, _joint: &crate::ecs::components::Joint2D) {}
    pub fn remove_joint(&mut self, _entity_id: u64) {}
    pub fn add_target_joint(&mut self, _entity: u64, _joint: &crate::ecs::components::Joint2D, _follow: Option<u64>, _at: [f32; 2]) {}
    pub fn sync_joint_targets(&mut self, _position_of: impl Fn(u64) -> Option<[f32; 2]>) {}
    pub fn target_follows(&self) -> Vec<u64> { Vec::new() }
    pub fn set_motor_speed(&mut self, _entity_id: u64, _speed: f32) {}
    pub fn move_characters(&mut self, _world: &mut hecs::World, _dt: f32) {}
    pub fn sync_tilemap_colliders(&mut self, _world: &hecs::World) {}
    pub fn blend_transforms_for_render(&mut self, _world: &mut hecs::World) {}
    pub fn restore_stepped_poses(&mut self, _world: &mut hecs::World) {}
    pub fn step_hz(&self) -> f32 { 60.0 }
    pub fn set_step_hz(&mut self, _hz: f32) {}
    pub fn set_fixed_timestep(&mut self, _seconds: f32) {}
}
