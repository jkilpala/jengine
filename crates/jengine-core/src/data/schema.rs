//! `.dataschema` asset format — a user-authored data *type* definition.
//!
//! A schema is a named list of typed fields. Data tables (`.data`) hold rows
//! that conform to a schema. Reference fields let a row point at a single row
//! in another table, constrained to a target schema.
//!
//! See [`docs/scripting-api.md`] for the runtime `get_data` accessor and the
//! data-oriented editor plan for the authoring workflow.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Major schema version understood by this build. The loader rejects any
/// `.dataschema` file whose `version` field differs.
pub const DATA_SCHEMA_VERSION: u32 = 1;

/// Top-level `.dataschema` asset: one named data type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataSchema {
    /// Type name, e.g. `"EnemyType"`. Used as the reference target and as the
    /// `schema` field of tables that use it.
    pub name: String,

    /// Schema version. Must equal [`DATA_SCHEMA_VERSION`] at load.
    pub version: u32,

    /// Ordered list of fields. Order is preserved for column display.
    #[serde(default)]
    pub fields: Vec<DataFieldDef>,
}

/// A single field in a schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataFieldDef {
    /// Field name. Unique within a schema. Becomes a JSON key on each row.
    pub name: String,

    /// Field type and its type-specific configuration.
    #[serde(rename = "type")]
    pub ty: DataFieldType,

    /// Default value applied to new rows. `Null` if omitted.
    #[serde(default)]
    pub default: serde_json::Value,
}

/// The type of a schema field. Internally tagged by `kind` so the JSON reads
/// e.g. `{ "kind": "enum", "options": ["a", "b"] }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DataFieldType {
    /// UTF-8 text.
    String,
    /// 64-bit signed integer (stored as a JSON number).
    Int,
    /// 64-bit float (stored as a JSON number).
    Float,
    /// Boolean.
    Bool,
    /// RGBA color, stored as `[r, g, b, a]` floats in `0..=1`.
    Color,
    /// 2D vector, stored as `[x, y]` floats.
    Vec2,
    /// One choice from a fixed set of string options.
    Enum {
        /// Allowed values. The row stores one of these strings.
        #[serde(default)]
        options: Vec<String>,
    },
    /// Reference to a single row in a table of the named schema. Stored as
    /// `{ "table": "<relative path>", "id": "<row id>" }` or `Null`.
    Ref {
        /// Target schema name. Only rows of tables using this schema are valid.
        schema: String,
    },
    /// Path to a project asset (texture, atlas, prefab, ...). Stored as a
    /// relative-path string.
    Asset {
        /// Which kind of asset this field points at (drives the picker).
        #[serde(default)]
        asset_kind: AssetKind,
    },
    /// A homogeneous list of `element`-typed values. Stored as a JSON array.
    Array {
        /// Element type. May itself be any non-array type.
        element: Box<DataFieldType>,
    },
    /// An inline nested object with its own named sub-fields. Stored as a JSON
    /// object keyed by sub-field name.
    Object {
        /// The nested fields. May themselves be objects (arbitrary nesting).
        #[serde(default)]
        fields: Vec<DataFieldDef>,
    },
}

/// Categories of project asset an [`DataFieldType::Asset`] field can point at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// Any asset file.
    #[default]
    Any,
    /// A texture image (`.png`, `.jpg`).
    Texture,
    /// A sprite atlas (`.atlas`).
    Atlas,
    /// A prefab (`.prefab`).
    Prefab,
    /// A scene (`.scene`).
    Scene,
    /// An audio clip (`.ogg`, `.wav`, ...).
    Audio,
}

