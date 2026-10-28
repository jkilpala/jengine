//! Integration test for Slice 7.2 — the behavior graph runtime MVP.
//!
//! Drives a `BehaviorGraphSystem` through the same Phase-2 contract
//! the runtime uses (after `update_input_actions`, before native
//! motion), and asserts:
//!
//! - `OnTick → GetInputAxis → SetVelocity` writes velocity from input.
//! - `BounceOnBounds` is idempotent (insert once; no re-insert/reset).
//! - `DespawnAfter` does NOT reset an existing `Lifetime` across ticks
//!   (spec §2.6 footgun protection).
#![cfg(feature = "behavior")]

use std::path::PathBuf;

use glam::Vec2;
use hecs::World;

use jengine_core::behavior::BehaviorGraphSystem;
use jengine_core::ecs::components::{
    BounceArea, ExpireAction, Force2D, InputAxis2D, Lifetime, Name, Transform, Velocity2D,
};

const PLAYER_MOVER: &str = r#"{
    "name": "PlayerMover", "version": 1,
    "nodes": [
        { "id": "n0", "op": "OnTick" },
        { "id": "n1", "op": "GetInputAxis" },
        { "id": "n2", "op": "SetVelocity",
          "in": { "vel": { "from": "n1", "scale": 200.0 } } },
        { "id": "n3", "op": "BounceOnBounds" }
    ],
    "edges": [
        { "from": "n0", "to": "n2" },
        { "from": "n0", "to": "n3" }
    ]
}"#;

const DESPAWN_AFTER: &str = r#"{
    "name": "Despawner", "version": 1,
    "nodes": [
        { "id": "n0", "op": "OnTick" },
        { "id": "n1", "op": "DespawnAfter",
          "in": { "seconds": { "value": 5.0 } } }
    ],
    "edges": [{ "from": "n0", "to": "n1" }]
}"#;

#[test]
fn player_mover_writes_velocity_from_input_and_inserts_bounce_area() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let entity = world.spawn((
        Velocity2D::default(),
        InputAxis2D {
            current: Vec2::new(1.0, -0.5),
            ..Default::default()
        },
    ));

    system
        .attach_inline(entity, PathBuf::from("test://mover"), PLAYER_MOVER)
        .expect("compile + attach");

    // Tick once.
    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let vel = world.get::<&Velocity2D>(entity).expect("velocity present");
    assert!((vel.x - 200.0).abs() < 1e-3, "vx = {}", vel.x);
    assert!((vel.y - (-100.0)).abs() < 1e-3, "vy = {}", vel.y);
    drop(vel);

    // BounceOnBounds is a setup op — must have inserted a BounceArea.
    let area = world.get::<&BounceArea>(entity).expect("bounce area inserted");
    let default_area = BounceArea::default();
    assert_eq!(area.min_x, default_area.min_x);
    assert_eq!(area.max_y, default_area.max_y);
    drop(area);

    // Tick a second time with different input — velocity must update;
    // BounceArea must still be present and unchanged (no churn).
    {
        let mut axis = world.get::<&mut InputAxis2D>(entity).unwrap();
        axis.current = Vec2::new(-0.25, 0.0);
    }
    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let vel = world.get::<&Velocity2D>(entity).unwrap();
    assert!((vel.x - (-50.0)).abs() < 1e-3, "vx after = {}", vel.x);
    assert!((vel.y - 0.0).abs() < 1e-3, "vy after = {}", vel.y);
}

#[test]
fn despawn_after_is_idempotent_does_not_reset_existing_lifetime() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let entity = world.spawn(());
    system
        .attach_inline(entity, PathBuf::from("test://despawn"), DESPAWN_AFTER)
        .expect("attach");

    // Tick 1 — inserts Lifetime { remaining: 5.0 }.
    system.tick_pre_simulation(&mut world, 0.0);
    {
        let life = world.get::<&Lifetime>(entity).expect("lifetime inserted");
        assert!((life.remaining - 5.0).abs() < 1e-6);
        assert!(matches!(life.on_expire, ExpireAction::Despawn));
    }

    // Manually advance the lifetime as if `update_lifetimes` ran — the
    // setup op must NOT reset this on the next tick.
    {
        let mut life = world.get::<&mut Lifetime>(entity).unwrap();
        life.remaining = 1.25;
    }

    // Tick 2 + 3 — DespawnAfter should be a no-op since Lifetime is present.
    system.tick_pre_simulation(&mut world, 0.0);
    system.tick_pre_simulation(&mut world, 0.0);
    let life = world.get::<&Lifetime>(entity).unwrap();
    assert!(
        (life.remaining - 1.25).abs() < 1e-6,
        "DespawnAfter reset Lifetime.remaining ({}); spec §2.6 says it must NOT",
        life.remaining
    );

    // Compile-time warning was emitted (setup op under OnTick).
    let g_id = system
        .cache
        .load_from_json_with_key(PathBuf::from("test://despawn"), DESPAWN_AFTER)
        .unwrap();
    let g = system.cache.get(g_id);
    assert_eq!(g.diagnostics.len(), 1);
}

#[test]
fn telemetry_counts_instances_and_ops_per_frame() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let e1 = world.spawn((Velocity2D::default(), InputAxis2D::default()));
    let e2 = world.spawn((Velocity2D::default(), InputAxis2D::default()));
    system
        .attach_inline(e1, PathBuf::from("test://m"), PLAYER_MOVER)
        .unwrap();
    system
        .attach_inline(e2, PathBuf::from("test://m"), PLAYER_MOVER)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    assert_eq!(system.counters.instances_ticked, 2);
    // tick_program for PLAYER_MOVER has 4 ops (GetInputAxis,
    // SetVelocity, BounceOnBounds, plus the OnTick marker that we
    // do NOT emit — but the topo phase emits GetInputAxis as a
    // producer; control-flow phase emits SetVelocity + BounceOnBounds).
    // Per instance: 3 ops × 2 instances = 6.
    assert_eq!(system.counters.ops_executed, 6);

    // Re-tick — counters reset each frame.
    system.tick_pre_simulation(&mut world, 1.0 / 60.0);
    assert_eq!(system.counters.instances_ticked, 2);
    assert_eq!(system.counters.ops_executed, 6);
}

