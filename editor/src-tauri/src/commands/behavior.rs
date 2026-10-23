//! Behavior-graph editor commands.
//!
//! - Slice E.1: expose the runtime's op catalog so the editor can
//!   render a palette and validate connections without
//!   re-implementing the rules.
//! - Slice E.2: validate a behavior graph by running the existing
//!   runtime compiler and converting its `Result<_, CompileError>`
//!   into a stable JSON `CompileReport` for the editor.

use jengine_core::behavior::{
    compile, op_catalog, BehaviorAsset, BehaviorEdge, CompileDiagnostic, CompileError,
    CompileWarning, ControlPort, OpDescriptor,
};
use serde::Serialize;

/// Returns the v1 op catalog. Cheap; the editor caches the result
/// for the session.
#[tauri::command]
pub fn behavior_op_catalog() -> Vec<OpDescriptor> {
    op_catalog()
}

/// One compiler diagnostic the editor can pin to a specific node /
/// pin. `code` is a stable identifier (e.g. `"unknown_op"`); `message`
/// is the user-facing description.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub node_id: Option<String>,
    pub pin: Option<String>,
    pub code: &'static str,
    pub message: String,
}

/// Result of `compile_behavior`. `ok = true` only when there are zero
/// errors (warnings are allowed).
#[derive(Debug, Clone, Serialize)]
pub struct CompileReport {
    pub ok: bool,
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
}

/// Validate a `.behavior` JSON document. Returns a non-empty
/// `errors` list if either parsing or compiling fails. Never returns
/// a Tauri error — the editor wants to render the diagnostic, not a
/// dialog.
#[tauri::command]
pub fn compile_behavior(json: String) -> CompileReport {
    let asset: BehaviorAsset = match serde_json::from_str(&json) {
        Ok(asset) => asset,
        Err(err) => {
            return CompileReport {
                ok: false,
                errors: vec![Diagnostic {
                    node_id: None,
                    pin: None,
                    code: "json_parse",
                    message: err.to_string(),
                }],
                warnings: Vec::new(),
            };
        }
    };

    match compile(&asset) {
        Ok(graph) => CompileReport {
            ok: true,
            errors: Vec::new(),
            warnings: graph
                .diagnostics
                .iter()
                .map(warning_to_diagnostic)
                .collect(),
        },
        Err(err) => CompileReport {
            ok: false,
            errors: vec![compile_error_to_diagnostic(&err)],
            warnings: Vec::new(),
        },
    }
}

