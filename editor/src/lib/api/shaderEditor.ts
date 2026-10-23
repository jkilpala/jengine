// TypeScript API for shader editor Tauri commands.
import { invoke } from "@tauri-apps/api/core";

// ─── Types ───────────────────────────────────────────────────────────

export type PinType = "Float" | "Vec2" | "Vec3" | "Vec4" | "Sampler2D";

export interface PinDef {
  name: string;
  pin_type: PinType;
  direction: "Input" | "Output";
}

export interface CustomPin {
  name: string;
  pin_type: PinType;
}

export interface Edge {
  from_node: number;
  from_pin: number;
  to_node: number;
  to_pin: number;
}

/** Tagged union — the `type` field determines the variant. */
export type NodeType =
  | { type: "UvInput" }
  | { type: "ColorInput"; color: [number, number, number, number] }
  | { type: "TimeInput" }
  | { type: "PositionInput" }
  | { type: "ScreenPosition" }
  | { type: "WorldToScreen" }
  | { type: "FloatConstant"; value: number }
  | { type: "Vec2Constant"; value: [number, number] }
  | { type: "Vec3Constant"; value: [number, number, number] }
  | { type: "Vec4Constant"; value: [number, number, number, number] }
  | { type: "TextureSample" }
  | { type: "TexelSize" }
  | { type: "Add" }
  | { type: "Subtract" }
  | { type: "Multiply" }
  | { type: "Divide" }
  | { type: "Modulo" }
  | { type: "Power" }
  | { type: "SquareRoot" }
  | { type: "Abs" }
  | { type: "Negate" }
  | { type: "Floor" }
  | { type: "Ceil" }
  | { type: "Clamp" }
  | { type: "Mix" }
  | { type: "Step" }
  | { type: "SmoothStep" }
  | { type: "Sin" }
  | { type: "Cos" }
  | { type: "Min" }
  | { type: "Max" }
  | { type: "Dot" }
  | { type: "Length" }
  | { type: "Normalize" }
  | { type: "Distance" }
  | { type: "Noise"; octaves?: number }
  | { type: "Voronoi" }
  | { type: "SceneColor" }
  | { type: "Combine"; components: number }
  | { type: "Split" }
  | { type: "ColorMix" }
  | { type: "Brightness" }
  | { type: "Saturation" }
  | { type: "Contrast" }
  | { type: "Emission" }
  | { type: "Outline" }
  | { type: "FragmentOutput" }
  /** Moves the vertices: a world-space offset added before projection. */
  | { type: "VertexOffset" }
  | {
      type: "CustomGlsl";
      code: string;
      inputs: CustomPin[];
      outputs: CustomPin[];
    };

export interface ShaderNode {
  id: number;
  node_type: NodeType;
  position: [number, number];
  label?: string;
}

export interface ShaderGraphMeta {
  name: string;
  description?: string;
  author?: string;
}

export interface ShaderGraph {
  meta: ShaderGraphMeta;
  nodes: ShaderNode[];
  edges: Edge[];
  next_id: number;
}

export interface PaletteCategory {
  category: string;
  nodes: PaletteNode[];
}

export interface PaletteNode {
  name: string;
  node_type: NodeType;
}

export interface ValidationResult {
  valid: boolean;
  errors: string[];
}

export interface GenerateResult {
  /** Complete WGSL module (vertex + fragment entry points). */
  wgsl: string;
}

export interface PinInfo {
  name: string;
  pin_type: string;
}

export interface NodePinsResult {
  inputs: PinInfo[];
  outputs: PinInfo[];
}

// ─── Commands ────────────────────────────────────────────────────────

/** Get the categorized node palette for the sidebar. */
export function shaderGetPalette(): Promise<PaletteCategory[]> {
  return invoke<PaletteCategory[]>("shader_get_palette");
}

/** Validate a shader graph. */
export function shaderValidate(
  graphJson: string,
): Promise<ValidationResult> {
  return invoke<ValidationResult>("shader_validate", {
    graphJson,
  });
}

/** Generate a WGSL module from a shader graph. */
export function shaderGenerate(
  graphJson: string,
): Promise<GenerateResult> {
  return invoke<GenerateResult>("shader_generate", {
    graphJson,
  });
}

export interface PreviewGlslResult {
  /** GLSL ES 300 fragment shader transpiled from the generated WGSL. */
  glsl: string;
  /** The WGSL the GLSL was derived from. */
  wgsl: string;
}

/** Generate a GLSL ES 300 fragment shader for the WebGL2 live preview. */
export function shaderGeneratePreviewGlsl(
  graphJson: string,
): Promise<PreviewGlslResult> {
  return invoke<PreviewGlslResult>("shader_generate_preview_glsl", {
    graphJson,
  });
}

/** Load a .shader file from the project. */
export function shaderLoad(
  projectPath: string,
  relativePath: string,
): Promise<string> {
  return invoke<string>("shader_load", { projectPath, relativePath });
}

/** Save a shader graph to a .shader file. */
export function shaderSave(
  projectPath: string,
  relativePath: string,
  graphJson: string,
): Promise<void> {
  return invoke<void>("shader_save", {
    projectPath,
    relativePath,
    graphJson,
  });
}

/** Create a new empty shader graph with the given name. */
export function shaderCreateNew(name: string): Promise<string> {
  return invoke<string>("shader_create_new", { name });
}

/** Get pin definitions for a node type. */
export function shaderGetNodePins(
  nodeTypeJson: string,
): Promise<NodePinsResult> {
  return invoke<NodePinsResult>("shader_get_node_pins", { nodeTypeJson });
}
