//! **Paintable sprites** (Oct 2026): a sprite a game paints stains into -
//! blood on a floor, scorch marks, oil, footprints, paint. Painted once,
//! kept for good, and free every frame after.
//!
//! A [`Paintable`] on a sprite takes a **copy** of the sprite's texture the
//! first frame its pixels are loaded (as `ecs::carve` does), so the authored
//! texture and every other sprite drawing it are untouched. `Sprite.paint(e,
//! x, y, size, r, g, b, a, shape, rotation)` queues a [`Stamp`] in world
//! units; [`apply_paints`] - run inside `carve::apply_carves`, the one step
//! every host already makes with its assets each frame, so no host was
//! wired - rasterizes it into the copy and hands the changed rectangle to
//! the `AssetManager`, which writes just that rectangle to the GPU. The
//! sprite pass draws the copy in place of the sprite's texture.
//!
//! The shapes are drawn, not loaded, so no stamp texture has to reach a
//! runtime: a soft `disc`, a `splat` (a ragged blob with droplets thrown
//! along `rotation` - blood from a hit, paint from a throw) and a `streak`
//! (a tapered smear along `rotation` - a drag mark, a skid, a burn line).
//! Each splat is the same in every runtime: its shape is a hash of where it
//! landed. `blend` says how a stamp meets the floor: `over` covers it by the
//! colour's alpha, `multiply` darkens it by the colour (a scorch, a soak).
//!
//! **A press dents the surface** (`Sprite.press(e, x, y, size, depth, shape,
//! rotation)`): the same shapes, written into a height field kept beside the
//! sprite and folded into a copy of its **normal map**, so a light catches
//! the rim of a footprint in snow. The normals are worked out again only
//! inside the stamped rectangle, from the authored map plus the height's
//! slope, and the authored alpha (the specular mask) is kept. A press *sets*
//! a depth rather than digging - a print over a print is no deeper - and a
//! negative depth raises a ridge. A sprite with no normal map has nothing to
//! dent and ignores presses. The normal pass draws the copy.
//!
//! **A stamp can wet the surface** (`gloss`, Oct 2026): it raises the
//! normal map copy's alpha - the specular mask - so fresh paint shines under
//! a light where the brick beside it is matt, and with `dry_seconds` the shine
//! fades back to the authored mask as it dries. And the `spray` shape is a
//! disc of speckle, densest at its middle: an aerosol's dots.
//!
//! The copy and the queue are runtime state, never saved: a scene loaded
//! again is clean again.

use glam::Vec2;
use hecs::World;
use serde::{Deserialize, Serialize};

use super::carve::TexelFrame;
use super::components::{MaterialComponent, SpriteComponent, Transform, WorldTransform};
use crate::asset::AssetManager;

/// The shape a stamp leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StampShape {
    /// A soft round spot.
    Disc,
    /// A ragged blob with droplets thrown along the stamp's rotation.
    #[default]
    Splat,
    /// A tapered smear along the stamp's rotation, `size` long.
    Streak,
    /// A disc `size` across of fine speckle, densest at its middle - an
    /// aerosol's mist of dots; a stroke of them fills in.
    Spray,
}

impl StampShape {
    /// The shape a script names; anything else is a splat.
    pub fn named(name: &str) -> Self {
        match name {
            "disc" => Self::Disc,
            "streak" => Self::Streak,
            "spray" => Self::Spray,
            _ => Self::Splat,
        }
    }
}

/// How a stamp meets what is under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PaintBlend {
    /// Cover it by the colour's alpha.
    #[default]
    Over,
    /// Darken it by the colour (white leaves it alone), by the alpha.
    Multiply,
    /// Rub it out: the texel's alpha falls by the stamp's - a glow fading,
    /// a stain washed away. The colour is left alone.
    Erase,
    /// Wet it: raise its normal map's alpha (the specular mask) toward the
    /// stamp's alpha, so it shines; it dries back over the paintable's
    /// `dry_seconds`. Needs a normal map, like a press.
    Gloss,
}

impl PaintBlend {
    /// The blend a script names, if it names one.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "over" => Some(Self::Over),
            "multiply" => Some(Self::Multiply),
            "erase" => Some(Self::Erase),
            "gloss" => Some(Self::Gloss),
            _ => None,
        }
    }
}

