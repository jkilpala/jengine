/**
 * `settings.asset_packs` as Project Settings edits it: parts of the game a
 * build moves out into downloadable archives (Rust `asset::packs`, the
 * builder's `asset_packs`).
 */
import type { AssetPackSettings } from "./api/project";

/** The rule `asset::packs::valid_name` applies: it names a file. */
export function validPackName(name: string): boolean {
  return /^[a-z0-9_-]{1,64}$/.test(name);
}

/** A pack's include paths as the text box shows them, one per line. */
export function includeText(include: string[]): string {
  return include.join("\n");
}

/** The text box back into include paths: trimmed, `\` as `/`, blanks gone. */
export function parseIncludes(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim().replace(/\\/g, "/"))
    .filter((l) => l.length > 0);
}

/**
 * Why the packs would fail a build, or `null`: a bad or repeated name. (A
 * file claimed by two packs is caught by the build, which sees the files.)
 */
export function packProblem(settings: AssetPackSettings | undefined): string | null {
  const seen = new Set<string>();
  for (const p of settings?.packs ?? []) {
    if (!validPackName(p.name)) return `"${p.name}" is not a pack name: use lowercase letters, digits, - and _`;
    if (seen.has(p.name)) return `two packs are named "${p.name}"`;
    seen.add(p.name);
  }
  return null;
}

/** What a save writes: `undefined` when nothing is set, so the key goes. */
export function cleanPacks(settings: AssetPackSettings | undefined): AssetPackSettings | undefined {
  if (!settings) return undefined;
  const url = (settings.url ?? "").trim();
  const packs = (settings.packs ?? []).map((p) => ({ name: p.name.trim(), include: parseIncludes(p.include.join("\n")) }));
  if (!url && packs.length === 0) return undefined;
  const out: AssetPackSettings = {};
  if (url) out.url = url;
  if (packs.length) out.packs = packs;
  return out;
}

/** A new pack's name: `pack`, `pack-2`, ... - the first not taken. */
export function newPackName(settings: AssetPackSettings | undefined): string {
  const taken = new Set((settings?.packs ?? []).map((p) => p.name));
  if (!taken.has("pack")) return "pack";
  for (let i = 2; ; i++) if (!taken.has(`pack-${i}`)) return `pack-${i}`;
}
