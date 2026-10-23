import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { apply, skeletonFromRig, type Affine } from "./skelEdit";
import {
  autoWeights,
  boneFrames,
  brushHits,
  centredPlacement,
  fromSkinnedMesh,
  gridMesh,
  handOver,
  normalise,
  paintWeights,
  pixelToRoot,
  placementOf,
  renameWeights,
  rootToPixel,
  segmentsOf,
  skin,
  smoothWeights,
  toSkinnedMesh,
  type SkinnedMeshData,
} from "./skinEdit";

const fixture = JSON.parse(
  readFileSync(new URL("../../../crates/jengine-core/tests/fixtures/skin_sample.json", import.meta.url), "utf-8"),
);

describe("skinning, as the engine skins", () => {
  const bind = skeletonFromRig({ name: "s", bones: fixture.bones });
  const posed = bind.map((b) =>
    b.name in fixture.pose ? { ...b, local: { ...b.local, rotation: fixture.pose[b.name] } } : b,
  );
  const mesh = fromSkinnedMesh(fixture.mesh as SkinnedMeshData, bind);

  it("reads each vertex back to where its offsets put it", () => {
    expect(mesh.vertices).toEqual(fixture.mesh.vertices.map((v: any) => v.rest_position));
  });

  it("bends the mesh where the engine does", () => {
    const got = skin(mesh, boneFrames(bind), boneFrames(posed));
    const c = Math.cos(fixture.root.rotation);
    const s = Math.sin(fixture.root.rotation);
    const root: Affine = [c, s, -s, c, fixture.root.position[0], fixture.root.position[1]];
    got.forEach((p, i) => {
      const w = apply(root, p[0], p[1]);
      expect(w[0]).toBeCloseTo(fixture.expected[i][0], 3);
      expect(w[1]).toBeCloseTo(fixture.expected[i][1], 3);
    });
  });

  it("writes the offsets the engine reads", () => {
    const out = toSkinnedMesh(mesh, bind, fixture.mesh.texture_id);
    expect(out.bones).toEqual(["a", "b"]);
    out.vertices.forEach((v, i) => {
      const want = fixture.mesh.vertices[i].influences;
      const got = [...v.influences].sort((a, b) => a.bone_index - b.bone_index);
      expect(got.length).toBe(want.length);
      got.forEach((inf, k) => {
        expect(inf.bone_index).toBe(want[k].bone_index);
        expect(inf.weight).toBeCloseTo(want[k].weight, 4);
        expect(inf.offset[0]).toBeCloseTo(want[k].offset[0], 4);
        expect(inf.offset[1]).toBeCloseTo(want[k].offset[1], 4);
      });
    });
  });

  it("at the bind pose, nothing moves", () => {
    const f = boneFrames(bind);
    expect(skin(mesh, f, f)).toEqual(mesh.vertices.map((v) => [expect.closeTo(v[0], 6), expect.closeTo(v[1], 6)]));
  });
});