#[test]
fn detach_on_entity_despawn_is_picked_up_by_sync_instances() {
    use jengine_core::ecs::components::BehaviorGraphComponent;

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    // sync_instances() requires real disk; bypass with attach_inline,
    // then drive sync_instances over the world to trigger detach when
    // the component disappears.
    let entity = world.spawn((
        Velocity2D::default(),
        InputAxis2D::default(),
        BehaviorGraphComponent {
            graph_path: "ignored.behavior".into(),
            overrides: Default::default(),
        },
    ));
    system
        .attach_inline(entity, PathBuf::from("test://m"), PLAYER_MOVER)
        .unwrap();
    assert_eq!(system.instances.len(), 1);

    // Despawn the entity; sync_instances must drop the orphaned instance.
    world.despawn(entity).unwrap();
    let _errors = system.sync_instances(&mut world, std::path::Path::new("/nonexistent"));
    assert_eq!(system.instances.len(), 0);
}

// ── Slice 7.3 ── triggers + payload pins + Phase 6 dispatch ──

const ON_SPAWN_SETS_VELOCITY: &str = r#"{
    "name": "SpawnVel", "version": 1,
    "nodes": [
        { "id": "s", "op": "OnSpawn" },
        { "id": "v", "op": "SetVelocity",
          "in": { "vel": { "value": [42.0, -7.0] } } }
    ],
    "edges": [{ "from": "s", "to": "v" }]
}"#;

const ON_DESPAWN_SETS_VELOCITY: &str = r#"{
    "name": "DespawnVel", "version": 1,
    "nodes": [
        { "id": "d", "op": "OnDespawn" },
        { "id": "v", "op": "SetVelocity",
          "in": { "vel": { "value": [-1.0, -1.0] } } }
    ],
    "edges": [{ "from": "d", "to": "v" }]
}"#;

const ON_COLLISION_ADDS_VELOCITY: &str = r#"{
    "name": "ColAdd", "version": 1,
    "nodes": [
        { "id": "c", "op": "OnCollision" },
        { "id": "a", "op": "AddVelocity",
          "in": { "delta": { "value": [5.0, 0.0] } } }
    ],
    "edges": [{ "from": "c", "to": "a" }]
}"#;

#[test]
fn dispatch_spawn_fires_on_spawn_program_and_sets_velocity() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let entity = world.spawn((Velocity2D::default(),));
    system
        .attach_inline(entity, PathBuf::from("test://spawn"), ON_SPAWN_SETS_VELOCITY)
        .unwrap();

    // Velocity should still be zero — attach_inline does not auto-fire.
    let vel = world.get::<&Velocity2D>(entity).unwrap();
    assert_eq!(vel.x, 0.0);
    drop(vel);

    // Manually dispatch OnSpawn (the inline-attach path).
    system.dispatch_spawn(entity, &mut world);

    let vel = world.get::<&Velocity2D>(entity).unwrap();
    assert!((vel.x - 42.0).abs() < 1e-6);
    assert!((vel.y - (-7.0)).abs() < 1e-6);
}

#[test]
fn dispatch_despawn_fires_before_detach_via_sync_instances() {
    use jengine_core::ecs::components::BehaviorGraphComponent;

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let entity = world.spawn((
        Velocity2D::default(),
        BehaviorGraphComponent {
            graph_path: "ignored.behavior".into(),
            overrides: Default::default(),
        },
    ));
    system
        .attach_inline(entity, PathBuf::from("test://despawn"), ON_DESPAWN_SETS_VELOCITY)
        .unwrap();

    // Drop the component (entity itself stays alive) so sync_instances
    // detaches and OnDespawn fires.
    world.remove_one::<BehaviorGraphComponent>(entity).unwrap();
    let _ = system.sync_instances(&mut world, std::path::Path::new("/nonexistent"));

    let vel = world.get::<&Velocity2D>(entity).unwrap();
    assert!((vel.x - (-1.0)).abs() < 1e-6, "OnDespawn did not run before detach");
    assert_eq!(system.instances.len(), 0);
}

#[test]
fn dispatch_collision_runs_on_collision_program() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let entity = world.spawn((Velocity2D::default(),));
    let other = world.spawn(());
    system
        .attach_inline(entity, PathBuf::from("test://col"), ON_COLLISION_ADDS_VELOCITY)
        .unwrap();

    system.dispatch_collision(entity, other, true, &mut world);
    system.dispatch_collision(entity, other, false, &mut world);

    let vel = world.get::<&Velocity2D>(entity).unwrap();
    assert!((vel.x - 10.0).abs() < 1e-6, "collision delta accumulated twice");
}

#[test]
fn unsubscribed_dispatch_is_a_noop_and_does_not_count_events() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let entity = world.spawn((Velocity2D::default(),));
    // PLAYER_MOVER subscribes only to OnTick.
    system
        .attach_inline(entity, PathBuf::from("test://mover"), PLAYER_MOVER)
        .unwrap();

    system.dispatch_collision(entity, entity, true, &mut world);
    assert_eq!(system.counters.events_dispatched, 0);
    assert_eq!(system.counters.ops_executed, 0);
}

// ── Slice 7.4A ── gate ops (If / Cooldown / Once) ──

const ONCE_SETS_VELOCITY: &str = r#"{
    "name": "OnceVel", "version": 1,
    "nodes": [
        { "id": "t", "op": "OnTick" },
        { "id": "o", "op": "Once" },
        { "id": "a", "op": "AddVelocity",
          "in": { "delta": { "value": [1.0, 0.0] } } }
    ],
    "edges": [
        { "from": "t", "to": "o" },
        { "from": "o", "to": "a" }
    ]
}"#;

const COOLDOWN_HALF_SECOND: &str = r#"{
    "name": "CDHalf", "version": 1,
    "nodes": [
        { "id": "t", "op": "OnTick" },
        { "id": "cd", "op": "Cooldown",
          "in": { "seconds": { "value": 0.5 } } },
        { "id": "a", "op": "AddVelocity",
          "in": { "delta": { "value": [1.0, 0.0] } } }
    ],
    "edges": [
        { "from": "t", "to": "cd" },
        { "from": "cd", "to": "a" }
    ]
}"#;

