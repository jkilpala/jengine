import { writable } from "svelte/store";
import { getPackageEditorSchema, type PackageEditorSchema } from "../api/packages";

/** Reactive store for the package editor schema — reloads when enabled packages change. */
export const packageSchema = writable<PackageEditorSchema | null>(null);

/** Reload the package editor schema for the given enabled packages list. */
export async function reloadPackageSchema(
  enabledPackages: string[],
  projectPath?: string,
): Promise<void> {
  try {
    const schema = await getPackageEditorSchema(enabledPackages, projectPath);
    packageSchema.set(schema);
  } catch {
    packageSchema.set(null);
  }
}
