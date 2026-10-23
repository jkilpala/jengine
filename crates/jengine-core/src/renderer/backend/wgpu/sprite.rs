//! [`WgpuSpriteBatch`] — wgpu equivalent of [`crate::renderer::sprite::SpriteBatch`].
//!
//! API surface mirrors the legacy batcher: [`begin`], [`draw_sprite`],
//! [`draw_sprite_uv`], [`draw_sprite_anchored`], [`flush`]. Internals replace
//! the VAO/VBO/EBO + glow draw call with a wgpu vertex buffer + index buffer
//! + render pipeline + bind group.
//!
//! Pipeline + bind-group layouts are built once at construction. A frame's
//! vertices go into one arena buffer at increasing offsets, the globals
//! uniform is written once per distinct view-projection (once per pass), and
//! bind groups are cached by (globals slot, texture) - each of those used to
//! be per flush, which is per texture group per frame.
//!
//! Phase 3 covers basic sprite parity. Material/blend-mode variants and the
//! pipeline cache come in Phase 3.b once the runtime swap lands.

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec4};
use wgpu::util::DeviceExt;

use super::texture::WgpuTexture;

/// One vertex of anything the sprite pipeline draws.
///
/// `texcoord` is **projective**: `(u * q, v * q, q)`, and the fragment
/// stage divides by `q` after interpolation. A sprite, a glyph, a tile,
/// a shadow - everything rectangular - pushes `q = 1`, which makes the
/// division a no-op and the result identical to plain `(u, v)`. A
/// [`Self::draw_mesh`] caller with a trapezoid sets `q` per vertex to the
/// depth ratio of its edges, and the texture is mapped in perspective
/// instead of bending along the quad's diagonal. It is the trick hardware
/// used before it had a perspective divide for free, and it is the only
/// change to the vertex format the textured mesh needed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct SpriteVertex {
    position: [f32; 2],
    texcoord: [f32; 3],
    color:    [f32; 4],
}

/// One rectangle, drawn as an instance: the shader makes its four corners
/// (`sprite_instance.wgsl`). 68 bytes against four [`SpriteVertex`]es' 144,
/// and no corner arithmetic on the CPU. Every rectangle the batch draws -
/// [`WgpuSpriteBatch::draw_sprite_uv`], `draw_sprite_anchored` and
/// `draw_sprite_anchored_skewed` - is one; meshes, triangles and free quads
/// stay vertices.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct SpriteInstance {
    center: [f32; 2],
    /// Left, bottom, right, top, in the sprite's own frame.
    rect:   [f32; 4],
    /// cos and sin of the turn, and the horizontal shear.
    turn:   [f32; 3],
    uv:     [f32; 4],
    color:  [f32; 4],
}

impl SpriteInstance {
    /// The four vertices the CPU made for this rectangle before instances -
    /// the same arithmetic, so a flush through a pipeline with no instanced
    /// twin draws exactly what it always did.
    fn vertices(&self) -> [SpriteVertex; 4] {
        let [left, bottom, right, top] = self.rect;
        let [cos_r, sin_r, skew] = self.turn;
        let corners = [(left, top), (right, top), (right, bottom), (left, bottom)];
        let [u_min, v_min, u_max, v_max] = self.uv;
        let uvs = [[u_min, v_min], [u_max, v_min], [u_max, v_max], [u_min, v_max]];
        let mut out = [SpriteVertex::zeroed(); 4];
        for (i, &(cx, cy)) in corners.iter().enumerate() {
            let x = if skew != 0.0 { cx + skew * cy } else { cx };
            let rotated = Vec2::new(x * cos_r - cy * sin_r, x * sin_r + cy * cos_r);
            out[i] = SpriteVertex {
                position: (Vec2::from(self.center) + rotated).into(),
                texcoord: flat_uv(uvs[i]),
                color: self.color,
            };
        }
        out
    }
}

/// The sprite vertex layout, one [`SpriteVertex`] per vertex.
const VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: std::mem::size_of::<SpriteVertex>() as wgpu::BufferAddress,
    step_mode:    wgpu::VertexStepMode::Vertex,
    attributes:   &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x3, 2 => Float32x4],
};

/// The instanced layout, one [`SpriteInstance`] per sprite.
const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: std::mem::size_of::<SpriteInstance>() as wgpu::BufferAddress,
    step_mode:    wgpu::VertexStepMode::Instance,
    attributes:   &wgpu::vertex_attr_array![
        3 => Float32x2,
        4 => Float32x4,
        5 => Float32x3,
        6 => Float32x4,
        7 => Float32x4,
    ],
};

/// A pipeline's vertex stage: `vs_main` over [`VERTEX_LAYOUT`], or its
/// instanced twin, `vs_instanced` over [`INSTANCE_LAYOUT`].
fn sprite_vertex_state(module: &wgpu::ShaderModule, instanced: bool) -> wgpu::VertexState<'_> {
    wgpu::VertexState {
        module,
        entry_point: Some(if instanced { "vs_instanced" } else { "vs_main" }),
        compilation_options: Default::default(),
        buffers: if instanced { std::slice::from_ref(&INSTANCE_LAYOUT) } else { std::slice::from_ref(&VERTEX_LAYOUT) },
    }
}

/// Graph pipelines' instanced twins, by the vertex pipeline's address, with
/// a weak handle so an address a dropped pipeline left behind cannot answer
/// for a new one. Owned by the batch that built them - a thread-local here
/// kept GPU objects past their device and hung the thread's exit.
type Twins = std::collections::HashMap<usize, (std::sync::Weak<wgpu::RenderPipeline>, std::sync::Arc<wgpu::RenderPipeline>)>;