const IF_BRANCHES: &str = r#"{
    "name": "IfBranch", "version": 1,
    "nodes": [
        { "id": "t", "op": "OnTick" },
        { "id": "i", "op": "If", "in": { "cond": { "value": COND } } },
        { "id": "y", "op": "SetVelocity",
          "in": { "vel": { "value": [10.0, 0.0] } } },
        { "id": "n", "op": "SetVelocity",
          "in": { "vel": { "value": [-10.0, 0.0] } } }
    ],
    "edges": [
        { "from": "t", "to": "i" },
        { "from": "i", "out": "then", "to": "y" },
        { "from": "i", "out": "else", "to": "n" }
    ]
}"#;

#[test]
fn once_fires_exactly_one_time_across_many_ticks() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn((Velocity2D::default(),));
    system
        .attach_inline(entity, PathBuf::from("test://once"), ONCE_SETS_VELOCITY)
        .unwrap();

    for _ in 0..50 {
        system.tick_pre_simulation(&mut world, 1.0 / 60.0);
    }

    let vel = world.get::<&Velocity2D>(entity).unwrap();
    assert!(
        (vel.x - 1.0).abs() < 1e-6,
        "Once should fire exactly once; vx = {}",
        vel.x
    );
}

#[test]
fn cooldown_fires_at_t0_and_then_every_half_second() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn((Velocity2D::default(),));
    system
        .attach_inline(entity, PathBuf::from("test://cd"), COOLDOWN_HALF_SECOND)
        .unwrap();

    // Cooldown(0.5) fires when remaining <= 0, then sets remaining = 0.5.
    // dt=0.1, 11 ticks → simulated time ≈1.0s. Expected fires:
    //   T1 (t=0.0)         → fire #1, remaining = 0.5
    //   T2..T6 (t=0.1..0.5) → countdown to ~0
    //   T7 (t=0.6)         → fire #2, remaining = 0.5
    //   T8..T11            → countdown
    // Total: 2 fires → vx = 2.0.
    let dt = 0.1;
    for _ in 0..11 {
        system.tick_pre_simulation(&mut world, dt);
    }

    let vel = world.get::<&Velocity2D>(entity).unwrap();
    assert!(
        (vel.x - 2.0).abs() < 1e-3,
        "Cooldown(0.5) should have fired 2× over 11 dt=0.1 ticks; vx = {}",
        vel.x
    );
}

#[test]
fn if_runs_then_branch_when_condition_true() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn((Velocity2D::default(),));
    system
        .attach_inline(
            entity,
            PathBuf::from("test://if-true"),
            &IF_BRANCHES.replace("COND", "true"),
        )
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let vel = world.get::<&Velocity2D>(entity).unwrap();
    assert!((vel.x - 10.0).abs() < 1e-6, "then branch should set +10; vx = {}", vel.x);
}

#[test]
fn if_runs_else_branch_when_condition_false() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn((Velocity2D::default(),));
    system
        .attach_inline(
            entity,
            PathBuf::from("test://if-false"),
            &IF_BRANCHES.replace("COND", "false"),
        )
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let vel = world.get::<&Velocity2D>(entity).unwrap();
    assert!((vel.x - (-10.0)).abs() < 1e-6, "else branch should set -10; vx = {}", vel.x);
}

// ── Slice 7.4B ── motion ops (Translate / ApplyForce / GetPosition / GetVelocity) ──

const TRANSLATE_RIGHT: &str = r#"{
    "name": "TransRight", "version": 1,
    "nodes": [
        { "id": "t", "op": "OnTick" },
        { "id": "tr", "op": "Translate",
          "in": { "delta": { "value": [1.0, 0.0] } } }
    ],
    "edges": [{ "from": "t", "to": "tr" }]
}"#;

const APPLY_FORCE: &str = r#"{
    "name": "PushUp", "version": 1,
    "nodes": [
        { "id": "t", "op": "OnTick" },
        { "id": "f", "op": "ApplyForce",
          "in": { "force": { "value": [0.0, 5.0] } } }
    ],
    "edges": [{ "from": "t", "to": "f" }]
}"#;

const COPY_VELOCITY_TO_POSITION: &str = r#"{
    "name": "CopyVel", "version": 1,
    "nodes": [
        { "id": "t", "op": "OnTick" },
        { "id": "gv", "op": "GetVelocity" },
        { "id": "tr", "op": "Translate",
          "in": { "delta": { "from": "gv", "scale": 0.1 } } }
    ],
    "edges": [{ "from": "t", "to": "tr" }]
}"#;

const GET_POS_INTO_VELOCITY: &str = r#"{
    "name": "PosToVel", "version": 1,
    "nodes": [
        { "id": "t", "op": "OnTick" },
        { "id": "gp", "op": "GetPosition" },
        { "id": "sv", "op": "SetVelocity",
          "in": { "vel": { "from": "gp" } } }
    ],
    "edges": [{ "from": "t", "to": "sv" }]
}"#;

#[test]
fn translate_moves_entity_exactly_n_units_in_n_frames() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn((Transform::default(),));
    system
        .attach_inline(entity, PathBuf::from("test://trans"), TRANSLATE_RIGHT)
        .unwrap();

    for _ in 0..10 {
        system.tick_pre_simulation(&mut world, 1.0 / 60.0);
    }

    let t = world.get::<&Transform>(entity).unwrap();
    assert!(
        (t.position.x - 10.0).abs() < 1e-5,
        "Translate(1,0) over 10 frames should yield x=10; got {}",
        t.position.x
    );
    assert!(t.position.y.abs() < 1e-5);
}

#[test]
fn translate_on_entity_without_transform_bumps_missing_counter() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn(()); // no Transform
    system
        .attach_inline(entity, PathBuf::from("test://trans"), TRANSLATE_RIGHT)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    assert_eq!(system.counters.missing_transform_writes, 1);
    assert!(world.get::<&Transform>(entity).is_err());
}

#[test]
fn apply_force_accumulates_into_force_component() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn((Force2D::default(),));
    system
        .attach_inline(entity, PathBuf::from("test://force"), APPLY_FORCE)
        .unwrap();

    for _ in 0..3 {
        system.tick_pre_simulation(&mut world, 1.0 / 60.0);
    }

    let f = world.get::<&Force2D>(entity).unwrap();
    assert!((f.x - 0.0).abs() < 1e-5);
    assert!((f.y - 15.0).abs() < 1e-5, "ApplyForce(0,5) × 3 → y=15; got {}", f.y);
}

