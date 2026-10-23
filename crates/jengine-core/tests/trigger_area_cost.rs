//! `update_trigger_areas` should cost nothing when there is nothing to test.
//!
//! It builds a candidate list over **every** entity with a Transform — a
//! `Tags` lookup and a `HashSet<String>` clone apiece — and only then loops
//! over the triggers. A scene with no triggers, or with triggers that filter
//! on nothing, was paying all of that every frame for a loop that ran zero
//! times or never read the tags.
//!
//! Found while looking for what else scaled with entity count after the
//! script-mirror rewrite (BUGS.md #30).

use hecs::World;
use jengine_core::ecs::behaviors::update_trigger_areas;
use jengine_core::ecs::components::{
    Tags, Transform, TriggerArea2D, TriggerOverlaps, TriggerShape,
};

fn world_with(entities: usize, tagged: bool) -> World {
    let mut world = World::new();
    for i in 0..entities {
        let mut t = Transform::default();
        t.position.x = (i % 100) as f32 * 8.0;
        t.position.y = (i / 100) as f32 * 8.0;
        if tagged {
            world.spawn((t, Tags(["crowd".to_string()].into_iter().collect())));
        } else {
            world.spawn((t,));
        }
    }
    world
}

fn add_trigger(world: &mut World, match_tags: Vec<String>) -> hecs::Entity {
    world.spawn((
        Transform::default(),
        TriggerArea2D {
            shape: TriggerShape::Circle { radius: 32.0 },
            match_tags,
            ..Default::default()
        },
    ))
}

#[test]
fn a_world_with_no_triggers_is_left_untouched() {
    // The early-out must be exactly equivalent to running the old loop: with
    // no triggers the update list was empty, so nothing was ever written.
    let mut world = world_with(50, true);
    update_trigger_areas(&mut world);
    assert_eq!(
        world.query::<&TriggerOverlaps>().iter().count(),
        0,
        "something was written despite there being no triggers"
    );
}

#[test]
fn a_trigger_still_detects_overlap() {
    // The optimisation must not break the thing itself.
    let mut world = World::new();
    let inside = world.spawn((Transform::default(),));
    let mut far = Transform::default();
    far.position.x = 5_000.0;
    world.spawn((far,));
    let trigger = add_trigger(&mut world, vec![]);

    update_trigger_areas(&mut world);

    let overlaps = world.get::<&TriggerOverlaps>(trigger).expect("trigger got overlaps");
    assert_eq!(overlaps.staying.len(), 1, "expected exactly the near entity");
    assert_eq!(overlaps.staying[0], inside.to_bits().get());
}

#[test]
fn tag_filtering_still_works() {
    // `needs_tags` decides whether the candidate pass clones tag sets at all,
    // so a trigger that filters on tags must still see them.
    let mut world = World::new();
    let tagged = world.spawn((
        Transform::default(),
        Tags(["pickup".to_string()].into_iter().collect()),
    ));
    world.spawn((Transform::default(),)); // untagged, same spot
    let trigger = add_trigger(&mut world, vec!["pickup".to_string()]);

    update_trigger_areas(&mut world);

    let overlaps = world.get::<&TriggerOverlaps>(trigger).unwrap();
    assert_eq!(
        overlaps.staying,
        vec![tagged.to_bits().get()],
        "tag filtering selected the wrong set"
    );
}

fn trigger_at(world: &mut World, x: f32, y: f32, radius: f32) -> hecs::Entity {
    let mut t = Transform::default();
    t.position = glam::Vec2::new(x, y);
    world.spawn((t, TriggerArea2D { shape: TriggerShape::Circle { radius }, ..Default::default() }))
}

fn thing_at(world: &mut World, x: f32, y: f32) -> hecs::Entity {
    let mut t = Transform::default();
    t.position = glam::Vec2::new(x, y);
    world.spawn((t,))
}

fn staying(world: &World, trigger: hecs::Entity) -> Vec<u64> {
    world.get::<&TriggerOverlaps>(trigger).map(|o| o.staying.clone()).unwrap_or_default()
}

