//! Runtime backend trait — outer present-loop primitives the runtime calls
//! every frame: clear-color, viewport size, and resize.
//!
//! Phase 8 cut the glow impl. Only the wgpu impl remains, but the trait
//! is kept object-safe so a future second backend can slot in without
//! changing the runtime's `Box<dyn RuntimeBackend>` storage.

use super::BackendKind;

pub trait RuntimeBackend {
    /// Which GPU backend this implementation drives.
    fn backend_kind(&self) -> BackendKind;

    /// Current swapchain / viewport size in pixels.
    fn viewport_size(&self) -> (u32, u32);

    /// Resize the swapchain. Called from the windowing layer on
    /// `WindowEvent::Resized`.
    fn resize(&mut self, width: u32, height: u32);

    /// Configure the per-frame clear color (RGBA, linear 0..=1).
    fn set_clear_color(&mut self, r: f32, g: f32, b: f32, a: f32);
}

#[cfg(feature = "gpu-wgpu")]
impl RuntimeBackend for super::wgpu::WgpuRenderer {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Wgpu
    }

    fn viewport_size(&self) -> (u32, u32) {
        super::wgpu::WgpuRenderer::viewport_size(self)
    }

    fn resize(&mut self, width: u32, height: u32) {
        super::wgpu::WgpuRenderer::resize(self, glam::UVec2::new(width.max(1), height.max(1)));
    }

    fn set_clear_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        super::wgpu::WgpuRenderer::set_clear_color(self, r, g, b, a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_backend_is_dyn_safe() {
        fn _accept(_b: Box<dyn RuntimeBackend>) {}
    }
}
