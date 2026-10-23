//! Phase 3.c-4 Step 6c — wgpu twin of [`crate::renderer::lighting::LightingPass`].
//!
//! **Architecture mirror of the gl pass:**
//! - Owns a separate `scene_buffer` (so the composite step can read the
//!   scene without a feedback loop) and a `light_buffer` (additive
//!   accumulation target, cleared to ambient).
//! - Two pipelines for light geometry: `point_pipeline` and
//!   `spot_pipeline`, both rendering a unit quad scaled to the light's
//!   radius with **additive blend** (`(ONE, ONE)`).
//! - One `composite_pipeline` that reads `scene_buffer` × `light_buffer`
//!   and writes the multiplied result to a caller-supplied destination
//!   view (typically `WgpuPostProcessStack::scene_target`).
//!
//! **Per-light data:** packed into one big uniform buffer with
//! per-light slots aligned to `min_uniform_buffer_offset_alignment`
//! (256 bytes — the WebGPU spec minimum and the wgpu default). Both
//! point and spot pipelines share the same `Light` WGSL struct so a
//! single buffer feeds both passes; we just bind it twice with
//! different dynamic offsets. Spot-only fields (direction, angle) are
//! present-but-ignored for point lights.
//!
//! Uses [`crate::renderer::lighting::LightData`] /
//! [`crate::renderer::lighting::gather_lights`] verbatim — those are
//! backend-agnostic and operate on `&hecs::World`.
//!
//! **Out of scope this slice:**
//! - Wiring into `wgpu_scene_runner` (Step 6d).
//! - Light culling against the camera frustum (gl path doesn't do this
//!   either; would be a future perf win for both backends).

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::framebuffer::WgpuRenderTarget;
use crate::renderer::camera::Camera2D;
use crate::renderer::lighting::LightData;

// Both lights append `light_shadow.wgsl`: one shadow, written once.
const POINT_LIGHT_WGSL: &str = concat!(
    include_str!("../../../../assets/shaders/wgsl/point_light.wgsl"),
    include_str!("../../../../assets/shaders/wgsl/light_shadow.wgsl"),
    include_str!("../../../../assets/shaders/wgsl/light_cookie.wgsl"),
    include_str!("../../../../assets/shaders/wgsl/light_specular.wgsl"),
);
const AMBIENT_OCCLUSION_WGSL: &str =
    include_str!("../../../../assets/shaders/wgsl/ambient_occlusion.wgsl");
const SPOT_LIGHT_WGSL: &str = concat!(
    include_str!("../../../../assets/shaders/wgsl/spot_light.wgsl"),
    include_str!("../../../../assets/shaders/wgsl/light_shadow.wgsl"),
    include_str!("../../../../assets/shaders/wgsl/light_cookie.wgsl"),
    include_str!("../../../../assets/shaders/wgsl/light_specular.wgsl"),
);
const LIGHT_COMPOSITE_WGSL: &str =
    include_str!("../../../../assets/shaders/wgsl/light_composite.wgsl");

/// Maximum lights of *each kind* per frame; see
/// [`crate::renderer::lighting::MAX_LIGHTS_PER_KIND`], which the occluder
/// packing honours too.
const MAX_LIGHTS_PER_KIND: usize = crate::renderer::lighting::MAX_LIGHTS_PER_KIND;

/// Light slots the per-light buffer starts with; it doubles as a frame
/// needs more.
const INITIAL_LIGHT_SLOTS: usize = 64;

/// The per-light uniform buffer and the bind group over it, grown on demand
/// so a scene pays for the lights it has rather than for the cap.
struct LightSlots {
    buffer:     wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    capacity:   usize,
}

impl LightSlots {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("WgpuLightingPass per-light uniform"),
            size:  LIGHT_STRIDE * capacity as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("WgpuLightingPass light bind"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding:  0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size:   std::num::NonZeroU64::new(std::mem::size_of::<LightUniform>() as u64),
                }),
            }],
        });
        Self { buffer, bind_group, capacity }
    }

    /// Room for `needed` lights: the next power of two past it.
    fn ensure(&mut self, device: &wgpu::Device, layout: &wgpu::BindGroupLayout, needed: usize) {
        if needed > self.capacity {
            *self = Self::new(device, layout, needed.next_power_of_two());
        }
    }
}

/// Per-light dynamic-offset stride. WebGPU spec minimum for
/// `min_uniform_buffer_offset_alignment` is 256, which is also wgpu's
/// adapter default. Hardcoding avoids a runtime query and is correct
/// on every adapter we target.
const LIGHT_STRIDE: u64 = 256;

/// The occluder pool uploaded per frame, shared by every light's range -
/// [`crate::renderer::lighting::OCCLUDER_POOL`]. Must match `OCCLUDER_POOL`
/// in the light shaders.
pub(crate) const MAX_OCCLUDER_SEGMENTS: usize = crate::renderer::lighting::OCCLUDER_POOL;

/// The occluder pool, which each light reads its own range of
/// (`LightUniform::occ`). Layout matches
/// the `Occluders` WGSL struct byte-for-byte: a `vec4<u32>` count
/// header (only `.x` used), `MAX_OCCLUDER_SEGMENTS` `vec4<f32>` segments
/// `[x0, y0, x1, y1]`, then `MAX_OCCLUDER_SEGMENTS` `vec4<f32>` shadow
/// tints `[r, g, b, opacity]` (parallel to the segments), then the edges'
/// owners (`LightData::occluder_owners`), four `u32`s to a `vec4<u32>`.
/// 16,144 bytes: inside the 16 KB a uniform buffer is guaranteed.
///
/// Built from [`crate::renderer::lighting::pack_light_occluders`], which is
/// also how the CPU's `light_at` sees it. (The lit-sprite batch still binds
/// one, from [`pack_occluders`], for a layout its shader no longer reads.)
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct OccluderUniform {
    count:     [u32; 4],
    segs:      [[f32; 4]; MAX_OCCLUDER_SEGMENTS],
    seg_tints: [[f32; 4]; MAX_OCCLUDER_SEGMENTS],
    seg_owner: [[u32; 4]; MAX_OCCLUDER_SEGMENTS / 4],
}

const _: () = assert!(std::mem::size_of::<OccluderUniform>() <= 16384, "a uniform buffer is only guaranteed 16 KB");

impl Default for OccluderUniform {
    fn default() -> Self {
        Self {
            count:     [0; 4],
            segs:      [[0.0; 4]; MAX_OCCLUDER_SEGMENTS],
            seg_tints: [[0.0; 4]; MAX_OCCLUDER_SEGMENTS],
            seg_owner: [[0; 4]; MAX_OCCLUDER_SEGMENTS / 4],
        }
    }
}