#[test]
fn apply_force_on_entity_without_force_bumps_missing_counter() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn(()); // no Force2D
    system
        .attach_inline(entity, PathBuf::from("test://force"), APPLY_FORCE)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    assert_eq!(system.counters.missing_force_writes, 1);
}

#[test]
fn get_velocity_feeds_translate_for_kinematic_motion() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    // vy = 100 px/s; with scale=0.1 → translate delta = (0, 10) per frame.
    let entity = world.spawn((
        Transform::default(),
        Velocity2D { x: 0.0, y: 100.0, gravity: 0.0 },
    ));
    system
        .attach_inline(entity, PathBuf::from("test://copy"), COPY_VELOCITY_TO_POSITION)
        .unwrap();

    for _ in 0..5 {
        system.tick_pre_simulation(&mut world, 1.0 / 60.0);
    }

    let t = world.get::<&Transform>(entity).unwrap();
    assert!(
        (t.position.y - 50.0).abs() < 1e-4,
        "GetVelocity(y=100)*0.1 over 5 frames → y=50; got {}",
        t.position.y
    );
}

#[test]
fn get_position_writes_pin_then_set_velocity_consumes_it() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    // position = (3, 4) → after one tick velocity should equal (3, 4).
    let entity = world.spawn((
        Transform {
            position: Vec2::new(3.0, 4.0),
            ..Default::default()
        },
        Velocity2D::default(),
    ));
    system
        .attach_inline(entity, PathBuf::from("test://p2v"), GET_POS_INTO_VELOCITY)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let v = world.get::<&Velocity2D>(entity).unwrap();
    assert!((v.x - 3.0).abs() < 1e-5);
    assert!((v.y - 4.0).abs() < 1e-5);
}

#[test]
fn get_velocity_on_entity_without_velocity_bumps_missing_counter() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    // No Velocity2D, no Transform — GetVelocity reads ZERO; Translate fails.
    let entity = world.spawn(());
    system
        .attach_inline(entity, PathBuf::from("test://copy"), COPY_VELOCITY_TO_POSITION)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    assert_eq!(system.counters.missing_velocity_reads, 1);
}

// ── Slice 7.4C ── structural commands (SpawnEntity / Despawn / PlaySound) ──

const SPAWN_BULLET_PER_TICK: &str = r#"{
    "name": "Spawner", "version": 1,
    "nodes": [
        { "id": "t", "op": "OnTick" },
        { "id": "s", "op": "SpawnEntity",
          "params": { "prefab_id": "bullet" },
          "in": {
              "at": { "value": [10.0, 20.0] },
              "with_velocity": { "value": [100.0, 0.0] }
          } }
    ],
    "edges": [{ "from": "t", "to": "s" }]
}"#;

const DESPAWN_SELF: &str = r#"{
    "name": "Suicide", "version": 1,
    "nodes": [
        { "id": "t", "op": "OnTick" },
        { "id": "d", "op": "Despawn" }
    ],
    "edges": [{ "from": "t", "to": "d" }]
}"#;

const PLAY_BOOM: &str = r#"{
    "name": "Boom", "version": 1,
    "nodes": [
        { "id": "t", "op": "OnTick" },
        { "id": "p", "op": "PlaySound",
          "params": { "sound_id": "boom.wav" },
          "in": { "volume": { "value": 0.5 } } }
    ],
    "edges": [{ "from": "t", "to": "p" }]
}"#;

#[test]
fn spawn_entity_op_queues_command_without_immediate_world_change() {
    use jengine_core::behavior::BehaviorCommand;
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn(());
    system
        .attach_inline(entity, PathBuf::from("test://spawner"), SPAWN_BULLET_PER_TICK)
        .unwrap();

    let pre_count = world.iter().count();
    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    // Tick is deferred — world has not been mutated yet.
    assert_eq!(world.iter().count(), pre_count, "Spawn must defer to barrier");
    assert_eq!(system.pending_command_count(), 1);

    // Drain at the Phase 7 barrier.
    let counters = system.flush_commands(&mut world);
    assert_eq!(counters.spawned, 1);
    assert_eq!(world.iter().count(), pre_count + 1);

    // Verify the spawned entity carries the prefab id + position + velocity.
    let mut found = 0;
    for (_e, (name, t, v)) in world.query::<(&Name, &Transform, &Velocity2D)>().iter() {
        if name.0 == "bullet" {
            assert!((t.position.x - 10.0).abs() < 1e-5);
            assert!((t.position.y - 20.0).abs() < 1e-5);
            assert!((v.x - 100.0).abs() < 1e-5);
            found += 1;
        }
    }
    assert_eq!(found, 1, "expected one spawned bullet entity");

    // Confirm command was the right shape (after drain, queue is empty).
    assert_eq!(system.pending_command_count(), 0);
    let _ = BehaviorCommand::Despawn { entity }; // touch the type so the import is exercised
}

#[test]
fn despawn_op_removes_entity_only_at_barrier() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn((Transform::default(),));
    system
        .attach_inline(entity, PathBuf::from("test://kill"), DESPAWN_SELF)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);
    // Queued, not applied.
    assert!(world.contains(entity));
    assert_eq!(system.pending_command_count(), 1);

    let counters = system.flush_commands(&mut world);
    assert_eq!(counters.despawned, 1);
    assert!(!world.contains(entity));
}

#[test]
fn despawn_of_already_dead_entity_bumps_missing_target_counter() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn((Transform::default(),));
    system
        .attach_inline(entity, PathBuf::from("test://kill"), DESPAWN_SELF)
        .unwrap();

    // Queue two ticks worth of Despawn before flushing.
    system.tick_pre_simulation(&mut world, 1.0 / 60.0);
    system.tick_pre_simulation(&mut world, 1.0 / 60.0);
    assert_eq!(system.pending_command_count(), 2);

    let counters = system.flush_commands(&mut world);
    assert_eq!(counters.despawned, 1);
    assert_eq!(counters.missing_despawn_targets, 1);
}

#[test]
fn play_sound_op_collects_request_for_host_forwarding() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn(());
    system
        .attach_inline(entity, PathBuf::from("test://boom"), PLAY_BOOM)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);
    let counters = system.flush_commands(&mut world);
    assert_eq!(counters.play_sound, 1);
    assert_eq!(counters.play_sound_requests.len(), 1);
    let req = &counters.play_sound_requests[0];
    assert_eq!(req.sound_id, "boom.wav");
    assert!((req.volume - 0.5).abs() < 1e-6);
}

