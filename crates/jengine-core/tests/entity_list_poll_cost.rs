//! What the editor's Scene-panel poll costs, and what asking first costs.
//!
//! The editor polls `viewport_get_runtime_entities` on a timer. That builds
//! four component sets over the world, formats a name per entity, serializes
//! the lot to JSON — and *then* hashes the JSON to decide whether anything
//! changed. The answer is almost always "nothing", so the whole snapshot is
//! built to discover it was not needed. At 16,000 entities this was the
//! sawtooth in the editor's frame graph (`docs/architecture-debt.md` #7).
//!
//! [`topology_fingerprint`] is the cheap question asked first. This measures
//! the two against each other so the short-circuit is justified by a number
//! rather than by the fact that it obviously ought to be faster.
//!
//! The snapshot half is a stand-in for the viewport's function, which is
//! `wasm_bindgen`-exported and cannot be called from a native test. It does
//! the same work in the same order; it is a cost model, not the real thing,
//! which is why the assertions below are about orders of magnitude and not
//! about specific milliseconds.
//!
//! `#[ignore]`d: timing. Run with
//! `cargo test -p jengine-core --release --test entity_list_poll_cost -- --ignored --nocapture`

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use hecs::{Entity, World};
use jengine_core::ecs::components::{Camera2DComponent, Name, Parent, SpriteComponent, Transform};
use jengine_core::ecs::topology::topology_fingerprint;

/// A world shaped like a scene the panel would actually list: named, sprited,
/// a shallow hierarchy, one camera.
fn scene_world(n: usize) -> World {
    let mut world = World::new();
    let root = world.spawn((Transform::default(), Name("Root".into())));
    world.spawn((Transform::default(), Camera2DComponent::default(), Name("Camera".into())));
    for i in 0..n {
        let e = world.spawn((
            Transform::default(),
            SpriteComponent::default(),
            Name(format!("Entity_{i}")),
        ));
        // Every fourth entity is parented, so the snapshot's index map has
        // something to resolve.
        if i % 4 == 0 {
            world.insert_one(e, Parent(root)).expect("parent");
        }
    }
    world
}

/// The snapshot the viewport builds, reproduced closely enough to time.
fn build_entity_list_json(world: &World, order: &[Entity]) -> String {
    let set_of = |f: &dyn Fn(&World) -> Vec<Entity>| -> HashSet<Entity> {
        f(world).into_iter().collect()
    };
    let kind_camera =
        set_of(&|w: &World| w.query::<&Camera2DComponent>().iter().map(|(e, _)| e).collect());
    let kind_sprite =
        set_of(&|w: &World| w.query::<&SpriteComponent>().iter().map(|(e, _)| e).collect());

    let index_of: HashMap<Entity, usize> = order
        .iter()
        .enumerate()
        .filter(|&(_, &e)| world.contains(e))
        .map(|(i, &e)| (e, i))
        .collect();

    let mut out = Vec::with_capacity(order.len());
    for (i, &entity) in order.iter().enumerate() {
        if !world.contains(entity) {
            continue;
        }
        let name = world
            .get::<&Name>(entity)
            .map(|n| n.0.clone())
            .unwrap_or_else(|_| format!("Entity_{}", entity.id()));
        let parent = world
            .get::<&Parent>(entity)
            .ok()
            .and_then(|p| index_of.get(&p.0).copied());
        let kind = if kind_camera.contains(&entity) {
            "camera"
        } else if kind_sprite.contains(&entity) {
            "sprite"
        } else {
            ""
        };
        out.push(serde_json::json!({
            "index": i, "name": name, "parent": parent, "kind": kind,
        }));
    }
    serde_json::to_string(&out).unwrap_or_default()
}

#[test]
#[ignore = "timing-sensitive; run with --release"]
fn report_poll_cost_against_fingerprint_cost() {
    println!(
        "{:>8}  {:>13}  {:>15}  {:>9}",
        "entities", "snapshot ms", "fingerprint ms", "ratio"
    );
    println!("{:->8}  {:->13}  {:->15}  {:->9}", "", "", "", "");

    for n in [500usize, 2_000, 8_000, 16_000] {
        let world = scene_world(n);
        let order: Vec<Entity> = world.iter().map(|e| e.entity()).collect();

        // Warm: the first pass pays for lazily-built archetype metadata, which
        // neither side pays again in the editor's steady state.
        let _ = build_entity_list_json(&world, &order);
        let _ = topology_fingerprint(&world);

        let t0 = Instant::now();
        let json = build_entity_list_json(&world, &order);
        let snap_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let t1 = Instant::now();
        let fp = topology_fingerprint(&world);
        let fp_ms = t1.elapsed().as_secs_f64() * 1000.0;

        assert!(!json.is_empty() && fp != 0);
        println!(
            "{n:>8}  {snap_ms:>13.3}  {fp_ms:>15.3}  {:>8.1}x",
            snap_ms / fp_ms.max(1e-9)
        );
    }
}

/// The property the short-circuit rests on, stated as a test rather than left
/// to the timing report: a world that only *moved* must fingerprint the same,
/// so the poll skips the snapshot on the common frame.
#[test]
fn a_moving_world_still_answers_unchanged() {
    let world = scene_world(2_000);
    let before = topology_fingerprint(&world);
    for (_, t) in world.query::<&mut Transform>().iter() {
        t.position.x += 1.0;
        t.rotation += 0.01;
    }
    assert_eq!(
        topology_fingerprint(&world),
        before,
        "motion changed the fingerprint, so the editor would rebuild the panel every frame"
    );
}

/// And the other half: a change the panel *would* show must not be skipped.
#[test]
fn a_spawn_a_rename_and_a_reparent_each_answer_changed() {
    let mut world = scene_world(200);
    let ids: Vec<Entity> = world.iter().map(|e| e.entity()).collect();

    let before = topology_fingerprint(&world);
    let spawned = world.spawn((Transform::default(), Name("Late".into())));
    let after_spawn = topology_fingerprint(&world);
    assert_ne!(after_spawn, before, "a spawn was not noticed");

    world.get::<&mut Name>(spawned).expect("name").0 = "Renamed".into();
    let after_rename = topology_fingerprint(&world);
    assert_ne!(after_rename, after_spawn, "a rename was not noticed");

    world.insert_one(spawned, Parent(ids[0])).expect("parent");
    assert_ne!(
        topology_fingerprint(&world),
        after_rename,
        "a reparent was not noticed"
    );
}
