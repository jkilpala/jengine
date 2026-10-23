/**
 * Which device the Game view and the GUI editor are previewing, if any
 * (`devicePreview.ts`). Shared, so choosing a phone in one shows it in the
 * other; remembered per browser profile, which is a convenience only.
 */
import { writable } from "svelte/store";

export interface DevicePreviewState {
  /** A `DEVICE_PROFILES` id, or null for none. */
  id: string | null;
  portrait: boolean;
}

const KEY = "jengine.devicePreview";

function load(): DevicePreviewState {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const v = JSON.parse(raw);
      return { id: typeof v.id === "string" ? v.id : null, portrait: !!v.portrait };
    }
  } catch {
    // Storage unavailable or corrupt - start with no device.
  }
  return { id: null, portrait: false };
}

export const devicePreview = writable<DevicePreviewState>(load());

devicePreview.subscribe((v) => {
  try {
    localStorage.setItem(KEY, JSON.stringify(v));
  } catch {
    // Not persisted - fine.
  }
});
