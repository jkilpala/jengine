//! Every `.shader` a project ships compiles to a pipeline.
//!
//! A graph that generates invalid WGSL is not an error anyone sees: the
//! sprite draws with the default shader, or a validation message scrolls past
//! in the log, and the effect is simply not there. This builds every shipped
//! graph through the real `GraphPipelineCache` on a headless device inside a
//! validation error scope. The behaviour-graph twin is
//! `every_shipped_graph_compiles`. Without a GPU adapter it prints why and
//! passes.
#![cfg(feature = "gpu-wgpu")]

use std::path::{Path, PathBuf};

use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::graph_pipeline_cache::{GraphPipelineCache, GraphPipelineKey};
use jengine_core::renderer::material::BlendMode;
use jengine_core::renderer::shader_graph::ShaderGraph;

fn shader_files() -> Vec<PathBuf> {
    let projects = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects");
    let mut out = Vec::new();
    for project in std::fs::read_dir(&projects).expect("projects dir").flatten() {
        let dir = project.path().join("assets/shaders");
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            if e.path().extension().is_some_and(|x| x == "shader") {
                out.push(e.path());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn every_shipped_shader_graph_builds_a_pipeline() {
    let files = shader_files();
    assert!(files.len() >= 5, "found only {} shader graphs - is the walk broken?", files.len());
    let Some(gpu) = HeadlessGpu::new(8, 8) else {
        eprintln!("shipped_shader_graphs_compile: no GPU adapter - skipped");
        return;
    };
    let batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut cache = GraphPipelineCache::new();
    let mut failures = Vec::new();
    for path in &files {
        let name = path.display().to_string();
        let json = std::fs::read_to_string(path).expect("read");
        let graph: ShaderGraph = match serde_json::from_str(&json) {
            Ok(g) => g,
            Err(e) => {
                failures.push(format!("{name}: does not parse: {e}"));
                continue;
            }
        };
        gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let built = cache.insert_from_graph(
            &gpu.device,
            gpu.color_format,
            &batch,
            GraphPipelineKey { shader_graph_path: name.clone(), blend_mode: BlendMode::Alpha },
            &graph,
        );
        let error = pollster::block_on(gpu.device.pop_error_scope());
        match (built, error) {
            (Err(e), _) => failures.push(format!("{name}: {e:#}")),
            (_, Some(e)) => failures.push(format!("{name}: the generated WGSL does not validate: {e}")),
            _ => {}
        }
    }
    assert!(failures.is_empty(), "shader graphs that would draw with the default shader:\n{}", failures.join("\n"));
}