/// `(u, v)` with a weight of one: a flat vertex.
#[inline]
fn flat_uv(uv: [f32; 2]) -> [f32; 3] {
    [uv[0], uv[1], 1.0]
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct GlobalsUniform {
    view_projection: [[f32; 4]; 4],
    /// Seconds since runtime start in `.x`, `1` in `.y` when the flush draws
    /// cutouts ([`WgpuSpriteBatch::set_cutout`]); `.zw` reserved. Stored as a
    /// `vec4<f32>` so the WGSL uniform-address-space layout is
    /// unambiguous (an `f32` tail member would require trailing padding
    /// that wgpu/naga round up past our Rust struct size, producing
    /// "binding too small" validation errors).
    time:            [f32; 4],
    /// This flush's material parameters: what a graph's `MaterialParam`
    /// nodes read (`renderer::material_params`). Zero for any shader that
    /// declares none - a shader whose `Globals` stops at `time` still binds,
    /// since a uniform struct may be smaller than its buffer.
    params:          [[f32; 4]; 4],
    /// This flush's sprite mask; [`crate::renderer::mask::MaskUniform::NONE`]
    /// keeps everything. See [`WgpuSpriteBatch::set_mask`].
    mask:            crate::renderer::mask::MaskUniform,
}

const MAX_SPRITES:        usize = 8192;
const VERTICES_PER_SPRITE: usize = 4;
const INDICES_PER_SPRITE:  usize = 6;

/// WGSL source for the sprite shader (vertex + fragment in one module).
const SPRITE_WGSL: &str = concat!(
    include_str!("../../../../assets/shaders/wgsl/sprite.wgsl"),
    include_str!("../../../../assets/shaders/wgsl/sprite_mask.wgsl"),
    include_str!("../../../../assets/shaders/wgsl/sprite_instance.wgsl"),
);

/// The frame's vertices, in one buffer written at increasing offsets.
///
/// `queue.write_buffer` calls are coalesced before any command executes, so
/// two flushes in one submit must not write the same bytes - the second
/// would win for both draws. The batch used to answer that with **one
/// buffer per flush**, each pre-sized for a full batch (about 1.2 MB), so a
/// frame with forty texture groups held forty-eight megabytes of vertex
/// buffers to draw a few thousand sprites. Distinct *offsets* in one buffer
/// keep the flushes apart just as well: each flush writes at the cursor and
/// draws from its own slice.
///
/// When a frame needs more than the arena holds, a larger buffer replaces it
/// and the old one is kept in `retired` until the next `begin_frame` - the
/// pass may still reference it. wgpu keeps a buffer alive for the GPU after
/// the handle is dropped, so retiring is about this frame's encoder, not the
/// hardware.
pub(crate) struct VertexArena {
    pub(crate) buffer: wgpu::Buffer,
    capacity: u64,
    cursor:   u64,
    retired:  Vec<wgpu::Buffer>,
    label:    &'static str,
}

impl VertexArena {
    pub(crate) fn new(device: &wgpu::Device, capacity: u64, label: &'static str) -> Self {
        Self { buffer: Self::allocate(device, capacity, label), capacity, cursor: 0, retired: Vec::new(), label }
    }

    fn allocate(device: &wgpu::Device, capacity: u64, label: &'static str) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size:  capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Write `bytes` at the cursor and return the range they occupy in
    /// `self.buffer`, growing the arena first when they do not fit.
    pub(crate) fn push(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) -> std::ops::Range<u64> {
        let needed = bytes.len() as u64;
        if self.cursor + needed > self.capacity {
            let capacity = (self.capacity * 2).max(needed).max(INITIAL_ARENA_BYTES);
            let fresh = Self::allocate(device, capacity, self.label);
            self.retired.push(std::mem::replace(&mut self.buffer, fresh));
            self.capacity = capacity;
            self.cursor = 0;
        }
        let start = self.cursor;
        queue.write_buffer(&self.buffer, start, bytes);
        // The next write starts on a copy-aligned boundary.
        let align = wgpu::COPY_BUFFER_ALIGNMENT;
        self.cursor = (start + needed).div_ceil(align) * align;
        start..start + needed
    }

    pub(crate) fn begin_frame(&mut self) {
        self.cursor = 0;
        self.retired.clear();
    }
}

/// The arena starts at one full batch of vertices and doubles from there.
const INITIAL_ARENA_BYTES: u64 =
    (MAX_SPRITES * VERTICES_PER_SPRITE * std::mem::size_of::<SpriteVertex>()) as u64;

/// How many frames a cached bind group may go unused before it is dropped.
/// A bind group holds its texture alive, so an unbounded cache would keep
/// every texture a scene ever drew - including ones the asset manager has
/// since replaced - for the life of the batch.
const BIND_GROUP_IDLE_FRAMES: u64 = 300;

/// One globals (view-projection + time) uniform buffer per distinct value
/// in a frame. Same reasoning as [`VertexArena`]: `queue.write_buffer` calls
/// within one submit are coalesced and run before any draw, so two passes
/// that wrote a single uniform buffer would both read whichever wrote LAST. The PiP pass overwriting the main pass's view-projection
/// (sprites rendered through the game camera regardless of editor zoom)
/// is the canonical symptom.
struct GlobalsBufferSlot {
    buffer: wgpu::Buffer,
}

const GLOBALS_SLOT_BYTES: u64 = std::mem::size_of::<GlobalsUniform>() as u64;

/// Coverage-weighted multiply blend used by the shadow-flush pipeline.
///
/// The canonical sprite shader emits `texel * vertex_color`. For a shadow
/// with color `[r, g, b, a]` over an opaque sprite texel the fragment is
/// `(r*a-ish, g*a-ish, b*a-ish, a)`. We want
/// `result = dst * mix(1, src.rgb/src.a, src.a)`, i.e. the destination
/// darkened by `src.a` toward `src.rgb` where the sprite is opaque and
/// untouched where it is transparent. That decomposes to
///   `result = src.rgb*dst + (1 - src.a)*dst`
/// which is exactly `src_factor = Dst, dst_factor = OneMinusSrcAlpha`.
///
/// For the default shadow color `[0, 0, 0, 0.4]` over a fully-opaque
/// sprite pixel this gives `dst * 0.6` (i.e. 40% darker), and over a
/// fully-transparent pixel `dst * 1.0` (untouched). True
/// `BlendMode::Multiply` (`dst_factor = Zero`) would force the destination
/// to pure black wherever the sprite is opaque regardless of shadow
/// alpha, which is not what we want here. See the `shadow_multiply_blend`
/// test for the regression guard.
pub(crate) const SHADOW_MULTIPLY_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Dst,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation:  wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation:  wgpu::BlendOperation::Add,
    },
};

/// Batched sprite renderer for wgpu.
pub struct WgpuSpriteBatch {
    /// The canonical pipelines, one set per colour-target format, and which
    /// set flushes use now.
    ///
    /// One batch draws into two formats in a frame: the world into the
    /// scene target (`WgpuRenderer::scene_format`, a float format when HDR
    /// is available) and the UI into the swapchain (`color_format`) after
    /// the post chain. A wgpu pipeline is built for one format, so the batch
    /// keeps a set for each and the caller says which target it is drawing
    /// into with [`Self::set_target_format`]. Forgetting to is not silent:
    /// wgpu rejects a draw whose pipeline format does not match the pass.
    pipeline_sets:   Vec<(wgpu::TextureFormat, PipelineSet)>,
    current_set:     usize,
    /// The blend [`Self::flush`] draws with. `Alpha` unless a render pass
    /// says otherwise for a group - see [`Self::set_blend`].
    blend:           crate::renderer::material::BlendMode,
    shader:          wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    bind_group_layout: wgpu::BindGroupLayout,
    /// Group 1 for graphs that read the scene: the copy and its sampler.
    scene_layout: wgpu::BindGroupLayout,
    /// The last `capture_scene`, kept while its size and format hold.
    scene_copy: Option<SceneCopy>,
    vertices_gpu:    VertexArena,
    index_buffer:    wgpu::Buffer,
    /// One per *distinct* globals value in the frame - in practice one per
    /// pass (the picture-in-picture is a second) - not one per flush.
    globals_slots:   Vec<GlobalsBufferSlot>,
    /// How many globals slots this frame has written, and the value the
    /// last one holds, so a flush with the same view-projection and time
    /// reuses it instead of writing another.
    globals_used:    usize,
    last_globals:    Option<GlobalsUniform>,
    /// Bind groups by `(globals slot, texture id)`, with the frame they were
    /// last used. Building one per flush per frame was a wgpu object
    /// allocation per texture group per frame; the same texture drawn
    /// through the same pass tomorrow needs the same bind group.
    bind_groups:     std::collections::HashMap<(usize, u64, u64), (wgpu::BindGroup, u64)>,
    frame_index:     u64,

    vertices:        Vec<SpriteVertex>,
    sprite_count:    usize,
    /// The rectangles queued as instances, the arena they are uploaded
    /// into, and the order the two kinds were queued in: runs of
    /// `(instanced, count)`, so a flush draws them as queued.
    instances:       Vec<SpriteInstance>,
    instances_gpu:   VertexArena,
    runs:            Vec<(bool, u32)>,
    /// Whether flushes may draw instances (on unless a test turns it off).
    instancing:      bool,
    /// The instanced twins of the graph pipelines this batch built
    /// ([`Self::build_graph_pipeline`]).
    graph_twins:     std::sync::Mutex<Twins>,

    /// Seconds since runtime start, written into `globals.time` on every
    /// flush. The caller updates this once per frame via [`Self::set_time`];
    /// graph-driven materials read it as `globals.time`.
    time_seconds: f32,

    /// Material parameters written into `globals.params` on subsequent
    /// flushes. The sprite pass sets them per group - sprites whose
    /// parameters differ never share a flush. See [`Self::set_params`].
    params: [[f32; 4]; 4],
    /// The mask written into the globals on subsequent flushes.
    mask: crate::renderer::mask::MaskUniform,
    /// A texture mask's texture (`MaskShape::Texture`), kept from the flush
    /// that answered [`Self::request_mask_texture`]; bound as group 0's
    /// binding 3 until the mask changes.
    mask_texture: Option<(std::sync::Arc<wgpu::TextureView>, u64)>,
    /// The next flush hands over a mask texture instead of drawing.
    mask_texture_wanted: bool,
    /// The velocity pass's pipeline (`velocity.wgsl`), made the first time
    /// `motion_blur` asks for it.
    velocity_pipeline: Option<std::sync::Arc<wgpu::RenderPipeline>>,
    /// What binding 3 holds when there is no texture mask - one white texel
    /// - and the sampler every mask texture is read with (linear, clamped,
    /// so a mask drawn larger than it was painted stays soft). Made on the
    /// first flush, which has the queue.
    mask_blank: Option<(std::sync::Arc<wgpu::TextureView>, wgpu::Sampler)>,
    /// A GPU particle emitter to draw at the next flush instead of its
    /// quads (`particles::gpu`), and the pipelines that draw it.
    pending_gpu: Option<(crate::particles::gpu::GpuParticleUniform, u32, Option<u64>)>,
    gpu_particles: Option<super::gpu_particles::WgpuGpuParticles>,
    /// Stateful GPU particles (`particles::gpu_state`): unchecked, or what
    /// this device can do - `Some(None)` draws them in closed form.
    particle_states: Option<Option<super::gpu_particles::WgpuParticleStates>>,
    /// Whether flushes draw cutouts - read by graph materials
    /// ([`Self::set_cutout`]).
    cutout: bool,

