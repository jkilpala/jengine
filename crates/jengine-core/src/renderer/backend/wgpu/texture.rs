//! [`WgpuTexture`] — wgpu equivalent of [`crate::renderer::texture::Texture`].

use anyhow::Result;
use glam::UVec2;

use crate::renderer::texture_filter::TextureFilter;

/// GPU-resident 2D texture (sampled from sprite shaders).
///
/// Holds the underlying `wgpu::Texture`, a default `TextureView`, and its
/// `Sampler` - nearest-neighbour unless it was built filtered
/// ([`Self::from_rgba_filtered`], which also gives it mipmaps). Each
/// `WgpuTexture` is self-contained so it can be bound into a `BindGroup`
/// without any extra plumbing.
pub struct WgpuTexture {
    pub texture: wgpu::Texture,
    /// Shared so a sprite batch can keep it bound as a mask texture
    /// (`renderer::mask`) after the flush that handed it over.
    pub view:    std::sync::Arc<wgpu::TextureView>,
    pub sampler: std::sync::Arc<wgpu::Sampler>,
    pub width:   u32,
    pub height:  u32,
    /// Unique for the life of the process, so a bind-group cache can key on
    /// the texture rather than on an address that a reload would reuse.
    pub id:      u64,
    /// A signed distance field (the built-in font): every sprite flush of it
    /// draws through the shader's distance branch (`globals.time.z`).
    pub sdf:     bool,
}

