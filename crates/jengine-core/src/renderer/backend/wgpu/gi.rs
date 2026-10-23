//! 2D global illumination by **radiance cascades** (Oct 2026): light that
//! bounces. The lit outlines of the occluders - the scene's colour times the
//! direct light on them - shine again, and every pixel gathers what reaches
//! it round the occluders between, coloured by what it came off. Run by
//! [`super::WgpuLightingPass::render_lights`] after the direct lights, into
//! the same light buffer the composite multiplies by the scene - so a bounce
//! tints what it lands on, and no host was wired.
//!
//! All fragment passes (`global_illumination.wgsl`), so WebGL2 runs it too,
//! at a fraction of the frame's resolution ([`GlobalIllumination::scale`]):
//! the occluders' edges drawn thick into a seed texture; each edge texel's
//! light; jump flooding, which turns the seeds into a distance field the
//! rays sphere-trace through; the cascades from the top down, each probe's
//! rays over its interval merged with the cascade above (four times the
//! directions, twice the spacing, four times the reach); and cascade 0's
//! probes, averaged and interpolated, added to the light buffer.
//!
//! It needs a float world (`scene_format` `Rgba16Float`): the seeds are
//! texel coordinates. Without one it does nothing and says so once.
//! `light_at` does not see bounced light, as it does not see specular.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::framebuffer::WgpuRenderTarget;
use crate::ecs::components::GlobalIllumination;
use crate::renderer::camera::Camera2D;
use crate::renderer::lighting::LightData;

const SHADER: &str = include_str!("../../../../assets/shaders/wgsl/global_illumination.wgsl");
const GI_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Uniform slots, one per pass a frame can make (256-byte aligned).
const SLOTS: usize = 40;
const SLOT: u64 = 256;
/// The most cascades: enough to cross a 4K frame at a quarter of its pixels.
pub const MAX_CASCADES: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GiUniform {
    vp: [[f32; 4]; 4],
    size: [f32; 4],
    p: [f32; 4],
    ambient: [f32; 4],
}

/// How many cascades reach across `w` x `h` GI texels: the smallest count
/// whose last interval, `(4^n - 1) / 3`, passes the diagonal.
pub fn cascades_for(w: u32, h: u32) -> u32 {
    let diag = ((w * w + h * h) as f32).sqrt();
    let mut n = 1;
    while n < MAX_CASCADES && ((4f32.powi(n as i32) - 1.0) / 3.0) < diag {
        n += 1;
    }
    n
}

/// The GI texture's size for a frame of `w` x `h` at `scale`: a whole
/// number of the top cascade's probes each way, so every cascade's texture
/// is exactly twice it.
pub fn gi_size(w: u32, h: u32, scale: u32, cascades: u32) -> (u32, u32) {
    let s = scale.max(1);
    let unit = 1u32 << (cascades.saturating_sub(1)).min(MAX_CASCADES);
    let up = |v: u32| v.div_ceil(s).max(1).div_ceil(unit) * unit;
    (up(w), up(h))
}

pub struct GiPass {
    occluder: wgpu::RenderPipeline,
    source: wgpu::RenderPipeline,
    jfa: wgpu::RenderPipeline,
    cascade: wgpu::RenderPipeline,
    apply: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    uniform_bind: wgpu::BindGroup,
    tex_layout: wgpu::BindGroupLayout,
    dummy: wgpu::TextureView,
    seeds: [WgpuRenderTarget; 2],
    src: WgpuRenderTarget,
    cascades: [WgpuRenderTarget; 2],
    /// The frame size and settings the textures were made for.
    made_for: (u32, u32, u32, u32),
}

