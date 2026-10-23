//! [`WgpuRenderTarget`] — wgpu equivalent of
//! [`crate::renderer::framebuffer::RenderTarget`].
//!
//! Phase 3.c-4 Step 6a: foundational primitive for both lighting and
//! postprocess. Owns a single-color-attachment offscreen texture that
//! can be:
//! - Used as a render target via [`WgpuRenderTarget::begin_pass`] (or
//!   manually via the public `view` field), then
//! - Sampled in a subsequent pass via the public `view` + `sampler`
//!   pair (e.g. fed into a postprocess fullscreen quad).
//!
//! Like the gl `RenderTarget`, there is no depth attachment — 2D only.
//! The format is caller-chosen (typically `WgpuRenderer::color_format`)
//! so the postprocess output can be blitted directly into the
//! swapchain without a format-conversion pass.
//!
//! **Lifetime note vs gl path:** the gl `RenderTarget::bind` mutates
//! global GL state (binds an FBO + sets viewport). wgpu has no global
//! state — render targets are scoped to a `RenderPass`, which itself is
//! scoped to the encoder's lifetime. Callers that want the gl-style
//! "bind, draw, unbind" feel should use [`WgpuRenderTarget::begin_pass`].
//! Callers that need finer control (multiple attachments, custom load
//! ops) can build the `RenderPassDescriptor` themselves using the
//! public `view` field.

use anyhow::Result;
use wgpu::TextureFormat;

/// Off-screen render target with a single color attachment.
pub struct WgpuRenderTarget {
    pub texture: wgpu::Texture,
    pub view:    wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub width:   u32,
    pub height:  u32,
    pub format:  TextureFormat,
}

impl WgpuRenderTarget {
    /// Create a new render target with the given dimensions and format.
    ///
    /// `format` should typically be `WgpuRenderer::color_format` so the
    /// postprocess output can be blitted directly into the swapchain
    /// without a format-conversion pass.
    pub fn new(
        device: &wgpu::Device,
        width:  u32,
        height: u32,
        format: TextureFormat,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            anyhow::bail!(
                "WgpuRenderTarget::new: zero dimensions ({}x{})",
                width, height
            );
        }
        let (texture, view, sampler) = Self::create_resources(device, width, height, format);
        Ok(Self { texture, view, sampler, width, height, format })
    }

    /// Begin a render pass that targets this render target.
    ///
    /// Convenience wrapper around `encoder.begin_render_pass(...)`.
    /// Pass `Some(color)` to clear, `None` to load existing contents.
    pub fn begin_pass<'a>(
        &'a self,
        encoder: &'a mut wgpu::CommandEncoder,
        clear:   Option<wgpu::Color>,
    ) -> wgpu::RenderPass<'a> {
        let load = match clear {
            Some(c) => wgpu::LoadOp::Clear(c),
            None    => wgpu::LoadOp::Load,
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("WgpuRenderTarget pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// Resize the target, recreating the underlying GPU texture in place.
    /// No-op if the dimensions are unchanged.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width == 0 || height == 0 || (width == self.width && height == self.height) {
            return;
        }
        self.width  = width;
        self.height = height;
        let (texture, view, sampler) =
            Self::create_resources(device, width, height, self.format);
        // Drop order: old `texture` / `view` / `sampler` are dropped
        // when reassigned. wgpu refcounts internally so any pass still
        // holding a borrow keeps them alive until that pass ends.
        self.texture = texture;
        self.view    = view;
        self.sampler = sampler;
    }

    fn create_resources(
        device: &wgpu::Device,
        width:  u32,
        height: u32,
        format: TextureFormat,
    ) -> (wgpu::Texture, wgpu::TextureView, wgpu::Sampler) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("WgpuRenderTarget"),
            size:            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            format,
            usage:           wgpu::TextureUsages::RENDER_ATTACHMENT
                           | wgpu::TextureUsages::TEXTURE_BINDING
                           | wgpu::TextureUsages::COPY_SRC,
            view_formats:    &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Linear sampling is what postprocess wants (smooth blur,
        // bloom upsample, etc.). The gl `RenderTarget` also uses
        // GL_LINEAR for both min/mag — match that.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label:          Some("WgpuRenderTarget sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter:     wgpu::FilterMode::Linear,
            min_filter:     wgpu::FilterMode::Linear,
            mipmap_filter:  wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        (texture, view, sampler)
    }
}
