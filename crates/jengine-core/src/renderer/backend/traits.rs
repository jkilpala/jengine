//! Backend-neutral GPU resource trait surface.
//!
//! These traits exist so the renderer can be progressively migrated off the
//! direct `glow::Context` API. Phase 1 of the wgpu migration introduces the
//! trait *surface* only — the renderer modules and `ecs/systems.rs` still use
//! `glow::Context` directly. Phase 3 swaps consumers over to `&dyn GpuDevice`
//! once the wgpu PoC (Phase 2) has proven the shape.
//!
//! Design notes
//! -----------
//! - All GPU handles (`GpuBuffer`, `GpuTexture`, `GpuShader`, `GpuPipeline`)
//!   are opaque: callers never pull the underlying GL/wgpu type out. This is
//!   what lets us remove `glow` entirely in Phase 7 without touching ECS code.
//! - Resources are RAII (`Drop` releases). The legacy `destroy(&self, gl)`
//!   pattern goes away in the same step that consumers move onto these traits.
//! - We use trait objects (`Box<dyn GpuTexture>`) over generics to keep the
//!   blast radius small — every renderer function would otherwise need a
//!   `<B: GpuDevice>` parameter. Per-frame draw calls live behind the
//!   `RenderPass` which keeps allocations off the hot path.
//! - `wgpu`'s async device acquisition is hidden behind `pollster::block_on`
//!   inside the implementation; the trait stays sync.
//!
//! See `/memories/session/plan.md` for the full migration plan.

use glam::UVec2;

/// Top-level GPU device. One instance per window/surface. Replaces the
/// `gl: &glow::Context` parameter that currently threads through the renderer.
pub trait GpuDevice {
    /// Which backend this device is implemented on. Useful for logging and
    /// for the rare site that needs backend-specific behavior.
    fn kind(&self) -> super::BackendKind;

    /// Pixel size of the swapchain / default framebuffer.
    fn surface_size(&self) -> UVec2;

    /// Resize the swapchain. Called from the windowing layer on resize events.
    fn resize(&mut self, new_size: UVec2);
}

/// Opaque GPU buffer — vertex, index, or uniform/storage. Backend chooses the
/// concrete usage based on the [`BufferKind`] passed at creation time.
pub trait GpuBuffer {
    /// Total byte length of the buffer.
    fn size(&self) -> usize;
}

/// What the buffer will be used for. Drives backend-specific usage flags
/// (`GL_ARRAY_BUFFER` vs `wgpu::BufferUsages::VERTEX`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferKind {
    Vertex,
    Index,
    Uniform,
    Storage,
}

/// Opaque GPU texture (2D, sampled). Backed by either a `glow` texture name or
/// a `wgpu::Texture` + `wgpu::TextureView`.
pub trait GpuTexture {
    fn size(&self) -> UVec2;
}

/// Opaque shader program / pipeline module. On `glow` this wraps a linked
/// program; on `wgpu` it wraps a `ShaderModule` (the full `RenderPipeline` is
/// built on demand keyed by `(shader, blend_mode, color_format)`).
pub trait GpuShader {}

/// Opaque, ready-to-bind render pipeline. wgpu requires the full pipeline
/// state up front (shader + vertex layout + blend + depth + target format).
/// On `glow` this is essentially a tagged shader + cached GL state setter.
pub trait GpuPipeline {}

/// One frame's render pass. Acquired from [`GpuDevice`] at the start of a
/// frame and dropped at the end (which submits on the wgpu backend). Mirrors
/// `wgpu::RenderPass`. The `glow` impl is a thin state-machine wrapper.
pub trait RenderPass {
    fn set_pipeline(&mut self, pipeline: &dyn GpuPipeline);
    fn set_vertex_buffer(&mut self, slot: u32, buffer: &dyn GpuBuffer);
    fn set_index_buffer(&mut self, buffer: &dyn GpuBuffer);
    fn set_texture(&mut self, slot: u32, texture: &dyn GpuTexture);
    fn draw_indexed(&mut self, index_count: u32, instance_count: u32);
}
