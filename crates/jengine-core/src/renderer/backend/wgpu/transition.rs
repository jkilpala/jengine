//! The GPU half of scene transitions (`renderer::transition`): the texture
//! the outgoing frame is copied into, the dissolve's mask, and the pass that
//! draws the held frame over a new one (`transition.wgsl`). Owned by the
//! [`super::WgpuRenderer`] and handed to each frame that needs it, so
//! `WgpuFrame::present` copies and draws with no host wiring.

use wgpu::util::DeviceExt;

const TRANSITION_WGSL: &str = include_str!("../../../../assets/shaders/wgsl/transition.wgsl");

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct TransitionU {
    /// x style, y progress, z aspect, w 1 when the mask is a texture.
    p: [f32; 4],
}

/// The held frame, the mask and the pipeline that wipes one over the other.
pub struct WgpuTransition {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    /// The surface's own format (what a frame is copied out in) and the
    /// format it is sampled in (sRGB, so it decodes to linear).
    surface_format: wgpu::TextureFormat,
    view_format: wgpu::TextureFormat,
    held: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    mask: (wgpu::Texture, wgpu::TextureView),
    mask_is_texture: bool,
    mask_rev: u64,
    bind: Option<wgpu::BindGroup>,
}

impl WgpuTransition {
    /// For frames of `surface_format`, drawn through views of `view_format`.
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat, view_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("transition"),
            source: wgpu::ShaderSource::Wgsl(TRANSITION_WGSL.into()),
        });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("transition"),
            entries: &[
                tex_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("transition"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("transition"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: view_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("transition"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("transition"),
            contents: bytemuck::bytes_of(&TransitionU { p: [0.0; 4] }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let mask = Self::mask_texture(device, 1, 1);
        Self {
            pipeline,
            layout,
            sampler,
            uniform,
            surface_format,
            view_format,
            held: None,
            mask,
            mask_is_texture: false,
            mask_rev: 0,
            bind: None,
        }
    }

    fn mask_texture(device: &wgpu::Device, w: u32, h: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let t = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("transition mask"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let v = t.create_view(&Default::default());
        (t, v)
    }

    /// The texture a frame is copied into, `width` x `height`: made (or
    /// remade at a new size, losing what it held) as needed; and the mask
    /// brought up to date.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        if self.held.as_ref().is_none_or(|(_, _, hw, hh)| (*hw, *hh) != (w, h)) {
            let view_formats = if self.view_format != self.surface_format { vec![self.view_format] } else { Vec::new() };
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("transition frame"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.surface_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &view_formats,
            });
            let v = t.create_view(&wgpu::TextureViewDescriptor { format: Some(self.view_format), ..Default::default() });
            self.held = Some((t, v, w, h));
            self.bind = None;
        }
        if let Some((rev, mask)) = crate::renderer::transition::mask_if_changed(self.mask_rev) {
            self.mask_rev = rev;
            match mask {
                Some(m) if m.width > 0 && m.height > 0 && m.rgba.len() >= (m.width * m.height * 4) as usize => {
                    self.mask = Self::mask_texture(device, m.width, m.height);
                    queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &self.mask.0,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &m.rgba,
                        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(m.width * 4), rows_per_image: Some(m.height) },
                        wgpu::Extent3d { width: m.width, height: m.height, depth_or_array_layers: 1 },
                    );
                    self.mask_is_texture = true;
                }
                _ => {
                    self.mask = Self::mask_texture(device, 1, 1);
                    self.mask_is_texture = false;
                }
            }
            self.bind = None;
        }
        if self.bind.is_none()
            && let Some((_, view, _, _)) = &self.held
        {
            self.bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("transition"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.mask.1) },
                    wgpu::BindGroupEntry { binding: 3, resource: self.uniform.as_entire_binding() },
                ],
            }));
        }
    }

    /// The texture the outgoing frame is held in (tests write into it).
    pub fn held_texture(&self) -> Option<&wgpu::Texture> {
        self.held.as_ref().map(|(t, ..)| t)
    }

    /// Copy a finished frame (`src`, the swapchain image) into the held
    /// texture. Sizes must match, which [`Self::prepare`] arranged.
    pub fn copy_from(&self, encoder: &mut wgpu::CommandEncoder, src: &wgpu::Texture) {
        let Some((dst, _, w, h)) = &self.held else { return };
        if (src.width(), src.height()) != (*w, *h) {
            return;
        }
        encoder.copy_texture_to_texture(
            src.as_image_copy(),
            dst.as_image_copy(),
            wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
        );
    }

    /// Draw the held frame over `target` as `style` leaves it at `progress`.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        style: crate::renderer::transition::Style,
        progress: f32,
    ) {
        let (Some(bind), Some((_, _, w, h))) = (&self.bind, &self.held) else { return };
        let u = TransitionU {
            p: [style.index() as f32, progress, *w as f32 / *h as f32, if self.mask_is_texture { 1.0 } else { 0.0 }],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("transition"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
}