fn compile_error_to_diagnostic(err: &CompileError) -> Diagnostic {
    let message = err.to_string();
    match err {
        CompileError::UnknownOp { node_id, .. } => Diagnostic {
            node_id: Some(node_id.clone()),
            pin: None,
            code: "unknown_op",
            message,
        },
        CompileError::DuplicateNodeId { node_id } => Diagnostic {
            node_id: Some(node_id.clone()),
            pin: None,
            code: "duplicate_node_id",
            message,
        },
        CompileError::EdgeRefNotFound { from, .. } => Diagnostic {
            node_id: Some(from.clone()),
            pin: None,
            code: "edge_ref_not_found",
            message,
        },
        CompileError::MissingInput { node_id, pin, .. } => Diagnostic {
            node_id: Some(node_id.clone()),
            pin: Some((*pin).to_string()),
            code: "missing_input",
            message,
        },
        CompileError::PinRefNotFound { node_id, pin, .. } => Diagnostic {
            node_id: Some(node_id.clone()),
            pin: Some((*pin).to_string()),
            code: "pin_ref_not_found",
            message,
        },
        CompileError::PinTypeMismatch { node_id, pin, .. } => Diagnostic {
            node_id: Some(node_id.clone()),
            pin: Some((*pin).to_string()),
            code: "pin_type_mismatch",
            message,
        },
        CompileError::PinSourceNotProducer { node_id, pin, .. } => Diagnostic {
            node_id: Some(node_id.clone()),
            pin: Some((*pin).to_string()),
            code: "pin_source_not_producer",
            message,
        },
        CompileError::DataCycle { node_id } => Diagnostic {
            node_id: Some(node_id.clone()),
            pin: None,
            code: "data_cycle",
            message,
        },
        CompileError::LiteralTypeMismatch { node_id, pin, .. } => Diagnostic {
            node_id: Some(node_id.clone()),
            pin: Some((*pin).to_string()),
            code: "literal_type_mismatch",
            message,
        },
        CompileError::UnknownInputPin { node_id, pin, .. } => Diagnostic {
            node_id: Some(node_id.clone()),
            pin: Some(pin.clone()),
            code: "unknown_input_pin",
            message,
        },
        CompileError::InvalidControlPort { from, .. } => Diagnostic {
            node_id: Some(from.clone()),
            pin: None,
            code: "invalid_control_port",
            message,
        },
        CompileError::InvalidKeyTriggerParam { node_id, param, .. } => Diagnostic {
            node_id: Some(node_id.clone()),
            pin: Some((*param).to_string()),
            code: "invalid_key_trigger_param",
            message,
        },
        CompileError::DuplicateOnKeyTrigger { second, .. } => Diagnostic {
            node_id: Some(second.clone()),
            pin: None,
            code: "duplicate_on_key_trigger",
            message,
        },
        CompileError::OnEventMissingName { node_id } => Diagnostic {
            node_id: Some(node_id.clone()),
            pin: None,
            code: "on_event_missing_name",
            message,
        },
        CompileError::OnMouseClickInvalidButton { node_id, .. } => Diagnostic {
            node_id: Some(node_id.clone()),
            pin: Some("button".to_string()),
            code: "on_mouse_click_invalid_button",
            message,
        },
    }
}

fn warning_to_diagnostic(diag: &CompileDiagnostic) -> Diagnostic {
    let (code, message) = match &diag.kind {
        CompileWarning::SetupOpUnderOnTick { op } => (
            "setup_op_under_on_tick",
            format!(
                "node `{}`: setup op `{}` is reachable from OnTick; \
                 wire it from OnSpawn instead",
                diag.node_id, op,
            ),
        ),
    };
    Diagnostic {
        node_id: Some(diag.node_id.clone()),
        pin: None,
        code,
        message,
    }
}

/// Writes a `.behavior` JSON document to disk.
///
/// Slice E.4: needed for `Ctrl+S` save. Validates that the JSON
/// parses as a `BehaviorAsset` so we never persist garbage; does
/// **not** require it to compile cleanly (warnings/errors are
/// surfaced through `compile_behavior` and the editor decides
/// whether to save anyway).
///
/// Slice E.9: re-serialises through [`canonical_behavior_json`] so
/// that whatever the editor sends in (with arbitrary key order,
/// arbitrary edge order, mixed indentation), the on-disk form is
/// stable. Saving the same logical graph twice is a byte-identical
/// no-op which keeps version-control diffs noise-free.
#[tauri::command]
pub fn write_behavior(path: String, json: String) -> Result<(), String> {
    let asset: BehaviorAsset = serde_json::from_str(&json)
        .map_err(|e| format!("Refusing to write invalid behavior JSON: {e}"))?;
    let canonical = canonical_behavior_json(&asset)
        .map_err(|e| format!("Failed to canonicalise behavior JSON: {e}"))?;
    std::fs::write(&path, canonical).map_err(|e| format!("Failed to write behavior: {e}"))?;
    Ok(())
}

