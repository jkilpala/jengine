import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// Types — mirror crates/jengine-core/src/data/{schema,table}.rs
// ---------------------------------------------------------------------------

export const DATA_SCHEMA_VERSION = 1;
export const DATA_TABLE_VERSION = 1;

/** Which kind of project asset an `asset` field points at. */
export type AssetKind =
  | "any"
  | "texture"
  | "atlas"
  | "prefab"
  | "scene"
  | "audio";

/** A schema field's type. Internally tagged by `kind` to match serde. */
export type DataFieldType =
  | { kind: "string" }
  | { kind: "int" }
  | { kind: "float" }
  | { kind: "bool" }
  | { kind: "color" }
  | { kind: "vec2" }
  | { kind: "enum"; options: string[] }
  | { kind: "ref"; schema: string }
  | { kind: "asset"; asset_kind: AssetKind }
  | { kind: "array"; element: DataFieldType }
  | { kind: "object"; fields: DataFieldDef[] };

/** All field-type discriminators, for the type dropdown. */
export const FIELD_TYPE_KINDS = [
  "string",
  "int",
  "float",
  "bool",
  "color",
  "vec2",
  "enum",
  "ref",
  "asset",
  "array",
  "object",
] as const;

export type FieldTypeKind = (typeof FIELD_TYPE_KINDS)[number];

/** One field in a schema. Note the JSON key is `type` (serde rename). */
export interface DataFieldDef {
  name: string;
  type: DataFieldType;
  default: unknown;
}

/** A `.dataschema` file. */
export interface DataSchema {
  name: string;
  version: number;
  fields: DataFieldDef[];
}

/** A row in a table: `id` plus the flattened schema fields. */
export interface DataRow {
  id: string;
  [field: string]: unknown;
}

/** A `.data` table file. */
export interface DataTable {
  schema: string;
  version: number;
  rows: DataRow[];
}

/** A reference field value pointing at a single row. */
export interface DataRef {
  table: string;
  id: string;
}

/** A schema discovered in the project (for pickers). */
export interface SchemaEntry {
  name: string;
  relative_path: string;
}

/** A table + row ids that uses a given schema (for the reference picker). */
export interface TableRowsEntry {
  relative_path: string;
  schema: string;
  row_ids: string[];
}

// ---------------------------------------------------------------------------
// Factories
// ---------------------------------------------------------------------------

export function defaultDataSchema(name: string): DataSchema {
  return { name, version: DATA_SCHEMA_VERSION, fields: [] };
}

export function defaultDataTable(schemaName: string): DataTable {
  return { schema: schemaName, version: DATA_TABLE_VERSION, rows: [] };
}

/** A fresh field-type value for the given discriminator. */
export function defaultFieldType(kind: FieldTypeKind): DataFieldType {
  switch (kind) {
    case "enum":
      return { kind: "enum", options: [] };
    case "ref":
      return { kind: "ref", schema: "" };
    case "asset":
      return { kind: "asset", asset_kind: "any" };
    case "array":
      return { kind: "array", element: { kind: "string" } };
    case "object":
      return { kind: "object", fields: [] };
    default:
      return { kind } as DataFieldType;
  }
}

/** The default row value for a field type (used when adding a new row). */
export function defaultValueForType(ty: DataFieldType): unknown {
  switch (ty.kind) {
    case "string":
      return "";
    case "int":
    case "float":
      return 0;
    case "bool":
      return false;
    case "color":
      return [1, 1, 1, 1];
    case "vec2":
      return [0, 0];
    case "enum":
      return ty.options[0] ?? "";
    case "ref":
      return null;
    case "asset":
      return "";
    case "array":
      return [];
    case "object": {
      const obj: Record<string, unknown> = {};
      for (const f of ty.fields) obj[f.name] = defaultValueForType(f.type);
      return obj;
    }
  }
}

/** Human label for a field-type discriminator. */
export function fieldTypeLabel(kind: FieldTypeKind): string {
  switch (kind) {
    case "vec2":
      return "Vec2";
    case "ref":
      return "Reference";
    default:
      return kind.charAt(0).toUpperCase() + kind.slice(1);
  }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

export function loadDataSchema(path: string): Promise<DataSchema> {
  return invoke<DataSchema>("load_data_schema", { path });
}

export function saveDataSchema(path: string, schema: DataSchema): Promise<void> {
  return invoke("save_data_schema", { path, schema });
}

export function loadDataTable(path: string): Promise<DataTable> {
  return invoke<DataTable>("load_data_table", { path });
}

export function saveDataTable(path: string, table: DataTable): Promise<void> {
  return invoke("save_data_table", { path, table });
}

export function listDataSchemas(projectPath: string): Promise<SchemaEntry[]> {
  return invoke<SchemaEntry[]>("list_data_schemas", { projectPath });
}

export function listDataTablesForSchema(
  projectPath: string,
  schema: string,
): Promise<TableRowsEntry[]> {
  return invoke<TableRowsEntry[]>("list_data_tables_for_schema", {
    projectPath,
    schema,
  });
}
