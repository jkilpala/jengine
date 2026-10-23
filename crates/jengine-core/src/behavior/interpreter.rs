//! Per-instance, per-event interpreter. Owns no buffers — the caller
//! (the `BehaviorGraphSystem`) provides scratch via `BehaviorExecContext`.
//!
//! Slice 7.3: dispatches one [`EventProgram`] per [`TriggerKind`] via
//! [`tick_event`]. Payload pins (entity / bool / float) are written
//! before stepping the program so future ops can read them.
//!
//! See [`docs/step7-behavior-graph-spec.md`](../../../../docs/step7-behavior-graph-spec.md) §4.6.

use glam::Vec2;
use hecs::World;

use crate::behavior::command::BehaviorCommand;
use crate::behavior::compile::{
    BoolInput, BoundsLiteral, CompiledGraph, FloatInput, OpInstruction, Vec2Input,
};
use crate::behavior::event::{EventPayload, TriggerKind};
use crate::behavior::pin::PinFrame;
use crate::behavior::state::GraphInstanceState;
use crate::ecs::components::{
    BounceArea, CustomComponents, ExpireAction, Force2D, InputAxis2D, Lifetime, SpriteComponent,
    Tags, Transform, Velocity2D,
};

/// Caller-owned scratch + telemetry for a single graph instance tick.
pub struct BehaviorExecContext<'a> {
    pub dt: f32,
    pub pins: &'a mut PinFrame,
    pub counters: &'a mut BehaviorCounters,
    /// Structural commands (`SpawnEntity` / `Despawn` / `PlaySound`)
    /// pushed by ops during this tick. Drained at the Phase 7 command
    /// barrier (spec §4.4); never applied during the tick itself.
    pub commands: &'a mut Vec<BehaviorCommand>,
}

/// Per-frame telemetry. Surfaced via the system + (later) FrameCounters.
#[derive(Debug, Default, Clone, Copy)]
pub struct BehaviorCounters {
    pub instances_ticked: u32,
    pub ops_executed: u32,
    pub events_dispatched: u32,
    /// Commands pushed onto the system command queue this frame
    /// (`SpawnEntity` / `Despawn` / `PlaySound`). Each push counts
    /// once — drains at the Phase 7 barrier are bookkeeping, not
    /// emissions. Surfaced as `FrameCounters.behavior_commands_emitted`.
    pub commands_emitted: u32,
    /// Wall-clock micros spent inside `tick_pre_simulation` (Phase 2)
    /// this frame. Reset at the start of every `tick_pre_simulation`.
    pub tick_time_us: u64,
    /// Wall-clock micros spent inside event dispatch (Phase 6) this
    /// frame. Accumulated across `dispatch_*` calls; reset alongside
    /// `tick_time_us` at the start of `tick_pre_simulation`.
    pub event_time_us: u64,
    /// Microseconds the OnTick phase exceeded `tick_budget_ms` by.
    /// `0` when within budget. Surfaced as
    /// `FrameCounters.behavior_overbudget_ms` (converted to ms).
    pub tick_overbudget_us: u64,
    /// Microseconds the event-dispatch phase exceeded
    /// `event_budget_ms` by. `0` when within budget. Surfaced as
    /// `FrameCounters.behavior_critical_overbudget_ms` (ms).
    pub event_overbudget_us: u64,
    pub missing_axis_reads: u32,
    pub missing_velocity_writes: u32,
    pub missing_velocity_reads: u32,
    pub missing_transform_reads: u32,
    pub missing_transform_writes: u32,
    pub missing_force_writes: u32,
    pub missing_sprite_writes: u32,
    pub missing_tag_writes: u32,
    pub missing_custom_writes: u32,
    pub missing_custom_reads: u32,
}

