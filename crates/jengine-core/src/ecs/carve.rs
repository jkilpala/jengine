//! **Carvable sprites** (Oct 2026): a sprite whose pixels a game can blow
//! holes in, and whose collider follows what is left - a Worms island, a
//! wall a cannon chews through, dirt a mole digs.
//!
//! A [`Carvable`] on a sprite takes a **copy** of the sprite's texture the
//! first frame its pixels are loaded ([`apply_carves`]), so the authored
//! texture - and every other sprite drawing it - is untouched, and keeps a
//! mask of which texels are solid. `Terrain.carve(e, x, y, radius)` queues a
//! hole in world units; [`apply_carves`], which every host calls once a
//! frame, clears it from the copy and the mask (darkening a `scorch` rim
//! round it) and hands the changed rectangle to the `AssetManager`, which
//! writes just that rectangle to the GPU texture the next time it is drawn.
//! The sprite pass draws the copy in place of the sprite's texture.
//!
//! With `collider` on, the physics sync builds a static body from the mask
//! and rebuilds it when the mask changes - so what stood on a pixel that is
//! gone falls. It is solid, not a line: marching squares every
//! `collider_step` texels give each cell the outline crosses a small convex
//! polygon, and the cells wholly inside merge into boxes ([`solid_pieces`]).
//! A rapier polyline along the [`outline`] was tried first and has no
//! inside - a crate put down overlapping a slope was pushed out of the *far*
//! side and fell through the island.
//!
//! The copy, the mask and the queue are runtime state, never saved: a scene
//! loaded again is whole again.

use std::sync::Arc;

use glam::Vec2;
use hecs::World;
use serde::{Deserialize, Serialize};

use super::components::{SpriteComponent, Transform, WorldTransform};
use crate::asset::AssetManager;

/// One queued hole: a disc in world units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Carve {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
}

/// What [`apply_carves`] keeps for a carvable sprite. Never serialized.
#[derive(Debug, Clone, Default)]
pub struct CarveState {
    /// The texture the sprite draws instead of its own: a copy, carved.
    pub texture: Option<String>,
    /// The sprite texture the copy was made from - a change makes a new one.
    pub source: String,
    pub width: u32,
    pub height: u32,
    /// One byte a texel, 1 where solid.
    pub mask: Arc<Vec<u8>>,
    /// Bumped by every change to the mask; the physics sync rebuilds on it.
    pub rev: u64,
}

/// The `carvable` component.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Carvable {
    /// A texel at or below this alpha (0..1) is empty, to the mask and so to
    /// the collider.
    pub alpha_cutoff: f32,
    /// Build a static collider from the solid texels' outline.
    pub collider: bool,
    /// The outline's resolution, in texels: smaller follows the art closer
    /// and costs more segments.
    pub collider_step: u32,
    /// A rim this wide (world units) round each hole is darkened - scorched
    /// earth. 0 for a clean cut.
    pub scorch: f32,
    /// The rim's colour, multiplied into the texels (linear).
    pub scorch_color: [f32; 4],
    /// The collider's friction. Rapier's default of 0.5 lets a crate slide
    /// off any slope steeper than about 27 degrees.
    pub friction: f32,
    /// Holes queued since the last [`apply_carves`].
    #[serde(skip)]
    pub pending: Vec<Carve>,
    #[serde(skip)]
    pub state: CarveState,
}

impl Default for Carvable {
    fn default() -> Self {
        Self {
            alpha_cutoff: 0.5,
            collider: true,
            collider_step: 3,
            scorch: 0.0,
            scorch_color: [0.25, 0.18, 0.12, 1.0],
            friction: 0.8,
            pending: Vec::new(),
            state: CarveState::default(),
        }
    }
}

impl Carvable {
    /// Whether texel `(x, y)` is solid; false outside the texture.
    pub fn solid(&self, x: i64, y: i64) -> bool {
        let s = &self.state;
        x >= 0
            && y >= 0
            && (x as u32) < s.width
            && (y as u32) < s.height
            && s.mask.get((y as u32 * s.width + x as u32) as usize).is_some_and(|m| *m != 0)
    }
}

