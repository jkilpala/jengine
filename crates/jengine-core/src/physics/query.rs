//! **Shape queries** (Oct 2026): what overlaps a circle, a box or a point,
//! and where a circle swept along a line first touches something - the
//! questions an explosion's radius, a melee swing, a "can I stand here" check
//! and a thick bullet ask. Before them a script had `raycast`, which is a line
//! with no width, and `find_in_radius`, which reads transforms and never
//! colliders, so a long wall whose middle was in a blast's radius was missed
//! because its centre was not.
//!
//! One implementation over a list of shapes: [`super::PhysicsWorld`] hands it
//! the live collider set (native), [`super::ray::RaySnapshot`] the shapes it
//! already keeps for `raycast` (the browser hosts) - so every runtime answers
//! with the same arithmetic, sensors included as `raycast` includes them.

use rapier2d::na::{Point2, Vector2};
use rapier2d::parry::query::{self, ShapeCastOptions};
use rapier2d::prelude::{Ball, Cuboid, Isometry, Shape};

use super::RayHit;

/// The shape a script asks about, in world units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QueryShape {
    Point { x: f32, y: f32 },
    Circle { x: f32, y: f32, radius: f32 },
    /// `width` x `height` about its centre, turned by `rotation` radians.
    Box { x: f32, y: f32, width: f32, height: f32, rotation: f32 },
}

