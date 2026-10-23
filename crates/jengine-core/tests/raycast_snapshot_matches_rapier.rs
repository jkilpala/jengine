//! `raycast` for the runtimes that cannot borrow their physics world, and
//! the one thing that has to be true of it: it must agree with rapier.
//!
//! Native answers a ray by lending the live `PhysicsWorld` to the script
//! snapshot for the length of one tick. The editor and web hosts cannot:
//! they own their world in an `Rc<RefCell<…>>` and scripts run *inside* a
//! `borrow_mut()`, so a callback that reached back for the app would panic
//! on a double borrow. That is why `raycast` was specified against a
//! `__jengine_play.state.physics_raycast` hook that **no host ever
//! installed** — it was in the editor's `BRIDGE_FUNCTIONS`, so every
//! name-level parity check counted it as present, and it returned `null`
//! silently for years.
//!
//! `physics::ray::RaySnapshot` is the answer, and it is the same one
//! `adapters/js/nav.rs` used for pathfinding: the callback owns its own
//! data, refreshed once a frame before scripts run.
//!
//! The risk in that design is divergence. A raycast that disagreed with the
//! simulation about where a wall is would be worse than no raycast, so the
//! snapshot builds its shapes with `physics::collider_builder_for` — the
//! same function `add_collider` uses — and intersects with parry's own
//! `RayCast`, which is what rapier calls once its broadphase has picked a
//! candidate. All that is missing is the broadphase.
//!
//! `the_snapshot_and_the_live_world_agree` is what holds them together.

#![cfg(feature = "physics")]

use hecs::World;
use jengine_core::ecs::components::{
    BodyType, Collider2D, ColliderShape, RigidBody2D, Transform,
};
use jengine_core::physics::PhysicsWorld;
use jengine_core::physics::ray::RaySnapshot;

const LENGTH_UNIT: f32 = 100.0;

/// The same scene twice: as ECS components, and as a live physics world.
fn both_ways(bodies: &[(u64, ColliderShape, (f32, f32), f32)]) -> (RaySnapshot, PhysicsWorld) {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
    for (id, shape, (x, y), rot) in bodies {
        let collider = Collider2D { shape: shape.clone(), ..Default::default() };
        world.spawn((
            Transform {
                position: glam::Vec2::new(*x, *y),
                rotation: *rot,
                scale: glam::Vec2::ONE,
            },
            collider.clone(),
        ));
        physics.add_body(
            *id,
            &RigidBody2D { body_type: BodyType::Static, ..Default::default() },
            *x,
            *y,
            *rot,
        );
        physics.add_collider(*id, &collider);
    }
    physics.step(1.0 / 60.0);
    let mut snap = RaySnapshot::new();
    snap.refresh(&world);
    (snap, physics)
}

/// Five colliders, deliberately including turned ones and every shape a
/// browser scene is likely to hold. An axis-aligned box would hide a
/// rotation the two sides composed differently.
fn scene() -> Vec<(u64, ColliderShape, (f32, f32), f32)> {
    vec![
        (0, ColliderShape::Box { width: 40.0, height: 400.0 }, (300.0, 0.0), 0.0),
        (1, ColliderShape::Circle { radius: 60.0 }, (-200.0, 120.0), 0.0),
        (2, ColliderShape::Box { width: 300.0, height: 30.0 }, (100.0, -260.0), 0.6),
        (3, ColliderShape::Capsule { half_height: 50.0, radius: 24.0 }, (-80.0, -120.0), 0.0),
        (
            4,
            ColliderShape::ConvexPolygon {
                vertices: vec![[-60.0, -40.0], [60.0, -40.0], [40.0, 40.0], [-40.0, 40.0]],
            },
            (480.0, 200.0),
            -0.35,
        ),
    ]
}

#[test]
fn the_snapshot_and_the_live_world_agree() {
    // **The test the whole design rests on.** If these two ever disagree, a
    // script gets a different answer about where a wall is depending on
    // which runtime it is in - which is the exact class of bug this repo
    // has the most of, and the reason `raycast` was left unimplemented
    // rather than approximated.
    let (snap, physics) = both_ways(&scene());
    assert_eq!(snap.len(), 5, "the snapshot should hold every collider");

    let mut compared = 0;
    let mut hits = 0;
    for i in 0..72 {
        let a = i as f32 * std::f32::consts::TAU / 72.0;
        for (ox, oy) in [(0.0, 0.0), (-400.0, 0.0), (200.0, 300.0)] {
            let (dx, dy) = (a.cos(), a.sin());
            let live = physics.raycast(ox, oy, dx, dy, 1200.0);
            let snapped = snap.cast(ox, oy, dx, dy, 1200.0);
            compared += 1;
            match (&live, &snapped) {
                (None, None) => {}
                (Some(l), Some(s)) => {
                    hits += 1;
                    assert_eq!(
                        l.entity_id, s.entity_id,
                        "different entity from ({ox}, {oy}) at angle {a:.2}"
                    );
                    assert!(
                        (l.distance - s.distance).abs() < 0.5,
                        "different distance at angle {a:.2}: {} vs {}",
                        l.distance,
                        s.distance
                    );
                    assert!(
                        (l.normal_x - s.normal_x).abs() < 0.02
                            && (l.normal_y - s.normal_y).abs() < 0.02,
                        "different normal at angle {a:.2}: ({}, {}) vs ({}, {})",
                        l.normal_x,
                        l.normal_y,
                        s.normal_x,
                        s.normal_y
                    );
                }
                _ => panic!(
                    "one found a hit and the other did not, from ({ox}, {oy}) at angle {a:.2}: \
                     rapier {live:?}, snapshot {snapped:?}"
                ),
            }
        }
    }
    assert_eq!(compared, 216);
    assert!(hits > 60, "only {hits} of {compared} rays hit anything - the scene is too sparse");
}