/// One queued stamp, in world units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stamp {
    pub x: f32,
    pub y: f32,
    /// Across for a disc or splat; the length of a streak.
    pub size: f32,
    /// Linear colour and alpha.
    pub color: [f32; 4],
    pub shape: StampShape,
    /// Radians: where a splat's droplets fly, a streak's direction.
    pub rotation: f32,
    /// `0` paints colour. Anything else is a **press**: how deep the shape
    /// dents the normal map, as the rise of its rim over about a texel (`1`
    /// is a rim near 45 degrees); negative raises it.
    pub depth: f32,
    /// How this stamp meets the sprite, over the `paintable`'s own `blend`:
    /// one canvas painted `over` and rubbed out with `erase`.
    pub blend: Option<PaintBlend>,
}

/// What [`apply_paints`] keeps for a paintable sprite. Never serialized.
#[derive(Debug, Clone, Default)]
pub struct PaintState {
    /// The texture the sprite draws instead of its own: a copy, painted.
    pub texture: Option<String>,
    /// The sprite texture the copy was made from - a change makes a new one.
    pub source: String,
    pub width: u32,
    pub height: u32,
    /// The normal map the sprite's normals are drawn from instead of its
    /// material's: a copy, pressed. `None` until the first press.
    pub normal: Option<String>,
    /// The material's normal map the copy was made from.
    pub normal_source: String,
    pub normal_size: (u32, u32),
    /// The authored normal map's texels: every press works from these.
    pub normal_base: Vec<u8>,
    /// How far each normal-map texel is pressed down (negative) or up.
    pub dents: Vec<f32>,
    /// How wet each normal-map texel is, 0..1 ([`PaintBlend::Gloss`]).
    pub gloss: Vec<f32>,
    /// The normal-map texels still drying, and when they last dried.
    pub wet: Option<(i64, i64, i64, i64)>,
    pub dried_at: f64,
}

/// The `paintable` component.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Paintable {
    /// How stamps meet the sprite.
    pub blend: PaintBlend,
    /// Paint only where the sprite is at least this opaque (0..1): stains
    /// stay on the floor and off the holes in it.
    pub alpha_cutoff: f32,
    /// How long a `gloss` stamp takes to dry, in seconds - its shine falls
    /// to a third in that time. 0: it stays wet.
    pub dry_seconds: f32,
    /// Stamps queued since the last [`apply_paints`].
    #[serde(skip)]
    pub pending: Vec<Stamp>,
    #[serde(skip)]
    pub state: PaintState,
}

impl Default for Paintable {
    fn default() -> Self {
        Self {
            blend: PaintBlend::Over,
            alpha_cutoff: 0.5,
            dry_seconds: 0.0,
            pending: Vec::new(),
            state: PaintState::default(),
        }
    }
}

/// The most stamps one sprite keeps queued between two frames; a script
/// painting faster than that loses the oldest.
pub const MAX_PENDING: usize = 1024;

impl Paintable {
    /// Queue a stamp (the oldest is dropped past [`MAX_PENDING`]).
    pub fn push(&mut self, stamp: Stamp) {
        if self.pending.len() >= MAX_PENDING {
            self.pending.remove(0);
        }
        self.pending.push(stamp);
    }
}

fn hash(x: u32) -> u32 {
    let mut h = x.wrapping_mul(0x9E37_79B1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85EB_CA77);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE3D);
    h ^ (h >> 16)
}

/// A value in 0..1 from a seed and an index.
fn rnd(seed: u32, i: u32) -> f32 {
    (hash(seed ^ hash(i.wrapping_add(0x51ED_270B))) >> 8) as f32 / (1u32 << 24) as f32
}

