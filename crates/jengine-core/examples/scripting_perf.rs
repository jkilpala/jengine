//! P3.9 — scripted-entity perf check for the bridge runner.
//!
//! Drives N entities for M frames through both `LuauAdapter` and (when the
//! `js-adapter-host` feature is enabled) `JsAdapter`, using the same
//! `ScriptBridgeRunner` path that `ScriptSystem` uses in production. No GPU,
//! no scene file, no AssetManager — just the script tick cost in isolation.
//!
//! Two workloads:
//!   * `cheap`   — `update(e, dt)` does nothing. Measures dispatch + FFI
//!                 baseline (the floor a scripted entity costs the engine).
//!   * `typical` — read position from snapshot, increment x by `velocity*dt`,
//!                 write position back. One snapshot read + one
//!                 `BridgeCommand::SetPosition` per entity per frame.
//!
//! Run:
//!   cargo run -p jengine-core --release --example scripting_perf \
//!     --features js-adapter-host -- --entities 100 --frames 1000
//!
//! Defaults are tuned to surface the P3.9 acceptance criterion ("100 scripted
//! entities, no perf regression vs current Lua") while finishing in seconds.

use std::time::Instant;

use jengine_core::scripting::bridge::adapter::ScriptAdapter;
use jengine_core::scripting::bridge::adapters::LuauAdapter;
#[cfg(feature = "js-adapter-host")]
use jengine_core::scripting::bridge::adapters::JsAdapter;
use jengine_core::scripting::bridge::capability::Capabilities;
use jengine_core::scripting::bridge::context::BridgeReadSnapshot;
use jengine_core::scripting::bridge::runner::ScriptBridgeRunner;

#[derive(Clone, Copy)]
struct Args {
    entities: u64,
    frames: u32,
    workload: Workload,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Workload {
    Cheap,
    Typical,
}

impl Workload {
    fn as_str(&self) -> &'static str {
        match self {
            Workload::Cheap => "cheap",
            Workload::Typical => "typical",
        }
    }

    /// Source common to both adapters (Luau syntax; `JsAdapter` transpiles).
    fn source(&self) -> &'static str {
        match self {
            Workload::Cheap => "function update(e, dt)\nend\n",
            Workload::Typical => {
                // One snapshot read + one mutation per frame. Mirrors the
                // simplest "kinematic mover" pattern script authors write.
                "function update(e, dt)\n  \
                   local x, y = get_position(e)\n  \
                   set_position(e, x + 60.0 * dt, y)\n\
                 end\n"
            }
        }
    }
}

fn parse_args() -> Args {
    let mut args = Args { entities: 100, frames: 1000, workload: Workload::Typical };
    let mut iter = std::env::args().skip(1);
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--entities" | "-n" => {
                if let Some(v) = iter.next() {
                    args.entities = v.parse().expect("--entities expects an integer");
                }
            }
            "--frames" | "-f" => {
                if let Some(v) = iter.next() {
                    args.frames = v.parse().expect("--frames expects an integer");
                }
            }
            "--workload" | "-w" => {
                if let Some(v) = iter.next() {
                    args.workload = match v.as_str() {
                        "cheap" => Workload::Cheap,
                        "typical" => Workload::Typical,
                        other => panic!("unknown workload '{other}' (expected cheap|typical)"),
                    };
                }
            }
            "--help" | "-h" => {
                eprintln!(
                    "scripting_perf [--entities N] [--frames M] [--workload cheap|typical]"
                );
                std::process::exit(0);
            }
            other => panic!("unknown arg '{other}' (try --help)"),
        }
    }
    args
}

struct Sample {
    backend: &'static str,
    init_us: f64,
    /// Total time for the steady-state frames (excludes the init frame).
    steady_total_us: f64,
    steady_frames: u32,
    entities: u64,
    cmds_per_frame: usize,
}

impl Sample {
    fn print(&self) {
        let per_frame = self.steady_total_us / self.steady_frames as f64;
        let per_entity_ns = (per_frame * 1_000.0) / self.entities as f64;
        let frame_budget_pct = per_frame / 16_666.0 * 100.0;
        println!(
            "  {:<6}  init {:>9.1} us  | steady {:>9.2} us/frame  | {:>7.1} ns/entity  | {:>5.2}% of 60Hz frame  | {} cmds/frame",
            self.backend,
            self.init_us,
            per_frame,
            per_entity_ns,
            frame_budget_pct,
            self.cmds_per_frame,
        );
    }
}

fn build_loads(entities: u64, path: &str, source: &str) -> Vec<(u64, String, String)> {
    (1..=entities)
        .map(|id| (id, path.to_string(), source.to_string()))
        .collect()
}

fn run_one(
    label: &'static str,
    adapter: Box<dyn ScriptAdapter>,
    args: Args,
    path: &str,
) -> Sample {
    let mut runner = ScriptBridgeRunner::new(adapter, Capabilities::empty());
    let mut snap = BridgeReadSnapshot::default();
    // Seed every entity with a position so `get_position(e)` succeeds in the
    // typical workload. Cheap workload doesn't read but seeding is harmless.
    for id in 1..=args.entities {
        snap.positions.insert(id, (0.0, 0.0));
    }
    let loads = build_loads(args.entities, path, args.workload.source());

    // Frame 0: cold load + per-entity init + first update. Reported separately
    // from steady-state because load is amortised across all subsequent frames.
    let t0 = Instant::now();
    let (cmds0, errs) = runner.run_frame(&loads, &snap, 0.016, &[]);
    let init_us = t0.elapsed().as_secs_f64() * 1_000_000.0;
    if !errs.is_empty() {
        panic!("{label}: errors during cold frame: {errs:?}");
    }

    // Steady-state frames.
    let t1 = Instant::now();
    let mut last_cmd_count = cmds0.len();
    for _ in 0..args.frames {
        let (cmds, errs) = runner.run_frame(&loads, &snap, 0.016, &[]);
        if !errs.is_empty() {
            panic!("{label}: errors during steady frame: {errs:?}");
        }
        last_cmd_count = cmds.len();
    }
    let steady_total_us = t1.elapsed().as_secs_f64() * 1_000_000.0;

    Sample {
        backend: label,
        init_us,
        steady_total_us,
        steady_frames: args.frames,
        entities: args.entities,
        cmds_per_frame: last_cmd_count,
    }
}

fn main() {
    let args = parse_args();

    println!(
        "scripting_perf — entities={} frames={} workload={}",
        args.entities,
        args.frames,
        args.workload.as_str(),
    );
    println!(
        "  format: backend  init us  | steady us/frame  | ns/entity  | % of 60Hz frame  | cmds/frame"
    );

    // --- Luau ---
    let luau = LuauAdapter::new(Capabilities::empty()).expect("luau adapter");
    let path = format!("perf_{}.luau", args.workload.as_str());
    let sample = run_one("luau", Box::new(luau), args, &path);
    sample.print();

    // --- JS (host QuickJS) — opt-in via `--features js-adapter-host` ---
    #[cfg(feature = "js-adapter-host")]
    {
        let js = JsAdapter::new();
        let sample = run_one("js", Box::new(js), args, &path);
        sample.print();
    }
    #[cfg(not(feature = "js-adapter-host"))]
    {
        println!("  js     skipped — rebuild with `--features js-adapter-host` to include");
    }
}
