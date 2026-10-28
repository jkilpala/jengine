//! Times the CPU benchmark natively and prints one JSON line:
//!
//! ```text
//! cargo run -p jengine-benchmark-cpu --release [-- --frames 600 --loads 20 --runs 5]
//! ```
//!
//! Each workload is run `runs` times after a warm-up, each time on a fresh
//! world (a pile that has gone to sleep is cheaper to step), and the fastest
//! run is reported - the least disturbed by the rest of the machine.

use std::time::Instant;

use jengine_benchmark_cpu::Bench;

fn arg(name: &str, default: u32) -> u32 {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// The fastest of `runs` timed calls of `f` (after one untimed), each on a
/// fresh `Bench`; the checksums summed, to print.
fn best(runs: u32, f: impl Fn(&mut Bench)) -> (f64, f64) {
    let mut checksum = 0.0;
    let ms = (0..=runs)
        .map(|_| {
            let mut bench = Bench::new();
            let t = Instant::now();
            f(&mut bench);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            checksum += bench.checksum();
            ms
        })
        .skip(1)
        .fold(f64::MAX, f64::min);
    (ms, checksum)
}

fn main() {
    let (frames, loads, spawns, runs) = (arg("--frames", 600), arg("--loads", 20), arg("--spawns", 10_000), arg("--runs", 5));
    let (frame, a) = best(runs, |b| b.frames(frames));
    let (load, b) = best(runs, |b| b.load_scene(loads));
    let (spawn, c) = best(runs, |b| b.spawn(spawns));
    let (spawn_registry, d) = best(runs, |b| b.spawn_registry(spawns));
    println!(
        "{{\"frame_ms\":{:.4},\"scene_load_ms\":{:.3},\"spawn_us\":{:.3},\"spawn_registry_us\":{:.3},\"scene_bytes\":{},\"checksum\":{}}}",
        frame / frames as f64,
        load / loads as f64,
        spawn * 1000.0 / spawns as f64,
        spawn_registry * 1000.0 / spawns as f64,
        Bench::new().scene_bytes(),
        a + b + c + d
    );
}
