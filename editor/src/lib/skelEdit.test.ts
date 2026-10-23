import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  addBone,
  blendExtent,
  blendWeights,
  boneAt,
  formatClip,
  formatRig,
  keyPose,
  keyTimes,
  moveBoneKeys,
  moveKey,
  parseClip,
  parseRig,
  poseAt,
  positionAt,
  removeBone,
  removeBoneKeys,
  removeKey,
  renameBone,
  rotationToward,
  sampleScalar,
  sampleVec2,
  sceneSkeletons,
  setBone,
  setKey,
  skeletonFromRig,
  skeletonFromScene,
  snap,
  worldPose,
  type Rig,
  type SkelClip,
} from "./skelEdit";

const fixture = JSON.parse(
  readFileSync(new URL("../../../crates/jengine-core/tests/fixtures/skeletal_sample.json", import.meta.url), "utf-8"),
);

describe("sampling, as the engine samples", () => {
  for (const c of fixture.cases) {
    it(c.name, () => {
      if (c.kind === "scalar") expect(sampleScalar(c.keys, c.t)).toBeCloseTo(c.expected, 4);
      else if (c.kind === "vec2") {
        const v = sampleVec2(c.keys, c.t)!;
        expect(v[0]).toBeCloseTo(c.expected[0], 4);
        expect(v[1]).toBeCloseTo(c.expected[1], 4);
      } else {
        const w = blendWeights(c.points, c.p);
        expect(w.length).toBe(c.expected.length);
        w.forEach((x: number, i: number) => expect(x).toBeCloseTo(c.expected[i], 4));
      }
    });
  }
  it("no keys is no value", () => {
    expect(sampleScalar([], 1)).toBeUndefined();
    expect(sampleVec2(undefined, 1)).toBeUndefined();
  });
});

const arm: Rig = {
  name: "arm",
  bones: [
    { name: "elbow", parent: "shoulder", position: [70, 0], rotation: -0.6, length: 55 },
    { name: "shoulder", position: [0, 0], rotation: 0, length: 70 },
    { name: "wrist", parent: "elbow", position: [55, 0], rotation: 0, length: 25 },
  ],
};