/// The blobs a stamp is made of, in its own frame (x along `rotation`),
/// in units of the stamp's size: (centre, radius, stretch along x).
fn blobs(stamp: &Stamp) -> Vec<(Vec2, f32, f32)> {
    match stamp.shape {
        StampShape::Disc | StampShape::Spray => vec![(Vec2::ZERO, 0.5, 1.0)],
        StampShape::Streak => {
            // A run of overlapping blobs, fattest at the start, tapering.
            (0..12)
                .map(|i| {
                    let t = i as f32 / 11.0;
                    (Vec2::new(-0.5 + t, 0.0), 0.12 * (1.0 - 0.7 * t), 1.6)
                })
                .collect()
        }
        StampShape::Splat => {
            let seed = hash((stamp.x * 7.31) as i32 as u32 ^ hash((stamp.y * 3.17) as i32 as u32));
            let mut out = vec![(Vec2::ZERO, 0.36, 1.0)];
            // Lobes round the edge make it ragged.
            for i in 0..7 {
                let a = std::f32::consts::TAU * (i as f32 + rnd(seed, i)) / 7.0;
                let d = 0.18 + 0.12 * rnd(seed, i + 20);
                out.push((Vec2::from_angle(a) * d, 0.14 + 0.1 * rnd(seed, i + 40), 1.0));
            }
            // Droplets thrown forward, along +x, in a cone.
            for i in 0..14 {
                let spread = (rnd(seed, i + 60) - 0.5) * 1.3;
                let d = 0.45 + 0.75 * rnd(seed, i + 80).powf(0.7);
                let r = 0.02 + 0.06 * (1.0 - d / 1.2) * rnd(seed, i + 100);
                out.push((Vec2::from_angle(spread) * d, r.max(0.015), 1.0 + d * 0.8));
            }
            out
        }
    }
}

/// How much of a stamp covers local point `p` (stamp units, x along its
/// rotation), 0..1, softened over `soft` units.
fn coverage(blobs: &[(Vec2, f32, f32)], p: Vec2, soft: f32) -> f32 {
    let mut best = 0.0f32;
    for (c, r, stretch) in blobs {
        let d = p - *c;
        // A droplet stretched along x: a teardrop's length.
        let d = Vec2::new(d.x / stretch, d.y);
        let k = ((r - d.length()) / soft.max(1e-4) + 0.5).clamp(0.0, 1.0);
        best = best.max(k);
    }
    best
}

fn srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// A world point in an entity's local space (rotation and scale undone).
fn to_local(w: &WorldTransform, p: Vec2) -> Vec2 {
    let d = Vec2::from_angle(-w.rotation).rotate(p - w.position);
    Vec2::new(d.x / w.scale.x.abs().max(1e-6) * w.scale.x.signum(), d.y / w.scale.y.abs().max(1e-6) * w.scale.y.signum())
}

/// Make each paintable sprite's copy the first frame its texture's pixels
/// are loaded, then rasterize the queued stamps into it - colour into the
/// copy of its texture, presses into the copy of its normal map. Run by
/// `carve::apply_carves`, which every host calls once a frame.
pub fn apply_paints(world: &mut World, assets: &mut AssetManager) {
    for (e, (p, s, t, w, m)) in world.query_mut::<(
        &mut Paintable,
        &SpriteComponent,
        &Transform,
        Option<&WorldTransform>,
        Option<&MaterialComponent>,
    )>() {
        let source = crate::asset::resolve_texture_id(&s.texture_id);
        if p.state.texture.is_none() || p.state.source != source {
            let Some(src) = assets.get_texture_bytes(source) else { continue };
            let data = src.clone();
            let id = format!("{source}#painted:{}", e.to_bits());
            let kept = std::mem::take(&mut p.state);
            p.state = PaintState {
                texture: Some(id.clone()),
                source: source.to_string(),
                width: data.width,
                height: data.height,
                ..kept
            };
            assets.insert_texture_bytes(id.clone(), data);
            #[cfg(feature = "gpu-wgpu")]
            assets.remove_wgpu_texture(&id);
        }
        dry(p, assets);
        if p.pending.is_empty() {
            continue;
        }
        let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        let own = p.blend;
        let (presses, paints): (Vec<Stamp>, Vec<Stamp>) = p
            .pending
            .drain(..)
            .partition(|s| s.depth != 0.0 || s.blend.unwrap_or(own) == PaintBlend::Gloss);
        if !paints.is_empty() {
            paint_colour(p, s, &wt, &paints, assets);
        }
        if !presses.is_empty() {
            let normal = m
                .and_then(|m| m.normal_map_id.as_deref())
                .filter(|id| !id.is_empty())
                .map(|id| crate::asset::resolve_texture_id(id).to_string());
            press_normals(p, s, &wt, normal, presses, assets, e.to_bits().get());
        }
    }
}

