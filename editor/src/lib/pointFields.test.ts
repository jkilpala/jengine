import { describe, expect, it } from "vitest";
import {
  colliderPointsPath,
  getAtPath,
  insertionIndex,
  pointListInfo,
  pointsToEdit,
  readPoints,
  type Point,
} from "./pointFields";

describe("point lists", () => {
  it("finds a collider's vertex list by its shape", () => {
    expect(colliderPointsPath({ ConvexPolygon: { vertices: [] } })).toBe("collider.shape.ConvexPolygon.vertices");
    expect(colliderPointsPath({ Polyline: { vertices: [] } })).toBe("collider.shape.Polyline.vertices");
    expect(colliderPointsPath({ Box: { width: 1, height: 1 } })).toBeNull();
    expect(colliderPointsPath(null)).toBeNull();
  });

  it("knows which lists close, and which are in world units", () => {
    const comps = { line: { points: [], closed: false }, spline_shape: { closed: true }, nav_region: {}, camera: {} };
    expect(pointListInfo(comps, "line.points")).toEqual({ closed: false, min: 2, world: false });
    expect(pointListInfo(comps, "spline_shape.points").closed).toBe(true);
    expect(pointListInfo(comps, "nav_region.outline")).toEqual({ closed: true, min: 3, world: false });
    expect(pointListInfo(comps, "camera.confine").world).toBe(true);
    expect(pointListInfo(comps, "collider.shape.Polyline.vertices").closed).toBe(false);
  });

  it("reads an absent list as empty and creates it to edit", () => {
    const comps: Record<string, unknown> = { camera: { zoom: 1 } };
    expect(readPoints(comps, "camera.confine")).toEqual([]);
    pointsToEdit(comps, "camera.confine").push([1, 2]);
    expect(getAtPath(comps, "camera.confine")).toEqual([[1, 2]]);
    pointsToEdit(comps, "line.points").push([0, 0]);
    expect(comps.line).toEqual({ points: [[0, 0]] });
  });

  it("adds a point into the edge it lands on", () => {
    const square: Point[] = [[0, 0], [10, 0], [10, 10], [0, 10]];
    expect(insertionIndex(square, [5, -1], true)).toBe(1);
    expect(insertionIndex(square, [11, 5], true)).toBe(2);
    // The closing edge, back to the first point.
    expect(insertionIndex(square, [-1, 5], true)).toBe(4);
    // An open line has no closing edge: a point past its end goes on the end.
    const line: Point[] = [[0, 0], [10, 0]];
    expect(insertionIndex(line, [20, 0], false)).toBe(2);
    expect(insertionIndex(line, [5, 1], false)).toBe(1);
    expect(insertionIndex(line, [-5, 0], false)).toBe(0);
    // Far from every edge, with a reach: at the end.
    expect(insertionIndex(square, [50, 50], true, 8)).toBe(4);
    expect(insertionIndex([[0, 0]], [1, 1], true)).toBe(1);
  });
});
