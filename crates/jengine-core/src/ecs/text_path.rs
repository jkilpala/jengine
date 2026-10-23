//! Text along a path (Oct 2026): a name lettered round an arch, words on a
//! swinging string of pennants, a marquee scrolling round a carousel - world
//! text whose baseline is a curve, each letter standing on it and turned
//! with it.
//!
//! A [`TextPath`] on an entity with a `text` names an entity carrying a
//! `vector_path` (flattened as `path_follow` flattens it) or a `line`, in
//! world space - so a path that moves or swings carries its letters. Each
//! glyph is laid out as the text always is ([`text_layout::glyph_quads`]),
//! then its distance along the baseline becomes a distance along the path
//! and its height a distance off it ([`bend`]): the glyph stands on the
//! curve, turned to its tangent. `offset` is where the text starts, `side`
//! how far off the path it rides (to the left of the path's direction,
//! which is up for a path drawn left to right), and `scroll` moves `offset`
//! on by itself - a marquee. On a closed path, or with `repeat`, the text
//! wraps round. The renderer draws the bent glyphs as one textured mesh of
//! the font's atlas, so a distance-field font stays sharp at every turn.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Name, Transform};
use crate::ecs::path_follow::MeasuredPath;
use crate::renderer::text_layout::GlyphQuad;

/// Lay this entity's `text` along a path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextPath {
    /// The entity whose `vector_path` (or `line`) the text runs along.
    #[serde(default)]
    pub path: String,
    /// Where along the path the text starts, in world units.
    #[serde(default)]
    pub offset: f32,
    /// How far off the path the text rides: plus is to the left of the
    /// path's direction.
    #[serde(default)]
    pub side: f32,
    /// World units a second the text moves along (a marquee).
    #[serde(default)]
    pub scroll: f32,
    /// Extra room between letters, in world units.
    #[serde(default)]
    pub spacing: f32,
    /// Wrap round past the end of an open path (a closed one always does).
    #[serde(default)]
    pub repeat: bool,
}

impl Default for TextPath {
    fn default() -> Self {
        Self { path: String::new(), offset: 0.0, side: 0.0, scroll: 0.0, spacing: 0.0, repeat: false }
    }
}

/// The path a text runs along, in world space: the named entity's
/// `vector_path`, else its `line`.
pub fn source(world: &World, name: &str) -> Option<MeasuredPath> {
    if let Some(p) = crate::ecs::path_follow::measured_path(world, name) {
        return Some(p);
    }
    let (e, line) = world
        .query::<(&Name, &crate::renderer::line2d::Line2D)>()
        .iter()
        .find(|(_, (n, _))| n.0 == name)
        .map(|(e, (_, l))| (e, l.points.clone()))
        .map(|(e, p)| (e, p.into_iter().map(Vec2::from).collect::<Vec<_>>()))?;
    if line.len() < 2 {
        return None;
    }
    let points = match crate::ecs::hierarchy::world_transform_of(world, e) {
        Some(w) => line.iter().map(|p| w.apply(*p)).collect(),
        None => match world.get::<&Transform>(e) {
            Ok(t) => {
                let (sin, cos) = t.rotation.sin_cos();
                line.iter()
                    .map(|p| {
                        let s = *p * t.scale;
                        t.position + Vec2::new(s.x * cos - s.y * sin, s.x * sin + s.y * cos)
                    })
                    .collect()
            }
            Err(_) => line,
        },
    };
    Some(MeasuredPath::new(points, false))
}

/// Bend `quads` - glyphs laid out with their first line's top at the
/// origin, unturned - onto `path`: each glyph's centre `x` becomes a
/// distance `offset + x` along it and its height above the line's middle a
/// distance off it, and it is turned to the tangent there. Glyphs past the
/// end of an open path are dropped unless `wrap`.
pub fn bend(quads: &[GlyphQuad], line_height: f32, path: &MeasuredPath, offset: f32, side: f32, spacing: f32, wrap: bool) -> Vec<GlyphQuad> {
    let mut len = path.length();
    // A closed path goes round its closing segment too.
    if path.closed && path.points.len() >= 2 {
        len += path.points[path.points.len() - 1].distance(path.points[0]);
    }
    if len <= 1e-3 {
        return Vec::new();
    }
    let wrap = wrap || path.closed;
    let mut out = Vec::with_capacity(quads.len());
    for (k, q) in quads.iter().enumerate() {
        let mut d = offset + q.center.x + spacing * k as f32;
        if wrap {
            d = d.rem_euclid(len);
        } else if d < 0.0 || d > len {
            continue;
        }
        let (p, dir) = closed_sample(path, d);
        let normal = Vec2::new(-dir.y, dir.x);
        let up = q.center.y + line_height * 0.5 + side;
        out.push(GlyphQuad { center: p + normal * up, size: q.size, rotation: dir.y.atan2(dir.x), uv: q.uv });
    }
    out
}

/// A closed path's last point joins its first, which `MeasuredPath` does
/// not walk: sample the closing segment by hand.
fn closed_sample(path: &MeasuredPath, d: f32) -> (Vec2, Vec2) {
    let len = path.length();
    if path.closed && d > len && path.points.len() >= 2 {
        let (a, b) = (*path.points.last().unwrap(), path.points[0]);
        let t = ((d - len) / a.distance(b).max(1e-6)).clamp(0.0, 1.0);
        return (a.lerp(b, t), (b - a).try_normalize().unwrap_or(Vec2::X));
    }
    path.sample(d)
}

