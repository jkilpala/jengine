//! Lines (Oct 2026; Godot's `Line2D`, Unity's `LineRenderer`): a polyline
//! with a width that can vary along it, a colour gradient, a texture
//! stretched or tiled along it, and proper joints and caps.
//!
//! `vector_path` strokes a path at one width in one colour; a trail is a
//! ribbon through where something has been. A [`Line2D`] is the drawn line
//! itself - a laser, a rope bridge's rope seen flat, a road on a map, a
//! lightning bolt, a graph - shaped by its points and styled along its
//! length (`t`, 0 at the first point and 1 at the last, by distance).
//!
//! [`line_mesh`] builds it as triangles in the entity's local space; the
//! sprite pass draws them in world space, sorted by `z_order` like any other
//! drawable, through the same path a trail uses (`InterleavedItem::Skinned`).
//! **Tiling is cut into the geometry**: every sampler in the engine clamps,
//! so a tiled line is split wherever the texture repeats and each piece
//! runs u from 0 to 1, as a spline shape's edge is.

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// How a texture lies along a line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineTextureMode {
    /// Once over the whole line.
    #[default]
    Stretch,
    /// Repeated every `tile_length` world units.
    Tile,
}

/// How two segments meet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineJoint {
    /// A pointed corner (mitre), cut back to a bevel when sharper than
    /// `sharp_limit` allows.
    #[default]
    Sharp,
    /// A corner cut straight across.
    Bevel,
    /// A rounded corner.
    Round,
}

/// How an end of the line is finished.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineCap {
    /// Square, at the end point.
    #[default]
    None,
    /// Square, half the width past the end point.
    Box,
    /// A half disc.
    Round,
}

fn default_width() -> f32 {
    10.0
}
fn default_white() -> [f32; 4] {
    [1.0; 4]
}
fn default_sharp_limit() -> f32 {
    2.0
}
fn default_round_steps() -> u32 {
    8
}

/// `line`: a styled polyline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Line2D {
    /// The points, in the entity's local space.
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    /// Join the last point back to the first.
    #[serde(default)]
    pub closed: bool,
    #[serde(default = "default_width")]
    pub width: f32,
    /// `[t, multiplier]` pairs: the width along the line is `width` times
    /// this curve, linear between its points (empty: 1 all along).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub width_curve: Vec<[f32; 2]>,
    /// A linear colour when there is no gradient, and multiplied into it.
    #[serde(default = "default_white")]
    pub color: [f32; 4],
    /// `[t, r, g, b, a]` stops along the line (empty: `color` all along).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gradient: Vec<[f32; 5]>,
    /// The texture along the line; empty is plain colour.
    #[serde(default)]
    pub texture_id: String,
    #[serde(default)]
    pub texture_mode: LineTextureMode,
    /// World units a tile covers in `tile` mode (0: the line's width).
    #[serde(default)]
    pub tile_length: f32,
    #[serde(default)]
    pub joint: LineJoint,
    /// How far a sharp corner may reach, in half-widths, before it is cut
    /// to a bevel.
    #[serde(default = "default_sharp_limit")]
    pub sharp_limit: f32,
    #[serde(default)]
    pub begin_cap: LineCap,
    #[serde(default)]
    pub end_cap: LineCap,
    /// Segments in a round joint or cap's half turn.
    #[serde(default = "default_round_steps")]
    pub round_steps: u32,
    #[serde(default)]
    pub z_order: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    #[serde(default)]
    pub blend_mode: crate::renderer::material::BlendMode,
}

impl Default for Line2D {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            closed: false,
            width: default_width(),
            width_curve: Vec::new(),
            color: default_white(),
            gradient: Vec::new(),
            texture_id: Default::default(),
            texture_mode: LineTextureMode::Stretch,
            tile_length: 0.0,
            joint: LineJoint::Sharp,
            sharp_limit: default_sharp_limit(),
            begin_cap: LineCap::None,
            end_cap: LineCap::None,
            round_steps: default_round_steps(),
            z_order: 0,
            sort_layer: None,
            blend_mode: Default::default(),
        }
    }
}