/// Where a stamp lands in a texture of `per` texels per world unit: the
/// texel at its centre and the texel rectangle it can reach, clipped.
fn stamp_rect(
    stamp: &Stamp,
    frame: &TexelFrame,
    wt: &WorldTransform,
    per: Vec2,
    width: i64,
    height: i64,
) -> Option<(Vec2, (i64, i64, i64, i64))> {
    // The stamp's reach in world units (droplets fly past its size).
    let reach = stamp.size * if stamp.shape == StampShape::Splat { 1.3 } else { 0.6 };
    let centre = frame.texel_of(to_local(wt, Vec2::new(stamp.x, stamp.y)));
    let half = Vec2::splat(reach) * per;
    let x0 = ((centre.x - half.x).floor() as i64).max(0);
    let x1 = ((centre.x + half.x).ceil() as i64).min(width - 1);
    let y0 = ((centre.y - half.y).floor() as i64).max(0);
    let y1 = ((centre.y + half.y).ceil() as i64).min(height - 1);
    (x0 <= x1 && y0 <= y1).then_some((centre, (x0, y0, x1, y1)))
}

/// How much of `stamp` covers texel (x, y), 0..1.
fn texel_coverage(stamp: &Stamp, shape: &[(Vec2, f32, f32)], wt: &WorldTransform, centre: Vec2, per: Vec2, x: i64, y: i64) -> f32 {
    // From a texel back to the stamp's frame: world offset, turned by the
    // stamp's rotation (and the sprite's own), over its size.
    let turn = Vec2::from_angle(-(stamp.rotation - wt.rotation));
    let soft = 1.2 / (stamp.size * per.x.max(1e-6)).max(1.0);
    // Texel offset to local units (y up in local space).
    let d = Vec2::new((x as f32 + 0.5 - centre.x) / per.x, -(y as f32 + 0.5 - centre.y) / per.y);
    let q = turn.rotate(d) / stamp.size;
    let c = coverage(shape, q, soft);
    if stamp.shape != StampShape::Spray || c <= 0.0 {
        return c;
    }
    // A speckle per texel, from where the stamp landed: dense in the middle,
    // a scatter at the rim.
    let seed = hash((stamp.x * 13.7) as i32 as u32 ^ hash((stamp.y * 7.9) as i32 as u32));
    let density = 0.18 + 0.82 * (1.0 - q.length() * 2.0).clamp(0.0, 1.0).powf(1.5);
    let roll = rnd(seed, (x as u32).wrapping_mul(7919) ^ (y as u32).wrapping_mul(104_729));
    if roll < density { c } else { 0.0 }
}

fn grow(dirty: &mut Option<(i64, i64, i64, i64)>, r: (i64, i64, i64, i64)) {
    *dirty = Some(match *dirty {
        None => r,
        Some((a, b, c, d)) => (a.min(r.0), b.min(r.1), c.max(r.2), d.max(r.3)),
    });
}