/// How a carvable sprite's texels sit in its entity's local space: the
/// quad's size and anchor, and the texel rectangle it shows.
#[derive(Debug, Clone, Copy)]
pub struct TexelFrame {
    pub size: Vec2,
    pub anchor: Vec2,
    /// `uv_rect` in texels: x0, y0, x1, y1 (y down).
    pub texels: [f32; 4],
}

impl TexelFrame {
    pub fn of(sprite: &SpriteComponent, width: u32, height: u32) -> Self {
        let uv = sprite.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let (w, h) = (width as f32, height as f32);
        let texels = [uv[0] * w, uv[1] * h, uv[2] * w, uv[3] * h];
        let natural = Vec2::new((texels[2] - texels[0]).abs(), (texels[3] - texels[1]).abs());
        Self { size: sprite.drawn_size(natural), anchor: Vec2::from(sprite.anchor), texels }
    }

    /// The texel under local point `p` (fractional, y down).
    pub fn texel_of(&self, p: Vec2) -> Vec2 {
        let u = p.x / self.size.x + self.anchor.x;
        let v = 1.0 - (p.y / self.size.y + self.anchor.y);
        let [x0, y0, x1, y1] = self.texels;
        Vec2::new(x0 + u * (x1 - x0), y0 + v * (y1 - y0))
    }

    /// The local point at texel `t` - the inverse of [`Self::texel_of`].
    pub fn local_of(&self, t: Vec2) -> Vec2 {
        let [x0, y0, x1, y1] = self.texels;
        let u = (t.x - x0) / (x1 - x0);
        let v = (t.y - y0) / (y1 - y0);
        Vec2::new((u - self.anchor.x) * self.size.x, (1.0 - v - self.anchor.y) * self.size.y)
    }

    /// Texels per local unit, across and down.
    pub fn texels_per_unit(&self) -> Vec2 {
        let [x0, y0, x1, y1] = self.texels;
        Vec2::new((x1 - x0).abs() / self.size.x.abs().max(1e-6), (y1 - y0).abs() / self.size.y.abs().max(1e-6))
    }
}

/// A world point in an entity's local space (rotation and scale undone;
/// shear is not).
fn to_local(w: &WorldTransform, p: Vec2) -> Vec2 {
    let d = Vec2::from_angle(-w.rotation).rotate(p - w.position);
    Vec2::new(d.x / w.scale.x.abs().max(1e-6) * w.scale.x.signum(), d.y / w.scale.y.abs().max(1e-6) * w.scale.y.signum())
}