/// Spec §4.4 conflict-priority rule documented:
/// behavior `Despawn` (priority 1) drains FIRST, then a hypothetical
/// script `set_velocity` (priority 2) on the same entity arrives at
/// a now-dead handle — the script command is rejected by the world
/// (entity gone). The graph effectively "wins by ordering" because
/// the script runs against a despawned target.
///
/// We simulate priority-2 by issuing the world-write *after*
/// `flush_commands`. The expected resolution: entity is gone; the
/// post-barrier write fails (returns `Err`).
#[test]
fn graph_despawn_runs_before_script_write_in_same_frame() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn((Transform::default(), Velocity2D::default()));
    system
        .attach_inline(entity, PathBuf::from("test://kill"), DESPAWN_SELF)
        .unwrap();

    // Phase 2: graph runs, queues Despawn.
    system.tick_pre_simulation(&mut world, 1.0 / 60.0);
    // Phase 7a: behavior commands drain.
    system.flush_commands(&mut world);
    // Phase 7b: a hypothetical script writes velocity — the entity is
    // already gone, so the write fails. This documents the winning
    // contract: by-ordering, structural Despawn wins over a same-frame
    // script set_velocity.
    let post = world.query_one_mut::<&mut Velocity2D>(entity);
    assert!(post.is_err(), "script should observe entity already despawned");
}

// ── Slice 7.4D ── higher-level gameplay ops (LookAt / MoveToward) ──

const LOOK_AT_TARGET: &str = r#"{
    "name": "Looker", "version": 1,
    "nodes": [
        { "id": "t", "op": "OnTick" },
        { "id": "l", "op": "LookAt",
          "in": { "target_pos": { "value": [10.0, 10.0] } } }
    ],
    "edges": [{ "from": "t", "to": "l" }]
}"#;

const MOVE_TOWARD_TARGET: &str = r#"{
    "name": "Mover", "version": 1,
    "nodes": [
        { "id": "t", "op": "OnTick" },
        { "id": "m", "op": "MoveToward",
          "in": {
              "target_pos": { "value": [100.0, 0.0] },
              "speed": { "value": 50.0 }
          } }
    ],
    "edges": [{ "from": "t", "to": "m" }]
}"#;

#[test]
fn look_at_sets_rotation_to_atan2_of_target_minus_position() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    // Entity at origin, target at (10,10) → expected rotation = atan2(10,10) = π/4.
    let entity = world.spawn((Transform::default(),));
    system
        .attach_inline(entity, PathBuf::from("test://look"), LOOK_AT_TARGET)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let t = world.get::<&Transform>(entity).unwrap();
    let expected = (10.0_f32).atan2(10.0_f32);
    assert!(
        (t.rotation - expected).abs() < 1e-5,
        "rotation = {} expected {}",
        t.rotation,
        expected
    );
}

#[test]
fn look_at_leaves_rotation_unchanged_when_target_equals_position() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    // Spawn entity AT the target (10,10) with a pre-existing rotation.
    let entity = world.spawn((Transform {
        position: Vec2::new(10.0, 10.0),
        rotation: 1.234,
        scale: Vec2::ONE,
    },));
    system
        .attach_inline(entity, PathBuf::from("test://look"), LOOK_AT_TARGET)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let t = world.get::<&Transform>(entity).unwrap();
    assert!(
        (t.rotation - 1.234).abs() < 1e-6,
        "rotation should be unchanged when target == position; got {}",
        t.rotation
    );
}

#[test]
fn look_at_without_transform_bumps_both_counters() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn(()); // No Transform.
    system
        .attach_inline(entity, PathBuf::from("test://look"), LOOK_AT_TARGET)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    assert_eq!(system.counters.missing_transform_reads, 1);
    assert_eq!(system.counters.missing_transform_writes, 1);
}

#[test]
fn move_toward_sets_velocity_in_direction_of_target() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    // Entity at origin, target at (100,0), speed 50 → velocity = (50,0).
    let entity = world.spawn((Transform::default(), Velocity2D::default()));
    system
        .attach_inline(entity, PathBuf::from("test://move"), MOVE_TOWARD_TARGET)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let v = world.get::<&Velocity2D>(entity).unwrap();
    assert!((v.x - 50.0).abs() < 1e-5, "vx = {}", v.x);
    assert!(v.y.abs() < 1e-5, "vy = {}", v.y);
}

#[test]
fn move_toward_zeros_velocity_when_target_equals_position() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    // Place entity AT the target so delta == 0.
    let entity = world.spawn((
        Transform {
            position: Vec2::new(100.0, 0.0),
            rotation: 0.0,
            scale: Vec2::ONE,
        },
        Velocity2D { x: 99.0, y: 99.0, gravity: 0.0 },
    ));
    system
        .attach_inline(entity, PathBuf::from("test://move"), MOVE_TOWARD_TARGET)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let v = world.get::<&Velocity2D>(entity).unwrap();
    assert!(v.x.abs() < 1e-5 && v.y.abs() < 1e-5, "vel = ({}, {})", v.x, v.y);
}

#[test]
fn move_toward_normalizes_arbitrary_target_to_unit_speed() {
    // Target (3, 4): magnitude 5, normalized (0.6, 0.8). At speed 50 →
    // velocity = (30, 40).
    const GRAPH: &str = r#"{
        "name": "Mover", "version": 1,
        "nodes": [
            { "id": "t", "op": "OnTick" },
            { "id": "m", "op": "MoveToward",
              "in": {
                  "target_pos": { "value": [3.0, 4.0] },
                  "speed": { "value": 50.0 }
              } }
        ],
        "edges": [{ "from": "t", "to": "m" }]
    }"#;
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn((Transform::default(), Velocity2D::default()));
    system
        .attach_inline(entity, PathBuf::from("test://move"), GRAPH)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let v = world.get::<&Velocity2D>(entity).unwrap();
    assert!((v.x - 30.0).abs() < 1e-4, "vx = {}", v.x);
    assert!((v.y - 40.0).abs() < 1e-4, "vy = {}", v.y);
}

