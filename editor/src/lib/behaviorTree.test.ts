import { describe, expect, it } from "vitest";
import {
  addChild,
  arity,
  changeType,
  flatten,
  getNode,
  moveNode,
  newNode,
  NODE_KINDS,
  removeNode,
  shownName,
  summary,
  wrapNode,
  type BtNode,
} from "./behaviorTree";

const guard = (): BtNode => ({
  type: "selector",
  reactive: true,
  children: [
    { type: "sequence", label: "chase", children: [{ type: "near", target: "Player", distance: 100 }, { type: "move_to", target: "Player" }] },
    { type: "wait", seconds: 2 },
  ],
});

describe("behaviour tree editing", () => {
  it("has a node for every group, each starting as a valid shape", () => {
    expect(new Set(NODE_KINDS.map((k) => k.group))).toEqual(new Set(["composite", "decorator", "condition", "action"]));
    for (const k of NODE_KINDS) {
      expect(k.default.type).toBe(k.type);
      const a = arity(k.default);
      if (k.group === "composite") expect(a).toBe("many");
      else if (k.group === "decorator") expect(a).toBe("one");
      else expect(a).toBe("none");
    }
  });

  it("flattens into outline rows by path", () => {
    expect(flatten(guard()).map((r) => [r.path.join("."), r.node.type])).toEqual([
      ["", "selector"],
      ["0", "sequence"],
      ["0.0", "near"],
      ["0.1", "move_to"],
      ["1", "wait"],
    ]);
  });

  it("adds, removes and moves without touching the original", () => {
    const tree = guard();
    const added = addChild(tree, [0], newNode("event"))!;
    expect(added.path).toEqual([0, 2]);
    expect(getNode(added.root, [0, 2])?.type).toBe("event");
    expect(childrenOf0(tree)).toHaveLength(2);
    expect(addChild(tree, [1], newNode("wait"))).toBeNull();
    const moved = moveNode(tree, [1], -1)!;
    expect(moved.path).toEqual([0]);
    expect(getNode(moved.root, [0])?.type).toBe("wait");
    expect(moveNode(tree, [0], -1)).toBeNull();
    const removed = removeNode(tree, [0, 0])!;
    expect(getNode(removed, [0, 0])?.type).toBe("move_to");
    expect(removeNode(tree, [])).toBeNull();
  });

  it("wraps in a decorator, and a decorator keeps one child", () => {
    const wrapped = wrapNode(guard(), [1], "cooldown");
    expect(getNode(wrapped, [1])).toEqual({ type: "cooldown", seconds: 2, child: { type: "wait", seconds: 2 } });
    // The decorator's only child cannot be removed; adding replaces it.
    expect(removeNode(wrapped, [1, 0])).toBeNull();
    const replaced = addChild(wrapped, [1], newNode("chance"))!;
    expect(getNode(replaced.root, [1, 0])?.type).toBe("chance");
  });

  it("changes a type keeping label, shared fields and children", () => {
    const t = changeType(guard(), [0], "selector");
    const n = getNode(t, [0])!;
    expect(n.type).toBe("selector");
    expect(n.label).toBe("chase");
    expect(n.children).toHaveLength(2);
    const dec = changeType(guard(), [0], "invert");
    expect(getNode(dec, [0])?.child?.type).toBe("near");
    const leaf = changeType(guard(), [1], "cooldown");
    expect(getNode(leaf, [1])).toMatchObject({ type: "cooldown", seconds: 2 });
  });

  it("names nodes as the running readout does and summarises them", () => {
    expect(shownName({ type: "move_to", away: true })).toBe("flee");
    expect(shownName({ type: "call", call: "attack" })).toBe("attack");
    expect(shownName({ type: "wait", label: "nap" })).toBe("nap");
    expect(summary({ type: "check", key: "alert", op: "set" })).toBe("alert set");
    expect(summary({ type: "near", target: "Player", distance: 100 })).toBe("Player within 100");
  });
});

function childrenOf0(t: BtNode): BtNode[] {
  return (t.children![0].children ?? []) as BtNode[];
}