/// Run one OnTick pass for one instance of `graph`. Thin wrapper over
/// [`tick_event`] kept for callers that only care about the tick path.
pub fn tick_graph(
    graph: &CompiledGraph,
    state: &mut GraphInstanceState,
    world: &mut World,
    ctx: &mut BehaviorExecContext<'_>,
) {
    let dt = ctx.dt;
    tick_event(
        graph,
        state,
        world,
        ctx,
        TriggerKind::OnTick,
        &EventPayload::Tick { dt },
    );
}

/// Dispatch one event into one instance of `graph`. No-op if the
/// graph isn't subscribed to `kind`.
pub fn tick_event(
    graph: &CompiledGraph,
    state: &mut GraphInstanceState,
    world: &mut World,
    ctx: &mut BehaviorExecContext<'_>,
    kind: TriggerKind,
    payload: &EventPayload,
) {
    let Some(program) = graph.programs.get(&kind) else {
        return;
    };
    ctx.pins.reset(&graph.pin_layout);

    // Payload → pins. Tick / Lifecycle have no payload pins.
    match payload {
        EventPayload::Tick { .. } | EventPayload::Lifecycle => {}
        EventPayload::Collision { other, enter } => {
            if let Some(slot) = program.payload_pins.other_entity {
                ctx.pins.write_entity(slot, *other);
            }
            if let Some(slot) = program.payload_pins.enter {
                ctx.pins.write_bool(slot, *enter);
            }
        }
        EventPayload::TriggerOverlap { other } => {
            if let Some(slot) = program.payload_pins.other_entity {
                ctx.pins.write_entity(slot, *other);
            }
        }
        EventPayload::Timer { tick_index } => {
            if let Some(slot) = program.payload_pins.tick_index {
                ctx.pins.write_float(slot, *tick_index);
            }
        }
        // OnKey: no payload pins. The (key, edge) filter is baked
        // into the program at compile time and the dispatcher only
        // fires on a match, so handlers don't need to inspect the
        // key from inside the program.
        EventPayload::Key { .. } => {}
        EventPayload::Event { value } => {
            if let Some(slot) = program.payload_pins.event_value {
                ctx.pins.write_float(slot, *value);
            }
        }
        EventPayload::MouseClick { pos, button } => {
            if let Some(slot) = program.payload_pins.mouse_pos {
                ctx.pins.write_vec2(slot, *pos);
            }
            if let Some(slot) = program.payload_pins.mouse_button {
                ctx.pins.write_float(slot, *button as f32);
            }
        }
    }

    ctx.counters.events_dispatched += 1;
    if matches!(kind, TriggerKind::OnTick) {
        ctx.counters.instances_ticked += 1;
    }
    let entity = state.entity;
    run_block(&program.ops, entity, world, state, ctx);
}

