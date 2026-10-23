import { derived } from "svelte/store";
import { palette, themeMode } from "./themeStore";

// The palette's colours as plain values, for code that cannot use
// `var(--ed-*)`: a 2D canvas's `fillStyle`, a colour mixed in script. CSS,
// inline styles and SVG attributes should keep using the variables.
//
// Derived from the palette stores, so a canvas that reads `$themeColors` in
// its draw effect redraws when the palette changes. themeStore sets the
// `data-*` attributes in its own subscriber, which runs before this one.

export type ThemeColors = Record<ThemeToken, string>;

export type ThemeToken =
  | "crust" | "mantle" | "base" | "raised" | "hover" | "line" | "line-strong"
  | "ink" | "text" | "muted" | "faint"
  | "accent" | "accent-low" | "accent-high" | "a2" | "a3"
  | "ok" | "warn" | "err" | "on-accent" | "sun1" | "sun2"
  | "cat-1" | "cat-2" | "cat-3" | "cat-4" | "cat-5" | "cat-6" | "cat-7" | "cat-8";

const TOKENS: ThemeToken[] = [
  "crust", "mantle", "base", "raised", "hover", "line", "line-strong",
  "ink", "text", "muted", "faint",
  "accent", "accent-low", "accent-high", "a2", "a3",
  "ok", "warn", "err", "on-accent", "sun1", "sun2",
  "cat-1", "cat-2", "cat-3", "cat-4", "cat-5", "cat-6", "cat-7", "cat-8",
];

// Dusk dark, for a context with no document (unit tests).
const FALLBACK: ThemeColors = {
  crust: "#120a20", mantle: "#170d29", base: "#1b0f2e", raised: "#241439",
  hover: "#2e1a48", line: "#3d2459", "line-strong": "#553374",
  ink: "#fff1e6", text: "#e8d5dc", muted: "#b89bb8", faint: "#8f7193",
  accent: "#ff5d73", "accent-low": "#47193d", "accent-high": "#ffb3bd",
  a2: "#3ee0c5", a3: "#ff9f43", ok: "#3ee0c5", warn: "#ff9f43", err: "#ff6b81",
  "on-accent": "#1b0f2e", sun1: "#ffcf5c", sun2: "#ff4f8b",
  "cat-1": "#ff5d73", "cat-2": "#3ee0c5", "cat-3": "#ff9f43", "cat-4": "#ffd166",
  "cat-5": "#8fb3ff", "cat-6": "#c49bff", "cat-7": "#7fe08a", "cat-8": "#ffb3bd",
};

export function readThemeColors(): ThemeColors {
  if (typeof document === "undefined" || typeof getComputedStyle === "undefined") {
    return { ...FALLBACK };
  }
  const style = getComputedStyle(document.documentElement);
  const out = { ...FALLBACK };
  for (const t of TOKENS) {
    const v = style.getPropertyValue(`--ed-${t}`).trim();
    if (v) out[t] = v;
  }
  return out;
}

export const themeColors = derived([palette, themeMode], () => readThemeColors());

/** `#rrggbb` (or `#rgb`) at an alpha, as `rgba(...)`, for canvas strokes and fills. */
export function withAlpha(hex: string, alpha: number): string {
  let h = hex.trim().replace("#", "");
  if (h.length === 3) h = h.split("").map((c) => c + c).join("");
  const n = parseInt(h.slice(0, 6), 16);
  if (Number.isNaN(n)) return hex;
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}