/// Slice E.9: deterministic on-disk form of a [`BehaviorAsset`].
///
/// The format itself (`BTreeMap` for `params`, `inputs`,
/// `editor.positions`, and `editor.extra`) already sorts keys for
/// us. The remaining variables are:
///
/// - **Edge order**. Stable sort by `(from, out, to, in)` so two
///   logically-equivalent graphs serialise identically regardless
///   of authoring order.
/// - **Trailing newline**. Always present; matches POSIX text-file
///   conventions and what most editors append on save.
///
/// Node order is **not** rewritten — node lists carry semantic
/// authoring intent (e.g. event order in palettes) and rewriting
/// would surprise users. The function is idempotent: feeding its
/// output back through it returns the same bytes.
pub fn canonical_behavior_json(asset: &BehaviorAsset) -> Result<String, serde_json::Error> {
    let mut canonical = asset.clone();
    canonical.edges.sort_by(edge_sort_key);
    let mut out = serde_json::to_string_pretty(&canonical)?;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn edge_sort_key(a: &BehaviorEdge, b: &BehaviorEdge) -> std::cmp::Ordering {
    a.from
        .cmp(&b.from)
        .then_with(|| port_key(&a.out).cmp(port_key(&b.out)))
        .then_with(|| a.to.cmp(&b.to))
        .then_with(|| port_key(&a.r#in).cmp(port_key(&b.r#in)))
}

fn port_key(p: &ControlPort) -> &str {
    match p {
        ControlPort::Exec => "exec",
        ControlPort::Then => "then",
        ControlPort::Else => "else",
        ControlPort::Other(s) => s.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_report_round_trip_minimal_ok() {
        let json = r#"{
            "name": "Probe",
            "version": 1,
            "nodes": [{ "id": "tick", "op": "OnTick" }],
            "edges": []
        }"#;
        let report = compile_behavior(json.to_string());
        assert!(report.ok, "expected ok report, got {report:?}");
        assert!(report.errors.is_empty());
    }

    #[test]
    fn compile_report_unknown_op_is_diagnostic() {
        let json = r#"{
            "name": "Probe",
            "version": 1,
            "nodes": [{ "id": "n0", "op": "DoesNotExist" }],
            "edges": []
        }"#;
        let report = compile_behavior(json.to_string());
        assert!(!report.ok);
        assert_eq!(report.errors.len(), 1);
        let diag = &report.errors[0];
        assert_eq!(diag.code, "unknown_op");
        assert_eq!(diag.node_id.as_deref(), Some("n0"));
    }

    #[test]
    fn compile_report_garbage_json_is_diagnostic_not_error() {
        let report = compile_behavior("{ this is not json".to_string());
        assert!(!report.ok);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].code, "json_parse");
        assert!(report.errors[0].node_id.is_none());
    }

    #[test]
    fn write_behavior_round_trips_valid_json_to_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("probe.behavior");
        let json = r#"{"name":"Probe","version":1,"nodes":[{"id":"tick","op":"OnTick"}],"edges":[]}"#;
        write_behavior(path.to_string_lossy().to_string(), json.to_string()).expect("write");
        let on_disk = std::fs::read_to_string(&path).expect("read back");
        // Slice E.9: written form is the canonical pretty-printed
        // serialisation, not the original input string.
        let asset: BehaviorAsset = serde_json::from_str(json).expect("parse input");
        let expected = canonical_behavior_json(&asset).expect("canonical");
        assert_eq!(on_disk, expected);
    }

    #[test]
    fn write_behavior_refuses_invalid_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.behavior");
        let err = write_behavior(
            path.to_string_lossy().to_string(),
            "{ not json".to_string(),
        )
        .unwrap_err();
        assert!(err.contains("invalid behavior JSON"), "got: {err}");
        assert!(!path.exists(), "must not create file when JSON invalid");
    }

    // ── Slice E.9 ───────────────────────────────────────────────

    #[test]
    fn canonical_form_is_idempotent() {
        let json = r#"{
            "name": "Mix",
            "version": 1,
            "nodes": [
                { "id": "a", "op": "OnTick" },
                { "id": "b", "op": "BounceOnBounds" },
                { "id": "c", "op": "BounceOnBounds" }
            ],
            "edges": [
                { "from": "a", "to": "c" },
                { "from": "a", "to": "b" }
            ]
        }"#;
        let asset: BehaviorAsset = serde_json::from_str(json).expect("parse");
        let once = canonical_behavior_json(&asset).expect("first pass");
        let asset2: BehaviorAsset = serde_json::from_str(&once).expect("reparse");
        let twice = canonical_behavior_json(&asset2).expect("second pass");
        assert_eq!(once, twice, "canonical form must be idempotent");
    }

    #[test]
    fn canonical_form_sorts_edges_stably() {
        let json = r#"{
            "name": "Edges",
            "version": 1,
            "nodes": [
                { "id": "a", "op": "If" },
                { "id": "b", "op": "BounceOnBounds" },
                { "id": "c", "op": "BounceOnBounds" }
            ],
            "edges": [
                { "from": "a", "out": "else", "to": "b" },
                { "from": "a", "out": "then", "to": "c" },
                { "from": "a", "out": "then", "to": "b" }
            ]
        }"#;
        let asset: BehaviorAsset = serde_json::from_str(json).expect("parse");
        let canon = canonical_behavior_json(&asset).expect("canonical");
        let reparsed: BehaviorAsset = serde_json::from_str(&canon).expect("reparse");
        let keys: Vec<(String, String, String)> = reparsed
            .edges
            .iter()
            .map(|e| (e.from.clone(), port_key(&e.out).to_string(), e.to.clone()))
            .collect();
        // Sort order: (from, out, to). "else" < "then" lexicographically.
        assert_eq!(
            keys,
            vec![
                ("a".to_string(), "else".to_string(), "b".to_string()),
                ("a".to_string(), "then".to_string(), "b".to_string()),
                ("a".to_string(), "then".to_string(), "c".to_string()),
            ]
        );
    }

    #[test]
    fn canonical_form_ends_with_newline() {
        let json = r#"{"name":"X","version":1,"nodes":[],"edges":[]}"#;
        let asset: BehaviorAsset = serde_json::from_str(json).expect("parse");
        let canon = canonical_behavior_json(&asset).expect("canonical");
        assert!(canon.ends_with('\n'), "missing trailing newline: {canon:?}");
    }

    #[test]
    fn canonical_form_round_trips_workspace_behaviors() {
        // Walk every .behavior under projects/. Each must (a) parse,
        // (b) canonicalise, and (c) re-canonicalise to the same bytes.
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace root")
            .to_path_buf();
        let projects = workspace_root.join("projects");
        if !projects.is_dir() {
            return; // No projects in checkout — nothing to verify.
        }
        let mut visited = 0;
        for entry in walkdir(&projects) {
            if entry.extension().and_then(|e| e.to_str()) != Some("behavior") {
                continue;
            }
            let raw = std::fs::read_to_string(&entry)
                .unwrap_or_else(|e| panic!("read {entry:?}: {e}"));
            let asset: BehaviorAsset = serde_json::from_str(&raw)
                .unwrap_or_else(|e| panic!("parse {entry:?}: {e}"));
            let once = canonical_behavior_json(&asset)
                .unwrap_or_else(|e| panic!("canonicalise {entry:?}: {e}"));
            let asset2: BehaviorAsset = serde_json::from_str(&once)
                .unwrap_or_else(|e| panic!("reparse {entry:?}: {e}"));
            let twice = canonical_behavior_json(&asset2)
                .unwrap_or_else(|e| panic!("re-canonicalise {entry:?}: {e}"));
            assert_eq!(once, twice, "non-idempotent canonical form for {entry:?}");
            visited += 1;
        }
        assert!(visited > 0, "expected at least one .behavior fixture");
    }

    fn walkdir(root: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(read) = std::fs::read_dir(&dir) else { continue };
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.is_file() {
                    out.push(path);
                }
            }
        }
        out
    }
}
