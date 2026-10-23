import { describe, it, expect } from "vitest";
import { canOpenAsset, entryScenePath } from "./openAsset";
import type { ProjectInfo } from "./project";

/**
 * `canOpenAsset` decides whether Ctrl+P greys a row out and whether a
 * find-in-files hit is clickable, so it has to agree with the switch in
 * `openAsset` itself. A type handled there but missing here is a file the
 * editor can open and refuses to.
 */
describe("canOpenAsset", () => {
  it("accepts every type the dispatch handles", () => {
    for (const path of [
      "assets/scenes/a.scene",
      "assets/prefabs/a.prefab",
      "assets/scripts/a.lua",
      "assets/scripts/a.js",
      "assets/ui/a.gui",
      "assets/atlases/a.atlas",
      "assets/shaders/a.shader",
      "assets/fx/a.particle",
      "assets/anim/a.anim",
      "assets/fonts/a.font",
      "assets/maps/a.tilemap",
      "assets/mat/a.material",
      "assets/ai/a.behavior",
      "assets/a.sortlayers",
      "assets/data/a.dataschema",
      "assets/data/a.data",
      "assets/sfx/a.ogg",
      "assets/sfx/a.wav",
      "assets/sprites/a.png",
      "assets/sprites/a.jpg",
    ]) {
      expect(canOpenAsset(path), path).toBe(true);
    }
  });

  it("rejects types with no editor", () => {
    expect(canOpenAsset("assets/readme.txt")).toBe(false);
    expect(canOpenAsset("assets/blob.bin")).toBe(false);
  });

  it("sends a Rust package's files to the code editor", () => {
    expect(canOpenAsset("packages/tide-rules/src/lib.rs")).toBe(true);
    expect(canOpenAsset("packages/tide-rules/Cargo.toml")).toBe(true);
    expect(canOpenAsset("packages/tide-rules/README.md")).toBe(true);
  });

  it("is case-insensitive about the extension", () => {
    expect(canOpenAsset("assets/A.PNG")).toBe(true);
    expect(canOpenAsset("assets/A.Scene")).toBe(true);
  });

  it("treats a file with no extension as unopenable", () => {
    expect(canOpenAsset("assets/LICENSE")).toBe(false);
  });

  it("does not read a dot in a directory as an extension", () => {
    // `assets/v1.2/readme` has no extension; taking `2/readme` as one would
    // make an arbitrary file look openable.
    expect(canOpenAsset("assets/v1.2/readme")).toBe(false);
    expect(canOpenAsset("assets/v1.2/a.scene")).toBe(true);
  });
});

describe("entryScenePath", () => {
  const project = (settings: unknown, legacy: string | null = null) =>
    ({ path: "/p", settings, default_scene: legacy }) as unknown as ProjectInfo;

  it("prefixes the stored path with assets/", () => {
    // `entry_scene` is stored relative to `assets/`, matching how the runtime
    // resolves it; `openAsset` wants a project-root-relative path.
    expect(entryScenePath(project({ entry_scene: "scenes/hub.scene" }))).toBe(
      "assets/scenes/hub.scene",
    );
  });

  it("falls back to the legacy top-level default_scene", () => {
    expect(entryScenePath(project({}, "scenes/old.scene"))).toBe(
      "assets/scenes/old.scene",
    );
  });

  it("prefers entry_scene over the legacy field", () => {
    expect(
      entryScenePath(project({ entry_scene: "scenes/new.scene" }, "scenes/old.scene")),
    ).toBe("assets/scenes/new.scene");
  });

  it("returns null when no entry scene is set", () => {
    expect(entryScenePath(project({}))).toBeNull();
  });
});
