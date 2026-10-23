// The scene's post-process settings as the Inspector edits them.
//
// One table - `PP_EFFECTS` - says which parameters each effect has, under
// the names the renderer reads (`crates/jengine-core/src/renderer/backend/
// wgpu/postprocess.rs`), with the renderer's defaults. The Inspector draws
// its sliders from it rather than listing them by hand, which is how every
// slider came to write a `u_`-prefixed key the renderer never read: moving
// one did nothing, for as long as the panel existed.
// `editor_post_params_match_the_renderer` (a jengine-core test) reads this
// file and fails on a name the renderer does not know.

import type { PostProcessEffectSettings, PostProcessSettings } from "./api/scene";

export interface PpParam {
  key: string;
  label: string;
  min: number;
  max: number;
  step: number;
  default: number;
  decimals: number;
}

export interface PpEffectSpec {
  name: string;
  label: string;
  params: PpParam[];
  /** A `Vec4` colour parameter, edited with a colour picker. */
  color?: { key: string; label: string; default: [number, number, number, number] };
}

export const PP_EFFECTS: PpEffectSpec[] = [
  {
    name: "bloom",
    label: "Bloom",
    params: [
      { key: "threshold", label: "Threshold", min: 0, max: 4, step: 0.05, default: 0.8, decimals: 2 },
      { key: "knee", label: "Soft knee", min: 0, max: 1, step: 0.05, default: 0.5, decimals: 2 },
      { key: "intensity", label: "Intensity", min: 0, max: 4, step: 0.05, default: 0.8, decimals: 2 },
      { key: "scatter", label: "Scatter", min: 0, max: 1, step: 0.05, default: 0.7, decimals: 2 },
    ],
  },
  {
    name: "vignette",
    label: "Vignette",
    params: [
      { key: "intensity", label: "Intensity", min: 0, max: 1, step: 0.02, default: 0.5, decimals: 2 },
      { key: "smoothness", label: "Smoothness", min: 0, max: 1, step: 0.02, default: 0.4, decimals: 2 },
    ],
    color: { key: "color", label: "Colour", default: [0, 0, 0, 1] },
  },
  {
    name: "color_grade",
    label: "Color Grading",
    params: [
      { key: "brightness", label: "Brightness", min: 0, max: 3, step: 0.02, default: 1, decimals: 2 },
      { key: "contrast", label: "Contrast", min: 0, max: 2, step: 0.02, default: 1, decimals: 2 },
      { key: "saturation", label: "Saturation", min: 0, max: 2, step: 0.02, default: 1, decimals: 2 },
    ],
    // Alpha is the tint's strength: 0 leaves the image alone.
    color: { key: "tint_color", label: "Tint", default: [1, 1, 1, 0] },
  },
  {
    name: "chromatic",
    label: "Chromatic Aberration",
    params: [
      { key: "strength", label: "Strength", min: 0, max: 0.02, step: 0.0005, default: 0.005, decimals: 4 },
    ],
  },
  {
    // The frame point-sampled in cells of `height / lines` pixels, on a grid
    // anchored to the world: pixel art drawn at any window size.
    name: "pixelate",
    label: "Pixelate",
    params: [
      { key: "lines", label: "Lines", min: 45, max: 720, step: 1, default: 180, decimals: 0 },
    ],
  },
  {
    // Film grain over the finished frame, after the tonemap: a new pattern
    // every frame, gentler in the highlights.
    name: "film_grain",
    label: "Film Grain",
    params: [
      { key: "intensity", label: "Intensity", min: 0, max: 0.5, step: 0.01, default: 0.08, decimals: 2 },
      { key: "size", label: "Size (px)", min: 1, max: 6, step: 0.5, default: 1.5, decimals: 1 },
    ],
  },
  {
    // Anti-aliasing over the finished frame, after the tonemap: stair-stepped
    // edges (a turned sprite, a thin line) blurred along themselves.
    name: "fxaa",
    label: "Anti-aliasing (FXAA)",
    params: [
      { key: "span", label: "Span (texels)", min: 1, max: 16, step: 1, default: 8, decimals: 0 },
    ],
  },
  {
    // The frame quantised to a palette with ordered dithering, last in the
    // output pass (`renderer::palette`). Presets: 0 Game Boy, 1 PICO-8,
    // 2 CGA, 3 Sweetie 16, 4 EGA; a custom list (`count`, `c0`..) is set
    // from a script or the scene file.
    name: "palette",
    label: "Palette (retro)",
    params: [
      { key: "preset", label: "Preset", min: 0, max: 4, step: 1, default: 1, decimals: 0 },
      { key: "dither", label: "Dither", min: 0, max: 2, step: 0.05, default: 0.6, decimals: 2 },
      { key: "cell", label: "Cell (px)", min: 1, max: 8, step: 1, default: 2, decimals: 0 },
      { key: "cycle_start", label: "Cycle From", min: 0, max: 16, step: 1, default: 0, decimals: 0 },
      { key: "cycle_end", label: "Cycle To", min: 0, max: 16, step: 1, default: 0, decimals: 0 },
      { key: "cycle_speed", label: "Cycle Speed", min: 0, max: 20, step: 0.5, default: 0, decimals: 1 },
    ],
  },
  {
    // Rain on the lens: beads forming and drying, drops running down and
    // leaving trails, each bending the frame behind it; mist steams the
    // glass except where drops have wiped it. `time` (a script's) fixes it.
    name: "lens_rain",
    label: "Lens Rain",
    params: [
      { key: "amount", label: "Amount", min: 0, max: 1, step: 0.05, default: 0.6, decimals: 2 },
      { key: "size", label: "Drop size (px)", min: 2, max: 40, step: 1, default: 10, decimals: 0 },
      { key: "speed", label: "Speed", min: 0, max: 4, step: 0.1, default: 1, decimals: 1 },
      { key: "mist", label: "Mist", min: 0, max: 1, step: 0.05, default: 0, decimals: 2 },
      { key: "refraction", label: "Refraction", min: 0, max: 4, step: 0.1, default: 1, decimals: 1 },
    ],
  },
  {
    // Line boil: the frame read through a smooth noise offset, redrawn
    // `rate` times a second, so every line wobbles as if drawn again.
    // `seed` (a script's) fixes the pattern.
    name: "boil",
    label: "Line Boil",
    params: [
      { key: "amount", label: "Amount (px)", min: 0, max: 6, step: 0.25, default: 1.5, decimals: 2 },
      { key: "scale", label: "Scale (px)", min: 4, max: 96, step: 1, default: 24, decimals: 0 },
      { key: "rate", label: "Rate (per s)", min: 1, max: 24, step: 1, default: 8, decimals: 0 },
    ],
  },
  {
    // An oil painting: a Kuwahara filter in the output pass - flat areas
    // become strokes, edges stay sharp.
    name: "paint",
    label: "Paint (Kuwahara)",
    params: [
      { key: "radius", label: "Radius (texels)", min: 1, max: 16, step: 1, default: 4, decimals: 0 },
      { key: "strength", label: "Strength", min: 0, max: 1, step: 0.05, default: 1, decimals: 2 },
    ],
  },
  {
    // A print: dots on a turned screen, sized by the colour under them.
    // Mode 0 is one screen of ink on paper, 1 cyan / magenta / yellow /
    // black. `paper` (a colour) is set from a script or the scene file.
    name: "halftone",
    label: "Halftone",
    params: [
      { key: "mode", label: "Mode (0 mono, 1 CMYK)", min: 0, max: 1, step: 1, default: 0, decimals: 0 },
      { key: "cell", label: "Cell (px)", min: 2, max: 32, step: 1, default: 6, decimals: 0 },
      { key: "angle", label: "Angle (deg)", min: 0, max: 90, step: 1, default: 45, decimals: 0 },
      { key: "strength", label: "Strength", min: 0, max: 1, step: 0.05, default: 1, decimals: 2 },
    ],
    color: { key: "ink", label: "Ink", default: [0.02, 0.02, 0.03, 1] },
  },
  {
    name: "crt",
    label: "CRT",
    params: [
      { key: "lines", label: "Scanlines", min: 45, max: 720, step: 1, default: 180, decimals: 0 },
      { key: "scanline", label: "Line Gap", min: 0, max: 1, step: 0.01, default: 0.4, decimals: 2 },
      { key: "mask", label: "RGB Mask", min: 0, max: 1, step: 0.01, default: 0.2, decimals: 2 },
      { key: "curvature", label: "Curvature", min: 0, max: 0.2, step: 0.005, default: 0.05, decimals: 3 },
    ],
  },
  {
    // Light scattered from a source through the gaps in what stands before
    // it: god rays. The source is a world point.
    name: "light_shafts",
    label: "Light Shafts",
    params: [
      { key: "source_x", label: "Source X", min: -20000, max: 20000, step: 10, default: 0, decimals: 0 },
      { key: "source_y", label: "Source Y", min: -20000, max: 20000, step: 10, default: 1000, decimals: 0 },
      { key: "length", label: "Length", min: 0.05, max: 4, step: 0.05, default: 1, decimals: 2 },
      { key: "intensity", label: "Intensity", min: 0, max: 4, step: 0.05, default: 0.8, decimals: 2 },
      { key: "decay", label: "Decay", min: 0.8, max: 1, step: 0.005, default: 0.97, decimals: 3 },
      { key: "threshold", label: "Threshold", min: 0, max: 0.99, step: 0.01, default: 0.6, decimals: 2 },
    ],
    color: { key: "color", label: "Colour", default: [1, 0.95, 0.85, 1] },
  },
  {
    // The reflections a bright light leaves in a lens: ghosts across the
    // frame through its centre, a halo ring, colour fringes, and an optional
    // horizontal anamorphic streak.
    name: "lens_flare",
    label: "Lens Flare",
    params: [
      { key: "threshold", label: "Threshold", min: 0, max: 8, step: 0.05, default: 1, decimals: 2 },
      { key: "intensity", label: "Intensity", min: 0, max: 4, step: 0.05, default: 0.6, decimals: 2 },
      { key: "ghosts", label: "Ghosts", min: 0, max: 8, step: 1, default: 5, decimals: 0 },
      { key: "spacing", label: "Spacing", min: 0, max: 2, step: 0.01, default: 0.35, decimals: 2 },
      { key: "halo", label: "Halo", min: 0, max: 1, step: 0.01, default: 0.45, decimals: 2 },
      { key: "halo_width", label: "Halo Width", min: 0.001, max: 0.5, step: 0.005, default: 0.06, decimals: 3 },
      { key: "dispersion", label: "Dispersion", min: 0, max: 0.05, step: 0.001, default: 0.006, decimals: 3 },
      { key: "streak", label: "Streak", min: 0, max: 4, step: 0.05, default: 0, decimals: 2 },
      { key: "streak_length", label: "Streak Length", min: 0, max: 1, step: 0.01, default: 0.25, decimals: 2 },
    ],
    color: { key: "tint", label: "Tint", default: [1, 0.9, 0.8, 1] },
  },
  {
    // Each pixel smeared along its motion across the screen since the last
    // frame - a sprite's own and the camera's (a velocity pass draws it).
    // What the camera follows stays sharp.
    name: "motion_blur",
    label: "Motion Blur",
    params: [
      { key: "shutter", label: "Shutter", min: 0, max: 4, step: 0.05, default: 0.8, decimals: 2 },
      { key: "samples", label: "Samples", min: 2, max: 16, step: 1, default: 12, decimals: 0 },
      { key: "max_length", label: "Max Length", min: 0, max: 0.5, step: 0.005, default: 0.08, decimals: 3 },
    ],
  },
  {
    // Each layer blurred by how far its z is from the one in focus (the
    // velocity pass draws each sprite's z). Pull `focus` to rack it.
    name: "depth_of_field",
    label: "Depth of Field",
    params: [
      { key: "focus", label: "Focus (z)", min: -200, max: 200, step: 0.5, default: 0, decimals: 1 },
      { key: "range", label: "Sharp Range", min: 0, max: 100, step: 0.5, default: 0, decimals: 1 },
      { key: "strength", label: "Blur per z", min: 0, max: 8, step: 0.05, default: 1, decimals: 2 },
      { key: "max_radius", label: "Max Radius", min: 0, max: 32, step: 0.5, default: 12, decimals: 1 },
    ],
  },
  {
    // Sharp in a horizontal band, blurred away from it: a town read as a
    // model. The band is in world units.
    name: "tilt_shift",
    label: "Tilt-Shift",
    params: [
      { key: "focus_y", label: "Focus Y", min: -20000, max: 20000, step: 10, default: 0, decimals: 0 },
      { key: "band", label: "Band", min: 0, max: 2000, step: 5, default: 60, decimals: 0 },
      { key: "falloff", label: "Falloff", min: 0, max: 4000, step: 10, default: 220, decimals: 0 },
      { key: "max_blur", label: "Max Blur", min: 0, max: 48, step: 0.5, default: 10, decimals: 1 },
      { key: "min_blur", label: "Min Blur", min: 0, max: 48, step: 0.5, default: 0, decimals: 1 },
    ],
  },
  {
    // The same disc everywhere: a world softened behind a menu - or, with a
    // focus rectangle (world units; 0 size for none), everywhere but there.
    name: "blur",
    label: "Blur",
    params: [
      { key: "radius", label: "Radius", min: 0, max: 48, step: 0.5, default: 6, decimals: 1 },
      { key: "focus_x", label: "Focus X", min: -20000, max: 20000, step: 10, default: 0, decimals: 0 },
      { key: "focus_y", label: "Focus Y", min: -20000, max: 20000, step: 10, default: 0, decimals: 0 },
      { key: "focus_w", label: "Focus W", min: 0, max: 20000, step: 10, default: 0, decimals: 0 },
      { key: "focus_h", label: "Focus H", min: 0, max: 20000, step: 10, default: 0, decimals: 0 },
      { key: "falloff", label: "Falloff", min: 0, max: 4000, step: 5, default: 40, decimals: 0 },
      { key: "focus_blur", label: "Focus Blur", min: 0, max: 48, step: 0.5, default: 0, decimals: 1 },
    ],
  },
];

