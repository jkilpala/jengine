import { describe, expect, it } from "vitest";
import { gizmosFor, placeLocal, type WorldXf } from "./gizmoShapes";

const still: WorldXf = { x: 100, y: 50, rotation: 0, scaleX: 1, scaleY: 1 };

describe("scene view gizmos", () => {
  it("places local points scaled, turned and moved", () => {
    const xf: WorldXf = { x: 10, y: 0, rotation: Math.PI / 2, scaleX: 2, scaleY: 1 };
    const [x, y] = placeLocal([1, 0], xf);
    expect(x).toBeCloseTo(10);
    expect(y).toBeCloseTo(2);
  });

  it("draws a light's reach and a spot light's cone where it is aimed", () => {
    const g = gizmosFor({ point_light: { radius: 120, color: [1, 1, 1] } }, still);
    expect(g).toEqual([{ kind: "circle", center: [100, 50], radius: 120, color: "rgb(255, 255, 255)", label: "light" }]);
    // Turned a quarter, aimed along its own +x: the cone points up.
    const spot = gizmosFor({ spot_light: { radius: 100, angle: 0.3, direction: 0 } }, { ...still, rotation: Math.PI / 2 })[0];
    if (spot.kind !== "path") throw new Error("a cone is a path");
    const mid = spot.points[1 + 12];
    expect(mid[0]).toBeCloseTo(100);
    expect(mid[1]).toBeCloseTo(150);
    expect(spot.points[0]).toEqual([100, 50]);
  });

  it("draws a spatial sound's two ranges, and nothing for a flat one", () => {
    expect(gizmosFor({ audio_source: { spatial: false, min_distance: 50, max_distance: 400 } }, still)).toEqual([]);
    const g = gizmosFor({ audio_source: { spatial: true, min_distance: 50, max_distance: 400 } }, still);
    expect(g.map((x) => (x.kind === "circle" ? [x.radius, !!x.dashed] : null))).toEqual([
      [50, false],
      [400, true],
    ]);
  });

  it("places outlines with the entity, but a confine area as it is", () => {
    const xf: WorldXf = { x: 0, y: 0, rotation: 0, scaleX: 2, scaleY: 2 };
    const tri = [[0, 0], [10, 0], [0, 10]];
    const [nav] = gizmosFor({ nav_region: { outline: tri } }, xf);
    expect(nav.kind === "path" && nav.points[1]).toEqual([20, 0]);
    const [confine] = gizmosFor({ camera: { confine: tri } }, xf);
    expect(confine.kind === "path" && confine.points[1]).toEqual([10, 0]);
  });

  it("draws an occluder's polygon, else its box, and nothing when disabled or from its sprite", () => {
    expect(gizmosFor({ light_occluder: { enabled: true, size: [10, 20], polygon: [] } }, still)[0]).toMatchObject({ kind: "path", closed: true });
    expect(gizmosFor({ light_occluder: { enabled: false, size: [10, 20] } }, still)).toEqual([]);
    expect(gizmosFor({ light_occluder: { enabled: true, size: [10, 20], from_sprite: true } }, still)).toEqual([]);
  });
});
