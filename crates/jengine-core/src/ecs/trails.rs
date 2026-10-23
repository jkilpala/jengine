//! Trails: the points a [`Trail`] records as its entity moves, and the
//! ribbon drawn through them.
//!
//! Recording is [`tick_trails`], which every host calls right after it
//! resolves the hierarchy, so it reads the `WorldTransform` the frame ends
//! with - a trail on a child records where the child really is, and what
//! ran before it (scripts, motion, physics, in whatever order that host
//! runs them) is already in it. It used to run inside the motion step and
//! read the *previous* frame's resolve, which on native - where scripts run
//! before motion - recorded a swing's first point where the blade had been
//! resting (`tests/trails_reach_every_runtime.rs`). The ribbon's head is the
//! entity's position when it is drawn ([`ribbon`]).

use glam::{Vec2, Vec4};
use hecs::World;

use super::components::{Trail, TrailPoint, Transform, WorldTransform};

/// Age every trail's points by `dt`, drop the ones past their lifetime, and
/// record a new one wherever an emitting trail's entity has moved
/// `min_distance` from its newest point.
pub fn tick_trails(world: &mut World, dt: f32) {
    let _span = crate::profiling::trace::span("trails");
    for (_, (trail, t, w, own)) in
        world.query_mut::<(&mut Trail, &Transform, Option<&WorldTransform>, Option<&crate::ecs::time_scale::TimeScale>)>()
    {
        let position = w.map_or(t.position, |w| w.position);
        record(trail, position, dt * crate::ecs::time_scale::factor(own));
    }
}

/// One entity's step of [`tick_trails`].
pub fn record(trail: &mut Trail, position: Vec2, dt: f32) {
    let lifetime = trail.lifetime.max(0.0);
    for p in &mut trail.points {
        p.age += dt;
    }
    trail.points.retain(|p| p.age < lifetime);
    if !trail.emitting {
        return;
    }
    let moved = trail
        .points
        .last()
        .is_none_or(|last| last.position.distance(position) >= trail.min_distance.max(0.0));
    if moved {
        trail.points.push(TrailPoint { position, age: 0.0 });
        let excess = trail.points.len().saturating_sub(Trail::MAX_POINTS);
        trail.points.drain(..excess);
    }
}

