//! The normal pass's batch: sprites drawn into the lighting pass's normal
//! buffer, each with its normal map (or a flat one) and its diffuse's alpha
//! as coverage - see `ecs::wgpu_systems::render_normals_wgpu`.
//!
//! Until Sep 2026 this was a **forward** pipeline that shaded
//! `lit_sprite` materials itself against a small fixed array of lights, drawn
//! after every other sprite whatever its z. The deferred point and spot
//! lights read the normal buffer now, so a normal-mapped sprite is an
//! ordinary sprite that sorts with the rest. The frame bind group still
//! carries the lights and occluder buffers the forward shader read; only the
//! view-projection is used.
//!
//! Vertex layout, per-quad index pattern, and `MAX_SPRITES` cap mirror the
//! default sprite batch so the rest of the renderer can stay agnostic.
//!
//! Entities without a `normal_map_id` get a 1×1 flat-normal fallback
//! texture (RGBA = 0.5, 0.5, 1.0) created via [`Self::flat_normal_texture`].

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec4};
use wgpu::util::DeviceExt;
use wgpu::TextureFormat;

use super::texture::WgpuTexture;
use super::lighting::{pack_occluders, OccluderUniform};

const LIT_SPRITE_WGSL: &str =
    include_str!("../../../../assets/shaders/wgsl/lit_sprite.wgsl");

const MAX_SPRITES:        usize = 8192;
const VERTICES_PER_SPRITE: usize = 4;
const INDICES_PER_SPRITE:  usize = 6;

/// Vertex layout — identical to `WgpuSpriteBatch::SpriteVertex`. Lit-sprite
/// draws use the same batched quad geometry as default sprites; only the
/// shader + bind groups differ.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LitSpriteVertex {
    pub position: [f32; 2],
    pub texcoord: [f32; 2],
    pub color:    [f32; 4],
}

/// Hard cap on simultaneously active lights per lit-sprite frame. Mirrors
/// the `MAX_LIT_SPRITE_LIGHTS` constant in the WGSL — must stay in sync.
pub const MAX_LIT_SPRITE_LIGHTS: usize = 16;

/// Per-frame globals — view-projection matrix, ambient color, light count.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LitSpriteGlobals {
    pub view_projection: [[f32; 4]; 4],
    /// `rgb` = ambient color added to every fragment, `a` = unused.
    pub ambient:         [f32; 4],
    /// `x` = active light count (cast to `u32` in shader), `yzw` unused.
    pub light_count:     [f32; 4],
}

/// One light entry. Layout is **byte-identical** to the `Light` struct in
/// `lit_sprite.wgsl` — 64 bytes including the trailing pad.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct LitSpriteLight {
    /// `xy` = world position, `z` = radius, `w` = falloff exponent.
    pub pos_radius_falloff: [f32; 4],
    /// `rgb` = color, `a` = intensity.
    pub color_intensity:    [f32; 4],
    /// Reserved for future spot-light support — unused for now.
    pub spot:               [f32; 4],
    /// Trailing pad to keep struct = 64 bytes.
    pub _pad:               [f32; 4],
}

/// Lit-sprite pipeline + the two bind-group layouts that surround it.
///
/// **Bind group 0** (per-frame, shared across all draws this frame):
///   - binding 0: [`LitSpriteGlobals`] uniform
///   - binding 1: `[LitSpriteLight; MAX_LIT_SPRITE_LIGHTS]` uniform
///
/// **Bind group 1** (per-draw, one per (diffuse, normal) texture pair):
///   - binding 0: diffuse texture
///   - binding 1: diffuse sampler
///   - binding 2: normal texture (use a flat (0.5,0.5,1) 1×1 fallback when absent)
///   - binding 3: normal sampler
use super::sprite::VertexArena;