    pub frame_draw_calls:   u32,
    pub frame_sprite_count: u32,
    /// Quads refused this frame because a run overflowed [`MAX_SPRITES`]
    /// without the caller flushing. Every render loop checks [`Self::is_full`]
    /// per sprite, so this only counts a *single* sprite whose fill emits
    /// more quads than a whole batch holds. It used to be silent: 16,000
    /// boids on one texture drew as 8,192 for as long as the swarm existed.
    pub frame_dropped: u32,
    /// The `frame_dropped` last reported, so a steady overflow is logged
    /// when it changes rather than every frame.
    last_dropped_logged: u32,
    /// The quads of a mesh that did not fit the batch (four vertices each),
    /// kept for the caller to draw after a flush - [`Self::take_overflow`].
    /// One mesh bigger than a batch (Prism Storm's grid and sparks) used to
    /// lose everything past 8,192 quads; [`Self::begin`] counts what nobody
    /// took as dropped.
    overflow: Vec<SpriteVertex>,
}

/// The quads of a mesh too big for one batch, between a flush and the next
/// ([`WgpuSpriteBatch::take_overflow`]).
pub struct MeshOverflow(Vec<SpriteVertex>);

impl WgpuSpriteBatch {
    /// Create a new sprite batch. `color_format` must match the swapchain
    /// (or render target) the batcher will draw into.
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Result<Self> {
        let vertices_gpu = VertexArena::new(device, INITIAL_ARENA_BYTES, "WgpuSpriteBatch vertex arena");

        // Pre-fill the static index buffer with `MAX_SPRITES` quads worth of indices.
        let mut indices: Vec<u16> = Vec::with_capacity(MAX_SPRITES * INDICES_PER_SPRITE);
        for i in 0..MAX_SPRITES as u16 {
            let base = i * 4;
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
        }
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:    Some("WgpuSpriteBatch indices"),
            contents: bytemuck::cast_slice(&indices),
            usage:    wgpu::BufferUsages::INDEX,
        });

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("WgpuSpriteBatch globals[0]"),
            size:  GLOBALS_SLOT_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("sprite bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding:    0,
                    // Visible to both stages: vertex uses `viewproj`,
                    // graph-generated fragment shaders read `time`.
                    // WebGPU/Dawn rejects FRAGMENT access if binding is
                    // VERTEX-only (native wgpu is lenient).
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
                    ty:         wgpu::BindingType::Texture {
                        sample_type:    wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding:    2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // A texture mask (`sprite_mask.wgsl`'s `mask_tex`, `mask_samp`).
                wgpu::BindGroupLayoutEntry {
                    binding:    3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Texture {
                        sample_type:    wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding:    4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("sprite.wgsl"),
            source: wgpu::ShaderSource::Wgsl(SPRITE_WGSL.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let set = PipelineSet::build(device, &shader, &pipeline_layout, color_format);

        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite scene bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        Ok(Self {
            scene_layout,
            scene_copy: None,
            pipeline_sets: vec![(color_format, set)],
            current_set: 0,
            blend: crate::renderer::material::BlendMode::Alpha,
            shader,
            pipeline_layout,
            bind_group_layout,
            vertices_gpu,
            index_buffer,
            globals_slots: vec![GlobalsBufferSlot { buffer: globals_buffer }],
            globals_used: 0,
            last_globals: None,
            bind_groups: std::collections::HashMap::new(),
            frame_index: 0,
            vertices: Vec::with_capacity(MAX_SPRITES * VERTICES_PER_SPRITE),
            sprite_count: 0,
            instances: Vec::with_capacity(MAX_SPRITES),
            instances_gpu: VertexArena::new(
                device,
                (MAX_SPRITES * std::mem::size_of::<SpriteInstance>()) as u64,
                "WgpuSpriteBatch instance arena",
            ),
            runs: Vec::new(),
            instancing: true,
            graph_twins: std::sync::Mutex::new(Twins::new()),
            time_seconds: 0.0,
            params: [[0.0; 4]; 4],
            mask: crate::renderer::mask::MaskUniform::NONE,
            mask_texture: None,
            mask_texture_wanted: false,
            velocity_pipeline: None,
            mask_blank: None,
            pending_gpu: None,
            gpu_particles: None,
            particle_states: None,
            cutout: false,
            frame_draw_calls: 0,
            frame_sprite_count: 0,
            frame_dropped: 0,
            last_dropped_logged: 0,
            overflow: Vec::new(),
        })
    }

    /// Reset state at the start of a new frame.
    ///
    /// Clears queued vertices **and** rewinds the per-flush vertex buffer
    /// pool cursor + frame counters. Call this **once** per render, before
    /// any [`Self::draw_sprite`] / [`Self::flush`] calls.
    ///
    /// Between texture groups inside the same frame, use [`Self::begin`]
    /// instead — it only clears the in-progress vertex queue so each
    /// flush keeps writing to its own GPU buffer (wgpu pre-submit
    /// coalesces all `queue.write_buffer` calls; reusing one buffer would
    /// make every draw read whichever flush wrote last).
    pub fn begin_frame(&mut self) {
        if self.frame_dropped != self.last_dropped_logged {
            if self.frame_dropped > 0 {
                log::warn!(
                    "sprite batch dropped {} quads last frame: one sprite emitted more than {MAX_SPRITES} quads",
                    self.frame_dropped
                );
            }
            self.last_dropped_logged = self.frame_dropped;
        }
        self.vertices.clear();
        self.instances.clear();
        self.runs.clear();
        self.instances_gpu.begin_frame();
        self.sprite_count       = 0;
        self.frame_draw_calls   = 0;
        self.frame_sprite_count = 0;
        self.frame_dropped      = 0;
        self.vertices_gpu.begin_frame();
        self.globals_used       = 0;
        self.last_globals       = None;
        self.frame_index        = self.frame_index.wrapping_add(1);
        let now = self.frame_index;
        self.bind_groups.retain(|_, (_, used)| now.wrapping_sub(*used) <= BIND_GROUP_IDLE_FRAMES);
    }

    /// Reset between texture groups within a single frame. Clears the
    /// in-progress vertex queue only — frame counters and the pool
    /// cursor are left intact. See [`Self::begin_frame`].
    pub fn begin(&mut self) {
        if !self.overflow.is_empty() {
            self.frame_dropped += (self.overflow.len() / 4) as u32;
            self.overflow.clear();
        }
        self.vertices.clear();
        self.instances.clear();
        self.runs.clear();
        self.sprite_count = 0;
    }

    /// Set the value written into `globals.time` on subsequent flushes.
    /// Drives time-based shaders (e.g. graph-built materials that use
    /// a `TimeInput` node). Call once per frame before any flushes.
    pub fn set_time(&mut self, seconds: f32) {
        self.time_seconds = seconds;
    }

    /// The clock [`Self::set_time`] last set - what rich text's effects
    /// move by (`renderer::rich_text`).
    pub fn time(&self) -> f32 {
        self.time_seconds
    }

    /// Clip subsequent flushes to a mask (`renderer::mask`), or
    /// [`MaskUniform::NONE`](crate::renderer::mask::MaskUniform::NONE) for
    /// none. Every sprite shader reads it, the default and every graph. The
    /// collectors set it per group and put it back, as they do the blend. A
    /// texture mask also needs its texture: [`Self::request_mask_texture`].
    pub fn set_mask(&mut self, mask: crate::renderer::mask::MaskUniform) {
        if mask.info[0] != 4.0 {
            self.mask_texture = None;
        }
        self.mask = mask;
    }

    /// Have the next flush hand over its texture as the mask's
    /// (`MaskShape::Texture`) instead of drawing. A collector sets a texture
    /// mask, calls this and then the host's own flush callback with the
    /// mask's texture id - so the host uploads and looks the texture up as it
    /// would for a sprite, and needs no wiring of its own. A host that drops
    /// the flush (the texture is not loaded yet) leaves the mask blank for
    /// the frame; [`Self::end_mask_texture_request`] makes sure the request
    /// does not swallow the next real flush.
    pub fn request_mask_texture(&mut self) {
        self.mask_texture_wanted = true;
    }

    /// Withdraw a [`Self::request_mask_texture`] no flush answered.
    pub fn end_mask_texture_request(&mut self) {
        self.mask_texture_wanted = false;
    }

    /// Whether a mask texture is bound now - for tests.
    pub fn has_mask_texture(&self) -> bool {
        self.mask_texture.is_some()
    }

    /// Draw a GPU particle emitter (`particles::gpu`) at the next flush, in
    /// place of the batch's quads, with the flush's texture, blend, target
    /// format and view-projection. The sprite collector sets it per emitter,
    /// so a host's flush closure draws it without knowing.
    /// A stateful emitter (`particles::gpu_state`) names its entity's key,
    /// and is drawn from the buffer `step_particle_states` keeps for it.
    pub fn set_gpu_particles(&mut self, draw: Option<(crate::particles::gpu::GpuParticleUniform, u32, Option<u64>)>) {
        self.pending_gpu = draw;
    }

    /// Step every stateful GPU particle emitter one frame
    /// (`particles::gpu_state`), in a compute pass on `encoder` - called
    /// once a frame by `ecs::frame_draw::draw_frame`, before any view.
    pub fn step_particle_states(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        emitters: &[crate::particles::gpu_state::StateEmitter],
    ) {
        if emitters.is_empty() && !matches!(self.particle_states, Some(Some(_))) {
            return;
        }
        let states = self.particle_states.get_or_insert_with(|| super::gpu_particles::WgpuParticleStates::try_new(device));
        if let Some(s) = states {
            s.step(device, queue, encoder, emitters);
        }
    }

    /// Whether this device steps stateful particles (false: they are drawn
    /// in closed form). Asks the device the first time.
    pub fn particle_states_supported(&mut self, device: &wgpu::Device) -> bool {
        self.particle_states
            .get_or_insert_with(|| super::gpu_particles::WgpuParticleStates::try_new(device))
            .is_some()
    }

    /// The particles of the stateful emitter `key`, read back from the GPU
    /// (for tests).
    pub fn read_particle_state(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: u64,
    ) -> Option<Vec<crate::particles::gpu_state::StateParticle>> {
        self.particle_states.as_ref()?.as_ref()?.read(device, queue, key)
    }

    /// Set the material parameters written into `globals.params` on
    /// subsequent flushes - what a graph's `MaterialParam` nodes read.
    /// The sprite pass calls this at the start of every group.
    pub fn set_params(&mut self, params: [[f32; 4]; 4]) {
        self.params = params;
    }

    /// Whether the next flushes draw **cutouts**: a graph material then draws
    /// black at its own alpha (`shader_codegen`'s fragment entries read it).
    /// The default sprite shader ignores it - a plain cutout is drawn black by
    /// its vertex colour. The world pass sets it per group and leaves it off.
    pub fn set_cutout(&mut self, cutout: bool) {
        self.cutout = cutout;
    }

    /// Drop currently queued (not-yet-flushed) sprite vertices without
    /// touching the per-frame counters. Used by host `flush_by_id` closures
    /// when the requested texture is missing — without this, the queued
    /// vertices would leak into the next `flush()` and be drawn against
    /// whatever texture that flush binds (most visibly: the font atlas).
    pub fn discard_pending(&mut self) {
        self.vertices.clear();
        self.instances.clear();
        self.runs.clear();
        self.sprite_count = 0;
    }

    /// Queue a sprite (full texture, centered).
    pub fn draw_sprite(&mut self, position: Vec2, size: Vec2, rotation: f32, color: Vec4) {
        self.draw_sprite_uv(position, size, rotation, color, [0.0, 0.0, 1.0, 1.0]);
    }

    /// Queue a single textured triangle (slice 3.5.E — skinned mesh
    /// rendering). The triangle is stored as a degenerate quad
    /// (`[p0, p1, p2, p2]`) so it shares the static quad index buffer
    /// — the second sub-triangle `(p0, p2, p2)` produces no fragments.
    /// This costs one quad slot (4 verts) per visible triangle, which
    /// keeps the slice E renderer additive without a new pipeline.
    pub fn draw_triangle(
        &mut self,
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        uv0: [f32; 2],
        uv1: [f32; 2],
        uv2: [f32; 2],
        color: Vec4,
    ) {
        if self.sprite_count >= MAX_SPRITES {
            self.frame_dropped += 1;
            return;
        }
        let color_arr: [f32; 4] = color.into();
        // Vertex 0..2: the real triangle.
        self.vertices.push(SpriteVertex {
            position: p0.into(),
            texcoord: flat_uv(uv0),
            color: color_arr,
        });
        self.vertices.push(SpriteVertex {
            position: p1.into(),
            texcoord: flat_uv(uv1),
            color: color_arr,
        });
        self.vertices.push(SpriteVertex {
            position: p2.into(),
            texcoord: flat_uv(uv2),
            color: color_arr,
        });
        // Vertex 3 = duplicate of p2 so the static index pattern
        // (0,1,2, 0,2,3) emits one real triangle + one degenerate.
        self.vertices.push(SpriteVertex {
            position: p2.into(),
            texcoord: flat_uv(uv2),
            color: color_arr,
        });
        self.sprite_count += 1;
        self.note(false);
    }

    /// Queue a textured triangle list with **projective** texture
    /// coordinates: each vertex is `(position, (u, v, q), colour)`, drawn
    /// with the texture divided by `q` per pixel - so a trapezoid whose
    /// far edge carries `q = far / near` is textured in perspective rather
    /// than bent along its diagonal. `q = 1` everywhere is an ordinary
    /// flat mesh.
    ///
    /// Consecutive index triples that share their first vertex and chain
    /// (`a b c, a c d`) take one quad slot; any other triangle takes a slot
    /// of its own as [`Self::draw_triangle`] does, so a fan is the cheap
    /// way to write a convex polygon. Indices out of range are skipped.
    /// Returns how many quad slots were used.
    pub fn draw_mesh(
        &mut self,
        positions: &[Vec2],
        uvq: &[[f32; 3]],
        colors: &[[f32; 4]],
        indices: &[u32],
    ) -> usize {
        let n = positions.len().min(uvq.len()).min(colors.len());
        let vertex = |i: usize| SpriteVertex { position: positions[i].into(), texcoord: uvq[i], color: colors[i] };
        let mut used = 0usize;
        let mut t = 0;
        while t + 2 < indices.len() {
            let (a, b, c) = (indices[t] as usize, indices[t + 1] as usize, indices[t + 2] as usize);
            if a >= n || b >= n || c >= n {
                t += 3;
                continue;
            }
            // Two triangles `a b c, a c d` are one quad `a b c d` under the
            // static index pattern `(0 1 2, 0 2 3)`.
            let quad_d = (t + 5 < indices.len()
                && indices[t + 3] as usize == a
                && indices[t + 4] as usize == c
                && (indices[t + 5] as usize) < n)
                .then(|| indices[t + 5] as usize);
            let d = match quad_d {
                Some(d) => {
                    t += 6;
                    d
                }
                // A lone triangle: the fourth corner repeats the third so
                // the second sub-triangle is degenerate and draws nothing.
                None => {
                    t += 3;
                    c
                }
            };
            if self.sprite_count >= MAX_SPRITES {
                self.overflow.extend([vertex(a), vertex(b), vertex(c), vertex(d)]);
                continue;
            }
            self.vertices.extend([vertex(a), vertex(b), vertex(c), vertex(d)]);
            self.sprite_count += 1;
            self.note(false);
            used += 1;
        }
        used
    }

    /// Whether a mesh left quads that did not fit ([`Self::draw_mesh`]).
    pub fn has_overflow(&self) -> bool {
        !self.overflow.is_empty()
    }

    /// The quads a mesh left over, for the caller to put back with
    /// [`Self::resume_overflow`] once it has flushed and begun again:
    ///
    /// ```ignore
    /// while batch.has_overflow() {
    ///     let rest = batch.take_overflow();
    ///     flush(batch); batch.begin();
    ///     batch.resume_overflow(rest);
    /// }
    /// ```
    pub fn take_overflow(&mut self) -> MeshOverflow {
        MeshOverflow(std::mem::take(&mut self.overflow))
    }

    /// Queue quads [`Self::take_overflow`] gave back - as many as fit; the
    /// rest stay over for the next round.
    pub fn resume_overflow(&mut self, quads: MeshOverflow) {
        let mut it = quads.0.chunks_exact(4);
        for q in it.by_ref() {
            if self.sprite_count >= MAX_SPRITES {
                self.overflow.extend_from_slice(q);
                continue;
            }
            self.vertices.extend_from_slice(q);
            self.sprite_count += 1;
            self.note(false);
        }
    }

    /// Returns `true` if the batch is at its `MAX_SPRITES` capacity. Callers
    /// emitting many sprites in a loop (e.g. particle systems) should check
    /// this and `flush(...)` mid-stream to avoid silently dropping draws.
    pub fn is_full(&self) -> bool {
        self.sprite_count >= MAX_SPRITES
    }

    /// Queue a sprite with custom UV rect.
    pub fn draw_sprite_uv(
        &mut self,
        position: Vec2,
        size: Vec2,
        rotation: f32,
        color: Vec4,
        uv_rect: [f32; 4],
    ) {
        if self.sprite_count >= MAX_SPRITES { self.frame_dropped += 1; return; }
        let half = size * 0.5;
        self.push_instance(position, [-half.x, -half.y, half.x, half.y], rotation, 0.0, color, uv_rect);
    }

    /// Queue a sprite with a custom anchor (pivot). `anchor = [0.5, 0.5]` is centered.
    pub fn draw_sprite_anchored(
        &mut self,
        position: Vec2,
        size: Vec2,
        rotation: f32,
        color: Vec4,
        uv_rect: [f32; 4],
        anchor: [f32; 2],
    ) {
        if self.sprite_count >= MAX_SPRITES { self.frame_dropped += 1; return; }
        let rect = [
            -anchor[0] * size.x,
            -anchor[1] * size.y,
            (1.0 - anchor[0]) * size.x,
            (1.0 - anchor[1]) * size.y,
        ];
        self.push_instance(position, rect, rotation, 0.0, color, uv_rect);
    }

    /// Queue a sprite with a custom anchor AND a horizontal shear applied
    /// before rotation. `skew_x` shifts each corner's local x by
    /// `skew_x * local_y`, so positive values lean the top of the quad
    /// to the right (matching the "stretched shadow" feel of an off-axis
    /// light source). With `skew_x = 0.0` this is identical to
    /// [`Self::draw_sprite_anchored`].
    pub fn draw_sprite_anchored_skewed(
        &mut self,
        position: Vec2,
        size: Vec2,
        rotation: f32,
        skew_x: f32,
        color: Vec4,
        uv_rect: [f32; 4],
        anchor: [f32; 2],
    ) {
        if self.sprite_count >= MAX_SPRITES { self.frame_dropped += 1; return; }
        let rect = [
            -anchor[0] * size.x,
            -anchor[1] * size.y,
            (1.0 - anchor[0]) * size.x,
            (1.0 - anchor[1]) * size.y,
        ];
        self.push_instance(position, rect, rotation, skew_x, color, uv_rect);
    }

    /// Queue one rectangle as an instance.
    fn push_instance(&mut self, position: Vec2, rect: [f32; 4], rotation: f32, skew: f32, color: Vec4, uv: [f32; 4]) {
        self.instances.push(SpriteInstance {
            center: position.into(),
            rect,
            turn: [rotation.cos(), rotation.sin(), skew],
            uv,
            color: color.into(),
        });
        self.sprite_count += 1;
        self.note(true);
    }

    /// One more quad of this kind, in queue order.
    fn note(&mut self, instanced: bool) {
        match self.runs.last_mut() {
            Some((kind, n)) if *kind == instanced => *n += 1,
            _ => self.runs.push((instanced, 1)),
        }
    }

    /// Draw rectangles as instances (`true`, the default) or as vertices the
    /// CPU makes - which a test compares against.
    pub fn set_instancing(&mut self, on: bool) {
        self.instancing = on;
    }

    /// The instanced twin of a pipeline this batch or a graph built, if it
    /// has one.
    fn twin_of(&self, pipeline: &wgpu::RenderPipeline) -> Option<std::sync::Arc<wgpu::RenderPipeline>> {
        for (_, set) in &self.pipeline_sets {
            if let Some(t) = set.twin_of(pipeline) {
                return Some(t);
            }
        }
        let key = pipeline as *const wgpu::RenderPipeline as usize;
        let twins = self.graph_twins.lock().unwrap_or_else(|e| e.into_inner());
        let (vertex, twin) = twins.get(&key)?;
        let alive = vertex.upgrade()?;
        std::ptr::eq(alive.as_ref(), pipeline).then(|| std::sync::Arc::clone(twin))
    }

    /// Build a graph material's pipeline against this batch's layouts - and,
    /// when its WGSL has `vs_instanced` (every generated graph does), its
    /// instanced twin, which this batch keeps so its flushes draw rectangles
    /// through the graph as instances. `GraphPipelineCache` builds through
    /// here; [`Self::build_custom_pipeline`] builds the pipeline alone.
    /// The velocity pass's pipeline: sprites drawn with their screen motion
    /// in the vertex colour, into a [`super::postprocess::VELOCITY_FORMAT`]
    /// target, where they cover (`velocity.wgsl`). Built once.
    pub fn velocity_pipeline(&mut self, device: &wgpu::Device) -> std::sync::Arc<wgpu::RenderPipeline> {
        if let Some(p) = &self.velocity_pipeline {
            return p.clone();
        }
        let (pipeline, _) = Self::build_pipeline_pair(
            device,
            super::postprocess::VELOCITY_FORMAT,
            &self.bind_group_layout,
            None,
            include_str!("../../../../assets/shaders/wgsl/velocity.wgsl"),
            crate::renderer::material::BlendMode::Alpha,
            Some("velocity pass"),
        );
        self.velocity_pipeline = Some(pipeline.clone());
        pipeline
    }

    pub fn build_graph_pipeline(
        &self,
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        reads_scene: bool,
        wgsl: &str,
        blend_mode: crate::renderer::material::BlendMode,
        label: Option<&str>,
    ) -> std::sync::Arc<wgpu::RenderPipeline> {
        let (pipeline, twin) = Self::build_pipeline_pair(
            device,
            color_format,
            &self.bind_group_layout,
            reads_scene.then_some(&self.scene_layout),
            wgsl,
            blend_mode,
            label,
        );
        if let Some(twin) = twin {
            let mut twins = self.graph_twins.lock().unwrap_or_else(|e| e.into_inner());
            twins.retain(|_, (vertex, _)| vertex.strong_count() > 0);
            twins.insert(std::sync::Arc::as_ptr(&pipeline) as usize, (std::sync::Arc::downgrade(&pipeline), twin));
        }
        pipeline
    }

    /// Queue a sprite drawn as an arbitrary quad defined by four local-space
    /// corner offsets (in order `[TL, TR, BR, BL]`) added to `position`. No
    /// anchor / size / rotation is applied — the caller supplies the exact
    /// geometry. `uv_rect` is mapped corner-to-corner (TL→u_min,v_min …
    /// BL→u_min,v_max). Used for faked perspective / trapezoid walls.
    pub fn draw_sprite_quad(
        &mut self,
        position: Vec2,
        corners: [[f32; 2]; 4],
        color: Vec4,
        uv_rect: [f32; 4],
    ) {
        if self.sprite_count >= MAX_SPRITES { self.frame_dropped += 1; return; }

        let [u_min, v_min, u_max, v_max] = uv_rect;
        let uvs = [
            [u_min, v_min], [u_max, v_min], [u_max, v_max], [u_min, v_max],
        ];
        for (i, corner) in corners.iter().enumerate() {
            self.vertices.push(SpriteVertex {
                position: [position.x + corner[0], position.y + corner[1]],
                texcoord: flat_uv(uvs[i]),
                color:    color.into(),
            });
        }
        self.sprite_count += 1;
        self.note(false);
    }

    /// Draw into targets of `format` from now on: every flush uses the
    /// pipelines built for it, built the first time a format is named.
    ///
    /// Call it when switching between the world (the scene target) and the
    /// UI (the swapchain) - see the `pipeline_sets` field. Cheap when the
    /// format is already current.
    pub fn set_target_format(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        if self.pipeline_sets[self.current_set].0 == format {
            return;
        }
        if let Some(i) = self.pipeline_sets.iter().position(|(f, _)| *f == format) {
            self.current_set = i;
            return;
        }
        let set = PipelineSet::build(device, &self.shader, &self.pipeline_layout, format);
        self.pipeline_sets.push((format, set));
        self.current_set = self.pipeline_sets.len() - 1;
    }

    /// Blend what [`Self::flush`] draws from now on with `blend`.
    ///
    /// The render passes set it per group from the sprite's material
    /// (`MaterialComponent.blend_mode`, or a built-in name like
    /// `"additive"`) or the emitter's `blend_mode`, and put it back to
    /// `Alpha` when the group is flushed. Until Sep 2026 nothing read a
    /// sprite's blend mode at all: `"blend_mode": "Additive"` drew
    /// alpha-blended in every runtime, and particles had no blend at all.
    /// A graph material's pipeline carries its own blend and ignores this.
    pub fn set_blend(&mut self, blend: crate::renderer::material::BlendMode) {
        self.blend = blend;
    }

    /// The blend [`Self::flush`] currently draws with.
    pub fn blend(&self) -> crate::renderer::material::BlendMode {
        self.blend
    }

    /// The format flushes currently draw into. Graph materials build their
    /// pipelines for it.
    pub fn target_format(&self) -> wgpu::TextureFormat {
        self.pipeline_sets[self.current_set].0
    }

    /// Borrow the sprite bind-group layout. Graph-driven materials build
    /// their own `wgpu::RenderPipeline` against this same layout so that
    /// `flush_with_pipeline()` can swap pipelines without rebinding.
    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    /// The layout of group 1 - the scene copy and its sampler - which a graph
    /// with a `SceneColor` node is built against as well as group 0.
    pub fn scene_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.scene_layout
    }

    /// Copy `source` - the target the world was just drawn into - so a graph
    /// that reads the scene (`SceneColor`) can sample it while drawing into
    /// that same target. Call between passes (a copy cannot happen inside
    /// one); every later `flush_with_pipeline` binds the copy as group 1.
    pub fn capture_scene(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) {
        let stale = self.scene_copy.as_ref().is_none_or(|c| c.size != size || c.format != format);
        if stale {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("WgpuSpriteBatch scene copy"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("WgpuSpriteBatch scene sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sprite scene bg"),
                layout: &self.scene_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                ],
            });
            self.scene_copy = Some(SceneCopy { texture, bind_group, size, format });
        }
        let copy = self.scene_copy.as_ref().expect("made above");
        encoder.copy_texture_to_texture(
            source.as_image_copy(),
            copy.texture.as_image_copy(),
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
    }

    /// Borrow the default sprite pipeline (used by [`Self::flush`]).
    pub fn default_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline_sets[self.current_set].1.alpha.0
    }

    /// Borrow the coverage-weighted multiply variant of the sprite
    /// pipeline. Pass it to [`Self::flush_with_pipeline`] to draw the
    /// queued sprites with `src.rgb*dst + (1 - src.a)*dst` blending — the
    /// shadow renderer uses this so shadows darken whatever they overlay
    /// by their alpha (`1 - src.a` is the "amount of dst kept"). The
    /// returned reference is suitable for passing as
    /// `&wgpu::RenderPipeline`.
    pub fn multiply_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline_sets[self.current_set].1.multiply.0
    }

    /// Convenience wrapper around [`Self::flush_with_pipeline`] that uses
    /// the built-in coverage-weighted multiply pipeline (see
    /// [`Self::multiply_pipeline`]). Keeps borrow-checker pain out of the
    /// call sites (a single `&mut self` call instead of juggling a pipeline
    /// reference alongside the mutable borrow).
    pub fn flush_multiply<'pass>(
        &mut self,
        device:          &wgpu::Device,
        queue:           &wgpu::Queue,
        pass:            &mut wgpu::RenderPass<'pass>,
        view_projection: &Mat4,
        texture:         &WgpuTexture,
    ) -> usize {
        let pipeline = std::sync::Arc::clone(&self.pipeline_sets[self.current_set].1.multiply.0);
        self.flush_with_pipeline(device, queue, pass, view_projection, texture, &pipeline)
    }

    /// [`Self::flush_with_pipeline`] with additive blending, so the queued
    /// quads brighten what is under them. Lightmaps use it to add baked
    /// light into the light buffer.
    pub fn flush_additive<'pass>(
        &mut self,
        device:          &wgpu::Device,
        queue:           &wgpu::Queue,
        pass:            &mut wgpu::RenderPass<'pass>,
        view_projection: &Mat4,
        texture:         &WgpuTexture,
    ) -> usize {
        let pipeline = std::sync::Arc::clone(&self.pipeline_sets[self.current_set].1.additive.0);
        self.flush_with_pipeline(device, queue, pass, view_projection, texture, &pipeline)
    }

    /// Build a custom render pipeline against the sprite vertex layout and
    /// bind-group layout, using the supplied WGSL source. This is the entry
    /// point for `.shader` graph-driven materials: codegen produces WGSL,
    /// the cache calls this helper to compile a pipeline, and rendering
    /// switches to it via [`Self::flush_with_pipeline`].
    ///
    /// The WGSL module MUST declare:
    ///   - `vs_main` vertex entry matching the sprite vertex layout
    ///     (location 0 = `vec2<f32>` position, 1 = `vec3<f32>` projective
    ///     uv - `(u * q, v * q, q)`, divide by `.z` in the fragment stage -
    ///     2 = `vec4<f32>` color), and
    ///   - `fs_main` fragment entry returning `vec4<f32>`,
    ///     with the same bind-group 0 layout (Globals UBO, texture, sampler)
    ///     as the canonical `sprite.wgsl`.
    pub fn build_custom_pipeline(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        bind_group_layout: &wgpu::BindGroupLayout,
        scene_layout: Option<&wgpu::BindGroupLayout>,
        wgsl: &str,
        blend_mode: crate::renderer::material::BlendMode,
        label: Option<&str>,
    ) -> std::sync::Arc<wgpu::RenderPipeline> {
        Self::build_pipeline_pair(device, color_format, bind_group_layout, scene_layout, wgsl, blend_mode, label).0
    }

    /// A pipeline and, when the shader has `vs_instanced`, its instanced twin.
    #[allow(clippy::type_complexity)]
    fn build_pipeline_pair(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        bind_group_layout: &wgpu::BindGroupLayout,
        scene_layout: Option<&wgpu::BindGroupLayout>,
        wgsl: &str,
        blend_mode: crate::renderer::material::BlendMode,
        label: Option<&str>,
    ) -> (std::sync::Arc<wgpu::RenderPipeline>, Option<std::sync::Arc<wgpu::RenderPipeline>>) {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label,
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });
        // A graph that reads the scene has the copy as group 1.
        let layouts: Vec<&wgpu::BindGroupLayout> = match scene_layout {
            Some(scene) => vec![bind_group_layout, scene],
            None => vec![bind_group_layout],
        };
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("graph sprite layout"),
            bind_group_layouts: &layouts,
            push_constant_ranges: &[],
        });
        let blend = blend_mode_to_wgpu(blend_mode);
        let make = |instanced: bool| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label,
            layout: Some(&pipeline_layout),
            vertex: sprite_vertex_state(&shader, instanced),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                // A multiply graph needs its colour premultiplied, as the
                // canonical multiply pipeline does.
                entry_point: Some(if blend_mode == crate::renderer::material::BlendMode::Multiply {
                    "fs_premultiplied"
                } else {
                    "fs_main"
                }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend:  Some(blend),
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
        // And its instanced twin - every generated graph has `vs_instanced`;
        // a hand-written shader without it has none, and its rectangles go
        // as vertices.
        let twin = wgsl.contains("fn vs_instanced(").then(|| std::sync::Arc::new(make(true)));
        (std::sync::Arc::new(make(false)), twin)
    }

    /// Upload buffered vertices and issue one draw call. Returns the sprite
    /// count that was flushed (matches the legacy `SpriteBatch::flush`).
    ///
    /// Resets the internal buffer so subsequent `draw_*` calls start a new
    /// batch.
    pub fn flush<'pass>(
        &mut self,
        device:          &wgpu::Device,
        queue:           &wgpu::Queue,
        pass:            &mut wgpu::RenderPass<'pass>,
        view_projection: &Mat4,
        texture:         &WgpuTexture,
    ) -> usize {
        // The default pipeline is stored in an `Arc`, so cloning is cheap
        // (just a refcount bump). This sidesteps the &self / &mut self
        // borrow conflict when delegating to `flush_with_pipeline`.
        let pipeline = std::sync::Arc::clone(self.pipeline_sets[self.current_set].1.for_blend(self.blend));
        self.flush_with_pipeline(device, queue, pass, view_projection, texture, &pipeline)
    }

    /// Same as [`Self::flush`] but uses a caller-supplied render pipeline.
    /// The pipeline MUST have been created against [`Self::bind_group_layout`]
    /// and the same vertex layout / color target as the default sprite
    /// pipeline — typically by `GraphPipelineCache` from a `.shader` graph.
    pub fn flush_with_pipeline<'pass>(
        &mut self,
        device:          &wgpu::Device,
        queue:           &wgpu::Queue,
        pass:            &mut wgpu::RenderPass<'pass>,
        view_projection: &Mat4,
        texture:         &WgpuTexture,
        pipeline:        &wgpu::RenderPipeline,
    ) -> usize {
        if self.mask_texture_wanted {
            // This flush answers `request_mask_texture`: keep its texture as
            // the mask's, and draw nothing.
            self.mask_texture_wanted = false;
            self.mask_texture = Some((texture.view.clone(), texture.id));
            return 0;
        }
        if let Some((mut u, n, state)) = self.pending_gpu.take() {
            u.view_projection = view_projection.to_cols_array_2d();
            let (format, blend) = (self.target_format(), self.blend);
            match (state, self.particle_states.as_mut()) {
                // Stepped: drawn from its buffer (nothing until its first step).
                (Some(key), Some(Some(states))) => {
                    if states.has(key) {
                        states.draw(device, queue, pass, key, &u, texture, format, blend);
                    }
                }
                // Closed form - or stateful on a device that cannot step.
                _ => {
                    let gp = self.gpu_particles.get_or_insert_with(|| super::gpu_particles::WgpuGpuParticles::new(device));
                    gp.draw(device, queue, pass, &u, n, texture, format, blend);
                }
            }
            self.frame_draw_calls += 1;
            crate::profiling::trace::draw();
            self.begin();
            return n as usize;
        }
        let count = self.sprite_count;
        if count == 0 { return 0; }

        // Instances go to the GPU as they are when the pipeline has an
        // instanced twin; otherwise they become the vertices the CPU always
        // made, in queue order, and the flush is what it always was.
        let twin = if self.instances.is_empty() || !self.instancing { None } else { self.twin_of(pipeline) };
        if twin.is_none() && !self.instances.is_empty() {
            let mut vertices = Vec::with_capacity(count * VERTICES_PER_SPRITE);
            let (mut v, mut n) = (0usize, 0usize);
            for &(instanced, len) in &self.runs {
                let len = len as usize;
                if instanced {
                    for inst in &self.instances[n..n + len] {
                        vertices.extend_from_slice(&inst.vertices());
                    }
                    n += len;
                } else {
                    vertices.extend_from_slice(&self.vertices[v..v + len * VERTICES_PER_SPRITE]);
                    v += len * VERTICES_PER_SPRITE;
                }
            }
            self.vertices = vertices;
            self.instances.clear();
            self.runs.clear();
        }

        // This flush's vertices go into the frame's arena at its own offset,
        // so the pre-submit coalescing of `write_buffer` cannot make a later
        // draw read them (see [`VertexArena`]).
        let range = (!self.vertices.is_empty())
            .then(|| self.vertices_gpu.push(device, queue, bytemuck::cast_slice::<SpriteVertex, u8>(&self.vertices)));
        let instance_range = (!self.instances.is_empty())
            .then(|| self.instances_gpu.push(device, queue, bytemuck::cast_slice::<SpriteInstance, u8>(&self.instances)));

        // The globals slot: reused while the view-projection and time are
        // what the last flush wrote, which within one pass they are. A pass
        // through another camera (the editor's picture-in-picture) gets a
        // fresh slot, because two flushes in one submit must not write one
        // uniform buffer - the last write would win for both.
        let globals = GlobalsUniform {
            view_projection: view_projection.to_cols_array_2d(),
            // `.z`: the texture is a distance field (the built-in font).
            time:            [self.time_seconds, if self.cutout { 1.0 } else { 0.0 }, if texture.sdf { 1.0 } else { 0.0 }, 0.0],
            params:          self.params,
            mask:            self.mask,
        };
        let globals_idx = if self.last_globals == Some(globals) && self.globals_used > 0 {
            self.globals_used - 1
        } else {
            let idx = self.globals_used;
            if idx >= self.globals_slots.len() {
                self.globals_slots.push(GlobalsBufferSlot {
                    buffer: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("WgpuSpriteBatch globals[+]"),
                        size:  GLOBALS_SLOT_BYTES,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                });
            }
            queue.write_buffer(&self.globals_slots[idx].buffer, 0, bytemuck::bytes_of(&globals));
            self.globals_used = idx + 1;
            self.last_globals = Some(globals);
            idx
        };

        // The bind group for (this globals slot, this texture, this mask
        // texture), built the first time the three are seen and kept while
        // they are in use.
        let (blank, mask_sampler) = self.mask_blank.get_or_insert_with(|| mask_blank(device, queue));
        let (mask_view, mask_id) = match &self.mask_texture {
            Some((view, id)) => (view.clone(), *id),
            None => (blank.clone(), u64::MAX),
        };
        let now = self.frame_index;
        let (bind_group, used) = self
            .bind_groups
            .entry((globals_idx, texture.id, mask_id))
            .or_insert_with(|| {
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label:   Some("sprite bg"),
                    layout:  &self.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: self.globals_slots[globals_idx].buffer.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&texture.view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&texture.sampler) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&mask_view) },
                        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(mask_sampler) },
                    ],
                });
                (bg, now)
            });
        *used = now;

        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &*bind_group, &[]);
        // Group 1 is the scene copy, which only a pipeline built against
        // `scene_bind_group_layout` reads; binding it for the others costs
        // nothing, and they ignore it.
        if let Some(copy) = &self.scene_copy {
            pass.set_bind_group(1, &copy.bind_group, &[]);
        }
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        match (&twin, instance_range) {
            // Every quad a vertex quad: one draw, as always.
            (None, _) | (_, None) => {
                if let Some(range) = range {
                    pass.set_vertex_buffer(0, self.vertices_gpu.buffer.slice(range));
                    pass.draw_indexed(0..(count * INDICES_PER_SPRITE) as u32, 0, 0..1);
                    self.frame_draw_calls += 1;
                    crate::profiling::trace::draw();
                }
            }
            // Runs in queue order: vertex quads through the pipeline, rectangles
            // through its twin, six indices of the first quad per instance.
            (Some(twin), Some(instance_range)) => {
                let (mut q, mut n) = (0u32, 0u32);
                let mut bound: Option<bool> = None;
                for &(instanced, len) in &self.runs {
                    if bound != Some(instanced) {
                        if instanced {
                            pass.set_pipeline(twin);
                            pass.set_vertex_buffer(0, self.instances_gpu.buffer.slice(instance_range.clone()));
                        } else {
                            pass.set_pipeline(pipeline);
                            if let Some(range) = range.clone() {
                                pass.set_vertex_buffer(0, self.vertices_gpu.buffer.slice(range));
                            }
                        }
                        bound = Some(instanced);
                    }
                    if instanced {
                        pass.draw_indexed(0..INDICES_PER_SPRITE as u32, 0, n..n + len);
                        n += len;
                    } else {
                        let first = q * INDICES_PER_SPRITE as u32;
                        pass.draw_indexed(first..first + len * INDICES_PER_SPRITE as u32, 0, 0..1);
                        q += len;
                    }
                    self.frame_draw_calls += 1;
                    crate::profiling::trace::draw();
                }
            }
        }

        self.frame_sprite_count += count as u32;
        self.vertices.clear();
        self.instances.clear();
        self.runs.clear();
        self.sprite_count = 0;
        count
    }
}

