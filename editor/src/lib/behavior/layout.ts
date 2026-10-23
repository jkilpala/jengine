/**
 * Behavior-graph layout + geometry utilities (Slice E.3).
 *
 * Pure functions: parsing a `.behavior` JSON document into the
 * editor's working shape, computing per-node bounding boxes from
 * the op catalog, and laying out nodes that lack saved positions
 * via a hand-rolled layered (Sugiyama-lite) algorithm.
 */

import type { OpDescriptor, PinDescriptor } from "../api/behavior";

// ─── Geometry constants ───────────────────────────────────────────

/** Width of every node card. */
export const NODE_WIDTH = 200;
/** Height of the coloured header (op name + exec ports). */
export const NODE_HEADER_HEIGHT = 28;
/** Vertical pitch of one input/output pin row. */
export const NODE_PIN_HEIGHT = 20;
/** Vertical padding inside the body below the last pin. */
export const NODE_BODY_PADDING = 8;
/** Horizontal pitch between layered ranks for auto-layout. */
export const LAYOUT_RANK_PITCH = 260;
/** Vertical pitch between sibling nodes within a rank. */
export const LAYOUT_ROW_PITCH = 140;
/** Top-left origin for auto-layout. */
export const LAYOUT_ORIGIN_X = 80;
export const LAYOUT_ORIGIN_Y = 80;

/** Compute the full height of a node card given its pin counts. */
export function nodeHeight(inputCount: number, outputCount: number): number {
    const rows = Math.max(inputCount, outputCount);
    return NODE_HEADER_HEIGHT + rows * NODE_PIN_HEIGHT + NODE_BODY_PADDING;
}

// ─── Parsed document shape (mirror of BehaviorAsset) ─────────────

export interface BehaviorPinBindingLiteral {
    value: unknown;
}
export interface BehaviorPinBindingReference {
    from: string;
    out?: string;
    scale?: number;
    offset?: number;
}
export type BehaviorPinBinding =
    | BehaviorPinBindingLiteral
    | BehaviorPinBindingReference;

export function isReferenceBinding(
    b: BehaviorPinBinding,
): b is BehaviorPinBindingReference {
    return typeof (b as BehaviorPinBindingReference).from === "string";
}

export interface BehaviorNode {
    id: string;
    op: string;
    params?: Record<string, unknown>;
    in?: Record<string, BehaviorPinBinding>;
}

export interface BehaviorEdge {
    from: string;
    out?: string; // exec | then | else | other
    to: string;
    in?: string;
}

export interface BehaviorPosition {
    x: number;
    y: number;
}

/**
 * Free-floating note attached to the canvas. Pure editor metadata
 * — the runtime preserves it through `editor.extra` but never
 * inspects it. Color is an optional CSS color string; when
 * omitted the canvas falls back to a default Catppuccin yellow.
 */
export interface BehaviorComment {
    id: string;
    text: string;
    x: number;
    y: number;
    w: number;
    h: number;
    color?: string;
}

export interface BehaviorEditorMeta {
    positions?: Record<string, BehaviorPosition>;
    comments?: BehaviorComment[];
    [extra: string]: unknown;
}

export interface BehaviorDoc {
    name: string;
    version: number;
    nodes: BehaviorNode[];
    edges: BehaviorEdge[];
    editor?: BehaviorEditorMeta;
}

/** Parse a JSON string into a working `BehaviorDoc`. Returns
 *  `null` on parse failure (the diagnostic strip already reports
 *  the parse error separately). */
export function parseBehaviorDoc(json: string): BehaviorDoc | null {
    try {
        const raw = JSON.parse(json) as Partial<BehaviorDoc>;
        return {
            name: raw.name ?? "",
            version: raw.version ?? 1,
            nodes: raw.nodes ?? [],
            edges: raw.edges ?? [],
            editor: raw.editor,
        };
    } catch {
        return null;
    }
}

// ─── Auto-layout (layered, control-flow-driven) ──────────────────

/**
 * Compute a position map for every node in `doc`, preferring saved
 * `editor.positions` and filling in the remainder via a layered
 * placement keyed off control-flow edges.
 */
