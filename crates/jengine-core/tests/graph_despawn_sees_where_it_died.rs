//! **An `OnDespawn` handler knows where its entity was.**
//!
//! The obvious use of `OnDespawn` is something that happens *at the place
//! of death*: a splitter spawning its halves, a burst, a dropped coin. All of
//! them read `GetPosition` in the handler.
//!
//! The probe that started this file asks the plainest version of that: an
//! entity at (300, 200) with a graph that, on despawn, spawns a marker at its
//! own position. Where does the marker land?

use hecs::World;
use jengine_core::behavior::BehaviorGraphSystem;
use jengine_core::ecs::components::{BehaviorGraphComponent, Name, Transform};
use std::collections::HashMap;

const DROP_ON_DEATH: &str = r#"{
  "name": "DropOnDeath",
  "version": 1,
  "nodes": [
    { "id": "gone",  "op": "OnDespawn" },
    { "id": "here",  "op": "GetPosition" },
    { "id": "drop",  "op": "SpawnEntity",
      "params": { "prefab_id": "Marker" },
      "in": { "at": { "from": "here" } } }
  ],
  "edges": [ { "from": "gone", "to": "drop" } ]
}"#;

fn graphs() -> HashMap<String, String> {
    HashMap::from([("drop.behavior".to_string(), DROP_ON_DEATH.to_string())])
}

fn markers(world: &World) -> Vec<(f32, f32)> {
    world
        .query::<(&Name, &Transform)>()
        .iter()
        .filter(|(_, (n, _))| n.0.contains("Marker"))
        .map(|(_, (_, t))| (t.position.x, t.position.y))
        .collect()
}

fn spawn_victim(world: &mut World) -> hecs::Entity {
    let t = Transform { position: glam::Vec2::new(300.0, 200.0), ..Default::default() };
    world.spawn((
        Name("Victim".into()),
        t,
        BehaviorGraphComponent { graph_path: "drop.behavior".into(), ..Default::default() },
    ))
}

/// Despawned by anything that is not the graph - `despawn_dead`, a
/// `Lifetime`, a script's `despawn`. This is the common case: an enemy dies
/// because the combat layer took its health, not because it chose to.
#[test]
fn a_graph_despawned_from_outside_drops_where_it_died() {
    let mut world = World::new();
    let mut sys = BehaviorGraphSystem::new();
    let g = graphs();

    let victim = spawn_victim(&mut world);
    sys.sync_instances_inline(&mut world, &g);
    sys.flush_commands_and_route_events(&mut world, None);

    world.despawn(victim).unwrap();
    sys.sync_instances_inline(&mut world, &g);
    sys.flush_commands_and_route_events(&mut world, None);

    let m = markers(&world);
    assert_eq!(m.len(), 1, "OnDespawn should have spawned one marker, got {m:?}");
    let (x, y) = m[0];
    assert!(
        (x - 300.0).abs() < 0.01 && (y - 200.0).abs() < 0.01,
        "the marker should land where the victim died, (300, 200), and landed at ({x}, {y})"
    );
}
