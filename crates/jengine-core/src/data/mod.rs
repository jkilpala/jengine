//! Data-oriented design assets: user-authored schemas (`.dataschema`) and
//! tables of rows (`.data`) that conform to them.
//!
//! - [`schema`] — the `.dataschema` type definition format.
//! - [`table`] — the `.data` table-of-rows format and reference values.
//! - [`registry`] — the runtime lookup surface behind the `get_data` script API.
//!
//! Rows can reference a single row in another table (constrained to a target
//! schema), enabling data-driven design where e.g. an enemy's `drops` field
//! points at rows in an items table.

pub mod registry;
pub mod schema;
pub mod table;

pub use registry::{DataRegistry, generation, global, install_global};
pub use schema::{
    AssetKind, DataFieldDef, DataFieldType, DataSchema, DataSchemaError, DATA_SCHEMA_VERSION,
};
pub use table::{DataRef, DataRow, DataTable, DataTableError, DATA_TABLE_VERSION};