/// Errors from parsing or validating a `.dataschema` file.
#[derive(Debug, Error)]
pub enum DataSchemaError {
    /// JSON (de)serialization failure.
    #[error("data schema JSON error: {0}")]
    Parse(#[from] serde_json::Error),

    /// The file's `version` did not match [`DATA_SCHEMA_VERSION`].
    #[error("unsupported data schema version {found} (this build supports {supported})")]
    UnsupportedVersion {
        /// Version found in the file.
        found: u32,
        /// Version this build understands.
        supported: u32,
    },

    /// Two fields share a name.
    #[error("duplicate field name '{0}' in schema")]
    DuplicateField(String),
}

impl DataSchema {
    /// Create an empty schema with the current version.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: DATA_SCHEMA_VERSION,
            fields: Vec::new(),
        }
    }

    /// Parse a `.dataschema` JSON document and validate version + field names.
    pub fn from_json(json: &str) -> Result<Self, DataSchemaError> {
        let schema: DataSchema = serde_json::from_str(json)?;
        if schema.version != DATA_SCHEMA_VERSION {
            return Err(DataSchemaError::UnsupportedVersion {
                found: schema.version,
                supported: DATA_SCHEMA_VERSION,
            });
        }
        schema.validate()?;
        Ok(schema)
    }

    /// Serialize to pretty JSON.
    pub fn to_json_pretty(&self) -> Result<String, DataSchemaError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Validate structural invariants (unique field names).
    pub fn validate(&self) -> Result<(), DataSchemaError> {
        let mut seen = std::collections::HashSet::new();
        for field in &self.fields {
            if !seen.insert(field.name.as_str()) {
                return Err(DataSchemaError::DuplicateField(field.name.clone()));
            }
        }
        Ok(())
    }

    /// Look up a field definition by name.
    pub fn field(&self, name: &str) -> Option<&DataFieldDef> {
        self.fields.iter().find(|f| f.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_schema_with_every_field_type() {
        let mut schema = DataSchema::new("EnemyType");
        schema.fields = vec![
            DataFieldDef {
                name: "display_name".into(),
                ty: DataFieldType::String,
                default: serde_json::json!(""),
            },
            DataFieldDef {
                name: "health".into(),
                ty: DataFieldType::Int,
                default: serde_json::json!(100),
            },
            DataFieldDef {
                name: "speed".into(),
                ty: DataFieldType::Float,
                default: serde_json::json!(1.0),
            },
            DataFieldDef {
                name: "flying".into(),
                ty: DataFieldType::Bool,
                default: serde_json::json!(false),
            },
            DataFieldDef {
                name: "tint".into(),
                ty: DataFieldType::Color,
                default: serde_json::json!([1.0, 1.0, 1.0, 1.0]),
            },
            DataFieldDef {
                name: "spawn_offset".into(),
                ty: DataFieldType::Vec2,
                default: serde_json::json!([0.0, 0.0]),
            },
            DataFieldDef {
                name: "rarity".into(),
                ty: DataFieldType::Enum {
                    options: vec!["common".into(), "rare".into()],
                },
                default: serde_json::json!("common"),
            },
            DataFieldDef {
                name: "boss_of".into(),
                ty: DataFieldType::Ref {
                    schema: "ZoneType".into(),
                },
                default: serde_json::Value::Null,
            },
            DataFieldDef {
                name: "sprite".into(),
                ty: DataFieldType::Asset {
                    asset_kind: AssetKind::Texture,
                },
                default: serde_json::json!(""),
            },
            DataFieldDef {
                name: "drops".into(),
                ty: DataFieldType::Array {
                    element: Box::new(DataFieldType::Ref {
                        schema: "ItemType".into(),
                    }),
                },
                default: serde_json::json!([]),
            },
            DataFieldDef {
                name: "stats".into(),
                ty: DataFieldType::Object {
                    fields: vec![
                        DataFieldDef {
                            name: "attack".into(),
                            ty: DataFieldType::Int,
                            default: serde_json::json!(0),
                        },
                        DataFieldDef {
                            name: "defense".into(),
                            ty: DataFieldType::Int,
                            default: serde_json::json!(0),
                        },
                    ],
                },
                default: serde_json::json!({ "attack": 0, "defense": 0 }),
            },
        ];

        let json = schema.to_json_pretty().unwrap();
        let parsed = DataSchema::from_json(&json).unwrap();
        assert_eq!(schema, parsed);
    }

    #[test]
    fn rejects_wrong_version() {
        let json = r#"{ "name": "X", "version": 999, "fields": [] }"#;
        assert!(matches!(
            DataSchema::from_json(json),
            Err(DataSchemaError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_field() {
        let mut schema = DataSchema::new("X");
        schema.fields = vec![
            DataFieldDef {
                name: "a".into(),
                ty: DataFieldType::Int,
                default: serde_json::Value::Null,
            },
            DataFieldDef {
                name: "a".into(),
                ty: DataFieldType::Float,
                default: serde_json::Value::Null,
            },
        ];
        assert!(matches!(
            schema.validate(),
            Err(DataSchemaError::DuplicateField(_))
        ));
    }
}
