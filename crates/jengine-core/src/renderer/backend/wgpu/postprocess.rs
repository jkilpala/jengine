//! wgpu post-process effect chain, and the pass that puts a frame on screen.
//!
//! The scene is drawn into [`WgpuPostProcessStack::scene_target`] (= ping),
//! in the **scene format** - `WgpuRenderer::scene_format`, a float format
//! wherever the adapter has one, so light and glows may exceed 1.0. Each
//! frame [`WgpuPostProcessStack::execute`] runs the scene's effect chain in
//! that format, ping-ponging between two targets, and then the **output
//! pass** writes the result into the display view in its own format:
//! exposure, the tonemap, and a dither. The output pass always runs, so an
//! empty chain costs one full-screen pass, as the passthrough blit it
//! replaced did.
//!
//! Effects:
//!
//! - `bloom` - a mip chain (see `bloom.wgsl`): a soft-knee threshold, a
//!   13-tap downsample through up to [`MAX_BLOOM_MIPS`] half-resolution
//!   levels, a tent upsample back, added onto the source. It used to be one
//!   9-tap pass a few pixels wide.
//! - `vignette`, `color_grade`, `chromatic` - one full-screen pass each.
//!
//! Only the output pass knows the display. That is what lets UI be drawn
//! after it, untouched by bloom and vignette: the hosts open a pass on the
//! display view once `execute` returns.
//!
//! Per-frame, uniform values are pulled out of the
//! [`PostProcessEffectSettings::params`] map into stack-allocated PODs and
//! uploaded with `queue.write_buffer`. Missing keys fall back to defaults.

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use wgpu::{
    util::DeviceExt, BindGroup, BindGroupLayout, Buffer, RenderPipeline, ShaderModule,
    TextureFormat,
};

use super::framebuffer::WgpuRenderTarget;
use super::renderer::is_hdr_format;
use crate::renderer::material::UniformValue;
use crate::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings, Tonemap};

const BLOOM_WGSL:       &str = include_str!("../../../../assets/shaders/wgsl/bloom.wgsl");
const VIGNETTE_WGSL:    &str = include_str!("../../../../assets/shaders/wgsl/vignette.wgsl");
const COLOR_GRADE_WGSL: &str = include_str!("../../../../assets/shaders/wgsl/color_grade.wgsl");
const CHROMATIC_WGSL:   &str = include_str!("../../../../assets/shaders/wgsl/chromatic.wgsl");
const OUTPUT_WGSL:      &str = include_str!("../../../../assets/shaders/wgsl/output.wgsl");
const PIXELATE_WGSL:    &str = include_str!("../../../../assets/shaders/wgsl/pixelate.wgsl");
const CRT_WGSL:         &str = include_str!("../../../../assets/shaders/wgsl/crt.wgsl");
const LIGHT_SHAFTS_WGSL: &str = include_str!("../../../../assets/shaders/wgsl/light_shafts.wgsl");
const TILT_SHIFT_WGSL: &str = include_str!("../../../../assets/shaders/wgsl/tilt_shift.wgsl");
const MOTION_BLUR_WGSL: &str = include_str!("../../../../assets/shaders/wgsl/motion_blur.wgsl");
const LENS_FLARE_WGSL: &str = include_str!("../../../../assets/shaders/wgsl/lens_flare.wgsl");
const DEPTH_OF_FIELD_WGSL: &str = include_str!("../../../../assets/shaders/wgsl/depth_of_field.wgsl");

/// `lens_flare`: see `lens_flare.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct LensFlareU {
    threshold:  f32,
    intensity:  f32,
    ghosts:     f32,
    spacing:    f32,
    halo:       f32,
    halo_width: f32,
    dispersion: f32,
    streak:     f32,
    streak_len: f32,
    aspect:     f32,
    /// One texel of the quarter-size copy, in UV.
    texel:      [f32; 2],
    tint:       [f32; 4],
}
impl LensFlareU {
    fn new(params: &std::collections::HashMap<String, UniformValue>, width: u32, height: u32) -> Self {
        Self {
            threshold:  get_f32(params, "threshold").unwrap_or(1.0).max(0.0),
            intensity:  get_f32(params, "intensity").unwrap_or(0.6).max(0.0),
            ghosts:     get_f32(params, "ghosts").unwrap_or(5.0).clamp(0.0, 8.0),
            spacing:    get_f32(params, "spacing").unwrap_or(0.35).clamp(0.0, 2.0),
            halo:       get_f32(params, "halo").unwrap_or(0.45).clamp(0.0, 1.0),
            halo_width: get_f32(params, "halo_width").unwrap_or(0.06).clamp(0.001, 0.5),
            dispersion: get_f32(params, "dispersion").unwrap_or(0.006).clamp(0.0, 0.05),
            streak:     get_f32(params, "streak").unwrap_or(0.0).max(0.0),
            streak_len: get_f32(params, "streak_length").unwrap_or(0.25).clamp(0.0, 1.0),
            aspect:     width.max(1) as f32 / height.max(1) as f32,
            texel:      {
                let (w, h) = lens_flare_size(width, height);
                [1.0 / w as f32, 1.0 / h as f32]
            },
            tint:       get_vec4(params, "tint").unwrap_or([1.0, 0.9, 0.8, 1.0]),
        }
    }
}

/// The lens flare works on a copy a quarter of the frame's size.
fn lens_flare_size(width: u32, height: u32) -> (u32, u32) {
    ((width / 4).max(1), (height / 4).max(1))
}

/// The velocity pass's target (`motion_blur`): signed screen UV a sixtieth
/// of a second in `.rg`, so a float format.
pub const VELOCITY_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// `motion_blur`: see `motion_blur.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct MotionBlurU {
    shutter: f32,
    samples: f32,
    max_len: f32,
    texel:   f32,
}
impl MotionBlurU {
    fn from_params(params: &std::collections::HashMap<String, UniformValue>, width: u32) -> Self {
        Self {
            shutter: get_f32(params, "shutter").unwrap_or(0.8).clamp(0.0, 4.0),
            samples: get_f32(params, "samples").unwrap_or(12.0).clamp(2.0, 16.0),
            max_len: get_f32(params, "max_length").unwrap_or(0.08).clamp(0.0, 0.5),
            texel:   1.0 / width.max(1) as f32,
        }
    }
}

/// `depth_of_field`: see `depth_of_field.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct DepthOfFieldU {
    focus:      f32,
    range:      f32,
    strength:   f32,
    max_radius: f32,
}
impl DepthOfFieldU {
    fn from_params(params: &std::collections::HashMap<String, UniformValue>) -> Self {
        Self {
            focus:      get_f32(params, "focus").unwrap_or(0.0),
            range:      get_f32(params, "range").unwrap_or(0.0).max(0.0),
            strength:   get_f32(params, "strength").unwrap_or(1.0).max(0.0),
            max_radius: get_f32(params, "max_radius").unwrap_or(12.0).clamp(0.0, 32.0),
        }
    }
}

/// The deepest the bloom chain goes. Seven halvings take a 1080p frame to
/// 8 px, which is about as wide as a glow ever wants to be.
pub const MAX_BLOOM_MIPS: usize = 7;
/// A level smaller than this on either side adds nothing but a draw call.
const MIN_BLOOM_MIP_SIDE: u32 = 4;

