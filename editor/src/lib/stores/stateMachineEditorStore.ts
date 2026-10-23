import { writable, derived, get } from "svelte/store";
import type { StateMachineData, StateNodeLayout } from "../api/stateMachine";

/** The state machine data currently being edited (null when closed). */
export const stateMachineData = writable<StateMachineData | null>(null);

/** Entity ID being edited (so we can write changes back). */
export const stateMachineEntityId = writable<number | null>(null);

/** Layout positions for each state node (keyed by state name). Editor-only. */
export const stateMachineLayout = writable<Record<string, StateNodeLayout>>({});

/** Dirty flag — true when unsaved changes exist. */
export const stateMachineDirty = writable(false);

/** The name of the currently selected state (null = nothing selected). */
export const smSelectedState = writable<string | null>(null);

/** Selected transition: [fromState, transitionIndex] or null. */
export const smSelectedTransition = writable<[string, number] | null>(null);

// ---- Undo / redo ----
let undoStack: string[] = [];
let redoStack: string[] = [];

export const canUndoSM = writable(false);
export const canRedoSM = writable(false);

function snapshot(): string {
  return JSON.stringify(get(stateMachineData));
}

/** Execute a mutation on the state machine data with undo support. */
export function executeSM(label: string, fn: (data: StateMachineData) => void) {
  const data = get(stateMachineData);
  if (!data) return;
  undoStack.push(snapshot());
  redoStack = [];
  fn(data);
  stateMachineData.set(structuredClone(data));
  stateMachineDirty.set(true);
  canUndoSM.set(true);
  canRedoSM.set(false);
}

export function undoSM() {
  if (undoStack.length === 0) return;
  redoStack.push(snapshot());
  const prev = undoStack.pop()!;
  stateMachineData.set(JSON.parse(prev));
  stateMachineDirty.set(true);
  canUndoSM.set(undoStack.length > 0);
  canRedoSM.set(true);
}

export function redoSM() {
  if (redoStack.length === 0) return;
  undoStack.push(snapshot());
  const next = redoStack.pop()!;
  stateMachineData.set(JSON.parse(next));
  stateMachineDirty.set(true);
  canUndoSM.set(true);
  canRedoSM.set(redoStack.length > 0);
}

/** Open the state machine editor with data from an entity. */
export function openStateMachineEditor(
  entityId: number,
  data: StateMachineData,
) {
  undoStack = [];
  redoStack = [];
  canUndoSM.set(false);
  canRedoSM.set(false);
  stateMachineEntityId.set(entityId);
  stateMachineData.set(structuredClone(data));
  stateMachineDirty.set(false);
  smSelectedState.set(null);
  smSelectedTransition.set(null);
  // Auto-layout states in a circle
  autoLayout(data);
}

/** Close the state machine editor. */
export function closeStateMachineEditor() {
  stateMachineData.set(null);
  stateMachineEntityId.set(null);
  stateMachineLayout.set({});
  smSelectedState.set(null);
  smSelectedTransition.set(null);
  undoStack = [];
  redoStack = [];
}

/** Generate automatic layout for states arranged in a circle. */
function autoLayout(data: StateMachineData) {
  const names = Object.keys(data.states);
  const layout: Record<string, StateNodeLayout> = {};
  const count = names.length;
  const cx = 400;
  const cy = 300;
  const radius = Math.max(150, count * 40);
  names.forEach((name, i) => {
    const angle = (i / count) * Math.PI * 2 - Math.PI / 2;
    layout[name] = {
      x: cx + Math.cos(angle) * radius,
      y: cy + Math.sin(angle) * radius,
    };
  });
  stateMachineLayout.set(layout);
}
