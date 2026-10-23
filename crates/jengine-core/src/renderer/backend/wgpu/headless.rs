//! An offscreen wgpu device and colour target: rendering without a window.
//!
//! [`WgpuRenderer`](super::renderer::WgpuRenderer) is built from a surface - a
//! winit window natively, a canvas on the web - so nothing could draw a frame
//! outside a running host. That left the renderer untested: every check in this
//! repo works on the data *going in* (sort keys, uv rects, layout rects) and
//! nothing looks at the pixels coming out.
//!
//! The bugs that cost the most in Sep 2026 were all invisible to input-side
//! checks: a `uv_rect` in the wrong layout drew a smeared slice, an atlas whose
//! image did not match its metadata drew blanks, entity-UI widgets drew nothing
//! at all. Each was found by a person looking at the screen.
//!
//! This is the missing half: render into a texture, read the pixels back,
//! compare them against a known-good image.
//!
//! ```no_run
//! # use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
//! let Some(gpu) = HeadlessGpu::new(64, 64) else {
//!     return; // no adapter here - see `new`
//! };
//! let mut encoder = gpu.encoder();
//! {
//!     let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
//!     // draw through any batch built with `gpu.color_format`
//! }
//! gpu.submit(encoder);
//! let image = gpu.read_rgba();
//! ```

use anyhow::{Context, Result};

use crate::asset::RawTextureData;

/// The adapter the first headless device of this process was made on.
static ADAPTER: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The name of the adapter headless images are drawn on in this process
/// (`None` before the first [`HeadlessGpu::new`]) - which a golden image's
/// comparison needs, since two correct GPUs differ at edges.
pub fn adapter_name() -> Option<&'static str> {
    ADAPTER.get().map(String::as_str)
}

/// A device, a queue, and one colour texture to draw into.
pub struct HeadlessGpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// The format the target was created with. Build batches with this.
    pub color_format: wgpu::TextureFormat,
    pub width: u32,
    pub height: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// The adapter the images are drawn on.
    pub adapter: wgpu::AdapterInfo,
}

impl HeadlessGpu {
    /// Acquire an offscreen device, or `None` when the machine has no usable
    /// adapter.
    ///
    /// `None` rather than an error because that is the normal state of a
    /// headless CI box or a sandbox, and a test that cannot run there should
    /// say so and pass rather than fail for the wrong reason. Callers are
    /// expected to print a line explaining the skip.
    pub fn new(width: u32, height: u32) -> Option<Self> {
        Self::try_new(width, height)
            .map_err(|e| {
                log::warn!("[headless] no offscreen GPU: {e:#}");
                e
            })
            .ok()
    }

    fn try_new(width: u32, height: u32) -> Result<Self> {
        // wgpu's own variables pick the device - `WGPU_ADAPTER_NAME` (a part
        // of the name: `4070`, `radeon`), `WGPU_BACKEND` (`dx12`, `vulkan`),
        // `WGPU_POWER_PREF` (`high`, `low`) - and the choice is said once a
        // process, because a golden image is a picture of one adapter's
        // rasteriser: on a hybrid-GPU laptop the default is the integrated
        // GPU, and a mismatch reads as a rendering bug when it is a device.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::util::backend_bits_from_env().unwrap_or(wgpu::Backends::all()),
            ..Default::default()
        });
        // No surface to be compatible with: that is the whole point.
        let adapter = pollster::block_on(wgpu::util::initialize_adapter_from_env_or_default(&instance, None))
            .context("requesting an adapter")?;
        let info = adapter.get_info();
        if ADAPTER.set(info.name.clone()).is_ok() {
            eprintln!("[headless] rendering on {} ({:?}, {:?})", info.name, info.backend, info.device_type);
        }

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("jengine.headless.device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .context("requesting a device")?;

        // sRGB, like the surface formats the hosts pick, so colours in a
        // golden image match what a player sees.
        let color_format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("jengine.headless.color"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: color_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Ok(Self { device, queue, color_format, width, height, texture, view, adapter: info })
    }

    /// A fresh command encoder.
    pub fn encoder(&self) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("jengine.headless") })
    }

    /// A render pass over the colour target, cleared to `clear`.
    pub fn color_pass<'a>(
        &'a self,
        encoder: &'a mut wgpu::CommandEncoder,
        clear: wgpu::Color,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("jengine.headless.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// A render pass over the colour target that keeps what is there - a
    /// second pass in one frame.
    pub fn color_pass_load<'a>(&'a self, encoder: &'a mut wgpu::CommandEncoder) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("jengine.headless.pass (load)"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// The colour target itself, for passes that write it directly - the
    /// lighting composite takes a destination view rather than opening a pass.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// The colour target's texture, for copies out of it (it is `COPY_SRC`).
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub fn submit(&self, encoder: wgpu::CommandEncoder) {
        self.queue.submit(Some(encoder.finish()));
    }

    /// The colour target's pixels, tightly packed RGBA8.
    ///
    /// Copying out has to respect wgpu's 256-byte row alignment, so the rows
    /// come back padded and are trimmed here - forget that and every image is
    /// sheared by a few pixels per row.
    pub fn read_rgba(&self) -> RawTextureData {
        const ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let unpadded = self.width * 4;
        let padded = unpadded.div_ceil(ALIGN) * ALIGN;

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("jengine.headless.readback"),
            size: (padded * self.height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self.encoder();
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        self.submit(encoder);

        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::Maintain::Wait);

        let mapped = slice.get_mapped_range();
        let mut rgba = Vec::with_capacity((unpadded * self.height) as usize);
        for row in 0..self.height {
            let start = (row * padded) as usize;
            rgba.extend_from_slice(&mapped[start..start + unpadded as usize]);
        }
        drop(mapped);
        buffer.unmap();

        RawTextureData { width: self.width, height: self.height, rgba }
    }
}
