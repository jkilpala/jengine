//! 2D Renderer — wgpu-based rendering pipeline + backend-agnostic data types.
//!
//! Phase 8 of the migration cut the legacy `glow` (OpenGL 3.3 / WebGL2) path.
//! What's left in this module:
//!
//! - [`atlas`] / [`material`] / [`postprocess`] / [`text`] / [`lighting`] —
//!   pure data types (regions, blend modes, font glyphs, light data) shared
//!   by the wgpu pipeline and scene serialization.
//! - [`camera`] — `Camera2D` view/projection (no GPU types).
//! - [`shader_codegen`] / [`shader_graph`] — the visual-shader graph format
//!   used by the editor; codegen targets WGSL.
//! - [`backend`] — wgpu device/pipeline implementations under
//!   `backend::wgpu`.

#[cfg(feature = "spline_shape")]
pub mod spline_shape;
#[cfg(feature = "line2d")]
pub mod line2d;
#[cfg(feature = "lut")]
pub mod lut;
pub mod palette;
/// The finished frame copied into a texture or a file (`Screenshot.take`).
pub mod screenshot;
pub mod atlas;
#[cfg(not(target_arch = "wasm32"))]
pub mod atlas_asset;
pub mod backend;
pub mod camera;
pub mod frame_view;
pub mod pixel_perfect;
pub mod mask;
// Not gated to native: the wasm hosts use `insert_font_json` / `resolve_font`
// even though only native has the disk loader.
pub mod font_asset;
#[cfg(feature = "dynamic_font")]
pub mod dynamic_font;
#[cfg(feature = "gpu-wgpu")]
pub mod graph_pipeline_cache;
#[cfg(feature = "freeform_light")]
pub mod freeform_light;
pub mod lighting;
pub mod material;
/// Scene transitions: the old frame wiped away over the new scene.
pub mod transition;
/// The numbers a game hands a shader graph, per material instance.
pub mod material_params;
/// Line breaking and measurement for bitmap text — the layout pass the
/// single-line renderer never had.
pub mod text_layout;
pub mod vector_path;
pub mod postprocess;
pub mod shader_codegen;
pub mod shader_graph;
pub mod text;
/// Which GPU a game asks for, from project settings.
pub mod gpu_preference;
/// Markup in world text, and the typewriter.
pub mod rich_text;
/// Nearest or filtered (with mipmaps), per texture, from project settings.
pub mod texture_filter;