/// Every entity one of whose shapes overlaps `q` - touching counts - each
/// once, in ascending order of id (a stable order for a script to rely on).
pub fn overlaps<'a>(shapes: impl Iterator<Item = (u64, &'a dyn Shape, &'a Isometry<f32>)>, q: &QueryShape) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    match *q {
        QueryShape::Point { x, y } => {
            let p = Point2::new(x, y);
            for (id, shape, iso) in shapes {
                if shape.contains_point(iso, &p) {
                    out.push(id);
                }
            }
        }
        QueryShape::Circle { x, y, radius } => {
            let ball = Ball::new(radius.max(0.0));
            let at = Isometry::translation(x, y);
            for (id, shape, iso) in shapes {
                if query::intersection_test(&at, &ball, iso, shape).unwrap_or(false) {
                    out.push(id);
                }
            }
        }
        QueryShape::Box { x, y, width, height, rotation } => {
            let cuboid = Cuboid::new(Vector2::new(width.abs() * 0.5, height.abs() * 0.5));
            let at = Isometry::new(Vector2::new(x, y), rotation);
            for (id, shape, iso) in shapes {
                if query::intersection_test(&at, &cuboid, iso, shape).unwrap_or(false) {
                    out.push(id);
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Sweep a circle of `radius` from `(ox, oy)` along `(dx, dy)` up to
/// `max_dist` and report the first thing it touches - `raycast`'s contract
/// with a width: the direction is normalised, `distance` is how far the
/// circle's centre travelled, `point` and `normal` are where and how the
/// thing it met faces it. A circle that starts already touching something
/// hits it at distance 0.
pub fn circle_cast<'a>(
    shapes: impl Iterator<Item = (u64, &'a dyn Shape, &'a Isometry<f32>)>,
    ox: f32,
    oy: f32,
    dx: f32,
    dy: f32,
    radius: f32,
    max_dist: f32,
) -> Option<RayHit> {
    let dir = Vector2::new(dx, dy);
    let len = dir.norm();
    if len <= f32::EPSILON || !len.is_finite() || max_dist < 0.0 {
        return None;
    }
    let vel = dir / len;
    let ball = Ball::new(radius.max(0.0));
    let at = Isometry::translation(ox, oy);
    let still = Vector2::zeros();
    let options = ShapeCastOptions { compute_impact_geometry_on_penetration: true, ..ShapeCastOptions::with_max_time_of_impact(max_dist) };
    let mut best: Option<RayHit> = None;
    for (id, shape, iso) in shapes {
        let Ok(Some(hit)) = query::cast_shapes(&at, &vel, &ball, iso, &still, shape, options) else { continue };
        if best.as_ref().is_some_and(|b| b.distance <= hit.time_of_impact) {
            continue;
        }
        let point = iso * hit.witness2;
        let normal = iso * hit.normal2;
        best = Some(RayHit {
            entity_id: id,
            point_x: point.x,
            point_y: point.y,
            normal_x: normal.x,
            normal_y: normal.y,
            distance: hit.time_of_impact,
        });
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapier2d::prelude::SharedShape;

    fn world() -> Vec<(u64, SharedShape, Isometry<f32>)> {
        vec![
            // A long wall along y = 0, from x -200 to 200.
            (1, SharedShape::cuboid(200.0, 10.0), Isometry::translation(0.0, 0.0)),
            // A ball up at (0, 100).
            (2, SharedShape::ball(20.0), Isometry::translation(0.0, 100.0)),
            // A second collider of entity 1, off to the right.
            (1, SharedShape::cuboid(10.0, 10.0), Isometry::translation(400.0, 0.0)),
        ]
    }

    fn iter(w: &[(u64, SharedShape, Isometry<f32>)]) -> impl Iterator<Item = (u64, &dyn Shape, &Isometry<f32>)> {
        w.iter().map(|(id, s, iso)| (*id, &**s, iso))
    }

    #[test]
    fn a_circle_finds_a_wall_whose_middle_is_far_away() {
        let w = world();
        // At the wall's end, its centre 180 away: still found.
        assert_eq!(overlaps(iter(&w), &QueryShape::Circle { x: 180.0, y: 20.0, radius: 15.0 }), vec![1]);
        // Both, each once, though entity 1 has two colliders in reach.
        assert_eq!(overlaps(iter(&w), &QueryShape::Circle { x: 0.0, y: 50.0, radius: 60.0 }), vec![1, 2]);
        assert!(overlaps(iter(&w), &QueryShape::Circle { x: 0.0, y: 50.0, radius: 15.0 }).is_empty());
    }

    #[test]
    fn a_box_turns_and_a_point_is_inside_or_not() {
        let w = world();
        // A thin box between the wall and the ball reaches neither...
        let thin = QueryShape::Box { x: 0.0, y: 45.0, width: 4.0, height: 60.0, rotation: 0.0 };
        assert!(overlaps(iter(&w), &thin).is_empty());
        // ...turned flat it is still between them; long and upright, both.
        let tall = QueryShape::Box { x: 0.0, y: 45.0, width: 4.0, height: 120.0, rotation: 0.0 };
        assert_eq!(overlaps(iter(&w), &tall), vec![1, 2]);
        let flat = QueryShape::Box { x: 0.0, y: 45.0, width: 4.0, height: 120.0, rotation: std::f32::consts::FRAC_PI_2 };
        assert!(overlaps(iter(&w), &flat).is_empty());
        assert_eq!(overlaps(iter(&w), &QueryShape::Point { x: 5.0, y: 105.0 }), vec![2]);
        assert!(overlaps(iter(&w), &QueryShape::Point { x: 0.0, y: 60.0 }).is_empty());
    }

    #[test]
    fn a_swept_circle_stops_a_radius_short_of_what_a_ray_would_hit() {
        let w = world();
        // Down from (0, 300): the ball's top is at 120; a circle of 10 touches
        // it when its centre is at 130 - 170 travelled.
        let hit = circle_cast(iter(&w), 0.0, 300.0, 0.0, -5.0, 10.0, 1000.0).expect("hits the ball");
        assert_eq!(hit.entity_id, 2);
        assert!((hit.distance - 170.0).abs() < 0.1, "{hit:?}");
        assert!((hit.point_y - 120.0).abs() < 0.1 && hit.normal_y > 0.99, "touches the ball's top, facing up: {hit:?}");
        // Off to the side of the ball by 25: a ray misses it, a circle of 10 does not.
        let side = circle_cast(iter(&w), 25.0, 300.0, 0.0, -1.0, 10.0, 1000.0).expect("grazes the ball");
        assert_eq!(side.entity_id, 2);
        let thin = circle_cast(iter(&w), 25.0, 300.0, 0.0, -1.0, 0.0, 1000.0).expect("reaches the wall");
        assert_eq!(thin.entity_id, 1);
        // Not far enough: nothing.
        assert!(circle_cast(iter(&w), 0.0, 300.0, 0.0, -1.0, 10.0, 100.0).is_none());
    }
}