impl WgpuTexture {
    /// The next texture id. Every constructor takes one.
    pub fn next_id() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    /// Build a texture from raw RGBA8 pixel data.
    ///
    /// Storage format is `Rgba8UnormSrgb` so sampling produces linear color in
    /// the shader (matching how the GL path samples sRGB textures). That is
    /// right for **colour** and wrong for anything that is not colour - use
    /// [`Self::from_rgba_linear`] for a normal map.
    /// A texture to draw into and then sample - a camera's target
    /// (`ecs::views`). Linear filtering, so a view shown smaller or larger
    /// than it was drawn stays smooth.
    pub fn render_target(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("jengine.view_target"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("jengine.view_target"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { texture, view: view.into(), sampler: sampler.into(), width, height, id: Self::next_id(), sdf: false }
    }

    pub fn from_rgba(
        device: &wgpu::Device,
        queue:  &wgpu::Queue,
        width:  u32,
        height: u32,
        rgba:   &[u8],
    ) -> Result<Self> {
        Self::from_rgba_filtered(device, queue, width, height, rgba, TextureFilter::Nearest, true)
    }

    /// Build a texture sampled the way `filter` says - colour (`srgb`) or
    /// data. [`TextureFilter::Linear`] uploads a full mip chain
    /// (`texture_filter::mip_chain`) and samples it trilinearly;
    /// [`TextureFilter::Nearest`] is one level, point-sampled.
    pub fn from_rgba_filtered(
        device: &wgpu::Device,
        queue:  &wgpu::Queue,
        width:  u32,
        height: u32,
        rgba:   &[u8],
        filter: TextureFilter,
        srgb:   bool,
    ) -> Result<Self> {
        let format = if srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
        Self::from_rgba_format(device, queue, width, height, rgba, format, filter, true)
    }

    /// Build a **signed distance field** (a font atlas): filtered linearly
    /// and with **no mip chain**. A mip level averages the distances under it,
    /// and a thin stem - an `i`, an `l` - is a ridge narrower than a texel of
    /// the levels small text samples: averaged with the background around it
    /// its peak falls under the edge, and the letter thins to nothing (at 0.7
    /// of the built-in face's size `half` drew as `ha f`). Read whole and
    /// smoothed over one screen pixel by the shader (`fwidth`), the field
    /// needs no prefiltering. `small_text_keeps_its_thin_letters`.
    pub fn from_rgba_sdf(
        device: &wgpu::Device,
        queue:  &wgpu::Queue,
        width:  u32,
        height: u32,
        rgba:   &[u8],
    ) -> Result<Self> {
        let mut tex = Self::from_rgba_format(
            device, queue, width, height, rgba, wgpu::TextureFormat::Rgba8UnormSrgb, TextureFilter::Linear, false,
        )?;
        tex.sdf = true;
        Ok(tex)
    }

    /// Build a texture whose bytes are **data, not colour** - a normal map.
    ///
    /// `Rgba8Unorm`, so the shader samples the stored values unchanged. Through
    /// [`Self::from_rgba`] a normal map is gamma-decoded on the way in: a flat
    /// texel `(128, 128, 255)` samples as about `(0.22, 0.22, 1.0)` and decodes
    /// to a normal tilted some 40 degrees down-left, so every "flat" pixel and
    /// every bump leans the same way and the lighting reads as lopsided and
    /// faint. The lit pipeline's own flat-normal fallback was always linear for
    /// exactly this reason; real normal maps went through the colour path.
    pub fn from_rgba_linear(
        device: &wgpu::Device,
        queue:  &wgpu::Queue,
        width:  u32,
        height: u32,
        rgba:   &[u8],
    ) -> Result<Self> {
        Self::from_rgba_filtered(device, queue, width, height, rgba, TextureFilter::Nearest, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn from_rgba_format(
        device: &wgpu::Device,
        queue:  &wgpu::Queue,
        width:  u32,
        height: u32,
        rgba:   &[u8],
        format: wgpu::TextureFormat,
        filter: TextureFilter,
        mipmapped: bool,
    ) -> Result<Self> {
        let expected = (width as usize) * (height as usize) * 4;
        if rgba.len() != expected {
            anyhow::bail!(
                "WgpuTexture::from_rgba: pixel buffer is {} bytes, expected {} for {}x{}",
                rgba.len(), expected, width, height
            );
        }

        let mips = match filter {
            TextureFilter::Nearest => Vec::new(),
            TextureFilter::Linear if !mipmapped => Vec::new(),
            TextureFilter::Linear => {
                crate::renderer::texture_filter::mip_chain(width, height, rgba, format.is_srgb())
            }
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("WgpuTexture"),
            size:            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1 + mips.len() as u32,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            format,
            usage:           wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats:    &[],
        });
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture:   &texture,
                mip_level: 0,
                origin:    wgpu::Origin3d::ZERO,
                aspect:    wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::ImageDataLayout {
                offset:         0,
                bytes_per_row:  Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        for (level, (w, h, bytes)) in mips.iter().enumerate() {
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture:   &texture,
                    mip_level: level as u32 + 1,
                    origin:    wgpu::Origin3d::ZERO,
                    aspect:    wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::ImageDataLayout {
                    offset:         0,
                    bytes_per_row:  Some(w * 4),
                    rows_per_image: Some(*h),
                },
                wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mode = match filter {
            TextureFilter::Nearest => wgpu::FilterMode::Nearest,
            TextureFilter::Linear => wgpu::FilterMode::Linear,
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label:          Some("WgpuTexture sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter:     mode,
            min_filter:     mode,
            mipmap_filter:  mode,
            ..Default::default()
        });

        Ok(Self { texture, view: view.into(), sampler: sampler.into(), width, height, id: Self::next_id(), sdf: false })
    }

    /// Decode a PNG/JPEG/WebP byte buffer and upload it.
    pub fn from_bytes(device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) -> Result<Self> {
        let img = image::load_from_memory(bytes)?.to_rgba8();
        let (w, h) = img.dimensions();
        Self::from_rgba(device, queue, w, h, &img)
    }

    /// Decode an image file from disk (native only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_file(device: &wgpu::Device, queue: &wgpu::Queue, path: &str) -> Result<Self> {
        let img = image::open(path)?.to_rgba8();
        let (w, h) = img.dimensions();
        Self::from_rgba(device, queue, w, h, &img)
    }

    pub fn size(&self) -> UVec2 {
        UVec2::new(self.width, self.height)
    }
}
