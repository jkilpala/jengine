//! GPU backend façade — wgpu only after Phase 8.
//!
//! The wgpu submodule lives under [`wgpu`] and is gated behind the
//! `gpu-wgpu` Cargo feature. The [`traits`] module retains the
//! object-safe device/pipeline trait surface so future backends (e.g.
//! a Vulkan-direct path) can plug in without churning consumers.

pub mod traits;
pub use traits::{
    BufferKind, GpuBuffer, GpuDevice, GpuPipeline, GpuShader, GpuTexture, RenderPass,
};

pub mod runtime;
pub use runtime::RuntimeBackend;

#[cfg(feature = "gpu-wgpu")]
pub mod wgpu;

/// Identifier for the GPU backend that produced a given resource. Only
/// `Wgpu` exists today, but the enum is kept so call sites that want to
/// log/branch on backend identity keep compiling if a second backend
/// (e.g. native Vulkan) is added later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// `wgpu` (Vulkan / Metal / DX12 / WebGPU).
    Wgpu,
}

/// Returns the backend that the engine was *compiled* to prefer. Always
/// `Wgpu` after the Phase 8 glow cut.
pub const fn active_backend() -> BackendKind {
    BackendKind::Wgpu
}
