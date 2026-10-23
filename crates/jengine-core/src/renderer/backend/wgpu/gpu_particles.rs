//! The draw for GPU particles (`particles::gpu`): one instanced call per
//! emitter, six vertices per particle, every position computed by
//! `gpu_particles.wgsl` from the instance index and one uniform.
//!
//! Owned by [`super::WgpuSpriteBatch`], which draws a pending emitter in
//! place of its quads at the next flush - so GPU particles sort by `z_order`
//! with sprites in the one world pass and a host needs no wiring.

use std::sync::Arc;

use crate::particles::gpu::GpuParticleUniform;
use crate::renderer::material::BlendMode;

use super::WgpuTexture;

const SHADER: &str = include_str!("../../../../assets/shaders/wgsl/gpu_particles.wgsl");

pub(crate) struct WgpuGpuParticles {
    shader: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: Vec<(wgpu::TextureFormat, BlendMode, Arc<wgpu::RenderPipeline>)>,
}

impl WgpuGpuParticles {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu particles"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu particles"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu particles"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        Self { shader, layout, pipeline_layout, pipelines: Vec::new() }
    }

    fn pipeline(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat, blend: BlendMode) -> Arc<wgpu::RenderPipeline> {
        if let Some((_, _, p)) = self.pipelines.iter().find(|(f, b, _)| *f == format && *b == blend) {
            return Arc::clone(p);
        }
        let p = Arc::new(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gpu particles"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(super::sprite::blend_mode_to_wgpu(blend)),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        }));
        self.pipelines.push((format, blend, Arc::clone(&p)));
        p
    }

    /// Draw `count` particles of `uniform` with `texture`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        uniform: &GpuParticleUniform,
        count: u32,
        texture: &WgpuTexture,
        format: wgpu::TextureFormat,
        blend: BlendMode,
    ) {
        if count == 0 {
            return;
        }
        let pipeline = self.pipeline(device, format, blend);
        // A buffer per draw: two emitters in one submit must not share one.
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu particles uniform"),
            size: std::mem::size_of::<GpuParticleUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::bytes_of(uniform));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu particles"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&texture.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&texture.sampler) },
            ],
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..6, 0..count);
    }
}

// ─── particles that keep state (`particles::gpu_state`) ─────────────────

const STEP_SHADER: &str = include_str!("../../../../assets/shaders/wgsl/gpu_particles_step.wgsl");
const STATE_SHADER: &str = include_str!("../../../../assets/shaders/wgsl/gpu_particles_state.wgsl");

/// One stateful emitter's particles on the GPU, and the clock to step them by.
struct Slot {
    buffer: wgpu::Buffer,
    count: u32,
    clock: crate::particles::gpu_state::StepClock,
    /// The frame it was last stepped in; a slot not stepped is dropped.
    frame: u64,
}

/// The compute step, the draw that reads its buffers, and every stateful
/// emitter's buffer, keyed by its entity. Made only on a device that can run
/// compute and read storage buffers in the vertex stage.
pub(crate) struct WgpuParticleStates {
    step_layout: wgpu::BindGroupLayout,
    step_pipeline: wgpu::ComputePipeline,
    draw_shader: wgpu::ShaderModule,
    draw_layout: wgpu::BindGroupLayout,
    draw_pipeline_layout: wgpu::PipelineLayout,
    pipelines: Vec<(wgpu::TextureFormat, BlendMode, Arc<wgpu::RenderPipeline>)>,
    slots: std::collections::HashMap<u64, Slot>,
    frame: u64,
}

impl WgpuParticleStates {
    /// The step and the draw, if `device` can run them: compute, and
    /// storage buffers the vertex stage reads. WebGL2 asks for neither (its
    /// limits are zero); a native backend that cannot is caught by building
    /// the pipelines inside an error scope.
    pub(crate) fn try_new(device: &wgpu::Device) -> Option<Self> {
        let l = device.limits();
        if l.max_compute_workgroups_per_dimension == 0
            || l.max_storage_buffers_per_shader_stage == 0
            || l.max_compute_invocations_per_workgroup < crate::particles::gpu_state::WORKGROUP
        {
            log::info!("[gpu particles] no compute here: stateful emitters draw in closed form");
            return None;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let made = Self::new(device);
            // Build a draw pipeline too: the vertex stage's storage read is
            // what a downlevel backend refuses.
            let mut made = made;
            let _ = made.pipeline(device, wgpu::TextureFormat::Rgba8UnormSrgb, BlendMode::Additive);
            if let Some(e) = pollster::block_on(device.pop_error_scope()) {
                log::warn!("[gpu particles] stateful emitters draw in closed form here: {e}");
                return None;
            }
            Some(made)
        }
        #[cfg(target_arch = "wasm32")]
        {
            Some(Self::new(device))
        }
    }

