import { invoke } from "@tauri-apps/api/core";

/**
 * Behavior-graph editor API. Slice E.1 of the node-graph editor
 * follow-on plan (see `docs/archive/step7-node-graph-editor-plan.md`).
 *
 * The runtime is the source of truth for what ops exist and what
 * pins they expose; this module mirrors the
 * `crates/jengine-core/src/behavior/catalog.rs` types so the editor
 * can statically reference them.
 */

export type EffectKind =
    | "Trigger"
    | "Read"
    | "Pure"
    | "Gate"
    | "DirectWrite"
    | "SetupComponent"
    | "StructuralCommand"
    | "SideEffect";

export type PinType =
    | "Float"
    | "Vec2"
    | "Bool"
    | "EntityRef"
    | "Aabb"
    | "Enum"
    | "String";

/** Mirror of `behavior::catalog::DefaultValue` (untagged). */
export type DefaultValue = number | [number, number] | boolean | string;

export interface PinDescriptor {
    name: string;
    ty: PinType;
    default: DefaultValue | null;
    accepts_scale_offset: boolean;
}

export interface ParamDescriptor {
    name: string;
    ty: PinType;
    default: DefaultValue | null;
    enum_variants: string[];
    required: boolean;
}

export interface OpDescriptor {
    op: string;
    category: EffectKind;
    inputs: PinDescriptor[];
    outputs: PinDescriptor[];
    exec_in: boolean;
    exec_out: string[];
    params: ParamDescriptor[];
    summary: string;
}

export function loadBehaviorOpCatalog(): Promise<OpDescriptor[]> {
    return invoke<OpDescriptor[]>("behavior_op_catalog");
}

// ─── Slice E.2: compile_behavior ──────────────────────────────────

/**
 * One compiler diagnostic. `node_id` / `pin` may be `null` for
 * top-level errors (e.g. JSON parse). `code` is a stable identifier
 * (`"unknown_op"`, `"pin_type_mismatch"`, ...).
 */
export interface BehaviorDiagnostic {
    node_id: string | null;
    pin: string | null;
    code: string;
    message: string;
}

/** Result of validating a `.behavior` document. */
export interface CompileReport {
    ok: boolean;
    errors: BehaviorDiagnostic[];
    warnings: BehaviorDiagnostic[];
}

/**
 * Validates a `.behavior` JSON document by running the runtime
 * compiler. Always resolves; failures are returned as diagnostics in
 * `errors`, never thrown.
 */
export function compileBehavior(json: string): Promise<CompileReport> {
    return invoke<CompileReport>("compile_behavior", { json });
}

/**
 * Persists a `.behavior` JSON document to disk. Slice E.4. The
 * backend validates that the JSON parses as a `BehaviorAsset`
 * before writing — invalid documents reject with an error.
 */
export function writeBehavior(path: string, json: string): Promise<void> {
    return invoke<void>("write_behavior", { path, json });
}

/**
 * Minimal `.behavior` JSON document — a single `OnTick` node so the
 * file loads cleanly through the runtime compiler. Used by both the
 * asset-browser "New Behavior" button and the inspector's
 * behavior_graph "+ New" shortcut so the seed template stays in sync.
 */
export function defaultBehaviorTemplate(name: string): string {
    return JSON.stringify(
        {
            name,
            version: 1,
            nodes: [{ id: "tick", op: "OnTick" }],
            edges: [],
        },
        null,
        2,
    );
}
