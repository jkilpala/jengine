//! Cache of wgpu render pipelines built from `.shader` graph assets.
//!
//! Slice 13b-iii: graph-driven materials reference a `.shader` file plus a
//! [`BlendMode`]. The first time the renderer sees a `(path, blend)` pair,
//! it loads the JSON, runs it through [`crate::renderer::shader_codegen`]
//! to produce WGSL, then calls
//! [`WgpuSpriteBatch::build_custom_pipeline`] to compile a pipeline against
//! the same bind-group + vertex layout as the canonical `sprite.wgsl`.
//! Subsequent flushes for that material reuse the cached pipeline.
//!
//! Pipelines are stored as `Arc<wgpu::RenderPipeline>` so flush sites can
//! cheaply clone the handle.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::renderer::backend::wgpu::WgpuSpriteBatch;
use crate::renderer::material::BlendMode;
use crate::renderer::shader_codegen;
use crate::renderer::shader_graph::ShaderGraph;

/// Key for the pipeline cache: `.shader` asset path + blend mode.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphPipelineKey {
    pub shader_graph_path: String,
    pub blend_mode: BlendMode,
}

/// Cache of compiled graph pipelines, keyed by `(shader_path, blend_mode)`
/// **and the colour format** it was built for.
///
/// The format is part of the key because one frame draws the world in two:
/// the main pass into the scene target (`WgpuRenderer::scene_format`, float
/// where available) and the editor's picture-in-picture straight onto the
/// display (`color_format`). A pipeline built for one and handed to a pass
/// in the other is a wgpu validation error.
#[derive(Default)]
pub struct GraphPipelineCache {
    pipelines: HashMap<(GraphPipelineKey, wgpu::TextureFormat), Arc<wgpu::RenderPipeline>>,
    /// `.shader` files that previously failed to compile — recorded so we
    /// don't spam logs every frame. Cleared by [`Self::invalidate`].
    failed: HashMap<GraphPipelineKey, String>,
    /// Graphs (by path) with a `SceneColor` node, noted as they are built.
    scene_graphs: std::collections::HashSet<String>,
}

impl GraphPipelineCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the graph at `graph_path` reads the scene drawn under it - so
    /// its sprites are drawn after the world, over a copy of it. Known once
    /// its pipeline has been built.
    pub fn reads_scene(&self, graph_path: &str) -> bool {
        self.scene_graphs.contains(graph_path)
    }

    /// Look up a pipeline cached for `format`. Returns `None` on cache miss.
    pub fn get(&self, key: &GraphPipelineKey, format: wgpu::TextureFormat) -> Option<&Arc<wgpu::RenderPipeline>> {
        self.pipelines.get(&(key.clone(), format))
    }

    /// Get a pipeline, building it from the `.shader` file on miss. The
    /// graph JSON is read from `<assets_root>/<key.shader_graph_path>` (or
    /// just `key.shader_graph_path` if already absolute). On compile error,
    /// the failure is cached so subsequent calls return the same `Err`
    /// without re-reading the file.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn get_or_build(
        &mut self,
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        batch: &WgpuSpriteBatch,
        assets_root: Option<&std::path::Path>,
        key: &GraphPipelineKey,
    ) -> Result<Arc<wgpu::RenderPipeline>> {
        if let Some(pipeline) = self.pipelines.get(&(key.clone(), color_format)) {
            return Ok(Arc::clone(pipeline));
        }
        if let Some(err) = self.failed.get(key) {
            anyhow::bail!("graph pipeline previously failed: {err}");
        }

        let resolved = resolve_graph_path(assets_root, &key.shader_graph_path);
        let result = (|| -> Result<Arc<wgpu::RenderPipeline>> {
            let json = std::fs::read_to_string(&resolved)
                .with_context(|| format!("read shader graph '{}'", resolved.display()))?;
            let graph: ShaderGraph = serde_json::from_str(&json)
                .with_context(|| format!("parse shader graph '{}'", resolved.display()))?;
            let generated = shader_codegen::generate(&graph)
                .with_context(|| format!("codegen for '{}'", resolved.display()))?;
            crate::renderer::material_params::note_graph(&key.shader_graph_path, graph.param_names());
            let label_owned = format!("graph:{}", key.shader_graph_path);
            let reads_scene = shader_codegen::uses_scene_color(&graph);
            if reads_scene {
                crate::renderer::material_params::note_scene_reader(&key.shader_graph_path);
                self.scene_graphs.insert(key.shader_graph_path.clone());
            }
            Ok(batch.build_graph_pipeline(
                device,
                color_format,
                reads_scene,
                &generated.wgsl,
                key.blend_mode,
                Some(&label_owned),
            ))
        })();

        match result {
            Ok(pipeline) => {
                self.pipelines.insert((key.clone(), color_format), Arc::clone(&pipeline));
                Ok(pipeline)
            }
            Err(e) => {
                let msg = format!("{e:#}");
                log::error!("[GraphPipelineCache] failed to build '{}': {msg}", key.shader_graph_path);
                self.failed.insert(key.clone(), msg.clone());
                Err(e)
            }
        }
    }

    /// Compile and cache a pipeline from an already-parsed graph. Works on
    /// all platforms — the WASM runtime feeds in pre-fetched graphs this way.
    pub fn insert_from_graph(
        &mut self,
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        batch: &WgpuSpriteBatch,
        key: GraphPipelineKey,
        graph: &ShaderGraph,
    ) -> Result<Arc<wgpu::RenderPipeline>> {
        let generated = shader_codegen::generate(graph)
            .with_context(|| format!("codegen for '{}'", key.shader_graph_path))?;
        crate::renderer::material_params::note_graph(&key.shader_graph_path, graph.param_names());
        let label = format!("graph:{}", key.shader_graph_path);
        let reads_scene = shader_codegen::uses_scene_color(graph);
        if reads_scene {
            crate::renderer::material_params::note_scene_reader(&key.shader_graph_path);
            self.scene_graphs.insert(key.shader_graph_path.clone());
        }
        let pipeline = batch.build_graph_pipeline(device, color_format, reads_scene, &generated.wgsl, key.blend_mode, Some(&label));
        self.pipelines.insert((key, color_format), Arc::clone(&pipeline));
        Ok(pipeline)
    }

    /// Drop the cached pipeline (and any cached failure) for a key. Used
    /// when a `.shader` asset is edited and needs recompilation.
    pub fn invalidate(&mut self, key: &GraphPipelineKey) {
        self.pipelines.retain(|(k, _), _| k != key);
        self.failed.remove(key);
    }

    /// Drop everything. Used on device loss / context recreation.
    pub fn clear(&mut self) {
        self.pipelines.clear();
        self.failed.clear();
    }

    pub fn len(&self) -> usize {
        self.pipelines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pipelines.is_empty()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_graph_path(assets_root: Option<&std::path::Path>, path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    if let Some(root) = assets_root
        && let Some(found) = crate::asset::find_in_roots(path, &[root.to_path_buf()])
    {
        return found;
    }
    p.to_path_buf()
}

/// Whether material `mid` draws with a graph that reads the scene
/// (`NodeType::SceneColor`) - which every runtime skips in the world pass and
/// draws in a pass of its own after copying the scene. Ask after the
/// material's pipeline has been built (the cache learns it then).
pub fn material_reads_scene(assets: &crate::asset::AssetManager, cache: &GraphPipelineCache, mid: &str) -> bool {
    assets
        .get_material(mid)
        .and_then(|m| m.shader_graph_path.as_deref())
        .is_some_and(|path| cache.reads_scene(path))
}
