//! [`WgpuRenderer`] — the wgpu equivalent of the legacy
//! [`crate::renderer::Renderer`].
//!
//! Owns the `wgpu::Instance` / `Surface` / `Adapter` / `Device` / `Queue` and
//! tracks the swapchain configuration. A new [`WgpuFrame`] is acquired each
//! frame; it owns the render pass encoder and lives until present time.
//!
//! ## Why a separate type from `Renderer`?
//! The `glow` `Renderer` exposes `pub gl: glow::Context` which the entire
//! engine (including ECS systems) takes by reference. Mirroring that exact
//! shape in wgpu is awkward (multiple distinct objects, lifetimes on the
//! pass). Instead we keep both renderers as siblings during migration: the
//! runtime picks one based on `cfg(feature = "gpu-wgpu")` and ECS systems
//! eventually move onto the [`backend::traits::GpuDevice`] surface (Phase 3.b).

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use anyhow::{anyhow, Result};
use glam::UVec2;

use crate::renderer::gpu_preference::GpuPreference;
#[cfg(not(target_arch = "wasm32"))]
use pollster::block_on;
use wgpu::{
    Adapter, Device, Instance, Queue, Surface, SurfaceConfiguration, SurfaceError, TextureFormat,
};
#[cfg(not(target_arch = "wasm32"))]
use winit::window::Window;

/// Top-level renderer owning the wgpu device + swapchain.
///
/// Created from a winit `Window` (held as `Arc` so the surface gets a `'static`
/// borrow). The device + queue are public so the renderer's children
/// (`WgpuTexture`, `WgpuSpriteBatch`, …) can hand them out without forcing
/// every call site through accessor methods.
pub struct WgpuRenderer {
    pub instance: Instance,
    pub adapter:  Adapter,
    pub device:   Device,
    pub queue:    Queue,
    /// `None` while the app is suspended. A phone destroys the native window
    /// whenever the app leaves the foreground, and a surface outliving its
    /// window is undefined behaviour, so [`Self::suspend`] drops it and
    /// [`Self::resume`] builds a new one - the device, queue and everything
    /// uploaded to them survive. Desktop never suspends.
    pub surface:  Option<Surface<'static>>,
    pub config:   SurfaceConfiguration,
    /// Native: the winit window the surface was created from. Web builds use
    /// the canvas element directly and don't keep a winit window around.
    #[cfg(not(target_arch = "wasm32"))]
    pub window:   Arc<Window>,
    pub size:     UVec2,
    /// Color attachment format chosen for the swapchain. All render pipelines
    /// that target the screen must declare this format.
    pub color_format: TextureFormat,
    /// The format the **world** is drawn in: sprites, text, lighting, and
    /// the post chain up to its final output pass. `Rgba16Float` wherever
    /// the adapter can render to, blend and filter it, so light and
    /// additive glows can go past 1.0 and be tonemapped rather than clipped;
    /// otherwise the same as [`Self::color_format`]. See
    /// [`pick_scene_format`]. UI is drawn after the post chain, in
    /// `color_format`.
    pub scene_format: TextureFormat,
    /// RGBA clear color applied at the start of each frame.
    pub clear_color:  wgpu::Color,
    /// A scene transition's held frame and pass (`renderer::transition`),
    /// made the first time one is armed and shared with the frames that
    /// copy into it or draw it.
    transition: std::sync::Arc<std::sync::Mutex<Option<super::transition::WgpuTransition>>>,
}

