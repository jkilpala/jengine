//! Step 2D — shared `Path2D` asset for `PatrolPathRef`.
//!
//! A `Path2D` is just a list of waypoints. Many patrol entities can share
//! one path without each carrying a `Vec<Vec2>` clone. The library is a
//! plain `HashMap<String, Path2D>` for now — when the editor grows real
//! asset IDs we'll replace the `String` key without touching component
//! definitions.

use std::collections::HashMap;

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// A polyline of world-space waypoints.
///
/// `looped = true` is informational — the patrol system reads
/// `PatrolPathRef.mode` to decide what to do at the end. Storing it on
/// the asset itself lets editors render the path correctly without
/// guessing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Path2D {
    pub points: Vec<[f32; 2]>,
    #[serde(default)]
    pub looped: bool,
}

impl Path2D {
    pub fn new(points: impl IntoIterator<Item = Vec2>) -> Self {
        Self {
            points: points.into_iter().map(|p| [p.x, p.y]).collect(),
            looped: false,
        }
    }

    pub fn point(&self, i: usize) -> Option<Vec2> {
        self.points.get(i).map(|p| Vec2::new(p[0], p[1]))
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// Registry of named `Path2D` assets. Pass `&Path2DLibrary` to the patrol
/// system; if no library is available, patrol entities are left untouched
/// (they keep their last position, no panic).
#[derive(Debug, Clone, Default)]
pub struct Path2DLibrary {
    paths: HashMap<String, Path2D>,
}

impl Path2DLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: impl Into<String>, path: Path2D) {
        self.paths.insert(id.into(), path);
    }

    pub fn get(&self, id: &str) -> Option<&Path2D> {
        self.paths.get(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<Path2D> {
        self.paths.remove(id)
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}