fn fullscreen(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    fs: &str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    label: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some("vs_full"), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

impl GiPass {
    /// `light_format` is the light buffer's, which [`Self::run`] adds into.
    pub fn new(device: &wgpu::Device, light_format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("global illumination"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gi uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<GiUniform>() as u64),
                },
                count: None,
            }],
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gi textures"),
            entries: &[tex(0), tex(1), tex(2)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gi layout"),
            bind_group_layouts: &[&uniform_layout, &tex_layout],
            push_constant_ranges: &[],
        });
        let occluder = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gi occluders"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_occluder"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 }],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_occluder"),
                targets: &[Some(wgpu::ColorTargetState { format: GI_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let source = fullscreen(device, &layout, &module, "fs_source", GI_FORMAT, None, "gi source");
        let jfa = fullscreen(device, &layout, &module, "fs_jfa", GI_FORMAT, None, "gi jump flood");
        let cascade = fullscreen(device, &layout, &module, "fs_cascade", GI_FORMAT, None, "gi cascade");
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let apply = fullscreen(device, &layout, &module, "fs_apply", light_format, Some(add), "gi apply");
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gi uniforms"),
            size: SLOT * SLOTS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gi uniform bind"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<GiUniform>() as u64),
                }),
            }],
        });
        let dummy = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("gi dummy"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: GI_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let target = |w, h| WgpuRenderTarget::new(device, w, h, GI_FORMAT).expect("a GI target");
        Self {
            occluder,
            source,
            jfa,
            cascade,
            apply,
            uniform,
            uniform_bind,
            tex_layout,
            dummy,
            seeds: [target(1, 1), target(1, 1)],
            src: target(1, 1),
            cascades: [target(2, 2), target(2, 2)],
            made_for: (0, 0, 0, 0),
        }
    }

    fn textures(&self, device: &wgpu::Device, views: [&wgpu::TextureView; 3]) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gi textures"),
            layout: &self.tex_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(views[0]) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(views[1]) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(views[2]) },
            ],
        })
    }

    /// Bounce the light: read the scene and the direct light, add the
    /// bounce into `light`. `frame` is the light buffer's size.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        camera: &Camera2D,
        lights: &LightData,
        gi: &GlobalIllumination,
        scene: &wgpu::TextureView,
        light: &WgpuRenderTarget,
    ) {
        let (fw, fh) = (light.width, light.height);
        let scale = gi.scale.clamp(1, 16);
        let first = gi_size(fw, fh, scale, 1);
        let count = if gi.cascades == 0 { cascades_for(first.0, first.1) } else { gi.cascades.clamp(1, MAX_CASCADES) };
        let (gw, gh) = gi_size(fw, fh, scale, count);
        if self.made_for != (fw, fh, scale, count) {
            let target = |w, h| WgpuRenderTarget::new(device, w, h, GI_FORMAT).expect("a GI target");
            self.seeds = [target(gw, gh), target(gw, gh)];
            self.src = target(gw, gh);
            self.cascades = [target(gw * 2, gh * 2), target(gw * 2, gh * 2)];
            self.made_for = (fw, fh, scale, count);
        }

        // The uniforms: slot 0 the shared one, then one per jump, then one
        // per cascade.
        let ambient = lights.ambient.color.map(|c| c * lights.ambient.intensity);
        let base = GiUniform {
            vp: camera.view_projection().to_cols_array_2d(),
            size: [gw as f32, gh as f32, fw as f32, fh as f32],
            p: [0.0, count as f32, 0.0, gi.strength.max(0.0)],
            ambient: [ambient[0], ambient[1], ambient[2], scale as f32],
        };
        let mut jumps = Vec::new();
        let mut k = gw.max(gh).next_power_of_two() / 2;
        while k >= 1 {
            jumps.push(k);
            k /= 2;
        }
        let mut bytes = vec![0u8; SLOT as usize * SLOTS];
        let mut put = |slot: usize, u: GiUniform| {
            let at = slot * SLOT as usize;
            bytes[at..at + std::mem::size_of::<GiUniform>()].copy_from_slice(bytemuck::bytes_of(&u));
        };
        put(0, base);
        for (j, k) in jumps.iter().enumerate() {
            put(1 + j, GiUniform { p: [0.0, count as f32, *k as f32, base.p[3]], ..base });
        }
        let cascade_slot = 1 + jumps.len();
        for c in 0..count {
            put(cascade_slot + c as usize, GiUniform { p: [c as f32, count as f32, 0.0, base.p[3]], ..base });
        }
        queue.write_buffer(&self.uniform, 0, &bytes);
        let offset = |slot: usize| (slot as u64 * SLOT) as u32;

        // 1. The occluders' edges, thick, as seeds. A texel and a half wide,
        // so a ray stepping half a texel cannot slip between.
        let world_per_texel = {
            let (vw, _) = camera.viewport_size();
            let zoom = if camera.zoom > 0.0 { camera.zoom } else { 1.0 };
            vw / zoom / fw.max(1) as f32 * scale as f32
        };
        let half = world_per_texel * 0.75;
        let mut verts: Vec<[f32; 2]> = Vec::with_capacity(lights.occluders.len() * 6);
        for e in &lights.occluders {
            let (a, b) = (glam::Vec2::new(e[0], e[1]), glam::Vec2::new(e[2], e[3]));
            let d = (b - a).normalize_or_zero();
            if d == glam::Vec2::ZERO {
                continue;
            }
            let n = glam::Vec2::new(-d.y, d.x) * half;
            let (a, b) = (a - d * half, b + d * half);
            let q = [a - n, b - n, b + n, a + n];
            for i in [0, 1, 2, 0, 2, 3] {
                verts.push(q[i].into());
            }
        }
        let empty = self.textures(device, [&self.dummy, &self.dummy, &self.dummy]);
        {
            let mut pass = self.seeds[0].begin_pass(encoder, Some(wgpu::Color::TRANSPARENT));
            if !verts.is_empty() {
                let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("gi occluder edges"),
                    contents: bytemuck::cast_slice(&verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                pass.set_pipeline(&self.occluder);
                // The frame's part of the (rounded-up) GI texture.
                pass.set_viewport(0.0, 0.0, fw as f32 / scale as f32, fh as f32 / scale as f32, 0.0, 1.0);
                pass.set_bind_group(0, &self.uniform_bind, &[offset(0)]);
                pass.set_bind_group(1, &empty, &[]);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.draw(0..verts.len() as u32, 0..1);
            }
        }
        // 2. Each edge texel's light.
        {
            let bind = self.textures(device, [scene, &light.view, &self.seeds[0].view]);
            let mut pass = self.src.begin_pass(encoder, Some(wgpu::Color::TRANSPARENT));
            pass.set_pipeline(&self.source);
            pass.set_bind_group(0, &self.uniform_bind, &[offset(0)]);
            pass.set_bind_group(1, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        // 3. Jump flooding, ping-ponging the seeds.
        let mut from = 0usize;
        for j in 0..jumps.len() {
            let to = 1 - from;
            let bind = self.textures(device, [&self.seeds[from].view, &self.dummy, &self.dummy]);
            let mut pass = self.seeds[to].begin_pass(encoder, Some(wgpu::Color::TRANSPARENT));
            pass.set_pipeline(&self.jfa);
            pass.set_bind_group(0, &self.uniform_bind, &[offset(1 + j)]);
            pass.set_bind_group(1, &bind, &[]);
            pass.draw(0..3, 0..1);
            drop(pass);
            from = to;
        }
        let field = from;
        // 4. The cascades, top down, each merged with the one above.
        let mut upper: Option<usize> = None;
        for c in (0..count).rev() {
            let to = match upper {
                Some(u) => 1 - u,
                None => 0,
            };
            let above = match upper {
                Some(u) => &self.cascades[u].view,
                None => &self.dummy,
            };
            let bind = self.textures(device, [&self.src.view, &self.seeds[field].view, above]);
            let mut pass = self.cascades[to].begin_pass(encoder, Some(wgpu::Color::TRANSPARENT));
            pass.set_pipeline(&self.cascade);
            pass.set_bind_group(0, &self.uniform_bind, &[offset(cascade_slot + c as usize)]);
            pass.set_bind_group(1, &bind, &[]);
            pass.draw(0..3, 0..1);
            drop(pass);
            upper = Some(to);
        }
        // 5. Cascade 0's probes into the light buffer.
        let zero = upper.unwrap_or(0);
        let bind = self.textures(device, [&self.cascades[zero].view, &self.dummy, &self.dummy]);
        let mut pass = light.begin_pass(encoder, None);
        pass.set_pipeline(&self.apply);
        pass.set_bind_group(0, &self.uniform_bind, &[offset(0)]);
        pass.set_bind_group(1, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascades_reach_across_the_frame() {
        // 320 x 180 at a quarter: the diagonal is ~367 texels, so six
        // cascades (the last reaches 1365).
        assert_eq!(cascades_for(320, 180), 6);
        assert_eq!(cascades_for(16, 16), 4);
        let (w, h) = gi_size(1280, 720, 4, 6);
        assert_eq!((w % 32, h % 32), (0, 0), "a whole number of the top cascade's probes");
        assert!(w >= 320 && h >= 180);
    }
}
