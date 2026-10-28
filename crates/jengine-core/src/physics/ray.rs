//! `raycast` for the runtimes that cannot borrow their physics world.
//!
//! Native answers a ray by lending the live `PhysicsWorld` to the script
//! snapshot for the length of one tick. The browser hosts cannot: they own
//! their world in an `Rc<RefCell<…>>` and scripts run *inside* a
//! `borrow_mut()`, so a callback that reached back for the app would panic
//! on a double borrow. That is why `raycast` was specified against a
//! `__jengine_play.state.physics_raycast` hook and then never installed by
//! anybody - it was bound in the editor, returned `null` for years, and
//! every name-level parity check counted it as present.
//!
//! [`crate::scripting::bridge::adapters::js::nav`] solved the same problem
//! for pathfinding: the callback owns its own data, refreshed once a frame
//! before scripts run. This is that, for rays.
//!
//! # Why this does not just reimplement ray-vs-shape
//!
//! Because a raycast that disagreed with the simulation about where a wall
//! is would be worse than no raycast at all. So:
//!
//! - the shapes come from [`super::collider_builder_for`], the **same**
//!   function `add_collider` builds the simulation's colliders with;
//! - the intersection is parry's own `RayCast`, which is the code rapier
//!   itself calls once its broadphase has chosen a candidate.
//!
//! It holds what the physics world would: every `Collider2D`, and since
//! Oct 2026 the colliding layers of every tilemap and the land of every
//! carvable sprite too - built by the same functions the physics sync uses
//! (`tile_run_collider`, `carved_colliders`), and kept per entity until what
//! they are made of changes. Before that a browser ray went straight through
//! tile walls and carved ground that a native ray stopped at. A hit names a
//! spawned entity by the handle the script holds ([`RaySnapshot::refresh_named`]).
//!
//! What is missing is only the broadphase: this walks every collider
//! instead of querying a BVH. A browser scene has tens to hundreds of
//! them, and a ray is cast a handful of times a frame, so the walk is not
//! worth a tree. `the_snapshot_and_the_live_world_agree` in
//! `tests/raycast_snapshot_matches_rapier.rs` is what holds the two
//! together - it fires 216 rays at five shapes and requires the same
//! entity, distance and normal from both.

use std::collections::HashMap;

use rapier2d::prelude::{Isometry, Ray, SharedShape};

use super::RayHit;
use crate::ecs::components::{Collider2D, Transform};

/// The shapes built for one tilemap or carvable, local to its entity, and
/// the key of what they were built from.
type Built = (u64, Vec<(SharedShape, Isometry<f32>)>);

/// Every collider in the world as of the start of this frame.
///
/// Shapes are `SharedShape`, which is an `Arc`, and each is kept until what it
/// is built from changes, so a refresh is a hash and a refcount bump per
/// collider rather than a geometry rebuild. (It used to say that and build
/// every shape every frame anyway - see `colliders`.)
#[derive(Default)]
pub struct RaySnapshot {
    shapes: Vec<(u64, SharedShape, Isometry<f32>)>,
    /// Tilemap and carved shapes by entity id, rebuilt when their key moves.
    built: HashMap<u64, Built>,
    /// Each `Collider2D`'s built shape by entity id and
    /// [`super::shape_key`]. Building every one every frame cost a convex
    /// decomposition per traced outline per frame: the showcase's Baggage
    /// Claim, 26 bags with a `sprite_collider` each, spent over 100 ms a
    /// frame here in the editor and on the web, natively nothing.
    colliders: HashMap<u64, (u64, SharedShape)>,
}

/// A key for floats compared exactly.
fn mix(h: &mut u64, v: u64) {
    *h ^= v;
    *h = h.wrapping_mul(0x0000_0100_0000_01b3);
}

