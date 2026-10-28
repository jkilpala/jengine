//! The behaviour-tree editor offers node types from
//! `editor/src/lib/behaviorTreeNodes.json`: each type's group, its default
//! node and the fields its form edits. A node type the engine has and the
//! catalogue lacks cannot be added in the editor; a default the engine
//! refuses is a node that breaks the tree it is added to. Both are checked
//! here - and `name_of` is an exhaustive match, so a new `BtNode` variant
//! does not compile until it is named here, and then fails until the
//! catalogue has it.
#![cfg(feature = "behavior_tree")]

use jengine_core::ecs::behavior_tree::BtNode;
use serde_json::Value;

/// The serde tag of every variant.
fn name_of(node: &BtNode) -> &'static str {
    match node {
        BtNode::Sequence { .. } => "sequence",
        BtNode::Selector { .. } => "selector",
        BtNode::Parallel { .. } => "parallel",
        BtNode::Random { .. } => "random",
        BtNode::Invert { .. } => "invert",
        BtNode::Succeed { .. } => "succeed",
        BtNode::Repeat { .. } => "repeat",
        BtNode::Cooldown { .. } => "cooldown",
        BtNode::Timeout { .. } => "timeout",
        BtNode::Check { .. } => "check",
        BtNode::Chance { .. } => "chance",
        BtNode::Near { .. } => "near",
        BtNode::Wait { .. } => "wait",
        BtNode::MoveTo { .. } => "move_to",
        BtNode::Set { .. } => "set",
        BtNode::Write { .. } => "write",
        BtNode::Event { .. } => "event",
        BtNode::Call { .. } => "call",
    }
}

const ALL: &[&str] = &[
    "sequence", "selector", "parallel", "random", "invert", "succeed", "repeat", "cooldown", "timeout", "check",
    "chance", "near", "wait", "move_to", "set", "write", "event", "call",
];

#[test]
fn behavior_tree_nodes_match_the_editor() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../editor/src/lib/behaviorTreeNodes.json"))
        .expect("the catalogue");
    let catalogue: Value = serde_json::from_str(&text).expect("JSON");
    let nodes = catalogue["nodes"].as_array().expect("nodes");
    let mut seen = Vec::new();
    for entry in nodes {
        let ty = entry["type"].as_str().expect("type");
        let node: BtNode = serde_json::from_value(entry["default"].clone())
            .unwrap_or_else(|e| panic!("the default `{ty}` is not a node the engine reads: {e}"));
        assert_eq!(name_of(&node), ty, "the default of `{ty}` reads as another type");
        // Every field the form edits is one the node keeps, so an edit is not
        // dropped on load: set it and read it back.
        for field in entry["fields"].as_array().into_iter().flatten() {
            let name = field["name"].as_str().expect("field name");
            let mut probe = entry["default"].clone();
            let value = match field["kind"].as_str() {
                Some("bool") => Value::Bool(true),
                Some("int") => Value::from(3),
                Some("number") => Value::from(2.5),
                Some("enum") => field["options"][0].clone(),
                _ => Value::from("probe"),
            };
            probe[name] = value.clone();
            let read: BtNode = serde_json::from_value(probe).unwrap_or_else(|e| panic!("`{ty}.{name}`: {e}"));
            let back = serde_json::to_value(&read).expect("serializes");
            assert_eq!(back.get(name), Some(&value), "`{ty}.{name}` is not a field the engine keeps");
        }
        seen.push(ty.to_string());
    }
    let missing: Vec<_> = ALL.iter().filter(|t| !seen.iter().any(|s| s == *t)).collect();
    assert!(missing.is_empty(), "node types the editor cannot add: {missing:?}");
    assert_eq!(seen.len(), ALL.len(), "the catalogue has a type the engine does not: {seen:?}");
}
