/**
 * Device preview: what a phone's shape does to a game.
 *
 * A phone is wider than 16:9, and part of it is covered - a notch, a camera
 * hole, a pill-shaped island, rounded corners, a home bar. A profile here is
 * one such screen, and the editor uses it three ways: the Game view and the
 * GUI editor letterbox to its shape, draw what covers it, and hand the engine
 * its safe area (`viewport_set_safe_area`) so the UI and on-screen controls
 * move out of the way exactly as the engine does on the device
 * (`ui::scaling::SafeInsets`).
 *
 * Geometry is written for landscape with the camera on the **left** short
 * edge, in fractions of the screen's short side `S` - the unit notches are
 * specified in. Portrait turns it a quarter so the camera is on top.
 *
 * The numbers are representative, not any one model's exact figures: a
 * 20:9 Android phone with a punch-hole, a 19.5:9 phone with a notch and one
 * with an island, a plain 16:9 phone and a 4:3 tablet.
 */

export type Cutout =
  | { kind: "hole"; diameter: number; inset: number }
  | { kind: "notch"; length: number; depth: number }
  | { kind: "island"; length: number; thickness: number; inset: number };

export interface DeviceProfile {
  id: string;
  label: string;
  /** Long side over short side. */
  aspect: number;
  cutout: Cutout | null;
  /** Safe insets in landscape, fractions of the short side. */
  safe: { left: number; right: number; bottom: number };
  /** Screen corner radius, a fraction of the short side. */
  corner: number;
}

export const DEVICE_PROFILES: DeviceProfile[] = [
  {
    id: "phone-hole",
    label: "Phone 20:9 · punch-hole",
    aspect: 20 / 9,
    cutout: { kind: "hole", diameter: 0.035, inset: 0.03 },
    // What Android reports for one: 132 px on a 2400 x 1080 screen.
    safe: { left: 0.122, right: 0, bottom: 0 },
    corner: 0.06,
  },
  {
    id: "phone-notch",
    label: "Phone 19.5:9 · notch",
    aspect: 19.5 / 9,
    cutout: { kind: "notch", length: 0.55, depth: 0.08 },
    // iOS keeps both sides safe in landscape, and the home bar.
    safe: { left: 0.117, right: 0.117, bottom: 0.056 },
    corner: 0.1,
  },
  {
    id: "phone-island",
    label: "Phone 19.5:9 · island",
    aspect: 2556 / 1179,
    cutout: { kind: "island", length: 0.32, thickness: 0.095, inset: 0.028 },
    safe: { left: 0.15, right: 0.15, bottom: 0.054 },
    corner: 0.14,
  },
  {
    id: "phone-plain",
    label: "Phone 16:9 · no cut-out",
    aspect: 16 / 9,
    cutout: null,
    safe: { left: 0, right: 0, bottom: 0 },
    corner: 0.03,
  },
  {
    id: "tablet",
    label: "Tablet 4:3",
    aspect: 4 / 3,
    cutout: null,
    safe: { left: 0, right: 0, bottom: 0.02 },
    corner: 0.04,
  },
];

export function profileById(id: string | null | undefined): DeviceProfile | null {
  return DEVICE_PROFILES.find((p) => p.id === id) ?? null;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The screen's width over height in the given orientation. */
export function screenAspect(p: DeviceProfile, portrait: boolean): number {
  return portrait ? 1 / p.aspect : p.aspect;
}

/** The largest rect of `aspect` (w / h) centred in a container. */
export function fitRect(containerW: number, containerH: number, aspect: number): Rect {
  if (containerW <= 0 || containerH <= 0 || aspect <= 0) {
    return { x: 0, y: 0, width: Math.max(0, containerW), height: Math.max(0, containerH) };
  }
  let width = containerW;
  let height = width / aspect;
  if (height > containerH) {
    height = containerH;
    width = height * aspect;
  }
  // Centred on the whole-pixel size, so the margins differ by at most one.
  const w = Math.floor(width);
  const h = Math.floor(height);
  return { x: Math.floor((containerW - w) / 2), y: Math.floor((containerH - h) / 2), width: w, height: h };
}

/**
 * Safe insets as fractions of the screen's own width and height (left, top,
 * right, bottom) - what `viewport_set_safe_area` takes. Portrait turns the
 * landscape insets a quarter: the camera's left edge becomes the top.
 */
export function safeFractions(
  p: DeviceProfile,
  portrait: boolean,
): { left: number; top: number; right: number; bottom: number } {
  // In landscape the short side is the height: a fraction of S along x is
  // that over the aspect as a fraction of the width.
  const { left, right, bottom } = p.safe;
  if (!portrait) {
    return { left: left / p.aspect, top: 0, right: right / p.aspect, bottom };
  }
  // Portrait: the short side is the width. Landscape left -> top, right ->
  // bottom, bottom (home bar) -> right edge... except a home bar stays at the
  // bottom of the device, which in portrait is the short edge opposite the
  // camera - so it joins `right` there.
  return {
    left: 0,
    top: left / p.aspect,
    right: 0,
    bottom: Math.max(right, bottom) / p.aspect,
  };
}

/** The safe area in pixels of a `w` x `h` screen. */
export function safeRect(p: DeviceProfile, portrait: boolean, w: number, h: number): Rect {
  const f = safeFractions(p, portrait);
  return {
    x: f.left * w,
    y: f.top * h,
    width: w * (1 - f.left - f.right),
    height: h * (1 - f.top - f.bottom),
  };
}

/** What covers a `w` x `h` screen: cut-out shapes to fill, in pixels. */
export interface CoverShape {
  x: number;
  y: number;
  width: number;
  height: number;
  /** Corner radius; a circle is a square with radius half its side. */
  radius: number;
}

export function cutoutShapes(p: DeviceProfile, portrait: boolean, w: number, h: number): CoverShape[] {
  const c = p.cutout;
  if (!c) return [];
  const s = Math.min(w, h);
  // Built in landscape (camera on the left edge, centred along it), then
  // turned for portrait (camera on the top edge).
  let shape: CoverShape;
  if (c.kind === "hole") {
    const d = c.diameter * s;
    shape = { x: c.inset * s, y: (s - d) / 2, width: d, height: d, radius: d / 2 };
  } else if (c.kind === "notch") {
    const len = c.length * s;
    const dep = c.depth * s;
    shape = { x: -dep * 0.25, y: (s - len) / 2, width: dep * 1.25, height: len, radius: dep * 0.45 };
  } else {
    const len = c.length * s;
    const t = c.thickness * s;
    shape = { x: c.inset * s, y: (s - len) / 2, width: t, height: len, radius: t / 2 };
  }
  if (!portrait) return [shape];
  // Landscape (x across the long side from the camera edge, y along it) to
  // portrait (camera edge on top): swap axes.
  return [{ x: shape.y, y: shape.x, width: shape.height, height: shape.width, radius: shape.radius }];
}

export function cornerRadius(p: DeviceProfile, w: number, h: number): number {
  return p.corner * Math.min(w, h);
}
