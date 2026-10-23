import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { editThroughInstance, resolveVariants, viewOf, type PrefabEntity } from "./prefabInstance";
import {
  addPart,
  diffOverlay,
  duplicatePart,
  framesIn,
  instanceParts,
  movePart,
  partAt,
  partRows,
  removePart,
  renamePart,
  replacePart,
  resolveOverlay,
  unclaimedParts,
  withSceneParts,
} from "./prefabParts";

const fixture = JSON.parse(
  readFileSync(new URL("../../../crates/jengine-core/tests/fixtures/prefab_parts.json", import.meta.url), "utf-8"),
);
const raw = new Map<string, PrefabEntity>(fixture.prefabs.map((p: PrefabEntity) => [p.name, p]));
const prefabs = resolveVariants(raw);
const entities = fixture.scene.entities as { name: string; parent?: string; components: Record<string, any> }[];

describe("a placed prefab's parts, as the engine hangs them", () => {
  it("names, parents, places and textures every part as the fixture says", () => {
    const cart = entities[0];
    const parts = instanceParts(cart.name, prefabs.get("Cart")!, prefabs);
    // The overlay laid over its part, as the engine lays it.
    const rows = [
      { name: cart.name, parent: null, components: viewOf(cart, prefabs).components },
      ...parts.map((p) => {
        const over = entities.find((e) => e.name === p.name && e.parent === p.parent);
        return { name: p.name, parent: p.parent, components: over ? resolveOverlay(p.components, over.components) : p.components };
      }),
    ]
      .map((r: any) => ({
        name: r.name,
        parent: r.parent,
        position: r.components.transform?.position ?? [0, 0],
        texture: r.components.sprite?.texture_id ?? null,
      }))
      .sort((a, b) => a.name.localeCompare(b.name));
    expect(rows).toEqual(fixture.expected);
  });

  it("lists only the parts the scene does not lay over yet", () => {
    expect(unclaimedParts(prefabs, entities).map((p) => p.name)).toEqual(["Cart/Wheel", "Cart/Mast", "Cart/Mast/Flag"]);
  });
});

describe("editing a part in a scene", () => {
  const view = withSceneParts(prefabs, entities);

  it("shows an overlay entity as its part with the overlay over it", () => {
    const over = viewOf(entities[1] as any, view);
    expect(over.components.transform.position).toEqual([12, -6]);
    expect(over.components.sprite.texture_id).toBe("sprites/wheel.png");
  });

  it("keeps only the components that differ, whole", () => {
    const fresh = { name: "Cart/Mast/Flag", parent: "Cart/Mast", components: {} as Record<string, any> };
    editThroughInstance(fresh, view, (v) => {
      (v.components as any).sprite.color = [1, 0, 0, 1];
    });
    expect(Object.keys(fresh.components)).toEqual(["sprite"]);
    // Whole: the texture comes along, so the engine reads a complete sprite.
    expect(fresh.components.sprite.texture_id).toBe("sprites/flag.png");
    expect(fresh.components.sprite.color).toEqual([1, 0, 0, 1]);
    // Put back as it was, and the overlay is empty again.
    editThroughInstance(fresh, view, (v) => {
      (v.components as any).sprite.color = [1, 1, 1, 1];
    });
    expect(fresh.components).toEqual({});
  });

  it("does not treat an entity of a part's name under another parent as the part", () => {
    const stray = { name: "Cart/Wheel", parent: "Elsewhere", components: {} };
    expect(viewOf(stray, view)).toBe(stray);
  });

  it("a component the edit drops cannot be said by an overlay", () => {
    expect(diffOverlay({ sprite: { a: 1 } }, {})).toEqual({});
  });
});

describe("a prefab's part tree", () => {
  const cart = prefabs.get("Cart")!;

  it("reads parts by their path", () => {
    expect(partAt(cart, [2, 0])!.name).toBe("Flag");
    expect(partRows(cart).map((r) => `${"-".repeat(r.depth)}${r.part.name}`)).toEqual([
      "Cart",
      "-Wheel",
      "-Wheel",
      "-Mast",
      "--Flag",
    ]);
  });

  it("adds, duplicates, moves and removes parts, leaving the original alone", () => {
    const a = addPart(cart, [2]);
    expect(a.path).toEqual([2, 1]);
    expect(partAt(a.root, a.path)!.name).toBe("Part");
    const d = duplicatePart(cart, [2, 0]);
    expect(partAt(d.root, d.path)!.name).toBe("Flag 2");
    const m = movePart(cart, [0], 2);
    expect(m.path).toEqual([2]);
    expect(m.root.children!.map((c) => c.name)).toEqual(["Wheel", "Mast", "Wheel"]);
    expect(removePart(cart, [2]).children!.map((c) => c.name)).toEqual(["Wheel", "Wheel"]);
    expect(cart.children!.length).toBe(3);
  });

  it("refuses a name a sibling has, or none", () => {
    expect(renamePart(cart, [2], "Wheel")).toBe(cart);
    expect(renamePart(cart, [2], " ")).toBe(cart);
    expect(partAt(renamePart(cart, [2], "Pole"), [2])!.name).toBe("Pole");
  });

  it("replaces a part where it sits", () => {
    const r = replacePart(cart, [2, 0], (p) => ({ ...p, name: "Pennant" }));
    expect(partAt(r, [2, 0])!.name).toBe("Pennant");
    expect(partAt(cart, [2, 0])!.name).toBe("Flag");
  });

  it("places every part in the focused part's parent's space", () => {
    // Focused on the flag: its parent, the mast, is the space - the flag
    // sits at its own transform there, the cart's body below the mast.
    const frames = framesIn(cart, [2, 0]);
    const at = (path: number[]) => frames.find((f) => f.path.join() === path.join())!.frame;
    expect([at([2, 0])[4], at([2, 0])[5]]).toEqual([0, 8]);
    expect([at([])[4], at([])[5]]).toEqual([0, -10]);
    // Focused on the root: prefab space, parts where they hang.
    const top = framesIn(cart, []);
    expect(top.find((f) => f.path.join() === "2,0")!.frame.slice(4)).toEqual([0, 18]);
  });
});
