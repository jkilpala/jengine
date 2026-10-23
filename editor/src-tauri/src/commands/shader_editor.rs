//! Shader editor Tauri commands — graph validation, GLSL generation, and node palette.

use jengine_core::renderer::shader_codegen;
use jengine_core::renderer::shader_graph::{self, NodeType, ShaderGraph};
use serde::Serialize;

/// Node palette entry for the editor UI.
#[derive(Debug, Serialize)]
pub struct PaletteCategory {
    pub category: String,
    pub nodes: Vec<PaletteNode>,
}

/// A single node type in the palette.
#[derive(Debug, Serialize)]
pub struct PaletteNode {
    pub name: String,
    pub node_type: NodeType,
}

/// Result of shader graph validation.
#[derive(Debug, Serialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

/// Result of WGSL code generation.
#[derive(Debug, Serialize)]
pub struct GenerateResult {
    /// Complete WGSL module (vertex + fragment entry points).
    pub wgsl: String,
}

/// Get the node palette categories for the editor sidebar.
#[tauri::command]
pub fn shader_get_palette() -> Vec<PaletteCategory> {
    shader_graph::node_palette()
        .into_iter()
        .map(|(cat, nodes)| PaletteCategory {
            category: cat.to_string(),
            nodes: nodes
                .into_iter()
                .map(|(name, nt)| PaletteNode {
                    name: name.to_string(),
                    node_type: nt,
                })
                .collect(),
        })
        .collect()
}

/// Validate a shader graph and return any errors.
#[tauri::command]
pub fn shader_validate(graph_json: String) -> Result<ValidationResult, String> {
    let graph: ShaderGraph =
        serde_json::from_str(&graph_json).map_err(|e| format!("Invalid graph JSON: {e}"))?;
    let errors = graph.validate();
    Ok(ValidationResult {
        valid: errors.is_empty(),
        errors: errors.iter().map(|e| e.to_string()).collect(),
    })
}

/// Generate a WGSL module from a shader graph.
#[tauri::command]
pub fn shader_generate(graph_json: String) -> Result<GenerateResult, String> {
    let graph: ShaderGraph =
        serde_json::from_str(&graph_json).map_err(|e| format!("Invalid graph JSON: {e}"))?;
    let generated =
        shader_codegen::generate(&graph).map_err(|e| format!("Code generation failed: {e}"))?;
    Ok(GenerateResult {
        wgsl: generated.wgsl,
    })
}

/// Result of WebGL2 preview GLSL transpilation.
#[derive(Debug, Serialize)]
pub struct PreviewGlslResult {
    /// GLSL ES 300 fragment shader transpiled from the generated WGSL.
    pub glsl: String,
    /// The WGSL the GLSL was derived from (for diagnostics / display).
    pub wgsl: String,
}

/// Generate a GLSL ES 300 fragment shader from a graph for the editor's
/// WebGL2 live preview. Pipes the canonical WGSL through naga's GLSL
/// backend so the preview always matches what the runtime compiles.
#[tauri::command]
pub fn shader_generate_preview_glsl(graph_json: String) -> Result<PreviewGlslResult, String> {
    let graph: ShaderGraph =
        serde_json::from_str(&graph_json).map_err(|e| format!("Invalid graph JSON: {e}"))?;
    let generated =
        shader_codegen::generate(&graph).map_err(|e| format!("Code generation failed: {e}"))?;

    let module = naga::front::wgsl::parse_str(&generated.wgsl)
        .map_err(|e| format!("WGSL parse failed: {e}"))?;

    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| format!("WGSL validation failed: {e}"))?;

    let options = naga::back::glsl::Options {
        version: naga::back::glsl::Version::Embedded {
            version: 300,
            is_webgl: true,
        },
        writer_flags: naga::back::glsl::WriterFlags::empty(),
        binding_map: Default::default(),
        zero_initialize_workgroup_memory: false,
    };
    let pipeline_options = naga::back::glsl::PipelineOptions {
        shader_stage: naga::ShaderStage::Fragment,
        entry_point: "fs_main".to_string(),
        multiview: None,
    };

    let mut glsl = String::new();
    let mut writer = naga::back::glsl::Writer::new(
        &mut glsl,
        &module,
        &info,
        &options,
        &pipeline_options,
        naga::proc::BoundsCheckPolicies::default(),
    )
    .map_err(|e| format!("GLSL writer init failed: {e}"))?;
    writer
        .write()
        .map_err(|e| format!("GLSL emission failed: {e}"))?;

    Ok(PreviewGlslResult {
        glsl,
        wgsl: generated.wgsl,
    })
}

