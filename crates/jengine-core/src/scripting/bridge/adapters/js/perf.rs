//! Phase D §D5 — `require()` cache-hit micro-bench.
//!
//! Runs N `require("lib/x")` calls inside a single `update()` tick on both
//! [`LuauAdapter`] and the host [`JsAdapter`], then prints microseconds per
//! call. Ignored by default — run on demand:
//!
//! ```text
//! cargo test -p jengine-core --features js-adapter-host --release --lib \
//!     bridge::adapters::js::perf -- --ignored --nocapture
//! ```
//!
//! Goal (per `docs/archive/scripting-redesign-plan.md` Phase D / verification #5):
//! cache-hit `require()` should be sub-microsecond on the JS host adapter, and
//! comparable on Luau. The test does not assert a wall-clock threshold (CI
//! variance) — it prints numbers for human inspection.

use std::rc::Rc;
use std::time::Instant;

use super::JsAdapter;
use super::super::super::adapter::ScriptAdapter;
use super::super::super::capability::Capabilities;
use super::super::super::command::BridgeCommand;
use super::super::super::context::{BridgeContext, BridgeReadSnapshot};
use super::super::super::module::ModuleResolver;
use super::super::luau::LuauAdapter;

const ITERS: usize = 10_000;
const ENTITY_ID: u64 = 1;

/// Build a Luau script whose `update()` body performs `ITERS` cache-hit
/// `require()` calls in a tight loop. The first call (in the script's
/// top-level body, before `update`) primes the cache so every measured call
/// is a hit.
fn make_loop_script() -> String {
    format!(
        r#"
        require("lib/x")  -- prime the cache
        function update(e, dt)
            for i = 1, {n} do
                local m = require("lib/x")
            end
        end
        "#,
        n = ITERS,
    )
}

fn resolver() -> ModuleResolver {
    Rc::new(|path: &str| match path {
        "lib/x" => Ok("return { tag = 'ok' }".into()),
        other => Err(format!("not found: {other}")),
    })
}

#[test]
#[ignore]
fn require_cache_hit_perf_luau() {
    let mut a = LuauAdapter::new(Capabilities::empty()).expect("luau");
    a.set_module_resolver(resolver());
    let h = a.load("perf.luau", &make_loop_script()).expect("load");

    let snap = BridgeReadSnapshot::default();
    let mut cmds: Vec<BridgeCommand> = Vec::new();
    let mut ctx = BridgeContext::new(0.016, &snap, Capabilities::empty(), &mut cmds);

    // Warm-up tick (JIT, branch predictor, allocator).
    a.update(h, ENTITY_ID, &mut ctx).expect("warmup");

    let start = Instant::now();
    a.update(h, ENTITY_ID, &mut ctx).expect("measured tick");
    let elapsed = start.elapsed();

    let per_call_ns = elapsed.as_nanos() as f64 / ITERS as f64;
    eprintln!(
        "[D5/luau] {} require() cache-hit calls: total {:.2?}, {:.1} ns/call ({:.3} µs/call)",
        ITERS,
        elapsed,
        per_call_ns,
        per_call_ns / 1000.0,
    );
}

#[test]
#[ignore]
fn require_cache_hit_perf_js_host() {
    let mut a = JsAdapter::new();
    a.set_module_resolver(resolver());
    let h = a.load("perf.luau", &make_loop_script()).expect("load");

    let snap = BridgeReadSnapshot::default();
    let mut cmds: Vec<BridgeCommand> = Vec::new();
    let mut ctx = BridgeContext::new(0.016, &snap, Capabilities::empty(), &mut cmds);

    // Warm-up tick.
    a.update(h, ENTITY_ID, &mut ctx).expect("warmup");

    let start = Instant::now();
    a.update(h, ENTITY_ID, &mut ctx).expect("measured tick");
    let elapsed = start.elapsed();

    let per_call_ns = elapsed.as_nanos() as f64 / ITERS as f64;
    eprintln!(
        "[D5/js-host] {} require() cache-hit calls: total {:.2?}, {:.1} ns/call ({:.3} µs/call)",
        ITERS,
        elapsed,
        per_call_ns,
        per_call_ns / 1000.0,
    );
}