/// Make each carvable sprite's copy the first frame its texture's pixels are
/// loaded, then cut the queued holes from the copy and the mask. Every host
/// calls this once a frame, before it draws.
pub fn apply_carves(world: &mut World, assets: &mut AssetManager) {
    let _span = crate::profiling::trace::span("carves");
    // The one per-frame step every host makes with its assets, so the
    // occluders shaped like their sprites are traced here too
    // (`ecs::alpha_shape`): it needs the texture's pixels, and riding along
    // means no host had to learn a new call.
    super::alpha_shape::trace_occluder_shapes(world, assets);
    super::alpha_shape::trace_sprite_colliders(world, assets);
    // And a `sprite` emit shape's pixels (`particles::collect_emit_pixels`),
    // and the frame's colour grade (`renderer::lut`).
    crate::particles::collect_emit_pixels(world, assets);
    crate::renderer::lut::sync(world, assets);
    // A dissolving scene transition's texture (`renderer::transition`).
    crate::renderer::transition::sync_mask(assets);
    // Glyphs a font file has not baked yet (`renderer::dynamic_font`).
    crate::renderer::dynamic_font::sync(world, assets);
    // Stamps painted into a `paintable` sprite's copy (`ecs::paint`).
    super::paint::apply_paints(world, assets);
    // Falling-sand cells into their texture, and their solid cells into the
    // entity's carvable for the collider (`ecs::pixel_sim`).
    super::pixel_sim::sync(world, assets);
    // Smoke into its texture, drawn through the carvable too (`ecs::smoke`).
    super::smoke::sync(world, assets);
    // Normal maps worked out from a flat sprite's silhouette
    // (`ecs::auto_normal`), named in its material.
    super::auto_normal::sync(world, assets);
    // Snow lying on sprites' upward edges (`ecs::snow`), its picture remade
    // as it deepens or melts.
    super::snow::sync(world, assets);
    // Light flooded through tilemaps (`ecs::tile_light`), one texel a cell.
    super::tile_light::sync(world, assets);
    // Fog of war's picture (`ecs::fog_of_war`), one texel a cell.
    super::fog_of_war::sync(world, assets);
    // A screenshot the GPU has finished copying (`renderer::screenshot`).
    #[cfg(feature = "gpu-wgpu")]
    crate::renderer::screenshot::gpu::deliver(assets);
    for (e, (c, s, t, w)) in
        world.query_mut::<(&mut Carvable, &SpriteComponent, &Transform, Option<&WorldTransform>)>()
    {
        let source = crate::asset::resolve_texture_id(&s.texture_id);
        if c.state.texture.is_none() || c.state.source != source {
            let Some(src) = assets.get_texture_bytes(source) else { continue };
            let data = src.clone();
            let cutoff = (c.alpha_cutoff.clamp(0.0, 1.0) * 255.0) as u8;
            let mask: Vec<u8> = data.rgba.chunks_exact(4).map(|p| u8::from(p[3] > cutoff)).collect();
            let id = format!("{source}#carved:{}", e.to_bits());
            c.state = CarveState {
                texture: Some(id.clone()),
                source: source.to_string(),
                width: data.width,
                height: data.height,
                mask: Arc::new(mask),
                rev: c.state.rev + 1,
            };
            assets.insert_texture_bytes(id.clone(), data);
            #[cfg(feature = "gpu-wgpu")]
            assets.remove_wgpu_texture(&id);
        }
        if c.pending.is_empty() {
            continue;
        }
        let Some(id) = c.state.texture.clone() else { continue };
        let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        let frame = TexelFrame::of(s, c.state.width, c.state.height);
        let per = frame.texels_per_unit() / Vec2::new(wt.scale.x.abs().max(1e-6), wt.scale.y.abs().max(1e-6));
        let (width, height) = (c.state.width as i64, c.state.height as i64);
        let scorch = c.scorch.max(0.0);
        let tint = c.scorch_color;
        let Some(bytes) = assets.texture_bytes_mut(&id) else { continue };
        let mask = Arc::make_mut(&mut c.state.mask);
        let mut dirty: Option<(i64, i64, i64, i64)> = None;
        for carve in c.pending.drain(..) {
            let centre = frame.texel_of(to_local(&wt, Vec2::new(carve.x, carve.y)));
            let r = Vec2::splat(carve.radius.max(0.0)) * per;
            let outer = Vec2::splat(carve.radius.max(0.0) + scorch) * per;
            if outer.x <= 0.0 || outer.y <= 0.0 {
                continue;
            }
            let x0 = ((centre.x - outer.x).floor() as i64).max(0);
            let x1 = ((centre.x + outer.x).ceil() as i64).min(width - 1);
            let y0 = ((centre.y - outer.y).floor() as i64).max(0);
            let y1 = ((centre.y + outer.y).ceil() as i64).min(height - 1);
            if x0 > x1 || y0 > y1 {
                continue;
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let d = (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - centre) / Vec2::new(r.x.max(1e-6), r.y.max(1e-6));
                    let i = (y * width + x) as usize;
                    if d.length_squared() <= 1.0 {
                        mask[i] = 0;
                        bytes.rgba[i * 4 + 3] = 0;
                    } else if scorch > 0.0 && mask[i] != 0 {
                        let o = (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - centre) / outer;
                        let k = 1.0 - o.length();
                        if k > 0.0 {
                            // Darkest at the hole's edge, fading out over the rim.
                            let edge = r.x / outer.x;
                            let f = (k / (1.0 - edge).max(1e-3)).clamp(0.0, 1.0);
                            for ch in 0..3 {
                                let p = &mut bytes.rgba[i * 4 + ch];
                                let m = 1.0 + (tint[ch] - 1.0) * f * tint[3];
                                *p = (*p as f32 * m).round().clamp(0.0, 255.0) as u8;
                            }
                        }
                    }
                }
            }
            dirty = Some(match dirty {
                None => (x0, y0, x1, y1),
                Some((a, b, cc, d)) => (a.min(x0), b.min(y0), cc.max(x1), d.max(y1)),
            });
        }
        if let Some((x0, y0, x1, y1)) = dirty {
            c.state.rev += 1;
            assets.mark_texture_dirty(&id, x0 as u32, y0 as u32, (x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32);
        }
    }
}

