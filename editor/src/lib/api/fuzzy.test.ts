import { describe, it, expect } from "vitest";
import { fuzzyMatch } from "./fuzzy";

/** A slice of a real project, so ranking is judged against realistic paths. */
const FILES = [
  "scenes/11-text.scene",
  "scripts/lua/scene_11_text.lua",
  "scenes/01-transforms.scene",
  "scenes/65-custom-fonts.scene",
  "scripts/lua/player.lua",
  "scripts/lua/hub.lua",
  "ui/hub.gui",
  "fonts/bebasneue-regular.font",
  "sprites/player_idle.png",
];

/** Paths that match, best first — what the palette actually shows. */
function ranked(query: string): string[] {
  return FILES.map((path) => ({ path, m: fuzzyMatch(query, path) }))
    .filter((x) => x.m !== null)
    .sort((a, b) => b.m!.score - a.m!.score || a.path.localeCompare(b.path))
    .map((x) => x.path);
}

describe("fuzzyMatch", () => {
  it("ranks a typed filename first", () => {
    expect(ranked("player")[0]).toBe("scripts/lua/player.lua");
  });

  it("finds the file a greedy scan would miss", () => {
    // `pl` against `scripts/lua/player.lua`: matching left-to-right takes the
    // `p` in *scripts* and the `l` in *lua*, scoring a scattered match and
    // highlighting the wrong letters, while the obvious `pl` in `player` is
    // never considered. The basename pass is what fixes this.
    expect(ranked("pl")[0]).toBe("scripts/lua/player.lua");
  });

  it("highlights the characters it actually matched", () => {
    const m = fuzzyMatch("player", "scripts/lua/player.lua")!;
    const spelled = m.hit.map((i) => "scripts/lua/player.lua"[i]).join("");
    expect(spelled).toBe("player");
  });

  it("returns null when the query is not a subsequence", () => {
    expect(fuzzyMatch("zzz", "scripts/lua/player.lua")).toBeNull();
  });

  it("is case-insensitive", () => {
    expect(fuzzyMatch("PLAYER", "scripts/lua/player.lua")).not.toBeNull();
  });

  it("keeps every hit index inside the path", () => {
    for (const f of FILES) {
      const m = fuzzyMatch("a", f);
      if (!m) continue;
      expect(m.hit.every((i) => i >= 0 && i < f.length)).toBe(true);
    }
  });

  describe("multi-term queries", () => {
    it("matches terms in any order", () => {
      // The reported bug: `11 scene` found nothing, because the whole string
      // including the space was matched as one subsequence and no path
      // contains a literal space.
      expect(ranked("11 scene")).toContain("scenes/11-text.scene");
      expect(ranked("scene 11")).toContain("scenes/11-text.scene");
    });

    it("requires every term (AND, not OR)", () => {
      // Adding a word must narrow. If this became OR, typing more would
      // return more, which is the opposite of what anyone expects.
      expect(ranked("11 zzz")).toHaveLength(0);
    });

    it("ignores a trailing space while typing", () => {
      // Mid-typing the query is `11 `; blanking the list on every space bar
      // would make the palette flicker.
      expect(ranked("11 ")).toEqual(ranked("11"));
    });

    it("treats an all-whitespace query as empty", () => {
      expect(fuzzyMatch("   ", "a/b.lua")).not.toBeNull();
    });

    it("unions the highlights, sorted and in range", () => {
      const path = "scenes/11-text.scene";
      const m = fuzzyMatch("11 scene", path)!;
      const ascending = m.hit.every((v, i, a) => i === 0 || a[i - 1] < v);
      expect(ascending).toBe(true);
      expect(m.hit.every((i) => i >= 0 && i < path.length)).toBe(true);
      const spelled = m.hit.map((i) => path[i]).join("");
      expect(spelled).toContain("11");
      expect(spelled.toLowerCase()).toContain("scene");
    });

    it("works on the command palette's 'Category Title' strings", () => {
      expect(fuzzyMatch("view console", "View Show Console")).not.toBeNull();
    });
  });
});