export function layoutNodes(
    doc: BehaviorDoc,
): Record<string, BehaviorPosition> {
    const positions: Record<string, BehaviorPosition> = {};
    const saved = doc.editor?.positions ?? {};

    // Start from saved positions (copy to avoid mutating input).
    for (const [id, pos] of Object.entries(saved)) {
        positions[id] = { x: pos.x, y: pos.y };
    }

    const missing = doc.nodes.filter((n) => positions[n.id] == null);
    if (missing.length === 0) return positions;

    // Build control-flow adjacency (only `from -> to` edges count
    // as "predecessor"; data bindings do not influence layout).
    const incoming = new Map<string, string[]>();
    const outgoing = new Map<string, string[]>();
    for (const n of doc.nodes) {
        incoming.set(n.id, []);
        outgoing.set(n.id, []);
    }
    for (const e of doc.edges) {
        outgoing.get(e.from)?.push(e.to);
        incoming.get(e.to)?.push(e.from);
    }

    // Assign rank = longest path from any root (node with no
    // incoming control edges). Cycles fall back to rank 0.
    const rank = new Map<string, number>();
    const visiting = new Set<string>();
    function rankOf(id: string): number {
        const cached = rank.get(id);
        if (cached != null) return cached;
        if (visiting.has(id)) return 0;
        visiting.add(id);
        const preds = incoming.get(id) ?? [];
        let r = 0;
        for (const p of preds) {
            r = Math.max(r, rankOf(p) + 1);
        }
        visiting.delete(id);
        rank.set(id, r);
        return r;
    }
    for (const n of doc.nodes) rankOf(n.id);

    // Bucket nodes by rank (only those needing layout). Sort
    // within rank by id for determinism.
    const byRank = new Map<number, string[]>();
    for (const n of missing) {
        const r = rank.get(n.id) ?? 0;
        const list = byRank.get(r) ?? [];
        list.push(n.id);
        byRank.set(r, list);
    }
    for (const list of byRank.values()) list.sort();

    // Track which y-slots are occupied per rank by saved nodes so
    // we don't stack new nodes on top of them.
    const usedSlots = new Map<number, Set<number>>();
    for (const n of doc.nodes) {
        const pos = positions[n.id];
        if (pos == null) continue;
        const r = rank.get(n.id) ?? 0;
        const slot = Math.round((pos.y - LAYOUT_ORIGIN_Y) / LAYOUT_ROW_PITCH);
        const slots = usedSlots.get(r) ?? new Set();
        slots.add(slot);
        usedSlots.set(r, slots);
    }

    for (const [r, ids] of byRank) {
        const slots = usedSlots.get(r) ?? new Set<number>();
        let nextSlot = 0;
        for (const id of ids) {
            while (slots.has(nextSlot)) nextSlot++;
            slots.add(nextSlot);
            positions[id] = {
                x: LAYOUT_ORIGIN_X + r * LAYOUT_RANK_PITCH,
                y: LAYOUT_ORIGIN_Y + nextSlot * LAYOUT_ROW_PITCH,
            };
            nextSlot++;
        }
        usedSlots.set(r, slots);
    }

    return positions;
}

// ─── Op-catalog lookup helpers ───────────────────────────────────

export interface CatalogIndex {
    byOp: Map<string, OpDescriptor>;
}

export function buildCatalogIndex(catalog: OpDescriptor[]): CatalogIndex {
    const byOp = new Map<string, OpDescriptor>();
    for (const op of catalog) byOp.set(op.op, op);
    return { byOp };
}

/** Returns the y-offset (relative to the node's top) of an input pin. */
export function inputPinY(index: number): number {
    return NODE_HEADER_HEIGHT + index * NODE_PIN_HEIGHT + NODE_PIN_HEIGHT / 2;
}

/** Returns the y-offset of an output pin. Symmetric with inputs. */
export function outputPinY(index: number): number {
    return NODE_HEADER_HEIGHT + index * NODE_PIN_HEIGHT + NODE_PIN_HEIGHT / 2;
}

/** Index of a named input pin in the descriptor's `inputs` array,
 *  or -1 if not found. */
export function inputPinIndex(
    desc: OpDescriptor | undefined,
    name: string,
): number {
    if (!desc) return -1;
    return desc.inputs.findIndex((p) => p.name === name);
}