#[test]
fn move_toward_without_transform_bumps_missing_transform_read() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn((Velocity2D::default(),)); // No Transform.
    system
        .attach_inline(entity, PathBuf::from("test://move"), MOVE_TOWARD_TARGET)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    assert_eq!(system.counters.missing_transform_reads, 1);
    // Velocity write succeeds (component present).
    let v = world.get::<&Velocity2D>(entity).unwrap();
    // With pos defaulted to ZERO and target (100,0), direction is +X.
    assert!((v.x - 50.0).abs() < 1e-5);
}

#[test]
fn move_toward_without_velocity_bumps_missing_velocity_write() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let entity = world.spawn((Transform::default(),)); // No Velocity2D.
    system
        .attach_inline(entity, PathBuf::from("test://move"), MOVE_TOWARD_TARGET)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    assert_eq!(system.counters.missing_velocity_writes, 1);
    assert_eq!(system.counters.missing_transform_reads, 0);
}

// ── Slice 7.5 ── telemetry + budget ──

#[test]
fn commands_emitted_counter_increments_per_structural_op() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    // 2 spawners + 1 despawner + 1 boom = 4 commands per tick.
    let a = world.spawn(());
    let b = world.spawn(());
    let c = world.spawn((Transform::default(),));
    let d = world.spawn(());
    system
        .attach_inline(a, PathBuf::from("test://s1"), SPAWN_BULLET_PER_TICK)
        .unwrap();
    system
        .attach_inline(b, PathBuf::from("test://s2"), SPAWN_BULLET_PER_TICK)
        .unwrap();
    system
        .attach_inline(c, PathBuf::from("test://kill"), DESPAWN_SELF)
        .unwrap();
    system
        .attach_inline(d, PathBuf::from("test://boom"), PLAY_BOOM)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    assert_eq!(system.counters.commands_emitted, 4);
    assert_eq!(system.pending_command_count(), 4);
}

#[test]
fn entities_ticked_and_ops_executed_counters_match_workload() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    // Each PLAY_BOOM graph executes 2 ops on tick (OnTick trigger
    // node + PlaySound action). Spawn 5 instances.
    let entities: Vec<_> = (0..5)
        .map(|_| {
            let e = world.spawn(());
            system
                .attach_inline(e, PathBuf::from("test://boom"), PLAY_BOOM)
                .unwrap();
            e
        })
        .collect();
    let _ = entities;

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    assert_eq!(system.counters.instances_ticked, 5);
    // Trigger nodes (OnTick) are visited but produce no
    // OpInstruction; only PlaySound counts. So expected = 5.
    assert_eq!(system.counters.ops_executed, 5);
}

#[test]
fn tick_overbudget_records_micros_when_budget_is_zero() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    system.tick_budget_ms = 0.000_001; // ~1 ns — guaranteed to blow.

    // Spawn enough graphs that the OnTick pass takes > 1µs even on
    // very fast hosts — the budget is effectively 0µs, so any
    // non-zero elapsed registers as over-budget.
    for _ in 0..256 {
        let e = world.spawn(());
        system
            .attach_inline(e, PathBuf::from("test://boom"), PLAY_BOOM)
            .unwrap();
    }

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    assert!(
        system.counters.tick_overbudget_us > 0,
        "expected non-zero tick_overbudget_us, got {}",
        system.counters.tick_overbudget_us
    );
}

#[test]
fn tick_within_budget_leaves_overbudget_zero() {
    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    system.tick_budget_ms = 1_000.0; // 1 second — definitely fits.

    let e = world.spawn(());
    system
        .attach_inline(e, PathBuf::from("test://boom"), PLAY_BOOM)
        .unwrap();

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    assert_eq!(system.counters.tick_overbudget_us, 0);
}

#[test]
fn snapshot_into_frame_counters_folds_telemetry() {
    use jengine_core::profiling::FrameCounters;

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    system.tick_budget_ms = 0.000_001;

    // Spawn enough graphs to reliably blow the (effectively zero)
    // budget, plus one despawn graph so commands_emitted is
    // exercised on at least one instance.
    let killer = world.spawn((Transform::default(),));
    system
        .attach_inline(killer, PathBuf::from("test://kill"), DESPAWN_SELF)
        .unwrap();
    for _ in 0..256 {
        let e = world.spawn(());
        system
            .attach_inline(e, PathBuf::from("test://boom"), PLAY_BOOM)
            .unwrap();
    }

    system.tick_pre_simulation(&mut world, 1.0 / 60.0);

    let mut fc = FrameCounters::default();
    system.snapshot_into_frame_counters(&mut fc);

    assert_eq!(fc.behavior_entities_ticked, 257);
    assert!(fc.behavior_ops_executed >= 257);
    // 256 PlaySound + 1 Despawn.
    assert_eq!(fc.behavior_commands_emitted, 257);
    assert!(fc.behavior_overbudget_ms > 0.0);
    assert_eq!(fc.behavior_critical_overbudget_ms, 0.0);
}

// ── Slice 7.6c — host helpers for trigger + collision dispatch ──

#[test]
fn dispatch_trigger_overlaps_from_world_routes_entered_to_on_trigger() {
    use jengine_core::ecs::components::TriggerOverlaps;

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let trigger = world.spawn((Velocity2D::default(),));
    let other = world.spawn(());

    // OnTrigger → AddVelocity, so we can observe the dispatch.
    system
        .attach_inline(
            trigger,
            PathBuf::from("test://trig"),
            r#"{
                "name": "TrigVel", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTrigger" },
                    { "id": "a", "op": "AddVelocity",
                      "in": { "delta": { "value": [3.0, 0.0] } } }
                ],
                "edges": [{ "from": "t", "to": "a" }]
            }"#,
        )
        .unwrap();

    // Two distinct entered entities + one no-op trigger entity with
    // an empty entered list.
    let third = world.spawn(());
    let no_op_trigger = world.spawn(());
    world
        .insert_one(
            trigger,
            TriggerOverlaps {
                entered: vec![other.to_bits().get(), third.to_bits().get()],
                staying: vec![],
                exited: vec![],
            },
        )
        .unwrap();
    world
        .insert_one(
            no_op_trigger,
            TriggerOverlaps {
                entered: vec![],
                staying: vec![],
                exited: vec![],
            },
        )
        .unwrap();

    let dispatched = system.dispatch_trigger_overlaps_from_world(&mut world);
    assert_eq!(dispatched, 2, "two entered entries on the subscribed trigger");

    let vel = world.get::<&Velocity2D>(trigger).unwrap();
    assert!((vel.x - 6.0).abs() < 1e-6, "AddVelocity ran twice (vx = {})", vel.x);
}