/// The outline of a carvable's solid texels, as segments in the entity's
/// local space: marching squares on a grid every `step` texels, a segment
/// across each cell between its solid and empty corners. Outside the
/// texture is empty, so the outline closes along the image's edge.
pub fn outline(c: &Carvable, frame: &TexelFrame) -> Vec<(Vec2, Vec2)> {
    let step = c.collider_step.max(1) as i64;
    let (w, h) = (c.state.width as i64, c.state.height as i64);
    if w == 0 || h == 0 {
        return Vec::new();
    }
    // Sample points on the grid; a sample is solid if its texel is.
    let gw = (w + step - 1) / step + 2;
    let gh = (h + step - 1) / step + 2;
    let at = |gx: i64, gy: i64| -> bool { c.solid((gx - 1) * step + step / 2, (gy - 1) * step + step / 2) };
    let point = |gx: f32, gy: f32| -> Vec2 {
        let t = Vec2::new((gx - 1.0) * step as f32 + step as f32 / 2.0, (gy - 1.0) * step as f32 + step as f32 / 2.0);
        frame.local_of(Vec2::new(t.x.clamp(0.0, w as f32), t.y.clamp(0.0, h as f32)))
    };
    let mut segments = Vec::new();
    for gy in 0..gh - 1 {
        for gx in 0..gw - 1 {
            let tl = at(gx, gy);
            let tr = at(gx + 1, gy);
            let bl = at(gx, gy + 1);
            let br = at(gx + 1, gy + 1);
            let case = (tl as u8) | (tr as u8) << 1 | (br as u8) << 2 | (bl as u8) << 3;
            if case == 0 || case == 15 {
                continue;
            }
            let (x, y) = (gx as f32, gy as f32);
            let top = point(x + 0.5, y);
            let right = point(x + 1.0, y + 0.5);
            let bottom = point(x + 0.5, y + 1.0);
            let left = point(x, y + 0.5);
            let mut seg = |a: Vec2, b: Vec2| segments.push((a, b));
            match case {
                1 | 14 => seg(left, top),
                2 | 13 => seg(top, right),
                3 | 12 => seg(left, right),
                4 | 11 => seg(right, bottom),
                6 | 9 => seg(top, bottom),
                7 | 8 => seg(left, bottom),
                5 => {
                    seg(left, top);
                    seg(right, bottom);
                }
                10 => {
                    seg(top, right);
                    seg(left, bottom);
                }
                _ => {}
            }
        }
    }
    segments
}

