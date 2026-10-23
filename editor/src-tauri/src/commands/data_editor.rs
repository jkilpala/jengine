//! Data-oriented editor commands: load/save `.dataschema` and `.data` files,
//! plus discovery helpers that back the reference-field picker.
//!
//! Load/save take an absolute `path` (the asset browser resolves it), mirroring
//! [`super::scene`]. The `list_*` helpers scan the project's `assets/` tree.

use std::path::{Path, PathBuf};

use jengine_core::data::{DataSchema, DataTable};
use serde::Serialize;

/// Load and validate a `.dataschema` file.
#[tauri::command]
pub fn load_data_schema(path: String) -> Result<DataSchema, String> {
    let contents =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read schema: {e}"))?;
    DataSchema::from_json(&contents).map_err(|e| e.to_string())
}

/// Validate and save a `.dataschema` file (pretty JSON).
#[tauri::command]
pub fn save_data_schema(path: String, schema: DataSchema) -> Result<(), String> {
    schema.validate().map_err(|e| e.to_string())?;
    let json = schema.to_json_pretty().map_err(|e| e.to_string())?;
    if let Some(parent) = Path::new(&path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {e}"))?;
    }
    std::fs::write(&path, json).map_err(|e| format!("Failed to write schema: {e}"))
}

/// Load and validate a `.data` table file.
#[tauri::command]
pub fn load_data_table(path: String) -> Result<DataTable, String> {
    let contents =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read table: {e}"))?;
    DataTable::from_json(&contents).map_err(|e| e.to_string())
}

/// Validate and save a `.data` table file (pretty JSON).
#[tauri::command]
pub fn save_data_table(path: String, table: DataTable) -> Result<(), String> {
    table.validate().map_err(|e| e.to_string())?;
    let json = table.to_json_pretty().map_err(|e| e.to_string())?;
    if let Some(parent) = Path::new(&path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {e}"))?;
    }
    std::fs::write(&path, json).map_err(|e| format!("Failed to write table: {e}"))
}

/// A schema discovered in the project, for the schema/reference pickers.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaEntry {
    /// Schema name (`DataSchema::name`).
    pub name: String,
    /// Path relative to `assets/`, forward slashes (e.g. `data/enemy.dataschema`).
    pub relative_path: String,
}

/// List every `.dataschema` in the project's `assets/` tree.
#[tauri::command]
pub fn list_data_schemas(project_path: String) -> Result<Vec<SchemaEntry>, String> {
    let assets = PathBuf::from(&project_path).join("assets");
    let mut out = Vec::new();
    let mut files = Vec::new();
    collect_files(&assets, "dataschema", &assets, &mut files);
    for (abs, rel) in files {
        if let Ok(text) = std::fs::read_to_string(&abs) {
            if let Ok(schema) = DataSchema::from_json(&text) {
                out.push(SchemaEntry {
                    name: schema.name,
                    relative_path: rel,
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// A table (and its row ids) that uses a given schema, for the reference picker.
#[derive(Debug, Clone, Serialize)]
pub struct TableRowsEntry {
    /// Path relative to `assets/`, forward slashes (used as the ref `table`).
    pub relative_path: String,
    /// Schema name this table declares.
    pub schema: String,
    /// Row ids in the table.
    pub row_ids: Vec<String>,
}

/// List every `.data` table whose `schema` matches `schema`, with its row ids.
/// Backs the reference-field picker (only matching-schema rows are selectable).
#[tauri::command]
pub fn list_data_tables_for_schema(
    project_path: String,
    schema: String,
) -> Result<Vec<TableRowsEntry>, String> {
    let assets = PathBuf::from(&project_path).join("assets");
    let mut out = Vec::new();
    let mut files = Vec::new();
    collect_files(&assets, "data", &assets, &mut files);
    for (abs, rel) in files {
        if let Ok(text) = std::fs::read_to_string(&abs) {
            if let Ok(table) = DataTable::from_json(&text) {
                if table.schema == schema {
                    out.push(TableRowsEntry {
                        relative_path: rel,
                        schema: table.schema,
                        row_ids: table.rows.iter().map(|r| r.id.clone()).collect(),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(out)
}

/// Recursively collect files with the given extension under `dir`, returning
/// `(absolute path, path relative to `root` with forward slashes)`.
fn collect_files(dir: &Path, ext: &str, root: &Path, out: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, ext, root, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            if let Ok(rel) = path.strip_prefix(root) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.push((path.clone(), rel));
            }
        }
    }
}