#[test]
fn the_snapshot_respects_max_dist_the_same_way() {
    // Including either side of the boundary: the near face of the wall at
    // x 300 is at 280, so 281 hits and 280 is the edge case.
    let (snap, physics) = both_ways(&scene());
    for max in [50.0f32, 150.0, 279.0, 281.0, 600.0] {
        let live = physics.raycast(0.0, 0.0, 1.0, 0.0, max).map(|h| h.entity_id);
        let snapped = snap.cast(0.0, 0.0, 1.0, 0.0, max).map(|h| h.entity_id);
        assert_eq!(live, snapped, "they disagree about a ray of length {max}");
    }
}

#[test]
fn the_snapshot_normalises_its_direction_too() {
    // Rapier measures time of impact in multiples of the direction's
    // length, so a version that forgot this reports a hit at 280 units as
    // a distance of 28 - and `max_dist` means something else entirely.
    let (snap, _physics) = both_ways(&scene());
    let unit = snap.cast(0.0, 0.0, 1.0, 0.0, 1000.0).unwrap();
    let long = snap.cast(0.0, 0.0, 25.0, 0.0, 1000.0).unwrap();
    assert!((unit.distance - long.distance).abs() < 0.5, "{} vs {}", unit.distance, long.distance);
    assert!(snap.cast(0.0, 0.0, 0.0, 0.0, 1000.0).is_none(), "a zero direction is not a ray");
}

#[test]
fn the_nearest_hit_wins_even_though_there_is_no_broadphase() {
    // The one thing walking a list can get wrong that a BVH cannot.
    let (snap, _physics) = both_ways(&[
        (0, ColliderShape::Box { width: 40.0, height: 400.0 }, (600.0, 0.0), 0.0),
        (1, ColliderShape::Box { width: 40.0, height: 400.0 }, (300.0, 0.0), 0.0),
        (2, ColliderShape::Box { width: 40.0, height: 400.0 }, (900.0, 0.0), 0.0),
    ]);
    let hit = snap.cast(0.0, 0.0, 1.0, 0.0, 2000.0).unwrap();
    assert_eq!(hit.entity_id, 1, "the wall at 300 is the nearest, whatever order they were added");
}

#[test]
fn an_empty_snapshot_finds_nothing_and_a_refresh_picks_up_a_new_wall() {
    // The refresh is what makes it current. A host that forgot to call it
    // would answer from the scene as it was when play started, which is
    // worse than answering nothing, because it looks right at first.
    let mut world = World::new();
    let mut snap = RaySnapshot::new();
    snap.refresh(&world);
    assert!(snap.is_empty());
    assert!(snap.cast(0.0, 0.0, 1.0, 0.0, 1000.0).is_none());

    world.spawn((
        Transform { position: glam::Vec2::new(300.0, 0.0), ..Default::default() },
        Collider2D {
            shape: ColliderShape::Box { width: 40.0, height: 400.0 },
            ..Default::default()
        },
    ));
    assert!(snap.cast(0.0, 0.0, 1.0, 0.0, 1000.0).is_none(), "not until it is refreshed");
    snap.refresh(&world);
    assert!(snap.cast(0.0, 0.0, 1.0, 0.0, 1000.0).is_some(), "and then it is there");
}

#[test]
fn the_snapshot_honours_a_colliders_offset() {
    // A collider's offset is in the body's frame, so it turns with the
    // body. Two sides composing that differently is invisible until
    // something rotates, which is why this is its own test.
    let mut world = World::new();
    world.spawn((
        Transform {
            position: glam::Vec2::new(0.0, 0.0),
            rotation: std::f32::consts::FRAC_PI_2,
            scale: glam::Vec2::ONE,
        },
        Collider2D {
            shape: ColliderShape::Box { width: 40.0, height: 40.0 },
            // 300 along the body's x, which after a quarter turn is 300
            // along the world's y.
            offset: [300.0, 0.0],
            ..Default::default()
        },
    ));
    let mut snap = RaySnapshot::new();
    snap.refresh(&world);
    assert!(snap.cast(0.0, 0.0, 1.0, 0.0, 1000.0).is_none(), "it is not along x any more");
    let up = snap.cast(0.0, 0.0, 0.0, 1.0, 1000.0).expect("it should be above the origin");
    assert!((up.distance - 280.0).abs() < 2.0, "distance {}", up.distance);
}

#[test]
fn a_sensor_is_still_something_a_ray_can_see() {
    // Deliberate, and matching what native answers: rapier's
    // `QueryFilter::default()` includes sensors, and a script asking what
    // is along a line wants to know about a trigger volume.
    let mut world = World::new();
    world.spawn((
        Transform { position: glam::Vec2::new(300.0, 0.0), ..Default::default() },
        Collider2D {
            shape: ColliderShape::Box { width: 40.0, height: 400.0 },
            is_sensor: true,
            ..Default::default()
        },
    ));
    let mut snap = RaySnapshot::new();
    snap.refresh(&world);
    assert!(snap.cast(0.0, 0.0, 1.0, 0.0, 1000.0).is_some());
}