/// Recursively execute a flat slice of ops. Block ops (`If`,
/// `Cooldown`, `Once`) are inlined as `[header, body...]` (or
/// `[header, then..., else...]` for `If`); on a header, we either
/// recurse into the appropriate sub-slice or skip past it.
fn run_block(
    ops: &[OpInstruction],
    entity: hecs::Entity,
    world: &mut World,
    state: &mut GraphInstanceState,
    ctx: &mut BehaviorExecContext<'_>,
) {
    let mut pc = 0usize;
    while pc < ops.len() {
        ctx.counters.ops_executed += 1;
        match &ops[pc] {
            OpInstruction::If { cond, then_len, else_len } => {
                let tl = *then_len as usize;
                let el = *else_len as usize;
                let then_start = pc + 1;
                let else_start = then_start + tl;
                let after = else_start + el;
                if read_bool(cond, ctx.pins) {
                    run_block(&ops[then_start..else_start], entity, world, state, ctx);
                } else {
                    run_block(&ops[else_start..after], entity, world, state, ctx);
                }
                pc = after;
            }
            OpInstruction::Cooldown { seconds, state_slot, body_len } => {
                let bl = *body_len as usize;
                let body_start = pc + 1;
                let after = body_start + bl;
                let remaining = state.floats[state_slot.0 as usize];
                if remaining <= 0.0 {
                    let s = read_float(seconds, ctx.pins);
                    state.floats[state_slot.0 as usize] = s;
                    run_block(&ops[body_start..after], entity, world, state, ctx);
                } else {
                    state.floats[state_slot.0 as usize] = remaining - ctx.dt;
                }
                pc = after;
            }
            OpInstruction::Once { state_slot, body_len } => {
                let bl = *body_len as usize;
                let body_start = pc + 1;
                let after = body_start + bl;
                if !state.bools[state_slot.0 as usize] {
                    state.bools[state_slot.0 as usize] = true;
                    run_block(&ops[body_start..after], entity, world, state, ctx);
                }
                pc = after;
            }
            OpInstruction::Delay { seconds, state_slot, body_len } => {
                let bl = *body_len as usize;
                let body_start = pc + 1;
                let after = body_start + bl;
                let target = read_float(seconds, ctx.pins);
                let elapsed = state.floats[state_slot.0 as usize];
                if elapsed >= target {
                    run_block(&ops[body_start..after], entity, world, state, ctx);
                } else {
                    state.floats[state_slot.0 as usize] = elapsed + ctx.dt;
                }
                pc = after;
            }
            OpInstruction::ForCount { count, state_slot, body_len } => {
                let bl = *body_len as usize;
                let body_start = pc + 1;
                let after = body_start + bl;
                let limit = read_float(count, ctx.pins).max(0.0) as u32;
                let visited = state.counters[state_slot.0 as usize];
                if visited < limit {
                    state.counters[state_slot.0 as usize] = visited + 1;
                    run_block(&ops[body_start..after], entity, world, state, ctx);
                }
                pc = after;
            }
            other => {
                tick_op(other, entity, world, state, ctx);
                pc += 1;
            }
        }
    }
}