/// Pack parallel `occluders` (`[x0,y0,x1,y1]`) and `tints`
/// (`[r,g,b,opacity]`) slices into an [`OccluderUniform`], truncating to
/// [`MAX_OCCLUDER_SEGMENTS`]. Tints shorter than the segment list leave
/// the remainder zeroed (opaque-neutral).
pub(crate) fn pack_occluders(
    occluders: &[[f32; 4]],
    tints:     &[[f32; 4]],
    owners:    &[u32],
) -> OccluderUniform {
    let n_occ = occluders.len().min(MAX_OCCLUDER_SEGMENTS);
    let mut u = OccluderUniform {
        count: [n_occ as u32, 0, 0, 0],
        ..Default::default()
    };
    u.segs[..n_occ].copy_from_slice(&occluders[..n_occ]);
    let n_tint = tints.len().min(n_occ);
    u.seg_tints[..n_tint].copy_from_slice(&tints[..n_tint]);
    for (i, &o) in owners.iter().take(n_occ).enumerate() {
        u.seg_owner[i / 4][i % 4] = o;
    }
    u
}

/// Packed per-light uniform — 48 bytes of payload, padded to
/// [`LIGHT_STRIDE`] in the buffer. Layout matches the `Light` WGSL
/// struct in the three light shaders byte-for-byte.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
struct LightUniform {
    /// xy = world position, z = radius, w = falloff.
    pos_radius_falloff: [f32; 4],
    /// rgb = color, a = intensity.
    color_intensity:    [f32; 4],
    /// x = spot direction (radians), y = spot half-angle (radians),
    /// z = shadow softness, w = cast-shadows flag (this light's occluder
    /// range is not empty).
    spot:               [f32; 4],
    /// x = start, y = count: this light's range of the occluder pool;
    /// z = which self-shadow-free occluders contain the light
    /// (`PackedOccluders::point_inside`); w = `lighting::occluder_flags`:
    /// which of the shader's first-pass jobs this range needs.
    occ:                [u32; 4],
    /// x = height (the tangent of the light's elevation, for normal maps),
    /// y = the most a face turned to it may take relative to flat.
    extra:              [f32; 4],
    /// x = the light layers it lights (`light_mask`, `on_light_layers`).
    layers:             [u32; 4],
}

/// The composite's uniform: how bright light may get (`max_light`), padded
/// to 16 bytes.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CompositeUniform {
    max_light: [f32; 4],
    /// x: toon lighting's dither cell in world units (0 is none).
    dither: [f32; 4],
    /// Screen to world, for the dither pattern: the inverse of the camera's
    /// view-projection.
    inv_view_projection: [[f32; 4]; 4],
    /// The fog (`ecs::fog::PlacedFog`): colour and density (density 0 is
    /// none), shape, drift and scatter, and its box.
    fog: [[f32; 4]; 4],
}

impl CompositeUniform {
    /// As an 8-bit light buffer always did: light stops at 1.0.
    const LDR: Self =
        Self { max_light: [1.0, 0.0, 0.0, 0.0], dither: [0.0; 4], inv_view_projection: IDENTITY, fog: [[0.0; 4]; 4] };
    /// Effectively unbounded - half-float tops out at 65504.
    const HDR: Self =
        Self { max_light: [65000.0, 0.0, 0.0, 0.0], dither: [0.0; 4], inv_view_projection: IDENTITY, fog: [[0.0; 4]; 4] };
}

