//! **A trigger's every output can be wired, by name.**
//!
//! The graph compiler keyed its output map by node id alone and ignored a
//! reference's `"out"`, so each node could hand over one output - the
//! "primary". Three triggers have more than one, and the others were
//! advertised by the catalog and the editor's palette and could not be read:
//!
//! * `OnCollision.enter` - so a graph could not tell a contact **starting**
//!   from one **ending**, and its "on hit" logic ran on both. A projectile
//!   that damages on contact damaged twice; a pickup that counts itself
//!   counted twice.
//! * `OnMouseClick.button` - so "right click cancels" could not be a graph.
//!
//! And an `out` naming something that does not exist compiled, silently
//! handing over the primary instead. It is an error now.

use hecs::World;
use jengine_core::behavior::{BehaviorAsset, BehaviorGraphSystem, compile};
use jengine_core::ecs::components::{BehaviorGraphComponent, CustomComponents, Transform};
use jengine_core::physics::{ContactEvent, ContactType};
use std::collections::HashMap;

/// Counts the contacts that *started*, and separately every one it heard.
const COUNT_HITS: &str = r#"{
  "name": "CountHits",
  "version": 1,
  "nodes": [
    { "id": "hit",   "op": "OnCollision" },
    { "id": "gate",  "op": "If", "in": { "cond": { "from": "hit", "out": "enter" } } },

    { "id": "n",     "op": "GetCustomField", "params": { "component": "C", "field": "hits" } },
    { "id": "n1",    "op": "MathFloat", "params": { "op": "Add" },
      "in": { "a": { "from": "n" }, "b": { "value": 1.0 } } },
    { "id": "save",  "op": "SetCustomField", "params": { "component": "C", "field": "hits" },
      "in": { "value": { "from": "n1" } } },

    { "id": "all",   "op": "GetCustomField", "params": { "component": "C", "field": "heard" } },
    { "id": "all1",  "op": "MathFloat", "params": { "op": "Add" },
      "in": { "a": { "from": "all" }, "b": { "value": 1.0 } } },
    { "id": "log",   "op": "SetCustomField", "params": { "component": "C", "field": "heard" },
      "in": { "value": { "from": "all1" } } }
  ],
  "edges": [
    { "from": "hit",  "to": "gate" },
    { "from": "gate", "out": "then", "to": "save" },
    { "from": "hit",  "to": "log" }
  ]
}"#;

fn field(world: &World, e: hecs::Entity, f: &str) -> f64 {
    world
        .get::<&CustomComponents>(e)
        .ok()
        .and_then(|c| c.0.get("C").and_then(|m| m.get(f)).and_then(|v| v.as_f64()))
        .unwrap_or(0.0)
}

#[test]
fn a_collision_graph_can_tell_a_contact_starting_from_one_ending() {
    let mut world = World::new();
    let mut sys = BehaviorGraphSystem::new();
    let graphs = HashMap::from([("count.behavior".to_string(), COUNT_HITS.to_string())]);

    let me = world.spawn((
        Transform::default(),
        CustomComponents(Default::default()),
        BehaviorGraphComponent { graph_path: "count.behavior".into(), ..Default::default() },
    ));
    let wall = world.spawn((Transform::default(),));
    let errors = sys.sync_instances_inline(&mut world, &graphs);
    assert!(errors.is_empty(), "{errors:?}");

    let (a, b) = (u64::from(me.id()), u64::from(wall.id()));
    sys.dispatch_contact_events(&[ContactEvent::simple(a, b, ContactType::Started)], &mut world);
    sys.dispatch_contact_events(&[ContactEvent::simple(a, b, ContactType::Stopped)], &mut world);

    assert_eq!(field(&world, me, "heard"), 2.0, "both halves of the contact reach the graph");
    assert_eq!(
        field(&world, me, "hits"),
        1.0,
        "and `OnCollision.enter` lets it count only the one that started"
    );
}

fn compiles(json: &str) -> Result<(), String> {
    let asset = BehaviorAsset::from_json(json).map_err(|e| e.to_string())?;
    compile(&asset).map(|_| ()).map_err(|e| e.to_string())
}

#[test]
fn a_mouse_click_graph_can_read_which_button() {
    let graph = r#"{ "name": "Click", "version": 1,
      "nodes": [
        { "id": "click", "op": "OnMouseClick" },
        { "id": "right", "op": "Compare", "params": { "op": "Eq" },
          "in": { "a": { "from": "click", "out": "button" }, "b": { "value": 1.0 } } },
        { "id": "gate",  "op": "If", "in": { "cond": { "from": "right" } } },
        { "id": "say",   "op": "Log", "params": { "label": "right" } } ],
      "edges": [ { "from": "click", "to": "gate" },
                 { "from": "gate", "out": "then", "to": "say" } ] }"#;
    compiles(graph).expect("OnMouseClick.button should be wirable");
}

