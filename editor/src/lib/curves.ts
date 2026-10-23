/**
 * Curves and gradients as the engine stores them: a list of keys, each
 * `[t, ...values]` with `t` from 0 to 1, read linearly between keys and held
 * flat past the first and last. A curve has one value (`[t, v]` - a line's
 * `width_curve`, a particle's `size_curve`); a gradient four, a linear RGBA
 * colour (`[t, r, g, b, a]` - a line's `gradient`, a particle's
 * `color_curve`). The widgets edit these lists and nothing else, so what they
 * write is exactly what a scene file holds.
 *
 * Every function returns a new list; none mutates its argument.
 */

export type Key = number[];

const clamp01 = (t: number) => Math.min(1, Math.max(0, t));

/** Keys in order of `t`, stable for equal `t`. */
export function sortKeys(keys: Key[]): Key[] {
  return keys
    .map((k, i) => [k, i] as const)
    .sort((a, b) => a[0][0] - b[0][0] || a[1] - b[1])
    .map(([k]) => k);
}

/** The values at `t` (without the `t`), as the engine reads them: linear
 *  between keys, the end keys' values held past them. `fallback` when there
 *  are no keys. */
export function sample(keys: Key[], t: number, fallback: number[]): number[] {
  if (keys.length === 0) return [...fallback];
  const sorted = sortKeys(keys);
  if (t <= sorted[0][0]) return sorted[0].slice(1);
  const last = sorted[sorted.length - 1];
  if (t >= last[0]) return last.slice(1);
  for (let i = 1; i < sorted.length; i++) {
    const a = sorted[i - 1];
    const b = sorted[i];
    if (t <= b[0]) {
      const span = b[0] - a[0];
      const k = span > 0 ? (t - a[0]) / span : 1;
      return a.slice(1).map((v, j) => v + (b[j + 1] - v) * k);
    }
  }
  return last.slice(1);
}

/** Add a key at `t`. With no `values` it takes the curve's own value there,
 *  so adding a key does not change the curve. Returns the new list and the
 *  new key's index in it. */
export function insertKey(
  keys: Key[],
  t: number,
  fallback: number[],
  values?: number[],
): { keys: Key[]; index: number } {
  const key = [clamp01(t), ...(values ?? sample(keys, clamp01(t), fallback))];
  const next = sortKeys([...keys, key]);
  return { keys: next, index: next.indexOf(key) };
}

/** Remove the key at `index`. */
export function removeKey(keys: Key[], index: number): Key[] {
  return keys.filter((_, i) => i !== index);
}

/** Move the key at `index` to `t` (clamped to 0..1) with new `values` (or
 *  its own), keeping the list sorted. Returns the list and where the key
 *  went - a key dragged past its neighbour changes place. */
export function moveKey(
  keys: Key[],
  index: number,
  t: number,
  values?: number[],
): { keys: Key[]; index: number } {
  const old = keys[index];
  if (!old) return { keys, index };
  const key = [clamp01(t), ...(values ?? old.slice(1))];
  const next = sortKeys(keys.map((k, i) => (i === index ? key : k)));
  return { keys: next, index: next.indexOf(key) };
}

/** Parse the text fallback: a JSON list of keys, each `width` numbers
 *  (`t` plus the values). Empty text is no keys. */
export function parseKeys(text: string, width: number): { keys: Key[] } | { error: string } {
  const trimmed = text.trim();
  if (trimmed === "") return { keys: [] };
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch (err) {
    return { error: String((err as Error).message ?? err) };
  }
  const ok =
    Array.isArray(parsed) &&
    parsed.every(
      (k) =>
        Array.isArray(k) &&
        k.length === width &&
        k.every((v) => typeof v === "number" && Number.isFinite(v)),
    );
  if (!ok) return { error: `each key is ${width} numbers: [t, ...]` };
  return { keys: sortKeys(parsed as Key[]) };
}

/** The text fallback for a list of keys: one key a line once there are a
 *  few, so a long gradient stays readable. */
export function formatKeys(keys: Key[]): string {
  if (keys.length === 0) return "";
  const round = (v: number) => Math.round(v * 10000) / 10000;
  const rows = keys.map((k) => JSON.stringify(k.map(round)));
  return rows.length <= 3 ? `[${rows.join(", ")}]` : `[\n  ${rows.join(",\n  ")}\n]`;
}

/** The value range a curve graph shows: 0 to at least 1, widened to the
 *  keys and given headroom so the top key is not on the edge. */
export function valueRange(keys: Key[]): { min: number; max: number } {
  let min = 0;
  let max = 1;
  for (const k of keys) {
    min = Math.min(min, k[1]);
    max = Math.max(max, k[1]);
  }
  if (max > 1) max *= 1.15;
  if (min < 0) min *= 1.15;
  return { min, max };
}

// ─── Colour ──────────────────────────────────────────────────────────────

/** Linear to sRGB, one channel, clamped to 0..1. */
export function toSrgb(c: number): number {
  const v = clamp01(c);
  return v <= 0.0031308 ? v * 12.92 : 1.055 * Math.pow(v, 1 / 2.4) - 0.055;
}

/** sRGB to linear, one channel. */
export function toLinear(c: number): number {
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

/** A linear RGB colour that may be brighter than 1 (an HDR particle) as a
 *  picker colour plus an intensity: `rgb = picked * intensity`. A colour
 *  within 0..1 has an intensity of 1. */
export function splitIntensity(rgb: number[]): { rgb: number[]; intensity: number } {
  const peak = Math.max(rgb[0], rgb[1], rgb[2]);
  if (peak <= 1) return { rgb: rgb.slice(0, 3), intensity: 1 };
  return { rgb: rgb.slice(0, 3).map((c) => c / peak), intensity: peak };
}

/** `#rrggbb` for a linear colour (sRGB on screen, as the picker shows it). */
export function hexOf(rgb: number[]): string {
  return (
    "#" +
    rgb
      .slice(0, 3)
      .map((c) =>
        Math.round(toSrgb(c) * 255)
          .toString(16)
          .padStart(2, "0"),
      )
      .join("")
  );
}

/** The linear colour a `#rrggbb` picker value means. */
export function rgbOf(hex: string): number[] {
  return [1, 3, 5].map((i) => toLinear(parseInt(hex.slice(i, i + 2), 16) / 255));
}

/** A CSS `linear-gradient` drawing a gradient's keys left to right, its
 *  colours in sRGB as the screen shows them. CSS interpolates in sRGB and the
 *  engine in linear light, so stops are added between keys until the two
 *  agree to the eye. */
export function cssGradient(keys: Key[], fallback: number[] = [1, 1, 1, 1]): string {
  const steps = 24;
  const stops: string[] = [];
  for (let i = 0; i <= steps; i++) {
    const t = i / steps;
    const [r, g, b, a] = sample(keys, t, fallback.slice(0, 4));
    const { rgb } = splitIntensity([r, g, b]);
    const css = rgb.map((c) => Math.round(toSrgb(c) * 255));
    stops.push(`rgba(${css[0]}, ${css[1]}, ${css[2]}, ${clamp01(a).toFixed(3)}) ${(t * 100).toFixed(1)}%`);
  }
  return `linear-gradient(to right, ${stops.join(", ")})`;
}