/// A trail's ribbon for one frame, in world space: two vertices per point,
/// one quad per segment.
#[derive(Debug, Clone, Default)]
pub struct Ribbon {
    pub positions: Vec<Vec2>,
    pub uvq: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// Points closer than this are one point: a ribbon cannot turn across a
/// segment of no length.
const SAME_POINT: f32 = 1e-3;

/// The ribbon from `head` (the entity's position now) back through the
/// recorded points. `None` when there is nothing to draw: no point behind
/// the head, or a trail that is invisible anyway.
///
/// **A stopped trail has no head**: once `emitting` is off the ribbon is the
/// recorded points alone, fading where they were left. It used to reach
/// from them to wherever the entity went next - a pooled knife parked off
/// screen as its trail stopped drew a streak to the parking spot for the
/// trail's whole lifetime (Shooter Classics' Crossfire waited two frames to
/// dodge it).
pub fn ribbon(trail: &Trail, head: Vec2) -> Option<Ribbon> {
    if trail.color[3] <= 0.0 && trail.color_end[3] <= 0.0 {
        return None;
    }
    let lifetime = trail.lifetime.max(1e-6);
    // Head first, then newest to oldest.
    let mut nodes: Vec<(Vec2, f32)> = Vec::with_capacity(trail.points.len() + 1);
    if trail.emitting {
        nodes.push((head, 0.0));
    }
    for p in trail.points.iter().rev() {
        if nodes.last().is_some_and(|(q, _)| q.distance(p.position) < SAME_POINT) {
            continue;
        }
        nodes.push((p.position, p.age));
    }
    if nodes.len() < 2 {
        return None;
    }
    let nodes = smooth(&nodes);

    let mut along = Vec::with_capacity(nodes.len());
    let mut length = 0.0;
    for (i, (p, _)) in nodes.iter().enumerate() {
        if i > 0 {
            length += p.distance(nodes[i - 1].0);
        }
        along.push(length);
    }

    let n = nodes.len();
    let mut out = Ribbon {
        positions: Vec::with_capacity(n * 2),
        uvq: Vec::with_capacity(n * 2),
        colors: Vec::with_capacity(n * 2),
        indices: Vec::with_capacity((n - 1) * 6),
    };
    let (c0, c1) = (Vec4::from(trail.color), Vec4::from(trail.color_end));
    for i in 0..n {
        let (p, age) = nodes[i];
        // The direction through a point is its neighbours' - the average of
        // the two segments it joins - so the ribbon bends at it rather than
        // kinking. A hairpin cancels that out; then the segment ahead does.
        let before = nodes[i.saturating_sub(1)].0;
        let after = nodes[(i + 1).min(n - 1)].0;
        let dir = (after - before)
            .try_normalize()
            .or_else(|| (nodes[(i + 1).min(n - 1)].0 - p).try_normalize())
            .or_else(|| (p - nodes[i.saturating_sub(1)].0).try_normalize())
            .unwrap_or(Vec2::X);
        let normal = dir.perp();
        let f = (age / lifetime).clamp(0.0, 1.0);
        let half = (trail.width + (trail.width_end - trail.width) * f).max(0.0) * 0.5;
        let color: [f32; 4] = c0.lerp(c1, f).into();
        let u = if length > 0.0 { along[i] / length } else { 0.0 };
        out.positions.push(p + normal * half);
        out.positions.push(p - normal * half);
        out.uvq.push([u, 0.0, 1.0]);
        out.uvq.push([u, 1.0, 1.0]);
        out.colors.push(color);
        out.colors.push(color);
    }
    for i in 0..(n as u32 - 1) {
        // `a b c, a c d` - the batch draws the pair as one quad.
        let (a, d) = (2 * i, 2 * i + 1);
        let (b, c) = (2 * i + 2, 2 * i + 3);
        out.indices.extend_from_slice(&[a, b, c, a, c, d]);
    }
    Some(out)
}

/// Segments longer than this are cut into pieces along a curve.
const SMOOTH_STEP: f32 = 6.0;
/// At most this many pieces per segment.
const MAX_PIECES: usize = 16;

/// The nodes with every long segment replaced by a curve: a cubic leaving
/// each end along that node's [`tangent`], sized so that when the two ends
/// agree with one circle, the curve is (very nearly) that circle's arc. Ages
/// are interpolated along with positions.
///
/// Without it a fast arc is a polygon: a sword swept through half a turn in
/// ten frames records ten points, and a ribbon of ten straight chords reads
/// as a fan of flat blades rather than one curved slash. The first version
/// was a centripetal Catmull-Rom spline, which is right for evenly spaced
/// points and wrong for what a real display delivers: frames arrive in
/// bursts (one of 18 ms, three of 2 ms, at a steady 165 Hz), so a swing
/// records clusters of points a few units apart with 100-unit gaps between
/// them - and Catmull-Rom takes its end tangents from those tiny neighbour
/// segments' *lengths*, so every gap came out a straight line with a kink
/// at each cluster. The showcase's Trails met it.
fn smooth(nodes: &[(Vec2, f32)]) -> Vec<(Vec2, f32)> {
    let n = nodes.len();
    let tangents: Vec<Vec2> = (0..n).map(|i| tangent(nodes, i)).collect();
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n - 1 {
        let (a, age_a) = nodes[i];
        let (b, age_b) = nodes[i + 1];
        out.push(nodes[i]);
        let length = a.distance(b);
        if length <= SMOOTH_STEP {
            continue;
        }
        let (ta, tb) = (tangents[i], tangents[i + 1]);
        // An arc turning through `theta` is matched by tangents of length
        // `chord * (theta/2) / sin(theta/2)`; 1 for a straight line.
        let theta = ta.dot(tb).clamp(-1.0, 1.0).acos().min(2.5);
        let k = if theta < 1e-3 { 1.0 } else { (theta * 0.5) / (theta * 0.5).sin() };
        let (ma, mb) = (ta * length * k, tb * length * k);
        let pieces = ((length / SMOOTH_STEP).ceil() as usize).min(MAX_PIECES);
        for j in 1..pieces {
            let s = j as f32 / pieces as f32;
            let (s2, s3) = (s * s, s * s * s);
            let p = a * (2.0 * s3 - 3.0 * s2 + 1.0)
                + ma * (s3 - 2.0 * s2 + s)
                + b * (-2.0 * s3 + 3.0 * s2)
                + mb * (s3 - s2);
            out.push((p, age_a + (age_b - age_a) * s));
        }
    }
    out.push(nodes[n - 1]);
    out
}

/// The direction the path runs through node `i`: the tangent of the circle
/// through it and its two neighbours, which is exact for points on an arc
/// however unevenly they are spaced - the two chords' directions, each
/// weighted by the *other* chord's length. An end runs along its segment.
fn tangent(nodes: &[(Vec2, f32)], i: usize) -> Vec2 {
    let n = nodes.len();
    let p = nodes[i].0;
    let ahead = (i + 1 < n).then(|| nodes[i + 1].0 - p);
    let behind = (i > 0).then(|| p - nodes[i - 1].0);
    let t = match (behind, ahead) {
        (Some(b), Some(a)) => a.normalize_or_zero() * b.length() + b.normalize_or_zero() * a.length(),
        (Some(b), None) => b,
        (None, Some(a)) => a,
        (None, None) => Vec2::ZERO,
    };
    t.try_normalize().or_else(|| ahead.and_then(|a| a.try_normalize())).unwrap_or(Vec2::X)
}

impl Ribbon {
    /// A disc holding every vertex: centre and radius.
    pub fn bounds(&self) -> (Vec2, f32) {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in &self.positions {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        ((lo + hi) * 0.5, (hi - lo).length() * 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trail() -> Trail {
        Trail { lifetime: 1.0, width: 10.0, width_end: 0.0, min_distance: 4.0, ..Trail::default() }
    }

    #[test]
    fn a_point_is_recorded_only_after_moving_min_distance() {
        let mut t = trail();
        record(&mut t, Vec2::ZERO, 0.1);
        assert_eq!(t.points.len(), 1);
        record(&mut t, Vec2::new(3.0, 0.0), 0.1);
        assert_eq!(t.points.len(), 1, "3 is short of min_distance 4");
        record(&mut t, Vec2::new(4.0, 0.0), 0.1);
        assert_eq!(t.points.len(), 2);
    }

    #[test]
    fn points_age_out_after_their_lifetime() {
        let mut t = trail();
        record(&mut t, Vec2::ZERO, 0.0);
        record(&mut t, Vec2::new(10.0, 0.0), 0.6);
        assert_eq!(t.points.len(), 2);
        record(&mut t, Vec2::new(10.0, 0.0), 0.5);
        assert_eq!(t.points.len(), 1, "the first point is 1.1 s old, past a 1 s lifetime");
        assert!((t.points[0].age - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_trail_that_is_not_emitting_records_nothing_and_empties() {
        let mut t = trail();
        record(&mut t, Vec2::ZERO, 0.0);
        t.emitting = false;
        record(&mut t, Vec2::new(50.0, 0.0), 0.5);
        assert_eq!(t.points.len(), 1);
        record(&mut t, Vec2::new(90.0, 0.0), 0.6);
        assert!(t.points.is_empty());
    }

    #[test]
    fn a_trail_keeps_at_most_max_points() {
        let mut t = Trail { lifetime: 1000.0, ..trail() };
        for i in 0..(Trail::MAX_POINTS + 40) {
            record(&mut t, Vec2::new(i as f32 * 10.0, 0.0), 0.01);
        }
        assert_eq!(t.points.len(), Trail::MAX_POINTS);
        assert_eq!(t.points.last().unwrap().position.x, (Trail::MAX_POINTS + 39) as f32 * 10.0);
    }

    #[test]
    fn the_ribbon_narrows_and_fades_with_age() {
        // Segments shorter than the smoothing step, so every node is a point.
        let mut t = trail();
        t.points = vec![
            TrailPoint { position: Vec2::new(-10.0, 0.0), age: 1.0 },
            TrailPoint { position: Vec2::new(-5.0, 0.0), age: 0.5 },
        ];
        let r = ribbon(&t, Vec2::ZERO).unwrap();
        assert_eq!(r.positions.len(), 6);
        assert_eq!(r.indices.len(), 12);
        // Head: full width, full colour, u 0.
        assert!((r.positions[0].distance(r.positions[1]) - 10.0).abs() < 1e-4);
        assert_eq!(r.colors[0][3], 1.0);
        assert_eq!(r.uvq[0][0], 0.0);
        // Middle: half of each, u half way along.
        assert!((r.positions[2].distance(r.positions[3]) - 5.0).abs() < 1e-4);
        assert!((r.colors[2][3] - 0.5).abs() < 1e-6);
        assert!((r.uvq[2][0] - 0.5).abs() < 1e-6);
        // Tail: nothing left of it.
        assert!(r.positions[4].distance(r.positions[5]) < 1e-4);
        assert_eq!(r.colors[4][3], 0.0);
        assert_eq!(r.uvq[4][0], 1.0);
    }

    #[test]
    fn a_fast_arc_is_drawn_as_a_curve() {
        // A quarter turn a frame on a radius of 50: four points, and chords
        // between them would cut 15 units inside the circle.
        let mut t = trail();
        t.width = 0.0;
        t.width_end = 0.0;
        t.points = [3.0f32, 2.0, 1.0]
            .iter()
            .map(|&k| {
                let a = k * std::f32::consts::FRAC_PI_2;
                TrailPoint { position: Vec2::new(a.cos(), a.sin()) * 50.0, age: k * 0.1 }
            })
            .collect();
        let r = ribbon(&t, Vec2::new(50.0, 0.0)).unwrap();
        assert!(r.positions.len() > 20, "long segments are cut up: {}", r.positions.len());
        // The segment with a real neighbour at both ends (90 to 180 degrees);
        // the end segments lean on reflected neighbours and bow less. A
        // spline through a quarter turn's points is not a circle - its
        // middle sits at 0.88 of the radius - but a chord's sits at 0.71.
        let mut checked = 0;
        for p in r.positions.iter().filter(|p| p.x < -1.0 && p.y > 1.0) {
            assert!(p.length() > 43.5 && p.length() < 51.0, "{p:?} is off the arc");
            checked += 1;
        }
        assert!(checked > 10, "{checked}");
    }

    #[test]
    fn an_arc_sampled_in_bursts_stays_an_arc() {
        // What a display delivering frames in bursts records of a swing:
        // clusters of points 2 degrees apart, 60 degrees between clusters,
        // radius 100. Catmull-Rom drew each gap as a straight chord, 13.4
        // units inside the circle at its middle.
        let mut angles = Vec::new();
        for c in 0..4 {
            for k in 0..3 {
                angles.push((c as f32 * 60.0 + k as f32 * 2.0).to_radians());
            }
        }
        let mut t = trail();
        t.width = 0.0;
        t.width_end = 0.0;
        // Oldest first; the head is the last angle.
        let head_angle = angles.pop().unwrap();
        t.points = angles
            .iter()
            .enumerate()
            .map(|(i, a)| TrailPoint { position: Vec2::new(a.cos(), a.sin()) * 100.0, age: 0.5 - i as f32 * 0.01 })
            .collect();
        let r = ribbon(&t, Vec2::new(head_angle.cos(), head_angle.sin()) * 100.0).unwrap();
        let worst = r.positions.iter().map(|p| (p.length() - 100.0).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1.0, "a point of the ribbon is {worst} off the circle");
        assert!(r.positions.len() > 40, "{}", r.positions.len());
    }

    #[test]
    fn the_ribbon_is_across_its_path() {
        // A vertical trail is widened along x.
        let mut t = trail();
        t.points = vec![TrailPoint { position: Vec2::new(0.0, -40.0), age: 0.2 }];
        let r = ribbon(&t, Vec2::ZERO).unwrap();
        let across = r.positions[0] - r.positions[1];
        assert!(across.y.abs() < 1e-4 && (across.x.abs() - 10.0).abs() < 1e-4, "{across:?}");
    }

    /// A stopped trail is drawn from its last point, wherever its entity is
    /// now - parking a pooled sprite draws no streak to the parking spot.
    #[test]
    fn a_stopped_trail_does_not_follow_its_entity() {
        let mut t = trail();
        t.points = vec![
            TrailPoint { position: Vec2::new(0.0, 0.0), age: 0.2 },
            TrailPoint { position: Vec2::new(40.0, 0.0), age: 0.1 },
        ];
        let parked = Vec2::new(-9000.0, 0.0);
        let live = ribbon(&t, parked).unwrap();
        assert!(live.positions.iter().any(|p| p.x < -1000.0), "emitting, the head is the entity");
        t.emitting = false;
        let stopped = ribbon(&t, parked).unwrap();
        assert!(stopped.positions.iter().all(|p| p.x > -100.0), "stopped, nothing reaches the parking spot");
        t.points.truncate(1);
        assert!(ribbon(&t, parked).is_none(), "one point left is no segment");
    }

    #[test]
    fn nothing_is_drawn_without_a_point_behind_the_head() {
        let mut t = trail();
        assert!(ribbon(&t, Vec2::ZERO).is_none());
        t.points = vec![TrailPoint { position: Vec2::ZERO, age: 0.1 }];
        assert!(ribbon(&t, Vec2::ZERO).is_none(), "a point under the head is no segment");
    }
}

/// `Trail.set(e, field, value)`: write any `trail` field by name - a colour
/// a pooled ribbon takes from the team it is flying for (Paint Party's blobs),
/// a width, `lifetime`. The points it has recorded are kept (they are not
/// serialised), so the ribbon changes where it is rather than starting over.
pub fn set_trail_field(world: &World, entity: hecs::Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let Ok(mut c) = world.get::<&mut Trail>(entity) else {
        return Err("no trail on that entity".into());
    };
    let mut whole = serde_json::to_value(&*c).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    // Fields left out when empty are fields all the same.
    if !(map.contains_key(field) || field == "sort_layer") {
        return Err(format!("'{field}' is not a trail field"));
    }
    let value = match (field, value) {
        ("z_order", serde_json::Value::Number(n)) => serde_json::Value::from(n.as_f64().unwrap_or(0.0) as i64),
        _ => value.clone(),
    };
    map.insert(field.to_string(), value);
    let mut next: Trail = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    next.points = std::mem::take(&mut c.points);
    *c = next;
    Ok(())
}

#[cfg(test)]
mod set_field_tests {
    use super::*;

    #[test]
    fn a_trail_is_recoloured_by_name_and_keeps_its_points() {
        let mut world = World::new();
        let mut trail = Trail::default();
        trail.points.push(TrailPoint { position: Vec2::new(3.0, 4.0), age: 0.1 });
        let e = world.spawn((trail,));
        set_trail_field(&world, e, "color", &serde_json::json!([1.0, 0.2, 0.6, 1.0])).unwrap();
        set_trail_field(&world, e, "z_order", &serde_json::json!(7.0)).unwrap();
        let t = world.get::<&Trail>(e).unwrap();
        assert_eq!(t.color, [1.0, 0.2, 0.6, 1.0]);
        assert_eq!(t.z_order, 7);
        assert_eq!(t.points.len(), 1, "a recolour keeps the ribbon");
        drop(t);
        assert!(set_trail_field(&world, e, "colour", &serde_json::json!(1)).is_err());
    }
}