/// Naming the primary explicitly is the same as leaving `out` off.
#[test]
fn naming_the_primary_output_is_allowed() {
    let graph = r#"{ "name": "Timer", "version": 1,
      "nodes": [
        { "id": "t",   "op": "OnTimer", "params": { "interval_seconds": 1.0 } },
        { "id": "say", "op": "Log", "params": { "label": "tick" },
          "in": { "value": { "from": "t", "out": "tick_index" } } } ],
      "edges": [ { "from": "t", "to": "say" } ] }"#;
    compiles(graph).expect("naming a trigger's primary output should compile");

    let graph = r#"{ "name": "Read", "version": 1,
      "nodes": [
        { "id": "tick", "op": "OnTick" },
        { "id": "p",    "op": "GetPosition" },
        { "id": "go",   "op": "SetPosition", "in": { "pos": { "from": "p", "out": "value" } } } ],
      "edges": [ { "from": "tick", "to": "go" } ] }"#;
    compiles(graph).expect("naming a read's only output should compile");
}

/// A misspelt output used to compile and quietly hand over the primary.
#[test]
fn an_output_that_does_not_exist_is_refused_by_name() {
    let graph = r#"{ "name": "Bad", "version": 1,
      "nodes": [
        { "id": "hit",  "op": "OnCollision" },
        { "id": "gate", "op": "If", "in": { "cond": { "from": "hit", "out": "entered" } } },
        { "id": "say",  "op": "Log", "params": { "label": "x" } } ],
      "edges": [ { "from": "hit", "to": "gate" },
                 { "from": "gate", "out": "then", "to": "say" } ] }"#;
    let err = compiles(graph).expect_err("`out: entered` names nothing and should not compile");
    assert!(err.contains("hit.entered"), "the error should name what was asked for: {err}");
}

/// **A pure op can take a trigger's output.** It could not: payload slots
/// were allocated after every producer had been compiled, so this failed
/// with "unknown node `ev`" - a graph could act on what triggered it but
/// never compute with it.
#[test]
fn arithmetic_can_take_a_triggers_output() {
    let graph = r#"{ "name": "P", "version": 1,
      "nodes": [
        { "id": "ev", "op": "OnEvent", "params": { "name": "x" } },
        { "id": "twice", "op": "MathFloat", "params": { "op": "Mul" },
          "in": { "a": { "from": "ev" }, "b": { "value": 2.0 } } },
        { "id": "say", "op": "Log", "params": { "label": "v" }, "in": { "value": { "from": "twice" } } } ],
      "edges": [ { "from": "ev", "to": "say" } ] }"#;
    compiles(graph).expect("`MathFloat(OnEvent.value, 2)` should compile");
}

/// And it runs in the right order: the payload is written before the
/// prepended arithmetic reads it, so an event of 3 doubles to 6 rather than
/// to whatever the slot held last.
#[test]
fn arithmetic_on_a_triggers_output_sees_this_events_value() {
    let graph = r#"{ "name": "Double", "version": 1,
      "nodes": [
        { "id": "ev",    "op": "OnEvent", "params": { "name": "x" } },
        { "id": "twice", "op": "MathFloat", "params": { "op": "Mul" },
          "in": { "a": { "from": "ev" }, "b": { "value": 2.0 } } },
        { "id": "save",  "op": "SetCustomField", "params": { "component": "C", "field": "v" },
          "in": { "value": { "from": "twice" } } } ],
      "edges": [ { "from": "ev", "to": "save" } ] }"#;
    let mut world = World::new();
    let mut sys = BehaviorGraphSystem::new();
    let graphs = HashMap::from([("d.behavior".to_string(), graph.to_string())]);
    let e = world.spawn((
        Transform::default(),
        CustomComponents(Default::default()),
        BehaviorGraphComponent { graph_path: "d.behavior".into(), ..Default::default() },
    ));
    assert!(sys.sync_instances_inline(&mut world, &graphs).is_empty());

    sys.dispatch_event("x", 3.0, &mut world);
    assert_eq!(field(&world, e, "v"), 6.0);
    sys.dispatch_event("x", 5.0, &mut world);
    assert_eq!(field(&world, e, "v"), 10.0, "each event's own value, not the previous one");
}

