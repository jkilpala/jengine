import { writable } from "svelte/store";
import type { ShaderGraph } from "../api/shaderEditor";

/** The shader graph currently being edited (null when shader editor is closed). */
export const shaderEditorGraph = writable<ShaderGraph | null>(null);

/** The ID of the currently selected node in the shader editor (null when none). */
export const shaderSelectedNodeId = writable<number | null>(null);

/** Callback to request a canvas redraw after Inspector edits a node property. */
export const shaderRequestDraw = writable<(() => void) | null>(null);

/** Callback to mark the shader editor as dirty after Inspector edits. */
export const shaderMarkDirty = writable<(() => void) | null>(null);