#[test]
fn dispatch_trigger_overlaps_skips_zero_bits_entries() {
    use jengine_core::ecs::components::TriggerOverlaps;

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let trigger = world.spawn((Velocity2D::default(),));
    system
        .attach_inline(
            trigger,
            PathBuf::from("test://trig0"),
            r#"{
                "name": "TrigVel", "version": 1,
                "nodes": [
                    { "id": "t", "op": "OnTrigger" },
                    { "id": "a", "op": "AddVelocity",
                      "in": { "delta": { "value": [1.0, 0.0] } } }
                ],
                "edges": [{ "from": "t", "to": "a" }]
            }"#,
        )
        .unwrap();

    world
        .insert_one(
            trigger,
            TriggerOverlaps {
                entered: vec![0],
                staying: vec![],
                exited: vec![],
            },
        )
        .unwrap();

    let dispatched = system.dispatch_trigger_overlaps_from_world(&mut world);
    assert_eq!(dispatched, 0);
    let vel = world.get::<&Velocity2D>(trigger).unwrap();
    assert_eq!(vel.x, 0.0);
}

#[test]
fn dispatch_contact_events_routes_to_both_sides() {
    use jengine_core::physics::{ContactEvent, ContactType};

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    // Both entities subscribe; AddVelocity {5,0} on each side; one
    // Started event should fire the program twice (a→b + b→a).
    let a = world.spawn((Velocity2D::default(),));
    let b = world.spawn((Velocity2D::default(),));
    for e in [a, b] {
        system
            .attach_inline(e, PathBuf::from("test://col-pair"), ON_COLLISION_ADDS_VELOCITY)
            .unwrap();
    }

    let events = vec![ContactEvent::simple(a.to_bits().get(), b.to_bits().get(), ContactType::Started)];

    let dispatched = system.dispatch_contact_events(&events, &mut world);
    assert_eq!(dispatched, 2, "one event → two dispatches");

    assert!((world.get::<&Velocity2D>(a).unwrap().x - 5.0).abs() < 1e-6);
    assert!((world.get::<&Velocity2D>(b).unwrap().x - 5.0).abs() < 1e-6);
}

#[test]
fn dispatch_contact_events_unsubscribed_side_is_a_noop() {
    use jengine_core::physics::{ContactEvent, ContactType};

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let subscribed = world.spawn((Velocity2D::default(),));
    let plain = world.spawn(());
    system
        .attach_inline(
            subscribed,
            PathBuf::from("test://col-one"),
            ON_COLLISION_ADDS_VELOCITY,
        )
        .unwrap();

    let events = vec![ContactEvent::simple(subscribed.to_bits().get(), plain.to_bits().get(), ContactType::Started)];

    let dispatched = system.dispatch_contact_events(&events, &mut world);
    // Both sides are visited; only the subscribed side actually runs.
    assert_eq!(dispatched, 2);
    assert!((world.get::<&Velocity2D>(subscribed).unwrap().x - 5.0).abs() < 1e-6);
}

/// Contacts as `PhysicsWorld` actually reports them: the bare `Entity::id()`.
///
/// Every other test here built events with `to_bits()`, which the physics
/// layer never produces - so they passed while every real contact was dropped
/// by `Entity::from_bits` and `OnCollision` never fired anywhere (BUGS #92).
#[test]
fn dispatch_contact_events_accepts_the_physics_id_encoding() {
    use jengine_core::physics::{ContactEvent, ContactType};

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();
    let a = world.spawn((Velocity2D::default(),));
    let b = world.spawn((Velocity2D::default(),));
    system
        .attach_inline(a, PathBuf::from("test://col-id"), ON_COLLISION_ADDS_VELOCITY)
        .unwrap();

    let events = vec![ContactEvent::simple(a.id() as u64, b.id() as u64, ContactType::Started)];
    assert_eq!(system.dispatch_contact_events(&events, &mut world), 2);
    assert!((world.get::<&Velocity2D>(a).unwrap().x - 5.0).abs() < 1e-6);
}

/// A contact naming an entity that is not in the world is skipped.
///
/// This used to send `0` as the "no entity" case. It is not one: `0` is the
/// index of the first entity spawned, and `PhysicsWorld` keys bodies by index.
#[test]
fn dispatch_contact_events_skips_unknown_entities() {
    use jengine_core::physics::{ContactEvent, ContactType};

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let a = world.spawn((Velocity2D::default(),));
    system
        .attach_inline(a, PathBuf::from("test://col-skip"), ON_COLLISION_ADDS_VELOCITY)
        .unwrap();

    let events = vec![ContactEvent::simple(a.to_bits().get(), 999_999, ContactType::Started)];

    let dispatched = system.dispatch_contact_events(&events, &mut world);
    assert_eq!(dispatched, 0);
    assert_eq!(world.get::<&Velocity2D>(a).unwrap().x, 0.0);
}

// ── OnKey trigger (Slice 7.7) ──────────────────────────────────────

const ON_KEY_PRESSED_SETS_VELOCITY: &str = r#"{
    "name": "JumpOnSpace", "version": 1,
    "nodes": [
        { "id": "k", "op": "OnKey",
          "params": { "key": "Space", "edge": "pressed" } },
        { "id": "v", "op": "SetVelocity",
          "in": { "vel": { "value": [0.0, 500.0] } } }
    ],
    "edges": [{ "from": "k", "to": "v" }]
}"#;

const ON_KEY_RELEASED_ESC: &str = r#"{
    "name": "ReleasedEsc", "version": 1,
    "nodes": [
        { "id": "k", "op": "OnKey",
          "params": { "key": "Escape", "edge": "released" } },
        { "id": "v", "op": "SetVelocity",
          "in": { "vel": { "value": [9.0, 0.0] } } }
    ],
    "edges": [{ "from": "k", "to": "v" }]
}"#;

const ON_KEY_HELD_D: &str = r#"{
    "name": "HeldD", "version": 1,
    "nodes": [
        { "id": "k", "op": "OnKey",
          "params": { "key": "D", "edge": "held" } },
        { "id": "v", "op": "AddVelocity",
          "in": { "delta": { "value": [10.0, 0.0] } } }
    ],
    "edges": [{ "from": "k", "to": "v" }]
}"#;

