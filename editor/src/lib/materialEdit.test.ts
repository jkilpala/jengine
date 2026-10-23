import { describe, expect, it } from "vitest";
import { formatMaterial, graphParams, parseMaterial, uniformToVec4, withGraph, withUniform } from "./materialEdit";

describe("material files", () => {
  it("reads a file defensively and keeps keys it does not know", () => {
    const m = parseMaterial('{"shader_graph_path": "assets/shaders/a.shader", "blend_mode": "Additive", "extra": 1}');
    expect(m.shader).toBe("default_sprite");
    expect(m.extra).toBe(1);
  });

  it("sets and clears the graph with the shader name that says so", () => {
    const m = withGraph(parseMaterial("{}"), "assets/shaders/glow.shader");
    expect(m).toMatchObject({ shader: "graph", shader_graph_path: "assets/shaders/glow.shader" });
    expect(withGraph(m, " ")).toEqual({ shader: "default_sprite" });
  });

  it("lists a graph's parameters in slot order", () => {
    const graph = {
      nodes: [
        { node_type: { type: "MaterialParam", name: "tint" } },
        { node_type: { type: "Add" } },
        { node_type: { type: "MaterialParam", name: "glow" } },
        { node_type: { type: "MaterialParam", name: "tint" } },
      ],
    };
    expect(graphParams(graph)).toEqual(["glow", "tint"]);
    expect(graphParams(null)).toEqual([]);
  });

  it("reads every uniform shape as four numbers and writes a vec4", () => {
    expect(uniformToVec4({ Float: 2 })).toEqual([2, 0, 0, 0]);
    expect(uniformToVec4({ Vec2: [1, 2] })).toEqual([1, 2, 0, 0]);
    expect(uniformToVec4(undefined)).toEqual([0, 0, 0, 0]);
    let m = withUniform(parseMaterial("{}"), "tint", [1, 0.5, 0, 1]);
    expect(m.uniforms).toEqual({ tint: { Vec4: [1, 0.5, 0, 1] } });
    m = withUniform(m, "tint", null);
    expect(m.uniforms).toBeUndefined();
    expect(formatMaterial(m).endsWith("}\n")).toBe(true);
  });
});
