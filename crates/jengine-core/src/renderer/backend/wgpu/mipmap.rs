//! Mipmaps made on the GPU (Oct 2026).
//!
//! [`texture_filter::mip_chain`](crate::renderer::texture_filter::mip_chain)
//! builds a linear-filtered texture's levels on the CPU, on the frame the
//! texture is first drawn - 32 ms of a 50 ms first frame for a showcase scene
//! in a browser, most of it one sky and one ground. This makes the same levels
//! on the GPU: a pass per level reading the level above with `textureLoad` -
//! the same 2x2 box, clamped at the edge - averaged in linear light and
//! weighted by alpha for colour (the sRGB texture decodes on load and the sRGB
//! target encodes on store), plainly for data.
//!
//! Each level is drawn into a scratch target and copied into the texture, not
//! drawn into the texture itself: reading one level of a texture while drawing
//! into another is a feedback loop on WebGL2.
//!
//! Alpha and data are rounded as the CPU rounds, and come out the CPU's bytes;
//! the GPU's sRGB encode may round a colour a step from the CPU's. A texture
//! changed at runtime takes the CPU's chain whole on its first change
//! (`AssetManager::patch_dirty`), so no level is ever a mix of the two.

use wgpu::util::DeviceExt;

const SHADER: &str = r#"
struct Level {
    // The level read from; the one drawn is the next.
    src: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var<uniform> level: Level;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle over the viewport.
    var corners = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(corners[i], 0.0, 1.0);
}

// A byte the CPU's way: `(v * 255 + 0.5) as u8`, ties up. Stored as an exact
// k/255 the target keeps it as it is; left to the store, a tie (an average of
// four bytes is a quarter step often enough) rounds to even, and a level a
// step off is read by the next, so the difference grew down the chain.
fn byte(v: f32) -> f32 {
    return floor(v * 255.0 + 0.5) / 255.0;
}

fn texel(x: u32, y: u32) -> vec4<f32> {
    let size = textureDimensions(tex, level.src);
    let at = vec2<i32>(i32(min(x, size.x - 1u)), i32(min(y, size.y - 1u)));
    return textureLoad(tex, at, i32(level.src));
}

// Colour: the average in linear light, weighted by alpha, so a transparent
// texel's colour - whatever the painter left there - puts no fringe round an
// edge. As `texture_filter::downsample`.
@fragment
fn fs_colour(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let x = u32(p.x) * 2u;
    let y = u32(p.y) * 2u;
    var rgb = vec3<f32>(0.0);
    var alpha = 0.0;
    for (var k = 0u; k < 4u; k = k + 1u) {
        let t = texel(x + k % 2u, y + k / 2u);
        rgb = rgb + t.rgb * t.a;
        alpha = alpha + t.a;
    }
    var out = vec3<f32>(0.0);
    if alpha > 0.0 {
        out = rgb / alpha;
    }
    // Colour goes through the target's sRGB encode, which may round a step
    // from the CPU's table; alpha is linear, and exact.
    return vec4<f32>(out, byte(alpha / 4.0));
}

// Data (a normal map): the plain average.
@fragment
fn fs_data(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let x = u32(p.x) * 2u;
    let y = u32(p.y) * 2u;
    var sum = vec4<f32>(0.0);
    for (var k = 0u; k < 4u; k = k + 1u) {
        sum = sum + texel(x + k % 2u, y + k / 2u);
    }
    let avg = sum / 4.0;
    return vec4<f32>(byte(avg.x), byte(avg.y), byte(avg.z), byte(avg.w));
}
"#;

/// How many levels a `width` x `height` texture's full chain has, the base
/// included - one more than [`texture_filter::mip_chain`](crate::renderer::texture_filter::mip_chain)
/// makes, which a texture patched at runtime is checked against.
pub fn mip_level_count(width: u32, height: u32) -> u32 {
    let (mut w, mut h, mut n) = (width, height, 1);
    while w > 1 || h > 1 {
        w = (w / 2).max(1);
        h = (h / 2).max(1);
        n += 1;
    }
    n
}

/// The pipelines that fill a texture's mip levels from its base. Made once
/// for a device and kept beside the textures it fills (`AssetManager`); never
/// in a thread-local, which would outlive the device.
pub struct MipGenerator {
    layout: wgpu::BindGroupLayout,
    colour: wgpu::RenderPipeline,
    data: wgpu::RenderPipeline,
}

impl MipGenerator {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("jengine.mipmap"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jengine.mipmap"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("jengine.mipmap"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = |entry: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("jengine.mipmap"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        Self {
            colour: pipeline("fs_colour", wgpu::TextureFormat::Rgba8UnormSrgb),
            data: pipeline("fs_data", wgpu::TextureFormat::Rgba8Unorm),
            layout,
        }
    }

    /// Fill every level of `texture` below its base from the base, which must
    /// already be written. `texture` is `Rgba8UnormSrgb` (colour) or
    /// `Rgba8Unorm` (data), made with `TEXTURE_BINDING | COPY_DST`.
    pub fn generate(&self, device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) {
        let levels = texture.mip_level_count();
        if levels < 2 {
            return;
        }
        let format = texture.format();
        let pipeline = if format.is_srgb() { &self.colour } else { &self.data };
        let (w1, h1) = ((texture.width() / 2).max(1), (texture.height() / 2).max(1));
        // The largest level drawn; every smaller one uses its corner.
        let scratch = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("jengine.mipmap.scratch"),
            size: wgpu::Extent3d { width: w1, height: h1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let scratch_view = scratch.create_view(&wgpu::TextureViewDescriptor::default());
        let source_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("jengine.mipmap") });
        let (mut w, mut h) = (texture.width(), texture.height());
        for level in 1..levels {
            w = (w / 2).max(1);
            h = (h / 2).max(1);
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("jengine.mipmap.level"),
                contents: bytemuck::cast_slice(&[level - 1, 0u32, 0, 0]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("jengine.mipmap"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source_view) },
                    wgpu::BindGroupEntry { binding: 1, resource: uniform.as_entire_binding() },
                ],
            });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("jengine.mipmap"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &scratch_view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_viewport(0.0, 0.0, w as f32, h as f32, 0.0, 1.0);
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_texture(
                wgpu::ImageCopyTexture { texture: &scratch, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::ImageCopyTexture { texture, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        }
        queue.submit(Some(encoder.finish()));
    }
}