export function outputPinIndex(
    desc: OpDescriptor | undefined,
    name: string,
): number {
    if (!desc) return -1;
    return desc.outputs.findIndex((p) => p.name === name);
}

/** Look up a pin descriptor by output name (defaults to first
 *  output if `name` is undefined — matches runtime behaviour). */
export function findOutputPin(
    desc: OpDescriptor | undefined,
    name: string | undefined,
): { index: number; pin: PinDescriptor } | null {
    if (!desc || desc.outputs.length === 0) return null;
    if (name == null) {
        return { index: 0, pin: desc.outputs[0] };
    }
    const idx = desc.outputs.findIndex((p) => p.name === name);
    if (idx < 0) return null;
    return { index: idx, pin: desc.outputs[idx] };
}

// ─── Visual styling ──────────────────────────────────────────────

/**
 * Category header colours, as the editor's category tokens (theme.css), so
 * they follow the palette. Mirrors `EffectKind` from the runtime catalog.
 * They are drawn as SVG fills and inline styles, which resolve `var()`.
 */
export function categoryColor(category: string): string {
    switch (category) {
        case "Trigger":
            return "var(--ed-cat-1)";
        case "Read":
            return "var(--ed-cat-5)";
        case "Pure":
            return "var(--ed-cat-2)";
        case "Gate":
            return "var(--ed-cat-4)";
        case "DirectWrite":
            return "var(--ed-cat-7)";
        case "SetupComponent":
            return "var(--ed-cat-3)";
        case "StructuralCommand":
            return "var(--ed-cat-6)";
        case "SideEffect":
            return "var(--ed-cat-8)";
        default:
            return "var(--ed-faint)";
    }
}

/** Edge colour by pin type (data bindings). */
export function pinTypeColor(ty: string): string {
    switch (ty) {
        case "Vec2":
            return "var(--ed-cat-2)";
        case "Float":
            return "var(--ed-cat-4)";
        case "Bool":
            return "var(--ed-cat-8)";
        case "EntityRef":
            return "var(--ed-cat-7)";
        case "Aabb":
            return "var(--ed-text)";
        case "Enum":
            return "var(--ed-cat-6)";
        case "String":
            return "var(--ed-cat-3)";
        default:
            return "var(--ed-muted)";
    }
}

/** Colour used for control (exec) edges. */
export const CONTROL_EDGE_COLOR = "var(--ed-text)";

/** Visual radius of an exec pin (drawn as a diamond). */
export const EXEC_PIN_RADIUS = 5;

/** Y-offset (relative to node top) for exec_out pin `index` when
 *  `count` exec_out names are stacked vertically inside the
 *  header. For a single output we centre it on the header midline. */
export function execOutPinY(index: number, count: number): number {
    if (count <= 1) return NODE_HEADER_HEIGHT / 2;
    const spacing = NODE_HEADER_HEIGHT / (count + 1);
    return spacing * (index + 1);
}

/** Y-offset for the lone exec_in pin (always header midline). */
export function execInPinY(): number {
    return NODE_HEADER_HEIGHT / 2;
}

/** Returns the index of an exec_out name within a descriptor's
 *  `exec_out` array, or -1. Treats `null` / `undefined` / `"exec"`
 *  identically when the descriptor only emits one default exec. */
export function execOutIndex(
    desc: OpDescriptor | undefined,
    name: string | null | undefined,
): number {
    if (!desc || desc.exec_out.length === 0) return -1;
    if (name == null) return desc.exec_out.length === 1 ? 0 : -1;
    return desc.exec_out.indexOf(name);
}

/** True if a wire from `(srcKind, srcType)` may legally connect to
 *  `(dstKind, dstType)`. Exec ↔ exec; data ↔ data with same
 *  `PinType`. */
export function pinsCompatible(
    srcKind: "exec" | "data",
    srcType: string | null,
    dstKind: "exec" | "data",
    dstType: string | null,
): boolean {
    if (srcKind !== dstKind) return false;
    if (srcKind === "exec") return true;
    return srcType != null && srcType === dstType;
}

/** MIME type used to ferry an op name through HTML5 drag-and-drop
 *  from the palette onto the canvas. */
export const PALETTE_DND_MIME = "application/x-jengine-op";