fn tick_op(
    op: &OpInstruction,
    entity: hecs::Entity,
    world: &mut World,
    state: &mut GraphInstanceState,
    ctx: &mut BehaviorExecContext<'_>,
) {
    match op {
        OpInstruction::OnTick => { /* trigger marker, no-op */ }

        OpInstruction::GetInputAxis { output } => {
            let v = match world.get::<&InputAxis2D>(entity) {
                Ok(axis) => axis.current,
                Err(_) => {
                    ctx.counters.missing_axis_reads += 1;
                    Vec2::ZERO
                }
            };
            ctx.pins.write_vec2(*output, v);
        }

        OpInstruction::GetPosition { output } => {
            let v = match world.get::<&Transform>(entity) {
                Ok(t) => t.position,
                // Gone from the world - an `OnDespawn` handler. Answer where
                // it was rather than the origin.
                Err(_) => match &state.last_seen {
                    Some(t) => t.position,
                    None => {
                        ctx.counters.missing_transform_reads += 1;
                        Vec2::ZERO
                    }
                },
            };
            ctx.pins.write_vec2(*output, v);
        }

        OpInstruction::GetVelocity { output } => {
            let v = match world.get::<&Velocity2D>(entity) {
                Ok(vel) => Vec2::new(vel.x, vel.y),
                Err(_) => {
                    ctx.counters.missing_velocity_reads += 1;
                    Vec2::ZERO
                }
            };
            ctx.pins.write_vec2(*output, v);
        }

        OpInstruction::SetVelocity { vel } => {
            let v = read_vec2(vel, ctx.pins);
            match world.query_one_mut::<&mut Velocity2D>(entity) {
                Ok(vel_c) => {
                    vel_c.x = v.x;
                    vel_c.y = v.y;
                }
                Err(_) => {
                    ctx.counters.missing_velocity_writes += 1;
                }
            }
        }

        OpInstruction::AddVelocity { delta } => {
            let d = read_vec2(delta, ctx.pins);
            match world.query_one_mut::<&mut Velocity2D>(entity) {
                Ok(vel_c) => {
                    vel_c.x += d.x;
                    vel_c.y += d.y;
                }
                Err(_) => {
                    ctx.counters.missing_velocity_writes += 1;
                }
            }
        }

        OpInstruction::Translate { delta } => {
            let d = read_vec2(delta, ctx.pins);
            match world.query_one_mut::<&mut Transform>(entity) {
                Ok(t) => {
                    t.position += d;
                }
                Err(_) => {
                    ctx.counters.missing_transform_writes += 1;
                }
            }
        }

        OpInstruction::ApplyForce { force } => {
            let f = read_vec2(force, ctx.pins);
            match world.query_one_mut::<&mut Force2D>(entity) {
                Ok(fc) => {
                    fc.x += f.x;
                    fc.y += f.y;
                }
                Err(_) => {
                    ctx.counters.missing_force_writes += 1;
                }
            }
        }

        OpInstruction::BounceOnBounds { bounds } => {
            let target = bounds_or_default(bounds);
            // Idempotent: insert if absent; update bounds only if changed.
            let needs_insert = world.get::<&BounceArea>(entity).is_err();
            if needs_insert {
                let _ = world.insert_one(entity, target);
            } else if let Ok(mut existing) = world.get::<&mut BounceArea>(entity)
                && !bounds_equal(&existing, &target)
            {
                *existing = target;
            }
        }

        OpInstruction::DespawnAfter { seconds } => {
            // Idempotent: only insert if NOT already present (spec §2.6).
            if world.get::<&Lifetime>(entity).is_ok() {
                return;
            }
            let s = read_float(seconds, ctx.pins);
            let _ = world.insert_one(
                entity,
                Lifetime {
                    remaining: s,
                    on_expire: ExpireAction::Despawn,
                },
            );
        }

        OpInstruction::Compare { a, op, b, output } => {
            let lhs = read_float(a, ctx.pins);
            let rhs = read_float(b, ctx.pins);
            ctx.pins.write_bool(*output, op.apply(lhs, rhs));
        }

        OpInstruction::Not { input, output } => {
            let v = read_bool(input, ctx.pins);
            ctx.pins.write_bool(*output, !v);
        }

        OpInstruction::MakeVec2 { x, y, output } => {
            let xv = read_float(x, ctx.pins);
            let yv = read_float(y, ctx.pins);
            ctx.pins.write_vec2(*output, Vec2::new(xv, yv));
        }

        OpInstruction::GetVec2X { input, output } => {
            let v = read_vec2(input, ctx.pins);
            ctx.pins.write_float(*output, v.x);
        }

        OpInstruction::GetVec2Y { input, output } => {
            let v = read_vec2(input, ctx.pins);
            ctx.pins.write_float(*output, v.y);
        }

        OpInstruction::MathFloat { op, a, b, output } => {
            let av = read_float(a, ctx.pins);
            let bv = read_float(b, ctx.pins);
            ctx.pins.write_float(*output, op.apply(av, bv));
        }

        OpInstruction::MathVec2 { op, a, b, output } => {
            let av = read_vec2(a, ctx.pins);
            let bv = read_vec2(b, ctx.pins);
            ctx.pins.write_vec2(*output, op.apply(av, bv));
        }

        OpInstruction::ScaleVec2 { v, s, output } => {
            let vv = read_vec2(v, ctx.pins);
            let sv = read_float(s, ctx.pins);
            ctx.pins.write_vec2(*output, vv * sv);
        }

        OpInstruction::Length { input, output } => {
            let v = read_vec2(input, ctx.pins);
            ctx.pins.write_float(*output, v.length());
        }

        OpInstruction::Normalize { input, output } => {
            let v = read_vec2(input, ctx.pins);
            let n = if v.length_squared() > 1e-12 {
                v.normalize()
            } else {
                Vec2::ZERO
            };
            ctx.pins.write_vec2(*output, n);
        }

        OpInstruction::Distance { a, b, output } => {
            let av = read_vec2(a, ctx.pins);
            let bv = read_vec2(b, ctx.pins);
            ctx.pins.write_float(*output, (av - bv).length());
        }

        OpInstruction::Dot { a, b, output } => {
            let av = read_vec2(a, ctx.pins);
            let bv = read_vec2(b, ctx.pins);
            ctx.pins.write_float(*output, av.dot(bv));
        }

        OpInstruction::Lerp { a, b, t, output } => {
            let av = read_float(a, ctx.pins);
            let bv = read_float(b, ctx.pins);
            let tv = read_float(t, ctx.pins);
            ctx.pins.write_float(*output, av + (bv - av) * tv);
        }

        OpInstruction::Clamp { input, min, max, output } => {
            let v = read_float(input, ctx.pins);
            let mn = read_float(min, ctx.pins);
            let mx = read_float(max, ctx.pins);
            let clamped = if mn > mx { mn } else { v.clamp(mn, mx) };
            ctx.pins.write_float(*output, clamped);
        }

        OpInstruction::MathUnary { op, input, output } => {
            let v = read_float(input, ctx.pins);
            ctx.pins.write_float(*output, op.apply(v));
        }

        OpInstruction::Atan2 { y, x, output } => {
            let yv = read_float(y, ctx.pins);
            let xv = read_float(x, ctx.pins);
            ctx.pins.write_float(*output, yv.atan2(xv));
        }

        OpInstruction::BoolOp { op, a, b, output } => {
            let av = read_bool(a, ctx.pins);
            let bv = read_bool(b, ctx.pins);
            ctx.pins.write_bool(*output, op.apply(av, bv));
        }

        OpInstruction::SelectFloat { cond, a, b, output } => {
            let c = read_bool(cond, ctx.pins);
            let v = if c { read_float(a, ctx.pins) } else { read_float(b, ctx.pins) };
            ctx.pins.write_float(*output, v);
        }

        OpInstruction::SelectVec2 { cond, a, b, output } => {
            let c = read_bool(cond, ctx.pins);
            let v = if c { read_vec2(a, ctx.pins) } else { read_vec2(b, ctx.pins) };
            ctx.pins.write_vec2(*output, v);
        }

        OpInstruction::GetRotation { output } => {
            let r = match world.get::<&Transform>(entity) {
                Ok(t) => t.rotation,
                // Gone from the world - an `OnDespawn` handler. Answer where
                // it was rather than the origin.
                Err(_) => match &state.last_seen {
                    Some(t) => t.rotation,
                    None => {
                        ctx.counters.missing_transform_reads += 1;
                        0.0
                    }
                },
            };
            ctx.pins.write_float(*output, r);
        }

        OpInstruction::GetScale { output } => {
            let s = match world.get::<&Transform>(entity) {
                Ok(t) => t.scale,
                // Gone from the world - an `OnDespawn` handler. Answer where
                // it was rather than the origin.
                Err(_) => match &state.last_seen {
                    Some(t) => t.scale,
                    None => {
                        ctx.counters.missing_transform_reads += 1;
                        Vec2::ONE
                    }
                },
            };
            ctx.pins.write_vec2(*output, s);
        }

        OpInstruction::GetDeltaTime { output } => {
            ctx.pins.write_float(*output, ctx.dt);
        }

        OpInstruction::GetNearestByTag { tag, exclude_self, output } => {
            // Reference point: this entity's own position.
            let self_pos = match world.get::<&Transform>(entity) {
                Ok(t) => t.position,
                Err(_) => {
                    ctx.counters.missing_transform_reads += 1;
                    Vec2::ZERO
                }
            };
            let mut best: Option<hecs::Entity> = None;
            let mut best_d2 = f32::INFINITY;
            for (e, (tags, t)) in world.query::<(&Tags, &Transform)>().iter() {
                if *exclude_self && e == entity {
                    continue;
                }
                if !tags.0.contains(tag) {
                    continue;
                }
                let d2 = (t.position - self_pos).length_squared();
                if d2 < best_d2 {
                    best_d2 = d2;
                    best = Some(e);
                }
            }
            // No match → leave the pin at its reset `None`.
            if let Some(e) = best {
                ctx.pins.write_entity(*output, e);
            }
        }

        OpInstruction::GetEntityPosition { entity: ent_slot, output } => {
            // Empty ref or despawned/invalid entity → fall back to this
            // entity's own position so movement toward it is a no-op.
            let self_pos = || match world.get::<&Transform>(entity) {
                Ok(t) => t.position,
                Err(_) => Vec2::ZERO,
            };
            let pos = match ctx.pins.read_entity(*ent_slot) {
                Some(e) => match world.get::<&Transform>(e) {
                    Ok(t) => t.position,
                    Err(_) => self_pos(),
                },
                None => self_pos(),
            };
            ctx.pins.write_vec2(*output, pos);
        }

        OpInstruction::GetCustomField { component, field, output } => {
            let value = match world.get::<&CustomComponents>(entity) {
                Ok(customs) => match customs.0.get(component) {
                    Some(serde_json::Value::Object(map)) => {
                        map.get(field).and_then(|v| v.as_f64()).map(|v| v as f32)
                    }
                    _ => None,
                },
                Err(_) => None,
            };
            match value {
                Some(v) => ctx.pins.write_float(*output, v),
                None => {
                    ctx.counters.missing_custom_reads += 1;
                    ctx.pins.write_float(*output, 0.0);
                }
            }
        }

        OpInstruction::SpawnEntity {
            prefab_id,
            at,
            rotation,
            scale,
            with_velocity,
        } => {
            let pos = read_vec2(at, ctx.pins);
            let rot = rotation.as_ref().map(|r| read_float(r, ctx.pins)).unwrap_or(0.0);
            let scl = scale
                .as_ref()
                .map(|s| read_vec2(s, ctx.pins))
                .unwrap_or(glam::Vec2::ONE);
            let vel = with_velocity.as_ref().map(|v| read_vec2(v, ctx.pins));
            ctx.commands.push(BehaviorCommand::Spawn {
                prefab_id: prefab_id.clone(),
                at: pos,
                rotation: rot,
                scale: scl,
                with_velocity: vel,
            });
            ctx.counters.commands_emitted += 1;
        }

        OpInstruction::Despawn => {
            ctx.commands.push(BehaviorCommand::Despawn { entity });
            ctx.counters.commands_emitted += 1;
        }

        OpInstruction::DespawnByTag { tag } => {
            ctx.commands.push(BehaviorCommand::DespawnByTag { tag: tag.clone() });
            ctx.counters.commands_emitted += 1;
        }

        OpInstruction::ChangeScene { scene_id } => {
            ctx.commands.push(BehaviorCommand::ChangeScene {
                scene_id: scene_id.clone(),
            });
            ctx.counters.commands_emitted += 1;
        }

        OpInstruction::PlaySound { sound_id, volume } => {
            let vol = read_float(volume, ctx.pins);
            ctx.commands.push(BehaviorCommand::PlaySound {
                sound_id: sound_id.clone(),
                volume: vol,
            });
            ctx.counters.commands_emitted += 1;
        }

        OpInstruction::LookAt { target_pos } => {
            let target = read_vec2(target_pos, ctx.pins);
            // Need both a position read AND rotation write — both come
            // from the same `Transform` so a single mutable query covers
            // both. Missing component bumps both counters per spec §5.
            match world.query_one_mut::<&mut Transform>(entity) {
                Ok(t) => {
                    let delta = target - t.position;
                    if delta.length_squared() > 1e-12 {
                        t.rotation = delta.y.atan2(delta.x);
                    }
                }
                Err(_) => {
                    ctx.counters.missing_transform_reads += 1;
                    ctx.counters.missing_transform_writes += 1;
                }
            }
        }

        OpInstruction::MoveToward { target_pos, speed } => {
            let target = read_vec2(target_pos, ctx.pins);
            let spd = read_float(speed, ctx.pins);
            // Read position from Transform.
            let pos = match world.query_one_mut::<&Transform>(entity) {
                Ok(t) => t.position,
                Err(_) => {
                    ctx.counters.missing_transform_reads += 1;
                    Vec2::ZERO
                }
            };
            let delta = target - pos;
            let new_vel = if delta.length_squared() > 1e-12 {
                delta.normalize() * spd
            } else {
                Vec2::ZERO
            };
            match world.query_one_mut::<&mut Velocity2D>(entity) {
                Ok(v) => {
                    v.x = new_vel.x;
                    v.y = new_vel.y;
                }
                Err(_) => {
                    ctx.counters.missing_velocity_writes += 1;
                }
            }
        }

        OpInstruction::SetPosition { pos } => {
            let p = read_vec2(pos, ctx.pins);
            match world.query_one_mut::<&mut Transform>(entity) {
                Ok(t) => t.position = p,
                Err(_) => ctx.counters.missing_transform_writes += 1,
            }
        }

        OpInstruction::SetRotation { rot } => {
            let r = read_float(rot, ctx.pins);
            match world.query_one_mut::<&mut Transform>(entity) {
                Ok(t) => t.rotation = r,
                Err(_) => ctx.counters.missing_transform_writes += 1,
            }
        }

        OpInstruction::SetScale { scale } => {
            let s = read_vec2(scale, ctx.pins);
            match world.query_one_mut::<&mut Transform>(entity) {
                Ok(t) => t.scale = s,
                Err(_) => ctx.counters.missing_transform_writes += 1,
            }
        }

        OpInstruction::SetSpriteColor { r, g, b, a } => {
            let rv = read_float(r, ctx.pins).clamp(0.0, 1.0);
            let gv = read_float(g, ctx.pins).clamp(0.0, 1.0);
            let bv = read_float(b, ctx.pins).clamp(0.0, 1.0);
            let av = read_float(a, ctx.pins).clamp(0.0, 1.0);
            match world.query_one_mut::<&mut SpriteComponent>(entity) {
                Ok(s) => s.color = [rv, gv, bv, av],
                Err(_) => ctx.counters.missing_sprite_writes += 1,
            }
        }

        OpInstruction::SetSpriteAlpha { alpha } => {
            let av = read_float(alpha, ctx.pins).clamp(0.0, 1.0);
            match world.query_one_mut::<&mut SpriteComponent>(entity) {
                Ok(s) => s.color[3] = av,
                Err(_) => ctx.counters.missing_sprite_writes += 1,
            }
        }

        OpInstruction::AddTag { tag } => {
            if let Ok(t) = world.query_one_mut::<&mut Tags>(entity) {
                t.0.insert(tag.clone());
            } else {
                let mut set = std::collections::HashSet::new();
                set.insert(tag.clone());
                if world.insert_one(entity, Tags(set)).is_err() {
                    ctx.counters.missing_tag_writes += 1;
                }
            }
        }

        OpInstruction::RemoveTag { tag } => {
            if let Ok(t) = world.query_one_mut::<&mut Tags>(entity) {
                t.0.remove(tag);
            }
        }

        OpInstruction::SetCustomField { component, field, value } => {
            let v = read_float(value, ctx.pins);
            // **Creates the component's entry if it is not there yet**, as a
            // script's `set_custom` does. It used to write only into an entry
            // that already existed, so a graph keeping its state in
            // `Enemy.state` wrote nothing unless the prefab happened to carry
            // an `"Enemy": {}` - silently, with one telemetry counter moving.
            // The same write worked from Lua and vanished from a graph.
            //
            // Still does nothing on an entity with no `CustomComponents` at
            // all, which is also what `set_custom` does: adding a component
            // is a structural change, and ops here do not make those.
            match world.query_one_mut::<&mut CustomComponents>(entity) {
                Ok(customs) => match customs
                    .0
                    .entry(component.clone())
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
                {
                    serde_json::Value::Object(map) => {
                        map.insert(field.clone(), serde_json::json!(v));
                    }
                    // The name is taken by something that is not an object
                    // - a script stored a bare number there. Leave it.
                    _ => ctx.counters.missing_custom_writes += 1,
                },
                Err(_) => ctx.counters.missing_custom_writes += 1,
            }
        }

        OpInstruction::Log { label, value } => match value {
            Some(v) => {
                let n = read_float(v, ctx.pins);
                log::info!("[Behavior] {}: {}", label, n);
            }
            None => log::info!("[Behavior] {}", label),
        },

        OpInstruction::PlayAnimation { anim_id } => {
            ctx.commands.push(BehaviorCommand::PlayAnimation {
                entity,
                anim_id: anim_id.clone(),
            });
            ctx.counters.commands_emitted += 1;
        }

        OpInstruction::StopAnimation => {
            ctx.commands.push(BehaviorCommand::StopAnimation { entity });
            ctx.counters.commands_emitted += 1;
        }

        OpInstruction::EmitEvent { name, value } => {
            let v = value.as_ref().map(|v| read_float(v, ctx.pins));
            // Where it happened, from the live transform or - in an
            // `OnDespawn` handler, where the entity has already gone - the
            // last one the system saw.
            let at = match world.get::<&Transform>(entity) {
                Ok(t) => Some(t.position),
                Err(_) => state.last_seen.as_ref().map(|t| t.position),
            };
            ctx.commands.push(BehaviorCommand::EmitEvent {
                name: name.clone(),
                value: v,
                at,
            });
            ctx.counters.commands_emitted += 1;
        }

        // Block ops are dispatched in `run_block`; reaching here is a bug.
        OpInstruction::If { .. }
        | OpInstruction::Cooldown { .. }
        | OpInstruction::Once { .. }
        | OpInstruction::Delay { .. }
        | OpInstruction::ForCount { .. } => {
            debug_assert!(false, "block ops handled in run_block, not tick_op");
        }

        // Pure passthrough — emitted as an editor anchor only.
        OpInstruction::Sequence => {}
    }
}

