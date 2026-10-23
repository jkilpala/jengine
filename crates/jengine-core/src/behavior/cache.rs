//! Compiled-graph cache. Maps `.behavior` asset paths to interned
//! [`GraphId`]s and lazily compiles + caches the typed
//! [`CompiledGraph`] form.
//!
//! Slice 7.2: cache is in-memory and never invalidates. Hot-reload
//! (last-good-graph fallback per spec §4.8) lands when the editor
//! gains a watcher; for now, the runtime treats compiled graphs as
//! immutable for the lifetime of a scene load.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;

use crate::behavior::compile::{compile, CompileError, CompiledGraph};
use crate::behavior::format::{BehaviorAsset, BehaviorFormatError};

/// Opaque interned id for a compiled graph. Indexes into
/// [`BehaviorGraphCache::graphs`] internally; consumers treat it as a
/// transparent handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GraphId(pub u32);

#[derive(Debug, Default)]
pub struct BehaviorGraphCache {
    by_path: HashMap<PathBuf, GraphId>,
    graphs: Vec<Arc<CompiledGraph>>,
}

impl BehaviorGraphCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.graphs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.graphs.is_empty()
    }

    /// Read + parse + compile a `.behavior` file from disk. If the
    /// path is already cached, returns the existing id.
    pub fn load_from_path(&mut self, path: &Path) -> Result<GraphId, BehaviorCacheError> {
        if let Some(id) = self.by_path.get(path) {
            return Ok(*id);
        }
        let json = std::fs::read_to_string(path).map_err(|e| BehaviorCacheError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        let id = self.load_from_json_with_key(path.to_path_buf(), &json)?;
        Ok(id)
    }

    /// Compile `json` and intern it under `key`. The key need not be
    /// a real filesystem path — tests use synthetic keys to avoid
    /// touching disk.
    pub fn load_from_json_with_key(
        &mut self,
        key: PathBuf,
        json: &str,
    ) -> Result<GraphId, BehaviorCacheError> {
        if let Some(id) = self.by_path.get(&key) {
            return Ok(*id);
        }
        let asset = BehaviorAsset::from_json(json).map_err(BehaviorCacheError::Format)?;
        let graph = compile(&asset).map_err(BehaviorCacheError::Compile)?;
        let id = GraphId(self.graphs.len() as u32);
        self.graphs.push(Arc::new(graph));
        self.by_path.insert(key, id);
        Ok(id)
    }

    pub fn get(&self, id: GraphId) -> &Arc<CompiledGraph> {
        &self.graphs[id.0 as usize]
    }
}

#[derive(Debug, Error)]
pub enum BehaviorCacheError {
    #[error("failed to read `.behavior` from {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    Format(#[from] BehaviorFormatError),

    #[error(transparent)]
    Compile(#[from] CompileError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caches_by_path() {
        let mut cache = BehaviorGraphCache::new();
        let json = r#"{ "name": "T", "version": 1,
            "nodes": [{ "id": "n0", "op": "OnTick" }], "edges": [] }"#;
        let a = cache
            .load_from_json_with_key(PathBuf::from("a.behavior"), json)
            .expect("load");
        let b = cache
            .load_from_json_with_key(PathBuf::from("a.behavior"), json)
            .expect("load");
        assert_eq!(a, b);
        assert_eq!(cache.len(), 1);
    }
}