impl WgpuRenderer {
    /// Create a new renderer for the given window. Blocks on adapter + device
    /// acquisition (wgpu's only async surface).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(window: Arc<Window>) -> Result<Self> {
        Self::new_with_gpu(window, GpuPreference::default())
    }

    /// [`Self::new`] asking for the GPU a project prefers
    /// (`settings.gpu_preference`, [`GpuPreference`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_with_gpu(window: Arc<Window>, gpu: GpuPreference) -> Result<Self> {
        let inner = window.inner_size();
        let size = UVec2::new(inner.width.max(1), inner.height.max(1));

        let instance = super::create_instance();
        let surface = instance.create_surface(window.clone())?;

        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference:       gpu.to_wgpu(),
            compatible_surface:     Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| anyhow!("no compatible wgpu adapter"))?;

        log::info!("{}", super::describe_adapter(&adapter));

        let (device, queue) = block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label:             Some("jengine wgpu device"),
                required_features: wgpu::Features::empty(),
                // `downlevel_defaults` runs on WebGL2 and any reasonable native
                // GPU - except for its 2048 px texture cap, which is also the
                // swapchain's: a 2400 x 1080 phone, or a window wider than
                // 2048 on a desktop, failed `Surface::configure` with a panic.
                // So the size limits come from the adapter.
                required_limits:   wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints:      wgpu::MemoryHints::Performance,
            },
            None,
        ))?;

        let surface_caps = surface.get_capabilities(&adapter);
        let (surface_format, color_format) = pick_surface_format(&surface_caps.formats);

        let config = SurfaceConfiguration {
            usage:        surface_usage(&surface_caps),
            format:       surface_format,
            width:        size.x,
            height:       size.y,
            present_mode: pick_present_mode(&surface_caps),
            alpha_mode:   surface_caps.alpha_modes[0],
            view_formats: srgb_view_formats(surface_format, color_format),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let scene_format = pick_scene_format(&adapter, color_format);

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            surface: Some(surface),
            config,
            window,
            size,
            color_format,
            scene_format,
            clear_color: wgpu::Color { r: 0.1, g: 0.1, b: 0.12, a: 1.0 },
            transition: Default::default(),
        })
    }

    /// Web-only async constructor: build a renderer from an existing
    /// `<canvas>` element. wgpu's adapter/device acquisition is genuinely
    /// async on the browser (can't `block_on` the main thread).
    ///
    /// `canvas_size` is used to configure the swapchain — typically the
    /// canvas's own `width`/`height` attributes.
    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    pub async fn new_from_canvas_async(
        canvas:      web_sys::HtmlCanvasElement,
        canvas_size: UVec2,
    ) -> Result<Self> {
        Self::new_from_canvas_async_with_gpu(canvas, canvas_size, GpuPreference::default()).await
    }

    /// [`Self::new_from_canvas_async`] asking for the GPU a project prefers.
    /// The browser may ignore the request.
    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    pub async fn new_from_canvas_async_with_gpu(
        canvas:      web_sys::HtmlCanvasElement,
        canvas_size: UVec2,
        gpu:         GpuPreference,
    ) -> Result<Self> {
        let size = UVec2::new(canvas_size.x.max(1), canvas_size.y.max(1));

        let instance = super::create_instance();
        let surface = instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference:       gpu.to_wgpu(),
                compatible_surface:     Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| anyhow!("no compatible wgpu adapter (WebGPU required)"))?;

        log::info!("{}", super::describe_adapter(&adapter));

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label:             Some("jengine wgpu device (web)"),
                    required_features: wgpu::Features::empty(),
                    // Size limits from the adapter, as natively: a high-DPI
                    // canvas passes 2048 px easily.
                    required_limits:   wgpu::Limits::downlevel_webgl2_defaults()
                        .using_resolution(adapter.limits()),
                    memory_hints:      wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
        // A WebGPU canvas offers no sRGB format at all - see
        // `pick_surface_format` for what that used to cost.
        let (surface_format, color_format) = pick_surface_format(&surface_caps.formats);

        let config = SurfaceConfiguration {
            usage:        surface_usage(&surface_caps),
            format:       surface_format,
            width:        size.x,
            height:       size.y,
            present_mode: pick_present_mode(&surface_caps),
            alpha_mode:   surface_caps.alpha_modes[0],
            view_formats: srgb_view_formats(surface_format, color_format),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let scene_format = pick_scene_format(&adapter, color_format);

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            surface: Some(surface),
            config,
            size,
            color_format,
            scene_format,
            clear_color: wgpu::Color { r: 0.1, g: 0.1, b: 0.12, a: 1.0 },
            transition: Default::default(),
        })
    }

    /// Resize the swapchain. No-op if either dimension is zero (minimized).
    pub fn resize(&mut self, new_size: UVec2) {
        if new_size.x == 0 || new_size.y == 0 || new_size == self.size {
            return;
        }
        self.size = new_size;
        self.config.width = new_size.x;
        self.config.height = new_size.y;
        if let Some(surface) = &self.surface {
            surface.configure(&self.device, &self.config);
        }
    }

    /// Whether there is a surface to draw to - false between
    /// [`Self::suspend`] and [`Self::resume`].
    pub fn is_suspended(&self) -> bool {
        self.surface.is_none()
    }

    /// Drop the surface: the app left the foreground and its window is going
    /// away. Everything else is kept for [`Self::resume`].
    pub fn suspend(&mut self) {
        self.surface = None;
    }

    /// Build a surface for the window again after [`Self::suspend`], at the
    /// window's current size - a phone may have been turned in the meantime.
    ///
    /// The colour format has to be the one the renderer started with, since
    /// every pipeline was built against it; a surface that cannot take it is
    /// an error rather than a silent mismatch.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn resume(&mut self) -> Result<()> {
        if self.surface.is_some() {
            return Ok(());
        }
        let surface = self.instance.create_surface(self.window.clone())?;
        let caps = surface.get_capabilities(&self.adapter);
        if !caps.formats.contains(&self.color_format) {
            return Err(anyhow!(
                "resumed surface cannot use {:?} (offers {:?})",
                self.color_format, caps.formats
            ));
        }
        let inner = self.window.inner_size();
        self.size = UVec2::new(inner.width.max(1), inner.height.max(1));
        self.config.width = self.size.x;
        self.config.height = self.size.y;
        surface.configure(&self.device, &self.config);
        self.surface = Some(surface);
        Ok(())
    }

    /// Turn vsync on or off (`Display.set_vsync`), reconfiguring the
    /// swapchain only when the present mode changes. Returns whether frames
    /// now wait for the refresh. Suspended, it changes the mode the next
    /// surface is built with.
    pub fn set_vsync(&mut self, on: bool) -> bool {
        let modes = match &self.surface {
            Some(surface) => surface.get_capabilities(&self.adapter).present_modes,
            None => vec![wgpu::PresentMode::Fifo],
        };
        let mode = pick_vsync_mode(&modes, on);
        if mode != self.config.present_mode {
            log::info!("wgpu present mode: {mode:?} (vsync {})", if on { "on" } else { "off" });
            self.config.present_mode = mode;
            if let Some(surface) = &self.surface {
                surface.configure(&self.device, &self.config);
            }
        }
        self.vsync()
    }

    /// Whether frames wait for the display's refresh.
    pub fn vsync(&self) -> bool {
        matches!(self.config.present_mode, wgpu::PresentMode::Fifo | wgpu::PresentMode::FifoRelaxed)
    }

    pub fn set_clear_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        self.clear_color = wgpu::Color { r: r as f64, g: g as f64, b: b as f64, a: a as f64 };
    }

    pub fn viewport_size(&self) -> (u32, u32) {
        (self.size.x, self.size.y)
    }

    /// Begin a frame. Returns a [`WgpuFrame`] that holds the surface texture
    /// and a command encoder. Drop / call [`WgpuFrame::present`] to submit.
    pub fn begin_frame(&self) -> Result<WgpuFrame, SurfaceError> {
        // Suspended: report the surface lost, which every caller already
        // treats as "skip this frame".
        let Some(surface) = &self.surface else {
            return Err(SurfaceError::Lost);
        };
        let surface_texture = surface.get_current_texture()?;
        // Through the sRGB view the pipelines were built for, which on a
        // WebGPU canvas is not the surface's own format.
        let view = surface_texture.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.color_format),
            ..Default::default()
        });
        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        // A screenshot asked for this frame (`renderer::screenshot`): the
        // buffer it is copied into, made here where the device is.
        crate::renderer::screenshot::gpu::poll(&self.device);
        let copyable = self.config.usage.contains(wgpu::TextureUsages::COPY_SRC);
        let capture = if copyable {
            crate::renderer::screenshot::gpu::begin(&self.device, self.config.width, self.config.height, self.config.format)
        } else {
            None
        };
        // A scene transition (`renderer::transition`): this frame is copied
        // while one waits for its scene, and the held frame drawn over it
        // while one plays.
        crate::renderer::transition::set_supported(copyable);
        let plan = crate::renderer::transition::frame_plan();
        let transition = (plan.capture || plan.draw.is_some()).then(|| {
            let mut gpu = self.transition.lock().unwrap_or_else(|e| e.into_inner());
            let t = gpu.get_or_insert_with(|| {
                super::transition::WgpuTransition::new(&self.device, self.config.format, self.color_format)
            });
            t.prepare(&self.device, &self.queue, self.config.width, self.config.height);
            (std::sync::Arc::clone(&self.transition), plan)
        });
        Ok(WgpuFrame { surface_texture, view, encoder, clear_color: self.clear_color, capture, transition })
    }
}