fn read_vec2(input: &Vec2Input, pins: &PinFrame) -> Vec2 {
    match input {
        Vec2Input::Literal(v) => *v,
        Vec2Input::Pin { slot, scale, offset } => {
            let v = pins.read_vec2(*slot);
            v * *scale + Vec2::splat(*offset)
        }
    }
}

fn read_float(input: &FloatInput, pins: &PinFrame) -> f32 {
    match input {
        FloatInput::Literal(v) => *v,
        FloatInput::Pin { slot, scale, offset } => pins.read_float(*slot) * *scale + *offset,
    }
}

fn read_bool(input: &BoolInput, pins: &PinFrame) -> bool {
    match input {
        BoolInput::Literal(v) => *v,
        BoolInput::Pin { slot } => pins.read_bool(*slot),
    }
}

fn bounds_or_default(bounds: &Option<BoundsLiteral>) -> BounceArea {
    match bounds {
        Some(b) => BounceArea {
            min_x: b.min_x,
            min_y: b.min_y,
            max_x: b.max_x,
            max_y: b.max_y,
            damping: b.damping,
        },
        None => BounceArea::default(),
    }
}

fn bounds_equal(a: &BounceArea, b: &BounceArea) -> bool {
    a.min_x == b.min_x
        && a.min_y == b.min_y
        && a.max_x == b.max_x
        && a.max_y == b.max_y
        && a.damping == b.damping
}
