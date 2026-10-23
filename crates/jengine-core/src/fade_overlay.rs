//! Slice 12.A — fullscreen fade overlay for scene transition polish.
//!
//! Rendered after `WgpuPostProcessStack::execute` blits the scene to the
//! swapchain, so the overlay sits on top of every post-processed pixel
//! and isn't itself bloomed / vignetted / color-graded.
//!
//! State is driven by the script API `fade_in(duration, [r,g,b])`.
//! `fade_in` starts at fully-opaque `color` and tweens alpha down to 0
//! over `duration` seconds — call it from `on_ready` for a "reveal"
//! polish on every scene transition.

use anyhow::Result;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, Device, Queue, RenderPipeline, TextureFormat, TextureView,
};

/// Convert a `[f32; 4]` to its little-endian byte representation.
/// We avoid pulling in `bytemuck` just for one 16-byte uniform.
fn color_bytes(color: [f32; 4]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (i, v) in color.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&v.to_ne_bytes());
    }
    out
}

const FADE_WGSL: &str = r#"
struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    // Same fullscreen-triangle as blit.wgsl — no VBO required.
    let x = f32((i32(vertex_index) & 1) << 2) - 1.0;
    let y = f32((i32(vertex_index) & 2) << 1) - 1.0;
    var out: VsOut;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    return out;
}

struct FadeU { color: vec4<f32> };

@group(0) @binding(0) var<uniform> u: FadeU;

@fragment
fn fs_main(_in: VsOut) -> @location(0) vec4<f32> {
    return u.color;
}
"#;

/// Stateful transition the runner ticks each frame.
#[derive(Debug, Clone)]
pub struct TransitionState {
    /// `[r, g, b]` of the overlay; alpha is computed from `elapsed`.
    pub color: [f32; 3],
    /// Total tween duration in seconds.
    pub duration: f32,
    /// Time accumulated since the transition started.
    pub elapsed: f32,
    /// Direction. `FadeIn` starts opaque and reveals the scene;
    /// `FadeOut` starts clear and covers the scene.
    pub mode: TransitionMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransitionMode {
    /// Opaque → clear (reveal). Use in `on_ready`.
    FadeIn,
    /// Clear → opaque (hide). Use to cover an upcoming `load_scene`.
    FadeOut,
    /// P1.3 — Screen flash. Starts at `initial_alpha` and decays to 0 over
    /// `duration`. Same overlay pipeline as fade, but the initial alpha is
    /// caller-controlled (a 0.4 white flash is a "hit indicator"; a 0.8
    /// red flash is a "damage taken" punch).
    Flash { initial_alpha: f32 },
}

impl TransitionState {
    pub fn new_fade_in(duration: f32, color: [f32; 3]) -> Self {
        Self { color, duration: duration.max(0.0001), elapsed: 0.0, mode: TransitionMode::FadeIn }
    }
    pub fn new_fade_out(duration: f32, color: [f32; 3]) -> Self {
        Self { color, duration: duration.max(0.0001), elapsed: 0.0, mode: TransitionMode::FadeOut }
    }
    /// P1.3 — start a screen flash. `color` is RGB; `initial_alpha`
    /// (clamped to 0..=1) is the starting opacity that decays linearly to
    /// zero over `duration`.
    pub fn new_flash(duration: f32, color: [f32; 3], initial_alpha: f32) -> Self {
        Self {
            color,
            duration: duration.max(0.0001),
            elapsed: 0.0,
            mode: TransitionMode::Flash {
                initial_alpha: initial_alpha.clamp(0.0, 1.0),
            },
        }
    }
    /// Returns `true` when the transition has run to completion.
    pub fn is_done(&self) -> bool { self.elapsed >= self.duration }
    /// Premultiplied-alpha overlay color for this frame.
    pub fn current_color(&self) -> [f32; 4] {
        let t = (self.elapsed / self.duration).clamp(0.0, 1.0);
        let alpha = match self.mode {
            TransitionMode::FadeIn  => 1.0 - t,
            TransitionMode::FadeOut => t,
            TransitionMode::Flash { initial_alpha } => initial_alpha * (1.0 - t),
        };
        // Premultiplied so the standard-alpha blend at the bind site
        // composites `color * alpha + dst * (1 - alpha)`.
        [self.color[0] * alpha, self.color[1] * alpha, self.color[2] * alpha, alpha]
    }
}

pub struct FadeOverlay {
    pipeline: RenderPipeline,
    #[allow(dead_code)]
    bind_layout: BindGroupLayout,
    bind_group: BindGroup,
    uniform:  Buffer,
}

impl FadeOverlay {
    pub fn new(device: &Device, color_format: TextureFormat) -> Result<Self> {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("FadeOverlay"),
            source: wgpu::ShaderSource::Wgsl(FADE_WGSL.into()),
        });

        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label:              Some("FadeOverlay uniform"),
            size:               16,
            usage:              wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("FadeOverlay layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding:    0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("FadeOverlay bind"),
            layout:  &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding:  0,
                resource: uniform.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:                Some("FadeOverlay pipeline layout"),
            bind_group_layouts:   &[&bind_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("FadeOverlay pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module:              &module,
                entry_point:         Some("vs_main"),
                buffers:             &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module:      &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format:     color_format,
                    // Premultiplied source, standard alpha for dst.
                    blend:      Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation:  wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation:  wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology:           wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face:         wgpu::FrontFace::Ccw,
                cull_mode:          None,
                unclipped_depth:    false,
                polygon_mode:       wgpu::PolygonMode::Fill,
                conservative:       false,
            },
            depth_stencil: None,
            multisample:   wgpu::MultisampleState::default(),
            multiview:     None,
            cache:         None,
        });

        Ok(Self { pipeline, bind_layout, bind_group, uniform })
    }

    /// Composite the overlay onto `view` (typically the swapchain). The
    /// pass uses `LoadOp::Load` so existing pixels are preserved.
    pub fn render(
        &self,
        queue:   &Queue,
        encoder: &mut wgpu::CommandEncoder,
        view:    &TextureView,
        color:   [f32; 4],
    ) {
        queue.write_buffer(&self.uniform, 0, &color_bytes(color));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("FadeOverlay pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load:  wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set:      None,
            timestamp_writes:         None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_in_starts_opaque_and_ends_clear() {
        let mut s = TransitionState::new_fade_in(1.0, [1.0, 0.0, 0.0]);
        let start = s.current_color();
        assert!((start[3] - 1.0).abs() < 1e-4);
        s.elapsed = 1.0;
        let end = s.current_color();
        assert_eq!(end[3], 0.0);
        assert!(s.is_done());
    }

    #[test]
    fn flash_starts_at_initial_alpha_and_decays() {
        let mut s = TransitionState::new_flash(0.5, [1.0, 1.0, 1.0], 0.6);
        let start = s.current_color();
        assert!((start[3] - 0.6).abs() < 1e-4);
        // RGB is premultiplied.
        assert!((start[0] - 0.6).abs() < 1e-4);
        s.elapsed = 0.25; // half way
        let mid = s.current_color();
        assert!((mid[3] - 0.3).abs() < 1e-4);
        s.elapsed = 0.5;
        let end = s.current_color();
        assert_eq!(end[3], 0.0);
    }

    #[test]
    fn flash_clamps_initial_alpha() {
        let above = TransitionState::new_flash(1.0, [1.0; 3], 2.0);
        assert!((above.current_color()[3] - 1.0).abs() < 1e-4);
        let below = TransitionState::new_flash(1.0, [1.0; 3], -0.5);
        assert_eq!(below.current_color()[3], 0.0);
    }
}
