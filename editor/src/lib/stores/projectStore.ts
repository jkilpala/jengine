import { writable } from "svelte/store";
import type { ProjectInfo } from "../api/project";

/** The currently open project, or null if no project is open. */
export const project = writable<ProjectInfo | null>(null);

/** Bump this to request the AssetBrowser to refresh its tree. */
export const assetRefreshRequest = writable(0);
export function requestAssetRefresh() {
  assetRefreshRequest.update((n) => n + 1);
}