export const TONEMAPS = ["aces", "agx", "reinhard", "none"] as const;
export type TonemapName = (typeof TONEMAPS)[number];

export function effectSpec(name: string): PpEffectSpec | undefined {
  return PP_EFFECTS.find((e) => e.name === name);
}

/** Every known effect, off, with the renderer's defaults - what a scene
 *  with no post block shows in the Inspector. */
export function defaultEffects(): PostProcessEffectSettings[] {
  return PP_EFFECTS.map((spec) => {
    const params: Record<string, unknown> = {};
    for (const p of spec.params) params[p.key] = { Float: p.default };
    if (spec.color) params[spec.color.key] = { Vec4: [...spec.color.default] };
    return { name: spec.name, enabled: false, params };
  });
}

/** The scene's effects, plus any known effect it does not list (off), so
 *  every effect has a row to turn on. */
export function effectsFor(pp: PostProcessSettings | undefined): PostProcessEffectSettings[] {
  const own = pp?.effects ?? [];
  const missing = defaultEffects().filter((d) => !own.some((e) => e.name === d.name));
  return [...own.map((e) => ({ ...e, params: { ...e.params } })), ...missing];
}

export function getParam(effect: PostProcessEffectSettings, key: string): number {
  const val = effect.params[key];
  if (val && typeof val === "object" && "Float" in val) return (val as { Float: number }).Float;
  if (typeof val === "number") return val;
  const spec = effectSpec(effect.name)?.params.find((p) => p.key === key);
  return spec?.default ?? 0;
}