/// One white texel - what a sprite flush binds as its mask texture when it
/// has no texture mask - and the sampler mask textures are read with.
fn mask_blank(device: &wgpu::Device, queue: &wgpu::Queue) -> (std::sync::Arc<wgpu::TextureView>, wgpu::Sampler) {
    let size = wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sprite mask blank"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        &[255u8; 4],
        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("sprite mask sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    (std::sync::Arc::new(view), sampler)
}

/// A copy of the scene target for graphs that read it (`capture_scene`).
struct SceneCopy {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
    format: wgpu::TextureFormat,
}

/// The canonical sprite pipelines for one colour-target format: one per
/// [`BlendMode`](crate::renderer::material::BlendMode), plus the shadow
/// multiply. All built up front - a render pass picks a blend per group and
/// has no device to build one with.
struct PipelineSet {
    alpha:    (std::sync::Arc<wgpu::RenderPipeline>, std::sync::Arc<wgpu::RenderPipeline>),
    multiply_blend: (std::sync::Arc<wgpu::RenderPipeline>, std::sync::Arc<wgpu::RenderPipeline>),
    screen:   (std::sync::Arc<wgpu::RenderPipeline>, std::sync::Arc<wgpu::RenderPipeline>),
    premultiplied: (std::sync::Arc<wgpu::RenderPipeline>, std::sync::Arc<wgpu::RenderPipeline>),
    /// Coverage-weighted multiply: the shadow renderer
    /// (`render_shadows_wgpu`) darkens bright backgrounds by the shadow's
    /// alpha - `result = src.rgb*dst + (1 - src.a)*dst`, equivalent to
    /// lerping the destination toward `src.rgb` by `src.a`.
    multiply: (std::sync::Arc<wgpu::RenderPipeline>, std::sync::Arc<wgpu::RenderPipeline>),
    /// `src.rgb * src.a + dst` - what adds a lightmap into the light buffer.
    additive: (std::sync::Arc<wgpu::RenderPipeline>, std::sync::Arc<wgpu::RenderPipeline>),
}

impl PipelineSet {
    fn build(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        format: wgpu::TextureFormat,
    ) -> Self {
        // `fs_premultiplied` for the two multiply blends, whose arithmetic
        // assumes a premultiplied colour (see `SHADOW_MULTIPLY_BLEND`).
        // Each pipeline with its instanced twin (`vs_instanced`).
        let one = |blend: wgpu::BlendState, label: &'static str, entry: &'static str, instanced: bool| {
            std::sync::Arc::new(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label:  Some(label),
                layout: Some(layout),
                vertex: sprite_vertex_state(shader, instanced),
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend:  Some(blend),
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
            }))
        };
        let build = |blend: wgpu::BlendState, label: &'static str, entry: &'static str| {
            (one(blend, label, entry, false), one(blend, label, entry, true))
        };
        use crate::renderer::material::BlendMode;
        Self {
            alpha: build(wgpu::BlendState::ALPHA_BLENDING, "sprite pipeline", "fs_main"),
            multiply: build(SHADOW_MULTIPLY_BLEND, "sprite pipeline (shadow multiply)", "fs_premultiplied"),
            additive: build(blend_mode_to_wgpu(BlendMode::Additive), "sprite pipeline (additive)", "fs_main"),
            multiply_blend: build(
                blend_mode_to_wgpu(BlendMode::Multiply),
                "sprite pipeline (multiply)",
                "fs_premultiplied",
            ),
            screen: build(blend_mode_to_wgpu(BlendMode::Screen), "sprite pipeline (screen)", "fs_main"),
            premultiplied: build(
                blend_mode_to_wgpu(BlendMode::PremultipliedAlpha),
                "sprite pipeline (premultiplied)",
                "fs_main",
            ),
        }
    }

    fn for_blend(&self, blend: crate::renderer::material::BlendMode) -> &std::sync::Arc<wgpu::RenderPipeline> {
        use crate::renderer::material::BlendMode;
        match blend {
            BlendMode::Alpha => &self.alpha.0,
            BlendMode::Additive => &self.additive.0,
            BlendMode::Multiply => &self.multiply_blend.0,
            BlendMode::Screen => &self.screen.0,
            BlendMode::PremultipliedAlpha => &self.premultiplied.0,
        }
    }

    /// The instanced twin of one of this set's pipelines.
    fn twin_of(&self, pipeline: &wgpu::RenderPipeline) -> Option<std::sync::Arc<wgpu::RenderPipeline>> {
        [&self.alpha, &self.multiply_blend, &self.screen, &self.premultiplied, &self.multiply, &self.additive]
            .into_iter()
            .find(|(v, _)| std::ptr::eq(v.as_ref(), pipeline))
            .map(|(_, t)| std::sync::Arc::clone(t))
    }
}

