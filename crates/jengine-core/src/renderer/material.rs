//! Material data — serializable blend modes + uniform values used by
//! `MaterialComponent` and `.material` asset files.
//!
//! The wgpu render pipeline reads `MaterialComponent.material_id` directly;
//! no GPU-bound `Material` / `MaterialLibrary` types live here anymore.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Blend mode for sprite rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[derive(Default)]
pub enum BlendMode {
    /// Standard alpha blending: `src * srcAlpha + dst * (1 - srcAlpha)`.
    #[default]
    Alpha,
    /// Additive blending: `src * srcAlpha + dst`. Good for fire, glow, particles.
    Additive,
    /// Multiply blending: `src * dst`. Good for shadows, darkening overlays.
    Multiply,
    /// Screen blending: `src + dst - src * dst`. Good for lightening effects.
    Screen,
    /// Pre-multiplied alpha: `src + dst * (1 - srcAlpha)`.
    PremultipliedAlpha,
}


/// A dynamic uniform value that can be set on a material.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UniformValue {
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Int(i32),
}

/// Serializable material definition — references a shader by name and stores
/// blend mode + uniform overrides. Saved as `.material` asset files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialData {
    /// Name of the shader to use (e.g. "default_sprite", "additive").
    pub shader: String,
    /// Optional path to a `.shader` graph asset. When `Some`, the renderer
    /// builds a custom wgpu pipeline by running this graph through
    /// `shader_codegen` (see Slice 13b). Path is interpreted relative to
    /// the project's assets root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shader_graph_path: Option<String>,
    /// Blend mode override.
    #[serde(default)]
    pub blend_mode: BlendMode,
    /// Custom uniform values to set when rendering with this material.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub uniforms: HashMap<String, UniformValue>,
}

impl Default for MaterialData {
    fn default() -> Self {
        Self {
            shader: "default_sprite".to_string(),
            shader_graph_path: None,
            blend_mode: BlendMode::Alpha,
            uniforms: HashMap::new(),
        }
    }
}

/// Material ids that name no `.material` file: the default sprite pipeline,
/// the blend names `ecs::wgpu_systems::sprite_blend` reads, and `lit_sprite`
/// (a normal-mapped sprite, drawn by the default pipeline since normal maps
/// went deferred). A runtime that loads materials on demand skips these
/// rather than requesting a file that does not exist.
pub fn is_builtin_material(id: &str) -> bool {
    matches!(id, "" | "default" | "default_sprite" | "additive" | "multiply" | "screen" | "lit_sprite" | "unlit")
}
