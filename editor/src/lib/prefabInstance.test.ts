import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  changedByComponent,
  fieldChanged,
  diffEdit,
  isOverridden,
  putOverride,
  resolveInstance,
  resolveVariants,
  revertPath,
  setByPath,
  type Components,
  type PrefabEntity,
} from "./prefabInstance";

// The fixture the Rust merge is tested against too
// (`prefab_merge_matches_the_editor`), so the two cannot drift apart.
const fixture = JSON.parse(
  readFileSync(new URL("../../../crates/jengine-core/tests/fixtures/prefab_merge.json", import.meta.url), "utf-8"),
) as { template: Components; cases: { name: string; entity: Components; expected: Components }[] };

describe("instance resolution matches the engine's", () => {
  for (const c of fixture.cases) {
    it(c.name, () => {
      const resolved = resolveInstance(fixture.template, c.entity);
      delete resolved.prefab_instance;
      expect(resolved).toEqual(c.expected);
    });
  }

  it("does not touch the template or the entity", () => {
    const template = structuredClone(fixture.template);
    const entity = { prefab_instance: { prefab_id: "Hero", overrides: { "sprite.z_order": 9 } } };
    resolveInstance(template, entity);
    expect(template).toEqual(fixture.template);
    expect(entity.prefab_instance.overrides).toEqual({ "sprite.z_order": 9 });
  });
});

describe("set by path", () => {
  it("creates missing objects and ignores what it cannot reach", () => {
    const root: Record<string, unknown> = { a: { list: [1, 2] }, n: 3 };
    setByPath(root, "b.c", 1);
    setByPath(root, "a.list.5", 9);
    setByPath(root, "n.x", 1);
    expect(root).toEqual({ a: { list: [1, 2] }, n: 3, b: { c: 1 } });
  });
});

describe("variants", () => {
  it("extend their base, a variant of a variant too, and survive a cycle", () => {
    const prefabs = new Map<string, PrefabEntity>([
      ["Slime", { name: "Slime", components: { sprite: { texture_id: "slime.png", color: [1, 1, 1, 1] }, health: { hp: 3 } } }],
      ["FireSlime", { name: "FireSlime", components: { prefab_instance: { prefab_id: "Slime", overrides: { "sprite.color": [1, 0.4, 0.2, 1] } }, point_light: { radius: 60 } } }],
      ["BigFireSlime", { name: "BigFireSlime", components: { prefab_instance: { prefab_id: "FireSlime", overrides: { "health.hp": 9 } } } }],
      ["A", { name: "A", components: { prefab_instance: { prefab_id: "B" } } }],
      ["B", { name: "B", components: { prefab_instance: { prefab_id: "A" } } }],
    ]);
    const out = resolveVariants(prefabs);
    const big = out.get("BigFireSlime")!.components;
    expect(big.sprite).toEqual({ texture_id: "slime.png", color: [1, 0.4, 0.2, 1] });
    expect(big.health).toEqual({ hp: 9 });
    expect(big.point_light).toEqual({ radius: 60 });
    expect(out.get("A")).toBeDefined();
  });
});

describe("editing an instance", () => {
  const template = fixture.template;
  const stored = (): Components => ({
    transform: { position: [40, 10], rotation: 0, scale: [1, 1] },
    prefab_instance: { prefab_id: "Hero" },
  });
  const edit = (s: Components, change: (c: Record<string, any>) => void) => {
    const before = resolveInstance(template, s);
    const after = structuredClone(before) as Record<string, any>;
    change(after);
    return diffEdit(s, template, before, after);
  };

  it("writes a prefab field as an override and the entity's own component whole", () => {
    const out = edit(stored(), (c) => {
      c.sprite.color = [0, 1, 0, 1];
      c.transform.position = [50, 10];
    });
    expect(out.prefab_instance).toEqual({ prefab_id: "Hero", overrides: { "sprite.color": [0, 1, 0, 1] } });
    expect((out.transform as any).position).toEqual([50, 10]);
    expect(out.sprite).toBeUndefined();
    // And it resolves to what was edited.
    expect((resolveInstance(template, out).sprite as any).color).toEqual([0, 1, 0, 1]);
  });

  it("adds a component as the entity's own and removes a prefab one with null", () => {
    const out = edit(stored(), (c) => {
      c.tags = ["boss"];
      delete c.point_light;
    });
    expect(out.tags).toEqual(["boss"]);
    expect((out.prefab_instance as any).overrides).toEqual({ point_light: null });
    expect(resolveInstance(template, out).point_light).toBeUndefined();
  });

  it("unsets an optional field with a null override", () => {
    const out = edit(stored(), (c) => {
      delete c.sprite.size;
    });
    expect((out.prefab_instance as any).overrides).toEqual({ "sprite.size": null });
  });

  it("an unchanged edit leaves no overrides behind", () => {
    const out = edit(stored(), () => {});
    expect(out).toEqual(stored());
  });

  it("never keeps a path beside its ancestor", () => {
    const o: Record<string, unknown> = { sprite: { texture_id: "x.png", color: [1, 1, 1, 1] } };
    putOverride(o, "sprite.color", [0, 0, 0, 1]);
    expect(o).toEqual({ sprite: { texture_id: "x.png", color: [0, 0, 0, 1] } });
    const p: Record<string, unknown> = { "sprite.color": [0, 0, 0, 1], "sprite.size": [4, 4] };
    putOverride(p, "sprite", { texture_id: "y.png" });
    expect(p).toEqual({ sprite: { texture_id: "y.png" } });
  });

  it("knows what is overridden and reverts it", () => {
    const s = edit(stored(), (c) => {
      c.sprite.color = [0, 1, 0, 1];
      c.point_light.radius = 10;
    });
    expect(isOverridden(s, "sprite.color")).toBe(true);
    expect(isOverridden(s, "sprite")).toBe(true);
    expect(isOverridden(s, "sprite.z_order")).toBe(false);
    expect(isOverridden(s, "transform.position")).toBe(true);
    const back = revertPath(s, template, "sprite.color");
    expect((back.prefab_instance as any).overrides).toEqual({ "point_light.radius": 10 });
    expect((resolveInstance(template, back).sprite as any).color).toEqual([1, 1, 1, 1]);
    const all = revertPath(revertPath(back, template, "point_light"), template, "transform");
    expect(all).toEqual({ prefab_instance: { prefab_id: "Hero" } });
  });

  it("reverts a field inside a whole-component override from the template", () => {
    const s: Components = {
      prefab_instance: { prefab_id: "Hero", overrides: { sprite: { texture_id: "g.png", color: [0, 0, 0, 1], z_order: 2, size: [32, 32] } } },
    };
    const back = revertPath(s, template, "sprite.texture_id");
    expect(((back.prefab_instance as any).overrides.sprite as any).texture_id).toBe("hero.png");
  });
});

