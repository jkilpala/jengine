import { writable, get, type Writable, type Unsubscriber } from "svelte/store";
import {
  type EditorSettings,
  defaultEditorSettings,
  loadEditorSettings,
  saveEditorSettings,
} from "../api/editorSettings";
import { project } from "./projectStore";
import { showColliders, showProfiler } from "./viewportStore";

/** Per-project editor settings, loaded when a project opens. */
export const editorSettings = writable<EditorSettings>(defaultEditorSettings());

// --- Auto-sync: watch external stores and persist changes back --

/** Debounce handle for batching rapid changes into a single save. */
let saveTimer: ReturnType<typeof setTimeout> | null = null;

function debouncedSave(): void {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => { saveTimer = null; saveSettings(); }, 300);
}

/**
 * Map of external stores → editor-settings keys.
 * When you add a new persisted setting backed by an external store,
 * just add an entry here — the subscription wiring is automatic.
 */
const trackedStores: Array<{ store: Writable<any>; key: keyof EditorSettings }> = [
  { store: showColliders, key: "show_colliders" },
  { store: showProfiler,  key: "show_profiler" },
];

/** Active subscriptions — cleaned up and re-created on each project load. */
let unsubs: Unsubscriber[] = [];

/** Whether we are currently applying loaded settings (skip re-saving). */
let syncing = false;

function startTracking(): void {
  stopTracking();
  for (const { store, key } of trackedStores) {
    // Skip the first emit (the current value) — we just loaded it.
    let first = true;
    const unsub = store.subscribe((value: any) => {
      if (first) { first = false; return; }
      if (syncing) return;
      editorSettings.update((s) => ({ ...s, [key]: value }));
      debouncedSave();
    });
    unsubs.push(unsub);
  }
}

function stopTracking(): void {
  for (const unsub of unsubs) unsub();
  unsubs = [];
}

// --- Public API --

/** Load editor settings for the currently open project. */
export async function loadSettings(): Promise<void> {
  const proj = get(project);
  if (!proj) return;

  let settings: EditorSettings;
  try {
    settings = await loadEditorSettings(proj.path);
  } catch {
    settings = defaultEditorSettings();
  }

  // Apply loaded values — suppress re-saving during sync
  syncing = true;
  editorSettings.set(settings);
  showColliders.set(settings.show_colliders);
  showProfiler.set(settings.show_profiler);
  syncing = false;

  // (Re-)start tracking external store changes
  startTracking();
}

/** Save the current editor settings to disk. */
export async function saveSettings(): Promise<void> {
  const proj = get(project);
  if (!proj) return;
  try {
    await saveEditorSettings(proj.path, get(editorSettings));
  } catch (e) {
    console.error("Failed to save editor settings:", e);
  }
}

/** Update a single field and auto-save. */
export function updateSetting<K extends keyof EditorSettings>(
  key: K,
  value: EditorSettings[K],
): void {
  editorSettings.update((s) => ({ ...s, [key]: value }));
  debouncedSave();
}
