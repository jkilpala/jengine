//! Input map asset (`.input`) — backlog 2.5.
//!
//! A versioned JSON document describing default input actions for a
//! project. Loaded once at startup; bindings can be remapped at runtime
//! via the script API (`input_set_bindings` and friends) and persisted
//! through the save system (2.3).
//!
//! ## Format
//!
//! ```json
//! {
//!   "version": 1,
//!   "actions": [
//!     { "name": "jump",  "keys": ["Space", "W"],     "dead_zone": 0.0 },
//!     { "name": "shoot", "keys": ["Mouse0", "Enter"], "dead_zone": 0.0 }
//!   ]
//! }
//! ```
//!
//! `dead_zone` is optional (defaults to `0.0`) and is reserved for
//! future gamepad axis bindings — the keyboard dispatch in
//! [`super::InputState`] ignores it today.

use serde::{Deserialize, Serialize};

use super::InputAction;

/// Current `.input` schema version. Older versions are not auto-migrated;
/// unknown versions log a warning and fall back to an empty action list
/// (mirroring the 2.3 save-system policy).
pub const INPUT_MAP_VERSION: u32 = 1;

/// Top-level `.input` asset structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputMap {
    /// Schema version; see [`INPUT_MAP_VERSION`].
    pub version: u32,
    /// Default action bindings.
    pub actions: Vec<InputAction>,
}

impl InputMap {
    /// Build an empty map at the current schema version.
    pub fn new() -> Self {
        Self {
            version: INPUT_MAP_VERSION,
            actions: Vec::new(),
        }
    }

    /// Parse a JSON string. Returns `None` (and logs a warning) for
    /// version mismatches; serde errors propagate.
    pub fn from_json_str(text: &str) -> Result<Option<Self>, serde_json::Error> {
        let map: Self = serde_json::from_str(text)?;
        if map.version != INPUT_MAP_VERSION {
            log::warn!(
                "InputMap: unknown version {} (expected {}), ignoring",
                map.version,
                INPUT_MAP_VERSION
            );
            return Ok(None);
        }
        Ok(Some(map))
    }

    /// Serialize to a pretty JSON string for on-disk storage.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_actions() {
        let mut map = InputMap::new();
        map.actions.push(InputAction {
            name: "jump".into(),
            keys: vec!["Space".into(), "W".into()],
            dead_zone: 0.0,
        });
        let json = map.to_json_string().unwrap();
        let parsed = InputMap::from_json_str(&json).unwrap().unwrap();
        assert_eq!(parsed.version, INPUT_MAP_VERSION);
        assert_eq!(parsed.actions.len(), 1);
        assert_eq!(parsed.actions[0].name, "jump");
        assert_eq!(parsed.actions[0].keys, vec!["Space", "W"]);
    }

    #[test]
    fn missing_dead_zone_defaults_to_zero() {
        let json = r#"{ "version": 1, "actions": [{ "name": "fire", "keys": ["Space"] }] }"#;
        let parsed = InputMap::from_json_str(json).unwrap().unwrap();
        assert_eq!(parsed.actions[0].dead_zone, 0.0);
    }

    #[test]
    fn unknown_version_is_rejected() {
        let json = r#"{ "version": 999, "actions": [] }"#;
        let parsed = InputMap::from_json_str(json).unwrap();
        assert!(parsed.is_none());
    }
}
