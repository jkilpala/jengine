import { writable } from "svelte/store";
import type { EntityData } from "../api/scene";

/** Clipboard holding copied entity data for paste operations. */
export const entityClipboard = writable<EntityData[] | null>(null);