const IDENTITY: [[f32; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct VpUniform {
    view_projection: [[f32; 4]; 4],
}

/// `ambient_occlusion.wgsl`'s `Ao`: clip space back to world, and
/// `[radius, strength, target width, target height]`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct AoUniform {
    inv_view_projection: [[f32; 4]; 4],
    params: [f32; 4],
}

/// 2D lighting pipeline for the wgpu backend.
pub struct WgpuLightingPass {
    scene_buffer: WgpuRenderTarget,
    light_buffer: WgpuRenderTarget,
    /// The lights' highlights (`light_specular.wgsl`), summed beside the
    /// light buffer by the same pass and **added** by the composite after
    /// the multiply - a highlight sits on top of a surface, so it is not
    /// darkened by the surface's colour.
    specular_light: WgpuRenderTarget,
    /// The world's surface normals, which the point and spot lights read
    /// (`normal_response` in their shaders). Drawn by
    /// `ecs::wgpu_systems::render_normals_wgpu` into [`Self::normal_pass`];
    /// cleared flat by [`Self::render_lights`] on any frame nothing did.
    normal_buffer: WgpuRenderTarget,
    /// Each surface's specular strength and shininess, written beside the
    /// normals by the same pass (`fs_normal`) and read by
    /// `light_specular.wgsl`; cleared to nothing with them.
    specular_buffer: WgpuRenderTarget,
    /// Each pixel's **light layers** (`LightLayers`), written beside the
    /// normals by the same pass with no blending - a mask cannot be half one
    /// thing - and read by the lights (`on_light_layers`). Cleared to every
    /// layer, so what the pass does not draw is lit by every light.
    layer_buffer: WgpuRenderTarget,
    normal_bgl:    wgpu::BindGroupLayout,
    /// The normals and a white cookie: what a light without a cookie binds.
    normal_bind:   wgpu::BindGroup,
    /// A 1x1 white texture and its sampler, the cookie of every light that
    /// has none.
    white_cookie:  (wgpu::Texture, wgpu::TextureView, wgpu::Sampler),
    white_written: std::cell::Cell<bool>,
    /// This frame's cookies (`PointLight2D::cookie`), each bound beside the
    /// normals: built by [`Self::prepare_cookies`], read by
    /// [`Self::render_lights`]. A light whose cookie is not here (its
    /// picture not loaded yet) is not drawn rather than drawn uncut.
    cookie_binds:  std::cell::RefCell<std::collections::HashMap<String, wgpu::BindGroup>>,
    /// Whether [`Self::normal_pass`] ran this frame. Reset by
    /// [`Self::render_lights`].
    normals_drawn: std::cell::Cell<bool>,
    /// `Unlit` things, drawn in z order with the world's lit ones
    /// (`ecs::wgpu_systems::WorldLayer::Emission`) and added by the composite
    /// after the light multiply - so they are unlit *and* hidden by lit
    /// things in front of them. Cleared by [`Self::composite`] on a frame
    /// nothing drew into it.
    emission_buffer: WgpuRenderTarget,
    emission_drawn:  std::cell::Cell<bool>,
    /// The occluder-pool overflow last logged, so a steady overflow is
    /// reported when it changes rather than every frame.
    last_dropped:  std::cell::Cell<usize>,

    color_format: wgpu::TextureFormat,

    // Light geometry pipelines (additive blend, share the same vertex
    // buffer + uniform layout).
    point_pipeline: wgpu::RenderPipeline,
    spot_pipeline:  wgpu::RenderPipeline,
    // The same, taking light away (`subtract` on a light).
    point_sub_pipeline: wgpu::RenderPipeline,
    spot_sub_pipeline:  wgpu::RenderPipeline,

    // Unit quad in [-1, 1] for light volumes.
    quad_vertex_buffer: wgpu::Buffer,
    quad_index_buffer:  wgpu::Buffer,

    // VP uniform (group 0) — rewritten each frame.
    vp_buffer:     wgpu::Buffer,
    vp_bind_group: wgpu::BindGroup,

    // Per-light dynamic-offset uniform (group 1).
    light_bgl:            wgpu::BindGroupLayout,
    light_slots:          std::cell::RefCell<LightSlots>,

    // Occluder segments (group 2, point pipeline only) — rewritten each
    // frame from `LightData::occluders`.
    occluder_buffer:     wgpu::Buffer,
    occluder_bind_group: wgpu::BindGroup,

    /// Ambient occlusion (`ambient_occlusion.wgsl`): a fullscreen multiply
    /// over the ambient-cleared light buffer, its own uniform and its own
    /// copy of the occluder pool (the occluders near the view, whole).
    ao_pipeline:    wgpu::RenderPipeline,
    ao_uniform:     wgpu::Buffer,
    ao_occluders:   wgpu::Buffer,
    ao_bind_group:  wgpu::BindGroup,
    /// Occluders ambient occlusion last had no room for, logged on change.
    ao_last_dropped: std::cell::Cell<usize>,

    // Composite pipeline + cached bind group.
    composite_pipeline:           wgpu::RenderPipeline,
    composite_over_pipeline:      wgpu::RenderPipeline,
    composite_bind_group_layout:  wgpu::BindGroupLayout,
    composite_sampler:            wgpu::Sampler,
    /// Cached `(scene_buffer.view, light_buffer.view)` bind group.
    /// Rebuilt on `resize` because the underlying views are recreated.
    composite_bind_group: wgpu::BindGroup,
    /// `max_light` for the composite - see [`Self::set_hdr`].
    composite_uniform:    wgpu::Buffer,
    /// Light that bounces ([`super::gi`]), made the first frame a scene
    /// asks for it.
    gi: std::cell::RefCell<Option<super::gi::GiPass>>,
    /// Said once: a frame not drawn in floats cannot bounce light.
    gi_refused: std::cell::Cell<bool>,
}

impl WgpuLightingPass {
    /// Build the lighting pass. `color_format` is the format the world is
    /// drawn in - `WgpuRenderer::scene_format` - which the scene buffer, the
    /// light buffer and the composite's destination all share.
    pub fn new(
        device:       &wgpu::Device,
        width:        u32,
        height:       u32,
        color_format: wgpu::TextureFormat,
    ) -> Result<Self> {
        let scene_buffer = WgpuRenderTarget::new(device, width, height, color_format)?;
        let light_buffer = WgpuRenderTarget::new(device, width, height, color_format)?;
        let specular_light = WgpuRenderTarget::new(device, width, height, color_format)?;
        let normal_buffer = WgpuRenderTarget::new(device, width, height, super::lit_sprite::NORMAL_BUFFER_FORMAT)?;
        let specular_buffer = WgpuRenderTarget::new(device, width, height, super::lit_sprite::NORMAL_BUFFER_FORMAT)?;
        let layer_buffer = WgpuRenderTarget::new(device, width, height, super::lit_sprite::LAYER_BUFFER_FORMAT)?;
        let emission_buffer = WgpuRenderTarget::new(device, width, height, color_format)?;
        // The normals, and the light's cookie beside them (`light_cookie.wgsl`):
        // WebGPU guarantees four bind groups and the lights use all four.
        let normal_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("WgpuLightingPass normal + cookie bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding:    0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type:    wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding:    1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type:    wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding:    2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // The specular beside the normals (`light_specular.wgsl`).
                wgpu::BindGroupLayoutEntry {
                    binding:    3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type:    wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
                // Each pixel's light layers (`on_light_layers`).
                wgpu::BindGroupLayoutEntry {
                    binding:    4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type:    wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
            ],
        });
        // Filled with white by the first `render_lights`, which has the queue
        // (wgpu starts a texture zeroed - a black cookie would stop every light).
        let white_cookie = {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("WgpuLightingPass white cookie"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
            (texture, view, sampler)
        };
        let normal_bind =
            make_normal_bind(device, &normal_bgl, [&normal_buffer, &specular_buffer, &layer_buffer], &white_cookie.1, &white_cookie.2);

        // ---------- Quad geometry (unit quad [-1, 1]) ----------
        let quad_vertices: [[f32; 2]; 4] = [
            [-1.0, -1.0],
            [ 1.0, -1.0],
            [ 1.0,  1.0],
            [-1.0,  1.0],
        ];
        let quad_indices: [u16; 6] = [0, 1, 2, 2, 3, 0];

        let quad_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:    Some("WgpuLightingPass quad vertices"),
            contents: bytemuck::cast_slice(&quad_vertices),
            usage:    wgpu::BufferUsages::VERTEX,
        });
        let quad_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:    Some("WgpuLightingPass quad indices"),
            contents: bytemuck::cast_slice(&quad_indices),
            usage:    wgpu::BufferUsages::INDEX,
        });

        // ---------- VP uniform (group 0) ----------
        let vp_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("WgpuLightingPass vp uniform"),
            size:  std::mem::size_of::<VpUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let vp_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("WgpuLightingPass vp bgl"),
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
        let vp_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("WgpuLightingPass vp bind"),
            layout:  &vp_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding:  0,
                resource: vp_buffer.as_entire_binding(),
            }],
        });

        // ---------- Per-light dynamic-offset uniform (group 1) ----------
        // One slot per light, point lights (suns first) then spot lights,
        // and a single bind group serving both pipelines by dynamic offset.
        // The buffer starts small and grows when a frame needs more
        // (`LightSlots::ensure`) - it used to be sized for the cap up front.
        let light_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("WgpuLightingPass light bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding:    0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: std::num::NonZeroU64::new(
                        std::mem::size_of::<LightUniform>() as u64,
                    ),
                },
                count: None,
            }],
        });
        let light_slots = std::cell::RefCell::new(LightSlots::new(device, &light_bgl, INITIAL_LIGHT_SLOTS));

        // ---------- Occluder uniform (group 2, shared by both pipelines) ----------
        let occluder_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("WgpuLightingPass occluder uniform"),
            size:  std::mem::size_of::<OccluderUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let occluder_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("WgpuLightingPass occluder bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding:    0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(
                        std::mem::size_of::<OccluderUniform>() as u64,
                    ),
                },
                count: None,
            }],
        });
        let occluder_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("WgpuLightingPass occluder bind"),
            layout:  &occluder_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding:  0,
                resource: occluder_buffer.as_entire_binding(),
            }],
        });

        // Both point and spot pipelines bind the occluder group (2) for
        // their in-shader cast-shadow test.
        let light_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:                Some("WgpuLightingPass light pipeline layout"),
            bind_group_layouts:   &[&vp_bgl, &light_bgl, &occluder_bgl, &normal_bgl],
            push_constant_ranges: &[],
        });

        let point_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("point_light.wgsl"),
            source: wgpu::ShaderSource::Wgsl(POINT_LIGHT_WGSL.into()),
        });
        let spot_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("spot_light.wgsl"),
            source: wgpu::ShaderSource::Wgsl(SPOT_LIGHT_WGSL.into()),
        });

        let point_pipeline =
            make_light_pipeline(device, &light_pipeline_layout, &point_module, color_format,
                                "WgpuLightingPass point pipeline", false);
        let spot_pipeline =
            make_light_pipeline(device, &light_pipeline_layout, &spot_module, color_format,
                                "WgpuLightingPass spot pipeline", false);
        let point_sub_pipeline =
            make_light_pipeline(device, &light_pipeline_layout, &point_module, color_format,
                                "WgpuLightingPass subtractive point pipeline", true);
        let spot_sub_pipeline =
            make_light_pipeline(device, &light_pipeline_layout, &spot_module, color_format,
                                "WgpuLightingPass subtractive spot pipeline", true);

        // ---------- Ambient occlusion ----------
        let ao_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("WgpuLightingPass ao uniform"),
            size:  std::mem::size_of::<AoUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ao_occluders = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("WgpuLightingPass ao occluders"),
            size:  std::mem::size_of::<OccluderUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let ao_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("WgpuLightingPass ao bgl"),
            entries: &[uniform_entry(0), uniform_entry(1)],
        });
        let ao_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("WgpuLightingPass ao bind"),
            layout:  &ao_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ao_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: ao_occluders.as_entire_binding() },
            ],
        });
        let ao_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:                Some("WgpuLightingPass ao layout"),
            bind_group_layouts:   &[&ao_bgl],
            push_constant_ranges: &[],
        });
        let ao_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("ambient_occlusion.wgsl"),
            source: wgpu::ShaderSource::Wgsl(AMBIENT_OCCLUSION_WGSL.into()),
        });
        let ao_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("WgpuLightingPass ao pipeline"),
            layout: Some(&ao_layout),
            vertex: wgpu::VertexState {
                module:              &ao_module,
                entry_point:         Some("vs_main"),
                buffers:             &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module:              &ao_module,
                entry_point:         Some("fs_main"),
                // The light buffer times the occlusion: dst * src.
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend:  Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::Src,
                            operation:  wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::One,
                            operation:  wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                // It runs in the light pass, which also writes the
                // highlights; occlusion leaves those alone.
                Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::empty(),
                })],
                compilation_options: Default::default(),
            }),
            primitive:     wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample:   wgpu::MultisampleState::default(),
            multiview:     None,
            cache:         None,
        });

        // ---------- Composite pipeline ----------
        let composite_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("light_composite.wgsl"),
            source: wgpu::ShaderSource::Wgsl(LIGHT_COMPOSITE_WGSL.into()),
        });
        let composite_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label:        Some("WgpuLightingPass composite sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter:   wgpu::FilterMode::Linear,
            min_filter:   wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let composite_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("WgpuLightingPass composite bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding:    0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding:    1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding:    2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty:         wgpu::BindingType::Sampler(
                            wgpu::SamplerBindingType::Filtering,
                        ),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding:    3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding:    4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding:    5,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:                Some("WgpuLightingPass composite pipeline layout"),
            bind_group_layouts:   &[&composite_bind_group_layout],
            push_constant_ranges: &[],
        });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("WgpuLightingPass composite pipeline"),
            layout: Some(&composite_layout),
            vertex: wgpu::VertexState {
                module:              &composite_module,
                entry_point:         Some("vs_main"),
                buffers:             &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module:              &composite_module,
                entry_point:         Some("fs_main"),
                // Multiply pass overwrites destination — no blend
                // (matches gl `gl.disable(BLEND)` around the composite).
                targets: &[Some(wgpu::ColorTargetState {
                    format:     color_format,
                    blend:      None,
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

        // The same, blended over the frame: the world above a Scene Color
        // sprite, drawn after the scene copy (`composite_over`).
        let composite_over_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("WgpuLightingPass composite-over pipeline"),
            layout: Some(&composite_layout),
            vertex: wgpu::VertexState {
                module:              &composite_module,
                entry_point:         Some("vs_main"),
                buffers:             &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module:              &composite_module,
                entry_point:         Some("fs_over"),
                targets: &[Some(wgpu::ColorTargetState {
                    format:     color_format,
                    blend:      Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample:   wgpu::MultisampleState::default(),
            multiview:     None,
            cache:         None,
        });

        let composite_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:    Some("WgpuLightingPass composite uniform"),
            contents: bytemuck::bytes_of(&CompositeUniform::LDR),
            usage:    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let composite_bind_group = make_composite_bind(
            device,
            &composite_bind_group_layout,
            &scene_buffer,
            &light_buffer,
            &specular_light,
            &emission_buffer,
            &composite_sampler,
            &composite_uniform,
        );

        Ok(Self {
            scene_buffer,
            light_buffer,
            specular_light,
            normal_buffer,
            specular_buffer,
            layer_buffer,
            normal_bgl,
            normal_bind,
            white_cookie,
            white_written: std::cell::Cell::new(false),
            cookie_binds: std::cell::RefCell::new(std::collections::HashMap::new()),
            normals_drawn: std::cell::Cell::new(false),
            emission_buffer,
            emission_drawn:  std::cell::Cell::new(false),
            last_dropped:  std::cell::Cell::new(0),
            color_format,
            point_pipeline,
            spot_pipeline,
            point_sub_pipeline,
            spot_sub_pipeline,
            quad_vertex_buffer,
            quad_index_buffer,
            vp_buffer,
            vp_bind_group,
            light_bgl,
            light_slots,
            occluder_buffer,
            occluder_bind_group,
            ao_pipeline,
            ao_uniform,
            ao_occluders,
            ao_bind_group,
            ao_last_dropped: std::cell::Cell::new(0),
            composite_pipeline,
            composite_over_pipeline,
            composite_bind_group_layout,
            composite_sampler,
            composite_bind_group,
            composite_uniform,
            gi: std::cell::RefCell::new(None),
            gi_refused: std::cell::Cell::new(false),
        })
    }

    /// Whether light may exceed 1.0 in the composite.
    ///
    /// The light buffer used to be an 8-bit target, so light saturated at
    /// 1.0 - a lamp at intensity 2 lit a floor to exactly its albedo, never
    /// past it - and every lit scene in the repo was tuned against that. In
    /// a float buffer nothing saturates, so the composite clamps light to
    /// 1.0 itself unless the scene asked for HDR, which keeps those scenes
    /// drawing as they always have. Call once a frame (or whenever the
    /// scene's post settings change), before [`Self::composite`].
    pub fn set_hdr(&self, queue: &wgpu::Queue, hdr: bool) {
        // Only `max_light.x`: the other three lanes are toon lighting's,
        // written by `render_lights` from the frame's `LightData`.
        let u = if hdr { CompositeUniform::HDR } else { CompositeUniform::LDR };
        queue.write_buffer(&self.composite_uniform, 0, bytemuck::bytes_of(&u.max_light[0]));
    }

    /// The render target the scene should be drawn into when lighting
    /// is active. Caller is expected to bind this and run all sprite /
    /// tilemap / particle / shadow passes against it.
    pub fn scene_target(&self) -> &WgpuRenderTarget {
        &self.scene_buffer
    }

    /// Resize both internal buffers and rebuild the cached composite
    /// bind group (its texture views are invalidated by resize).
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        // Hosts call this every frame for a pixel-perfect camera, whose
        // target follows the view: the same size is free.
        if width == 0 || height == 0 || (width == self.scene_buffer.width && height == self.scene_buffer.height) {
            return;
        }
        self.scene_buffer.resize(device, width, height);
        self.light_buffer.resize(device, width, height);
        self.specular_light.resize(device, width, height);
        self.normal_buffer.resize(device, width, height);
        self.specular_buffer.resize(device, width, height);
        self.layer_buffer.resize(device, width, height);
        self.emission_buffer.resize(device, width, height);
        self.normal_bind = make_normal_bind(
            device,
            &self.normal_bgl,
            [&self.normal_buffer, &self.specular_buffer, &self.layer_buffer],
            &self.white_cookie.1,
            &self.white_cookie.2,
        );
        // Each cookie's bind group holds the old normal view: rebuilt by the
        // next `prepare_cookies`.
        self.cookie_binds.borrow_mut().clear();
        self.composite_bind_group = make_composite_bind(
            device,
            &self.composite_bind_group_layout,
            &self.scene_buffer,
            &self.light_buffer,
            &self.specular_light,
            &self.emission_buffer,
            &self.composite_sampler,
            &self.composite_uniform,
        );
    }

    /// Color format the pass was built for. Useful for callers that
    /// want to assert format compatibility with the swapchain.
    pub fn color_format(&self) -> wgpu::TextureFormat {
        self.color_format
    }

    /// Upload the cookies `lights` name (`PointLight2D::cookie`) and bind each
    /// beside the normals, for [`Self::render_lights`] - call it just before,
    /// every frame a world has lights. A cookie whose picture is not loaded
    /// yet is left out, and its light is not drawn until it is.
    pub fn prepare_cookies(
        &self,
        device: &wgpu::Device,
        queue:  &wgpu::Queue,
        lights: &LightData,
        assets: &mut crate::asset::AssetManager,
    ) {
        let mut binds = self.cookie_binds.borrow_mut();
        binds.clear();
        let ids = lights
            .point_lights
            .iter()
            .map(|(_, l)| &l.cookie)
            .chain(lights.spot_lights.iter().map(|(_, _, l)| &l.cookie))
            .filter(|c| !c.is_empty());
        for id in ids {
            if binds.contains_key(id.as_str()) {
                continue;
            }
            if assets.get_wgpu_texture(id).is_none() && assets.ensure_wgpu_texture(id, device, queue).is_err() {
                continue;
            }
            let Some(t) = assets.get_wgpu_texture(id) else { continue };
            binds.insert(
                id.clone(),
                make_normal_bind(
                    device,
                    &self.normal_bgl,
                    [&self.normal_buffer, &self.specular_buffer, &self.layer_buffer],
                    &t.view,
                    &t.sampler,
                ),
            );
        }
    }

    /// Render all lights into the light buffer.
    ///
    /// The light buffer is cleared to ambient color (so unlit areas
    /// composite to the ambient term), then point + spot lights are
    /// drawn additively on top. Only lights that reach the camera's view
    /// are drawn, the nearest [`MAX_LIGHTS_PER_KIND`] of each kind when
    /// more do ([`crate::renderer::lighting::lights_for_view`]) - the
    /// view the sprite pass culls against.
    pub fn render_lights(
        &self,
        device:  &wgpu::Device,
        queue:   &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        camera:  &Camera2D,
        lights:  &LightData,
    ) {
        // Every occluder and the bounce settings, before the lights are
        // narrowed to the view's.
        let everything = lights;
        let view = crate::renderer::frame_view::FrameView::of(camera);
        let chosen = crate::renderer::lighting::lights_for_view(lights, view.center - view.half, view.center + view.half);
        let lights: &LightData = &chosen;
        if !self.white_written.replace(true) {
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &self.white_cookie.0,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &[255, 255, 255, 255],
                wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
                wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            );
        }
        // Upload VP.
        let vp = camera.view_projection();
        queue.write_buffer(
            &self.vp_buffer,
            0,
            bytemuck::bytes_of(&VpUniform { view_projection: vp.to_cols_array_2d() }),
        );

        // Pack lights into the dynamic-offset buffer. Point lights
        // occupy slots [0, MAX); spot lights occupy [MAX, 2*MAX). Slot
        // size is LIGHT_STRIDE so we have to write each LightUniform
        // at its own offset rather than a single bulk slice.
        // Suns first, then point lights: they are drawn by one pipeline, and
        // the packing below orders its ranges the same way.
        let points = crate::renderer::lighting::point_lights_and_suns(lights);
        let n_point = points.len();
        let n_spot  = lights.spot_lights.len().min(MAX_LIGHTS_PER_KIND);

        // Every light's own occluder list, packed the way the CPU's
        // `light_at` packs it. A light with an empty range skips the
        // per-fragment occluder loop entirely.
        let packed = crate::renderer::lighting::pack_light_occluders(lights);
        if packed.dropped != self.last_dropped.get() {
            if packed.dropped > 0 {
                log::warn!(
                    "[lighting] {} occluder edge(s) within a light's reach did not fit the pool of {} - \
                     the farthest from each light cast no shadow",
                    packed.dropped,
                    MAX_OCCLUDER_SEGMENTS
                );
            }
            self.last_dropped.set(packed.dropped);
        }
        // Does a light's range hold any edge that does not shadow its own
        // occluder's inside? Only then is the parity pass worth running.
        let any_free = |(start, count): (usize, usize)| -> u32 {
            crate::renderer::lighting::occluder_flags(&packed.owners[start..start + count])
        };
        // Every light's slot, points then spots, uploaded in one write.
        self.light_slots.borrow_mut().ensure(device, &self.light_bgl, n_point + n_spot);
        let mut stripe = vec![0u8; (n_point + n_spot) * LIGHT_STRIDE as usize];
        for (i, (pos, light, shadow_scale)) in points.iter().enumerate() {
            let (start, count) = packed.point[i];
            let cast_flag = if count > 0 { 1.0 } else { 0.0 };
            let u = LightUniform {
                pos_radius_falloff: [pos.x, pos.y, light.radius, light.falloff],
                color_intensity:    [
                    light.color[0],
                    light.color[1],
                    light.color[2],
                    light.intensity,
                ],
                // z = shadow softness (emitter radius); 0 = hard shadow.
                // w = cast-shadows flag (the range is not empty).
                // x = the turn its cookie takes.
                spot:               [light.turn, 0.0, light.shadow_softness, cast_flag],
                occ:                [start as u32, count as u32, packed.point_inside[i], any_free((start, count))],
                // z: how much this light stretches bounded shadows - a sun's
                // `1 / height`, `1` for a lamp. w: its cookie's reach, 0 for none.
                extra:              [light.height.max(0.02), crate::ecs::components::MAX_NORMAL_RESPONSE, *shadow_scale, cookie_reach(&light.cookie, light.radius, light.cookie_scale)],
                layers:             [light.light_mask, 0, 0, 0],
            };
            let at = i * LIGHT_STRIDE as usize;
            stripe[at..at + std::mem::size_of::<LightUniform>()].copy_from_slice(bytemuck::bytes_of(&u));
        }
        for (i, (pos, entity_rotation, light)) in
            lights.spot_lights.iter().take(n_spot).enumerate()
        {
            let dir = entity_rotation + light.direction;
            let (start, count) = packed.spot[i];
            let cast_flag = if count > 0 { 1.0 } else { 0.0 };
            let u = LightUniform {
                pos_radius_falloff: [pos.x, pos.y, light.radius, light.falloff],
                color_intensity:    [
                    light.color[0],
                    light.color[1],
                    light.color[2],
                    light.intensity,
                ],
                spot:               [dir, light.angle, light.shadow_softness, cast_flag],
                occ:                [start as u32, count as u32, packed.spot_inside[i], any_free((start, count))],
                extra:              [light.height.max(0.02), crate::ecs::components::MAX_NORMAL_RESPONSE, 1.0, cookie_reach(&light.cookie, light.radius, light.cookie_scale)],
                layers:             [light.light_mask, 0, 0, 0],
            };
            let at = (n_point + i) * LIGHT_STRIDE as usize;
            stripe[at..at + std::mem::size_of::<LightUniform>()].copy_from_slice(bytemuck::bytes_of(&u));
        }
        // One upload rather than one per light: a frame of a thousand lights
        // is one write, not a thousand.
        if !stripe.is_empty() {
            queue.write_buffer(&self.light_slots.borrow().buffer, 0, &stripe);
        }

        // The pool every light's range points into.
        let occ_uniform = pack_occluders(&packed.segments, &packed.tints, &packed.owners);
        queue.write_buffer(
            &self.occluder_buffer,
            0,
            bytemuck::bytes_of(&occ_uniform),
        );

        // A frame that drew no normals lights everything as flat. The
        // buffer is never left holding a previous frame's normals.
        if !self.normals_drawn.replace(false) {
            drop(self.normal_buffer.begin_pass(encoder, Some(FLAT_NORMAL)));
            drop(self.specular_buffer.begin_pass(encoder, Some(wgpu::Color::TRANSPARENT)));
            drop(self.layer_buffer.begin_pass(encoder, Some(ALL_LAYERS)));
        }

        // Toon lighting, for the composite: `max_light.y` bands (0 is off),
        // `.z` softness, then the dither cell and the screen-to-world matrix
        // it is anchored by. `set_hdr` writes `.x` alone, so the two never
        // clash.
        let mut tail = [0.0f32; 23];
        if let Some(t) = lights.toon {
            tail[0] = t.bands as f32;
            tail[1] = t.softness.clamp(0.0, 1.0);
            tail[3] = t.dither.max(0.0);
        }
        tail[7..].copy_from_slice(&vp.inverse().to_cols_array());
        queue.write_buffer(&self.composite_uniform, 4, bytemuck::cast_slice(&tail));
        // The fog, after them (`ecs::fog`): all zero is none.
        let fog = lights.fog.map_or([[0.0f32; 4]; 4], |f| [f.fog_color, f.shape, f.motion, f.area]);
        queue.write_buffer(&self.composite_uniform, 96, bytemuck::cast_slice(&fog));

        // Begin pass on the light buffer cleared to ambient.
        let amb = &lights.ambient;
        let clear = wgpu::Color {
            r: (amb.color[0] * amb.intensity) as f64,
            g: (amb.color[1] * amb.intensity) as f64,
            b: (amb.color[2] * amb.intensity) as f64,
            a: 1.0,
        };
        // Ambient occlusion: the occluders near the view, whole, and the
        // ambient-cleared buffer multiplied by the occlusion before any light
        // is added - so only the ambient term is occluded.
        let ao_draw = lights.occlusion.and_then(|ao| {
            let (segs, owners, dropped) =
                crate::renderer::lighting::ao_edges_for_view(lights, view.center - view.half, view.center + view.half, ao.radius);
            if dropped != self.ao_last_dropped.get() {
                if dropped > 0 {
                    log::warn!(
                        "[lighting] {dropped} occluder(s) near the view did not fit ambient occlusion's pool of {} edges",
                        MAX_OCCLUDER_SEGMENTS
                    );
                }
                self.ao_last_dropped.set(dropped);
            }
            if segs.is_empty() {
                return None;
            }
            // Each occluder's box and where its edges end, so a pixel far
            // from one skips all of it (`ao_occluder_bounds`).
            let (boxes, ends) = crate::renderer::lighting::ao_occluder_bounds(&segs, &owners);
            queue.write_buffer(&self.ao_occluders, 0, bytemuck::bytes_of(&pack_occluders(&segs, &boxes, &ends)));
            let u = AoUniform {
                inv_view_projection: vp.inverse().to_cols_array_2d(),
                params: [ao.radius, ao.strength, self.light_buffer.width as f32, self.light_buffer.height as f32],
            };
            queue.write_buffer(&self.ao_uniform, 0, bytemuck::bytes_of(&u));
            Some(())
        });

        // The lights' diffuse into the light buffer and their highlights
        // beside it (`light_specular.wgsl`).
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("WgpuLightingPass light + specular pass"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.light_buffer.view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.specular_light.view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                }),
            ],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        if ao_draw.is_some() {
            pass.set_pipeline(&self.ao_pipeline);
            pass.set_bind_group(0, &self.ao_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        let slots = self.light_slots.borrow();
        let cookies = self.cookie_binds.borrow();
        if n_point > 0 {
            pass.set_pipeline(&self.point_pipeline);
            pass.set_bind_group(0, &self.vp_bind_group, &[]);
            pass.set_bind_group(2, &self.occluder_bind_group, &[]);
            pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
            pass.set_index_buffer(
                self.quad_index_buffer.slice(..),
                wgpu::IndexFormat::Uint16,
            );
            // The lights that add, then those that take away
            // (`subtract`) - the order an 8-bit light buffer needs, which
            // clamps at each blend.
            for subtract in [false, true] {
                if subtract {
                    if !points.iter().any(|(_, l, _)| l.subtract) {
                        break;
                    }
                    pass.set_pipeline(&self.point_sub_pipeline);
                }
                for (i, (_, light, _)) in points.iter().enumerate() {
                    if light.subtract != subtract {
                        continue;
                    }
                    let Some(group3) = cookie_bind(&light.cookie, &self.normal_bind, &cookies) else { continue };
                    pass.set_bind_group(3, group3, &[]);
                    let offset = (i as u64 * LIGHT_STRIDE) as u32;
                    pass.set_bind_group(1, &slots.bind_group, &[offset]);
                    pass.draw_indexed(0..6, 0, 0..1);
                }
            }
        }
        if n_spot > 0 {
            pass.set_pipeline(&self.spot_pipeline);
            pass.set_bind_group(0, &self.vp_bind_group, &[]);
            pass.set_bind_group(2, &self.occluder_bind_group, &[]);
            pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
            pass.set_index_buffer(
                self.quad_index_buffer.slice(..),
                wgpu::IndexFormat::Uint16,
            );
            for subtract in [false, true] {
                if subtract {
                    if !lights.spot_lights.iter().take(n_spot).any(|(_, _, l)| l.subtract) {
                        break;
                    }
                    pass.set_pipeline(&self.spot_sub_pipeline);
                }
                for (i, (_, _, light)) in lights.spot_lights.iter().take(n_spot).enumerate() {
                    if light.subtract != subtract {
                        continue;
                    }
                    let Some(group3) = cookie_bind(&light.cookie, &self.normal_bind, &cookies) else { continue };
                    pass.set_bind_group(3, group3, &[]);
                    let offset = ((n_point + i) as u64 * LIGHT_STRIDE) as u32;
                    pass.set_bind_group(1, &slots.bind_group, &[offset]);
                    pass.draw_indexed(0..6, 0, 0..1);
                }
            }
        }

        // Pass dropped here ⇒ encoded into the command buffer.
        drop(pass);

        // Then the bounce, from the scene and the direct light just drawn.
        if let Some(gi) = everything.gi.filter(|g| g.strength > 0.0) {
            if self.light_buffer.format == wgpu::TextureFormat::Rgba16Float {
                let mut slot = self.gi.borrow_mut();
                let pass = slot.get_or_insert_with(|| super::gi::GiPass::new(device, self.light_buffer.format));
                pass.run(device, queue, encoder, camera, everything, &gi, &self.scene_buffer.view, &self.light_buffer);
            } else if !self.gi_refused.replace(true) {
                log::warn!("[lighting] global illumination needs a float frame (scene_format Rgba16Float); left out");
            }
        }
    }

    /// The normal buffer, cleared flat, for `render_normals_wgpu` to draw
    /// the world's normals into. Call after the world pass and before
    /// [`Self::render_lights`], in the same frame; a frame that skips it -
    /// a scene with no normal maps - is lit as flat.
    ///
    /// The pass has three targets: the normals, beside them each surface's
    /// specular (`fs_normal`), cleared to none, and its light layers,
    /// cleared to all.
    pub fn normal_pass<'a>(&'a self, encoder: &'a mut wgpu::CommandEncoder) -> wgpu::RenderPass<'a> {
        self.normals_drawn.set(true);
        let attach = |view: &'a wgpu::TextureView, clear: wgpu::Color| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
            })
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("WgpuLightingPass normal + specular pass"),
            color_attachments: &[
                attach(&self.normal_buffer.view, FLAT_NORMAL),
                attach(&self.specular_buffer.view, wgpu::Color::TRANSPARENT),
                attach(&self.layer_buffer.view, ALL_LAYERS),
            ],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// The emission buffer, cleared, for the world's `Unlit` things
    /// (`WorldLayer::Emission`). Call between the world pass and
    /// [`Self::composite`]; a frame that skips it composites no emission.
    pub fn emission_pass<'a>(&'a self, encoder: &'a mut wgpu::CommandEncoder) -> wgpu::RenderPass<'a> {
        self.emission_drawn.set(true);
        self.emission_buffer.begin_pass(encoder, Some(wgpu::Color::TRANSPARENT))
    }

    /// Reopen the light buffer **without clearing it**, for additive draws
    /// after [`Self::render_lights`] - baked lightmaps, which carry their own
    /// texture and so draw through the sprite batch rather than a light
    /// pipeline. Must be dropped before [`Self::composite`].
    pub fn light_buffer_pass<'a>(&'a self, encoder: &'a mut wgpu::CommandEncoder) -> wgpu::RenderPass<'a> {
        self.light_buffer.begin_pass(encoder, None)
    }

    /// Composite scene × light buffer onto `dest_view`. Caller chooses
    /// the destination — typically the postprocess scene_target so the
    /// PP chain runs on the lit result, or the swapchain view for a
    /// no-postprocess fast path.
    pub fn composite(
        &self,
        encoder:   &mut wgpu::CommandEncoder,
        dest_view: &wgpu::TextureView,
    ) {
        // No emission this frame: add nothing, not last frame's.
        if !self.emission_drawn.replace(false) {
            drop(self.emission_buffer.begin_pass(encoder, Some(wgpu::Color::TRANSPARENT)));
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("WgpuLightingPass composite pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view:           dest_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    // Composite covers the whole viewport — Load is
                    // safe and avoids stomping anything the caller
                    // might have pre-drawn (matches gl which doesn't
                    // clear before the composite either).
                    load:  wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes:         None,
            occlusion_query_set:      None,
        });
        pass.set_pipeline(&self.composite_pipeline);
        pass.set_bind_group(0, &self.composite_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// The scene buffer, cleared **transparent**, for a layer of the world
    /// drawn over the frame rather than under it: the part above a Scene
    /// Color sprite ([`crate::ecs::wgpu_systems::ZWindow::Above`]). Draw it,
    /// its emission pass if it has unlit things, then [`Self::composite_over`].
    /// The lights and the normal buffer are the frame's, drawn once.
    pub fn overlay_scene_pass<'a>(&'a self, encoder: &'a mut wgpu::CommandEncoder) -> wgpu::RenderPass<'a> {
        self.scene_buffer.begin_pass(encoder, Some(wgpu::Color::TRANSPARENT))
    }

    /// Composite the scene buffer lit - as [`Self::composite`] - **over**
    /// `dest_view` rather than replacing it: premultiplied source-over, so
    /// what the layer does not cover keeps the frame beneath.
    pub fn composite_over(&self, encoder: &mut wgpu::CommandEncoder, dest_view: &wgpu::TextureView) {
        if !self.emission_drawn.replace(false) {
            drop(self.emission_buffer.begin_pass(encoder, Some(wgpu::Color::TRANSPARENT)));
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("WgpuLightingPass composite-over pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view:           dest_view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes:         None,
            occlusion_query_set:      None,
        });
        pass.set_pipeline(&self.composite_over_pipeline);
        pass.set_bind_group(0, &self.composite_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Build the per-light render pipeline. Both point and spot lights
/// use identical state (additive blend, unit-quad vertex buffer,
/// shared bind group layouts) — they only differ in the shader.
fn make_light_pipeline(
    device:       &wgpu::Device,
    layout:       &wgpu::PipelineLayout,
    module:       &wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    label:        &'static str,
    subtract:     bool,
) -> wgpu::RenderPipeline {
    // A subtractive light takes its colour off what the lights before it
    // summed (destination minus source) and leaves alpha alone.
    let colour = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation:  if subtract { wgpu::BlendOperation::ReverseSubtract } else { wgpu::BlendOperation::Add },
    };
    let alpha = wgpu::BlendComponent {
        src_factor: if subtract { wgpu::BlendFactor::Zero } else { wgpu::BlendFactor::One },
        dst_factor: wgpu::BlendFactor::One,
        operation:  wgpu::BlendOperation::Add,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label:  Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: (std::mem::size_of::<f32>() * 2) as wgpu::BufferAddress,
                step_mode:    wgpu::VertexStepMode::Vertex,
                attributes:   &wgpu::vertex_attr_array![0 => Float32x2],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_main"),
            // Additive blend matches gl `blend_func(ONE, ONE)`.
            targets: &[
                Some(wgpu::ColorTargetState {
                format:     color_format,
                blend:      Some(wgpu::BlendState { color: colour, alpha }),
                write_mask: wgpu::ColorWrites::ALL,
            }),
                // The highlights, summed the same way.
                Some(wgpu::ColorTargetState {
                format:     color_format,
                blend:      Some(wgpu::BlendState { color: colour, alpha }),
                write_mask: wgpu::ColorWrites::ALL,
            }),
            ],
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
    })
}

/// A normal pointing out of the screen, encoded: what every pixel with no
/// normal map holds.
const FLAT_NORMAL: wgpu::Color = wgpu::Color { r: 0.5, g: 0.5, b: 1.0, a: 1.0 };

/// Every light layer, as the layer buffer holds it (its low eight bits): what
/// a pixel nothing in the normal pass drew is on.
const ALL_LAYERS: wgpu::Color = wgpu::Color { r: 255.0, g: 0.0, b: 0.0, a: 0.0 };

/// The normals, specular and layers, and a cookie beside them.
fn make_normal_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    [target, specular, layers]: [&WgpuRenderTarget; 3],
    cookie: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label:   Some("WgpuLightingPass normal + cookie bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&target.view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(cookie) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&specular.view) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&layers.view) },
        ],
    })
}

/// A light's cookie reach for its uniform: `radius * scale`, or 0 for none.
fn cookie_reach(cookie: &str, radius: f32, scale: f32) -> f32 {
    if cookie.is_empty() { 0.0 } else { (radius * scale).max(1e-3) }
}

/// What a light binds in group 3: the normals with its cookie, or with
/// white when it has none - `None` when its cookie is not ready.
fn cookie_bind<'a>(
    cookie: &str,
    plain: &'a wgpu::BindGroup,
    cookies: &'a std::collections::HashMap<String, wgpu::BindGroup>,
) -> Option<&'a wgpu::BindGroup> {
    if cookie.is_empty() { Some(plain) } else { cookies.get(cookie) }
}

/// Build the composite bind group binding scene + light textures and
/// the shared linear sampler.
fn make_composite_bind(
    device:   &wgpu::Device,
    layout:   &wgpu::BindGroupLayout,
    scene:    &WgpuRenderTarget,
    light:    &WgpuRenderTarget,
    specular: &WgpuRenderTarget,
    emission: &WgpuRenderTarget,
    sampler:  &wgpu::Sampler,
    uniform:  &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label:   Some("WgpuLightingPass composite bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding:  0,
                resource: wgpu::BindingResource::TextureView(&scene.view),
            },
            wgpu::BindGroupEntry {
                binding:  1,
                resource: wgpu::BindingResource::TextureView(&light.view),
            },
            wgpu::BindGroupEntry {
                binding:  2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding:  3,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding:  4,
                resource: wgpu::BindingResource::TextureView(&emission.view),
            },
            // The highlights, added after the multiply.
            wgpu::BindGroupEntry {
                binding:  5,
                resource: wgpu::BindingResource::TextureView(&specular.view),
            },
        ],
    })
}
