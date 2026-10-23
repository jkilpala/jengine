import { invoke } from "@tauri-apps/api/core";
import { createAssetFile } from "./project";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";

// ---------------------------------------------------------------------------
// GUI Document types (mirrors jengine-core/src/ui/gui_document.rs)
// ---------------------------------------------------------------------------

export type SizeValue =
  | { type: "Fixed"; value: number }
  | { type: "Percent"; value: number }
  | { type: "Auto" };

export interface EdgeInsets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export type FlexDirection = "Row" | "Column";
export type AlignItems = "Start" | "Center" | "End" | "Stretch";
export type JustifyContent = "Start" | "Center" | "End" | "SpaceBetween" | "SpaceAround";
export type GuiTextAlign = "Left" | "Center" | "Right";

export interface FlexLayout {
  direction: FlexDirection;
  gap: number;
  align_items: AlignItems;
  justify_content: JustifyContent;
}

export type GridTrack =
  | { type: "Fixed"; value: number }
  | { type: "Fraction"; value: number }
  | { type: "Auto" };

export interface GridLayout {
  columns: GridTrack[];
  rows: GridTrack[];
  column_gap: number;
  row_gap: number;
}

export type LayoutMode =
  | { mode: "Flex"; direction?: FlexDirection; gap?: number; align_items?: AlignItems; justify_content?: JustifyContent }
  | { mode: "Grid"; columns?: GridTrack[]; rows?: GridTrack[]; column_gap?: number; row_gap?: number };

export type GuiNodeType =
  | { type: "Panel" }
  | { type: "Text"; text: string; font_id: string; font_size: number; color: [number, number, number, number]; alignment: GuiTextAlign; rich?: boolean }
  | { type: "Image"; texture_id: string; tint: [number, number, number, number]; uv_rect?: [number, number, number, number] }
  | { type: "Button"; label: string; font_id: string; font_size: number; text_color: [number, number, number, number]; normal_color: [number, number, number, number]; hover_color: [number, number, number, number]; pressed_color: [number, number, number, number]; disabled?: boolean; alignment?: "Left" | "Center" | "Right"; hover_scale?: number }
  | { type: "ProgressBar"; value: number; min: number; max: number; fg_color: [number, number, number, number]; bg_color: [number, number, number, number]; border_color: [number, number, number, number] }
  | { type: "Slider"; value: number; min: number; max: number; track_color: [number, number, number, number]; thumb_color: [number, number, number, number]; thumb_size: number }
  | { type: "Toggle"; checked: boolean; on_color: [number, number, number, number]; off_color: [number, number, number, number]; thumb_color: [number, number, number, number] }
  | { type: "TextInput"; text: string; placeholder: string; font_id: string; font_size: number; color: [number, number, number, number]; bg_color: [number, number, number, number] }
  | { type: "Circle"; fill_color: [number, number, number, number]; stroke_color: [number, number, number, number]; stroke_width: number };

export interface NodeStyle {
  width: SizeValue;
  height: SizeValue;
  padding: EdgeInsets;
  margin: EdgeInsets;
  flex: FlexLayout;
  bg_color: [number, number, number, number];
  border_color: [number, number, number, number];
  border_width: number;
  visible: boolean;
  z_order: number;
  layout?: LayoutMode;
  grid_column?: number;
  grid_row?: number;
  offset_x: number;
  offset_y: number;
  constraints?: Constraints;
  focusable?: boolean;
  tab_order?: number;
  /** How children that exceed the content box are treated. */
  overflow?: Overflow;
  /** Scroll offsets in px, applied to children when overflow is "Scroll". */
  scroll_x?: number;
  scroll_y?: number;
  /** Rounded corners, px. */
  corner_radius?: number;
  /** 0..1, inherited by the node's children. */
  opacity?: number;
  /** Scale about the node's centre, inherited; applied by layout. */
  scale?: number;
  /** A soft drop shadow under the node. */
  shadow?: GuiShadow | null;
  /** How a screen reader reads the node (`ui::accessibility::Accessible`). */
  accessible?: GuiAccessible | null;
}

/** Mirrors `ui::accessibility::Accessible`. */
export interface GuiAccessible {
  label?: string;
  description?: string;
  hidden?: boolean;
}

/** Mirrors `ui::gui_document::GuiShadow`: offset is x right, y down. */
export interface GuiShadow {
  color: [number, number, number, number];
  offset: [number, number];
  blur: number;
}

/** Mirrors `ui::gui_document::Overflow` in the engine. */
export type Overflow = "Visible" | "Hidden" | "Scroll";

// ---------------------------------------------------------------------------
// Constraint types (edge-pinning)
// ---------------------------------------------------------------------------

export type ConstraintValue =
  | { type: "Px"; value: number }
  | { type: "Percent"; value: number };