/// The swapchain's usage: drawn into, and copied out of where the platform
/// allows it, for screenshots.
fn surface_usage(caps: &wgpu::SurfaceCapabilities) -> wgpu::TextureUsages {
    let copy = caps.usages & wgpu::TextureUsages::COPY_SRC;
    wgpu::TextureUsages::RENDER_ATTACHMENT | copy
}

/// One in-flight frame. Owns the swapchain texture + command encoder.
pub struct WgpuFrame {
    pub surface_texture: wgpu::SurfaceTexture,
    pub view:            wgpu::TextureView,
    pub encoder:         wgpu::CommandEncoder,
    pub clear_color:     wgpu::Color,
    /// A screenshot of this frame, copied out as it is presented.
    capture:             Option<crate::renderer::screenshot::gpu::Capture>,
    /// A scene transition this frame copies into or is drawn over.
    #[allow(clippy::type_complexity)]
    transition:          Option<(
        std::sync::Arc<std::sync::Mutex<Option<super::transition::WgpuTransition>>>,
        crate::renderer::transition::FramePlan,
    )>,
}

impl WgpuFrame {
    /// Begin a render pass that clears to the renderer's clear color and
    /// targets the swapchain.
    pub fn begin_main_pass(&mut self) -> wgpu::RenderPass<'_> {
        self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("main"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load:  wgpu::LoadOp::Clear(self.clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// Reopen the display view **without clearing it**: the pass UI is
    /// drawn in, after the post chain's output pass has written the frame.
    ///
    /// UI used to be drawn into the scene target before the post chain, so
    /// bloom haloed its text, the vignette darkened a caption in a corner,
    /// and chromatic aberration split a HUD's edges. Drawn here, in the
    /// display format (`WgpuRenderer::color_format`), it is exactly the
    /// colour it was authored.
    pub fn overlay_pass(&mut self) -> wgpu::RenderPass<'_> {
        self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load:  wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// Submit the encoder and present the frame - copied out first when a
    /// screenshot asked for it (`renderer::screenshot`), which is why every
    /// host has screenshots without a line of its own.
    ///
    /// A scene transition rides here the same way: the held frame of the
    /// old scene is drawn over this one while it plays, and this frame is
    /// copied into it while one waits for its scene (after the drawing, so
    /// it is what the player saw).
    pub fn present(mut self, queue: &wgpu::Queue) {
        let mut copied = false;
        if let Some((gpu, plan)) = &self.transition {
            let gpu = gpu.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(t) = gpu.as_ref() {
                if let Some((style, k)) = plan.draw {
                    t.draw(&mut self.encoder, queue, &self.view, style, k);
                }
                if plan.capture {
                    t.copy_from(&mut self.encoder, &self.surface_texture.texture);
                    copied = true;
                }
            }
        }
        if let Some(c) = &self.capture {
            c.encode(&mut self.encoder, &self.surface_texture.texture);
        }
        queue.submit(Some(self.encoder.finish()));
        if let Some(c) = self.capture.take() {
            c.submitted();
        }
        if copied {
            crate::renderer::transition::mark_captured();
        }
        self.surface_texture.present();
    }
}

/// The surface's format and the format frames are drawn in: an sRGB one,
/// always.
///
/// Every colour the engine draws is linear - textures are uploaded as sRGB
/// and sampled back to linear, and sprite, text and GUI colours are linear
/// values - so the target has to encode to sRGB on the way out. A desktop
/// surface offers an sRGB format and that is the end of it. **A WebGPU
/// canvas offers none** (`rgba8unorm`, `bgra8unorm`, `rgba16float`), and
/// until Sep 2026 this fell back to the first of those: the editor viewport
/// and every web build then wrote linear values straight to the screen, so
/// every texture and every colour drew darker there than natively - sRGB
/// mid grey, 128, drew as 55. Found when a game's slate backdrop,
/// picked by eye natively, was checked against what it had asked for.
///
/// So when the surface has no sRGB format, it is configured in its own
/// format with the sRGB variant as a *view* format (which WebGPU permits
/// for a canvas), and frames draw through that view.
pub fn pick_surface_format(formats: &[TextureFormat]) -> (TextureFormat, TextureFormat) {
    if let Some(f) = formats.iter().copied().find(|f| f.is_srgb()) {
        return (f, f);
    }
    let surface = formats.first().copied().unwrap_or(TextureFormat::Bgra8Unorm);
    (surface, surface.add_srgb_suffix())
}

/// The format the world is drawn in: a float format the adapter can render
/// to, blend into and filter, or `fallback` (the swapchain's format).
///
/// Float is what makes HDR possible at all. In an 8-bit target nothing is
/// brighter than 1.0, so a fire core and a lit wall are the same value,
/// light saturates at the albedo, and bloom cannot tell a light from a
/// white surface. All three capabilities are required: render (the world
/// is drawn into it), blend (sprites are alpha-blended and lights added),
/// and filter (bloom and the output pass sample it bilinearly). WebGPU and
/// every desktop adapter have all three for `Rgba16Float`; WebGL2 and older
/// GLES devices may not, and fall back to drawing exactly as before.
pub fn pick_scene_format(adapter: &wgpu::Adapter, fallback: TextureFormat) -> TextureFormat {
    let want = TextureFormat::Rgba16Float;
    let f = adapter.get_texture_format_features(want);
    let usable = f.allowed_usages.contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        && f.allowed_usages.contains(wgpu::TextureUsages::TEXTURE_BINDING)
        && f.flags.contains(wgpu::TextureFormatFeatureFlags::FILTERABLE)
        && f.flags.contains(wgpu::TextureFormatFeatureFlags::BLENDABLE);
    let chosen = if usable { want } else { fallback };
    log::info!("wgpu scene format: {chosen:?} ({})", if usable { "HDR" } else { "no float target; LDR" });
    chosen
}

/// Whether `format` can hold values above 1.0 - what decides if a scene's
/// `hdr` setting can take effect.
pub fn is_hdr_format(format: TextureFormat) -> bool {
    matches!(
        format,
        TextureFormat::Rgba16Float | TextureFormat::Rgba32Float | TextureFormat::Rg11b10Ufloat
    )
}

/// The view formats a surface must allow for [`pick_surface_format`]'s
/// answer: its sRGB twin, when it is not already sRGB.
pub fn srgb_view_formats(surface: TextureFormat, color: TextureFormat) -> Vec<TextureFormat> {
    if surface == color { Vec::new() } else { vec![color] }
}

/// Pick the swapchain present mode.
///
/// This used to be `surface_caps.present_modes[0]` — whatever the driver
/// happened to list first — which is **not** a default, and on Windows/DX12
/// is `Mailbox`. wgpu-hal maps `Mailbox` to `Present(0, 0)`: sync interval
/// zero, no tearing flag. The loop then free-runs and DWM latches whichever
/// buffer is newest at each composition tick, so the frame rate reads high
/// while the simulation advances on a clock unrelated to the display's
/// sampling instants. That is judder with a healthy FPS counter, and it gets
/// far worse with a second monitor attached, because DWM stops granting the
/// window an independent flip and resamples the present stream onto its own
/// composition clock.
///
/// `Fifo` is guaranteed present by both the WebGPU spec and wgpu, so the
/// fallback is only there to keep this total.
fn pick_present_mode(surface_caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
    let mode = if surface_caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
        wgpu::PresentMode::Fifo
    } else {
        surface_caps.present_modes[0]
    };
    log::info!(
        "wgpu present mode: {:?} (available: {:?})",
        mode, surface_caps.present_modes
    );
    mode
}

/// The present mode for vsync on or off among `modes`: on is `Fifo`; off
/// the first the surface offers of `Immediate` (tearing allowed, what "vsync
/// off" means in a game's options) and `Mailbox`, else `Fifo` after all - a
/// WebGPU canvas has only that.
pub fn pick_vsync_mode(modes: &[wgpu::PresentMode], on: bool) -> wgpu::PresentMode {
    use wgpu::PresentMode::*;
    if on {
        return Fifo;
    }
    [Immediate, Mailbox].into_iter().find(|m| modes.contains(m)).unwrap_or(Fifo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vsync_off_takes_what_the_surface_offers() {
        use wgpu::PresentMode::*;
        assert_eq!(pick_vsync_mode(&[Fifo, Mailbox, Immediate], false), Immediate);
        assert_eq!(pick_vsync_mode(&[Fifo, Mailbox], false), Mailbox);
        assert_eq!(pick_vsync_mode(&[Fifo], false), Fifo, "a canvas: always vsynced");
        assert_eq!(pick_vsync_mode(&[Mailbox, Immediate, Fifo], true), Fifo);
    }

    #[test]
    fn a_surface_with_srgb_is_drawn_in_it_directly() {
        let (surface, color) = pick_surface_format(&[TextureFormat::Bgra8Unorm, TextureFormat::Bgra8UnormSrgb]);
        assert_eq!((surface, color), (TextureFormat::Bgra8UnormSrgb, TextureFormat::Bgra8UnormSrgb));
        assert!(srgb_view_formats(surface, color).is_empty());
    }

    /// What a WebGPU canvas reports (wgpu 23's `surface_get_capabilities`):
    /// no sRGB format at all, the preferred one first.
    #[test]
    fn a_webgpu_canvas_is_drawn_through_an_srgb_view() {
        let caps = [TextureFormat::Bgra8Unorm, TextureFormat::Rgba8Unorm, TextureFormat::Rgba16Float];
        let (surface, color) = pick_surface_format(&caps);
        assert_eq!(surface, TextureFormat::Bgra8Unorm, "the canvas is configured in a format it offers");
        assert_eq!(color, TextureFormat::Bgra8UnormSrgb, "and drawn in its sRGB view");
        assert_eq!(srgb_view_formats(surface, color), vec![TextureFormat::Bgra8UnormSrgb]);
        assert!(color.is_srgb());
    }
}