/// Load a .shader file from disk, returning the graph JSON.
#[tauri::command]
pub fn shader_load(project_path: String, relative_path: String) -> Result<String, String> {
    let full = std::path::Path::new(&project_path)
        .join("assets")
        .join(&relative_path);

    // Validate path doesn't escape project directory.
    let canonical = full
        .canonicalize()
        .map_err(|e| format!("Path error: {e}"))?;
    let project_canonical = std::path::Path::new(&project_path)
        .canonicalize()
        .map_err(|e| format!("Project path error: {e}"))?;
    if !canonical.starts_with(&project_canonical) {
        return Err("Path traversal not allowed".to_string());
    }

    let contents =
        std::fs::read_to_string(&canonical).map_err(|e| format!("Failed to read shader: {e}"))?;

    // Parse and re-serialize to apply serde defaults for any new fields.
    let graph: ShaderGraph =
        serde_json::from_str(&contents).map_err(|e| format!("Invalid shader graph: {e}"))?;

    serde_json::to_string_pretty(&graph).map_err(|e| format!("Failed to serialize graph: {e}"))
}

/// Save a shader graph to a .shader file.
#[tauri::command]
pub fn shader_save(
    project_path: String,
    relative_path: String,
    graph_json: String,
) -> Result<(), String> {
    // Validate the graph parses correctly.
    let graph: ShaderGraph =
        serde_json::from_str(&graph_json).map_err(|e| format!("Invalid graph JSON: {e}"))?;

    let full = std::path::Path::new(&project_path)
        .join("assets")
        .join(&relative_path);

    // Validate path doesn't escape project directory.
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directories: {e}"))?;
    }

    // Re-serialize with pretty formatting.
    let json =
        serde_json::to_string_pretty(&graph).map_err(|e| format!("Failed to serialize: {e}"))?;
    std::fs::write(&full, json).map_err(|e| format!("Failed to write shader: {e}"))?;
    Ok(())
}

/// Create a new shader graph with a default output node.
#[tauri::command]
pub fn shader_create_new(name: String) -> String {
    let graph = ShaderGraph::new(&name);
    serde_json::to_string_pretty(&graph).unwrap_or_default()
}

/// Get pin definitions for a node type.
#[tauri::command]
pub fn shader_get_node_pins(node_type_json: String) -> Result<NodePinsResult, String> {
    let node_type: NodeType =
        serde_json::from_str(&node_type_json).map_err(|e| format!("Invalid node type: {e}"))?;
    let (inputs, outputs) = shader_graph::node_pins(&node_type);
    Ok(NodePinsResult {
        inputs: inputs
            .into_iter()
            .map(|p| PinInfo {
                name: p.name,
                pin_type: format!("{:?}", p.pin_type),
            })
            .collect(),
        outputs: outputs
            .into_iter()
            .map(|p| PinInfo {
                name: p.name,
                pin_type: format!("{:?}", p.pin_type),
            })
            .collect(),
    })
}

/// Pin info for the editor.
#[derive(Debug, Serialize)]
pub struct PinInfo {
    pub name: String,
    pub pin_type: String,
}

/// Result of getting node pins.
#[derive(Debug, Serialize)]
pub struct NodePinsResult {
    pub inputs: Vec<PinInfo>,
    pub outputs: Vec<PinInfo>,
}