export interface Constraints {
  left?: ConstraintValue;
  right?: ConstraintValue;
  top?: ConstraintValue;
  bottom?: ConstraintValue;
}

export function defaultConstraints(): Constraints {
  return {};
}

export function constraintsIsEmpty(c?: Constraints): boolean {
  return !c || (!c.left && !c.right && !c.top && !c.bottom);
}

export function resolveConstraintValue(cv: ConstraintValue, parentExtent: number): number {
  return cv.type === "Px" ? cv.value : (parentExtent * cv.value) / 100;
}

export interface GuiNode {
  id: string;
  node_type: GuiNodeType;
  style: NodeStyle;
  children: GuiNode[];
}

export type GuiAnchor = "TopLeft" | "TopCenter" | "TopRight" | "CenterLeft" | "Center" | "CenterRight" | "BottomLeft" | "BottomCenter" | "BottomRight";

export interface GuiDocument {
  name: string;
  anchor: GuiAnchor;
  root: GuiNode;
  /** `false` when the document's script runs its own cursor: the engine's
   *  controller / arrow-key navigation then leaves it alone. Absent is `true`. */
  navigable?: boolean;
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

export function defaultEdgeInsets(): EdgeInsets {
  return { top: 0, right: 0, bottom: 0, left: 0 };
}

export function defaultFlexLayout(): FlexLayout {
  return { direction: "Column", gap: 0, align_items: "Start", justify_content: "Start" };
}

export function defaultGridLayout(): GridLayout {
  return { columns: [{ type: "Fraction", value: 1 }], rows: [{ type: "Fraction", value: 1 }], column_gap: 0, row_gap: 0 };
}

export function defaultNodeStyle(): NodeStyle {
  return {
    width: { type: "Auto" },
    height: { type: "Auto" },
    padding: defaultEdgeInsets(),
    margin: defaultEdgeInsets(),
    flex: defaultFlexLayout(),
    bg_color: [0, 0, 0, 0],
    border_color: [0, 0, 0, 0],
    border_width: 0,
    visible: true,
    z_order: 0,
    offset_x: 0,
    offset_y: 0,
  };
}

export function defaultGuiNode(id: string, nodeType: GuiNodeType): GuiNode {
  return {
    id,
    node_type: nodeType,
    style: defaultNodeStyle(),
    children: [],
  };
}

export function defaultGuiDocument(name: string): GuiDocument {
  return {
    name,
    anchor: "TopLeft",
    root: {
      id: "root",
      node_type: { type: "Panel" },
      style: {
        ...defaultNodeStyle(),
        width: { type: "Percent", value: 100 },
        height: { type: "Percent", value: 100 },
        flex: { direction: "Column", gap: 0, align_items: "Center", justify_content: "Center" },
      },
      children: [],
    },
  };
}

// ---------------------------------------------------------------------------
// Node type helpers
// ---------------------------------------------------------------------------

export function getNodeTypeName(nodeType: GuiNodeType): string {
  return nodeType.type;
}

export function createNodeType(typeName: string): GuiNodeType {
  switch (typeName) {
    case "Panel":
      return { type: "Panel" };
    case "Text":
      return { type: "Text", text: "Text", font_id: "", font_size: 1, color: [1, 1, 1, 1], alignment: "Left" };
    case "Image":
      return { type: "Image", texture_id: "", tint: [1, 1, 1, 1] };
    case "Button":
      return { type: "Button", label: "Button", font_id: "", font_size: 1, normal_color: [0.3, 0.3, 0.35, 1], hover_color: [0.4, 0.4, 0.45, 1], pressed_color: [0.2, 0.2, 0.25, 1], text_color: [1, 1, 1, 1] };
    case "ProgressBar":
      return { type: "ProgressBar", value: 0.5, min: 0, max: 1, fg_color: [0.2, 0.8, 0.3, 1], bg_color: [0.2, 0.2, 0.2, 1], border_color: [0.4, 0.4, 0.4, 1] };
    case "Slider":
      return { type: "Slider", value: 0.5, min: 0, max: 1, track_color: [0.3, 0.3, 0.3, 1], thumb_color: [0.8, 0.8, 0.8, 1], thumb_size: 12 };
    case "Toggle":
      return { type: "Toggle", checked: false, on_color: [0.2, 0.8, 0.3, 1], off_color: [0.4, 0.4, 0.4, 1], thumb_color: [1, 1, 1, 1] };
    case "TextInput":
      return { type: "TextInput", text: "", placeholder: "Enter text...", font_id: "", font_size: 1, color: [1, 1, 1, 1], bg_color: [0.15, 0.15, 0.2, 1] };
    case "Circle":
      return { type: "Circle", fill_color: [1, 1, 1, 1], stroke_color: [0, 0, 0, 0], stroke_width: 0 };
    default:
      return { type: "Panel" };
  }
}

// ---------------------------------------------------------------------------
// File I/O (uses existing Tauri commands)
// ---------------------------------------------------------------------------

/** Create a new .gui file in the project's assets directory. */
export async function createGuiFile(projectPath: string, relativePath: string, name: string): Promise<void> {
  const doc = defaultGuiDocument(name);
  const json = JSON.stringify(doc, null, 2);
  await createAssetFile(projectPath, relativePath, json);
}

/** Read a .gui file and parse it. Migrates old externally-tagged format if needed. */
export async function readGuiFile(fullPath: string): Promise<GuiDocument> {
  const contents = await readTextFile(fullPath);
  const doc = JSON.parse(contents) as GuiDocument;
  migrateNode(doc.root);
  // Rename flex_layout → flex in styles
  renameFlexField(doc.root);
  // Fill in missing style defaults (JSON on disk omits fields with default values)
  applyStyleDefaults(doc.root);
  return doc;
}

/** Migrate a node tree from old externally-tagged format to internally-tagged. */
function migrateNode(node: GuiNode): void {
  const nt = node.node_type as Record<string, unknown>;
  if (!nt.type) {
    // Old format: { "Panel": { bg_color: [...] } } → { type: "Panel" }
    const key = Object.keys(nt)[0];
    if (key) {
      const data = (nt[key] as Record<string, unknown>) || {};
      // Clear old key
      delete nt[key];
      nt.type = key;
      // Copy variant fields to top level
      for (const [k, v] of Object.entries(data)) {
        // Panel.bg_color → move to style.bg_color instead (Panel has no fields in Rust)
        if (key === "Panel" && k === "bg_color") {
          node.style.bg_color = v as [number, number, number, number];
          continue;
        }
        // Image.color → Image.tint
        if (key === "Image" && k === "color") {
          nt.tint = v;
          continue;
        }
        nt[k] = v;
      }
    }
  }
  for (const child of node.children) {
    migrateNode(child);
  }
}

/** Rename flex_layout → flex in style objects (old TS field name). */
function renameFlexField(node: GuiNode): void {
  const style = node.style as Record<string, unknown>;
  if ("flex_layout" in style && !("flex" in style)) {
    style.flex = style.flex_layout;
    delete style.flex_layout;
  }
  for (const child of node.children) {
    renameFlexField(child);
  }
}

/** Fill in missing style fields with defaults (JSON on disk may omit them). */
function applyStyleDefaults(node: GuiNode): void {
  const def = defaultNodeStyle();
  const s = node.style;
  if (s.width == null) s.width = def.width;
  if (s.height == null) s.height = def.height;
  if (s.padding == null) s.padding = def.padding;
  if (s.margin == null) s.margin = def.margin;
  if (s.flex == null) s.flex = def.flex;
  if (s.bg_color == null) s.bg_color = def.bg_color;
  if (s.border_color == null) s.border_color = def.border_color;
  if (s.border_width == null) s.border_width = def.border_width;
  if (s.visible == null) s.visible = def.visible;
  if (s.z_order == null) s.z_order = def.z_order;
  if (s.offset_x == null) s.offset_x = def.offset_x;
  if (s.offset_y == null) s.offset_y = def.offset_y;
  for (const child of node.children) {
    applyStyleDefaults(child);
  }
}

/** Save a GUI document to a file. */
export async function saveGuiFile(fullPath: string, doc: GuiDocument): Promise<void> {
  const json = JSON.stringify(doc, null, 2);
  await writeTextFile(fullPath, json);
}

// ---------------------------------------------------------------------------
// Node tree helpers
// ---------------------------------------------------------------------------

/** Find a node by ID in the tree. */
export function findNode(root: GuiNode, id: string): GuiNode | null {
  if (root.id === id) return root;
  for (const child of root.children) {
    const found = findNode(child, id);
    if (found) return found;
  }
  return null;
}

/** Find the parent of a node by ID. Returns null for root. */
export function findParent(root: GuiNode, targetId: string): GuiNode | null {
  for (const child of root.children) {
    if (child.id === targetId) return root;
    const found = findParent(child, targetId);
    if (found) return found;
  }
  return null;
}

/** Remove a node from the tree by ID. Returns true if removed. */
export function removeNode(root: GuiNode, id: string): boolean {
  const idx = root.children.findIndex((c) => c.id === id);
  if (idx >= 0) {
    root.children.splice(idx, 1);
    return true;
  }
  for (const child of root.children) {
    if (removeNode(child, id)) return true;
  }
  return false;
}

/** Generate a unique node ID. */
let nextNodeId = 1;
export function generateNodeId(prefix = "node"): string {
  return `${prefix}_${nextNodeId++}`;
}