/// Colour stamps into the copy of the sprite's texture.
fn paint_colour(p: &Paintable, s: &SpriteComponent, wt: &WorldTransform, stamps: &[Stamp], assets: &mut AssetManager) {
    let Some(id) = p.state.texture.clone() else { return };
    let frame = TexelFrame::of(s, p.state.width, p.state.height);
    // Texels per world unit.
    let per = frame.texels_per_unit() / Vec2::new(wt.scale.x.abs().max(1e-6), wt.scale.y.abs().max(1e-6));
    let (width, height) = (p.state.width as i64, p.state.height as i64);
    let cutoff = (p.alpha_cutoff.clamp(0.0, 1.0) * 255.0) as u8;
    let Some(bytes) = assets.texture_bytes_mut(&id) else { return };
    let mut dirty = None;
    for stamp in stamps {
        if stamp.size <= 0.0 || stamp.color[3] <= 0.0 {
            continue;
        }
        let shape = blobs(stamp);
        let Some((centre, (x0, y0, x1, y1))) = stamp_rect(stamp, &frame, wt, per, width, height) else { continue };
        let col = [srgb(stamp.color[0]), srgb(stamp.color[1]), srgb(stamp.color[2])];
        let blend = stamp.blend.unwrap_or(p.blend);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let i = (y * width + x) as usize;
                if bytes.rgba[i * 4 + 3] < cutoff {
                    continue;
                }
                let a = texel_coverage(stamp, &shape, wt, centre, per, x, y) * stamp.color[3].clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let da = bytes.rgba[i * 4 + 3] as f32 / 255.0;
                match blend {
                    // Straight-alpha "over": on an opaque floor the colour
                    // moves toward the stamp's by its alpha, as it always did;
                    // on a transparent canvas the texel takes the stamp's
                    // colour and gains its alpha.
                    PaintBlend::Over => {
                        let oa = a + da * (1.0 - a);
                        for ch in 0..3 {
                            let dst = bytes.rgba[i * 4 + ch] as f32 / 255.0;
                            let out = if oa > 0.0 { (col[ch] * a + dst * da * (1.0 - a)) / oa } else { dst };
                            bytes.rgba[i * 4 + ch] = (out * 255.0).round().clamp(0.0, 255.0) as u8;
                        }
                        bytes.rgba[i * 4 + 3] = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
                    }
                    PaintBlend::Multiply => {
                        for ch in 0..3 {
                            let v = &mut bytes.rgba[i * 4 + ch];
                            let out = *v as f32 / 255.0 * (1.0 + (col[ch] - 1.0) * a);
                            *v = (out * 255.0).round().clamp(0.0, 255.0) as u8;
                        }
                    }
                    PaintBlend::Erase => {
                        bytes.rgba[i * 4 + 3] = (da * (1.0 - a) * 255.0).round().clamp(0.0, 255.0) as u8;
                    }
                    // Routed to the normal map (`press_normals`), never here.
                    PaintBlend::Gloss => {}
                }
            }
        }
        grow(&mut dirty, (x0, y0, x1, y1));
    }
    if let Some((x0, y0, x1, y1)) = dirty {
        assets.mark_texture_dirty(&id, x0 as u32, y0 as u32, (x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32);
    }
}

