/**
 * String tables (`locale/*.data`, Rust `locale`) as the editor reads them:
 * a row per key, a column per language. Used by the localisation panel's
 * report (what is missing in which language, which keys the project uses
 * that no table has) and by its **language preview**, which writes each
 * `text_key`'s text in the chosen language into the scene the viewport shows
 * - the lookup the engine does on load, mirrored here and checked against it
 * on one fixture (`crates/jengine-core/tests/fixtures/locale_lookup.json`).
 */

export type Row = { id: string } & Record<string, unknown>;
/** Table id -> rows. Only ids under `locale/` are string tables. */
export type Tables = Record<string, Row[]>;

const PREFIX = "locale/";

/** The string tables, in id order - the first to have a key wins. */
export function stringTables(tables: Tables): Row[][] {
  return Object.keys(tables)
    .filter((id) => id.startsWith(PREFIX))
    .sort()
    .map((id) => tables[id]);
}

/** Every language the tables carry (read from the rows), alphabetical. */
export function languages(tables: Tables): string[] {
  const out = new Set<string>();
  for (const rows of stringTables(tables)) for (const r of rows) for (const k of Object.keys(r)) if (k !== "id") out.add(k);
  return [...out].sort();
}

const base = (code: string) => code.split(/[-_]/)[0];

/** The best of `available` for `wanted`: itself, its base, a regional form
 *  of that base - case-insensitively. */
export function bestMatch(available: string[], wanted: string): string | null {
  const w = wanted.toLowerCase();
  const exact = available.find((c) => c.toLowerCase() === w);
  if (exact) return exact;
  const b = base(w);
  return available.find((c) => c.toLowerCase() === b) ?? available.find((c) => base(c.toLowerCase()) === b) ?? null;
}

/** The language a choice of `wanted` uses (a code no table has still
 *  answers, its lookups falling back). */
export function languageFor(tables: Tables, wanted: string): string {
  return bestMatch(languages(tables), wanted) ?? wanted;
}

/** The languages a lookup in `lang` tries, best first: itself, its base,
 *  English, the first code. */
export function chain(tables: Tables, lang: string): string[] {
  const available = languages(tables);
  const out = [lang];
  for (const c of [base(lang), bestMatch(available, "en"), available[0]]) {
    if (c && !out.includes(c)) out.push(c);
  }
  return out;
}

function cell(tables: Row[][], key: string, lang: string): string | null {
  for (const rows of tables) {
    const v = rows.find((r) => r.id === key)?.[lang];
    if (typeof v === "string" && v !== "") return v;
  }
  return null;
}

/** `key` in `lang` with its fallbacks, or null when no table has it. */
export function translate(tables: Tables, lang: string, key: string): string | null {
  const t = stringTables(tables);
  for (const l of chain(tables, lang)) {
    const v = cell(t, key, l);
    if (v !== null) return v;
  }
  return null;
}

// ─── The report ─────────────────────────────────────────────────────────

/** Every key the tables define (plural forms by their base key too). */
export function keys(tables: Tables): string[] {
  const out = new Set<string>();
  for (const rows of stringTables(tables)) for (const r of rows) out.add(r.id);
  return [...out].sort();
}

/** For each language, the keys with no text in it (not counting `_`
 *  settings such as `_decimal`) - what a translator still has to do. */
export function missingByLanguage(tables: Tables): Record<string, string[]> {
  const t = stringTables(tables);
  const out: Record<string, string[]> = {};
  for (const lang of languages(tables)) {
    out[lang] = keys(tables).filter((k) => !k.startsWith("_") && cell(t, k, lang) === null);
  }
  return out;
}

/** Whether `key` resolves - itself, or as a plural family (`key#other`). */
export function hasKey(tables: Tables, key: string): boolean {
  const all = keys(tables);
  return all.includes(key) || all.includes(`${key}#other`);
}

/** The `text_key`s a scene (or prefab) uses: on `text` and `ui_text`. */
export function textKeysIn(entities: { components?: Record<string, unknown> }[]): string[] {
  const out = new Set<string>();
  for (const e of entities) {
    for (const comp of ["text", "ui_text"]) {
      const k = (e.components?.[comp] as { text_key?: unknown } | undefined)?.text_key;
      if (typeof k === "string" && k) out.add(k);
    }
  }
  return [...out].sort();
}

/** The keys a script asks for by a literal string: `Locale.get("key", ...)`. */
export function scriptKeys(source: string): string[] {
  const out = new Set<string>();
  for (const m of source.matchAll(/Locale\.get\(\s*["']([^"']+)["']/g)) out.add(m[1]);
  return [...out].sort();
}

/** Entities with each `text_key`'s text written in `lang` - what the scene
 *  view shows while previewing a language. The entities given are not
 *  changed; a key no table has is left as the text it had. */
export function localized<E extends { components?: Record<string, unknown> }>(entities: E[], tables: Tables, lang: string): E[] {
  return entities.map((e) => {
    let out = e;
    for (const comp of ["text", "ui_text"]) {
      const c = e.components?.[comp] as { text_key?: unknown; text?: unknown } | undefined;
      if (!c || typeof c.text_key !== "string" || !c.text_key) continue;
      const text = translate(tables, lang, c.text_key);
      if (text === null) continue;
      if (out === e) out = { ...e, components: { ...e.components } };
      (out.components as Record<string, unknown>)[comp] = { ...c, text };
    }
    return out;
  });
}
