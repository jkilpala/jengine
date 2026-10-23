//! Sort layers asset (`.sortlayers`) — named ordering buckets for
//! renderable components.
//!
//! A `.sortlayers` JSON file declares an ordered list of layer names with
//! base `z_order` values. Components such as `SpriteComponent`,
//! `TextComponent`, `VectorPath`, `ParticleEmitter`, and `Tilemap` may
//! carry an optional `sort_layer` string referencing one of these
//! layers. At render time the effective draw order is
//! `base_z + z_order`, letting scenes avoid magic numbers and keep
//! per-pass ordering consistent.
//!
//! ## Format
//! ```json
//! {
//!   "layers": [
//!     { "name": "background", "base_z": -1000 },
//!     { "name": "default",    "base_z": 0 },
//!     { "name": "foreground", "base_z": 1000 }
//!   ]
//! }
//! ```
//!
//! The order of the array is purely cosmetic — `base_z` is the authority.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A single named ordering bucket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SortLayer {
    pub name: String,
    pub base_z: i32,
}

/// Ordered collection of [`SortLayer`]s. Resolution is by name lookup;
/// missing names fall back to a base of `0` so existing scenes without a
/// `sort_layer` keep their previous behavior.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SortLayers {
    #[serde(default)]
    pub layers: Vec<SortLayer>,
    /// Cached name → base_z lookup. Rebuilt on every deserialize via
    /// [`SortLayers::rebuild_index`]; user code should call it after
    /// mutating `layers` directly.
    #[serde(skip)]
    index: HashMap<String, i32>,
}

impl SortLayers {
    /// Build an empty set — equivalent to `Default::default()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct from an explicit layer list.
    pub fn from_layers(layers: Vec<SortLayer>) -> Self {
        let mut s = Self { layers, index: HashMap::new() };
        s.rebuild_index();
        s
    }

    /// Refresh the internal name → base_z map. Call after mutating
    /// `layers` directly (e.g. from the editor inspector).
    pub fn rebuild_index(&mut self) {
        self.index.clear();
        for l in &self.layers {
            self.index.insert(l.name.clone(), l.base_z);
        }
    }

    /// Base z value for a named layer, or `None` if the name isn't
    /// defined.
    pub fn base_for(&self, name: &str) -> Option<i32> {
        // Hit the cached index when populated; otherwise scan (handles the
        // case where a caller mutated `layers` and forgot to rebuild).
        if !self.index.is_empty() || self.layers.is_empty() {
            self.index.get(name).copied()
        } else {
            self.layers.iter().find(|l| l.name == name).map(|l| l.base_z)
        }
    }

    /// Resolve an effective render z from an optional layer name and a
    /// raw `z_order`. Unknown / `None` layer names fall back to just
    /// `z_order`, matching pre-existing behavior.
    pub fn resolve(&self, sort_layer: Option<&str>, z_order: i32) -> i32 {
        match sort_layer.and_then(|n| self.base_for(n)) {
            Some(b) => b.saturating_add(z_order),
            None => z_order,
        }
    }

    /// Parse a `.sortlayers` JSON string. Index is rebuilt automatically.
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let mut s: SortLayers = serde_json::from_str(json)
            .map_err(|e| anyhow::anyhow!("failed to parse .sortlayers: {e}"))?;
        s.rebuild_index();
        Ok(s)
    }
}

/// Resolve a scene's `sort_layers` field against a list of candidate
/// asset roots, load the referenced `.sortlayers` file into `assets`
/// (if not already cached), and mark it as the active set.
///
/// - If the scene has no `sort_layers` field, the active set is cleared
///   (subsequent `assets.active_sort_layers()` calls return an empty
///   passthrough set — preserving existing `z_order` behavior).
/// - Errors are returned individually so a missing `.sortlayers` asset
///   does not bring down an otherwise-valid scene; the active set falls
///   back to empty in that case.
#[cfg(not(target_arch = "wasm32"))]
pub fn resolve_scene_sort_layers(
    scene_json: &serde_json::Value,
    candidates: &[std::path::PathBuf],
    assets: &mut crate::asset::AssetManager,
) -> Vec<anyhow::Error> {
    let mut errors = Vec::new();

    let Some(rel) = scene_json.get("sort_layers").and_then(|v| v.as_str()) else {
        assets.set_active_sort_layers(None);
        return errors;
    };

    if assets.get_sort_layers(rel).is_none() {
        let mut loaded = false;
        if let Some(path) = crate::asset::find_in_roots(rel, candidates) {
            match assets.load_sort_layers(rel, &path.to_string_lossy()) {
                Ok(()) => loaded = true,
                Err(e) => {
                    errors.push(anyhow::anyhow!(
                        "loading sort layers {}: {e:#}",
                        path.display()
                    ));
                }
            }
        }
        if !loaded && errors.is_empty() {
            errors.push(anyhow::anyhow!(
                "sort_layers ref '{rel}' not found in any asset root"
            ));
        }
    }

    if assets.get_sort_layers(rel).is_some() {
        assets.set_active_sort_layers(Some(rel.to_string()));
    } else {
        assets.set_active_sort_layers(None);
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_uses_base_plus_z_order() {
        let s = SortLayers::from_layers(vec![
            SortLayer { name: "bg".into(), base_z: -1000 },
            SortLayer { name: "fg".into(), base_z: 1000 },
        ]);
        assert_eq!(s.resolve(Some("bg"), 5), -995);
        assert_eq!(s.resolve(Some("fg"), -7), 993);
    }

    #[test]
    fn unknown_or_missing_layer_falls_back_to_z_order() {
        let s = SortLayers::from_layers(vec![SortLayer {
            name: "ui".into(),
            base_z: 5000,
        }]);
        assert_eq!(s.resolve(None, 42), 42);
        assert_eq!(s.resolve(Some("does_not_exist"), 42), 42);
    }

    #[test]
    fn json_roundtrip_rebuilds_index() {
        let json = r#"{ "layers": [ { "name": "a", "base_z": 10 }, { "name": "b", "base_z": -10 } ] }"#;
        let s = SortLayers::from_json(json).unwrap();
        assert_eq!(s.resolve(Some("a"), 1), 11);
        assert_eq!(s.resolve(Some("b"), 1), -9);
    }

    #[test]
    fn empty_set_is_a_passthrough() {
        let s = SortLayers::default();
        assert_eq!(s.resolve(Some("anything"), 7), 7);
        assert_eq!(s.resolve(None, -3), -3);
    }
}
