import { describe, expect, it } from "vitest";
import { sceneUnderAssets } from "./runStore";

describe("Build & Run", () => {
  it("starts the open scene by its path under assets/", () => {
    expect(sceneUnderAssets("C:\\Work\\game", "C:\\Work\\game\\assets\\scenes\\title.scene")).toBe("scenes/title.scene");
    expect(sceneUnderAssets("C:/Work/game/", "c:/work/game/assets/a.scene")).toBe("a.scene");
  });
  it("refuses a scene outside the project's assets", () => {
    expect(sceneUnderAssets("C:/Work/game", "C:/Work/other/assets/a.scene")).toBeNull();
    expect(sceneUnderAssets("C:/Work/game", null)).toBeNull();
  });
});
