//! `.data` asset format — a *table* of rows conforming to one schema.
//!
//! A table names its schema and holds a list of rows. Each row has a unique
//! string `id` (its key, used as the reference target and the runtime lookup
//! key) plus one JSON value per schema field.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::schema::DataSchema;

/// Major table version understood by this build. The loader rejects any
/// `.data` file whose `version` field differs.
pub const DATA_TABLE_VERSION: u32 = 1;

/// Top-level `.data` asset: many rows of a single schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataTable {
    /// Name of the schema these rows conform to (matches [`DataSchema::name`]).
    pub schema: String,

    /// Table version. Must equal [`DATA_TABLE_VERSION`] at load.
    pub version: u32,

    /// Rows. Each `id` is unique within the table.
    #[serde(default)]
    pub rows: Vec<DataRow>,
}

/// One row in a table. The `id` is a first-class key; all remaining schema
/// fields are flattened alongside it in the JSON object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataRow {
    /// Stable unique key within the table. Referenced by other rows and used
    /// as the runtime `get_data(table, id, field)` lookup key.
    pub id: String,

    /// The row's field values, keyed by schema field name.
    #[serde(flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// A reference field value: points at a single row in a table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataRef {
    /// Relative path (or table id) of the target `.data` table.
    pub table: String,
    /// `id` of the target row within that table.
    pub id: String,
}

impl DataRef {
    /// Parse a reference from a JSON value, if it has the `{ table, id }` shape.
    pub fn from_value(value: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

/// Errors from parsing or validating a `.data` file.
#[derive(Debug, Error)]
pub enum DataTableError {
    /// JSON (de)serialization failure.
    #[error("data table JSON error: {0}")]
    Parse(#[from] serde_json::Error),

    /// The file's `version` did not match [`DATA_TABLE_VERSION`].
    #[error("unsupported data table version {found} (this build supports {supported})")]
    UnsupportedVersion {
        /// Version found in the file.
        found: u32,
        /// Version this build understands.
        supported: u32,
    },

    /// Two rows share an `id`.
    #[error("duplicate row id '{0}' in table")]
    DuplicateRowId(String),
}

impl DataTable {
    /// Create an empty table bound to the given schema name.
    pub fn new(schema: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
            version: DATA_TABLE_VERSION,
            rows: Vec::new(),
        }
    }

    /// Parse a `.data` JSON document and validate version + unique row ids.
    pub fn from_json(json: &str) -> Result<Self, DataTableError> {
        let table: DataTable = serde_json::from_str(json)?;
        if table.version != DATA_TABLE_VERSION {
            return Err(DataTableError::UnsupportedVersion {
                found: table.version,
                supported: DATA_TABLE_VERSION,
            });
        }
        table.validate()?;
        Ok(table)
    }

    /// Serialize to pretty JSON.
    pub fn to_json_pretty(&self) -> Result<String, DataTableError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Validate structural invariants (unique row ids).
    pub fn validate(&self) -> Result<(), DataTableError> {
        let mut seen = std::collections::HashSet::new();
        for row in &self.rows {
            if !seen.insert(row.id.as_str()) {
                return Err(DataTableError::DuplicateRowId(row.id.clone()));
            }
        }
        Ok(())
    }

    /// Look up a row by `id`.
    pub fn row(&self, id: &str) -> Option<&DataRow> {
        self.rows.iter().find(|r| r.id == id)
    }

    /// Check that every row supplies the fields declared by `schema`. Rows may
    /// carry extra keys (forward compatibility); missing keys are reported.
    ///
    /// Returns a list of human-readable problems; empty means fully conformant.
    pub fn conformance_issues(&self, schema: &DataSchema) -> Vec<String> {
        let mut issues = Vec::new();
        if self.schema != schema.name {
            issues.push(format!(
                "table declares schema '{}' but was checked against '{}'",
                self.schema, schema.name
            ));
        }
        for row in &self.rows {
            for field in &schema.fields {
                if !row.fields.contains_key(&field.name) {
                    issues.push(format!(
                        "row '{}' is missing field '{}'",
                        row.id, field.name
                    ));
                }
            }
        }
        issues
    }
}

impl DataRow {
    /// Get a field value by name.
    pub fn get(&self, field: &str) -> Option<&serde_json::Value> {
        self.fields.get(field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_table() -> DataTable {
        let mut table = DataTable::new("EnemyType");
        let mut goblin = serde_json::Map::new();
        goblin.insert("health".into(), serde_json::json!(30));
        goblin.insert("speed".into(), serde_json::json!(2.0));
        table.rows.push(DataRow {
            id: "goblin".into(),
            fields: goblin,
        });
        table
    }

    #[test]
    fn round_trip_table() {
        let table = sample_table();
        let json = table.to_json_pretty().unwrap();
        let parsed = DataTable::from_json(&json).unwrap();
        assert_eq!(table, parsed);
    }

    #[test]
    fn row_json_flattens_id_and_fields() {
        let table = sample_table();
        let json = serde_json::to_value(&table).unwrap();
        let row0 = &json["rows"][0];
        assert_eq!(row0["id"], serde_json::json!("goblin"));
        assert_eq!(row0["health"], serde_json::json!(30));
    }

    #[test]
    fn rejects_duplicate_row_id() {
        let mut table = DataTable::new("X");
        table.rows.push(DataRow {
            id: "dup".into(),
            fields: serde_json::Map::new(),
        });
        table.rows.push(DataRow {
            id: "dup".into(),
            fields: serde_json::Map::new(),
        });
        assert!(matches!(
            table.validate(),
            Err(DataTableError::DuplicateRowId(_))
        ));
    }

    #[test]
    fn parses_data_ref() {
        let value = serde_json::json!({ "table": "data/items.data", "id": "sword" });
        let r = DataRef::from_value(&value).unwrap();
        assert_eq!(r.table, "data/items.data");
        assert_eq!(r.id, "sword");
    }
}
