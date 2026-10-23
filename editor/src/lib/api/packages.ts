import { invoke } from "@tauri-apps/api/core";

// ─── Types ───────────────────────────────────────────────────────────────

export interface PackageInfo {
  name: string;
  source_type: "path" | "git";
  source_location: string;
  tag: string | null;
  optional: boolean;
  local_path: string | null;
  version: string | null;
  description: string | null;
  author: string | null;
}

/** Field type for schema-driven component rendering. */
export type FieldType =
  | { Float: { min: number | null; max: number | null } }
  | { Int: { min: number | null; max: number | null } }
  | "String"
  | "Bool"
  | "Vec2"
  | "Color"
  | "Color3"
  | "Points"
  | "Curve"
  | "Gradient"
  | "Json"
  | { Enum: { options: string[] } }
  | { AssetRef: { extensions: string[] } };

export interface ComponentFieldDescriptor {
  name: string;
  display_name: string;
  field_type: FieldType;
  default_value: unknown | null;
  description: string;
}

export interface ComponentEditorInfo {
  component_name: string;
  category: string;
  fields: ComponentFieldDescriptor[];
  /** The whole component as added, for a schema derived from the component
   * itself - fields the inspector cannot edit included. */
  default?: unknown;
}

export interface MenuItemDescriptor {
  path: string;
  command: string;
  shortcut: string | null;
}

export interface ToolbarAction {
  label: string;
  icon: string;
  command: string;
}

export interface PanelDescriptor {
  name: string;
  icon: string;
  default_zone: string;
  web_bundle_path: string;
  package_path: string;
}

export interface PackageEditorSchema {
  components: ComponentEditorInfo[];
  menu_items: MenuItemDescriptor[];
  toolbar_actions: ToolbarAction[];
  panels: PanelDescriptor[];
}

// ─── API Calls ───────────────────────────────────────────────────────────

export function listPackages(projectPath: string): Promise<PackageInfo[]> {
  return invoke<PackageInfo[]>("list_packages", { projectPath });
}

/** What New Rust Package would make of a name (`preview_rust_package`). */
export interface RustPackagePreview {
  /** Why the name cannot be used, or null when it can. */
  error: string | null;
  /** The namespace scripts call its commands by: `my-rules` is `MyRules`. */
  namespace: string;
}

export function previewRustPackage(projectPath: string, name: string): Promise<RustPackagePreview> {
  return invoke<RustPackagePreview>("preview_rust_package", { projectPath, name });
}

/** Scaffold a Rust package in the project and list it; returns its `src/lib.rs`, project-relative. */
export function createRustPackage(projectPath: string, name: string): Promise<string> {
  return invoke<string>("create_rust_package", { projectPath, name });
}

export function addPackage(
  projectPath: string,
  name: string,
  sourceType: "path" | "git",
  sourceLocation: string,
  tag: string | null = null,
  optional: boolean = false,
): Promise<void> {
  return invoke("add_package", {
    projectPath,
    name,
    sourceType,
    sourceLocation,
    tag,
    optional,
  });
}

export function removePackage(
  projectPath: string,
  name: string,
): Promise<boolean> {
  return invoke<boolean>("remove_package", { projectPath, name });
}

export function resolvePackages(
  projectPath: string,
): Promise<PackageInfo[]> {
  return invoke<PackageInfo[]>("resolve_packages", { projectPath });
}

export function getPackageEditorSchema(
  enabledPackages: string[],
  projectPath?: string,
): Promise<PackageEditorSchema> {
  return invoke<PackageEditorSchema>("get_package_editor_schema", {
    enabledPackages,
    projectPath: projectPath ?? null,
  });
}

export interface AvailablePackage {
  name: string;
  version: string;
  description: string;
  author: string;
}

export function getAvailablePackages(): Promise<AvailablePackage[]> {
  return invoke<AvailablePackage[]>("get_available_packages");
}

export interface DetectedPackageInfo {
  name: string;
  version: string | null;
  description: string | null;
}

export function detectPackageInfo(sourcePath: string): Promise<DetectedPackageInfo> {
  return invoke<DetectedPackageInfo>("detect_package_info", { sourcePath });
}

export function readPackagePanelHtml(
  packagePath: string,
  webBundlePath: string,
): Promise<string> {
  return invoke<string>("read_package_panel_html", { packagePath, webBundlePath });
}
