//! [`WgpuShapeBatch`] — wgpu equivalent of [`crate::renderer::shape::ShapeBatch`].
//!
//! Collects pre-tessellated triangle data (positions + per-vertex colors) and
//! flushes it as a single indexed draw. No texture; bind group is just the
//! globals uniform.

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ShapeVertex {
    position: [f32; 2],
    color:    [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlobalsUniform {
    view_projection: [[f32; 4]; 4],
}

const MAX_VERTICES: usize = 65_536;
const MAX_INDICES:  usize = MAX_VERTICES * 3;

const SHAPE_WGSL: &str = include_str!("../../../../assets/shaders/wgsl/shape.wgsl");

pub struct WgpuShapeBatch {
    pipeline:          wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group:        wgpu::BindGroup,
    vertex_buffer:     wgpu::Buffer,
    index_buffer:      wgpu::Buffer,
    globals_buffer:    wgpu::Buffer,

    vertices: Vec<ShapeVertex>,
    indices:  Vec<u32>,

    pub frame_draw_calls:     u32,
    pub frame_triangle_count: u32,
}

impl WgpuShapeBatch {
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Result<Self> {
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("WgpuShapeBatch vertices"),
            size:  (MAX_VERTICES * std::mem::size_of::<ShapeVertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("WgpuShapeBatch indices"),
            size:  (MAX_INDICES * std::mem::size_of::<u32>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let globals_init = GlobalsUniform { view_projection: Mat4::IDENTITY.to_cols_array_2d() };
        let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:    Some("WgpuShapeBatch globals"),
            contents: bytemuck::bytes_of(&globals_init),
            usage:    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shape bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding:    0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("shape bg"),
            layout:  &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding:  0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("shape.wgsl"),
            source: wgpu::ShaderSource::Wgsl(SHAPE_WGSL.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shape layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("shape pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ShapeVertex>() as wgpu::BufferAddress,
                    step_mode:    wgpu::VertexStepMode::Vertex,
                    attributes:   &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x4,
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend:  Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Ok(Self {
            pipeline,
            bind_group_layout,
            bind_group,
            vertex_buffer,
            index_buffer,
            globals_buffer,
            vertices: Vec::with_capacity(4096),
            indices:  Vec::with_capacity(8192),
            frame_draw_calls: 0,
            frame_triangle_count: 0,
        })
    }

    pub fn begin(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.frame_draw_calls = 0;
        self.frame_triangle_count = 0;
    }

    /// Append pre-tessellated triangles using a single flat color.
    pub fn draw_tessellated(
        &mut self,
        positions: &[[f32; 2]],
        tri_indices: &[u32],
        color: [f32; 4],
    ) {
        if self.vertices.len() + positions.len() > MAX_VERTICES
            || self.indices.len() + tri_indices.len() > MAX_INDICES
        {
            return;
        }
        let base = self.vertices.len() as u32;
        for &pos in positions {
            self.vertices.push(ShapeVertex { position: pos, color });
        }
        for &idx in tri_indices {
            self.indices.push(base + idx);
        }
    }

    /// Append pre-tessellated triangles with per-vertex colors.
    pub fn draw_tessellated_colored(
        &mut self,
        positions: &[[f32; 2]],
        colors:    &[[f32; 4]],
        tri_indices: &[u32],
    ) {
        if self.vertices.len() + positions.len() > MAX_VERTICES
            || self.indices.len() + tri_indices.len() > MAX_INDICES
        {
            return;
        }
        let base = self.vertices.len() as u32;
        for (i, &pos) in positions.iter().enumerate() {
            self.vertices.push(ShapeVertex {
                position: pos,
                color:    colors.get(i).copied().unwrap_or([1.0, 1.0, 1.0, 1.0]),
            });
        }
        for &idx in tri_indices {
            self.indices.push(base + idx);
        }
    }

    /// Append pre-tessellated triangles, transformed by `(offset, rotation, scale)`.
    pub fn draw_tessellated_transformed(
        &mut self,
        positions: &[[f32; 2]],
        tri_indices: &[u32],
        color: [f32; 4],
        offset: Vec2,
        rotation: f32,
        scale: Vec2,
    ) {
        if self.vertices.len() + positions.len() > MAX_VERTICES
            || self.indices.len() + tri_indices.len() > MAX_INDICES
        {
            return;
        }
        let base = self.vertices.len() as u32;
        let cos_r = rotation.cos();
        let sin_r = rotation.sin();
        for &pos in positions {
            let scaled = Vec2::new(pos[0] * scale.x, pos[1] * scale.y);
            let rotated = Vec2::new(
                scaled.x * cos_r - scaled.y * sin_r,
                scaled.x * sin_r + scaled.y * cos_r,
            );
            let world_pos = offset + rotated;
            self.vertices.push(ShapeVertex {
                position: world_pos.into(),
                color,
            });
        }
        for &idx in tri_indices {
            self.indices.push(base + idx);
        }
    }

    /// Upload buffered geometry and emit one draw call. Returns triangle count flushed.
    pub fn flush<'pass>(
        &mut self,
        queue:           &wgpu::Queue,
        pass:            &mut wgpu::RenderPass<'pass>,
        view_projection: &Mat4,
    ) -> usize {
        if self.indices.is_empty() {
            return 0;
        }
        let tri_count = self.indices.len() / 3;

        // Update globals + buffers.
        let globals = GlobalsUniform { view_projection: view_projection.to_cols_array_2d() };
        queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));
        queue.write_buffer(&self.index_buffer,  0, bytemuck::cast_slice(&self.indices));

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.indices.len() as u32, 0, 0..1);

        self.frame_draw_calls     += 1;

        crate::profiling::trace::draw();
        self.frame_triangle_count += tri_count as u32;
        self.vertices.clear();
        self.indices.clear();
        tri_count
    }

    /// Returns true if the batch is near capacity.
    pub fn is_full(&self) -> bool {
        self.vertices.len() >= MAX_VERTICES - 256
            || self.indices.len() >= MAX_INDICES - 768
    }

    pub fn draw_call_count(&self) -> u32     { self.frame_draw_calls }
    pub fn total_triangle_count(&self) -> u32 { self.frame_triangle_count }
}

// `bind_group_layout` is kept for potential future re-binding; suppress
// dead-code warning until the renderer wiring exposes it.
#[allow(dead_code)]
fn _bgl_used(b: &WgpuShapeBatch) -> &wgpu::BindGroupLayout { &b.bind_group_layout }
