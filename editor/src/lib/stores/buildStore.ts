import { writable, get } from "svelte/store";
import { toast } from "./toastStore";
import { focusPanel } from "./layoutStore";

/** One line of build output, as the builder emitted it. */
export interface BuildLine {
  text: string;
  timestamp: number;
  /** Coarse classification, used for colour only — see classify(). */
  kind: "plain" | "command" | "warning" | "error" | "success";
  /**
   * The line arrived terminated by a bare carriage return, so the next redraw
   * overwrites it rather than following it. Progress bars are drawn that way.
   */
  transient: boolean;
}

/**
 * A full `cargo` build prints thousands of lines. Keeping every one costs
 * memory and makes the panel slow to render, and nobody scrolls back that
 * far — the interesting part is always the tail.
 */
const MAX_LINES = 5000;

export const buildLines = writable<BuildLine[]>([]);
/** True while a build is running, so the panel can show it is live. */
export const buildRunning = writable(false);

/**
 * Colour classification. cargo writes everything to stderr, including
 * ordinary progress, so the stream a line arrived on says nothing about
 * whether it is a problem — the text is all there is to go on.
 */
function classify(text: string): BuildLine["kind"] {
  if (text.startsWith("$ ")) return "command";
  // `error[E0433]:`, `error:`, "Web build failed: ..."
  if (/^error(\[|:)|\bbuild failed\b|^\s*failed\b/i.test(text)) return "error";
  if (/^warning(\[|:)/i.test(text)) return "warning";
  if (/^\s*(Finished|Compiling|Building|Copied|Generated|Wrote|Running)\b/.test(text))
    return "success";
  if (/\bcomplete\b/i.test(text)) return "success";
  return "plain";
}

/**
 * Append a line, or redraw the last one.
 *
 * `replacesPrevious` is a terminal's carriage return: cargo and wasm-pack draw
 * a progress bar by rewriting the same line hundreds of times, and appending
 * each redraw would bury the actual build output. It only overwrites a line
 * that was *itself* a redraw — after a properly terminated line the cursor is
 * on a fresh one, so the first redraw of a new bar still appends.
 */
export function appendBuildLine(text: string, replacesPrevious = false) {
  buildLines.update((lines) => {
    const line = {
      text,
      timestamp: Date.now(),
      kind: classify(text),
      transient: replacesPrevious,
    };
    const last = lines[lines.length - 1];
    if (replacesPrevious && last?.transient) {
      return [...lines.slice(0, -1), line];
    }
    const next = [...lines, line];
    return next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next;
  });
}

export function clearBuildOutput() {
  buildLines.set([]);
}

let buildLabel = "";
let buildStarted = 0;

/** Called when a build starts: fresh transcript, marked live. */
export function beginBuild(label: string) {
  clearBuildOutput();
  buildRunning.set(true);
  buildLabel = label;
  buildStarted = Date.now();
  appendBuildLine(`=== ${label} ===`);
}

/** Called when a build ends, successfully or not. It toasts, so a build
 *  finishing is seen from whatever panel is in front. */
export function endBuild(message: string) {
  buildRunning.set(false);
  appendBuildLine(message);
  const failed = classify(message) === "error" || /\bfailed\b/i.test(message);
  const secs = buildStarted ? Math.round((Date.now() - buildStarted) / 1000) : 0;
  toast({
    kind: failed ? "err" : "ok",
    title: failed ? "Build failed" : "Build finished",
    detail: `${buildLabel || message}${secs ? ` - ${secs} s` : ""}`,
    action: () => focusPanel("build"),
  });
}

/** Whether anything has been captured yet — for the empty state. */
export function hasBuildOutput(): boolean {
  return get(buildLines).length > 0;
}