impl RaySnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many colliders are in the snapshot. For tests and diagnostics.
    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// Rebuild from the live world. Call once per frame **before** scripts
    /// run, the way the hosts refresh their nav snapshot.
    ///
    /// A sensor is included, deliberately: a sensor still has a shape and a
    /// script asking "what is along this line" wants to know about a
    /// trigger volume. Rapier's own `QueryFilter::default()` includes them
    /// too, so this matches what native answers.
    pub fn refresh(&mut self, world: &hecs::World) {
        self.refresh_named(world, &HashMap::new());
    }

    /// [`Self::refresh`], reporting an entity a script spawned by the handle
    /// the script holds for it: `names` maps entity id to handle (the host's
    /// spawn table, turned round). Without it a script could not tell its own
    /// bomb from the ground the ray found.
    pub fn refresh_named(&mut self, world: &hecs::World, names: &HashMap<u64, u64>) {
        let name = |id: u64| names.get(&id).copied().unwrap_or(id);
        self.shapes.clear();
        let mut seen = std::collections::HashSet::new();
        for (entity, (collider, transform)) in world.query::<(&Collider2D, &Transform)>().iter() {
            let id = entity.id() as u64;
            seen.insert(id);
            let key = super::shape_key(&collider.shape);
            if self.colliders.get(&id).is_none_or(|(k, _)| *k != key) {
                let built = super::collider_builder_for(&collider.shape, id).build();
                self.colliders.insert(id, (key, built.shared_shape().clone()));
            }
            let shape = self.colliders[&id].1.clone();
            // The collider's own offset is in the body's frame, so it is
            // rotated with the body before being added to the position -
            // exactly as rapier composes a collider's local transform onto
            // its parent.
            let rot = rapier2d::na::Rotation2::new(transform.rotation);
            let local = rot * rapier2d::na::Vector2::new(collider.offset[0], collider.offset[1]);
            let iso = Isometry::new(
                rapier2d::na::Vector2::new(
                    transform.position.x + local.x,
                    transform.position.y + local.y,
                ),
                transform.rotation,
            );
            self.shapes.push((name(id), shape, iso));
        }
        // An entity gone, or its id about to be reused, keeps no shape.
        self.colliders.retain(|id, _| seen.contains(id));
        let mut live = std::collections::HashSet::new();
        // Tilemaps: each colliding layer's cells, merged into runs.
        for (entity, (tm, tr, spec)) in world
            .query::<(&crate::tilemap::Tilemap, &Transform, Option<&crate::ecs::components::TilemapCollider>)>()
            .iter()
        {
            let layers: Vec<(usize, bool)> = tm
                .layers
                .iter()
                .enumerate()
                .filter_map(|(i, l)| {
                    let one_way = spec.is_some_and(|s| s.one_way.contains(&l.name));
                    let solid = l.collision || spec.is_some_and(|s| s.solid.contains(&l.name));
                    (one_way || solid).then_some((i, one_way && !l.collision))
                })
                .collect();
            if layers.is_empty() {
                continue;
            }
            let id = entity.id() as u64;
            live.insert(id);
            let mut key = 0xcbf2_9ce4_8422_2325u64;
            mix(&mut key, tm.revision_key());
            mix(&mut key, tm.shapes_key());
            for (i, one_way) in &layers {
                mix(&mut key, *i as u64 * 2 + *one_way as u64);
            }
            if self.built.get(&id).is_none_or(|(k, _)| *k != key) {
                let mut shapes = Vec::new();
                for (i, one_way) in &layers {
                    let l = &tm.layers[*i];
                    for b in super::tile_area_colliders(tm, l, (0, 0, l.width, l.height), *one_way) {
                        let col = b.build();
                        shapes.push((col.shared_shape().clone(), *col.position()));
                    }
                }
                self.built.insert(id, (key, shapes));
            }
            let placed = crate::tilemap::world_origin(world, entity, tr);
            let at = Isometry::translation(placed.x, placed.y);
            for (shape, local) in &self.built[&id].1 {
                self.shapes.push((name(id), shape.clone(), at * local));
            }
        }
        // Carved land (`ecs::carve`): the pieces its physics body is made of.
        for (entity, (c, s, t, w)) in world
            .query::<(
                &crate::ecs::carve::Carvable,
                &crate::ecs::components::SpriteComponent,
                &Transform,
                Option<&crate::ecs::components::WorldTransform>,
            )>()
            .iter()
        {
            if !c.collider || c.state.texture.is_none() {
                continue;
            }
            let id = entity.id() as u64;
            live.insert(id);
            let wt = w.cloned().unwrap_or_else(|| crate::ecs::components::WorldTransform::from(t));
            let mut key = 0x8422_2325_cbf2_9ce4u64;
            mix(&mut key, c.state.rev);
            mix(&mut key, wt.scale.x.to_bits() as u64);
            mix(&mut key, wt.scale.y.to_bits() as u64);
            if self.built.get(&id).is_none_or(|(k, _)| *k != key) {
                let shapes = super::carved_colliders(c, s, wt.scale)
                    .into_iter()
                    .map(|b| {
                        let col = b.build();
                        (col.shared_shape().clone(), *col.position())
                    })
                    .collect();
                self.built.insert(id, (key, shapes));
            }
            let at = Isometry::new(rapier2d::na::Vector2::new(wt.position.x, wt.position.y), wt.rotation);
            for (shape, local) in &self.built[&id].1 {
                self.shapes.push((name(id), shape.clone(), at * local));
            }
        }
        // Smooth terrain: its cells, as its physics body has them.
        #[cfg(feature = "smooth_terrain")]
        for (entity, (terrain, t, w)) in world
            .query::<(
                &crate::ecs::smooth_terrain::SmoothTerrain,
                &Transform,
                Option<&crate::ecs::components::WorldTransform>,
            )>()
            .iter()
        {
            if !terrain.collider || terrain.rev == 0 {
                continue;
            }
            let id = entity.id() as u64;
            live.insert(id);
            let wt = w.cloned().unwrap_or_else(|| crate::ecs::components::WorldTransform::from(t));
            let mut key = 0x9ce4_8422_2325_cbf2u64;
            mix(&mut key, terrain.rev);
            mix(&mut key, wt.scale.x.to_bits() as u64);
            mix(&mut key, wt.scale.y.to_bits() as u64);
            if self.built.get(&id).is_none_or(|(k, _)| *k != key) {
                let shapes = super::terrain_colliders(terrain, wt.scale)
                    .into_iter()
                    .map(|b| {
                        let col = b.build();
                        (col.shared_shape().clone(), *col.position())
                    })
                    .collect();
                self.built.insert(id, (key, shapes));
            }
            let at = Isometry::new(rapier2d::na::Vector2::new(wt.position.x, wt.position.y), wt.rotation);
            for (shape, local) in &self.built[&id].1 {
                self.shapes.push((name(id), shape.clone(), at * local));
            }
        }
        // Spline shapes: their fill, as their physics body has it.
        #[cfg(feature = "spline_shape")]
        for (entity, (shape, t, w)) in world
            .query::<(
                &crate::renderer::spline_shape::SplineShape,
                &Transform,
                Option<&crate::ecs::components::WorldTransform>,
            )>()
            .iter()
        {
            if !shape.collider {
                continue;
            }
            let id = entity.id() as u64;
            live.insert(id);
            let wt = w.cloned().unwrap_or_else(|| crate::ecs::components::WorldTransform::from(t));
            let mut key = 0x2325_cbf2_9ce4_8422u64;
            mix(&mut key, shape.key());
            mix(&mut key, wt.scale.x.to_bits() as u64);
            mix(&mut key, wt.scale.y.to_bits() as u64);
            if self.built.get(&id).is_none_or(|(k, _)| *k != key) {
                let shapes = super::spline_colliders(shape, wt.scale)
                    .into_iter()
                    .map(|b| {
                        let col = b.build();
                        (col.shared_shape().clone(), *col.position())
                    })
                    .collect();
                self.built.insert(id, (key, shapes));
            }
            let at = Isometry::new(rapier2d::na::Vector2::new(wt.position.x, wt.position.y), wt.rotation);
            for (shape, local) in &self.built[&id].1 {
                self.shapes.push((name(id), shape.clone(), at * local));
            }
        }
        self.built.retain(|id, _| live.contains(id));
    }

    /// Every entity a shape of which overlaps `q` (`physics::query`), as
    /// `PhysicsWorld::overlap` answers natively.
    pub fn overlap(&self, q: &super::query::QueryShape) -> Vec<u64> {
        super::query::overlaps(self.shapes.iter().map(|(id, s, iso)| (*id, &**s, iso)), q)
    }

    /// A circle swept along a line, as `PhysicsWorld::circle_cast` answers.
    pub fn circle_cast(&self, ox: f32, oy: f32, dx: f32, dy: f32, radius: f32, max_dist: f32) -> Option<RayHit> {
        super::query::circle_cast(self.shapes.iter().map(|(id, s, iso)| (*id, &**s, iso)), ox, oy, dx, dy, radius, max_dist)
    }

    /// Cast a ray and return the nearest hit.
    ///
    /// The contract is `PhysicsWorld::raycast`'s, to the letter: `dir` is
    /// normalised here, so `max_dist` and the reported distance are in
    /// world units whatever length the caller passed. Rapier measures time
    /// of impact in multiples of the direction's length, and a version that
    /// forgot this reported a hit at 400 units as a distance of 40.
    pub fn cast(&self, ox: f32, oy: f32, dx: f32, dy: f32, max_dist: f32) -> Option<RayHit> {
        let dir = rapier2d::na::Vector2::new(dx, dy);
        let len = dir.norm();
        if len <= f32::EPSILON || !len.is_finite() {
            return None;
        }
        let ray = Ray::new(rapier2d::na::Point2::new(ox, oy), dir / len);

        let mut best: Option<(f32, u64, rapier2d::na::Vector2<f32>)> = None;
        for (id, shape, iso) in &self.shapes {
            // `solid = true`, matching `cast_ray_and_get_normal`: a ray
            // starting inside a shape hits it at distance zero rather than
            // passing out through the far side.
            let Some(hit) = shape.cast_ray_and_get_normal(iso, &ray, max_dist, true) else {
                continue;
            };
            if best.is_none_or(|(d, _, _)| hit.time_of_impact < d) {
                best = Some((hit.time_of_impact, *id, hit.normal));
            }
        }

        best.map(|(toi, entity_id, normal)| {
            let point = ray.point_at(toi);
            RayHit {
                entity_id,
                point_x: point.x,
                point_y: point.y,
                normal_x: normal.x,
                normal_y: normal.y,
                distance: toi,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::ColliderShape;

    /// A traced outline, the shape a `sprite_collider` writes: an L of six
    /// corners, so the decomposition has real work to do.
    fn outline() -> Collider2D {
        let mut c = Collider2D::default();
        c.shape = ColliderShape::ConvexDecomposition {
            vertices: vec![[0.0, 0.0], [40.0, 0.0], [40.0, 10.0], [10.0, 10.0], [10.0, 40.0], [0.0, 40.0]],
            loops: Vec::new(),
        };
        c
    }

    fn shape_of(snap: &RaySnapshot) -> SharedShape {
        snap.shapes[0].1.clone()
    }

    #[test]
    fn a_collider_is_built_once_not_every_frame() {
        // The browsers refresh the snapshot every frame. Rebuilding each
        // shape there ran a convex decomposition per traced outline per
        // frame - over 100 ms a frame in Baggage Claim.
        let mut world = hecs::World::new();
        let e = world.spawn((outline(), Transform::default()));
        let mut snap = RaySnapshot::new();
        snap.refresh(&world);
        let first = shape_of(&snap);

        // Moving it is not changing it.
        world.get::<&mut Transform>(e).unwrap().position.x = 300.0;
        snap.refresh(&world);
        assert!(std::sync::Arc::ptr_eq(&first.0, &shape_of(&snap).0), "rebuilt a shape that did not change");
        assert!(snap.cast(300.0 + 5.0, 100.0, 0.0, -1.0, 500.0).is_some(), "the ray missed the moved shape");

        // Changing it is.
        world.get::<&mut Collider2D>(e).unwrap().shape = ColliderShape::Circle { radius: 5.0 };
        snap.refresh(&world);
        assert!(!std::sync::Arc::ptr_eq(&first.0, &shape_of(&snap).0), "kept a shape that changed");
    }

    #[test]
    fn a_despawned_collider_leaves_no_shape_behind() {
        let mut world = hecs::World::new();
        let e = world.spawn((outline(), Transform::default()));
        let mut snap = RaySnapshot::new();
        snap.refresh(&world);
        world.despawn(e).unwrap();
        snap.refresh(&world);
        assert!(snap.is_empty());
        assert!(snap.colliders.is_empty(), "a cache entry outlived its entity");
    }
}