/// The arena starts at one full batch of lit vertices and doubles from there.
/// See [`VertexArena`] for why one buffer at offsets replaces a buffer per
/// flush.
const LIT_INITIAL_ARENA_BYTES: u64 =
    (MAX_SPRITES * VERTICES_PER_SPRITE * std::mem::size_of::<LitSpriteVertex>()) as u64;

/// How many frames a cached texture bind group may go unused before it is
/// dropped; it holds its textures alive.
const LIT_BIND_GROUP_IDLE_FRAMES: u64 = 300;

/// The light-layer buffer's format: each pixel's `LightLayers` bits (the low
/// eight), written by the normal pass with no blending and read by the
/// lights - an integer, since a mask half one value and half another is no
/// mask at all.
pub const LAYER_BUFFER_FORMAT: TextureFormat = TextureFormat::R8Uint;

/// The normal buffer's format: data, not colour, so not sRGB - an encoded
/// normal goes in and comes out unchanged.
pub const NORMAL_BUFFER_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

pub struct WgpuLitSpritePipeline {
    /// Sprites drawn into the lighting pass's normal buffer
    /// (`fs_normal`, [`NORMAL_BUFFER_FORMAT`]) - see
    /// `ecs::wgpu_systems::render_normals_wgpu`.
    pub normal_pipeline:    wgpu::RenderPipeline,
    pub frame_bgl:          wgpu::BindGroupLayout,
    pub texture_bgl:        wgpu::BindGroupLayout,

    // Batching state — mirrors `WgpuSpriteBatch`.
    vertices_gpu:           VertexArena,
    /// The frame bind group binds three buffers that never change identity,
    /// so it is built once. It used to be built per flush.
    frame_bg:               Option<wgpu::BindGroup>,
    /// Texture bind groups by (diffuse id, normal id), with the frame they
    /// were last used; built per flush before.
    texture_bgs:            std::collections::HashMap<(u64, u64), (wgpu::BindGroup, u64)>,
    frame_index:            u64,
    index_buffer:           wgpu::Buffer,
    globals_buffer:         wgpu::Buffer,
    lights_buffer:          wgpu::Buffer,
    occluder_buffer:        wgpu::Buffer,

    vertices:               Vec<LitSpriteVertex>,
    sprite_count:           usize,

    pub frame_draw_calls:   u32,
    pub frame_sprite_count: u32,
    /// Quads refused because a run overflowed `MAX_SPRITES` without a flush;
    /// see `WgpuSpriteBatch::frame_dropped`.
    pub frame_dropped: u32,
    last_dropped_logged: u32,
}

