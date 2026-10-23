import { writable, get } from "svelte/store";

// Toasts: the design kit's cards in the bottom-right corner - a bar in the
// state's colour, a title, a detail. For things worth noticing from any
// panel: a build that finished or failed, a script that errored.

export type ToastKind = "ok" | "info" | "warn" | "err";

export interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  detail?: string;
  /** Show the detail in mono - a file and line, a path. */
  mono?: boolean;
  /** What clicking the toast does (open the script, show the Build panel). */
  action?: () => void;
  /** Wall-clock ms it goes at; refreshed when the same toast comes again. */
  until: number;
}

/** How long a toast stays, by kind: an error a little longer. */
const LIFE_MS: Record<ToastKind, number> = { ok: 5000, info: 5000, warn: 7000, err: 8000 };
/** A toast with the same title and detail within this is refreshed, not stacked. */
const SAME_WINDOW_MS = 10000;
/** More than this and the oldest go. */
export const MAX_TOASTS = 4;

export const toasts = writable<Toast[]>([]);

let nextId = 1;
let timer: ReturnType<typeof setTimeout> | null = null;

function schedule() {
  if (timer) clearTimeout(timer);
  timer = null;
  const list = get(toasts);
  if (list.length === 0) return;
  const soonest = Math.min(...list.map((t) => t.until));
  timer = setTimeout(sweep, Math.max(0, soonest - Date.now()));
}

function sweep() {
  const now = Date.now();
  toasts.update((list) => list.filter((t) => t.until > now));
  schedule();
}

export function toast(t: Omit<Toast, "id" | "until">): number {
  const now = Date.now();
  const until = now + LIFE_MS[t.kind];
  let id = 0;
  toasts.update((list) => {
    const same = list.find(
      (x) => x.title === t.title && x.detail === t.detail && x.until - LIFE_MS[x.kind] > now - SAME_WINDOW_MS,
    );
    if (same) {
      id = same.id;
      return list.map((x) => (x === same ? { ...x, ...t, until } : x));
    }
    id = nextId++;
    return [...list, { ...t, id, until }].slice(-MAX_TOASTS);
  });
  schedule();
  return id;
}

export function dismissToast(id: number) {
  toasts.update((list) => list.filter((t) => t.id !== id));
  schedule();
}
