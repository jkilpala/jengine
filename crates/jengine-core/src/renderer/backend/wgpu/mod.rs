//! `wgpu`-based GPU backend.
//!
//! Phase 1 (scaffolding) introduced [`create_instance`] / [`describe_adapter`].
//! Phase 3 (this module's children) adds the actual rendering primitives —
//! [`WgpuRenderer`], [`WgpuTexture`], [`WgpuSpriteBatch`] — which mirror the
//! existing `glow`-based [`Renderer`](crate::renderer::Renderer) /
//! [`Texture`](crate::renderer::texture::Texture) /
//! [`SpriteBatch`](crate::renderer::sprite::SpriteBatch) APIs. These run in
//! parallel to the `glow` path; the runtime swap happens in Phase 3.b.
//!
//! All types here are `#[cfg(feature = "gpu-wgpu")]` so the legacy build
//! continues to work unchanged.

/// An offscreen device and colour target: rendering with no window, so a test
/// can look at the pixels the renderer actually produces.
pub mod headless;
mod renderer;
mod shape;
mod sprite;
mod texture;
/// Mipmaps made on the GPU (`MipGenerator`).
pub mod mipmap;
mod framebuffer;
mod postprocess;
mod lighting;
/// 2D global illumination by radiance cascades (`GlobalIllumination`).
#[cfg(feature = "gi")]
pub mod gi;
mod lit_sprite;
mod gpu_particles;
mod transition;

pub use renderer::{WgpuFrame, WgpuRenderer, is_hdr_format, pick_scene_format};
pub use shape::WgpuShapeBatch;
pub use sprite::WgpuSpriteBatch;
pub use texture::WgpuTexture;
pub use mipmap::MipGenerator;
pub use framebuffer::WgpuRenderTarget;
pub use transition::WgpuTransition;
pub use postprocess::{VELOCITY_FORMAT, WgpuPostProcessStack};
pub use lighting::WgpuLightingPass;
pub use lit_sprite::{
    WgpuLitSpritePipeline, LitSpriteVertex, LitSpriteGlobals, LitSpriteLight,
    MAX_LIT_SPRITE_LIGHTS, NORMAL_BUFFER_FORMAT,
};

use wgpu::{Backends, Instance, InstanceDescriptor};

/// Build a `wgpu::Instance` with reasonable defaults for the current target.
///
/// - **Native**: prefer `PRIMARY` (Vulkan / Metal / DX12) and silently fall
///   back to `SECONDARY` (DX11 / GL) if no primary adapter exists.
/// - **Web**: `BROWSER_WEBGPU` first, then the `GL` (WebGL2) fallback that
///   `wgpu` ships out of the box.
///
/// On native, `WGPU_BACKEND` (`gl`, `vulkan`, `dx12`, a comma list) narrows
/// the choice - how a fallback is tested on a machine whose primary backend
/// works. Android's GLES is that fallback, compiled into every Android build
/// by wgpu itself; a debug Android build sets the variable from its launch
/// (`jengine android-smoke --backend gl`).
///
/// The fallback is created **only when no primary adapter exists** (Oct
/// 2026). Asking for both at once starts OpenGL's EGL every time, and on a
/// Linux machine whose EGL is incomplete wgpu panics there - the game died
/// before trying the Vulkan driver it had (found by running under WSL). And
/// should OpenGL panic when it *is* the only hope, that is a logged error,
/// not a crash.
pub fn create_instance() -> Instance {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let make = |backends: Backends| Instance::new(InstanceDescriptor { backends, ..Default::default() });
        if let Some(b) = wgpu::util::backend_bits_from_env().filter(|b| !b.is_empty()) {
            log::info!("WGPU_BACKEND narrows the graphics backends to {b:?}");
            return make(b);
        }
        let primary = make(Backends::PRIMARY);
        if !primary.enumerate_adapters(Backends::PRIMARY).is_empty() {
            return primary;
        }
        log::warn!("no Vulkan / Metal / DX12 adapter: trying the fallback (OpenGL, DX11)");
        match std::panic::catch_unwind(|| make(Backends::SECONDARY)) {
            Ok(instance) => instance,
            Err(_) => {
                log::error!("the fallback graphics API failed to start too; no adapter will be found");
                primary
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    Instance::new(InstanceDescriptor {
        backends: Backends::BROWSER_WEBGPU | Backends::GL,
        ..Default::default()
    })
}

/// Human-readable summary of the backend that an [`wgpu::Adapter`] resolved
/// to. Useful for the runtime/editor startup banner so we can tell at a glance
/// whether we're on Vulkan, Metal, DX12, GL or WebGPU.
pub fn describe_adapter(adapter: &wgpu::Adapter) -> String {
    let info = adapter.get_info();
    format!(
        "wgpu adapter: {} ({:?}, backend={:?})",
        info.name, info.device_type, info.backend
    )
}
