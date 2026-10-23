//! Runtime registry of loaded data schemas and tables.
//!
//! Each runtime (native, web, editor viewport) loads the project's
//! `.dataschema` and `.data` files into a [`DataRegistry`], then scripts read
//! values via `get_data(table, id, field)`. The registry is the single lookup
//! surface behind that script global.

use std::collections::HashMap;

use super::schema::DataSchema;
use super::table::{DataRef, DataRow, DataTable};

/// In-memory store of loaded schemas (keyed by schema name) and tables (keyed
/// by a normalized table id).
#[derive(Debug, Clone, Default)]
pub struct DataRegistry {
    schemas: HashMap<String, DataSchema>,
    tables: HashMap<String, DataTable>,
}

impl DataRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Normalize a table id: forward slashes, no leading `./`, and a trailing
    /// `.data` extension stripped. Lets scripts pass either `"data/enemies"` or
    /// `"data/enemies.data"` and get the same table.
    pub fn normalize_table_id(id: &str) -> String {
        let s = id.replace('\\', "/");
        let s = s.strip_prefix("./").unwrap_or(&s);
        s.strip_suffix(".data").unwrap_or(s).to_string()
    }

    /// Register (or replace) a schema by its [`DataSchema::name`].
    pub fn insert_schema(&mut self, schema: DataSchema) {
        self.schemas.insert(schema.name.clone(), schema);
    }

    /// Register (or replace) a table under the given id. The id is normalized.
    pub fn insert_table(&mut self, id: &str, table: DataTable) {
        self.tables
            .insert(Self::normalize_table_id(id), table);
    }

    /// Parse and register a `.dataschema` JSON document.
    pub fn load_schema_json(&mut self, json: &str) -> Result<(), super::schema::DataSchemaError> {
        let schema = DataSchema::from_json(json)?;
        self.insert_schema(schema);
        Ok(())
    }

    /// Parse and register a `.data` JSON document under the given table id.
    pub fn load_table_json(
        &mut self,
        id: &str,
        json: &str,
    ) -> Result<(), super::table::DataTableError> {
        let table = DataTable::from_json(json)?;
        self.insert_table(id, table);
        Ok(())
    }

    /// Every table id (normalized), in no particular order.
    pub fn table_ids(&self) -> impl Iterator<Item = &str> {
        self.tables.keys().map(String::as_str)
    }

    /// Number of loaded tables (useful for load-summary logging).
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    /// Look up a schema by name.
    pub fn schema(&self, name: &str) -> Option<&DataSchema> {
        self.schemas.get(name)
    }

    /// Look up a table by id (id is normalized before lookup).
    pub fn table(&self, id: &str) -> Option<&DataTable> {
        self.tables.get(&Self::normalize_table_id(id))
    }

    /// Look up a row by table id + row id.
    pub fn row(&self, table_id: &str, row_id: &str) -> Option<&DataRow> {
        self.table(table_id)?.row(row_id)
    }

    /// Read a single field value: `get_data(table, id, field)`.
    pub fn get(&self, table_id: &str, row_id: &str, field: &str) -> Option<&serde_json::Value> {
        self.row(table_id, row_id)?.get(field)
    }

    /// Read a field value as `f64` when it is a JSON number (or bool → 0/1).
    /// Mirrors the host-JS `get_custom` marshalling used by the script bridge.
    pub fn get_f64(&self, table_id: &str, row_id: &str, field: &str) -> Option<f64> {
        match self.get(table_id, row_id, field)? {
            serde_json::Value::Number(n) => n.as_f64(),
            serde_json::Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    /// Read a field value as a string when it is a JSON string.
    pub fn get_string(&self, table_id: &str, row_id: &str, field: &str) -> Option<&str> {
        self.get(table_id, row_id, field)?.as_str()
    }

    /// Resolve a reference field to the row it points at, following the
    /// `{ table, id }` shape. Returns `None` for a null/unset reference or a
    /// dangling target.
    pub fn resolve_ref(&self, table_id: &str, row_id: &str, field: &str) -> Option<&DataRow> {
        let value = self.get(table_id, row_id, field)?;
        let reference = DataRef::from_value(value)?;
        self.row(&reference.table, &reference.id)
    }
}

impl DataRegistry {
    /// Row ids of a table, in file order: what a package walks to read a
    /// whole table (every card, every enemy).
    pub fn row_ids(&self, table_id: &str) -> Vec<&str> {
        self.table(table_id).map(|t| t.rows.iter().map(|r| r.id.as_str()).collect()).unwrap_or_default()
    }

    /// Build from `{ "<table id>": { "rows": [ ... ] }, ... }` - the shape
    /// the editor pushes to the viewport (`viewport_seed_data_tables`) and
    /// the web runtime seeds its scripts with. Tables are not checked
    /// against schemas here; the editor validated them when they were saved.
    pub fn from_tables_json(json: &str) -> Result<Self, serde_json::Error> {
        let raw: HashMap<String, serde_json::Value> = serde_json::from_str(json)?;
        let mut registry = Self::new();
        for (id, table) in raw {
            let rows: Vec<DataRow> = serde_json::from_value(table.get("rows").cloned().unwrap_or_default())?;
            let schema = table.get("schema").and_then(|s| s.as_str()).unwrap_or_default().to_string();
            registry.insert_table(&id, DataTable { schema, version: super::table::DATA_TABLE_VERSION, rows });
        }
        Ok(registry)
    }
}

// ---------------------------------------------------------------------------
// The process-wide registry
// ---------------------------------------------------------------------------

/// The data tables loaded in this process, for Rust code that has no script
/// system in hand - a package's systems and commands. Scripts keep their
/// own handle (`get_data`); this is the same data, published once it is
/// loaded: by the native runner and the web runtime at startup, and by the
/// editor viewport when the editor seeds its tables.
static GLOBAL: std::sync::RwLock<Option<std::sync::Arc<DataRegistry>>> = std::sync::RwLock::new(None);

/// Bumped by every [`install_global`], so what reads the tables - the
/// localized text (`locale::stamp`) - knows when they changed.
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Publish the registry; a second call replaces the first.
pub fn install_global(registry: std::sync::Arc<DataRegistry>) {
    match GLOBAL.write() {
        Ok(mut slot) => *slot = Some(registry),
        Err(e) => log::error!("[data] could not publish the data tables: {e}"),
    }
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// How many times tables have been published.
pub fn generation() -> u64 {
    GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

/// The published registry, if a runtime has published one.
pub fn global() -> Option<std::sync::Arc<DataRegistry>> {
    GLOBAL.read().ok().and_then(|slot| slot.clone())
}

#[cfg(not(target_arch = "wasm32"))]
impl DataRegistry {
    /// Load every `.dataschema` and `.data` file under `assets_dir` into a new
    /// registry. Table ids are the file path relative to `assets_dir` with
    /// forward slashes and the `.data` extension stripped (e.g.
    /// `data/enemies.data` → id `data/enemies`), matching the ids scripts pass
    /// to `get_data` and the reference `table` values authored in the editor.
    ///
    /// Malformed files are skipped with a warning; a bad file never aborts the
    /// whole load.
    pub fn load_from_assets_dir(assets_dir: &std::path::Path) -> Self {
        let mut registry = Self::new();
        registry.load_dir_recursive(assets_dir, assets_dir);
        registry
    }

    fn load_dir_recursive(&mut self, dir: &std::path::Path, root: &std::path::Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.load_dir_recursive(&path, root);
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str());
            match ext {
                Some("dataschema") => match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        if let Err(e) = self.load_schema_json(&text) {
                            log::warn!("skipping data schema {}: {}", path.display(), e);
                        }
                    }
                    Err(e) => log::warn!("failed to read {}: {}", path.display(), e),
                },
                Some("data") => {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    match std::fs::read_to_string(&path) {
                        Ok(text) => {
                            if let Err(e) = self.load_table_json(&rel, &text) {
                                log::warn!("skipping data table {}: {}", path.display(), e);
                            }
                        }
                        Err(e) => log::warn!("failed to read {}: {}", path.display(), e),
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::table::DataRow;

    fn registry() -> DataRegistry {
        let mut reg = DataRegistry::new();
        let mut enemies = DataTable::new("EnemyType");
        let mut goblin = serde_json::Map::new();
        goblin.insert("health".into(), serde_json::json!(30));
        goblin.insert("flying".into(), serde_json::json!(false));
        goblin.insert(
            "drop".into(),
            serde_json::json!({ "table": "data/items", "id": "coin" }),
        );
        enemies.rows.push(DataRow {
            id: "goblin".into(),
            fields: goblin,
        });
        reg.insert_table("data/enemies.data", enemies);

        let mut items = DataTable::new("ItemType");
        let mut coin = serde_json::Map::new();
        coin.insert("value".into(), serde_json::json!(5));
        items.rows.push(DataRow {
            id: "coin".into(),
            fields: coin,
        });
        reg.insert_table("data/items", items);
        reg
    }

    #[test]
    fn normalizes_table_id() {
        assert_eq!(
            DataRegistry::normalize_table_id("data\\enemies.data"),
            "data/enemies"
        );
        assert_eq!(
            DataRegistry::normalize_table_id("./data/enemies"),
            "data/enemies"
        );
    }

    #[test]
    fn get_scalar_with_either_id_form() {
        let reg = registry();
        assert_eq!(reg.get_f64("data/enemies", "goblin", "health"), Some(30.0));
        assert_eq!(
            reg.get_f64("data/enemies.data", "goblin", "health"),
            Some(30.0)
        );
        assert_eq!(reg.get_f64("data/enemies", "goblin", "flying"), Some(0.0));
    }

    #[test]
    fn resolves_reference_field() {
        let reg = registry();
        let target = reg.resolve_ref("data/enemies", "goblin", "drop").unwrap();
        assert_eq!(target.id, "coin");
        assert_eq!(target.get("value"), Some(&serde_json::json!(5)));
    }

    #[test]
    fn missing_lookups_return_none() {
        let reg = registry();
        assert!(reg.get("data/enemies", "nope", "health").is_none());
        assert!(reg.get("data/enemies", "goblin", "nope").is_none());
        assert!(reg.table("data/unknown").is_none());
    }
}