export function getColor(effect: PostProcessEffectSettings, key: string): number[] {
  const val = effect.params[key];
  if (val && typeof val === "object" && "Vec4" in val) return (val as { Vec4: number[] }).Vec4;
  if (Array.isArray(val)) return val as number[];
  return [...(effectSpec(effect.name)?.color?.default ?? [1, 1, 1, 1])];
}

/**
 * A new settings block with `effects` replaced and **everything else kept**.
 * The Inspector used to write `{ effects }`, which would drop `hdr`,
 * `tonemap` and `exposure` the first time any slider moved.
 */
export function withEffects(
  pp: PostProcessSettings | undefined,
  effects: PostProcessEffectSettings[],
): PostProcessSettings {
  return { ...(pp ?? {}), effects };
}

export function withParam(
  effects: PostProcessEffectSettings[],
  index: number,
  key: string,
  value: unknown,
): PostProcessEffectSettings[] {
  return effects.map((e, i) => (i === index ? { ...e, params: { ...e.params, [key]: value } } : e));
}

/** HDR, tonemap and exposure, keeping the effects. Turning HDR off removes
 *  the tonemap and exposure too, so an LDR scene saves as it always did. */
export function withHdr(
  pp: PostProcessSettings | undefined,
  hdr: { hdr: boolean; tonemap?: TonemapName; exposure?: number },
): PostProcessSettings {
  const base: PostProcessSettings = { ...(pp ?? {}), effects: pp?.effects ?? [] };
  if (!hdr.hdr) {
    delete base.hdr;
    delete base.tonemap;
    delete base.exposure;
    return base;
  }
  return {
    ...base,
    hdr: true,
    tonemap: hdr.tonemap ?? base.tonemap ?? "aces",
    exposure: hdr.exposure ?? base.exposure ?? 1,
  };
}

/** Linear colour <-> the `#rrggbb` a colour input shows (sRGB), since every
 *  colour the engine draws is linear (CLAUDE.md, "Colours are linear"). */
export function linearToHex(v: number[]): string {
  const enc = (c: number) => {
    const x = Math.min(1, Math.max(0, c));
    const s = x <= 0.0031308 ? x * 12.92 : 1.055 * Math.pow(x, 1 / 2.4) - 0.055;
    return Math.round(s * 255).toString(16).padStart(2, "0");
  };
  return `#${enc(v[0])}${enc(v[1])}${enc(v[2])}`;
}

export function hexToLinear(hex: string, alpha: number): number[] {
  const dec = (i: number) => {
    const s = parseInt(hex.slice(i, i + 2), 16) / 255;
    return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return [dec(1), dec(3), dec(5), alpha];
}