/// Only entities inside some trigger's box become candidates (Oct 2026), so
/// what each trigger sees must be what it saw when every entity was one:
/// two triggers far apart, a thing between them in neither, and a thing
/// exactly on a circle's edge, which counts as inside.
#[test]
fn triggers_far_apart_each_see_their_own() {
    let mut world = World::new();
    let west = trigger_at(&mut world, -10_000.0, 0.0, 50.0);
    let east = trigger_at(&mut world, 10_000.0, 0.0, 50.0);
    let near_west = thing_at(&mut world, -10_000.0, 20.0);
    let on_edge = thing_at(&mut world, 10_050.0, 0.0);
    thing_at(&mut world, 0.0, 0.0);
    thing_at(&mut world, -10_000.0, 51.0);
    update_trigger_areas(&mut world);
    assert_eq!(staying(&world, west), vec![near_west.to_bits().get()]);
    assert_eq!(staying(&world, east), vec![on_edge.to_bits().get()], "an entity on the circle's edge is inside");
}

/// Past sixteen triggers the prefilter tests the union of their boxes, not
/// each - the answers must not change.
#[test]
fn many_triggers_each_see_their_own() {
    let mut world = World::new();
    let mut pairs = Vec::new();
    for i in 0..24 {
        let x = i as f32 * 1000.0;
        let trigger = trigger_at(&mut world, x, 0.0, 10.0);
        let inside = thing_at(&mut world, x + 5.0, 0.0);
        thing_at(&mut world, x + 500.0, 0.0);
        pairs.push((trigger, inside));
    }
    update_trigger_areas(&mut world);
    for (trigger, inside) in pairs {
        assert_eq!(staying(&world, trigger), vec![inside.to_bits().get()]);
    }
}

#[test]
#[ignore = "timing-sensitive; run with --release"]
fn no_triggers_costs_nothing_at_scale() {
    use std::time::Instant;

    // 8,000 tagged entities and no trigger: the old code built 8,000
    // candidates and cloned 8,000 tag sets per call.
    let mut world = world_with(8_000, true);
    let t0 = Instant::now();
    for _ in 0..100 {
        update_trigger_areas(&mut world);
    }
    let per_call_us = t0.elapsed().as_secs_f64() * 1000.0 * 1000.0 / 100.0;
    println!("no triggers, 8k entities: {per_call_us:.1} us/call");

    // What the candidate pass costs when it does run - i.e. what every scene
    // with no triggers used to pay anyway. Tag cloning is the expensive part,
    // so measure with and without a tag-filtering trigger.
    let mut with_trigger = world_with(8_000, true);
    add_trigger(&mut with_trigger, vec![]);
    let t1 = Instant::now();
    for _ in 0..20 {
        update_trigger_areas(&mut with_trigger);
    }
    println!(
        "one trigger, no tag filter: {:.1} us/call",
        t1.elapsed().as_secs_f64() * 1e6 / 20.0
    );

    let mut tag_filtered = world_with(8_000, true);
    add_trigger(&mut tag_filtered, vec!["crowd".to_string()]);
    let t2 = Instant::now();
    for _ in 0..20 {
        update_trigger_areas(&mut tag_filtered);
    }
    println!(
        "one trigger, tag filter: {:.1} us/call",
        t2.elapsed().as_secs_f64() * 1e6 / 20.0
    );

    // How much of the cost is the per-trigger scan versus the one-off
    // candidate build? If more triggers barely move the number, the build
    // dominates and a broadphase would not help.
    for triggers in [1usize, 10, 50] {
        let mut w = world_with(8_000, false);
        for _ in 0..triggers {
            add_trigger(&mut w, vec![]);
        }
        let t = Instant::now();
        for _ in 0..10 {
            update_trigger_areas(&mut w);
        }
        println!(
            "{triggers:>3} triggers, 8k entities: {:.1} us/call",
            t.elapsed().as_secs_f64() * 1e6 / 10.0
        );
    }
    assert!(
        per_call_us < 50.0,
        "{per_call_us:.1} us/call with nothing to do - the early-out is not working"
    );
}
