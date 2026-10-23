//! `.behavior` asset format — serde-serialisable representation of a
//! behavior graph. Slice 7.1 of Step 7.
//!
//! The runtime never consumes this struct directly on the hot path —
//! a later slice (7.2) will lower it into a typed `CompiledGraph`.
//! For now we just need a parser + serialiser with a stable schema
//! that preserves opaque editor metadata across round-trips.
//!
//! See [`docs/step7-behavior-graph-spec.md`](../../../../docs/step7-behavior-graph-spec.md) §3.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Major schema version understood by this build. Loader rejects any
/// `.behavior` file whose `version` field differs.
pub const BEHAVIOR_SCHEMA_VERSION: u32 = 1;

/// Top-level `.behavior` asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BehaviorAsset {
    /// Display name. Human-readable; not a stable id.
    pub name: String,

    /// Schema version. Must equal [`BEHAVIOR_SCHEMA_VERSION`] at load.
    pub version: u32,

    #[serde(default)]
    pub nodes: Vec<BehaviorNode>,

    #[serde(default)]
    pub edges: Vec<BehaviorEdge>,

    /// Opaque editor-only metadata. Preserved verbatim on round-trip;
    /// the runtime never reads its contents in Slice 7.1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<EditorMetadata>,
}

impl BehaviorAsset {
    /// Parse a `.behavior` JSON document and validate the schema
    /// version. Other validation (unknown ops, missing refs, pin
    /// types) lands in Slice 7.2 with the compiler.
    pub fn from_json(json: &str) -> Result<Self, BehaviorFormatError> {
        let asset: BehaviorAsset =
            serde_json::from_str(json).map_err(BehaviorFormatError::Parse)?;
        if asset.version != BEHAVIOR_SCHEMA_VERSION {
            return Err(BehaviorFormatError::UnsupportedSchemaVersion {
                found: asset.version,
                supported: BEHAVIOR_SCHEMA_VERSION,
            });
        }
        Ok(asset)
    }

    /// Serialise to pretty JSON.
    pub fn to_json_pretty(&self) -> Result<String, BehaviorFormatError> {
        serde_json::to_string_pretty(self).map_err(BehaviorFormatError::Parse)
    }
}

/// A single graph node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BehaviorNode {
    /// Stable per-graph id (e.g. `"n0"`). Referenced by edges and pin bindings.
    pub id: String,

    /// Op name from the v1 vocabulary (see spec §2). Validated by the
    /// compiler in Slice 7.2.
    pub op: String,

    /// Baked literal parameters (e.g. `seconds`, `prefab_id`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,

    /// Per-input-pin data bindings.
    #[serde(rename = "in", default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, PinBinding>,
}

/// A control-flow edge between two nodes' exec ports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BehaviorEdge {
    /// Source node id.
    pub from: String,

    /// Source exec port. `"exec"` for plain triggers, `"then"` /
    /// `"else"` for `If`. Other names are rejected by the compiler in
    /// Slice 7.2.
    #[serde(default = "default_exec_port")]
    pub out: ControlPort,

    /// Destination node id.
    pub to: String,

    /// Destination exec port. Always `"exec"` in v1.
    #[serde(rename = "in", default = "default_exec_port")]
    pub r#in: ControlPort,
}

fn default_exec_port() -> ControlPort {
    ControlPort::Exec
}

/// Named exec port on a node. v1 has only the three variants below;
/// unknown names round-trip through [`ControlPort::Other`] so that
/// future op vocab additions do not break existing files.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlPort {
    Exec,
    Then,
    Else,
    #[serde(untagged)]
    Other(String),
}

/// Data binding for an input pin.
///
/// Either a literal (`{"value": ...}`) or a reference to another node's
/// output pin (`{"from": "...", "out": "..."}`). The optional `scale`
/// and `offset` modifiers are applied numerically by the runtime;
/// type-checking happens in the compiler (Slice 7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PinBinding {
    Literal {
        value: serde_json::Value,
    },
    Reference {
        from: String,
        #[serde(default)]
        out: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<f32>,
    },
}

/// Opaque editor metadata. Round-tripped verbatim; no field is required.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EditorMetadata {
    /// Per-node 2D positions in the visual editor canvas.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub positions: BTreeMap<String, EditorNodePosition>,

    /// Anything else the editor wants to persist (comments, groups,
    /// colors). Preserved verbatim.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EditorNodePosition {
    pub x: f32,
    pub y: f32,
}

