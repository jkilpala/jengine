/**
 * `settings.app` / `settings.android` helpers for Project Settings.
 *
 * The rules and defaults here mirror `jengine_builder::app`, which is what a
 * build actually uses - the editor shows them so a field left empty says what
 * it will become, and a bad id is caught while typing rather than by the
 * build. Keep the two in step.
 */

/** Package-name rules shared by Android and iOS bundle ids. */
export function validateAppId(id: string): string | null {
  const trimmed = id.trim();
  if (trimmed === "") return null; // unset: the builder makes one up
  const segments = trimmed.split(".");
  if (segments.length < 2) {
    return "needs at least two dot-separated parts, like com.example.game";
  }
  for (const seg of segments) {
    if (!/^[A-Za-z][A-Za-z0-9_]*$/.test(seg)) {
      return `'${seg}' must start with a letter and hold only letters, digits and _`;
    }
  }
  return null;
}

/** The id a build uses when none is set: `com.jengine.<project>`. */
export function defaultAppId(projectName: string): string {
  let seg = projectName
    .split("")
    .map((c) => (/[A-Za-z0-9]/.test(c) ? c.toLowerCase() : "_"))
    .join("");
  if (!/^[a-z]/.test(seg)) seg = "g" + seg;
  return `com.jengine.${seg}`;
}

/** The orientation a build uses when none is set: the project's shape. */
export function defaultOrientation(width: number, height: number): "landscape" | "portrait" {
  return height > width ? "portrait" : "landscape";
}

export const DEFAULT_MIN_SDK = 26;
export const DEFAULT_TARGET_SDK = 36;
export const DEFAULT_VERSION = "1.0.0";

/** ABIs a build can target, with what each is for. */
export const ANDROID_ABIS: { id: string; label: string }[] = [
  { id: "arm64-v8a", label: "arm64-v8a — every phone since ~2017" },
  { id: "x86_64", label: "x86_64 — the emulator on a PC" },
];

/**
 * Drop what was left empty before a save. A cleared number box binds `null`
 * (or `NaN`), which the Rust side's `u32` refuses outright, and an emptied
 * text box binds `""`; both mean "use the default", which is spelled by
 * leaving the key out. Returns `undefined` when nothing is left, so an
 * untouched project writes no `app` / `android` block at all.
 */
export function cleanSettings<T extends object>(value: T | undefined): T | undefined {
  if (!value) return undefined;
  const out: Record<string, unknown> = {};
  for (const [key, v] of Object.entries(value)) {
    if (v == null) continue;
    if (typeof v === "string" && v.trim() === "") continue;
    if (typeof v === "number" && (!Number.isFinite(v) || v <= 0)) continue;
    if (Array.isArray(v) && v.length === 0) continue;
    out[key] = typeof v === "string" ? v.trim() : v;
  }
  return Object.keys(out).length ? (out as T) : undefined;
}