describe("posing a skeleton", () => {
  it("a rig puts parents first and chains joints along +x", () => {
    const skel = skeletonFromRig(arm);
    expect(skel.map((b) => b.name)).toEqual(["shoulder", "elbow", "wrist"]);
    const w = worldPose(skel);
    expect(w[1].origin[0]).toBeCloseTo(70);
    expect(w[1].origin[1]).toBeCloseTo(0);
    // The elbow's tip and the wrist's joint are one point.
    expect(w[2].origin[0]).toBeCloseTo(w[1].tip[0]);
    expect(w[2].origin[1]).toBeCloseTo(w[1].tip[1]);
  });

  it("a mirrored parent mirrors its children's places", () => {
    const skel = skeletonFromRig({
      name: "m",
      bones: [
        { name: "root", scale: [-1, 1], length: 10 },
        { name: "child", parent: "root", position: [10, 0], length: 5 },
      ],
    });
    const w = worldPose(skel);
    expect(w[1].origin[0]).toBeCloseTo(-10);
    expect(w[1].tip[0]).toBeCloseTo(-15);
  });

  it("a clip replaces only the channels it animates", () => {
    const skel = skeletonFromRig(arm);
    const clip: SkelClip = {
      name: "c",
      duration: 1,
      tracks: [{ bone_name: "elbow", rotation: [{ time: 0, value: 0 }, { time: 1, value: 1 }] }],
    };
    const posed = poseAt(skel, clip, 0.5);
    expect(posed[1].local.rotation).toBeCloseTo(0.5);
    expect(posed[1].local.position).toEqual([70, 0]);
    expect(posed[0]).toBe(skel[0]);
  });

  it("dragging a tip turns the bone to point at it, in its parent's frame", () => {
    const skel = skeletonFromRig(arm);
    const w = worldPose(skel);
    const r = rotationToward(skel, w, 1, [70, 50]);
    // Straight up from the elbow, in a parent that is not turned.
    expect(r).toBeCloseTo(Math.PI / 2);
    const turned = skeletonFromRig(setBone(arm, "shoulder", { rotation: Math.PI / 2 }));
    const tw = worldPose(turned);
    // The elbow is now at (0, 70); pointing it further up is no turn.
    expect(rotationToward(turned, tw, 1, [0, 200])).toBeCloseTo(0);
  });

  it("a drag never turns a bone by a whole turn", () => {
    const skel = skeletonFromRig(setBone(arm, "elbow", { rotation: 6.0 }));
    const w = worldPose(skel);
    const r = rotationToward(skel, w, 1, [70, -1]);
    expect(Math.abs(r - 6.0)).toBeLessThan(Math.PI);
  });

  it("a joint dragged lands where it was dropped", () => {
    const skel = skeletonFromRig(setBone(arm, "shoulder", { rotation: 1.0 }));
    const w = worldPose(skel);
    const pos = positionAt(skel, w, 1, [10, 20]);
    const moved = skel.map((b, i) => (i === 1 ? { ...b, local: { ...b.local, position: pos } } : b));
    const mw = worldPose(moved);
    expect(mw[1].origin[0]).toBeCloseTo(10);
    expect(mw[1].origin[1]).toBeCloseTo(20);
  });

  it("finds the bone under the pointer", () => {
    const w = worldPose(skeletonFromRig(arm));
    expect(boneAt(w, [35, 2], 5)).toBe(0);
    expect(boneAt(w, [35, 40], 5)).toBe(-1);
  });

  it("reads a scene's skeleton through its entities", () => {
    const entities = [
      { name: "Rig", components: { transform: { position: [0, -60] }, skeleton_root: { name: "lab" } } },
      { name: "Body", parent: "Rig", components: { transform: { position: [0, 0] }, bone: { name: "body", length: 120 } } },
      { name: "Arm", parent: "Body", components: { transform: { position: [0, 112] }, bone: { name: "arm", length: 60 } } },
      { name: "Hat", parent: "Arm", components: { transform: { position: [1, 1] } } },
      { name: "Other", components: { bone: { name: "stray", length: 5 } } },
    ];
    expect(sceneSkeletons(entities)).toEqual([{ index: 0, name: "lab" }]);
    const skel = skeletonFromScene(entities, 0);
    expect(skel.map((b) => [b.name, b.parent])).toEqual([
      ["body", -1],
      ["arm", 0],
    ]);
    expect(skel[1].local.position).toEqual([0, 112]);
  });
});

