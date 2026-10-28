//! Slice 7.4C — Structural command queue for behavior graphs.
//!
//! `SpawnEntity`, `Despawn`, and `PlaySound` ops do **not** mutate
//! the world directly. Instead they push a [`BehaviorCommand`] onto
//! a queue owned by the [`super::system::BehaviorGraphSystem`]. The
//! queue is drained once per frame at the **command barrier**
//! (spec §4.4 phase 7), interleaved with the script bridge command
//! queue.
//!
//! Conflict-priority rule (spec §4.4): structural commands carry an
//! implicit priority — `priority 1: behavior graph`. The host's
//! barrier driver merges these with `priority 2: script` commands;
//! when a script command targets the same entity that a graph
//! command would create / destroy in the same frame, the script wins.
//! In v1 the merge is documented but minimal: behavior commands
//! drain in the order pushed; script commands drain afterwards in a
//! separate pass and naturally override.
//!
//! # What the barrier applies and what it hands back
//!
//! Everything that needs only the world is applied here, once, for every
//! runtime: despawns, animation play/stop, and - given a [`PrefabSource`] -
//! spawning the named prefab. Sound and scene changes need the runtime's
//! audio backend and scene loader, so they come back as
//! [`CommandRouteCounters::play_sound_requests`] and
//! [`CommandRouteCounters::change_scene_requests`], and each runtime
//! forwards them through the path its own `play_sound` / `load_scene` use.
//!
//! Until Sep 2026 none of that happened. `SpawnEntity` made an entity with
//! nothing but a `Name` and a `Transform`; `PlaySound`, `ChangeScene`,
//! `PlayAnimation` and `StopAnimation` were recorded for "the host to
//! forward", and no host did - in any runtime (test-lab BUGS #93).

use std::collections::HashMap;

use glam::Vec2;
use hecs::World;

use crate::ecs::components::{CustomComponents, Name, Transform, Velocity2D};
use crate::package::component_registry::ComponentRegistry;
use crate::scene::format::EntityData;

/// What a runtime lends the command barrier so a graph's `SpawnEntity`
/// builds the prefab it names: the runtime's component registry (so a
/// package's components come along) and its loaded prefabs.
pub struct PrefabSource<'a> {
    pub registry: &'a ComponentRegistry,
    pub prefabs: &'a HashMap<String, EntityData>,
}

/// Spawn one entity from prefab data through `registry`, the way a scene
/// load does. Components the registry does not know land in
/// `CustomComponents`, as they do for a script's `spawn_prefab`.
pub fn spawn_entity_data(
    world: &mut World,
    registry: &ComponentRegistry,
    data: &EntityData,
) -> hecs::Entity {
    let mut builder = hecs::EntityBuilder::new();
    builder.add(Name(data.name.clone()));
    let unknown = data.components.add_to_builder(&mut builder, registry);
    if !unknown.is_empty() {
        builder.add(CustomComponents(unknown));
    }
    world.spawn(builder.build())
}

/// Structural command emitted by behavior graphs in Phase 2 / 6 and
/// drained at the Phase 7 command barrier.
#[derive(Debug, Clone, PartialEq)]
pub enum BehaviorCommand {
    /// Spawn a new entity. `prefab_id` is opaque in v1 — stored as a
    /// `Name` component so callers can resolve it. With_velocity
    /// inserts a `Velocity2D` component when supplied.
    Spawn {
        prefab_id: String,
        at: Vec2,
        rotation: f32,
        scale: Vec2,
        with_velocity: Option<Vec2>,
    },
    /// Despawn `entity`. No-op (counter bump) if the entity is no
    /// longer alive (already despawned by a higher-priority script
    /// command earlier this frame, etc.).
    Despawn { entity: hecs::Entity },
    /// Despawn every entity whose `Tags` component contains `tag`.
    /// Resolved at the command barrier (Phase 7) so callers see a
    /// consistent snapshot. Counter bumps `despawned_by_tag`.
    DespawnByTag { tag: String },
    /// Surface a request to change the active scene. v1 records the
    /// `scene_id` for the host (runtime / editor) to act on; the
    /// engine itself does not load scenes from inside command routing.
    ChangeScene { scene_id: String },
    /// Surface a sound-play request to the host. Volume is in `[0, 1]`
    /// nominally; the audio backend clamps as needed.
    PlaySound { sound_id: String, volume: f32 },
    /// Surface an animation-play request for `entity`. The host
    /// (runtime) is expected to forward to `SpriteAnimator` /
    /// `AnimStateMachine`. v1 simply records the request.
    PlayAnimation { entity: hecs::Entity, anim_id: String },
    /// Surface a request to halt any playing animation on `entity`.
    StopAnimation { entity: hecs::Entity },
    /// A named, optionally numeric event. Delivered to every graph's
    /// matching `OnEvent` at the barrier, **and to scripts**: each runtime
    /// hands [`EmitEventOutcome::script_payload`] to its `on_<name>`
    /// listeners. That second half is Sep 2026 - before it, this comment
    /// said "routing wiring is host-specific" and no host had written any,
    /// so a graph could talk to other graphs and never to the game.
    ///
    /// `at` is where the emitting entity was, read when the op ran - which
    /// matters most for the event that is emitted most, a death, where the
    /// entity is gone by the time anything reads the event.
    EmitEvent { name: String, value: Option<f32>, at: Option<glam::Vec2> },
}