/// The mesh of bent glyphs: two triangles a glyph, uv into the atlas, every
/// vertex `color`.
pub fn glyph_mesh(quads: &[GlyphQuad], color: [f32; 4]) -> (Vec<Vec2>, Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let (mut pos, mut uvq, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for q in quads {
        let (sin, cos) = q.rotation.sin_cos();
        let turn = |v: Vec2| Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
        let (hw, hh) = (q.size.x * 0.5, q.size.y * 0.5);
        let base = pos.len() as u32;
        // Top-left, top-right, bottom-right, bottom-left; v runs down.
        for (c, uv) in [
            (Vec2::new(-hw, hh), [q.uv[0], q.uv[1]]),
            (Vec2::new(hw, hh), [q.uv[2], q.uv[1]]),
            (Vec2::new(hw, -hh), [q.uv[2], q.uv[3]]),
            (Vec2::new(-hw, -hh), [q.uv[0], q.uv[3]]),
        ] {
            pos.push(q.center + turn(c));
            uvq.push([uv[0], uv[1], 1.0]);
            col.push(color);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (pos, uvq, col, idx)
}

/// Move every scrolling text on. Called inside `tick_motion`.
pub fn tick_text_paths(world: &mut World, dt: f32) {
    for (_, tp) in world.query_mut::<&mut TextPath>() {
        if tp.scroll != 0.0 {
            tp.offset += tp.scroll * dt;
            // Keep it small: only its place round the path matters, and a
            // float that grows for an hour loses the fraction.
            if tp.offset.abs() > 1.0e6 {
                tp.offset %= 1.0e6;
            }
        }
    }
}

/// Set any text_path field by name (`TextPath.set`).
pub fn set_text_path_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut s = world.get::<&mut TextPath>(entity).map_err(|_| "no text_path on that entity".to_string())?;
    let mut whole = serde_json::to_value(&*s).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a text_path field"));
    }
    map.insert(field.to_string(), value.clone());
    *s = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(x: f32) -> GlyphQuad {
        GlyphQuad { center: Vec2::new(x, -8.0), size: Vec2::new(6.0, 10.0), rotation: 0.0, uv: [0.0, 0.0, 1.0, 1.0] }
    }

    #[test]
    fn on_a_straight_path_the_text_is_where_it_was() {
        let path = MeasuredPath::new(vec![Vec2::new(100.0, 50.0), Vec2::new(300.0, 50.0)], false);
        let bent = bend(&[quad(5.0), quad(15.0)], 16.0, &path, 10.0, 0.0, 0.0, false);
        assert_eq!(bent.len(), 2);
        assert!((bent[0].center - Vec2::new(115.0, 50.0)).length() < 1e-4, "{:?}", bent[0].center);
        assert!((bent[1].center - Vec2::new(125.0, 50.0)).length() < 1e-4);
        assert!(bent[0].rotation.abs() < 1e-6);
    }

    #[test]
    fn round_a_circle_each_letter_stands_on_the_curve() {
        // An anticlockwise circle of radius 100: left of the direction is in.
        let pts: Vec<Vec2> = (0..64).map(|k| {
            let a = k as f32 / 64.0 * std::f32::consts::TAU;
            Vec2::new(a.cos(), a.sin()) * 100.0
        }).collect();
        let path = MeasuredPath::new(pts, true);
        let total = path.length() + path.points[63].distance(path.points[0]);
        let quads: Vec<GlyphQuad> = (0..8).map(|k| quad(k as f32 * total / 8.0)).collect();
        let bent = bend(&quads, 16.0, &path, 0.0, -20.0, 0.0, false);
        assert_eq!(bent.len(), 8, "a closed path keeps every letter");
        for g in &bent {
            let r = g.center.length();
            assert!((r - 120.0).abs() < 2.0, "riding 20 outside: {r}");
            // Turned to the tangent: the glyph's up points out from the centre.
            let up = Vec2::new(-g.rotation.sin(), g.rotation.cos());
            assert!(up.dot(g.center.normalize()) < -0.95, "up is in, toward the centre, for an anticlockwise path");
        }
    }

    #[test]
    fn an_open_path_drops_what_runs_off_and_repeat_wraps_it() {
        let path = MeasuredPath::new(vec![Vec2::ZERO, Vec2::new(50.0, 0.0)], false);
        let quads: Vec<GlyphQuad> = (0..10).map(|k| quad(k as f32 * 10.0)).collect();
        assert_eq!(bend(&quads, 16.0, &path, 0.0, 0.0, 0.0, false).len(), 6);
        assert_eq!(bend(&quads, 16.0, &path, 0.0, 0.0, 0.0, true).len(), 10);
        let (pos, uvq, _, idx) = glyph_mesh(&bend(&quads, 16.0, &path, 0.0, 0.0, 0.0, false), [1.0; 4]);
        assert_eq!((pos.len(), uvq.len(), idx.len()), (24, 24, 36));
    }
}
