//! Parallel chunk-iteration helpers for hot ECS systems.
//!
//! Step 2C of the scripting redesign plan. Native-only — `rayon` cannot run
//! on single-threaded WASM, so the public functions in this module are gated
//! `cfg(not(target_arch = "wasm32"))`. The serial fallbacks live in
//! [`super::systems`] and remain the WASM code path.
//!
//! ## Determinism
//!
//! All systems exposed here are commutative per-entity (each entity's update
//! depends only on its own components and a global `dt`), so chunk ordering
//! does not affect the end state. The only output that is *order-sensitive*
//! is [`update_lifetimes_par`], which returns despawn candidates: we sort by
//! `Entity::to_bits()` before returning so callers see a deterministic order
//! regardless of worker scheduling.
//!
//! ## Batch size
//!
//! 2048 was picked as a starting point: large enough that per-task rayon
//! overhead (~5 µs) is amortized, small enough that 100k+ entities split
//! across 8+ workers. Tunable per system if profiling shows otherwise.

#![cfg(not(target_arch = "wasm32"))]

use crate::ecs::components::{BounceArea, Gravity2D, Lifetime, ExpireAction, Transform, Velocity2D};
use hecs::{Entity, World};
use rayon::prelude::*;
use std::sync::Once;

const DEFAULT_BATCH: u32 = 2048;

/// Cap rayon's global thread pool to a safe number of workers BEFORE the
/// pool is first used. Idempotent — safe to call from any system entry
/// point. Cap defaults to `min(num_cpus / 2, 4)` so the engine never
/// pegs every core (which contributed to host crashes during
/// uncapped 100k-entity benchmarks). Override with
/// `JENGINE_RAYON_THREADS=N`.
pub fn ensure_pool_capped() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let cap = std::env::var("JENGINE_RAYON_THREADS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or_else(|| {
                let cores = std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4);
                (cores / 2).clamp(1, 4)
            });
        // If the global pool was already initialised by some other call site,
        // `build_global` returns Err — that's fine; we just log and continue.
        if let Err(e) = rayon::ThreadPoolBuilder::new()
            .num_threads(cap)
            .thread_name(|i| format!("jengine-ecs-{i}"))
            .build_global()
        {
            log::debug!("rayon global pool already initialised: {e}");
        } else {
            log::info!("jEngine ECS rayon pool capped to {cap} workers");
        }
    });
}

/// Parallel `vel.x += g.x * dt` over all `(Velocity2D, Gravity2D)` pairs.
pub fn update_gravity_par(world: &mut World, dt: f32) {
    world
        .query_mut::<(&mut Velocity2D, &Gravity2D)>()
        .into_iter_batched(DEFAULT_BATCH)
        .par_bridge()
        .for_each(|batch| {
            for (_, (vel, g)) in batch {
                vel.x += g.x * dt;
                vel.y += g.y * dt;
            }
        });
}

/// Parallel `transform.position += vel * dt` over all `(Transform, Velocity2D)` pairs.
pub fn update_velocity_integration_par(world: &mut World, dt: f32) {
    world
        .query_mut::<(&mut Transform, &Velocity2D)>()
        .into_iter_batched(DEFAULT_BATCH)
        .par_bridge()
        .for_each(|batch| {
            for (_, (transform, vel)) in batch {
                transform.position.x += vel.x * dt;
                transform.position.y += vel.y * dt;
            }
        });
}

/// Parallel bounce reflection at `BounceArea` boundaries.
pub fn update_bounds_bounce_par(world: &mut World) {
    world
        .query_mut::<(&mut Transform, &mut Velocity2D, &BounceArea)>()
        .into_iter_batched(DEFAULT_BATCH)
        .par_bridge()
        .for_each(|batch| {
            for (_, (transform, vel, area)) in batch {
                if transform.position.x < area.min_x {
                    transform.position.x = area.min_x;
                    vel.x = vel.x.abs() * area.damping;
                } else if transform.position.x > area.max_x {
                    transform.position.x = area.max_x;
                    vel.x = -(vel.x.abs() * area.damping);
                }
                if transform.position.y < area.min_y {
                    transform.position.y = area.min_y;
                    vel.y = vel.y.abs() * area.damping;
                } else if transform.position.y > area.max_y {
                    transform.position.y = area.max_y;
                    vel.y = -(vel.y.abs() * area.damping);
                }
            }
        });
}

/// Parallel `Lifetime.remaining -= dt`. Returns expired entities sorted by
/// `Entity::to_bits()` so caller-side despawn order is deterministic.
pub fn update_lifetimes_par(world: &mut World, dt: f32) -> Vec<Entity> {
    let mut expired: Vec<Entity> = world
        .query_mut::<&mut Lifetime>()
        .into_iter_batched(DEFAULT_BATCH)
        .par_bridge()
        .flat_map_iter(|batch| {
            let mut local = Vec::new();
            for (e, life) in batch {
                life.remaining -= dt;
                if life.remaining <= 0.0 && matches!(life.on_expire, ExpireAction::Despawn) {
                    local.push(e);
                }
            }
            local.into_iter()
        })
        .collect();
    expired.sort_unstable_by_key(|e| e.to_bits());
    expired
}