/// Per-drain telemetry. Stored on the system + surfaced for tests.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CommandRouteCounters {
    pub spawned: u32,
    pub despawned: u32,
    pub despawned_by_tag: u32,
    pub change_scene: u32,
    pub play_sound: u32,
    pub play_animation: u32,
    pub stop_animation: u32,
    pub emit_event: u32,
    /// Despawn issued for an entity that wasn't alive (already gone).
    pub missing_despawn_targets: u32,
    /// `SpawnEntity` named a prefab the runtime has not loaded. The entity
    /// is still spawned, bare, so the graph's position logic stays visible.
    pub missing_prefabs: u32,
    /// Sound-play requests. **The runtime must forward these** to its audio
    /// backend - core has none.
    pub play_sound_requests: Vec<PlaySoundOutcome>,
    /// Animation requests, already applied to the entity's `SpriteAnimator`
    /// by the barrier. Kept for telemetry and tests.
    pub play_animation_requests: Vec<PlayAnimationOutcome>,
    /// Stop-animation requests, already applied. Kept for telemetry.
    pub stop_animation_requests: Vec<hecs::Entity>,
    /// Named events collected for the host to route.
    pub emit_event_requests: Vec<EmitEventOutcome>,
    /// Scene-change requests. **The runtime must act on these** through
    /// the same path as a script's `load_scene`.
    pub change_scene_requests: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaySoundOutcome {
    pub sound_id: String,
    pub volume: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayAnimationOutcome {
    pub entity: hecs::Entity,
    pub anim_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmitEventOutcome {
    pub name: String,
    pub value: Option<f32>,
    /// Where the emitting entity was when the op ran.
    pub at: Option<glam::Vec2>,
}

impl EmitEventOutcome {
    /// What a script's `on_<name>(entity, data)` receives for a graph's
    /// event: `{ value, x, y }`, each present only when known.
    ///
    /// **One definition for every runtime.** Native builds a Lua table from
    /// it and the browser hosts pass it as JSON, so a script reads the same
    /// three fields wherever it runs. A table rather than the bare number
    /// because the number alone loses the position, and the position is the
    /// thing a death event is for - a score popup, a burst, a drop.
    pub fn script_payload(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        if let Some(v) = self.value {
            m.insert("value".into(), serde_json::json!(v));
        }
        if let Some(at) = self.at {
            m.insert("x".into(), serde_json::json!(at.x));
            m.insert("y".into(), serde_json::json!(at.y));
        }
        serde_json::Value::Object(m)
    }
}

/// The number a graph's `OnEvent` sees for a **script's** `send_event`.
///
/// `OnEvent` has one payload pin and it is a float, while a script's `data`
/// can be anything. The rule, decided here once so the three runtimes agree:
/// a number is itself, a string that parses as one is that number (the
/// scripting docs' own example sends `"42"`), a table with a numeric `value`
/// is that - which is what [`EmitEventOutcome::script_payload`] produces, so
/// a script can forward a graph's event back unchanged - and anything else
/// is `0`.
pub fn graph_value_from_script_data(data: Option<&serde_json::Value>) -> f32 {
    match data {
        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0) as f32,
        Some(serde_json::Value::String(s)) => s.trim().parse::<f32>().unwrap_or(0.0),
        Some(serde_json::Value::Object(m)) => {
            m.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32
        }
        Some(serde_json::Value::Bool(true)) => 1.0,
        _ => 0.0,
    }
}

/// Drain `commands` into `world` in order, with no prefabs available: a
/// `SpawnEntity` makes a bare entity named after the prefab. Runtimes use
/// [`route_behavior_commands_with`].
pub fn route_behavior_commands(
    commands: Vec<BehaviorCommand>,
    world: &mut World,
) -> CommandRouteCounters {
    route_behavior_commands_with(commands, world, None)
}

/// Drain `commands` into `world` in order. See the module docs for what is
/// applied here and what the caller must forward.
pub fn route_behavior_commands_with(
    commands: Vec<BehaviorCommand>,
    world: &mut World,
    prefabs: Option<&PrefabSource<'_>>,
) -> CommandRouteCounters {
    let mut c = CommandRouteCounters::default();
    for cmd in commands {
        match cmd {
            BehaviorCommand::Spawn {
                prefab_id,
                at,
                rotation,
                scale,
                with_velocity,
            } => {
                let found = prefabs.and_then(|p| p.prefabs.get(&prefab_id).map(|d| (p, d)));
                let entity = match found {
                    Some((p, data)) => {
                        let e = spawn_entity_data(world, p.registry, data);
                        // A prefab's parts, under it (`EntityData::children`).
                        crate::scene::prefab::spawn_children_with_registry(world, p.registry, e, &data.children);
                        e
                    }
                    None => {
                        if prefabs.is_some() {
                            c.missing_prefabs += 1;
                            log::warn!("[behavior] SpawnEntity: prefab '{prefab_id}' is not loaded");
                        }
                        world.spawn((Name(prefab_id),))
                    }
                };
                // The graph decides where; the prefab's own scale is kept and
                // multiplied, so a default graph scale of 1 changes nothing.
                let placed = match world.get::<&mut Transform>(entity) {
                    Ok(mut t) => {
                        t.position = at;
                        t.rotation = rotation;
                        t.scale *= scale;
                        true
                    }
                    Err(_) => false,
                };
                if !placed {
                    let _ = world.insert_one(entity, Transform { position: at, rotation, scale });
                }
                if let Some(v) = with_velocity {
                    let gravity = world.get::<&Velocity2D>(entity).map(|v| v.gravity).unwrap_or(0.0);
                    let _ = world.insert_one(entity, Velocity2D { x: v.x, y: v.y, gravity });
                }
                c.spawned += 1;
            }
            BehaviorCommand::Despawn { entity } => match world.despawn(entity) {
                Ok(()) => c.despawned += 1,
                Err(_) => c.missing_despawn_targets += 1,
            },
            BehaviorCommand::DespawnByTag { tag } => {
                let targets: Vec<hecs::Entity> = world
                    .query::<&crate::ecs::components::Tags>()
                    .iter()
                    .filter_map(|(e, tags)| if tags.0.contains(&tag) { Some(e) } else { None })
                    .collect();
                for e in targets {
                    if world.despawn(e).is_ok() {
                        c.despawned_by_tag += 1;
                    }
                }
            }
            BehaviorCommand::ChangeScene { scene_id } => {
                c.change_scene += 1;
                c.change_scene_requests.push(scene_id);
            }
            BehaviorCommand::PlaySound { sound_id, volume } => {
                c.play_sound += 1;
                c.play_sound_requests.push(PlaySoundOutcome { sound_id, volume });
            }
            BehaviorCommand::PlayAnimation { entity, anim_id } => {
                // What a script's `play_animation` does (`"pa"` in
                // `scripting::host_commands`).
                if let Ok(mut a) = world.get::<&mut crate::animation::SpriteAnimator>(entity) {
                    a.play(&anim_id);
                }
                c.play_animation += 1;
                c.play_animation_requests
                    .push(PlayAnimationOutcome { entity, anim_id });
            }
            BehaviorCommand::StopAnimation { entity } => {
                if let Ok(mut a) = world.get::<&mut crate::animation::SpriteAnimator>(entity) {
                    a.stop();
                }
                c.stop_animation += 1;
                c.stop_animation_requests.push(entity);
            }
            BehaviorCommand::EmitEvent { name, value, at } => {
                c.emit_event += 1;
                c.emit_event_requests.push(EmitEventOutcome { name, value, at });
            }
        }
    }
    c
}
