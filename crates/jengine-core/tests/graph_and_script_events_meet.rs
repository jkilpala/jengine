//! **Scripts and behaviour graphs share one event bus**, both ways.
//!
//! Until Sep 2026 they had two that never met. A graph's `EmitEvent` reached
//! other graphs' `OnEvent` at the barrier and stopped there - the enum's own
//! comment said "routing wiring is host-specific", and no host had written
//! any. And `BehaviorGraphSystem::dispatch_event` was called only from inside
//! the graph system, so a script's `send_event` never reached a graph.
//!
//! That is not a corner. It means an enemy authored as a graph cannot tell
//! the game it died, and a wave director written in Lua cannot tell a graph
//! anything - which is to say the two authoring surfaces this engine offers
//! for gameplay could not be used in one game. Found by the arcade's first
//! game whose enemies are graphs.
//!
//! This drives the real native path: a `ScriptSystem` running Luau, a
//! `BehaviorGraphSystem` running a graph, and the calls the native runner
//! makes between them. The browser hosts make the same calls from arms that
//! only run in a browser; `graph_events_reach_every_runtime` checks they
//! exist.

#![cfg(all(feature = "native", feature = "behavior"))]

use hecs::World;
use jengine_core::behavior::BehaviorGraphSystem;
use jengine_core::ecs::components::{
    BehaviorGraphComponent, CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform,
};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;
use std::collections::HashMap;

/// On despawn, say so - with a score. On "enrage", write down how angry.
const ENEMY: &str = r#"{
  "name": "Enemy",
  "version": 1,
  "nodes": [
    { "id": "gone",  "op": "OnDespawn" },
    { "id": "down",  "op": "EmitEvent",
      "params": { "name": "enemy_down" },
      "in": { "value": { "value": 50.0 } } },

    { "id": "rage",  "op": "OnEvent", "params": { "name": "enrage" } },
    { "id": "angry", "op": "SetCustomField",
      "params": { "component": "Enemy", "field": "rage" },
      "in": { "value": { "from": "rage", "out": "value" } } }
  ],
  "edges": [
    { "from": "gone", "to": "down" },
    { "from": "rage", "to": "angry" }
  ]
}"#;

const DIRECTOR: &str = r#"
function init(entity)
    on_event("enemy_down", entity)
end

function update(entity, dt)
    if get_custom(entity, "Probe", "sent") == nil then
        set_custom(entity, "Probe", "sent", 1)
        send_event("enrage", entity, 3)
    end
end

function on_enemy_down(entity, data)
    set_custom(entity, "Probe", "heard", (get_custom(entity, "Probe", "heard") or 0) + 1)
    if data ~= nil then
        set_custom(entity, "Probe", "value", data.value)
        set_custom(entity, "Probe", "x", data.x)
        set_custom(entity, "Probe", "y", data.y)
    end
end
"#;

struct Frame {
    world: World,
    scripts: ScriptSystem,
    graphs: BehaviorGraphSystem,
    library: HashMap<String, String>,
    input: InputState,
}

impl Frame {
    /// What the native runner does, in its order: scripts, then scripts'
    /// events to graphs, then the barrier, then graphs' events to scripts.
    fn step(&mut self) {
        let errors = self.graphs.sync_instances_inline(&mut self.world, &self.library);
        assert!(errors.is_empty(), "a test graph did not load: {errors:?}");
        self.scripts.run(&mut self.world, 1.0 / 60.0, &self.input, None, None);
        for ev in self.scripts.take_events_for_graphs() {
            self.graphs.deliver_script_event(&ev.name, ev.data.as_ref(), &mut self.world);
        }
        let routed = self.graphs.flush_commands_and_route_events(&mut self.world, None);
        self.scripts.queue_graph_events(&routed.emit_event_requests);
    }
}