// ─── per-effect uniform PODs ──────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct BloomU {
    threshold: f32,
    knee:      f32,
    intensity: f32,
    scatter:   f32,
    norm:      f32,
    _pad:      [f32; 3],
}
impl BloomU {
    /// `threshold` 0.8 suits an LDR scene, where nothing exceeds 1.0; an HDR
    /// scene wants 1.0 or more, so only what is brighter than white glows.
    fn from_params(params: &std::collections::HashMap<String, UniformValue>, levels: usize) -> Self {
        let threshold = get_f32(params, "threshold").unwrap_or(0.8).max(0.0);
        // `knee` is a fraction of the threshold, as in Unity.
        let knee = threshold * get_f32(params, "knee").unwrap_or(0.5).clamp(0.0, 1.0);
        let intensity = get_f32(params, "intensity").unwrap_or(0.8).max(0.0);
        let scatter = get_f32(params, "scatter").unwrap_or(0.7).clamp(0.0, 1.0);
        let norm = 1.0 / (0..levels.max(1)).map(|k| scatter.powi(k as i32)).sum::<f32>();
        Self { threshold, knee, intensity, scatter, norm, _pad: [0.0; 3] }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct VignetteU {
    intensity:  f32,
    smoothness: f32,
    _pad0:      f32,
    _pad1:      f32,
    color:      [f32; 4],
}
impl VignetteU {
    const DEFAULT: Self = Self {
        intensity:  0.5,
        smoothness: 0.4,
        _pad0:      0.0,
        _pad1:      0.0,
        color:      [0.0, 0.0, 0.0, 1.0],
    };
    fn from_params(params: &std::collections::HashMap<String, UniformValue>) -> Self {
        let mut u = Self::DEFAULT;
        if let Some(v) = get_f32(params, "intensity")  { u.intensity  = v; }
        if let Some(v) = get_f32(params, "smoothness") { u.smoothness = v; }
        if let Some(v) = get_vec4(params, "color")     { u.color      = v; }
        u
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ColorGradeU {
    brightness: f32,
    contrast:   f32,
    saturation: f32,
    _pad:       f32,
    tint:       [f32; 4],
}
impl ColorGradeU {
    const DEFAULT: Self = Self {
        brightness: 1.0,
        contrast:   1.0,
        saturation: 1.0,
        _pad:       0.0,
        // Alpha = tint strength (0 = no tint, 1 = full multiply by rgb).
        tint:       [1.0, 1.0, 1.0, 0.0],
    };
    fn from_params(params: &std::collections::HashMap<String, UniformValue>) -> Self {
        let mut u = Self::DEFAULT;
        if let Some(v) = get_f32(params, "brightness")  { u.brightness = v; }
        if let Some(v) = get_f32(params, "contrast")    { u.contrast   = v; }
        if let Some(v) = get_f32(params, "saturation")  { u.saturation = v; }
        if let Some(v) = get_vec4(params, "tint_color") { u.tint = v; }
        else if let Some(v) = get_vec4(params, "tint")  { u.tint = v; }
        u
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ChromaticU {
    strength: f32,
    _pad0:    f32,
    _pad1:    f32,
    _pad2:    f32,
}
impl ChromaticU {
    const DEFAULT: Self = Self { strength: 0.005, _pad0: 0.0, _pad1: 0.0, _pad2: 0.0 };
    fn from_params(params: &std::collections::HashMap<String, UniformValue>) -> Self {
        let mut u = Self::DEFAULT;
        if let Some(v) = get_f32(params, "strength") { u.strength = v; }
        u
    }
}

/// `pixelate`: the frame point-sampled in cells of `height / lines` screen
/// pixels, the grid anchored to the world. See `pixelate.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug, PartialEq)]
pub(crate) struct PixelateU {
    cell:   f32,
    _pad:   f32,
    offset: [f32; 2],
}
impl PixelateU {
    /// `lines` (default 180) is the art's vertical resolution. The grid's
    /// origin is where world (0, 0) falls on a `width` x `height` target
    /// seen through `view`, so art placed on the world's cell grid lands on
    /// the screen's; with no view it is the target's corner.
    pub(crate) fn new(
        params: &std::collections::HashMap<String, UniformValue>,
        width: u32,
        height: u32,
        view: Option<crate::renderer::frame_view::FrameView>,
    ) -> Self {
        let lines = get_f32(params, "lines").unwrap_or(180.0).max(1.0);
        let cell = (height as f32 / lines).max(1.0);
        let offset = match view {
            Some(v) if v.half.x > 0.0 && v.half.y > 0.0 => {
                let x = (v.half.x - v.center.x) / (2.0 * v.half.x) * width as f32;
                let y = (v.center.y + v.half.y) / (2.0 * v.half.y) * height as f32;
                [x.rem_euclid(cell), y.rem_euclid(cell)]
            }
            _ => [0.0, 0.0],
        };
        Self { cell, _pad: 0.0, offset }
    }
}

/// `crt`: scanlines, an RGB mask and a curved screen. See `crt.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct CrtU {
    lines:     f32,
    scanline:  f32,
    mask:      f32,
    curvature: f32,
}
impl CrtU {
    fn from_params(params: &std::collections::HashMap<String, UniformValue>) -> Self {
        Self {
            lines:     get_f32(params, "lines").unwrap_or(180.0).max(1.0),
            scanline:  get_f32(params, "scanline").unwrap_or(0.4).clamp(0.0, 1.0),
            mask:      get_f32(params, "mask").unwrap_or(0.2).clamp(0.0, 1.0),
            curvature: get_f32(params, "curvature").unwrap_or(0.05).max(0.0),
        }
    }
}

/// `tilt_shift` and `blur`: a disc blur whose radius runs from `min_px` in a
/// horizontal band - or, with `mode` 1, a rectangle - to `max_px` away from
/// it. See `tilt_shift.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct TiltShiftU {
    focus_v: f32,
    band:    f32,
    /// In v for the band; in pixels for the rectangle.
    falloff: f32,
    max_px:  f32,
    min_px:  f32,
    /// 0: the band; 1: the rectangle `rect`.
    mode:    f32,
    texel:   [f32; 2],
    /// The rectangle's centre and half size, in uv.
    rect:    [f32; 4],
}
impl TiltShiftU {
    /// `focus_y`, `band` and `falloff` are **world** units - the band sits
    /// on a street of the town, not a row of the screen, and follows it as
    /// the camera moves - put on the screen through `view`. With no view
    /// they are taken as screen v.
    pub(crate) fn new(
        params: &std::collections::HashMap<String, UniformValue>,
        width: u32,
        height: u32,
        view: Option<crate::renderer::frame_view::FrameView>,
    ) -> Self {
        let y = get_f32(params, "focus_y").unwrap_or(0.0);
        let band = get_f32(params, "band").unwrap_or(60.0).max(0.0);
        let falloff = get_f32(params, "falloff").unwrap_or(220.0).max(0.0);
        let (focus_v, band, falloff) = match view {
            Some(v) if v.half.y > 0.0 => (
                ((v.center.y + v.half.y) - y) / (2.0 * v.half.y),
                band / (2.0 * v.half.y),
                falloff / (2.0 * v.half.y),
            ),
            _ => (y, band, falloff),
        };
        Self {
            focus_v,
            band,
            falloff,
            max_px: get_f32(params, "max_blur").unwrap_or(10.0).clamp(0.0, 48.0),
            min_px: get_f32(params, "min_blur").unwrap_or(0.0).clamp(0.0, 48.0),
            mode: 0.0,
            texel: [1.0 / width.max(1) as f32, 1.0 / height.max(1) as f32],
            rect: [0.0; 4],
        }
    }
}

/// `blur`: the same disc everywhere - `tilt_shift` with one radius - or,
/// given a focus rectangle (`focus_x`, `focus_y` its centre, `focus_w`,
/// `focus_h` its size, all **world** units), everywhere but there: sharp
/// inside (at `focus_blur`, 0 by default), easing to `radius` over
/// `falloff` world units outside it - the room you are in, the world
/// outside it out of focus. The rectangle is put on the screen through
/// `view` (an unturned camera's); with no view it is taken as uv.
pub(crate) struct BlurU;
impl BlurU {
    pub(crate) fn uniform(
        params: &std::collections::HashMap<String, UniformValue>,
        width: u32,
        height: u32,
        view: Option<crate::renderer::frame_view::FrameView>,
    ) -> TiltShiftU {
        let r = get_f32(params, "radius").unwrap_or(6.0).clamp(0.0, 48.0);
        let texel = [1.0 / width.max(1) as f32, 1.0 / height.max(1) as f32];
        let (w, h) = (get_f32(params, "focus_w").unwrap_or(0.0), get_f32(params, "focus_h").unwrap_or(0.0));
        if w <= 0.0 || h <= 0.0 {
            return TiltShiftU { focus_v: 0.5, band: 0.0, falloff: 1.0, max_px: r, min_px: r, mode: 0.0, texel, rect: [0.0; 4] };
        }
        let (x, y) = (get_f32(params, "focus_x").unwrap_or(0.0), get_f32(params, "focus_y").unwrap_or(0.0));
        let falloff = get_f32(params, "falloff").unwrap_or(40.0).max(0.0);
        let (rect, falloff_px) = match view {
            Some(v) if v.half.x > 0.0 && v.half.y > 0.0 => (
                [
                    (x - (v.center.x - v.half.x)) / (2.0 * v.half.x),
                    ((v.center.y + v.half.y) - y) / (2.0 * v.half.y),
                    w * 0.5 / (2.0 * v.half.x),
                    h * 0.5 / (2.0 * v.half.y),
                ],
                falloff / (2.0 * v.half.x) * width as f32,
            ),
            _ => ([x, y, w * 0.5, h * 0.5], falloff * width as f32),
        };
        TiltShiftU {
            focus_v: 0.5,
            band: 0.0,
            falloff: falloff_px,
            max_px: r,
            min_px: get_f32(params, "focus_blur").unwrap_or(0.0).clamp(0.0, 48.0),
            mode: 1.0,
            texel,
            rect,
        }
    }
}

/// `light_shafts`: light scattered from a source through the gaps in what
/// stands before it. See `light_shafts.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct LightShaftsU {
    source:    [f32; 2],
    length:    f32,
    intensity: f32,
    color:     [f32; 4],
    decay:     f32,
    threshold: f32,
    aspect:    f32,
    _pad:      f32,
}
impl LightShaftsU {
    /// The source is a **world** point (`source_x`, `source_y`), put on the
    /// screen through `view` - so a sun far off to one side streams in at
    /// the right angle wherever the camera is. With no view it is taken as
    /// screen UV.
    pub(crate) fn new(
        params: &std::collections::HashMap<String, UniformValue>,
        width: u32,
        height: u32,
        view: Option<crate::renderer::frame_view::FrameView>,
    ) -> Self {
        let x = get_f32(params, "source_x").unwrap_or(0.0);
        let y = get_f32(params, "source_y").unwrap_or(1000.0);
        let source = match view {
            Some(v) if v.half.x > 0.0 && v.half.y > 0.0 => [
                (x - (v.center.x - v.half.x)) / (2.0 * v.half.x),
                ((v.center.y + v.half.y) - y) / (2.0 * v.half.y),
            ],
            _ => [x, y],
        };
        let c = get_vec4(params, "color").unwrap_or([1.0, 0.95, 0.85, 1.0]);
        Self {
            source,
            length:    get_f32(params, "length").unwrap_or(1.0).clamp(0.01, 10.0),
            intensity: get_f32(params, "intensity").unwrap_or(0.8).max(0.0),
            color:     c,
            decay:     get_f32(params, "decay").unwrap_or(0.97).clamp(0.5, 1.0),
            threshold: get_f32(params, "threshold").unwrap_or(0.6).clamp(0.0, 0.99),
            aspect:    width.max(1) as f32 / height.max(1) as f32,
            _pad:      0.0,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct OutputU {
    exposure: f32,
    tonemap:  f32,
    dither:   f32,
    /// 1 when the scene is a pixel-perfect camera's low-resolution target:
    /// read nearest from `source`.
    nearest:  f32,
    /// The part of the scene shown, `u0, v0, du, dv`.
    source:   [f32; 4],
    /// The colour grade (`renderer::lut`): x strength (0 is none), y the
    /// strip's slices.
    lut:      [f32; 4],
    /// Film grain (`film_grain`): x intensity, y size in pixels, z this
    /// frame's seed.
    grain:    [f32; 4],
    /// FXAA (`fxaa`): x on, y the farthest it blurs along an edge, in
    /// texels.
    aa:       [f32; 4],
    /// A palette (`palette`, `renderer::palette`): x how many colours (0
    /// is none), y dither strength, z the dither's cell in pixels, w the
    /// cycle's shift.
    pal:      [f32; 4],
    /// The cycling indices: x first, y one past the last.
    cyc:      [f32; 4],
    /// The colours, sRGB.
    palette:  [[f32; 4]; crate::renderer::palette::MAX_COLORS],
    /// A painting (`paint`): x on, y radius in texels, z strength.
    paint:    [f32; 4],
    /// A halftone (`halftone`): x 0 off, 1 mono, 2 cmyk; y cell in pixels;
    /// z the mono screen's angle in radians; w strength.
    ht:       [f32; 4],
    /// Its ink and paper, linear.
    ink:      [f32; 4],
    paper:    [f32; 4],
    /// Line boil (`boil`): x amount in pixels (0 off), y scale in pixels,
    /// z this drawing's seed.
    boil:     [f32; 4],
    /// Lens rain (`lens_rain`): x how many drops (0 off), y drop size in
    /// pixels, z the time in seconds, w how fast they run.
    rain:     [f32; 4],
    /// x mist, y refraction.
    rain2:    [f32; 4],
}

/// The output pass's colour grade: the strip's texture, and what it holds.
struct LutGpu {
    texture: wgpu::Texture,
    view:    wgpu::TextureView,
    /// `renderer::lut::ActiveLut::key`, 0 for the 1x1 stand-in.
    key:     u64,
    slices:  f32,
    strength: f32,
    /// Pixels to write before the next output pass (`execute` has the queue).
    pending: std::cell::RefCell<Option<std::sync::Arc<Vec<u8>>>>,
}

impl LutGpu {
    fn texture(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("PostProcess color LUT"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // The strip is painted in sRGB; sampling decodes it to the
            // linear colour the output pass returns.
            format: TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// The 1x1 stand-in bound while no grade is set (strength 0).
    fn none(device: &wgpu::Device) -> Self {
        let (texture, view) = Self::texture(device, 1, 1);
        Self { texture, view, key: 0, slices: 0.0, strength: 0.0, pending: std::cell::RefCell::new(None) }
    }
}

fn get_f32(p: &std::collections::HashMap<String, UniformValue>, key: &str) -> Option<f32> {
    match p.get(key)? {
        UniformValue::Float(v) => Some(*v),
        UniformValue::Int(v)   => Some(*v as f32),
        _ => None,
    }
}
fn get_vec4(p: &std::collections::HashMap<String, UniformValue>, key: &str) -> Option<[f32; 4]> {
    match p.get(key)? {
        UniformValue::Vec4(v) => Some(*v),
        UniformValue::Vec3(v) => Some([v[0], v[1], v[2], 1.0]),
        _ => None,
    }
}

// ─── shared GPU helpers ───────────────────────────────────────────────

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type:    wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled:   false,
        },
        count: None,
    }
}

/// `(texture, sampler, uniform)` - every pass but the bloom composite.
fn single_layout(device: &wgpu::Device, label: &str) -> BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label:   Some(label),
        entries: &[
            texture_entry(0),
            wgpu::BindGroupLayoutEntry {
                binding:    1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty:         wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count:      None,
            },
            wgpu::BindGroupLayoutEntry {
                binding:    2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size:   None,
                },
                count: None,
            },
        ],
    })
}

/// [`single_layout`] plus the bloom chain's mip 0 at binding 3.
fn composite_layout(device: &wgpu::Device) -> BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label:   Some("PostProcess bloom composite"),
        entries: &[
            texture_entry(0),
            wgpu::BindGroupLayoutEntry {
                binding:    1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty:         wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count:      None,
            },
            wgpu::BindGroupLayoutEntry {
                binding:    2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size:   None,
                },
                count: None,
            },
            texture_entry(3),
        ],
    })
}