/// Errors the format module can produce. Compiler-level errors land
/// in Slice 7.2 alongside `CompiledGraph`.
#[derive(Debug, Error)]
pub enum BehaviorFormatError {
    #[error("failed to parse .behavior JSON: {0}")]
    Parse(#[source] serde_json::Error),

    #[error("unsupported .behavior schema version {found}; this build supports {supported}")]
    UnsupportedSchemaVersion { found: u32, supported: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the example in `docs/step7-behavior-graph-spec.md` §3.2.
    const PLAYER_MOVER_JSON: &str = r#"{
        "name": "PlayerMover",
        "version": 1,
        "nodes": [
            { "id": "n0", "op": "OnTick" },
            { "id": "n1", "op": "GetInputAxis" },
            { "id": "n2", "op": "SetVelocity",
              "in": { "vel": { "from": "n1", "out": "value", "scale": 200.0 } } },
            { "id": "n3", "op": "BounceOnBounds" }
        ],
        "edges": [
            { "from": "n0", "out": "exec", "to": "n2", "in": "exec" },
            { "from": "n0", "out": "exec", "to": "n3", "in": "exec" }
        ],
        "editor": {
            "positions": {
                "n0": { "x": 100.0, "y": 100.0 },
                "n1": { "x": 100.0, "y": 220.0 },
                "n2": { "x": 320.0, "y": 160.0 },
                "n3": { "x": 320.0, "y": 280.0 }
            }
        }
    }"#;

    #[test]
    fn parses_player_mover_example() {
        let asset = BehaviorAsset::from_json(PLAYER_MOVER_JSON).expect("parse");
        assert_eq!(asset.name, "PlayerMover");
        assert_eq!(asset.version, BEHAVIOR_SCHEMA_VERSION);
        assert_eq!(asset.nodes.len(), 4);
        assert_eq!(asset.edges.len(), 2);

        let set_velocity = asset
            .nodes
            .iter()
            .find(|n| n.id == "n2")
            .expect("n2 present");
        assert_eq!(set_velocity.op, "SetVelocity");
        match set_velocity.inputs.get("vel").expect("vel binding") {
            PinBinding::Reference { from, out, scale, .. } => {
                assert_eq!(from, "n1");
                assert_eq!(out.as_deref(), Some("value"));
                assert_eq!(*scale, Some(200.0));
            }
            other => panic!("expected reference binding, got {other:?}"),
        }

        let editor = asset.editor.as_ref().expect("editor metadata");
        assert_eq!(editor.positions.len(), 4);
        assert_eq!(editor.positions["n0"], EditorNodePosition { x: 100.0, y: 100.0 });
    }

    #[test]
    fn round_trip_preserves_editor_metadata_and_unknown_extras() {
        // Extra editor field that the runtime doesn't understand —
        // must round-trip verbatim so a future editor build can read it.
        let json = r##"{
            "name": "WithExtras",
            "version": 1,
            "nodes": [{ "id": "n0", "op": "OnTick" }],
            "edges": [],
            "editor": {
                "positions": { "n0": { "x": 1.0, "y": 2.0 } },
                "comments": [{ "text": "hello", "x": 0.0, "y": 0.0 }],
                "group_color": "#ff8800"
            }
        }"##;

        let asset = BehaviorAsset::from_json(json).expect("parse");
        let editor = asset.editor.as_ref().expect("editor");
        assert!(editor.extra.contains_key("comments"));
        assert_eq!(
            editor.extra.get("group_color").and_then(|v| v.as_str()),
            Some("#ff8800")
        );

        // Re-serialise and re-parse; structures must be equal.
        let serialised = asset.to_json_pretty().expect("serialise");
        let reparsed = BehaviorAsset::from_json(&serialised).expect("reparse");
        assert_eq!(asset, reparsed);
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let json = r#"{ "name": "FromTheFuture", "version": 99, "nodes": [], "edges": [] }"#;
        let err = BehaviorAsset::from_json(json).expect_err("should reject");
        match err {
            BehaviorFormatError::UnsupportedSchemaVersion { found, supported } => {
                assert_eq!(found, 99);
                assert_eq!(supported, BEHAVIOR_SCHEMA_VERSION);
            }
            other => panic!("wrong error variant: {other:?}"),
        }
    }

    #[test]
    fn defaults_edge_ports_to_exec() {
        // Edges may omit `out` / `in` — both default to "exec".
        let json = r#"{
            "name": "DefaultPorts",
            "version": 1,
            "nodes": [
                { "id": "a", "op": "OnTick" },
                { "id": "b", "op": "BounceOnBounds" }
            ],
            "edges": [{ "from": "a", "to": "b" }]
        }"#;

        let asset = BehaviorAsset::from_json(json).expect("parse");
        let edge = &asset.edges[0];
        assert_eq!(edge.out, ControlPort::Exec);
        assert_eq!(edge.r#in, ControlPort::Exec);
    }

    #[test]
    fn parses_then_else_control_ports() {
        let json = r#"{
            "name": "Branchy",
            "version": 1,
            "nodes": [
                { "id": "g", "op": "If" },
                { "id": "t", "op": "PlaySound" },
                { "id": "e", "op": "PlaySound" }
            ],
            "edges": [
                { "from": "g", "out": "then", "to": "t" },
                { "from": "g", "out": "else", "to": "e" }
            ]
        }"#;

        let asset = BehaviorAsset::from_json(json).expect("parse");
        assert_eq!(asset.edges[0].out, ControlPort::Then);
        assert_eq!(asset.edges[1].out, ControlPort::Else);
    }

    #[test]
    fn literal_pin_binding_round_trips() {
        let json = r#"{
            "name": "Literal",
            "version": 1,
            "nodes": [
                { "id": "n0", "op": "DespawnAfter",
                  "in": { "seconds": { "value": 2.5 } } }
            ],
            "edges": []
        }"#;

        let asset = BehaviorAsset::from_json(json).expect("parse");
        let node = &asset.nodes[0];
        match node.inputs.get("seconds").expect("seconds") {
            PinBinding::Literal { value } => {
                assert_eq!(value.as_f64(), Some(2.5));
            }
            other => panic!("expected literal, got {other:?}"),
        }

        let serialised = asset.to_json_pretty().expect("serialise");
        let reparsed = BehaviorAsset::from_json(&serialised).expect("reparse");
        assert_eq!(asset, reparsed);
    }
}
