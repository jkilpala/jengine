// Times the CPU benchmark as WebAssembly in Node (V8, as Chrome runs it):
//
//   wasm-pack build crates/jengine-benchmark-cpu --target nodejs --release --out-dir <pkg>
//   node crates/jengine-benchmark-cpu/bench.mjs <pkg> [frames] [loads] [runs]
//
// Prints one JSON line in the same shape as the native binary. Each run
// starts from a fresh world; the fastest of `runs` (after one untimed) is
// reported.
import { createRequire } from "node:module";
import { resolve } from "node:path";

const [pkg, frames = 600, loads = 20, runs = 5, spawns = 10000] = process.argv.slice(2);
const { WasmBench } = createRequire(import.meta.url)(resolve(pkg, "jengine_benchmark_cpu.js"));

function best(f) {
  let fastest = Infinity;
  let checksum = 0;
  for (let i = 0; i <= runs; i++) {
    const bench = new WasmBench();
    const t = performance.now();
    f(bench);
    const ms = performance.now() - t;
    checksum += bench.checksum();
    bench.free();
    if (i > 0) fastest = Math.min(fastest, ms);
  }
  return [fastest, checksum];
}

const [frame, a] = best((b) => b.frames(Number(frames)));
const [load, b] = best((b) => b.load_scene(Number(loads)));
const [spawn, c] = best((b) => b.spawn(Number(spawns)));
const [spawnRegistry, d] = best((b) => b.spawn_registry(Number(spawns)));
const bench = new WasmBench();
console.log(JSON.stringify({
  frame_ms: +(frame / frames).toFixed(4),
  scene_load_ms: +(load / loads).toFixed(3),
  spawn_us: +((spawn * 1000) / spawns).toFixed(3),
  spawn_registry_us: +((spawnRegistry * 1000) / spawns).toFixed(3),
  scene_bytes: bench.scene_bytes(),
  checksum: a + b + c + d,
}));