impl WgpuLitSpritePipeline {
    pub fn new(device: &wgpu::Device, color_format: TextureFormat) -> Result<Self> {
        let frame_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("lit_sprite frame bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding:    0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding:    1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // binding 2: shared occluder set for cast shadows.
                wgpu::BindGroupLayoutEntry {
                    binding:    2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let texture_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("lit_sprite texture bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding:    0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Texture {
                        sample_type:    wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding:    1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding:    2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Texture {
                        sample_type:    wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding:    3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("lit_sprite.wgsl"),
            source: wgpu::ShaderSource::Wgsl(LIT_SPRITE_WGSL.into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lit_sprite layout"),
            bind_group_layouts: &[&frame_bgl, &texture_bgl],
            push_constant_ranges: &[],
        });

        let build = |entry: &str, format: TextureFormat, label: &str| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LitSpriteVertex>() as wgpu::BufferAddress,
                    step_mode:    wgpu::VertexStepMode::Vertex,
                    attributes:   &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x2,
                        2 => Float32x4,
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                // The normal, and the specular beside it (`fs_normal`).
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format,
                        blend:  Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format,
                        blend:  Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    // The light layers: replaced, never blended.
                    Some(wgpu::ColorTargetState {
                        format: LAYER_BUFFER_FORMAT,
                        blend:  None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
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
        let _ = color_format; // the normal buffer has its own format
        let normal_pipeline = build("fs_normal", NORMAL_BUFFER_FORMAT, "lit_sprite normal pipeline");

        let vertices_gpu = VertexArena::new(device, LIT_INITIAL_ARENA_BYTES, "WgpuLitSpriteBatch vertex arena");
        let mut indices: Vec<u16> = Vec::with_capacity(MAX_SPRITES * INDICES_PER_SPRITE);
        for i in 0..MAX_SPRITES as u16 {
            let base = i * 4;
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
        }
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:    Some("WgpuLitSpriteBatch indices"),
            contents: bytemuck::cast_slice(&indices),
            usage:    wgpu::BufferUsages::INDEX,
        });
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("WgpuLitSpriteBatch globals"),
            size:  std::mem::size_of::<LitSpriteGlobals>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let lights_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("WgpuLitSpriteBatch lights"),
            size:  (MAX_LIT_SPRITE_LIGHTS * std::mem::size_of::<LitSpriteLight>())
                       as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let occluder_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("WgpuLitSpriteBatch occluders"),
            size:  std::mem::size_of::<OccluderUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            normal_pipeline,
            frame_bgl,
            texture_bgl,
            vertices_gpu,
            frame_bg: None,
            texture_bgs: std::collections::HashMap::new(),
            frame_index: 0,
            index_buffer,
            globals_buffer,
            lights_buffer,
            occluder_buffer,
            vertices: Vec::with_capacity(MAX_SPRITES * VERTICES_PER_SPRITE),
            sprite_count: 0,
            frame_draw_calls: 0,
            frame_sprite_count: 0,
            frame_dropped: 0,
            last_dropped_logged: 0,
        })
    }

    /// Build a 1×1 flat-normal fallback texture (decoded normal = (0, 0, 1)).
    /// Uses `Rgba8Unorm` (NOT sRGB) so the encoded bytes round-trip exactly
    /// to the values the shader's `decode_normal` expects.
    pub fn flat_normal_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> WgpuTexture {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("lit_sprite flat normal"),
            size:            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            // Linear, NOT sRGB — normal-map data is not gamma-encoded.
            format:          wgpu::TextureFormat::Rgba8Unorm,
            usage:           wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats:    &[],
        });
        // RGBA = (128, 128, 255, 255) → sample (~0.5, 0.5, 1.0) → decode (0, 0, 1).
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture:   &texture,
                mip_level: 0,
                origin:    wgpu::Origin3d::ZERO,
                aspect:    wgpu::TextureAspect::All,
            },
            &[128u8, 128, 255, 255],
            wgpu::ImageDataLayout {
                offset:         0,
                bytes_per_row:  Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label:          Some("lit_sprite flat normal sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter:     wgpu::FilterMode::Nearest,
            min_filter:     wgpu::FilterMode::Nearest,
            mipmap_filter:  wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        WgpuTexture { texture, view: view.into(), sampler: sampler.into(), width: 1, height: 1, id: WgpuTexture::next_id(), sdf: false }
    }

    /// Reset state at the start of a new frame. See
    /// [`crate::renderer::backend::wgpu::sprite::WgpuSpriteBatch::begin_frame`]
    /// for full semantics. Call once per render; use [`Self::begin`]
    /// between texture groups within one frame.
    pub fn begin_frame(&mut self) {
        if self.frame_dropped != self.last_dropped_logged {
            if self.frame_dropped > 0 {
                log::warn!(
                    "lit sprite batch dropped {} quads last frame: one sprite emitted more than {MAX_SPRITES} quads",
                    self.frame_dropped
                );
            }
            self.last_dropped_logged = self.frame_dropped;
        }
        self.vertices.clear();
        self.sprite_count       = 0;
        self.frame_draw_calls   = 0;
        self.frame_sprite_count = 0;
        self.frame_dropped      = 0;
        self.vertices_gpu.begin_frame();
        self.frame_index        = self.frame_index.wrapping_add(1);
        let now = self.frame_index;
        self.texture_bgs.retain(|_, (_, used)| now.wrapping_sub(*used) <= LIT_BIND_GROUP_IDLE_FRAMES);
    }

    /// Reset between texture groups within a single frame. Clears queued
    /// vertices only — see [`Self::begin_frame`].
    pub fn begin(&mut self) {
        self.vertices.clear();
        self.sprite_count = 0;
    }

    /// Returns `true` when at `MAX_SPRITES` capacity.
    pub fn is_full(&self) -> bool {
        self.sprite_count >= MAX_SPRITES
    }

    /// Queue a sprite (anchor-aware, matches `WgpuSpriteBatch::draw_sprite_anchored`).
    pub fn draw_sprite_anchored(
        &mut self,
        position: Vec2,
        size:     Vec2,
        rotation: f32,
        color:    Vec4,
        uv_rect:  [f32; 4],
        anchor:   [f32; 2],
    ) {
        self.draw_sprite_anchored_skewed(position, size, rotation, 0.0, color, uv_rect, anchor);
    }

    /// [`Self::draw_sprite_anchored`] with a shear between the size and the
    /// rotation (matches `WgpuSpriteBatch::draw_sprite_anchored_skewed`), so
    /// a sheared sprite's normals land under its colour.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sprite_anchored_skewed(
        &mut self,
        position: Vec2,
        size:     Vec2,
        rotation: f32,
        skew_x:   f32,
        color:    Vec4,
        uv_rect:  [f32; 4],
        anchor:   [f32; 2],
    ) {
        if self.sprite_count >= MAX_SPRITES { self.frame_dropped += 1; return; }

        let left   = -anchor[0]         * size.x;
        let right  =  (1.0 - anchor[0]) * size.x;
        let bottom = -anchor[1]         * size.y;
        let top    =  (1.0 - anchor[1]) * size.y;
        let cos_r = rotation.cos();
        let sin_r = rotation.sin();

        let corners = [
            Vec2::new(left,  top),    // TL
            Vec2::new(right, top),    // TR
            Vec2::new(right, bottom), // BR
            Vec2::new(left,  bottom), // BL
        ];
        let [u_min, v_min, u_max, v_max] = uv_rect;
        let uvs = [
            [u_min, v_min], [u_max, v_min], [u_max, v_max], [u_min, v_max],
        ];

        for (i, corner) in corners.iter().enumerate() {
            let corner = Vec2::new(corner.x + skew_x * corner.y, corner.y);
            let rotated = Vec2::new(
                corner.x * cos_r - corner.y * sin_r,
                corner.x * sin_r + corner.y * cos_r,
            );
            self.vertices.push(LitSpriteVertex {
                position: (position + rotated).into(),
                texcoord: uvs[i],
                color:    color.into(),
            });
        }
        self.sprite_count += 1;
    }

    /// Upload the per-frame globals + lights uniforms. Call once per frame
    /// before any [`Self::flush`] calls. `lights` is truncated to
    /// `MAX_LIT_SPRITE_LIGHTS` and the count is recorded in `globals.light_count.x`.
    /// `occluders` / `occluder_tints` are the parallel shadow segment +
    /// tint slices (see [`crate::renderer::lighting::LightData`]); pass empty
    /// slices to disable cast shadows for this frame.
    pub fn upload_frame(
        &self,
        queue:           &wgpu::Queue,
        view_projection: &Mat4,
        ambient_rgb:     [f32; 3],
        lights:          &[LitSpriteLight],
        occluders:       &[[f32; 4]],
        occluder_tints:  &[[f32; 4]],
    ) {
        let n = lights.len().min(MAX_LIT_SPRITE_LIGHTS);
        let globals = LitSpriteGlobals {
            view_projection: view_projection.to_cols_array_2d(),
            ambient:         [ambient_rgb[0], ambient_rgb[1], ambient_rgb[2], 0.0],
            light_count:     [n as f32, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));

        // Pack lights into a fixed-size array. Unused slots stay zeroed
        // (intensity = 0 → contribute nothing even if the shader loop
        // touches them).
        let mut packed = [LitSpriteLight::default(); MAX_LIT_SPRITE_LIGHTS];
        for (dst, src) in packed.iter_mut().zip(lights.iter().take(n)) {
            *dst = *src;
        }
        queue.write_buffer(&self.lights_buffer, 0, bytemuck::cast_slice(&packed));

        // Shared occluder set for cast shadows.
        let occ = pack_occluders(occluders, occluder_tints, &[]);
        queue.write_buffer(&self.occluder_buffer, 0, bytemuck::bytes_of(&occ));
    }

    /// Flush queued sprites into the **normal buffer** - the pass
    /// `WgpuLightingPass::normal_pass` opens. Needs no
    /// [`Self::upload_frame`]: the shader reads only the view-projection,
    /// which this uploads itself.
    pub fn flush_normals<'pass>(
        &mut self,
        device:          &wgpu::Device,
        queue:           &wgpu::Queue,
        pass:            &mut wgpu::RenderPass<'pass>,
        view_projection: &Mat4,
        diffuse:         &WgpuTexture,
        normal:          &WgpuTexture,
    ) -> usize {
        self.upload_frame(queue, view_projection, [0.0; 3], &[], &[], &[]);
        self.flush_through(device, queue, pass, diffuse, normal)
    }

    fn flush_through<'pass>(
        &mut self,
        device:  &wgpu::Device,
        queue:   &wgpu::Queue,
        pass:    &mut wgpu::RenderPass<'pass>,
        diffuse: &WgpuTexture,
        normal:  &WgpuTexture,
    ) -> usize {
        let count = self.sprite_count;
        if count == 0 { return 0; }

        // This flush's vertices at their own offset in the frame's arena.
        let bytes = bytemuck::cast_slice::<LitSpriteVertex, u8>(&self.vertices);
        let range = self.vertices_gpu.push(device, queue, bytes);

        let frame_bg = self.frame_bg.get_or_insert_with(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label:   Some("lit_sprite frame bg"),
                layout:  &self.frame_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.globals_buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.lights_buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: self.occluder_buffer.as_entire_binding() },
                ],
            })
        });
        let now = self.frame_index;
        let (texture_bg, used) = self.texture_bgs.entry((diffuse.id, normal.id)).or_insert_with(|| {
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label:   Some("lit_sprite texture bg"),
                layout:  &self.texture_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&diffuse.view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&diffuse.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&normal.view) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&normal.sampler) },
                ],
            });
            (bg, now)
        });
        *used = now;

        pass.set_pipeline(&self.normal_pipeline);
        pass.set_bind_group(0, &*frame_bg, &[]);
        pass.set_bind_group(1, &*texture_bg, &[]);
        pass.set_vertex_buffer(0, self.vertices_gpu.buffer.slice(range));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..(count * INDICES_PER_SPRITE) as u32, 0, 0..1);

        self.frame_draw_calls   += 1;

        crate::profiling::trace::draw();
        self.frame_sprite_count += count as u32;
        self.vertices.clear();
        self.sprite_count = 0;
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test — verifies the WGSL parses and the pipeline builds end to
    /// end on a low-power adapter. Skipped silently if no adapter is
    /// available (e.g. headless CI without a wgpu backend).
    #[test]
    fn lit_sprite_pipeline_compiles() {
        let instance = wgpu::Instance::default();
        let adapter = match pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                ..Default::default()
            },
        )) {
            Some(a) => a,
            None => return,
        };
        let (device, _queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label:             Some("lit_sprite test device"),
                required_features: wgpu::Features::empty(),
                required_limits:   wgpu::Limits::downlevel_defaults(),
                memory_hints:      wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .expect("device");

        WgpuLitSpritePipeline::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb)
            .expect("lit sprite pipeline builds");
    }
}