/// Presses into the height field, and the normals they change into the copy
/// of the sprite's normal map. Presses wait while the normal map is loading.
fn press_normals(
    p: &mut Paintable,
    s: &SpriteComponent,
    wt: &WorldTransform,
    normal: Option<String>,
    presses: Vec<Stamp>,
    assets: &mut AssetManager,
    bits: u64,
) {
    // Nothing to dent without a normal map.
    let Some(source) = normal else { return };
    if p.state.normal.is_none() || p.state.normal_source != source {
        let Some(src) = assets.get_texture_bytes(&source) else {
            p.pending.extend(presses);
            return;
        };
        let data = src.clone();
        let id = format!("{source}#pressed:{bits}");
        p.state.normal_base = data.rgba.clone();
        p.state.dents = vec![0.0; (data.width * data.height) as usize];
        p.state.gloss = vec![0.0; (data.width * data.height) as usize];
        p.state.wet = None;
        p.state.normal_size = (data.width, data.height);
        p.state.normal_source = source;
        p.state.normal = Some(id.clone());
        assets.insert_texture_bytes(id.clone(), data);
        #[cfg(feature = "gpu-wgpu")]
        assets.remove_wgpu_texture(&id);
    }
    let Some(id) = p.state.normal.clone() else { return };
    let (nw, nh) = p.state.normal_size;
    let frame = TexelFrame::of(s, nw, nh);
    let per = frame.texels_per_unit() / Vec2::new(wt.scale.x.abs().max(1e-6), wt.scale.y.abs().max(1e-6));
    let (width, height) = (nw as i64, nh as i64);
    let mut dirty = None;
    for stamp in &presses {
        if stamp.size <= 0.0 {
            continue;
        }
        let shape = blobs(stamp);
        let Some((centre, (x0, y0, x1, y1))) = stamp_rect(stamp, &frame, wt, per, width, height) else { continue };
        if stamp.depth == 0.0 {
            // Gloss: wet it.
            let amount = stamp.color[3].clamp(0.0, 1.0);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let c = texel_coverage(stamp, &shape, wt, centre, per, x, y) * amount;
                    let g = &mut p.state.gloss[(y * width + x) as usize];
                    *g = g.max(c);
                }
            }
            grow(&mut dirty, (x0, y0, x1, y1));
            let mut wet = p.state.wet;
            grow(&mut wet, (x0, y0, x1, y1));
            p.state.wet = wet;
            continue;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let c = texel_coverage(stamp, &shape, wt, centre, per, x, y);
                if c <= 0.0 {
                    continue;
                }
                // A press sets a depth; it does not dig.
                let want = -stamp.depth * c;
                let h = &mut p.state.dents[(y * width + x) as usize];
                *h = if stamp.depth > 0.0 { h.min(want) } else { h.max(want) };
            }
        }
        // A slope reaches one texel past what changed.
        grow(&mut dirty, ((x0 - 1).max(0), (y0 - 1).max(0), (x1 + 1).min(width - 1), (y1 + 1).min(height - 1)));
    }
    let Some((x0, y0, x1, y1)) = dirty else { return };
    let dents = &p.state.dents;
    let gloss = &p.state.gloss;
    let base = &p.state.normal_base;
    let Some(bytes) = assets.texture_bytes_mut(&id) else { return };
    let at = |x: i64, y: i64| dents[(y.clamp(0, height - 1) * width + x.clamp(0, width - 1)) as usize];
    for y in y0..=y1 {
        for x in x0..=x1 {
            let i = (y * width + x) as usize * 4;
            // The slope of the dents, in the normal maps' encoding: x right,
            // y up, the image's rows running down (`normals_from_height`).
            let gx = (at(x + 1, y) - at(x - 1, y)) * 0.5;
            let gy = (at(x, y + 1) - at(x, y - 1)) * 0.5;
            let n0 = [0, 1, 2].map(|k| base[i + k] as f32 / 255.0 * 2.0 - 1.0);
            // Whiteout: the authored bumps and the dent's slope, added.
            let n = glam::Vec3::new(n0[0] - gx, n0[1] + gy, n0[2]).normalize_or(glam::Vec3::Z);
            for (k, v) in n.to_array().into_iter().enumerate() {
                bytes.rgba[i + k] = ((v * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            bytes.rgba[i + 3] = wet_alpha(base[i + 3], gloss[i / 4]);
        }
    }
    assets.mark_texture_dirty(&id, x0 as u32, y0 as u32, (x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32);
}

/// A normal-map texel's specular mask: the authored one, or the wet one if
/// that is shinier.
fn wet_alpha(base: u8, gloss: f32) -> u8 {
    base.max((gloss.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Let what a `gloss` stamp wet dry, a few times a second: the shine falls
/// by `e^(-t / dry_seconds)`, on the wall clock - this runs where the assets
/// are, which has no frame time - and only over the rectangle still wet.
fn dry(p: &mut Paintable, assets: &mut AssetManager) {
    let Some((x0, y0, x1, y1)) = p.state.wet else { return };
    if p.dry_seconds <= 0.0 {
        return;
    }
    let now = crate::profiling::unix_time();
    if p.state.dried_at <= 0.0 || now < p.state.dried_at {
        p.state.dried_at = now;
        return;
    }
    let elapsed = now - p.state.dried_at;
    if elapsed < 0.2 {
        return;
    }
    p.state.dried_at = now;
    let keep = (-(elapsed as f32) / p.dry_seconds).exp();
    let Some(id) = p.state.normal.clone() else { return };
    let width = p.state.normal_size.0 as i64;
    let Some(bytes) = assets.texture_bytes_mut(&id) else { return };
    let mut still = false;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let k = (y * width + x) as usize;
            let g = &mut p.state.gloss[k];
            if *g <= 0.0 {
                continue;
            }
            *g *= keep;
            if *g < 1.0 / 255.0 {
                *g = 0.0;
            } else {
                still = true;
            }
            bytes.rgba[k * 4 + 3] = wet_alpha(p.state.normal_base[k * 4 + 3], *g);
        }
    }
    assets.mark_texture_dirty(&id, x0 as u32, y0 as u32, (x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32);
    if !still {
        p.state.wet = None;
    }
}

/// `Sprite.paint`'s queueing, shared by every host: on a `paintable` only.
pub fn paint(world: &World, entity: hecs::Entity, stamp: Stamp) -> bool {
    match world.get::<&mut Paintable>(entity) {
        Ok(mut p) => {
            p.push(stamp);
            true
        }
        Err(_) => false,
    }
}