fn bind(
    device:  &wgpu::Device,
    layout:  &BindGroupLayout,
    src:     &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    uniform: &Buffer,
    extra:   Option<&wgpu::TextureView>,
    label:   &str,
) -> BindGroup {
    let mut entries = vec![
        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
    ];
    if let Some(view) = extra {
        entries.push(wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(view) });
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout, entries: &entries })
}

fn fullscreen_pipeline(
    device: &wgpu::Device,
    module: &ShaderModule,
    entry:  &str,
    layout: &BindGroupLayout,
    format: TextureFormat,
    blend:  Option<wgpu::BlendState>,
    label:  &str,
) -> RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label:                Some(label),
        bind_group_layouts:   &[layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point:         Some("vs_main"),
            buffers:             &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
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

fn uniform_buffer(device: &wgpu::Device, size: usize, label: &str) -> Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label:    Some(label),
        contents: &vec![0u8; size],
        usage:    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

fn draw_fullscreen(
    encoder:  &mut wgpu::CommandEncoder,
    target:   &wgpu::TextureView,
    load:     wgpu::LoadOp<wgpu::Color>,
    pipeline: &RenderPipeline,
    bind:     &BindGroup,
    label:    &str,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view:           target,
            resolve_target: None,
            ops:            wgpu::Operations { load, store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes:         None,
        occlusion_query_set:      None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind, &[]);
    pass.draw(0..3, 0..1);
}

#[derive(Copy, Clone, PartialEq, Eq)]
/// Which of the two targets a pass reads: `First` is `ping`, `Second` is `pong`.
enum Source { First, Second }

impl Source {
    fn flip(self) -> Self { match self { Source::First => Source::Second, Source::Second => Source::First } }
}

// ─── single-pass effects ──────────────────────────────────────────────

/// One full-screen pass reading ping or pong: pipeline, uniform, and a bind
/// group for each source.
struct Effect {
    pipeline:  RenderPipeline,
    uniform:   Buffer,
    bind_ping: BindGroup,
    bind_pong: BindGroup,
    label:     &'static str,
}

impl Effect {
    #[allow(clippy::too_many_arguments)]
    fn new(
        device:       &wgpu::Device,
        layout:       &BindGroupLayout,
        format:       TextureFormat,
        wgsl:         &str,
        label:        &'static str,
        uniform_size: usize,
        ping:         &WgpuRenderTarget,
        pong:         &WgpuRenderTarget,
        sampler:      &wgpu::Sampler,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some(label),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });
        let pipeline = fullscreen_pipeline(device, &module, "fs_main", layout, format, None, label);
        let uniform = uniform_buffer(device, uniform_size, label);
        let bind_ping = bind(device, layout, &ping.view, sampler, &uniform, None, label);
        let bind_pong = bind(device, layout, &pong.view, sampler, &uniform, None, label);
        Self { pipeline, uniform, bind_ping, bind_pong, label }
    }

    fn rebuild_binds(
        &mut self,
        device:  &wgpu::Device,
        layout:  &BindGroupLayout,
        ping:    &WgpuRenderTarget,
        pong:    &WgpuRenderTarget,
        sampler: &wgpu::Sampler,
    ) {
        self.rebuild_binds_with(device, layout, ping, pong, sampler, None);
    }

    /// [`Self::rebuild_binds`] with a second texture at binding 3.
    fn rebuild_binds_with(
        &mut self,
        device:  &wgpu::Device,
        layout:  &BindGroupLayout,
        ping:    &WgpuRenderTarget,
        pong:    &WgpuRenderTarget,
        sampler: &wgpu::Sampler,
        extra:   Option<&wgpu::TextureView>,
    ) {
        self.bind_ping = bind(device, layout, &ping.view, sampler, &self.uniform, extra, self.label);
        self.bind_pong = bind(device, layout, &pong.view, sampler, &self.uniform, extra, self.label);
    }

    /// [`Self::new`] with a second texture at binding 3.
    #[allow(clippy::too_many_arguments)]
    fn new_with(
        device:       &wgpu::Device,
        layout:       &BindGroupLayout,
        format:       TextureFormat,
        wgsl:         &str,
        label:        &'static str,
        uniform_size: usize,
        ping:         &WgpuRenderTarget,
        pong:         &WgpuRenderTarget,
        sampler:      &wgpu::Sampler,
        extra:        Option<&wgpu::TextureView>,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some(label),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });
        let pipeline = fullscreen_pipeline(device, &module, "fs_main", layout, format, None, label);
        let uniform = uniform_buffer(device, uniform_size, label);
        let bind_ping = bind(device, layout, &ping.view, sampler, &uniform, extra, label);
        let bind_pong = bind(device, layout, &pong.view, sampler, &uniform, extra, label);
        Self { pipeline, uniform, bind_ping, bind_pong, label }
    }

    fn bind_for(&self, src: Source) -> &BindGroup {
        match src {
            Source::First => &self.bind_ping,
            Source::Second => &self.bind_pong,
        }
    }
}

