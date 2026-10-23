/**
 * `settings.liveops` as Project Settings edits it: the live ops service a
 * built game asks (Rust `jengine_core::liveops`) and the remote config
 * values the game ships with.
 */
import type { LiveOpsSettings } from "./api/project";

/** A parameter name the service accepts (`liveops::valid_id`). */
export function validKey(key: string): boolean {
  return /^[A-Za-z0-9_.-]{1,64}$/.test(key);
}

/** A default as its text box shows it: a string as it is, anything else as JSON. */
export function valueText(v: unknown): string {
  return typeof v === "string" ? v : JSON.stringify(v);
}

/**
 * A text box back into a value: JSON when it reads as JSON (a number, true,
 * a list, a table), else the text as a string - so `10` is a number and
 * `Hello` a string, as a person typing either means.
 */
export function parseValue(text: string): unknown {
  const t = text.trim();
  if (t === "") return "";
  try {
    return JSON.parse(t);
  } catch {
    return text;
  }
}

/** Why these settings would not work, or `null`. */
export function liveOpsProblem(s: LiveOpsSettings | undefined): string | null {
  if (!s) return null;
  const url = (s.url ?? "").trim();
  if (url && !/^https?:\/\//.test(url)) return "the service's address starts with https:// (or http:// on your own machine)";
  if (url && !(s.public_key ?? "").trim().startsWith("jlo-public-"))
    return "with a service, the game needs its public key (jlo-public-...) - the dashboard at /admin/liveops shows it";
  for (const k of Object.keys(s.defaults ?? {})) if (!validKey(k)) return `"${k}" is not a key: letters, digits, _ - .`;
  return null;
}

/** What a save writes: `undefined` when nothing is set, so the key goes. */
export function cleanLiveOps(s: LiveOpsSettings | undefined): LiveOpsSettings | undefined {
  if (!s) return undefined;
  const out: LiveOpsSettings = {};
  for (const f of ["url", "channel", "editor_channel", "public_key", "region"] as const) {
    const v = (s[f] ?? "").trim();
    if (v) out[f] = v;
  }
  const defaults = Object.fromEntries(Object.entries(s.defaults ?? {}).filter(([k]) => k.trim() !== ""));
  if (Object.keys(defaults).length) out.defaults = defaults;
  // Not edited on the page yet, but a save must not erase them.
  const attributes = Object.fromEntries(Object.entries(s.attributes ?? {}).filter(([k]) => k.trim() !== ""));
  if (Object.keys(attributes).length) out.attributes = attributes;
  return Object.keys(out).length ? out : undefined;
}

/** A new default's key: `key`, `key_2`, ... - the first not taken. */
export function newKey(s: LiveOpsSettings | undefined): string {
  const taken = new Set(Object.keys(s?.defaults ?? {}));
  if (!taken.has("key")) return "key";
  for (let i = 2; ; i++) if (!taken.has(`key_${i}`)) return `key_${i}`;
}

/** `defaults` with `from` renamed to `to`, keeping its place. */
export function renameKey(defaults: Record<string, unknown>, from: string, to: string): Record<string, unknown> {
  if (from === to || to in defaults) return defaults;
  return Object.fromEntries(Object.entries(defaults).map(([k, v]) => [k === from ? to : k, v]));
}