/// A line as triangles: positions (local), texture coordinates (`u, v, q`
/// with `q = 1`), colours and indices.
#[derive(Debug, Clone, Default)]
pub struct LineMesh {
    pub positions: Vec<Vec2>,
    pub uvq: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl LineMesh {
    fn vertex(&mut self, p: Vec2, u: f32, v: f32, c: [f32; 4]) -> u32 {
        self.positions.push(p);
        self.uvq.push([u, v, 1.0]);
        self.colors.push(c);
        (self.positions.len() - 1) as u32
    }

    /// A triangle, always wound anticlockwise, so the mesh is consistent
    /// whichever way a joint's fan turns - nothing culls today, but a
    /// pipeline that did would otherwise lose every right-hand corner.
    fn tri(&mut self, a: u32, b: u32, c: u32) {
        let (pa, pb, pc) = (self.positions[a as usize], self.positions[b as usize], self.positions[c as usize]);
        if (pb - pa).perp_dot(pc - pa) < 0.0 {
            self.indices.extend_from_slice(&[a, c, b]);
        } else {
            self.indices.extend_from_slice(&[a, b, c]);
        }
    }

    /// The centre and radius of a circle round every vertex.
    pub fn bounds(&self) -> (Vec2, f32) {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in &self.positions {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        ((lo + hi) * 0.5, (hi - lo).length() * 0.5)
    }
}

/// `curve` (pairs of `t` and a value) at `t`, linear between its points
/// and flat past its ends; `fallback` when it is empty.
fn sample_curve(curve: &[[f32; 2]], t: f32, fallback: f32) -> f32 {
    match curve {
        [] => fallback,
        [only] => only[1],
        _ => {
            if t <= curve[0][0] {
                return curve[0][1];
            }
            for w in curve.windows(2) {
                if t <= w[1][0] {
                    let span = (w[1][0] - w[0][0]).max(1e-6);
                    return w[0][1] + (w[1][1] - w[0][1]) * ((t - w[0][0]) / span);
                }
            }
            curve[curve.len() - 1][1]
        }
    }
}

/// The gradient at `t`, times `color`.
fn sample_colour(line: &Line2D, t: f32) -> [f32; 4] {
    let g = &line.gradient;
    let base = match g.as_slice() {
        [] => [1.0; 4],
        [only] => [only[1], only[2], only[3], only[4]],
        _ => {
            let mut out = [g[g.len() - 1][1], g[g.len() - 1][2], g[g.len() - 1][3], g[g.len() - 1][4]];
            if t <= g[0][0] {
                out = [g[0][1], g[0][2], g[0][3], g[0][4]];
            } else {
                for w in g.windows(2) {
                    if t <= w[1][0] {
                        let k = (t - w[0][0]) / (w[1][0] - w[0][0]).max(1e-6);
                        for (c, o) in out.iter_mut().enumerate() {
                            *o = w[0][c + 1] + (w[1][c + 1] - w[0][c + 1]) * k;
                        }
                        break;
                    }
                }
            }
            out
        }
    };
    [base[0] * line.color[0], base[1] * line.color[1], base[2] * line.color[2], base[3] * line.color[3]]
}

/// One point of the line as built: where, how far along, its half-width,
/// colour, and the u its incoming and outgoing segments use.
#[derive(Debug, Clone, Copy)]
struct Station {
    p: Vec2,
    half: f32,
    color: [f32; 4],
    u_in: f32,
    u_out: f32,
    /// Whether two segments meet here at an angle (a corner, not a cut
    /// made for tiling).
    corner: bool,
}

fn perp(d: Vec2) -> Vec2 {
    Vec2::new(-d.y, d.x)
}

/// The line as triangles in local space, or `None` with fewer than two
/// distinct points or no width.
pub fn line_mesh(line: &Line2D) -> Option<LineMesh> {
    let mut pts: Vec<Vec2> = line.points.iter().map(|p| Vec2::new(p[0], p[1])).collect();
    pts.dedup_by(|a, b| a.distance_squared(*b) < 1e-8);
    let closed = line.closed && pts.len() > 2;
    if closed && pts[0].distance_squared(*pts.last().unwrap()) < 1e-8 {
        pts.pop();
    }
    if pts.len() < 2 || line.width <= 0.0 {
        return None;
    }
    if closed {
        pts.push(pts[0]);
    }
    // Distance along the line to each point.
    let mut along = vec![0.0f32];
    for w in pts.windows(2) {
        along.push(along.last().unwrap() + w[0].distance(w[1]));
    }
    let total = *along.last().unwrap();
    if total <= 1e-6 {
        return None;
    }
    let tile = if line.tile_length > 0.0 { line.tile_length } else { line.width };
    let tiled = line.texture_mode == LineTextureMode::Tile && tile > 1e-3;
    // Stations: each point, and for a tiled line a cut wherever the texture
    // starts again.
    let station = |p: Vec2, d: f32, corner: bool, u_in: f32, u_out: f32| Station {
        p,
        half: 0.5 * line.width * sample_curve(&line.width_curve, d / total, 1.0).max(0.0),
        color: sample_colour(line, d / total),
        u_in,
        u_out,
        corner,
    };
    let u_at = |d: f32| if tiled { (d / tile).fract() } else { d / total };
    let mut stations: Vec<Station> = Vec::new();
    for i in 0..pts.len() {
        let d = along[i];
        let corner = (i > 0 && i + 1 < pts.len()) || (closed && (i == 0 || i + 1 == pts.len()));
        let mut u = u_at(d);
        // A point exactly on a tile boundary ends the tile coming in.
        let u_in = if tiled && u == 0.0 && d > 0.0 { 1.0 } else { u };
        if i + 1 == pts.len() && !tiled {
            u = 1.0;
        }
        stations.push(station(pts[i], d, corner, if i + 1 == pts.len() && !tiled { 1.0 } else { u_in }, u));
        if tiled && i + 1 < pts.len() {
            // Cuts inside the segment to the next point.
            let (a, b) = (along[i], along[i + 1]);
            let mut k = (a / tile).floor() + 1.0;
            while k * tile < b - 1e-4 {
                let d = k * tile;
                let p = pts[i].lerp(pts[i + 1], (d - a) / (b - a));
                stations.push(station(p, d, false, 1.0, 0.0));
                k += 1.0;
            }
        }
    }
    let n = stations.len();
    // The direction of the segment leaving each station (the last has none).
    let dirs: Vec<Vec2> = (0..n - 1).map(|i| (stations[i + 1].p - stations[i].p).normalize_or_zero()).collect();
    let mut mesh = LineMesh::default();
    // Per station, the left and right edge points the segment leaving it
    // starts from and the one arriving ends at.
    let mut start_lr: Vec<(Vec2, Vec2)> = vec![(Vec2::ZERO, Vec2::ZERO); n];
    let mut end_lr: Vec<(Vec2, Vec2)> = vec![(Vec2::ZERO, Vec2::ZERO); n];
    for i in 0..n {
        let s = stations[i];
        let prev = if i > 0 { Some(dirs[i - 1]) } else if closed { Some(dirs[n - 2]) } else { None };
        let next = if i + 1 < n { Some(dirs[i]) } else if closed { Some(dirs[0]) } else { None };
        match (prev, next) {
            (Some(a), Some(b)) if s.corner && a.perp_dot(b).abs() > 1e-4 => {
                // A corner: the inner side meets at the mitre point; the
                // outer side is a mitre or is filled by the joint below.
                let (na, nb) = (perp(a), perp(b));
                let m = (na + nb).normalize_or_zero();
                let cos = m.dot(na).max(1e-3);
                let reach = s.half / cos;
                let left_turn = a.perp_dot(b) > 0.0;
                // On a hairpin the inner edges meet far down the bisector,
                // past the segments themselves: then each segment keeps its
                // own inner edge and the two overlap, rather than reaching
                // for a spike.
                let prev_len = if i > 0 { s.p.distance(stations[i - 1].p) } else { s.p.distance(stations[n - 2].p) };
                let next_len = if i + 1 < n { s.p.distance(stations[i + 1].p) } else { s.p.distance(stations[1].p) };
                let fold = reach > prev_len.min(next_len);
                let side = if left_turn { 1.0 } else { -1.0 };
                let (inner_a, inner_b) = if fold {
                    (s.p + na * s.half * side, s.p + nb * s.half * side)
                } else {
                    let p = s.p + m * reach * side;
                    (p, p)
                };
                let sharp_ok = line.joint == LineJoint::Sharp && reach <= s.half * line.sharp_limit.max(1.0);
                let outer_m = if left_turn { s.p - m * reach } else { s.p + m * reach };
                let (outer_a, outer_b) = if sharp_ok {
                    (outer_m, outer_m)
                } else if left_turn {
                    (s.p - na * s.half, s.p - nb * s.half)
                } else {
                    (s.p + na * s.half, s.p + nb * s.half)
                };
                if left_turn {
                    end_lr[i] = (inner_a, outer_a);
                    start_lr[i] = (inner_b, outer_b);
                } else {
                    end_lr[i] = (outer_a, inner_a);
                    start_lr[i] = (outer_b, inner_b);
                }
                // The joint, on the outer side, round the point - once: a
                // closed line's last station is its first again.
                if !sharp_ok && !(closed && i == n - 1) {
                    // Fanned from the inner corner: the segments' ends are
                    // slanted to meet there, so the gap to fill is the whole
                    // wedge from it to the two outer corners - fanned from
                    // the point itself, the half nearest the inner corner
                    // stayed open.
                    let v_outer = if left_turn { 1.0 } else { 0.0 };
                    let pivot = if fold {
                        mesh.vertex(s.p, s.u_out, 0.5, s.color)
                    } else {
                        mesh.vertex(inner_a, s.u_out, 1.0 - v_outer, s.color)
                    };
                    if line.joint == LineJoint::Round {
                        let (from, to) = ((outer_a - s.p).normalize_or_zero(), (outer_b - s.p).normalize_or_zero());
                        let sweep = from.angle_to(to);
                        let steps = ((sweep.abs() / std::f32::consts::PI) * line.round_steps as f32).ceil().max(1.0) as u32;
                        let mut last = mesh.vertex(outer_a, s.u_out, v_outer, s.color);
                        for k in 1..=steps {
                            let dir = Vec2::from_angle(sweep * k as f32 / steps as f32).rotate(from);
                            let v = mesh.vertex(s.p + dir * s.half, s.u_out, v_outer, s.color);
                            mesh.tri(pivot, last, v);
                            last = v;
                        }
                    } else {
                        let a = mesh.vertex(outer_a, s.u_out, v_outer, s.color);
                        let b = mesh.vertex(outer_b, s.u_out, v_outer, s.color);
                        mesh.tri(pivot, a, b);
                    }
                }
            }
            (a, b) => {
                let d = b.or(a).unwrap_or(Vec2::X);
                let nrm = perp(d) * s.half;
                end_lr[i] = (s.p + nrm, s.p - nrm);
                start_lr[i] = (s.p + nrm, s.p - nrm);
            }
        }
    }
    // The segments: a quad from each station to the next.
    for i in 0..n - 1 {
        let (a, b) = (stations[i], stations[i + 1]);
        let (al, ar) = start_lr[i];
        let (bl, br) = end_lr[i + 1];
        let v0 = mesh.vertex(al, a.u_out, 0.0, a.color);
        let v1 = mesh.vertex(ar, a.u_out, 1.0, a.color);
        let v2 = mesh.vertex(bl, b.u_in, 0.0, b.color);
        let v3 = mesh.vertex(br, b.u_in, 1.0, b.color);
        mesh.tri(v0, v1, v2);
        mesh.tri(v2, v1, v3);
    }
    if !closed {
        cap(&mut mesh, line.begin_cap, &stations[0], -dirs[0], 0.0, line.round_steps);
        let last = &stations[n - 1];
        cap(&mut mesh, line.end_cap, last, dirs[n - 2], last.u_in, line.round_steps);
    }
    Some(mesh)
}

/// Finish an end: `out` points away from the line.
fn cap(mesh: &mut LineMesh, kind: LineCap, s: &Station, out: Vec2, u: f32, steps: u32) {
    let side = perp(out) * s.half;
    match kind {
        LineCap::None => {}
        LineCap::Box => {
            let reach = out * s.half;
            let a = mesh.vertex(s.p + side, u, 1.0, s.color);
            let b = mesh.vertex(s.p - side, u, 0.0, s.color);
            let c = mesh.vertex(s.p + side + reach, u, 1.0, s.color);
            let d = mesh.vertex(s.p - side + reach, u, 0.0, s.color);
            mesh.tri(a, b, c);
            mesh.tri(c, b, d);
        }
        LineCap::Round => {
            let pivot = mesh.vertex(s.p, u, 0.5, s.color);
            let from = perp(out);
            let steps = steps.max(2);
            let mut last = mesh.vertex(s.p + from * s.half, u, 1.0, s.color);
            for k in 1..=steps {
                let dir = Vec2::from_angle(-std::f32::consts::PI * k as f32 / steps as f32).rotate(from);
                let v = mesh.vertex(s.p + dir * s.half, u, if k == steps { 0.0 } else { 0.5 }, s.color);
                mesh.tri(pivot, last, v);
                last = v;
            }
        }
    }
}

/// `Line.set(e, field, value)`: write any field by name - `points` with a
/// table of `{x, y}` pairs reshapes it.
pub fn set_line_field(world: &hecs::World, entity: hecs::Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let Ok(mut c) = world.get::<&mut Line2D>(entity) else {
        return Err("no line on that entity".into());
    };
    let mut whole = serde_json::to_value(&*c).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    // Fields left out when empty are fields all the same.
    let known = map.contains_key(field) || matches!(field, "width_curve" | "gradient" | "sort_layer");
    if !known {
        return Err(format!("'{field}' is not a line field"));
    }
    // A whole number where the field is a count stays a count.
    let value = match (field, value) {
        ("round_steps", serde_json::Value::Number(n)) => {
            serde_json::Value::from(n.as_f64().unwrap_or(8.0).max(1.0) as u64)
        }
        ("z_order", serde_json::Value::Number(n)) => serde_json::Value::from(n.as_f64().unwrap_or(0.0) as i64),
        _ => value.clone(),
    };
    map.insert(field.to_string(), value);
    *c = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    Ok(())
}