// ─── bloom ────────────────────────────────────────────────────────────

/// The bloom mip chain and its four pipelines. See `bloom.wgsl`.
struct Bloom {
    prefilter:        RenderPipeline,
    down:             RenderPipeline,
    /// Additive: each upsample adds onto what the downsample left.
    up:               RenderPipeline,
    composite:        RenderPipeline,
    composite_layout: BindGroupLayout,
    uniform:          Buffer,
    /// `mips[0]` is half the frame's size, each next one half again.
    mips:             Vec<WgpuRenderTarget>,
    prefilter_ping:   BindGroup,
    prefilter_pong:   BindGroup,
    /// `down_binds[i]` reads `mips[i]`, to write `mips[i + 1]`.
    down_binds:       Vec<BindGroup>,
    /// `up_binds[i]` reads `mips[i + 1]`, to add onto `mips[i]`.
    up_binds:         Vec<BindGroup>,
    composite_ping:   BindGroup,
    composite_pong:   BindGroup,
}

/// The sizes of the bloom chain for a `width` x `height` frame.
fn bloom_mip_sizes(width: u32, height: u32) -> Vec<(u32, u32)> {
    let mut sizes = Vec::new();
    let (mut w, mut h) = (width, height);
    while sizes.len() < MAX_BLOOM_MIPS {
        w = (w / 2).max(1);
        h = (h / 2).max(1);
        if w < MIN_BLOOM_MIP_SIDE || h < MIN_BLOOM_MIP_SIDE {
            break;
        }
        sizes.push((w, h));
    }
    if sizes.is_empty() {
        sizes.push(((width / 2).max(1), (height / 2).max(1)));
    }
    sizes
}

impl Bloom {
    #[allow(clippy::too_many_arguments)]
    fn new(
        device:  &wgpu::Device,
        layout:  &BindGroupLayout,
        format:  TextureFormat,
        width:   u32,
        height:  u32,
        ping:    &WgpuRenderTarget,
        pong:    &WgpuRenderTarget,
        sampler: &wgpu::Sampler,
    ) -> Result<Self> {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("PostProcess bloom"),
            source: wgpu::ShaderSource::Wgsl(BLOOM_WGSL.into()),
        });
        let composite_layout = composite_layout(device);
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation:  wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let prefilter = fullscreen_pipeline(device, &module, "fs_prefilter", layout, format, None, "bloom prefilter");
        let down = fullscreen_pipeline(device, &module, "fs_down", layout, format, None, "bloom down");
        let up = fullscreen_pipeline(device, &module, "fs_up", layout, format, Some(additive), "bloom up");
        let composite = fullscreen_pipeline(
            device, &module, "fs_composite", &composite_layout, format, None, "bloom composite",
        );
        let uniform = uniform_buffer(device, std::mem::size_of::<BloomU>(), "bloom uniform");
        let mips = Self::make_mips(device, format, width, height)?;
        let (prefilter_ping, prefilter_pong, down_binds, up_binds, composite_ping, composite_pong) =
            Self::make_binds(device, layout, &composite_layout, &mips, ping, pong, sampler, &uniform);
        Ok(Self {
            prefilter, down, up, composite, composite_layout, uniform, mips,
            prefilter_ping, prefilter_pong, down_binds, up_binds, composite_ping, composite_pong,
        })
    }

    fn make_mips(device: &wgpu::Device, format: TextureFormat, width: u32, height: u32) -> Result<Vec<WgpuRenderTarget>> {
        bloom_mip_sizes(width, height)
            .into_iter()
            .map(|(w, h)| WgpuRenderTarget::new(device, w, h, format))
            .collect()
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn make_binds(
        device:           &wgpu::Device,
        layout:           &BindGroupLayout,
        composite_layout: &BindGroupLayout,
        mips:             &[WgpuRenderTarget],
        ping:             &WgpuRenderTarget,
        pong:             &WgpuRenderTarget,
        sampler:          &wgpu::Sampler,
        uniform:          &Buffer,
    ) -> (BindGroup, BindGroup, Vec<BindGroup>, Vec<BindGroup>, BindGroup, BindGroup) {
        let prefilter_ping = bind(device, layout, &ping.view, sampler, uniform, None, "bloom prefilter ping");
        let prefilter_pong = bind(device, layout, &pong.view, sampler, uniform, None, "bloom prefilter pong");
        let mut down_binds = Vec::new();
        let mut up_binds = Vec::new();
        for i in 0..mips.len().saturating_sub(1) {
            down_binds.push(bind(device, layout, &mips[i].view, sampler, uniform, None, "bloom down"));
            up_binds.push(bind(device, layout, &mips[i + 1].view, sampler, uniform, None, "bloom up"));
        }
        let composite_ping = bind(
            device, composite_layout, &ping.view, sampler, uniform, Some(&mips[0].view), "bloom composite ping",
        );
        let composite_pong = bind(
            device, composite_layout, &pong.view, sampler, uniform, Some(&mips[0].view), "bloom composite pong",
        );
        (prefilter_ping, prefilter_pong, down_binds, up_binds, composite_ping, composite_pong)
    }

    #[allow(clippy::too_many_arguments)]
    fn resize(
        &mut self,
        device:  &wgpu::Device,
        layout:  &BindGroupLayout,
        format:  TextureFormat,
        width:   u32,
        height:  u32,
        ping:    &WgpuRenderTarget,
        pong:    &WgpuRenderTarget,
        sampler: &wgpu::Sampler,
    ) -> Result<()> {
        self.mips = Self::make_mips(device, format, width, height)?;
        let (a, b, c, d, e, f) = Self::make_binds(
            device, layout, &self.composite_layout, &self.mips, ping, pong, sampler, &self.uniform,
        );
        self.prefilter_ping = a;
        self.prefilter_pong = b;
        self.down_binds = c;
        self.up_binds = d;
        self.composite_ping = e;
        self.composite_pong = f;
        Ok(())
    }

    fn run(&self, encoder: &mut wgpu::CommandEncoder, src: Source, dst: &wgpu::TextureView) {
        let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        let prefilter_bind = match src {
            Source::First => &self.prefilter_ping,
            Source::Second => &self.prefilter_pong,
        };
        draw_fullscreen(encoder, &self.mips[0].view, clear, &self.prefilter, prefilter_bind, "bloom prefilter");
        for (i, b) in self.down_binds.iter().enumerate() {
            draw_fullscreen(encoder, &self.mips[i + 1].view, clear, &self.down, b, "bloom down");
        }
        for (i, b) in self.up_binds.iter().enumerate().rev() {
            draw_fullscreen(encoder, &self.mips[i].view, wgpu::LoadOp::Load, &self.up, b, "bloom up");
        }
        let composite_bind = match src {
            Source::First => &self.composite_ping,
            Source::Second => &self.composite_pong,
        };
        draw_fullscreen(encoder, dst, wgpu::LoadOp::Load, &self.composite, composite_bind, "bloom composite");
    }
}