fn setup() -> (Frame, hecs::Entity, hecs::Entity) {
    let dir = std::env::temp_dir().join(format!(
        "jengine_graph_script_events_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("director.lua");
    std::fs::write(&path, DIRECTOR).unwrap();

    let mut world = World::new();
    let director = world.spawn((
        Name("Director".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_string_lossy().into(),
            cull_distance: None,
        },
    ));
    let t = Transform { position: glam::Vec2::new(-120.0, 75.0), ..Default::default() };
    let enemy = world.spawn((
        Name("Moth".into()),
        t,
        CustomComponents(Default::default()),
        BehaviorGraphComponent { graph_path: "enemy.behavior".into(), ..Default::default() },
    ));

    let frame = Frame {
        world,
        scripts: ScriptSystem::new().expect("script system"),
        graphs: BehaviorGraphSystem::new(),
        library: HashMap::from([("enemy.behavior".to_string(), ENEMY.to_string())]),
        input: InputState::new(),
    };
    (frame, director, enemy)
}

fn probe(world: &World, e: hecs::Entity, component: &str, field: &str) -> Option<f64> {
    let c = world.get::<&CustomComponents>(e).ok()?;
    c.0.get(component)?.get(field)?.as_f64()
}

#[test]
fn a_scripts_event_reaches_a_graphs_on_event() {
    let (mut f, _director, enemy) = setup();
    for _ in 0..3 {
        f.step();
    }
    assert_eq!(
        probe(&f.world, enemy, "Enemy", "rage"),
        Some(3.0),
        "the director's `send_event(\"enrage\", entity, 3)` should have reached the graph's \
         OnEvent with its number"
    );
}

#[test]
fn a_graphs_event_reaches_a_scripts_listener_with_where_it_happened() {
    let (mut f, director, enemy) = setup();
    for _ in 0..2 {
        f.step();
    }
    // Killed from outside the graph - the way the combat layer kills.
    f.world.despawn(enemy).unwrap();
    for _ in 0..3 {
        f.step();
    }

    assert_eq!(
        probe(&f.world, director, "Probe", "heard"),
        Some(1.0),
        "the director should have heard `enemy_down` exactly once"
    );
    assert_eq!(probe(&f.world, director, "Probe", "value"), Some(50.0), "with its score");
    // Where it died, not the origin: the entity was gone before the graph
    // ran its `OnDespawn`, which is the case `last_seen` exists for.
    assert_eq!(probe(&f.world, director, "Probe", "x"), Some(-120.0), "and where it died");
    assert_eq!(probe(&f.world, director, "Probe", "y"), Some(75.0));
}

/// A graph's event goes to the scripts; it must not then come *back* to the
/// graphs as if a script had sent it. The graphs heard it at the barrier.
#[test]
fn a_graphs_event_is_not_handed_back_to_graphs() {
    let mut f = setup().0;
    // One graph emits `ping` every frame; another counts every `ping` it hears.
    const PINGER: &str = r#"{ "name": "Pinger", "version": 1,
      "nodes": [ { "id": "t", "op": "OnTick" },
                 { "id": "e", "op": "EmitEvent", "params": { "name": "ping" } } ],
      "edges": [ { "from": "t", "to": "e" } ] }"#;
    const COUNTER: &str = r#"{ "name": "Counter", "version": 1,
      "nodes": [ { "id": "h", "op": "OnEvent", "params": { "name": "ping" } },
                 { "id": "n", "op": "GetCustomField", "params": { "component": "C", "field": "n" } },
                 { "id": "p", "op": "MathFloat", "params": { "op": "Add" },
                   "in": { "a": { "from": "n" }, "b": { "value": 1.0 } } },
                 { "id": "w", "op": "SetCustomField", "params": { "component": "C", "field": "n" },
                   "in": { "value": { "from": "p" } } } ],
      "edges": [ { "from": "h", "to": "w" } ] }"#;
    f.library.insert("pinger.behavior".into(), PINGER.into());
    f.library.insert("counter.behavior".into(), COUNTER.into());
    f.world.spawn((
        Transform::default(),
        BehaviorGraphComponent { graph_path: "pinger.behavior".into(), ..Default::default() },
    ));
    let counter = f.world.spawn((
        Transform::default(),
        CustomComponents(Default::default()),
        BehaviorGraphComponent { graph_path: "counter.behavior".into(), ..Default::default() },
    ));

    // Ticks are `tick_pre_simulation`'s job; drive them as a runner does.
    for _ in 0..10 {
        f.graphs.tick_pre_simulation(&mut f.world, 1.0 / 60.0);
        f.step();
    }
    let n = probe(&f.world, counter, "C", "n").unwrap_or(0.0);
    assert!(
        (9.0..=10.0).contains(&n),
        "ten pings should be heard about ten times, not twice that: heard {n}"
    );
}
