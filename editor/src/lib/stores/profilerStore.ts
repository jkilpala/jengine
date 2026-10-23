/**
 * Recording a profiler capture in the editor. While recording, the viewport
 * keeps each frame's spans (`viewport_set_profiling`), the Lua runtime times
 * each script call into the map at `__jengine_profile_scripts`, and after
 * every `viewport_render` the viewport calls `profilerFrame`, which takes
 * the frame's record (`viewport_take_profile_frame`), adds the scripts' costs
 * and keeps it. The panel reads `profilerCapture`, which is refreshed a few
 * times a second rather than every frame.
 */
import { writable } from "svelte/store";
import { MAX_FRAMES, pushFrame, withScripts, type Capture, type FrameRecord } from "../profileCapture";

export const profilerRecording = writable(false);
export const profilerCapture = writable<Capture>({ runtime: "editor", frames: [] });

let recording = false;
let wasm: any = null;
let frames: FrameRecord[] = [];
let costs = new Map<string, { ms: number; calls: number }>();
let lastPublish = 0;
const PUBLISH_MS = 250;

function publish() {
  lastPublish = performance.now();
  profilerCapture.set({ runtime: "editor", frames: frames.slice() });
}

export async function startRecording(): Promise<void> {
  wasm = await import("../viewport-wasm/jengine_editor_viewport.js");
  if (typeof wasm.viewport_set_profiling !== "function") {
    throw new Error("this viewport build cannot record - rebuild the viewport");
  }
  frames = [];
  costs = new Map();
  (globalThis as any).__jengine_profile_scripts = costs;
  wasm.viewport_set_profiling(true);
  recording = true;
  profilerRecording.set(true);
  publish();
}

export function stopRecording(): void {
  if (!recording) return;
  recording = false;
  wasm?.viewport_set_profiling?.(false);
  delete (globalThis as any).__jengine_profile_scripts;
  profilerRecording.set(false);
  publish();
}

/** Called by the viewport after each rendered frame. */
export function profilerFrame(): void {
  if (!recording || !wasm) return;
  const text: string = wasm.viewport_take_profile_frame();
  if (!text || text === "null") return;
  pushFrame(frames, withScripts(JSON.parse(text) as FrameRecord, costs), MAX_FRAMES);
  costs.clear();
  if (performance.now() - lastPublish >= PUBLISH_MS) publish();
}

/** Show a capture read from a file (recording stops). */
export function showCapture(c: Capture): void {
  stopRecording();
  frames = c.frames.slice();
  profilerCapture.set(c);
}

export function clearCapture(): void {
  frames = [];
  publish();
}