    fn new(device: &wgpu::Device) -> Self {
        let step_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu particles step"),
            source: wgpu::ShaderSource::Wgsl(STEP_SHADER.into()),
        });
        let step_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu particles step"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let step_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu particles step"),
            bind_group_layouts: &[&step_layout],
            push_constant_ranges: &[],
        });
        let step_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpu particles step"),
            layout: Some(&step_pipeline_layout),
            module: &step_shader,
            entry_point: Some("cs_step"),
            compilation_options: Default::default(),
            cache: None,
        });
        let draw_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu particles state"),
            source: wgpu::ShaderSource::Wgsl(STATE_SHADER.into()),
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu particles state"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let draw_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu particles state"),
            bind_group_layouts: &[&draw_layout],
            push_constant_ranges: &[],
        });
        Self {
            step_layout,
            step_pipeline,
            draw_shader,
            draw_layout,
            draw_pipeline_layout,
            pipelines: Vec::new(),
            slots: std::collections::HashMap::new(),
            frame: 0,
        }
    }

    /// Step every stateful emitter one frame, in one compute pass on
    /// `encoder`; make a buffer for a new one (or one whose count changed)
    /// and drop the buffers of emitters that are gone.
    pub(crate) fn step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        emitters: &[crate::particles::gpu_state::StateEmitter],
    ) {
        use crate::particles::gpu_state::{self, StateParticle, StepUniform};
        use wgpu::util::DeviceExt;
        self.frame += 1;
        let frame = self.frame;
        let mut work: Vec<(wgpu::BindGroup, u32)> = Vec::new();
        for e in emitters {
            let count = e.emitter.count;
            let fresh = self.slots.get(&e.key).is_none_or(|s| s.count != count);
            if fresh {
                let init = gpu_state::initial(&e.emitter);
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("gpu particle state"),
                    contents: bytemuck::cast_slice::<StateParticle, u8>(&init),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                });
                self.slots.insert(e.key, Slot { buffer, count, clock: Default::default(), frame });
            }
            let slot = self.slots.get_mut(&e.key).expect("just made");
            slot.frame = frame;
            let (u, next) = gpu_state::step_uniform(e, &slot.clock);
            slot.clock = next;
            let Some(u) = u else { continue };
            let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("gpu particle step"),
                contents: bytemuck::bytes_of::<StepUniform>(&u),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gpu particle step"),
                layout: &self.step_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: slot.buffer.as_entire_binding() },
                ],
            });
            work.push((bind, count));
        }
        self.slots.retain(|_, s| s.frame == frame);
        let _ = queue;
        if work.is_empty() {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("gpu particles step"), timestamp_writes: None });
        pass.set_pipeline(&self.step_pipeline);
        for (bind, count) in &work {
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(count.div_ceil(gpu_state::WORKGROUP), 1, 1);
        }
    }

    /// Whether `key`'s particles have a buffer to draw from.
    pub(crate) fn has(&self, key: u64) -> bool {
        self.slots.contains_key(&key)
    }

    fn pipeline(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat, blend: BlendMode) -> Arc<wgpu::RenderPipeline> {
        if let Some((_, _, p)) = self.pipelines.iter().find(|(f, b, _)| *f == format && *b == blend) {
            return Arc::clone(p);
        }
        let p = Arc::new(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gpu particles state"),
            layout: Some(&self.draw_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.draw_shader,
                entry_point: Some("vs_state"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.draw_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(super::sprite::blend_mode_to_wgpu(blend)),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        }));
        self.pipelines.push((format, blend, Arc::clone(&p)));
        p
    }

    /// Draw `key`'s particles with the look in `uniform`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        key: u64,
        uniform: &GpuParticleUniform,
        texture: &WgpuTexture,
        format: wgpu::TextureFormat,
        blend: BlendMode,
    ) -> u32 {
        let pipeline = self.pipeline(device, format, blend);
        let Some(slot) = self.slots.get(&key) else { return 0 };
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu particles state uniform"),
            size: std::mem::size_of::<GpuParticleUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::bytes_of(uniform));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu particles state"),
            layout: &self.draw_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&texture.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&texture.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: slot.buffer.as_entire_binding() },
            ],
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..6, 0..slot.count);
        slot.count
    }

    /// `key`'s particles read back (a test's view of the GPU's state).
    pub(crate) fn read(&self, device: &wgpu::Device, queue: &wgpu::Queue, key: u64) -> Option<Vec<crate::particles::gpu_state::StateParticle>> {
        let slot = self.slots.get(&key)?;
        let size = slot.buffer.size();
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu particle state readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("gpu particle state readback") });
        encoder.copy_buffer_to_buffer(&slot.buffer, 0, &staging, 0, size);
        queue.submit(Some(encoder.finish()));
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv().ok()?.ok()?;
        let data = staging.slice(..).get_mapped_range();
        let out = bytemuck::cast_slice::<u8, crate::particles::gpu_state::StateParticle>(&data).to_vec();
        drop(data);
        staging.unmap();
        Some(out)
    }
}
