//! Neutral value type passed across the script bridge.
//!
//! The bridge intentionally supports only a small, portable value set. Every backend adapter
//! must be able to marshal these losslessly to and from its native runtime.

use std::collections::BTreeMap;

/// Opaque kind tag for engine-owned handles. Scripts cannot fabricate handles; they only
/// receive them from bridge calls and pass them back unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandleKind {
    Texture,
    Sound,
    Tween,
    Timer,
    GuiNode,
    Material,
    Animation,
}

/// Language-neutral value carried across the script bridge.
///
/// Adapters convert this to and from their native value representation. The set is
/// intentionally minimal — extensions require an architecture review.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
    EntityId(u64),
    Vec2(f32, f32),
    Color(f32, f32, f32, f32),
    Handle(HandleKind, u64),
    Array(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

impl Value {
    /// A JSON value as a bridge value. What an animation frame's event data
    /// (authored JSON) has to become before a script can read it; numbers
    /// that do not fit an `f64` become nil rather than lying.
    pub fn from_json(v: &serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Value::Nil,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => n.as_f64().map_or(Value::Nil, Value::Number),
            serde_json::Value::String(s) => Value::String(s.clone()),
            serde_json::Value::Array(a) => Value::Array(a.iter().map(Value::from_json).collect()),
            serde_json::Value::Object(o) => {
                Value::Map(o.iter().map(|(k, v)| (k.clone(), Value::from_json(v))).collect())
            }
        }
    }

    /// Returns `true` for [`Value::Nil`].
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    /// Best-effort numeric coercion. Returns `None` if the value is not numeric.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    /// Returns the string representation if the value is a [`Value::String`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Number(v as f64)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Number(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Number(v as f64)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_owned())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nil_is_nil() {
        assert!(Value::Nil.is_nil());
        assert!(!Value::Bool(true).is_nil());
    }

    #[test]
    fn numeric_coercion() {
        assert_eq!(Value::Number(3.5).as_number(), Some(3.5));
        assert_eq!(Value::Bool(true).as_number(), Some(1.0));
        assert_eq!(Value::Bool(false).as_number(), Some(0.0));
        assert_eq!(Value::String("x".into()).as_number(), None);
    }

    #[test]
    fn from_impls() {
        let v: Value = 42i32.into();
        assert_eq!(v, Value::Number(42.0));
        let v: Value = "hi".into();
        assert_eq!(v, Value::String("hi".into()));
    }
}