/// Map a logical `BlendMode` to wgpu's `BlendState`. Kept here next to the
/// pipeline construction it feeds into.
pub(crate) fn blend_mode_to_wgpu(mode: crate::renderer::material::BlendMode) -> wgpu::BlendState {
    use crate::renderer::material::BlendMode;
    match mode {
        BlendMode::Alpha => wgpu::BlendState::ALPHA_BLENDING,
        BlendMode::Additive => wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation:  wgpu::BlendOperation::Add,
            },
            // Light added covers nothing, so the coverage (alpha) under it is
            // kept. It used to add, which ran a float target's alpha past 1 and
            // made a layer composited over the frame (`composite_over`, the
            // world above a Scene Color sprite) darken under every glow.
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation:  wgpu::BlendOperation::Add,
            },
        },
        // Coverage-weighted, as shadows are: `src.rgb * dst + (1 - src.a) *
        // dst`. The plain `src * dst` this was ignored alpha, so a sprite's
        // transparent texels (black, alpha 0) multiplied what was under
        // them to black - a multiply disc drew as a black square.
        BlendMode::Multiply => SHADOW_MULTIPLY_BLEND,
        BlendMode::Screen => wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrc,
                operation:  wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation:  wgpu::BlendOperation::Add,
            },
        },
        BlendMode::PremultipliedAlpha => wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard for `render_shadows_wgpu`'s blend pipeline. The
    /// shadow demo (`projects/engine-test-lab/assets/scenes/31-shadows.scene`)
    /// relies on this exact blend state to darken bright backgrounds by
    /// `(1 - shadow.alpha)`. A previous iteration used true multiply
    /// (`src*dst + 0*dst`) which combined with the canonical sprite shader
    /// (`texel * vertex_color`) forced the destination to pure black
    /// wherever the sprite was opaque, regardless of shadow alpha. If this
    /// test fails, double-check the math in the doc-comment on
    /// `SHADOW_MULTIPLY_BLEND` before "fixing" the test.
    #[test]
    fn shadow_multiply_blend_is_coverage_weighted_not_true_multiply() {
        assert_eq!(SHADOW_MULTIPLY_BLEND.color.src_factor, wgpu::BlendFactor::Dst);
        assert_eq!(
            SHADOW_MULTIPLY_BLEND.color.dst_factor,
            wgpu::BlendFactor::OneMinusSrcAlpha,
            "shadow blend must keep (1 - src.a) of dst — true multiply (Zero) \
             would force dst to black wherever the sprite is opaque",
        );
        assert_eq!(SHADOW_MULTIPLY_BLEND.color.operation, wgpu::BlendOperation::Add);

        // Alpha channel: shadows do not write meaningful alpha (the
        // framebuffer is already opaque post-clear), but we still pin the
        // factors so a refactor that drops them gets flagged.
        assert_eq!(SHADOW_MULTIPLY_BLEND.alpha.src_factor, wgpu::BlendFactor::Zero);
        assert_eq!(SHADOW_MULTIPLY_BLEND.alpha.dst_factor, wgpu::BlendFactor::One);
        assert_eq!(SHADOW_MULTIPLY_BLEND.alpha.operation, wgpu::BlendOperation::Add);
    }

    /// Spot-check the resulting fragment color for the cases the demo
    /// scene shows off. This mirrors `result = src.rgb*dst + (1-src.a)*dst`
    /// on the CPU so the doc-comment math stays honest.
    #[test]
    fn shadow_multiply_blend_math_matches_documented_formula() {
        // Mirrors what the GPU computes under SHADOW_MULTIPLY_BLEND.
        fn blend(src: [f32; 4], dst: [f32; 3]) -> [f32; 3] {
            let inv_a = 1.0 - src[3];
            [
                src[0] * dst[0] + inv_a * dst[0],
                src[1] * dst[1] + inv_a * dst[1],
                src[2] * dst[2] + inv_a * dst[2],
            ]
        }

        let yellow_bg = [1.0_f32, 0.88, 0.4];

        // Default shadow color [0,0,0,0.4] over an opaque sprite texel:
        // shader emits texel*color = (0,0,0,0.4), so the destination is
        // darkened by 40%.
        let frag_opaque = [0.0_f32, 0.0, 0.0, 0.4];
        let out = blend(frag_opaque, yellow_bg);
        let expected = [yellow_bg[0] * 0.6, yellow_bg[1] * 0.6, yellow_bg[2] * 0.6];
        for i in 0..3 {
            assert!(
                (out[i] - expected[i]).abs() < 1e-5,
                "opaque shadow pixel: got {:?} want {:?}", out, expected,
            );
        }

        // Transparent sprite texel (alpha 0) → fragment is (0,0,0,0),
        // destination must be left untouched.
        let frag_transparent = [0.0_f32, 0.0, 0.0, 0.0];
        let out = blend(frag_transparent, yellow_bg);
        for i in 0..3 {
            assert!(
                (out[i] - yellow_bg[i]).abs() < 1e-5,
                "transparent shadow pixel must not change dst: got {:?}", out,
            );
        }

        // Fully-opaque pitch-black shadow (alpha = 1.0): destination
        // collapses to pure black.
        let frag_black = [0.0_f32, 0.0, 0.0, 1.0];
        let out = blend(frag_black, yellow_bg);
        for i in 0..3 {
            assert!(out[i].abs() < 1e-5, "full-alpha black shadow → black: {:?}", out);
        }
    }
}
