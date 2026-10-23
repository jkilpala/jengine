/**
 * A `.material` file (Rust `renderer::material::MaterialData`) as the
 * material editor edits it: the shader graph it draws with, its blend mode,
 * and its **parameter defaults** - the `uniforms` every sprite wearing it
 * draws with unless it sets its own (`renderer::material_params`). A graph
 * names its parameters with `MaterialParam` nodes; `graphParams` lists them
 * in the slot order the renderer uses.
 */

export const BLEND_MODES = ["Alpha", "Additive", "Multiply", "Screen", "PremultipliedAlpha"] as const;

/** A uniform as serde writes `UniformValue`: externally tagged. */
export type Uniform = { Float: number } | { Int: number } | { Vec2: number[] } | { Vec3: number[] } | { Vec4: number[] };

export interface MaterialFile {
  shader: string;
  shader_graph_path?: string;
  blend_mode?: string;
  uniforms?: Record<string, Uniform>;
  [other: string]: unknown;
}

/** A material file read defensively, unknown keys kept. */
export function parseMaterial(text: string): MaterialFile {
  const raw = JSON.parse(text) as Partial<MaterialFile>;
  return { ...raw, shader: typeof raw.shader === "string" ? raw.shader : "default_sprite" };
}

/** Set (or clear) the graph, keeping `shader` saying what draws it. */
export function withGraph(m: MaterialFile, graphPath: string): MaterialFile {
  const out = structuredClone(m);
  if (graphPath.trim()) {
    out.shader_graph_path = graphPath.trim();
    out.shader = "graph";
  } else {
    delete out.shader_graph_path;
    out.shader = "default_sprite";
  }
  return out;
}

/** A graph's parameter names in slot order - sorted, without repeats - as
 *  `material_params::slot_order` places them. */
export function graphParams(graph: unknown): string[] {
  const nodes = (graph as { nodes?: { node_type?: { type?: string; name?: string } }[] })?.nodes ?? [];
  const names = nodes
    .filter((n) => n?.node_type?.type === "MaterialParam" && typeof n.node_type.name === "string")
    .map((n) => n.node_type!.name as string);
  return [...new Set(names)].sort();
}

/** A uniform as the four numbers the shader reads (`as_vec4`). */
export function uniformToVec4(u: Uniform | undefined): [number, number, number, number] {
  if (!u) return [0, 0, 0, 0];
  if ("Float" in u) return [u.Float, 0, 0, 0];
  if ("Int" in u) return [u.Int, 0, 0, 0];
  const a = ("Vec2" in u ? u.Vec2 : "Vec3" in u ? u.Vec3 : u.Vec4) ?? [];
  return [a[0] ?? 0, a[1] ?? 0, a[2] ?? 0, a[3] ?? 0];
}

/** Set parameter `name` to four numbers (or clear it with null). */
export function withUniform(m: MaterialFile, name: string, value: [number, number, number, number] | null): MaterialFile {
  const out = structuredClone(m);
  const uniforms = { ...(out.uniforms ?? {}) };
  if (value === null) delete uniforms[name];
  else uniforms[name] = { Vec4: value };
  if (Object.keys(uniforms).length === 0) delete out.uniforms;
  else out.uniforms = uniforms;
  return out;
}

/** The material's file text: pretty JSON with a closing newline. */
export function formatMaterial(m: MaterialFile): string {
  return JSON.stringify(m, null, 2) + "\n";
}