#[test]
fn on_key_pressed_fires_only_on_just_pressed_edge() {
    use jengine_core::input::{InputState, KeyCode};

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let entity = world.spawn((Velocity2D::default(),));
    system
        .attach_inline(entity, PathBuf::from("test://onkey"), ON_KEY_PRESSED_SETS_VELOCITY)
        .unwrap();

    let mut input = InputState::new();
    // No keys pressed → no firing.
    system.dispatch_input_keys(&input, &mut world);
    assert_eq!(world.get::<&Velocity2D>(entity).unwrap().y, 0.0);

    // Press space → just_pressed fires.
    input.key_down(KeyCode::Space);
    system.dispatch_input_keys(&input, &mut world);
    assert!((world.get::<&Velocity2D>(entity).unwrap().y - 500.0).abs() < 1e-3);

    // Reset velocity to detect re-fire.
    {
        let mut v = world.get::<&mut Velocity2D>(entity).unwrap();
        v.y = 0.0;
    }

    // Next frame: still pressed but not "just_pressed" → no fire.
    input.begin_frame();
    assert!(input.is_pressed(KeyCode::Space));
    assert!(!input.is_just_pressed(KeyCode::Space));
    system.dispatch_input_keys(&input, &mut world);
    assert_eq!(world.get::<&Velocity2D>(entity).unwrap().y, 0.0);
}

#[test]
fn on_key_released_fires_on_just_released() {
    use jengine_core::input::{InputState, KeyCode};

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let entity = world.spawn((Velocity2D::default(),));
    system
        .attach_inline(entity, PathBuf::from("test://onkeyrel"), ON_KEY_RELEASED_ESC)
        .unwrap();

    let mut input = InputState::new();
    input.key_down(KeyCode::Escape);
    system.dispatch_input_keys(&input, &mut world);
    assert_eq!(world.get::<&Velocity2D>(entity).unwrap().x, 0.0);

    input.begin_frame();
    input.key_up(KeyCode::Escape);
    system.dispatch_input_keys(&input, &mut world);
    assert!((world.get::<&Velocity2D>(entity).unwrap().x - 9.0).abs() < 1e-3);
}

#[test]
fn on_key_held_fires_every_frame_while_pressed() {
    use jengine_core::input::{InputState, KeyCode};

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    let entity = world.spawn((Velocity2D::default(),));
    system
        .attach_inline(entity, PathBuf::from("test://onkeyheld"), ON_KEY_HELD_D)
        .unwrap();

    let mut input = InputState::new();
    input.key_down(KeyCode::D);
    // Held fires the frame the key first goes down, and every frame after.
    system.dispatch_input_keys(&input, &mut world);
    system.dispatch_input_keys(&input, &mut world);
    input.begin_frame();
    system.dispatch_input_keys(&input, &mut world);
    assert!((world.get::<&Velocity2D>(entity).unwrap().x - 30.0).abs() < 1e-3);
}

#[test]
fn on_key_compile_rejects_unknown_key_name() {
    use jengine_core::behavior::CompileError;

    let mut system = BehaviorGraphSystem::new();
    let mut world = World::new();
    let entity = world.spawn(());
    let bad = r#"{
        "name": "Bad", "version": 1,
        "nodes": [
            { "id": "k", "op": "OnKey",
              "params": { "key": "NotARealKey", "edge": "pressed" } }
        ],
        "edges": []
    }"#;
    let err = system
        .attach_inline(entity, PathBuf::from("test://badkey"), bad)
        .expect_err("should fail compile");
    assert!(
        matches!(
            err,
            jengine_core::behavior::BehaviorCacheError::Compile(
                CompileError::InvalidKeyTriggerParam { param: "key", .. }
            )
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn on_key_compile_rejects_duplicate_on_key_nodes() {
    use jengine_core::behavior::CompileError;

    let mut system = BehaviorGraphSystem::new();
    let mut world = World::new();
    let entity = world.spawn(());
    let dupe = r#"{
        "name": "Dupe", "version": 1,
        "nodes": [
            { "id": "k1", "op": "OnKey",
              "params": { "key": "Space", "edge": "pressed" } },
            { "id": "k2", "op": "OnKey",
              "params": { "key": "Escape", "edge": "pressed" } }
        ],
        "edges": []
    }"#;
    let err = system
        .attach_inline(entity, PathBuf::from("test://dupekey"), dupe)
        .expect_err("should fail compile");
    assert!(
        matches!(
            err,
            jengine_core::behavior::BehaviorCacheError::Compile(
                CompileError::DuplicateOnKeyTrigger { .. }
            )
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn dispatch_input_keys_skips_graphs_not_subscribed() {
    use jengine_core::input::{InputState, KeyCode};

    let mut world = World::new();
    let mut system = BehaviorGraphSystem::new();

    // Graph has OnSpawn, not OnKey — must not be touched by the
    // OnKey dispatcher (zero-cost-for-non-listeners contract).
    let entity = world.spawn((Velocity2D::default(),));
    system
        .attach_inline(entity, PathBuf::from("test://spawnonly"), ON_SPAWN_SETS_VELOCITY)
        .unwrap();

    let mut input = InputState::new();
    input.key_down(KeyCode::Space);
    system.dispatch_input_keys(&input, &mut world);
    assert_eq!(world.get::<&Velocity2D>(entity).unwrap().y, 0.0);
}



/// A scene with no graphs is not timed at all. It used to be: the empty
/// loop was measured, and an OS stall during it was logged as "OnTick over
/// budget (0 entities, 0 ops)" in scenes that had no graphs.
#[test]
fn a_world_without_graphs_is_never_over_budget() {
    let mut world = World::new();
    world.spawn((Transform::default(),));
    let mut sys = BehaviorGraphSystem::default();
    sys.tick_budget_ms = 1e-9; // any measured time at all would be over
    for _ in 0..10 {
        sys.tick_pre_simulation(&mut world, 1.0 / 60.0);
        assert_eq!(sys.counters.tick_time_us, 0, "nothing ran, so nothing was timed");
        assert_eq!(sys.counters.tick_overbudget_us, 0);
    }
}
