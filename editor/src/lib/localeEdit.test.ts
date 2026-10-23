import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  bestMatch,
  hasKey,
  keys,
  languageFor,
  languages,
  localized,
  missingByLanguage,
  scriptKeys,
  textKeysIn,
  translate,
  type Tables,
} from "./localeEdit";

// The engine's own answers (`locale_lookup_matches_the_editor`).
const fixture = JSON.parse(
  readFileSync(new URL("../../../crates/jengine-core/tests/fixtures/locale_lookup.json", import.meta.url), "utf-8"),
) as { tables: Tables; cases: { wanted: string; key: string; lang: string; text: string | null }[] };

describe("lookups match the engine's", () => {
  for (const c of fixture.cases) {
    it(`${c.wanted}: ${c.key}`, () => {
      const lang = languageFor(fixture.tables, c.wanted);
      expect(lang).toBe(c.lang);
      expect(translate(fixture.tables, lang, c.key)).toBe(c.text);
    });
  }
});

describe("the report", () => {
  const t = fixture.tables;
  it("reads languages and keys from the string tables only", () => {
    expect(languages(t)).toEqual(["en", "fi", "pt-BR"]);
    expect(keys(t)).toEqual(["_name", "door", "only_en", "play", "title"]);
    expect(bestMatch(["en", "pt-BR"], "PT")).toBe("pt-BR");
  });

  it("lists what each language still lacks", () => {
    expect(missingByLanguage(t)).toEqual({ en: [], fi: ["only_en"], "pt-BR": ["door", "only_en"] });
  });

  it("finds the keys scenes and scripts use", () => {
    expect(textKeysIn([{ components: { text: { text_key: "title" } } }, { components: { ui_text: { text_key: "play" }, text: {} } }])).toEqual([
      "play",
      "title",
    ]);
    expect(scriptKeys(`local s = Locale.get("greeting", { name = n })\nLocale.get('rooms', {n=2})`)).toEqual(["greeting", "rooms"]);
    expect(hasKey({ "locale/a": [{ id: "rooms#other", en: "x" }] }, "rooms")).toBe(true);
    expect(hasKey(t, "nope")).toBe(false);
  });

  it("writes text_key text in a language without touching the scene", () => {
    const scene = [{ name: "Sign", components: { text: { text_key: "title", text: "?" } } }, { name: "Plain", components: { text: { text: "x" } } }];
    const out = localized(scene, t, "fi");
    expect((out[0].components.text as { text: string }).text).toBe("Majatalo");
    expect(out[1]).toBe(scene[1]);
    expect(scene[0].components.text.text).toBe("?");
  });
});
