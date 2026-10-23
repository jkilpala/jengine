//! Slice 5.5c — deterministic-PRNG integration test.
//!
//! Asserts the two reproducibility properties supported by the slice
//! 5.5c per-chunk seeding scheme — seed = `mix_seed(scene_seed,
//! frame, system_id, chunk_id)`:
//!
//! 1. **Cross-run, same N**: identical seed/workers ⇒ bit-identical
//!    final write columns. (`*_is_reproducible_across_runs`).
//! 2. **Cross-N, chunk-0 prefix**: chunk 0 always uses
//!    `mix_seed(…, chunk_id=0)` regardless of pool size, so the
//!    leading `N_ENTITIES / max_workers` entities draw the same RNG
//!    stream for `N ∈ {1, 2, 4, 8}`.
//!    (`*_chunk0_prefix_matches_across_worker_counts`).
//!
//! Bit-equality across all entities for N≥2 would require per-entity
//! seeding (out of scope for 5.5c — see
//! `docs/scripting-performance-lessons.md`).
//!
//! Gated on `js-adapter-host` (BatchScriptPool only compiles with
//! that feature). Lua coverage piggybacks on `native`.
#![cfg(all(feature = "native", feature = "js-adapter-host"))]

use jengine_core::profiling::FrameCounters;
use jengine_core::scripting::worker_pool::{
    BatchScriptPool, OwnedReadColumn, OwnedWriteColumn,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const N_ENTITIES: usize = 1024;
const N_FRAMES: usize = 60;
const SCENE_SEED: u64 = 0xA5A5_5A5A_DEAD_BEEF;
/// Largest pool size we test — chunk-0 prefix length is
/// `N_ENTITIES / MAX_WORKERS`, the slice across which all N values
/// are guaranteed to agree.
const MAX_WORKERS: usize = 8;
const PREFIX_LEN: usize = N_ENTITIES / MAX_WORKERS;

const JS_SCRIPT: &str = r#"
module.exports.update = function(ctx, q) {
    const px = q.write.Position2D.x;
    const py = q.write.Position2D.y;
    const vx = q.read.Velocity2D.x;
    const vy = q.read.Velocity2D.y;
    for (let i = 0; i < q.count; i++) {
        const r = Math.random();
        px[i] = px[i] + vx[i] * ctx.dt + r;
        py[i] = py[i] + vy[i] * ctx.dt + r * 0.5;
    }
};
module.exports.system = { query: { read: ['Velocity2D'], write: ['Position2D'] } };
"#;

#[cfg(feature = "native")]
const LUA_SCRIPT: &str = r#"
system = { query = { read = {"Velocity2D"}, write = {"Position2D"} } }
function update(ctx, q)
    local px = q.write.Position2D.x
    local py = q.write.Position2D.y
    local vx = q.read.Velocity2D.x
    local vy = q.read.Velocity2D.y
    for i = 1, q.count do
        local r = math.random()
        px[i] = px[i] + vx[i] * ctx.dt + r
        py[i] = py[i] + vy[i] * ctx.dt + r * 0.5
    end
end
"#;

fn build_initial_columns() -> (Vec<OwnedReadColumn>, Vec<OwnedWriteColumn>) {
    let read = OwnedReadColumn {
        name: "Velocity2D".to_string(),
        x: (0..N_ENTITIES).map(|i| (i as f32) * 0.01).collect(),
        y: (0..N_ENTITIES).map(|i| (i as f32) * -0.01).collect(),
    };
    let write = OwnedWriteColumn {
        name: "Position2D".to_string(),
        x: vec![0.0; N_ENTITIES],
        y: vec![0.0; N_ENTITIES],
    };
    (vec![read], vec![write])
}

fn hash_columns(writes: &[OwnedWriteColumn]) -> u64 {
    let mut h = DefaultHasher::new();
    for col in writes {
        col.name.hash(&mut h);
        for v in &col.x {
            v.to_bits().hash(&mut h);
        }
        for v in &col.y {
            v.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

/// Hash only the first `PREFIX_LEN` entries of each column. This is
/// the chunk-0 prefix that every N ∈ {1, 2, 4, 8} pool is
/// guaranteed to compute identically (chunk 0 always seeds with
/// `chunk_id = 0`).
fn hash_prefix(writes: &[OwnedWriteColumn]) -> u64 {
    let mut h = DefaultHasher::new();
    for col in writes {
        col.name.hash(&mut h);
        for v in &col.x[..PREFIX_LEN] {
            v.to_bits().hash(&mut h);
        }
        for v in &col.y[..PREFIX_LEN] {
            v.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

fn run_js_workload(workers: u32) -> u64 {
    let writes = run_js_to_writes(workers);
    hash_columns(&writes)
}

fn run_js_workload_prefix(workers: u32) -> u64 {
    let writes = run_js_to_writes(workers);
    hash_prefix(&writes)
}

fn run_js_to_writes(workers: u32) -> Vec<OwnedWriteColumn> {
    let mut pool = BatchScriptPool::new_pooled(workers, None).expect("new_pooled");
    pool.set_scene_seed(SCENE_SEED);
    pool.load_js("det.js", JS_SCRIPT).expect("load_js");

    let (reads_initial, mut writes) = build_initial_columns();
    let mut counters = FrameCounters::default();
    for _ in 0..N_FRAMES {
        let reads = reads_initial.iter().map(|c| OwnedReadColumn {
            name: c.name.clone(),
            x: c.x.clone(),
            y: c.y.clone(),
        }).collect();
        let out = pool
            .run_js_batch("det.js", 1.0 / 60.0, reads, writes, vec![], 0, &mut counters)
            .expect("run_js_batch");
        writes = out.writes;
    }
    writes
}

#[cfg(feature = "native")]
fn run_lua_workload(workers: u32) -> u64 {
    let writes = run_lua_to_writes(workers);
    hash_columns(&writes)
}

#[cfg(feature = "native")]
fn run_lua_workload_prefix(workers: u32) -> u64 {
    let writes = run_lua_to_writes(workers);
    hash_prefix(&writes)
}

#[cfg(feature = "native")]
fn run_lua_to_writes(workers: u32) -> Vec<OwnedWriteColumn> {
    use jengine_core::scripting::batch_lua::BatchLuaBufferMode;
    let mut pool = BatchScriptPool::new_pooled(workers, Some(BatchLuaBufferMode::Table))
        .expect("new_pooled");
    pool.set_scene_seed(SCENE_SEED);
    pool.load_lua("det.lua", LUA_SCRIPT, BatchLuaBufferMode::Table)
        .expect("load_lua");

    let (reads_initial, mut writes) = build_initial_columns();
    let mut counters = FrameCounters::default();
    for _ in 0..N_FRAMES {
        let reads = reads_initial.iter().map(|c| OwnedReadColumn {
            name: c.name.clone(),
            x: c.x.clone(),
            y: c.y.clone(),
        }).collect();
        let out = pool
            .run_lua_batch("det.lua", 1.0 / 60.0, reads, writes, vec![], 0, &mut counters)
            .expect("run_lua_batch");
        writes = out.writes;
    }
    writes
}

#[test]
fn js_batch_chunk0_prefix_matches_across_worker_counts() {
    let h1 = run_js_workload_prefix(1);
    let h2 = run_js_workload_prefix(2);
    let h4 = run_js_workload_prefix(4);
    let h8 = run_js_workload_prefix(8);
    assert_eq!(h1, h2, "JS chunk-0 prefix: N=1 vs N=2 diverged ({h1:#x} vs {h2:#x})");
    assert_eq!(h1, h4, "JS chunk-0 prefix: N=1 vs N=4 diverged ({h1:#x} vs {h4:#x})");
    assert_eq!(h1, h8, "JS chunk-0 prefix: N=1 vs N=8 diverged ({h1:#x} vs {h8:#x})");
}

#[test]
fn js_batch_is_reproducible_across_runs() {
    let h_a = run_js_workload(4);
    let h_b = run_js_workload(4);
    assert_eq!(h_a, h_b, "JS: same seed/workers produced different hashes ({h_a:#x} vs {h_b:#x})");
}

#[cfg(feature = "native")]
#[test]
fn lua_batch_chunk0_prefix_matches_across_worker_counts() {
    let h1 = run_lua_workload_prefix(1);
    let h2 = run_lua_workload_prefix(2);
    let h4 = run_lua_workload_prefix(4);
    let h8 = run_lua_workload_prefix(8);
    assert_eq!(h1, h2, "Lua chunk-0 prefix: N=1 vs N=2 diverged ({h1:#x} vs {h2:#x})");
    assert_eq!(h1, h4, "Lua chunk-0 prefix: N=1 vs N=4 diverged ({h1:#x} vs {h4:#x})");
    assert_eq!(h1, h8, "Lua chunk-0 prefix: N=1 vs N=8 diverged ({h1:#x} vs {h8:#x})");
}

#[cfg(feature = "native")]
#[test]
fn lua_batch_is_reproducible_across_runs() {
    let h_a = run_lua_workload(4);
    let h_b = run_lua_workload(4);
    assert_eq!(h_a, h_b, "Lua: same seed/workers produced different hashes ({h_a:#x} vs {h_b:#x})");
}