/// A carvable's solid region as collider pieces in its local space, on the
/// same grid as [`outline`]: every cell the outline crosses is the convex
/// polygon of its solid corners and the crossings (two corners' worth for a
/// saddle), and the cells wholly inside are merged into as few rectangles as
/// `merge` makes of them, each `(centre, half extents)`. So the surface is
/// the outline - smooth to slide and roll on - and it has an inside: a body
/// pushed into the land (put down overlapping a slope, or shoved by a blast)
/// is pushed back out of the top. A bare polyline pushed such a crate out of
/// the far side, and it fell through the island.
pub fn solid_pieces(
    c: &Carvable,
    frame: &TexelFrame,
    merge: impl Fn(u32, u32, &dyn Fn(u32, u32) -> bool) -> Vec<(u32, u32, u32, u32)>,
) -> (Vec<(Vec2, Vec2)>, Vec<Vec<Vec2>>) {
    let step = c.collider_step.max(1) as i64;
    let (w, h) = (c.state.width as i64, c.state.height as i64);
    if w == 0 || h == 0 {
        return (Vec::new(), Vec::new());
    }
    let gw = (w + step - 1) / step + 2;
    let gh = (h + step - 1) / step + 2;
    let at = |gx: i64, gy: i64| -> bool { c.solid((gx - 1) * step + step / 2, (gy - 1) * step + step / 2) };
    let point = |gx: f32, gy: f32| -> Vec2 {
        let t = Vec2::new((gx - 1.0) * step as f32 + step as f32 / 2.0, (gy - 1.0) * step as f32 + step as f32 / 2.0);
        frame.local_of(Vec2::new(t.x.clamp(0.0, w as f32), t.y.clamp(0.0, h as f32)))
    };
    let mut polygons = Vec::new();
    for gy in 0..gh - 1 {
        for gx in 0..gw - 1 {
            // Corners clockwise from the top-left, each with the midpoint of
            // the edge that follows it.
            let corners = [(gx, gy), (gx + 1, gy), (gx + 1, gy + 1), (gx, gy + 1)];
            let solid = corners.map(|(x, y)| at(x, y));
            if solid.iter().all(|s| *s) || !solid.iter().any(|s| *s) {
                continue;
            }
            let mut poly = Vec::with_capacity(6);
            for i in 0..4 {
                let (x, y) = corners[i];
                let (nx, ny) = corners[(i + 1) % 4];
                if solid[i] {
                    poly.push(point(x as f32, y as f32));
                }
                if solid[i] != solid[(i + 1) % 4] {
                    poly.push(point((x + nx) as f32 * 0.5, (y + ny) as f32 * 0.5));
                }
            }
            if poly.len() >= 3 {
                polygons.push(poly);
            }
        }
    }
    let full = |cx: u32, cy: u32| -> bool {
        let (x, y) = (cx as i64, cy as i64);
        at(x, y) && at(x + 1, y) && at(x, y + 1) && at(x + 1, y + 1)
    };
    let boxes = merge((gw - 1) as u32, (gh - 1) as u32, &full)
        .into_iter()
        .map(|(cx, cy, cw, ch)| {
            let a = point(cx as f32, cy as f32);
            let b = point((cx + cw) as f32, (cy + ch) as f32);
            ((a + b) * 0.5, (b - a).abs() * 0.5)
        })
        .filter(|(_, half)| half.x > 0.0 && half.y > 0.0)
        .collect();
    (boxes, polygons)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::RawTextureData;

    fn world_with(size: f32, tex: u32) -> (World, AssetManager, hecs::Entity) {
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes(
            "land".into(),
            RawTextureData { width: tex, height: tex, rgba: vec![200; (tex * tex * 4) as usize] },
        );
        let mut world = World::new();
        let sprite = SpriteComponent { texture_id: "land".into(), size: Some([size, size]), ..Default::default() };
        let e = world.spawn((Transform::default(), sprite, Carvable::default()));
        (world, assets, e)
    }

    #[test]
    fn the_first_frame_copies_the_texture_and_masks_it() {
        let (mut world, mut assets, e) = world_with(64.0, 32);
        apply_carves(&mut world, &mut assets);
        let c = world.get::<&Carvable>(e).unwrap();
        let id = c.state.texture.clone().expect("a copy");
        assert_ne!(id, "land", "the authored texture is left alone");
        assert!(assets.has_texture_bytes(&id));
        assert_eq!((c.state.width, c.state.height), (32, 32));
        assert!(c.state.mask.iter().all(|m| *m == 1), "every opaque texel is solid");
    }

    #[test]
    fn a_carve_clears_a_disc_in_the_copy_and_the_mask_only() {
        let (mut world, mut assets, e) = world_with(64.0, 32);
        apply_carves(&mut world, &mut assets);
        // A hole of radius 16 world units at the sprite's centre: 8 texels.
        world.get::<&mut Carvable>(e).unwrap().pending.push(Carve { x: 0.0, y: 0.0, radius: 16.0 });
        apply_carves(&mut world, &mut assets);
        let c = world.get::<&Carvable>(e).unwrap();
        let id = c.state.texture.clone().unwrap();
        let bytes = assets.get_texture_bytes(&id).unwrap();
        let alpha = |x: u32, y: u32| bytes.rgba[((y * 32 + x) * 4 + 3) as usize];
        assert_eq!(alpha(16, 16), 0, "the centre is gone");
        assert_eq!(alpha(16 + 6, 16), 0, "and inside the radius");
        assert_eq!(alpha(16 + 10, 16), 200, "but not past it");
        assert!(!c.solid(16, 16) && c.solid(30, 30), "the mask agrees");
        assert_eq!(assets.get_texture_bytes("land").unwrap().rgba[((16 * 32 + 16) * 4 + 3) as usize], 200, "the original is whole");
        assert_eq!(c.state.rev, 2, "a carve is a new revision");
    }

    #[test]
    fn a_carve_lands_where_the_sprite_is_in_the_world() {
        let (mut world, mut assets, e) = world_with(64.0, 32);
        // Moved right 100, turned a quarter, doubled: world (100, 64) is the
        // sprite's local (64, 0) / 2 ... turned back by -90 degrees.
        *world.get::<&mut Transform>(e).unwrap() = Transform {
            position: Vec2::new(100.0, 0.0),
            rotation: std::f32::consts::FRAC_PI_2,
            scale: Vec2::splat(2.0),
        };
        apply_carves(&mut world, &mut assets);
        // Local (0, -24) is texel (16, 16 + 12): after a quarter turn
        // anticlockwise and the doubling it sits at world (100 + 48, 0).
        world.get::<&mut Carvable>(e).unwrap().pending.push(Carve { x: 148.0, y: 0.0, radius: 4.0 });
        apply_carves(&mut world, &mut assets);
        let c = world.get::<&Carvable>(e).unwrap();
        assert!(!c.solid(16, 28), "the hole is under the world point");
        assert!(c.solid(16, 4), "and not mirrored");
    }

    #[test]
    fn the_outline_rings_a_hole() {
        let (mut world, mut assets, e) = world_with(64.0, 32);
        apply_carves(&mut world, &mut assets);
        let frame = TexelFrame::of(&world.get::<&SpriteComponent>(e).unwrap(), 32, 32);
        let whole = outline(&world.get::<&Carvable>(e).unwrap(), &frame);
        world.get::<&mut Carvable>(e).unwrap().pending.push(Carve { x: 0.0, y: 0.0, radius: 12.0 });
        apply_carves(&mut world, &mut assets);
        let holed = outline(&world.get::<&Carvable>(e).unwrap(), &frame);
        assert!(holed.len() > whole.len(), "a hole adds its own ring of segments");
        // Every segment of the hole's ring is about its radius from the middle.
        let ring: Vec<_> = holed.iter().filter(|(a, _)| a.length() < 20.0).collect();
        assert!(!ring.is_empty());
        for (a, b) in ring {
            let r = ((*a + *b) * 0.5).length();
            assert!((r - 12.0).abs() < 4.5, "a ring segment {r} from the centre");
        }
        // And the outer edge is at the quad's edge, +-32.
        assert!(whole.iter().all(|(a, _)| a.x.abs() > 28.0 || a.y.abs() > 28.0));
    }
}