describe("applying to the prefab", () => {
  it("writes into a plain prefab's components and into a variant's overrides", async () => {
    const { applyToPrefab, instanceChanges } = await import("./prefabInstance");
    const plain = { name: "Hero", components: structuredClone(fixture.template) };
    const a = applyToPrefab(plain, "sprite.color", [0, 0, 1, 1]);
    expect((a.components.sprite as any).color).toEqual([0, 0, 1, 1]);
    expect(applyToPrefab(plain, "point_light", null).components.point_light).toBeUndefined();
    expect((plain.components.sprite as any).color).toEqual([1, 1, 1, 1]);
    const variant = { name: "Red", components: { prefab_instance: { prefab_id: "Hero", overrides: { "sprite.z_order": 5 } } } };
    const v = applyToPrefab(variant, "sprite.color", [1, 0, 0, 1]);
    expect(v.components).toEqual({
      prefab_instance: { prefab_id: "Hero", overrides: { "sprite.z_order": 5, "sprite.color": [1, 0, 0, 1] } },
    });
    expect(
      instanceChanges({
        transform: {},
        tags: ["x"],
        prefab_instance: { prefab_id: "Hero", overrides: { "sprite.color": [1, 0, 0, 1] } },
      }).map((r) => [r.path, r.own]),
    ).toEqual([["sprite.color", false], ["tags", true]]);
  });
});

describe("editing through an instance", () => {
  it("moves an instance whose position is an override, keeping it an override", async () => {
    const { editThroughInstance, viewOf } = await import("./prefabInstance");
    const prefabs = new Map([["Hero", { name: "Hero", components: structuredClone(fixture.template) }]]);
    const ent = {
      name: "Pet0",
      components: { prefab_instance: { prefab_id: "Hero", overrides: { "transform.position": [-420, 40] } } } as Components,
    };
    expect((viewOf(ent, prefabs).components.transform as any).position).toEqual([-420, 40]);
    editThroughInstance(ent, prefabs, (v) => {
      (v.components.transform as any).position = [10, 20];
    });
    expect(ent.components).toEqual({
      prefab_instance: { prefab_id: "Hero", overrides: { "transform.position": [10, 20] } },
    });
    // A plain entity is edited as it is.
    const plain = { name: "P", components: { tags: ["a"] } as Components };
    editThroughInstance(plain, prefabs, (v) => {
      v.components.tags = ["b"];
    });
    expect(plain.components.tags).toEqual(["b"]);
    expect(viewOf(plain, prefabs)).toBe(plain);
  });
});

describe("what an entity changes, by component", () => {
  it("an instance: its own components and its override paths", () => {
    const stored = {
      transform: { position: [1, 2] },
      script: { path: "a.lua" },
      prefab_instance: { prefab_id: "P", overrides: { "sprite.color": [1, 0, 0, 1], "sprite.size": [8, 8] } },
    };
    const c = changedByComponent(stored);
    expect([...c.keys()].sort()).toEqual(["script", "sprite"]);
    expect(c.get("sprite")).toEqual(["sprite.color", "sprite.size"]);
    expect(fieldChanged(c, "sprite", "color")).toBe(true);
    expect(fieldChanged(c, "sprite", "texture_id")).toBe(false);
    expect(fieldChanged(c, "script", "path")).toBe(true);
    expect(fieldChanged(c, "transform", "position")).toBe(false);
  });

  it("a part overlay: every component it holds", () => {
    const c = changedByComponent({ transform: { position: [0, 0] } }, true);
    expect(fieldChanged(c, "transform", "rotation")).toBe(true);
  });

  it("a plain entity changes nothing", () => {
    expect(changedByComponent({ sprite: {} }).size).toBe(0);
  });
});
