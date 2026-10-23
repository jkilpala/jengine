//! **Behaviour trees** (`behavior_tree`, Oct 2026; `ecs::behavior_tree`):
//! a creature asks every frame what it should be doing now, and the answer
//! is a tree of sequences, selectors and leaves ticked inside `tick_motion`,
//! with the entity's `Blackboard` custom component as its memory. These run
//! the trees through `tick_motion` itself, so what is checked is what every
//! host runs. Found by the showcase's Night Watch.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::behavior_tree::{BehaviorTree, blackboard, set_behavior_tree_field, set_blackboard};
use jengine_core::ecs::behaviors::tick_motion;
use jengine_core::ecs::callbacks;
use jengine_core::ecs::components::{Name, Tags, Transform, Velocity2D};
use serde_json::json;

fn tree(root: serde_json::Value) -> BehaviorTree {
    serde_json::from_value(json!({ "root": root })).expect("the tree parses")
}

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

fn run(world: &mut World, frames: usize) {
    for _ in 0..frames {
        tick_motion(world, 1.0 / 60.0, None, false);
    }
}

fn pos(world: &World, e: hecs::Entity) -> Vec2 {
    world.get::<&Transform>(e).unwrap().position
}

fn running(world: &World, e: hecs::Entity) -> String {
    blackboard(world, e, "_running").and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

/// A guard: chase the thief while alerted (reactive, so the alert cuts the
/// patrol short), else patrol between two posts.
fn guard() -> serde_json::Value {
    json!({ "type": "selector", "reactive": true, "children": [
        { "type": "sequence", "children": [
            { "type": "check", "key": "alert", "op": "set" },
            { "type": "move_to", "label": "chase", "target": "Thief", "speed": 300, "arrive": 20 }
        ]},
        { "type": "sequence", "label": "patrol", "children": [
            { "type": "move_to", "label": "to post B", "target": [400, 0], "speed": 120, "arrive": 4 },
            { "type": "wait", "label": "look around", "seconds": 0.5 },
            { "type": "move_to", "label": "to post A", "target": [0, 0], "speed": 120, "arrive": 4 }
        ]}
    ]})
}

#[test]
fn a_reactive_guard_drops_its_patrol_when_alerted() {
    let mut world = World::new();
    let g = world.spawn((at(0.0, 0.0), tree(guard())));
    world.spawn((at(0.0, 300.0), Name("Thief".into())));
    run(&mut world, 30);
    let p = pos(&world, g);
    assert!(p.x > 40.0 && p.y.abs() < 1e-3, "walking to post B: {p}");
    assert_eq!(running(&world, g), "to post B");
    set_blackboard(&mut world, g, "alert", json!(1));
    run(&mut world, 1);
    assert_eq!(running(&world, g), "chase", "the alert cut in the same frame");
    run(&mut world, 120);
    let p = pos(&world, g);
    assert!(p.distance(Vec2::new(0.0, 300.0)) <= 21.0, "caught up: {p}");
    // Alert over: the patrol starts again from its beginning.
    set_blackboard(&mut world, g, "alert", json!(0));
    run(&mut world, 2);
    assert_eq!(running(&world, g), "to post B");
}

#[test]
fn a_patrol_waits_at_its_post_and_comes_back() {
    let mut world = World::new();
    let g = world.spawn((at(0.0, 0.0), tree(guard())));
    world.spawn((at(0.0, 300.0), Name("Thief".into())));
    run(&mut world, 210); // 396 at 120/s is 3.3 s, then half a second looking
    assert_eq!(running(&world, g), "look around");
    assert!((pos(&world, g).x - 400.0).abs() <= 4.0);
    run(&mut world, 60);
    assert_eq!(running(&world, g), "to post A");
    assert!(pos(&world, g).x < 400.0);
}

#[test]
fn a_mover_with_velocity_is_steered_and_stopped_by_it() {
    let mut world = World::new();
    let e = world.spawn((at(0.0, 0.0), Velocity2D { x: 0.0, y: 0.0, ..Default::default() }, tree(json!(
        { "type": "move_to", "target": [100, 0], "speed": 200, "arrive": 2 }
    ))));
    run(&mut world, 2);
    assert!((world.get::<&Velocity2D>(e).unwrap().x - 200.0).abs() < 1e-3);
    run(&mut world, 60);
    assert!((pos(&world, e).x - 100.0).abs() <= 2.0, "{}", pos(&world, e));
    assert_eq!(world.get::<&Velocity2D>(e).unwrap().x, 0.0, "stopped on arrival");
}

#[test]
fn a_call_hands_work_to_the_script_and_waits_for_its_answer() {
    let mut world = World::new();
    let e = world.spawn((at(0.0, 0.0), tree(json!({ "type": "sequence", "children": [
        { "type": "call", "call": "dig" },
        { "type": "set", "key": "dug", "value": 1, "add": true }
    ]}))));
    callbacks::take();
    run(&mut world, 1);
    let owed = callbacks::take();
    assert_eq!(owed.len(), 1);
    assert_eq!((owed[0].entity, owed[0].handler), (e, "on_dig"));
    assert_eq!(blackboard(&world, e, "dig"), Some(json!("running")));
    run(&mut world, 5);
    assert!(callbacks::take().is_empty(), "called once, then waited");
    assert_eq!(running(&world, e), "dig");
    set_blackboard(&mut world, e, "dig", json!("success"));
    run(&mut world, 1);
    assert_eq!(blackboard(&world, e, "dug"), Some(json!(1.0)));
    // It loops: the next round calls the script again.
    run(&mut world, 1);
    assert_eq!(callbacks::take().len(), 1);
}

#[test]
fn an_aborted_call_is_told_so() {
    let mut world = World::new();
    let e = world.spawn((at(0.0, 0.0), tree(json!({ "type": "selector", "reactive": true, "children": [
        { "type": "check", "key": "stop", "op": "set" },
        { "type": "call", "call": "sing" }
    ]}))));
    run(&mut world, 3);
    set_blackboard(&mut world, e, "stop", json!(true));
    run(&mut world, 1);
    assert_eq!(blackboard(&world, e, "sing"), Some(json!("aborted")));
}

#[test]
fn decorators_and_composites_answer_as_they_should() {
    // repeat 3 times, then succeed; a counter shows how many.
    let mut world = World::new();
    let e = world.spawn((at(0.0, 0.0), BehaviorTree { looping: false, ..tree(json!(
        { "type": "repeat", "times": 3, "child": { "type": "set", "key": "n", "value": 1, "add": true } }
    )) }));
    callbacks::take();
    run(&mut world, 10);
    assert_eq!(blackboard(&world, e, "n"), Some(json!(3.0)), "once a frame, three times");
    assert!(callbacks::take().iter().any(|c| c.entity == e && c.handler == "on_tree_end"), "a tree that does not loop says it ended");

    // cooldown: the child runs at most once a second.
    let mut world = World::new();
    let e = world.spawn((at(0.0, 0.0), tree(json!(
        { "type": "cooldown", "seconds": 1.0, "child": { "type": "set", "key": "n", "value": 1, "add": true } }
    ))));
    run(&mut world, 60 * 3 + 1);
    let n = blackboard(&world, e, "n").unwrap().as_f64().unwrap();
    assert!((3.0..=4.0).contains(&n), "{n}");

    // timeout aborts a long wait; invert turns that failure into success.
    let mut world = World::new();
    let e = world.spawn((at(0.0, 0.0), BehaviorTree { looping: false, ..tree(json!({ "type": "sequence", "children": [
        { "type": "invert", "child": { "type": "timeout", "seconds": 0.25, "child": { "type": "wait", "seconds": 10 } } },
        { "type": "set", "key": "gave_up", "value": true }
    ]})) }));
    run(&mut world, 20);
    assert_eq!(blackboard(&world, e, "gave_up"), Some(json!(true)));

    // parallel: succeeds when one of two has.
    let mut world = World::new();
    let e = world.spawn((at(0.0, 0.0), BehaviorTree { looping: false, ..tree(json!({ "type": "sequence", "children": [
        { "type": "parallel", "succeed": 1, "children": [
            { "type": "wait", "seconds": 0.1 },
            { "type": "wait", "seconds": 5 }
        ]},
        { "type": "set", "key": "done", "value": true }
    ]})) }));
    run(&mut world, 10);
    assert_eq!(blackboard(&world, e, "done"), Some(json!(true)));
}

#[test]
fn random_is_steady_for_one_entity_and_differs_across_many() {
    let pick = || {
        let mut world = World::new();
        let es: Vec<_> = (0..12)
            .map(|_| world.spawn((at(0.0, 0.0), BehaviorTree { looping: false, ..tree(json!({ "type": "random", "children": [
                { "type": "set", "key": "pick", "value": "a" },
                { "type": "set", "key": "pick", "value": "b" },
                { "type": "set", "key": "pick", "value": "c" }
            ]})) })))
            .collect();
        run(&mut world, 1);
        es.iter().map(|e| blackboard(&world, *e, "pick").unwrap()).collect::<Vec<_>>()
    };
    let a = pick();
    assert_eq!(a, pick(), "the same world picks the same");
    assert!(a.iter().any(|p| p != &a[0]), "but not all alike: {a:?}");
}

#[test]
fn near_finds_the_closest_tagged_thing_and_remembers_where() {
    let mut world = World::new();
    let e = world.spawn((at(0.0, 0.0), BehaviorTree { looping: false, ..tree(json!({ "type": "sequence", "children": [
        { "type": "near", "target": "tag:coin", "distance": 100, "store": "coin" },
        { "type": "move_to", "target": "@coin", "speed": 600, "arrive": 1 }
    ]})) }));
    let mut tags = Tags::default();
    tags.0.insert("coin".into());
    world.spawn((at(90.0, 0.0), tags.clone()));
    world.spawn((at(-50.0, 0.0), tags));
    run(&mut world, 30);
    assert_eq!(blackboard(&world, e, "coin"), Some(json!([-50.0, 0.0])));
    assert!((pos(&world, e).x + 50.0).abs() <= 1.0);
}

#[test]
fn a_nav_agent_is_steered_rather_than_moved() {
    let mut world = World::new();
    let e = world.spawn((at(0.0, 0.0), jengine_core::navigation::NavAgent::default(), tree(json!(
        { "type": "move_to", "target": [200, 50], "speed": 90 }
    ))));
    run(&mut world, 1);
    let a = world.get::<&jengine_core::navigation::NavAgent>(e).unwrap();
    assert_eq!(a.target, Some([200.0, 50.0]));
    assert_eq!(a.speed, 90.0);
}

#[test]
fn a_script_can_swap_restart_and_pause_a_tree() {
    let mut world = World::new();
    let e = world.spawn((at(0.0, 0.0), tree(json!({ "type": "set", "key": "n", "value": 1, "add": true }))));
    run(&mut world, 3);
    set_behavior_tree_field(&world, e, "enabled", &json!(false)).unwrap();
    run(&mut world, 3);
    assert_eq!(blackboard(&world, e, "n"), Some(json!(3.0)));
    set_behavior_tree_field(&world, e, "root", &json!({ "type": "set", "key": "mode", "value": "new" })).unwrap();
    set_behavior_tree_field(&world, e, "enabled", &json!(true)).unwrap();
    run(&mut world, 1);
    assert_eq!(blackboard(&world, e, "mode"), Some(json!("new")));
    assert!(set_behavior_tree_field(&world, e, "colour", &json!(1)).is_err());
    assert!(set_behavior_tree_field(&world, e, "root", &json!({ "type": "dance" })).is_err());
}

#[test]
fn a_tree_round_trips_through_its_scene_key() {
    let t = tree(guard());
    let back: BehaviorTree = serde_json::from_value(serde_json::to_value(&t).unwrap()).unwrap();
    assert_eq!(back.root, t.root);
    let bundle: jengine_core::scene::format::ComponentBundle =
        serde_json::from_value(json!({ "behavior_tree": { "root": guard(), "loop": false } })).unwrap();
    let b = bundle.behavior_tree.expect("the scene key reaches the bundle");
    assert!(!b.looping);
}
