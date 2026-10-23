import { writable, get, type Readable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import {
    compileBehavior,
    loadBehaviorOpCatalog,
    writeBehavior,
    type CompileReport,
    type OpDescriptor,
} from "../api/behavior";
import {
    parseBehaviorDoc,
    isReferenceBinding,
    type BehaviorComment,
    type BehaviorDoc,
    type BehaviorEdge,
    type BehaviorNode,
    type BehaviorPosition,
} from "../behavior/layout";
import { logError, logInfo } from "./consoleStore";

/**
 * Behavior-graph editor store.
 *
 * - Slice E.1: op catalog cached for the session.
 * - Slice E.2: per-document state (path, raw JSON, latest compile
 *   report) for the read-only `BehaviorEditor.svelte` panel.
 */

// ─── Op catalog (Slice E.1) ───────────────────────────────────────

const internal = writable<OpDescriptor[] | null>(null);

/** Read-only catalog store. `null` until {@link ensureOpCatalog} resolves. */
export const opCatalog: Readable<OpDescriptor[] | null> = internal;

let inflight: Promise<OpDescriptor[]> | null = null;

/**
 * Loads the op catalog from the runtime, deduplicating concurrent
 * callers. Resolves with the cached catalog on subsequent calls.
 */
export function ensureOpCatalog(): Promise<OpDescriptor[]> {
    if (!inflight) {
        inflight = loadBehaviorOpCatalog()
            .then((catalog) => {
                internal.set(catalog);
                return catalog;
            })
            .catch((err) => {
                inflight = null; // allow retry
                throw err;
            });
    }
    return inflight;
}

// ─── Open document state (Slice E.2) ──────────────────────────────

/** Raw JSON of the currently open `.behavior` file (`null` if none). */
export const behaviorJson = writable<string | null>(null);

/** Full path of the currently open `.behavior` file (`null` if none). */
export const behaviorFilePath = writable<string | null>(null);

/** Latest compile report for the open document (`null` if none open). */
export const behaviorReport = writable<CompileReport | null>(null);

/** Status of the validator pipeline. Drives the header pill. */
export type CompileStatus = "idle" | "validating" | "ok" | "warn" | "error";
export const compileStatus = writable<CompileStatus>("idle");

/**
 * Slice E.8: when set, the canvas pans/centres on the named node
 * and resets the writable to `null`. The bumped `seq` lets the
 * canvas distinguish two clicks on the same diagnostic.
 */
export const focusNodeRequest = writable<{ nodeId: string; seq: number } | null>(
    null,
);
let focusSeq = 0;
export function requestFocusNode(nodeId: string): void {
    focusSeq += 1;
    focusNodeRequest.set({ nodeId, seq: focusSeq });
}

function splitPath(fullPath: string): {
    projectPath: string;
    relativePath: string;
} {
    const normalized = fullPath.replace(/\\/g, "/");
    const assetsIdx = normalized.indexOf("/assets/");
    if (assetsIdx >= 0) {
        return {
            projectPath: normalized.slice(0, assetsIdx),
            relativePath: normalized.slice(assetsIdx + "/assets/".length),
        };
    }
    const lastSlash = normalized.lastIndexOf("/");
    return {
        projectPath: normalized.slice(0, lastSlash),
        relativePath: normalized.slice(lastSlash + 1),
    };
}

/**
 * Opens a `.behavior` file in the editor. Reads its JSON via the
 * existing `read_asset_binary` command and runs `compile_behavior`
 * to populate the diagnostic strip. Errors are logged to the
 * console but never throw.
 */
export async function openBehaviorFile(fullPath: string) {
    try {
        const { projectPath, relativePath } = splitPath(fullPath);
        const bytes = await invoke<number[]>("read_asset_binary", {
            projectPath,
            relativePath,
        });
        const text = new TextDecoder().decode(new Uint8Array(bytes));
        behaviorJson.set(text);
        behaviorFilePath.set(fullPath);
        selectedNodeIds.set(new Set());
        behaviorDirty.set(false);
        undoStack = [];
        redoStack = [];
        undoDepth.set(0);
        redoDepth.set(0);
        cancelPendingCompile();
        compileStatus.set("validating");
        const report = await compileBehavior(text);
        applyReport(report);
        logInfo(`Opened behavior: ${fullPath.split(/[\\/]/).pop()}`);
    } catch (err) {
        logError(`Failed to open behavior: ${err}`);
    }
}

function applyReport(report: CompileReport): void {
    behaviorReport.set(report);
    if (!report.ok) compileStatus.set("error");
    else if (report.warnings.length > 0) compileStatus.set("warn");
    else compileStatus.set("ok");
}

/** Re-runs the compiler against the current in-memory JSON. */
export async function recompileCurrentBehavior() {
    const json = get(behaviorJson);
    if (json == null) return;
    try {
        const report = await compileBehavior(json);
        applyReport(report);
    } catch (err) {
        logError(`Failed to compile behavior: ${err}`);
    }
}

// Slice E.8: 150 ms debounce so rapid edits coalesce into one
// compile call. The pending timer is cleared on close.
let compileDebounce: ReturnType<typeof setTimeout> | null = null;
function scheduleRecompile(): void {
    if (get(behaviorJson) == null) return;
    compileStatus.set("validating");
    if (compileDebounce != null) clearTimeout(compileDebounce);
    compileDebounce = setTimeout(() => {
        compileDebounce = null;
        void recompileCurrentBehavior();
    }, 150);
}
function cancelPendingCompile(): void {
    if (compileDebounce != null) {
        clearTimeout(compileDebounce);
        compileDebounce = null;
    }
}

/** Closes the open behavior document, clearing all related state. */
export function closeBehaviorEditor() {
    cancelPendingCompile();
    behaviorJson.set(null);
    behaviorFilePath.set(null);
    behaviorReport.set(null);
    compileStatus.set("idle");
    focusNodeRequest.set(null);
    selectedNodeIds.set(new Set());
    behaviorDirty.set(false);
    undoStack = [];
    redoStack = [];
    undoDepth.set(0);
    redoDepth.set(0);
}

// ─── Slice E.4: selection, mutation, undo, save ──────────────────

/** IDs of currently-selected nodes. Updated by canvas + keyboard. */
export const selectedNodeIds = writable<Set<string>>(new Set());

/** True once the document has been mutated since open or save. */
export const behaviorDirty = writable<boolean>(false);

/** How many entries are on the undo stack (for toolbar enable state). */
export const undoDepth = writable<number>(0);
/** How many entries are on the redo stack. */
export const redoDepth = writable<number>(0);

let undoStack: string[] = [];
let redoStack: string[] = [];

function pushUndoSnapshot(json: string) {
    undoStack.push(json);
    if (undoStack.length > 100) undoStack.shift();
    redoStack = [];
    undoDepth.set(undoStack.length);
    redoDepth.set(0);
}

/** Replace the live JSON, clear redo, mark dirty, recompile. The
 *  caller is responsible for having pushed an undo snapshot first
 *  (or grouping mutations through a drag/commit pair). */
function setLiveJson(nextJson: string) {
    behaviorJson.set(nextJson);
    behaviorDirty.set(true);
    scheduleRecompile();
}

function serializeDoc(doc: BehaviorDoc): string {
    // Deterministic serialisation lives in Slice E.9; for E.4 we
    // emit pretty-printed JSON so saved files stay readable.
    return JSON.stringify(doc, null, 2) + "\n";
}

function withDoc(mutator: (doc: BehaviorDoc) => void): void {
    const json = get(behaviorJson);
    if (json == null) return;
    const doc = parseBehaviorDoc(json);
    if (!doc) return;
    pushUndoSnapshot(json);
    mutator(doc);
    setLiveJson(serializeDoc(doc));
}

/** Move a set of nodes by (dx, dy). Used by drag-end commit. */
export function commitMoveNodes(
    ids: Iterable<string>,
    dx: number,
    dy: number,
): void {
    if (dx === 0 && dy === 0) return;
    const idSet = new Set(ids);
    if (idSet.size === 0) return;
    withDoc((doc) => {
        const editor = (doc.editor ??= {} as { positions?: Record<string, BehaviorPosition> });
        const positions = (editor.positions ??= {});
        for (const id of idSet) {
            const cur = positions[id] ?? { x: 0, y: 0 };
            positions[id] = { x: cur.x + dx, y: cur.y + dy };
        }
    });
}

/** Persist absolute positions (used when dragging a node that had
 *  no saved position — the canvas resolves the layered fallback
 *  position before applying the delta). */
export function commitSetNodePositions(
    next: Map<string, BehaviorPosition>,
): void {
    if (next.size === 0) return;
    withDoc((doc) => {
        const editor = (doc.editor ??= {} as { positions?: Record<string, BehaviorPosition> });
        const positions = (editor.positions ??= {});
        for (const [id, pos] of next) {
            positions[id] = { x: pos.x, y: pos.y };
        }
    });
}

/** Delete every selected node + every edge that touches one of them. */
export function deleteSelectedNodes(): void {
    const selected = get(selectedNodeIds);
    if (selected.size === 0) return;
    withDoc((doc) => {
        doc.nodes = doc.nodes.filter((n) => !selected.has(n.id));
        doc.edges = doc.edges.filter(
            (e) => !selected.has(e.from) && !selected.has(e.to),
        );
        // Drop any data bindings referencing deleted nodes.
        for (const node of doc.nodes) {
            if (!node.in) continue;
            for (const [pin, binding] of Object.entries(node.in)) {
                if (
                    typeof (binding as { from?: string }).from === "string" &&
                    selected.has((binding as { from: string }).from)
                ) {
                    delete node.in[pin];
                }
            }
        }
        // Strip orphaned positions to keep editor.positions tidy.
        if (doc.editor?.positions) {
            for (const id of Object.keys(doc.editor.positions)) {
                if (selected.has(id)) delete doc.editor.positions[id];
            }
        }
    });
    selectedNodeIds.set(new Set());
}

// ─── Slice E.10d: copy / cut / paste ─────────────────────────────

/**
 * Snapshot of a clipboard payload. Stores the deep-cloned subset
 * of nodes (with their original ids), the internal edges, and
 * world-space positions. External bindings/edges are dropped at
 * copy time so paste only ever produces self-contained subgraphs.
 *
 * Module-scoped: lives for the editor session; not persisted to
 * the OS clipboard. Cross-tab copy/paste between two open
 * `.behavior` files works because the variable is shared.
 */
interface BehaviorClipboard {
    nodes: BehaviorNode[];
    edges: BehaviorEdge[];
    positions: Record<string, BehaviorPosition>;
}
let clipboard: BehaviorClipboard | null = null;

/** Returns true if the clipboard currently holds a payload. */
export function hasBehaviorClipboard(): boolean {
    return clipboard != null;
}

function snapshotSelectionToClipboard(): BehaviorClipboard | null {
    const json = get(behaviorJson);
    if (json == null) return null;
    const doc = parseBehaviorDoc(json);
    if (!doc) return null;
    const selected = get(selectedNodeIds);
    if (selected.size === 0) return null;
    const nodes: BehaviorNode[] = [];
    const positions: Record<string, BehaviorPosition> = {};
    for (const n of doc.nodes) {
        if (!selected.has(n.id)) continue;
        // Deep-clone via JSON to detach from the live doc.
        const cloned = JSON.parse(JSON.stringify(n)) as BehaviorNode;
        // Drop data bindings whose source is outside the selection
        // — they'd be dangling refs after paste.
        if (cloned.in) {
            for (const [pin, binding] of Object.entries(cloned.in)) {
                if (
                    isReferenceBinding(binding) &&
                    !selected.has(binding.from)
                ) {
                    delete cloned.in[pin];
                }
            }
        }
        nodes.push(cloned);
        const pos = doc.editor?.positions?.[n.id];
        if (pos) positions[n.id] = { x: pos.x, y: pos.y };
    }
    if (nodes.length === 0) return null;
    const edges: BehaviorEdge[] = doc.edges
        .filter((e) => selected.has(e.from) && selected.has(e.to))
        .map((e) => ({ ...e }));
    return { nodes, edges, positions };
}

/** Copy the current selection to the editor-session clipboard. */
export function copySelectedNodes(): void {
    const snap = snapshotSelectionToClipboard();
    if (!snap) return;
    clipboard = snap;
    logInfo(
        `Copied ${snap.nodes.length} behavior node${snap.nodes.length === 1 ? "" : "s"}`,
    );
}

/** Cut: copy then delete (single undo entry for the delete). */
export function cutSelectedNodes(): void {
    const snap = snapshotSelectionToClipboard();
    if (!snap) return;
    clipboard = snap;
    deleteSelectedNodes();
    logInfo(
        `Cut ${snap.nodes.length} behavior node${snap.nodes.length === 1 ? "" : "s"}`,
    );
}

/**
 * Paste the clipboard payload, generating fresh `n<i>` ids,
 * remapping internal edges + bindings, and offsetting positions
 * by (`offsetX`, `offsetY`) world units (default `(20, 20)` so
 * paste-in-place still nudges off the original). Selects the
 * pasted nodes. Single undo entry.
 */
export function pasteClipboard(offsetX = 20, offsetY = 20): void {
    const cb = clipboard;
    if (!cb) return;
    const json = get(behaviorJson);
    if (json == null) return;
    const newSelection = new Set<string>();
    withDoc((doc) => {
        // Allocate fresh ids in one pass so id collisions across
        // pasted nodes can't occur.
        const idMap = new Map<string, string>();
        const taken = new Set(doc.nodes.map((n) => n.id));
        for (const n of cb.nodes) {
            let i = taken.size;
            while (taken.has(`n${i}`)) i++;
            const fresh = `n${i}`;
            idMap.set(n.id, fresh);
            taken.add(fresh);
        }
        for (const n of cb.nodes) {
            const cloned = JSON.parse(JSON.stringify(n)) as BehaviorNode;
            cloned.id = idMap.get(n.id)!;
            // Remap any data bindings whose source is also in the
            // paste set; bindings to outsiders were already dropped
            // at copy time.
            if (cloned.in) {
                for (const binding of Object.values(cloned.in)) {
                    if (isReferenceBinding(binding)) {
                        const remapped = idMap.get(binding.from);
                        if (remapped) binding.from = remapped;
                    }
                }
            }
            doc.nodes.push(cloned);
            newSelection.add(cloned.id);
        }
        for (const e of cb.edges) {
            const from = idMap.get(e.from);
            const to = idMap.get(e.to);
            if (from && to) doc.edges.push({ ...e, from, to });
        }
        const editor = (doc.editor ??= {} as {
            positions?: Record<string, BehaviorPosition>;
        });
        const positions = (editor.positions ??= {});
        for (const [oldId, pos] of Object.entries(cb.positions)) {
            const fresh = idMap.get(oldId);
            if (!fresh) continue;
            positions[fresh] = {
                x: Math.round(pos.x + offsetX),
                y: Math.round(pos.y + offsetY),
            };
        }
    });
    selectedNodeIds.set(newSelection);
}

/** Undo the last mutation. */
export function undoBehavior(): void {
    if (undoStack.length === 0) return;
    const current = get(behaviorJson);
    if (current == null) return;
    redoStack.push(current);
    const prev = undoStack.pop()!;
    behaviorJson.set(prev);
    behaviorDirty.set(true);
    undoDepth.set(undoStack.length);
    redoDepth.set(redoStack.length);
    scheduleRecompile();
}

/** Redo the last undone mutation. */
export function redoBehavior(): void {
    if (redoStack.length === 0) return;
    const current = get(behaviorJson);
    if (current == null) return;
    undoStack.push(current);
    const next = redoStack.pop()!;
    behaviorJson.set(next);
    behaviorDirty.set(true);
    undoDepth.set(undoStack.length);
    redoDepth.set(redoStack.length);
    scheduleRecompile();
}

/** Persist the current document to disk. No-op if nothing is open.
 *  Slice E.8: if the latest compile report has errors, prompts the
 *  user to confirm — a saved file with errors will not load at
 *  runtime. Warnings save silently. */
export async function saveBehavior(): Promise<void> {
    const path = get(behaviorFilePath);
    const json = get(behaviorJson);
    if (path == null || json == null) return;
    const report = get(behaviorReport);
    if (report && !report.ok && report.errors.length > 0) {
        const n = report.errors.length;
        const confirmed = window.confirm(
            `This behavior has ${n} error${n === 1 ? "" : "s"} and will not load at runtime.\n\nSave anyway?`,
        );
        if (!confirmed) return;
    }
    try {
        await writeBehavior(path, json);
        behaviorDirty.set(false);
        logInfo(`Saved behavior: ${path.split(/[\\/]/).pop()}`);
    } catch (err) {
        logError(`Failed to save behavior: ${err}`);
    }
}

// ─── Slice E.10e: comment groups ─────────────────────────────────


/** Default size for a freshly-added comment, in world units. */
const DEFAULT_COMMENT_SIZE = { w: 240, h: 140 };

function nextFreeCommentId(doc: BehaviorDoc): string {
    const taken = new Set(
        (doc.editor?.comments ?? []).map((c) => c.id),
    );
    for (let i = 0; i < 1_000_000; i++) {
        const candidate = `c${i}`;
        if (!taken.has(candidate)) return candidate;
    }
    return `c${(doc.editor?.comments ?? []).length}`;
}

function ensureEditorComments(doc: BehaviorDoc): BehaviorComment[] {
    const editor = (doc.editor ??= {} as {
        comments?: BehaviorComment[];
    });
    return (editor.comments ??= []);
}

/**
 * Add a fresh comment box at (`worldX`, `worldY`). Single undo
 * entry; selects nothing (comments aren't part of the node
 * selection model). Returns the new id so the caller can focus
 * the text area immediately.
 */
export function addCommentAt(worldX: number, worldY: number): string {
    let newId = "c0";
    withDoc((doc) => {
        const comments = ensureEditorComments(doc);
        newId = nextFreeCommentId(doc);
        comments.push({
            id: newId,
            text: "Note",
            x: Math.round(worldX),
            y: Math.round(worldY),
            w: DEFAULT_COMMENT_SIZE.w,
            h: DEFAULT_COMMENT_SIZE.h,
        });
    });
    return newId;
}

/** Move + optionally resize a single comment. Single undo entry. */
export function commitCommentRect(
    id: string,
    rect: { x: number; y: number; w: number; h: number },
): void {
    withDoc((doc) => {
        const comments = doc.editor?.comments;
        if (!comments) return;
        const target = comments.find((c) => c.id === id);
        if (!target) return;
        target.x = Math.round(rect.x);
        target.y = Math.round(rect.y);
        target.w = Math.max(80, Math.round(rect.w));
        target.h = Math.max(60, Math.round(rect.h));
    });
}

/** Replace a comment's text. Single undo entry. */
export function updateCommentText(id: string, text: string): void {
    withDoc((doc) => {
        const comments = doc.editor?.comments;
        if (!comments) return;
        const target = comments.find((c) => c.id === id);
        if (!target) return;
        target.text = text;
    });
}

/** Remove a comment by id. Single undo entry. No-op if not found. */
export function deleteComment(id: string): void {
    withDoc((doc) => {
        const comments = doc.editor?.comments;
        if (!comments) return;
        const next = comments.filter((c) => c.id !== id);
        if (next.length === comments.length) return;
        if (next.length === 0) {
            delete (doc.editor as { comments?: BehaviorComment[] }).comments;
        } else {
            (doc.editor as { comments?: BehaviorComment[] }).comments = next;
        }
    });
}

// ─── Slice E.5: add node from palette ────────────────────────────

/**
 * Set by the palette when the user double-clicks an entry. The
 * canvas observes this writable via `$effect`, computes its own
 * viewport-centre world coords, calls {@link addNodeAt}, and
 * clears it back to `null`.
 */
export const requestCenterDrop = writable<string | null>(null);

/** Pick the next free `n<i>` id given the document's current set. */
function nextFreeNodeId(doc: BehaviorDoc): string {
    const taken = new Set(doc.nodes.map((n) => n.id));
    for (let i = 0; i < 1_000_000; i++) {
        const candidate = `n${i}`;
        if (!taken.has(candidate)) return candidate;
    }
    // Pathological fallback — the loop bound is effectively infinite.
    return `n${doc.nodes.length}`;
}

/** Build a default `params` map from a descriptor, including only
 *  params that have a non-null default. Pin defaults are *not*
 *  baked into `in` — those stay literal-editor territory (E.7). */
function defaultParamsFor(descriptor: OpDescriptor | undefined):
    | Record<string, unknown>
    | undefined {
    if (!descriptor) return undefined;
    const out: Record<string, unknown> = {};
    let any = false;
    for (const p of descriptor.params) {
        if (p.default == null) continue;
        out[p.name] = p.default;
        any = true;
    }
    return any ? out : undefined;
}

/**
 * Add a fresh node of the given op to the open document at
 * (`worldX`, `worldY`). Selects the new node afterwards. Returns
 * the assigned node id, or `null` if no document is open.
 */
export function addNodeAt(op: string, worldX: number, worldY: number): string | null {
    const json = get(behaviorJson);
    if (json == null) return null;
    const catalog = get(opCatalog) ?? [];
    const descriptor = catalog.find((d) => d.op === op);
    let newId = "n0";
    withDoc((doc) => {
        newId = nextFreeNodeId(doc);
        const node: { id: string; op: string; params?: Record<string, unknown> } = {
            id: newId,
            op,
        };
        const params = defaultParamsFor(descriptor);
        if (params) node.params = params;
        doc.nodes.push(node);
        const editor = (doc.editor ??= {} as {
            positions?: Record<string, BehaviorPosition>;
        });
        const positions = (editor.positions ??= {});
        positions[newId] = { x: Math.round(worldX), y: Math.round(worldY) };
    });
    selectedNodeIds.set(new Set([newId]));
    return newId;
}

// ─── Slice E.6: connect / disconnect ─────────────────────────────

/**
 * Add a control-flow edge `from --out--> to`. No-op if an
 * identical edge already exists. Single undo entry.
 */
export function addControlEdge(from: string, out: string, to: string): void {
    withDoc((doc) => {
        const exists = doc.edges.some(
            (e) => e.from === from && (e.out ?? "exec") === out && e.to === to,
        );
        if (exists) return;
        // Always serialise `out` so disambiguation (e.g. If's
        // `then`/`else`) survives round-trips. The runtime treats
        // a missing `out` as `"exec"`, but the canvas writes it
        // explicitly for clarity.
        doc.edges.push({ from, out, to });
    });
}

/**
 * Remove a control-flow edge identified by `from`/`out`/`to`.
 * `out` is matched leniently against the stored `out ?? "exec"`.
 */
export function removeControlEdge(
    from: string,
    out: string,
    to: string,
): void {
    withDoc((doc) => {
        doc.edges = doc.edges.filter(
            (e) => !(e.from === from && (e.out ?? "exec") === out && e.to === to),
        );
    });
}

/**
 * Add a data binding to `toId.toPin` referencing `fromId.fromOut`.
 * Replaces any existing binding on the same pin (single undo).
 */
export function addDataBinding(
    toId: string,
    toPin: string,
    fromId: string,
    fromOut: string | null,
): void {
    withDoc((doc) => {
        const node = doc.nodes.find((n) => n.id === toId);
        if (!node) return;
        const ins = (node.in ??= {});
        const binding: { from: string; out?: string } = { from: fromId };
        if (fromOut != null) binding.out = fromOut;
        ins[toPin] = binding;
    });
}

/** Remove the data binding on `toId.toPin`, if any. */
export function removeDataBinding(toId: string, toPin: string): void {
    withDoc((doc) => {
        const node = doc.nodes.find((n) => n.id === toId);
        if (!node?.in) return;
        delete node.in[toPin];
        if (Object.keys(node.in).length === 0) delete node.in;
    });
}

// ─── Slice E.7: literal editors + pin modifiers ──────────────────

/**
 * Write a literal value to `toId.toPin`. Replaces any existing
 * binding (literal or reference) on the same pin. Single undo.
 */
export function setPinLiteral(
    toId: string,
    toPin: string,
    value: unknown,
): void {
    withDoc((doc) => {
        const node = doc.nodes.find((n) => n.id === toId);
        if (!node) return;
        const ins = (node.in ??= {});
        ins[toPin] = { value } as { value: unknown };
    });
}

/**
 * Set or clear a `× scale` / `+ offset` modifier on a reference
 * binding. `value === null` clears the modifier (deletes the key).
 * No-op if the pin has no binding or holds a literal.
 */
export function setPinModifier(
    toId: string,
    toPin: string,
    kind: "scale" | "offset",
    value: number | null,
): void {
    withDoc((doc) => {
        const node = doc.nodes.find((n) => n.id === toId);
        const binding = node?.in?.[toPin] as
            | { from?: string; scale?: number; offset?: number }
            | undefined;
        if (!binding || typeof binding.from !== "string") return;
        if (value == null) {
            delete binding[kind];
        } else {
            binding[kind] = value;
        }
    });
}

/**
 * Set or clear a node param. `value === undefined` deletes the
 * key (and prunes an empty `params` map).
 */
export function setNodeParam(
    nodeId: string,
    paramName: string,
    value: unknown,
): void {
    withDoc((doc) => {
        const node = doc.nodes.find((n) => n.id === nodeId);
        if (!node) return;
        if (value === undefined) {
            if (!node.params) return;
            delete node.params[paramName];
            if (Object.keys(node.params).length === 0) delete node.params;
            return;
        }
        const params = (node.params ??= {});
        params[paramName] = value;
    });
}