describe("a mesh over a picture", () => {
  // 4x4 pixels, only the top-left 2x2 opaque.
  const rgba = new Uint8Array(4 * 4 * 4);
  for (const [x, y] of [[0, 0], [1, 0], [0, 1], [1, 1]]) rgba[(y * 4 + x) * 4 + 3] = 255;

  it("covers only cells with an opaque pixel, corners shared", () => {
    const m = gridMesh(rgba, 4, 4, 2, 10, { x: 0, y: 0, scale: 1 });
    expect(m.indices.length).toBe(6);
    expect(m.vertices.length).toBe(4);
    expect(new Set(m.vertices.map((v) => v.join()))).toEqual(new Set(["0,0", "2,0", "0,-2", "2,-2"]));
    // UVs run down from the picture's top.
    const top = m.vertices.findIndex((v) => v[0] === 0 && v[1] === 0);
    expect(m.uvs[top]).toEqual([0, 0]);
  });

  it("winds triangles counter-clockwise with y up", () => {
    const m = gridMesh(rgba, 4, 4, 1, 10, { x: 0, y: 0, scale: 1 });
    for (let t = 0; t < m.indices.length; t += 3) {
      const [a, b, c] = [m.vertices[m.indices[t]], m.vertices[m.indices[t + 1]], m.vertices[m.indices[t + 2]]];
      expect((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).toBeGreaterThan(0);
    }
  });

  it("maps pixels to root space and back", () => {
    const p = centredPlacement(100, 50, 2);
    expect(pixelToRoot(p, 50, 25)).toEqual([0, 0]);
    expect(rootToPixel(p, ...pixelToRoot(p, 13, 7))).toEqual([13, 7]);
  });

  it("finds a mesh's placement from its UVs", () => {
    const place = { x: -40, y: 30, scale: 0.5 };
    const m = gridMesh(rgba, 4, 4, 1, 10, place);
    const found = placementOf(m, 4, 4)!;
    expect(found.scale).toBeCloseTo(0.5);
    expect(found.x).toBeCloseTo(-40);
    expect(found.y).toBeCloseTo(30);
  });
});

describe("weights", () => {
  const arm = skeletonFromRig({
    name: "arm",
    bones: [
      { name: "upper", position: [0, 0], length: 10 },
      { name: "lower", parent: "upper", position: [10, 0], length: 10 },
    ],
  });
  const segs = segmentsOf(arm);

  it("lean on the nearest bone, and on both at a joint", () => {
    const [mid, joint] = autoWeights(
      [
        [15, 1],
        [10, 3],
      ],
      segs,
    );
    expect(mid.lower).toBeGreaterThan(0.9);
    expect(joint.upper).toBeCloseTo(0.5, 1);
    expect(joint.lower).toBeCloseTo(0.5, 1);
  });

  it("normalise to one and drop the dust", () => {
    expect(normalise({ a: 3, b: 1, c: 0.01 })).toEqual({ a: 0.75, b: 0.25 });
    expect(normalise({})).toEqual({});
  });

  it("paint up, and the others give way", () => {
    const w = paintWeights([{ upper: 0.5, lower: 0.5 }], [{ i: 0, f: 1 }], "lower", 0.5, () => null);
    expect(w[0].lower).toBeCloseTo(0.75);
    expect(w[0].upper).toBeCloseTo(0.25);
  });

  it("paint down a vertex's only bone and the heir takes it", () => {
    const w = paintWeights([{ upper: 1 }], [{ i: 0, f: 1 }], "upper", -0.5, () => "lower");
    expect(w[0]).toEqual({ upper: 0.5, lower: 0.5 });
    // With no heir it cannot give weight away.
    expect(paintWeights([{ upper: 1 }], [{ i: 0, f: 1 }], "upper", -0.5, () => null)[0]).toEqual({ upper: 1 });
  });

  it("the brush falls off to nothing at its edge", () => {
    const hits = brushHits(
      [
        [0, 0],
        [5, 0],
        [9.99, 0],
        [11, 0],
      ],
      [0, 0],
      10,
    );
    expect(hits.map((h) => h.i)).toEqual([0, 1, 2]);
    expect(hits[0].f).toBe(1);
    expect(hits[1].f).toBeCloseTo(0.5);
    expect(hits[2].f).toBeLessThan(0.01);
  });

  it("smooth pulls a vertex toward its neighbours", () => {
    const w = smoothWeights([{ a: 1 }, { b: 1 }, { b: 1 }], [0, 1, 2], [{ i: 0, f: 1 }], 0.5);
    expect(w[0].a).toBeCloseTo(0.5);
    expect(w[0].b).toBeCloseTo(0.5);
  });

  it("a removed bone hands its weight on; a renamed one keeps it", () => {
    expect(handOver([{ a: 0.4, b: 0.6 }], "a", "b")[0]).toEqual({ b: 1 });
    expect(renameWeights([{ a: 0.4, b: 0.6 }], "a", "c")[0]).toEqual({ b: 0.6, c: 0.4 });
  });

  it("a vertex with no weight is pinned to its nearest bone when written", () => {
    const out = toSkinnedMesh({ vertices: [[18, 0]], uvs: [[0, 0]], indices: [], weights: [{}] }, arm, "t.png");
    expect(out.vertices[0].influences).toEqual([{ bone_index: 1, weight: 1, offset: [8, 0] }]);
  });

  it("keeps at most four influences, renormalised", () => {
    const out = toSkinnedMesh(
      { vertices: [[0, 0]], uvs: [[0, 0]], indices: [], weights: [{ upper: 0.6, lower: 0.4 }] },
      arm,
      "t.png",
      {},
      1,
    );
    expect(out.vertices[0].influences).toEqual([{ bone_index: 0, weight: 1, offset: [0, 0] }]);
  });
});