// ─── lens flare ───────────────────────────────────────────────────────

/// `lens_flare`'s five passes and its two quarter-size targets. See
/// `lens_flare.wgsl`: the bright part of the frame into `a`, blurred through
/// `b` back into `a`, the features read from `a` into `b`, then the frame
/// plus `b`.
struct LensFlare {
    bright:           RenderPipeline,
    blur_h:           RenderPipeline,
    blur_v:           RenderPipeline,
    features:         RenderPipeline,
    composite:        RenderPipeline,
    composite_layout: BindGroupLayout,
    uniform:          Buffer,
    a:                WgpuRenderTarget,
    b:                WgpuRenderTarget,
    bright_ping:      BindGroup,
    bright_pong:      BindGroup,
    read_a:           BindGroup,
    read_b:           BindGroup,
    composite_ping:   BindGroup,
    composite_pong:   BindGroup,
}

impl LensFlare {
    #[allow(clippy::too_many_arguments)]
    fn new(
        device:  &wgpu::Device,
        layout:  &BindGroupLayout,
        format:  TextureFormat,
        width:   u32,
        height:  u32,
        ping:    &WgpuRenderTarget,
        pong:    &WgpuRenderTarget,
        sampler: &wgpu::Sampler,
    ) -> Result<Self> {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("PostProcess lens_flare"),
            source: wgpu::ShaderSource::Wgsl(LENS_FLARE_WGSL.into()),
        });
        let composite_layout = composite_layout(device);
        let pipe = |entry: &str, l: &BindGroupLayout| fullscreen_pipeline(device, &module, entry, l, format, None, entry);
        let (bright, blur_h, blur_v, features) =
            (pipe("fs_bright", layout), pipe("fs_blur_h", layout), pipe("fs_blur_v", layout), pipe("fs_features", layout));
        let composite = pipe("fs_composite", &composite_layout);
        let uniform = uniform_buffer(device, std::mem::size_of::<LensFlareU>(), "lens_flare uniform");
        let (w, h) = lens_flare_size(width, height);
        let a = WgpuRenderTarget::new(device, w, h, format)?;
        let b = WgpuRenderTarget::new(device, w, h, format)?;
        let (bright_ping, bright_pong, read_a, read_b, composite_ping, composite_pong) =
            Self::make_binds(device, layout, &composite_layout, &a, &b, ping, pong, sampler, &uniform);
        Ok(Self {
            bright, blur_h, blur_v, features, composite, composite_layout, uniform, a, b,
            bright_ping, bright_pong, read_a, read_b, composite_ping, composite_pong,
        })
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn make_binds(
        device:           &wgpu::Device,
        layout:           &BindGroupLayout,
        composite_layout: &BindGroupLayout,
        a:                &WgpuRenderTarget,
        b:                &WgpuRenderTarget,
        ping:             &WgpuRenderTarget,
        pong:             &WgpuRenderTarget,
        sampler:          &wgpu::Sampler,
        uniform:          &Buffer,
    ) -> (BindGroup, BindGroup, BindGroup, BindGroup, BindGroup, BindGroup) {
        (
            bind(device, layout, &ping.view, sampler, uniform, None, "lens_flare bright ping"),
            bind(device, layout, &pong.view, sampler, uniform, None, "lens_flare bright pong"),
            bind(device, layout, &a.view, sampler, uniform, None, "lens_flare read a"),
            bind(device, layout, &b.view, sampler, uniform, None, "lens_flare read b"),
            bind(device, composite_layout, &ping.view, sampler, uniform, Some(&b.view), "lens_flare composite ping"),
            bind(device, composite_layout, &pong.view, sampler, uniform, Some(&b.view), "lens_flare composite pong"),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn resize(
        &mut self,
        device:  &wgpu::Device,
        layout:  &BindGroupLayout,
        format:  TextureFormat,
        width:   u32,
        height:  u32,
        ping:    &WgpuRenderTarget,
        pong:    &WgpuRenderTarget,
        sampler: &wgpu::Sampler,
    ) -> Result<()> {
        let (w, h) = lens_flare_size(width, height);
        self.a = WgpuRenderTarget::new(device, w, h, format)?;
        self.b = WgpuRenderTarget::new(device, w, h, format)?;
        let (p, q, r, s, t, v) = Self::make_binds(
            device, layout, &self.composite_layout, &self.a, &self.b, ping, pong, sampler, &self.uniform,
        );
        self.bright_ping = p;
        self.bright_pong = q;
        self.read_a = r;
        self.read_b = s;
        self.composite_ping = t;
        self.composite_pong = v;
        Ok(())
    }

    fn run(&self, encoder: &mut wgpu::CommandEncoder, src: Source, dst: &wgpu::TextureView) {
        let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        let (bright, composite) = match src {
            Source::First => (&self.bright_ping, &self.composite_ping),
            Source::Second => (&self.bright_pong, &self.composite_pong),
        };
        draw_fullscreen(encoder, &self.a.view, clear, &self.bright, bright, "lens_flare bright");
        draw_fullscreen(encoder, &self.b.view, clear, &self.blur_h, &self.read_a, "lens_flare blur h");
        draw_fullscreen(encoder, &self.a.view, clear, &self.blur_v, &self.read_b, "lens_flare blur v");
        draw_fullscreen(encoder, &self.b.view, clear, &self.features, &self.read_a, "lens_flare features");
        draw_fullscreen(encoder, dst, wgpu::LoadOp::Load, &self.composite, composite, "lens_flare composite");
    }
}

// ─── stack ────────────────────────────────────────────────────────────

enum Pass<'a> {
    Single(&'a Effect),
    Bloom,
    LensFlare,
}

pub struct WgpuPostProcessStack {
    /// First target: scene + lights composite write here via [`Self::scene_target`].
    ping:           WgpuRenderTarget,
    /// Second target: the other half of the ping-pong pair for chained effects.
    pong:           WgpuRenderTarget,
    /// Linear, clamp-to-edge: every pass samples through it.
    sampler:        wgpu::Sampler,
    layout:         BindGroupLayout,

    bloom:          Bloom,
    vignette:       Effect,
    color_grade:    Effect,
    chromatic:      Effect,
    pixelate:       Effect,
    crt:            Effect,
    light_shafts:   Effect,
    tilt_shift:     Effect,
    blur:           Effect,
    /// `motion_blur`, reading `velocity` (binding 3, `composite_layout`).
    motion_blur:    Effect,
    /// `depth_of_field`, reading each pixel's z from `velocity`'s blue.
    depth_of_field: Effect,
    lens_flare:     LensFlare,
    motion_layout:  BindGroupLayout,
    /// Each pixel's motion across the screen, drawn by the velocity pass
    /// (`ecs::wgpu_systems::render_velocity_wgpu`) when `motion_blur` is on.
    velocity:       WgpuRenderTarget,
    /// Scene -> display: exposure, tonemap, colour grade, grain, dither.
    /// Always the last pass; bound with the LUT (`composite_layout`).
    output:         Effect,
    output_layout:  BindGroupLayout,
    lut:            LutGpu,
    /// Counts frames, for the grain's seed.
    frame:          std::cell::Cell<u32>,

    /// Current effect chain (scene-authored + script overrides). Read
    /// each frame in [`Self::execute`].
    settings:       PostProcessSettings,
    /// This frame's blend of `settings` with the post volumes the camera is
    /// in (`ecs::post_volume`), drawn instead of them; `None` draws them.
    blended:        Option<PostProcessSettings>,

    scene_format:   TextureFormat,
    output_format:  TextureFormat,
    /// A pixel-perfect camera's `PixelPlan::source`, or `None` to show the
    /// whole scene as it is.
    source:         Option<[f32; 4]>,
}

impl WgpuPostProcessStack {
    /// `scene_format` is what the world and every effect draw in
    /// (`WgpuRenderer::scene_format`); `output_format` is the display view's
    /// (`WgpuRenderer::color_format`), which only the output pass writes.
    pub fn new(
        device:        &wgpu::Device,
        width:         u32,
        height:        u32,
        scene_format:  TextureFormat,
        output_format: TextureFormat,
    ) -> Result<Self> {
        let ping = WgpuRenderTarget::new(device, width, height, scene_format)?;
        let pong = WgpuRenderTarget::new(device, width, height, scene_format)?;
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label:          Some("PostProcess sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter:     wgpu::FilterMode::Linear,
            min_filter:     wgpu::FilterMode::Linear,
            mipmap_filter:  wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let layout = single_layout(device, "PostProcess layout");

        let bloom = Bloom::new(device, &layout, scene_format, width, height, &ping, &pong, &sampler)?;
        let vignette = Effect::new(
            device, &layout, scene_format, VIGNETTE_WGSL, "PostProcess vignette",
            std::mem::size_of::<VignetteU>(), &ping, &pong, &sampler,
        );
        let color_grade = Effect::new(
            device, &layout, scene_format, COLOR_GRADE_WGSL, "PostProcess color_grade",
            std::mem::size_of::<ColorGradeU>(), &ping, &pong, &sampler,
        );
        let chromatic = Effect::new(
            device, &layout, scene_format, CHROMATIC_WGSL, "PostProcess chromatic",
            std::mem::size_of::<ChromaticU>(), &ping, &pong, &sampler,
        );
        let pixelate = Effect::new(
            device, &layout, scene_format, PIXELATE_WGSL, "PostProcess pixelate",
            std::mem::size_of::<PixelateU>(), &ping, &pong, &sampler,
        );
        let crt = Effect::new(
            device, &layout, scene_format, CRT_WGSL, "PostProcess crt",
            std::mem::size_of::<CrtU>(), &ping, &pong, &sampler,
        );
        let tilt_shift = Effect::new(
            device, &layout, scene_format, TILT_SHIFT_WGSL, "PostProcess tilt_shift",
            std::mem::size_of::<TiltShiftU>(), &ping, &pong, &sampler,
        );
        let blur = Effect::new(
            device, &layout, scene_format, TILT_SHIFT_WGSL, "PostProcess blur",
            std::mem::size_of::<TiltShiftU>(), &ping, &pong, &sampler,
        );
        let light_shafts = Effect::new(
            device, &layout, scene_format, LIGHT_SHAFTS_WGSL, "PostProcess light_shafts",
            std::mem::size_of::<LightShaftsU>(), &ping, &pong, &sampler,
        );
        let lens_flare = LensFlare::new(device, &layout, scene_format, width, height, &ping, &pong, &sampler)?;
        let motion_layout = composite_layout(device);
        let velocity = WgpuRenderTarget::new(device, width, height, VELOCITY_FORMAT)?;
        let motion_blur = Effect::new_with(
            device, &motion_layout, scene_format, MOTION_BLUR_WGSL, "PostProcess motion_blur",
            std::mem::size_of::<MotionBlurU>(), &ping, &pong, &sampler, Some(&velocity.view),
        );
        let depth_of_field = Effect::new_with(
            device, &motion_layout, scene_format, DEPTH_OF_FIELD_WGSL, "PostProcess depth_of_field",
            std::mem::size_of::<DepthOfFieldU>(), &ping, &pong, &sampler, Some(&velocity.view),
        );
        let output_layout = composite_layout(device);
        let lut = LutGpu::none(device);
        let output = Effect::new_with(
            device, &output_layout, output_format, OUTPUT_WGSL, "PostProcess output",
            std::mem::size_of::<OutputU>(), &ping, &pong, &sampler, Some(&lut.view),
        );

        Ok(Self {
            ping,
            pong,
            sampler,
            layout,
            bloom,
            vignette,
            color_grade,
            chromatic,
            pixelate,
            crt,
            light_shafts,
            tilt_shift,
            blur,
            motion_blur,
            depth_of_field,
            lens_flare,
            motion_layout,
            velocity,
            output,
            output_layout,
            lut,
            frame: std::cell::Cell::new(0),
            settings: PostProcessSettings::default(),
            blended: None,
            scene_format,
            output_format,
            source: None,
        })
    }

    /// The part of the scene target the output pass shows, scaled up
    /// nearest - a pixel-perfect camera's `PixelPlan::source` - or `None`
    /// for the whole target as it is. Set every frame by the hosts.
    pub fn set_source(&mut self, source: Option<[f32; 4]>) {
        self.source = source;
    }

    /// The render target the scene should be drawn into each frame.
    pub fn scene_target(&self) -> &WgpuRenderTarget { &self.ping }

    /// Whether `motion_blur` is on - whether a host draws the velocity pass.
    pub fn motion_blur_on(&self) -> bool {
        self.active().effects.iter().any(|e| e.enabled && e.name == "motion_blur")
    }

    /// Whether `depth_of_field` is on - the velocity pass draws for it too.
    pub fn depth_of_field_on(&self) -> bool {
        self.active().effects.iter().any(|e| e.enabled && e.name == "depth_of_field")
    }

    /// The z the velocity pass is cleared to where no sprite covers
    /// (`depth_of_field.wgsl`'s `UNKNOWN`): sharp itself, behind every layer
    /// - so a soft layer's edge spreads over empty space - and spreading onto
    /// nothing. Text, particles and meshes, which the pass does not draw,
    /// take the depth of the sprite under them, or this.
    pub fn depth_clear(&self) -> f32 {
        -65000.0
    }

    /// The velocity pass's target ([`VELOCITY_FORMAT`], the scene's size).
    pub fn velocity_target(&self) -> &WgpuRenderTarget { &self.velocity }

    /// The format the scene and every effect draw in.
    pub fn scene_format(&self) -> TextureFormat { self.scene_format }

    /// The display format the output pass writes.
    pub fn output_format(&self) -> TextureFormat { self.output_format }

    /// Whether this frame is HDR: the scene asked for it *and* the scene
    /// target is a float format. Hosts pass it to
    /// `WgpuLightingPass::set_hdr`, so light is clamped exactly when the
    /// output would not tonemap it.
    pub fn hdr_active(&self) -> bool {
        self.active().hdr && is_hdr_format(self.scene_format)
    }

    /// Replace the entire effect chain (typically called on scene load).
    pub fn set_settings(&mut self, settings: PostProcessSettings) {
        self.settings = settings;
    }

    /// Read-only view of the current chain — useful for inspector UIs.
    pub fn settings(&self) -> &PostProcessSettings { &self.settings }

    /// Draw `blended` this frame instead of the settings (`ecs::post_volume`),
    /// or the settings themselves with `None`. The settings - the scene's,
    /// with every script change - are left alone.
    pub fn set_blended(&mut self, blended: Option<PostProcessSettings>) {
        self.blended = blended;
    }

    /// What is drawn: the post volumes' blend when there is one, else the
    /// settings.
    pub fn active(&self) -> &PostProcessSettings {
        self.blended.as_ref().unwrap_or(&self.settings)
    }

    /// Toggle a named effect's `enabled` flag. Inserts the effect with
    /// default params if not already present so subsequent `set_effect_param`
    /// calls are not lost.
    pub fn set_effect_enabled(&mut self, name: &str, enabled: bool) {
        if name == "hdr" {
            self.settings.hdr = enabled;
            return;
        }
        if let Some(e) = self.settings.effects.iter_mut().find(|e| e.name == name) {
            e.enabled = enabled;
        } else {
            self.settings.effects.push(PostProcessEffectSettings {
                name:    name.to_string(),
                enabled,
                params:  Default::default(),
            });
        }
    }

    /// Set one named uniform on a named effect. Inserts the effect with
    /// default (enabled) params if it does not exist yet.
    ///
    /// The output pass is addressed as the effect `"tonemap"`: its
    /// `"exposure"` is the scene's exposure, and its `"mode"` is 0 none,
    /// 1 reinhard, 2 aces, 3 agx - so a script can grade exposure with
    /// `PostProcess.set_float("tonemap", "exposure", 1.4)`.
    pub fn set_effect_param(&mut self, name: &str, param: &str, value: UniformValue) {
        if name == "tonemap" {
            let v = match value {
                UniformValue::Float(v) => v,
                UniformValue::Int(v) => v as f32,
                _ => return,
            };
            match param {
                "exposure" => self.settings.exposure = Some(v.max(0.0)),
                "mode" => {
                    self.settings.tonemap = Some(match v.round() as i32 {
                        1 => Tonemap::Reinhard,
                        2 => Tonemap::Aces,
                        3 => Tonemap::Agx,
                        _ => Tonemap::None,
                    })
                }
                _ => {}
            }
            return;
        }
        if let Some(e) = self.settings.effects.iter_mut().find(|e| e.name == name) {
            e.params.insert(param.to_string(), value);
        } else {
            let mut params = std::collections::HashMap::new();
            params.insert(param.to_string(), value);
            self.settings.effects.push(PostProcessEffectSettings {
                name:    name.to_string(),
                enabled: true,
                params,
            });
        }
    }

    /// Resize ping + pong and the bloom chain, and rebuild every cached
    /// bind group.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let resized = width != self.ping.width || height != self.ping.height;
        self.ping.resize(device, width, height);
        self.pong.resize(device, width, height);
        if resized {
            let (l, p, q, s) = (&self.layout, &self.ping, &self.pong, &self.sampler);
            if let Err(e) = self.bloom.resize(device, l, self.scene_format, width, height, p, q, s) {
                log::error!("[post] bloom chain resize failed: {e:#}");
            }
            self.vignette   .rebuild_binds(device, l, p, q, s);
            self.color_grade.rebuild_binds(device, l, p, q, s);
            self.chromatic  .rebuild_binds(device, l, p, q, s);
            self.pixelate   .rebuild_binds(device, l, p, q, s);
            self.light_shafts.rebuild_binds(device, l, p, q, s);
            self.tilt_shift.rebuild_binds(device, l, p, q, s);
            self.blur.rebuild_binds(device, l, p, q, s);
            self.crt        .rebuild_binds(device, l, p, q, s);
            if let Err(e) = self.lens_flare.resize(device, l, self.scene_format, width, height, p, q, s) {
                log::error!("[post] lens flare resize failed: {e:#}");
            }
            self.velocity.resize(device, width, height);
            self.motion_blur.rebuild_binds_with(device, &self.motion_layout, p, q, s, Some(&self.velocity.view));
            self.depth_of_field.rebuild_binds_with(device, &self.motion_layout, p, q, s, Some(&self.velocity.view));
            self.output.rebuild_binds_with(device, &self.output_layout, p, q, s, Some(&self.lut.view));
        }
    }

    /// Take the frame's colour grade (`renderer::lut::active`): a new strip
    /// gets a texture of its own and the output pass is rebound to it, its
    /// pixels written by the next [`Self::execute`]; none binds the 1x1
    /// stand-in at strength 0. Cheap when nothing changed. Called once a
    /// frame by `ecs::wgpu_systems::apply_pixel_plan`, which every host
    /// calls for every camera.
    pub fn sync_lut(&mut self, device: &wgpu::Device) {
        let active = crate::renderer::lut::active();
        let key = active.as_ref().map_or(0, |l| l.key);
        if key != self.lut.key {
            self.lut = match &active {
                Some(l) => {
                    let (texture, view) = LutGpu::texture(device, l.width, l.height);
                    LutGpu {
                        texture,
                        view,
                        key,
                        slices: l.height as f32,
                        strength: l.strength,
                        pending: std::cell::RefCell::new(Some(l.rgba.clone())),
                    }
                }
                None => LutGpu::none(device),
            };
            self.output.rebuild_binds_with(device, &self.output_layout, &self.ping, &self.pong, &self.sampler, Some(&self.lut.view));
        }
        self.lut.strength = active.map_or(0.0, |l| l.strength);
    }

    /// Run the effect chain in the scene format, then the output pass into
    /// `output_view` (the display, in `output_format`).
    pub fn execute(
        &self,
        queue:       &wgpu::Queue,
        encoder:     &mut wgpu::CommandEncoder,
        output_view: &wgpu::TextureView,
    ) {
        // The scene's settings, or a post volume's blend of them.
        let settings = self.active();
        // Resolve the chain in authored order, skipping disabled and
        // unknown names, and push each effect's uniforms.
        let mut chain: Vec<Pass<'_>> = Vec::with_capacity(settings.effects.len());
        for s in &settings.effects {
            if !s.enabled { continue; }
            match s.name.as_str() {
                "bloom" => {
                    let u = BloomU::from_params(&s.params, self.bloom.mips.len());
                    queue.write_buffer(&self.bloom.uniform, 0, bytemuck::bytes_of(&u));
                    chain.push(Pass::Bloom);
                }
                "vignette" => {
                    let u = VignetteU::from_params(&s.params);
                    queue.write_buffer(&self.vignette.uniform, 0, bytemuck::bytes_of(&u));
                    chain.push(Pass::Single(&self.vignette));
                }
                "color_grade" => {
                    let u = ColorGradeU::from_params(&s.params);
                    queue.write_buffer(&self.color_grade.uniform, 0, bytemuck::bytes_of(&u));
                    chain.push(Pass::Single(&self.color_grade));
                }
                "chromatic" => {
                    let u = ChromaticU::from_params(&s.params);
                    queue.write_buffer(&self.chromatic.uniform, 0, bytemuck::bytes_of(&u));
                    chain.push(Pass::Single(&self.chromatic));
                }
                "pixelate" => {
                    let view = crate::renderer::frame_view::frame_view();
                    let u = PixelateU::new(&s.params, self.ping.width, self.ping.height, view);
                    queue.write_buffer(&self.pixelate.uniform, 0, bytemuck::bytes_of(&u));
                    chain.push(Pass::Single(&self.pixelate));
                }
                "tilt_shift" => {
                    let view = crate::renderer::frame_view::frame_view();
                    let u = TiltShiftU::new(&s.params, self.ping.width, self.ping.height, view);
                    queue.write_buffer(&self.tilt_shift.uniform, 0, bytemuck::bytes_of(&u));
                    chain.push(Pass::Single(&self.tilt_shift));
                }
                "blur" => {
                    let view = crate::renderer::frame_view::frame_view();
                    let u = BlurU::uniform(&s.params, self.ping.width, self.ping.height, view);
                    queue.write_buffer(&self.blur.uniform, 0, bytemuck::bytes_of(&u));
                    chain.push(Pass::Single(&self.blur));
                }
                "light_shafts" => {
                    let view = crate::renderer::frame_view::frame_view();
                    let u = LightShaftsU::new(&s.params, self.ping.width, self.ping.height, view);
                    queue.write_buffer(&self.light_shafts.uniform, 0, bytemuck::bytes_of(&u));
                    chain.push(Pass::Single(&self.light_shafts));
                }
                "lens_flare" => {
                    let u = LensFlareU::new(&s.params, self.ping.width, self.ping.height);
                    queue.write_buffer(&self.lens_flare.uniform, 0, bytemuck::bytes_of(&u));
                    chain.push(Pass::LensFlare);
                }
                "motion_blur" => {
                    let u = MotionBlurU::from_params(&s.params, self.ping.width);
                    queue.write_buffer(&self.motion_blur.uniform, 0, bytemuck::bytes_of(&u));
                    chain.push(Pass::Single(&self.motion_blur));
                }
                "depth_of_field" => {
                    let u = DepthOfFieldU::from_params(&s.params);
                    queue.write_buffer(&self.depth_of_field.uniform, 0, bytemuck::bytes_of(&u));
                    chain.push(Pass::Single(&self.depth_of_field));
                }
                "crt" => {
                    let u = CrtU::from_params(&s.params);
                    queue.write_buffer(&self.crt.uniform, 0, bytemuck::bytes_of(&u));
                    chain.push(Pass::Single(&self.crt));
                }
                _ => {}
            }
        }

        let mut src = Source::First;
        for pass in &chain {
            let dst = src.flip();
            let dst_view = match dst {
                Source::First => &self.ping.view,
                Source::Second => &self.pong.view,
            };
            match pass {
                Pass::Single(effect) => draw_fullscreen(
                    encoder, dst_view, wgpu::LoadOp::Load, &effect.pipeline, effect.bind_for(src), effect.label,
                ),
                Pass::Bloom => self.bloom.run(encoder, src, dst_view),
                Pass::LensFlare => self.lens_flare.run(encoder, src, dst_view),
            }
            src = dst;
        }

        // A new colour grade's pixels, once.
        if let Some(rgba) = self.lut.pending.borrow_mut().take() {
            let size = self.lut.texture.size();
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &self.lut.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba,
                wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(size.width * 4), rows_per_image: Some(size.height) },
                size,
            );
        }
        // Film grain: an effect in the chain's list, drawn here, after the
        // tonemap, in display space.
        let grain = settings
            .effects
            .iter()
            .find(|e| e.enabled && e.name == "film_grain")
            .map_or([0.0; 4], |e| {
                let frame = self.frame.get().wrapping_add(1);
                self.frame.set(frame);
                [
                    get_f32(&e.params, "intensity").unwrap_or(0.08).max(0.0),
                    get_f32(&e.params, "size").unwrap_or(1.5).max(1.0),
                    (frame % 997) as f32,
                    0.0,
                ]
            });

        // FXAA (Oct 2026): an effect in the chain's list, drawn here on the
        // tonemapped colour, so it smooths the edges the display shows.
        let aa = settings
            .effects
            .iter()
            .find(|e| e.enabled && e.name == "fxaa")
            .map_or([0.0; 4], |e| [1.0, get_f32(&e.params, "span").unwrap_or(8.0).clamp(1.0, 16.0), 0.0, 0.0]);

        // A palette (Oct 2026): an effect in the chain's list, applied here
        // last, so the display gets exactly its colours.
        let palette = settings
            .effects
            .iter()
            .find(|e| e.enabled && e.name == "palette")
            .map(|e| crate::renderer::palette::Palette::from_params(&e.params));
        let (pal, cyc, colors) = match &palette {
            Some(p) => (
                [p.count as f32, p.dither, p.cell, p.shift(crate::profiling::now_ms() / 1000.0)],
                [p.cycle.0 as f32, p.cycle.1 as f32, 0.0, 0.0],
                p.colors,
            ),
            None => ([0.0; 4], [0.0; 4], [[0.0; 4]; crate::renderer::palette::MAX_COLORS]),
        };

        // A painting and a print (Oct 2026): effects in the chain's list,
        // drawn here on the tonemapped colour, the frame the display shows.
        let paint = settings
            .effects
            .iter()
            .find(|e| e.enabled && e.name == "paint")
            .map_or([0.0; 4], |e| [
                1.0,
                get_f32(&e.params, "radius").unwrap_or(4.0).clamp(1.0, 16.0),
                get_f32(&e.params, "strength").unwrap_or(1.0).clamp(0.0, 1.0),
                0.0,
            ]);
        let (ht, ink, paper) = settings
            .effects
            .iter()
            .find(|e| e.enabled && e.name == "halftone")
            .map_or(([0.0; 4], [0.0; 4], [0.0; 4]), |e| (
                [
                    if get_f32(&e.params, "mode").unwrap_or(0.0) >= 0.5 { 2.0 } else { 1.0 },
                    get_f32(&e.params, "cell").unwrap_or(6.0).clamp(2.0, 64.0),
                    get_f32(&e.params, "angle").unwrap_or(45.0).to_radians(),
                    get_f32(&e.params, "strength").unwrap_or(1.0).clamp(0.0, 1.0),
                ],
                get_vec4(&e.params, "ink").unwrap_or([0.02, 0.02, 0.03, 1.0]),
                get_vec4(&e.params, "paper").unwrap_or([0.86, 0.82, 0.70, 1.0]),
            ));

        // Line boil (Oct 2026): a seed a drawing, `rate` drawings a second
        // on the wall clock - or a fixed `seed`, for a test or a script.
        let boil = settings
            .effects
            .iter()
            .find(|e| e.enabled && e.name == "boil")
            .map_or([0.0; 4], |e| [
                get_f32(&e.params, "amount").unwrap_or(1.5).clamp(0.0, 16.0),
                get_f32(&e.params, "scale").unwrap_or(24.0).max(1.0),
                get_f32(&e.params, "seed").unwrap_or_else(|| {
                    (crate::profiling::now_ms() / 1000.0 * get_f32(&e.params, "rate").unwrap_or(8.0).max(0.0) as f64).floor() as f32 % 997.0
                }),
                0.0,
            ]);

        // Lens rain (Oct 2026): worked out from the clock - the wall clock,
        // or a fixed `time`, for a test or a script.
        let (rain, rain2) = settings
            .effects
            .iter()
            .find(|e| e.enabled && e.name == "lens_rain")
            .map_or(([0.0; 4], [0.0; 4]), |e| (
                [
                    get_f32(&e.params, "amount").unwrap_or(0.6).clamp(0.0, 1.0),
                    get_f32(&e.params, "size").unwrap_or(10.0).clamp(2.0, 64.0),
                    get_f32(&e.params, "time").unwrap_or_else(|| (crate::profiling::now_ms() / 1000.0 % 3600.0) as f32),
                    get_f32(&e.params, "speed").unwrap_or(1.0).max(0.0),
                ],
                [
                    get_f32(&e.params, "mist").unwrap_or(0.0).clamp(0.0, 1.0),
                    get_f32(&e.params, "refraction").unwrap_or(1.0).max(0.0),
                    0.0,
                    0.0,
                ],
            ));

        let hdr = self.hdr_active();
        let out = OutputU {
            exposure: if hdr { settings.exposure() } else { 1.0 },
            tonemap:  settings.effective_tonemap(hdr).shader_index(),
            // Only an 8-bit display bands; a float one would show the noise.
            // A palette's colours are exact, so none under it either.
            dither:   if is_hdr_format(self.output_format) || palette.is_some() { 0.0 } else { 1.0 },
            nearest:  if self.source.is_some() { 1.0 } else { 0.0 },
            source:   self.source.unwrap_or([0.0, 0.0, 1.0, 1.0]),
            lut:      [if self.lut.slices >= 2.0 { self.lut.strength } else { 0.0 }, self.lut.slices, 0.0, 0.0],
            grain,
            aa,
            pal,
            cyc,
            palette: colors,
            paint,
            ht,
            ink,
            paper,
            boil,
            rain,
            rain2,
        };
        queue.write_buffer(&self.output.uniform, 0, bytemuck::bytes_of(&out));
        draw_fullscreen(
            encoder, output_view, wgpu::LoadOp::Load, &self.output.pipeline, self.output.bind_for(src), "PostProcess output",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::renderer::frame_view::FrameView;
    use glam::Vec2;

    #[test]
    fn a_pixelate_cell_is_one_art_pixel_at_any_height() {
        let p = std::collections::HashMap::new();
        assert_eq!(PixelateU::new(&p, 1280, 720, None).cell, 4.0);
        assert_eq!(PixelateU::new(&p, 1920, 1080, None).cell, 6.0);
        // Between whole multiples a cell is fractional - still one art
        // pixel, as a nearest upscale to that height would be.
        assert!((PixelateU::new(&p, 1600, 1000, None).cell - 5.5556).abs() < 1e-3);
    }

    #[test]
    fn the_pixelate_grid_moves_with_the_world() {
        // A 1280x720 view of 1280x720 world units: a camera 1 unit right of
        // the origin puts world (0, 0) one pixel left of the middle, so the
        // grid's origin moves one pixel left with it.
        let p = std::collections::HashMap::new();
        let at = |x: f32| {
            let v = FrameView {
                center: Vec2::new(x, 0.0),
                half: Vec2::new(640.0, 360.0),
                cull_mask: u32::MAX,
                pass: Default::default(),
                corners: [Vec2::ZERO; 4],
            };
            PixelateU::new(&p, 1280, 720, Some(v)).offset
        };
        assert_eq!(at(0.0), [0.0, 0.0]);
        assert!((at(1.0)[0] - 3.0).abs() < 1e-4, "{:?}", at(1.0));
        assert!((at(4.0)[0] - 0.0).abs() < 1e-4, "a whole cell brings the grid back: {:?}", at(4.0));
    }

    #[test]
    fn a_1080p_frame_gets_the_whole_bloom_chain() {
        let sizes = bloom_mip_sizes(1920, 1080);
        assert_eq!(sizes.len(), MAX_BLOOM_MIPS);
        assert_eq!(sizes[0], (960, 540));
        assert_eq!(sizes[6], (15, 8));
    }

    #[test]
    fn a_tiny_frame_still_gets_one_level() {
        assert_eq!(bloom_mip_sizes(6, 6), vec![(3, 3)]);
        assert_eq!(bloom_mip_sizes(16, 16).len(), 2, "8 and 4");
    }

    #[test]
    fn bloom_intensity_means_the_same_at_any_depth() {
        let p = std::collections::HashMap::new();
        let deep = BloomU::from_params(&p, 7);
        let shallow = BloomU::from_params(&p, 2);
        // norm * (sum of the scatter weights) is 1 either way.
        let sum = |n: usize, s: f32| (0..n).map(|k| s.powi(k as i32)).sum::<f32>();
        assert!((deep.norm * sum(7, deep.scatter) - 1.0).abs() < 1e-5);
        assert!((shallow.norm * sum(2, shallow.scatter) - 1.0).abs() < 1e-5);
    }
}