describe("editing a clip", () => {
  const empty: SkelClip = { name: "c", duration: 1, tracks: [] };

  it("keys in time order, and a key at the same time replaces", () => {
    let c = setKey(empty, "arm", "rotation", 0.5, 1).clip;
    c = setKey(c, "arm", "rotation", 0.1, 2).clip;
    const r = setKey(c, "arm", "rotation", 0.5, 3);
    expect(r.index).toBe(1);
    expect(r.clip.tracks![0].rotation).toEqual([
      { time: 0.1, value: 2 },
      { time: 0.5, value: 3 },
    ]);
    expect(empty.tracks).toEqual([]);
  });

  it("moves, removes, and merges a key dropped on another", () => {
    let c = setKey(empty, "arm", "position", 0, [1, 1]).clip;
    c = setKey(c, "arm", "position", 0.5, [2, 2]).clip;
    c = setKey(c, "arm", "position", 1, [3, 3]).clip;
    const m = moveKey(c, "arm", "position", 0, 0.75);
    expect(m.index).toBe(1);
    expect(m.clip.tracks![0].position!.map((k) => k.time)).toEqual([0.5, 0.75, 1]);
    const merged = moveKey(c, "arm", "position", 0, 1);
    expect(merged.clip.tracks![0].position).toEqual([
      { time: 0.5, value: [2, 2] },
      { time: 1, value: [1, 1] },
    ]);
    expect(removeKey(c, "arm", "position", 1).tracks![0].position!.length).toBe(2);
  });

  it("a bone's diamond moves and removes every channel at that time", () => {
    let c = setKey(empty, "arm", "rotation", 0.2, 1).clip;
    c = setKey(c, "arm", "scale", 0.2, [2, 2]).clip;
    c = setKey(c, "arm", "position", 0.6, [0, 0]).clip;
    expect(keyTimes(c.tracks![0])).toEqual([0.2, 0.6]);
    const moved = moveBoneKeys(c, "arm", 0.2, 0.4);
    expect(keyTimes(moved.tracks![0])).toEqual([0.4, 0.6]);
    expect(keyTimes(removeBoneKeys(c, "arm", 0.2).tracks![0])).toEqual([0.6]);
  });

  it("keying a pose keys what the bone shows now", () => {
    const skel = skeletonFromRig(arm);
    const c = keyPose(empty, skel, "elbow", 0.3, ["rotation", "position"]);
    expect(c.tracks![0].rotation).toEqual([{ time: 0.3, value: -0.6 }]);
    expect(c.tracks![0].position).toEqual([{ time: 0.3, value: [70, 0] }]);
  });

  it("writes what the engine reads, empty channels left out", () => {
    const c = setKey({ ...empty, tracks: [{ bone_name: "arm", scale: [] }] }, "arm", "rotation", 0, 1).clip;
    const back = JSON.parse(formatClip(c));
    expect(back.tracks).toEqual([{ bone_name: "arm", rotation: [{ time: 0, value: 1 }] }]);
    expect(parseClip(formatClip(c))).toEqual({ ...c, tracks: back.tracks });
  });

  it("snaps to the grid", () => {
    expect(snap(0.27, 0.05)).toBeCloseTo(0.25);
    expect(snap(-1, 0.05)).toBe(0);
    expect(snap(0.27, 0)).toBe(0.27);
  });
});

describe("editing a rig", () => {
  it("adds a bone at its parent's tip", () => {
    const { rig, name } = addBone(arm, "wrist");
    expect(name).toBe("bone");
    expect(rig.bones.find((b) => b.name === "bone")).toMatchObject({ parent: "wrist", position: [25, 0] });
  });

  it("removing a bone keeps its children where they were", () => {
    const before = worldPose(skeletonFromRig(arm));
    const after = skeletonFromRig(removeBone(arm, "elbow"));
    const w = worldPose(after);
    const wrist = after.findIndex((b) => b.name === "wrist");
    expect(after[wrist].parent).toBe(after.findIndex((b) => b.name === "shoulder"));
    expect(w[wrist].origin[0]).toBeCloseTo(before[2].origin[0]);
    expect(w[wrist].origin[1]).toBeCloseTo(before[2].origin[1]);
  });

  it("renames a bone and its children's links; refuses a taken name", () => {
    const r = renameBone(arm, "elbow", "forearm");
    expect(r.bones.find((b) => b.name === "wrist")!.parent).toBe("forearm");
    expect(renameBone(arm, "elbow", "wrist")).toBe(arm);
  });

  it("refuses a parent that would make a loop", () => {
    expect(setBone(arm, "shoulder", { parent: "wrist" })).toBe(arm);
    expect(setBone(arm, "wrist", { parent: "shoulder" }).bones.find((b) => b.name === "wrist")!.parent).toBe(
      "shoulder",
    );
  });

  it("round-trips one bone a line", () => {
    const text = formatRig(arm);
    expect(text.split("\n").filter((l) => l.includes('"name": "')).length).toBe(4);
    expect(parseRig(text)).toEqual(parseRig(JSON.stringify(arm)));
  });
});

describe("a blend space", () => {
  it("is drawn big enough for its points", () => {
    expect(blendExtent({ points: [{ clip: "a", at: [0.5, 0] }] })).toBe(1.25);
    expect(blendExtent({ points: [{ clip: "a", at: [3, 0] }] })).toBe(3.5);
  });
});
