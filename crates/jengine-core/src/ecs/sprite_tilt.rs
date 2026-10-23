//! **Sprite tilt** (Oct 2026): a sprite turned about its own x and y axes,
//! in perspective - a playing card leaning toward the pointer, a coin
//! spinning on its edge, a door swinging open, a card flipped over to show
//! its back.
//!
//! A [`SpriteTilt`] on an entity with a sprite draws that sprite as one quad
//! whose corners are the sprite's, turned about its middle by `yaw` (about
//! its up axis: positive swings the right side away) and `pitch` (about its
//! across axis: positive tips the top away), then seen from `distance` in
//! front of it. Every corner carries its perspective weight `q`, which the
//! sprite shader already divides out per pixel (`(u q, v q, q)`, as a
//! `mesh` and the Mode 7 ground do) - so the picture is foreshortened, not
//! bent along the quad's diagonal. Turned past a quarter, the sprite shows
//! its back: `back_texture`, or the front seen through from behind.
//!
//! `target` eases the turn toward a pair of angles at `speed`, so a script
//! that sets where a card should lean once gets the motion for nothing.
//! Collected by `render_world_items` as a `Skinned` item with the sprite's
//! own material, and eased in `tick_motion`, so no host was wired. A tilted
//! sprite draws whole: its `fill` is not applied.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{SpriteComponent, WorldTransform};

/// A sprite turned in perspective.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpriteTilt {
    /// Turn about the sprite's up axis, radians: positive swings its right
    /// side away. Past a quarter turn either way, its back shows.
    pub yaw: f32,
    /// Turn about the sprite's across axis, radians: positive tips its top
    /// away.
    pub pitch: f32,
    /// Where the turn eases to, `[yaw, pitch]`; none holds it where it is.
    pub target: Option<[f32; 2]>,
    /// How fast it eases there: the share of the way closed in a second, as
    /// a rate (`1 - e^(-speed t)`). 0 snaps.
    pub speed: f32,
    /// How far the eye is in front of the sprite, in the sprite's own drawn
    /// units - smaller is stronger perspective. At least the sprite's half
    /// diagonal is sensible.
    pub distance: f32,
    /// What shows when it faces away; empty shows the front seen from
    /// behind (mirrored).
    pub back_texture: String,
    /// The back's colour (times the sprite's when the back is the front).
    pub back_color: [f32; 4],
    /// Brightens the corners turned toward the eye and darkens those turned
    /// away by up to this much - a sheen that slides as it tilts.
    pub shine: f32,
}

impl Default for SpriteTilt {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            target: None,
            speed: 10.0,
            distance: 600.0,
            back_texture: String::new(),
            back_color: [1.0, 1.0, 1.0, 1.0],
            shine: 0.0,
        }
    }
}

impl SpriteTilt {
    /// Whether it faces away from the eye (its back shows).
    pub fn shows_back(&self) -> bool {
        self.yaw.cos() * self.pitch.cos() < 0.0
    }
}

/// A tilted sprite's quad: world positions, `(u q, v q, q)`, colours,
/// indices, and whether it is the back that shows.
pub struct TiltQuad {
    pub positions: Vec<Vec2>,
    pub uvq: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub back: bool,
}

/// The quad `tilt` makes of `sprite` (drawn at `size`, its texture's `uv`
/// rectangle `[u0, v0, u1, v1]`) placed by `wt`.
pub fn quad(sprite: &SpriteComponent, tilt: &SpriteTilt, wt: &WorldTransform, size: Vec2, uv: [f32; 4]) -> TiltQuad {
    let (ax, ay) = (sprite.anchor[0], sprite.anchor[1]);
    let centre = Vec2::new((0.5 - ax) * size.x, (0.5 - ay) * size.y);
    let half = size * 0.5;
    let reach = half.length().max(1.0);
    let d = tilt.distance.max(1.0);
    let (sy, cy) = tilt.yaw.sin_cos();
    let (sp, cp) = tilt.pitch.sin_cos();
    let back = tilt.shows_back();
    // A back of its own reads the right way round, so its texture runs
    // across the turned corners the other way; with none, the front is seen
    // through from behind - mirrored, as the turn alone leaves it.
    let own_back = back && !tilt.back_texture.is_empty();
    let [u0, v0, u1, v1] = if own_back { [0.0, 0.0, 1.0, 1.0] } else { uv };
    let (ul, ur) = if own_back { (u1, u0) } else { (u0, u1) };
    let color = if back {
        let b = tilt.back_color;
        if tilt.back_texture.is_empty() {
            [sprite.color[0] * b[0], sprite.color[1] * b[1], sprite.color[2] * b[2], sprite.color[3] * b[3]]
        } else {
            b
        }
    } else {
        sprite.color
    };
    // Bottom left, bottom right, top right, top left, about the middle.
    let corners = [(-half.x, -half.y, ul, v1), (half.x, -half.y, ur, v1), (half.x, half.y, ur, v0), (-half.x, half.y, ul, v0)];
    let mut positions = Vec::with_capacity(4);
    let mut uvq = Vec::with_capacity(4);
    let mut colors = Vec::with_capacity(4);
    for (x, y, u, v) in corners {
        // Yaw about the up axis, then pitch about the across axis; z is
        // toward the eye.
        let (x1, z1) = (x * cy, -x * sy);
        let (y2, z2) = (y * cp + z1 * sp, -y * sp + z1 * cp);
        let q = d / (d - z2).max(d * 0.05);
        positions.push(wt.apply(centre + Vec2::new(x1, y2) * q));
        uvq.push([u * q, v * q, q]);
        let k = (1.0 + tilt.shine * (z2 / reach).clamp(-1.0, 1.0)).max(0.0);
        colors.push([color[0] * k, color[1] * k, color[2] * k, color[3]]);
    }
    TiltQuad { positions, uvq, colors, indices: vec![0, 1, 2, 0, 2, 3], back }
}

/// Ease every tilt toward its target. Called from `tick_motion`.
pub fn tick_tilts(world: &mut World, dt: f32) {
    for (_, tilt) in world.query_mut::<&mut SpriteTilt>() {
        let Some([yaw, pitch]) = tilt.target else { continue };
        let k = if tilt.speed <= 0.0 { 1.0 } else { 1.0 - (-tilt.speed * dt.max(0.0)).exp() };
        tilt.yaw += (yaw - tilt.yaw) * k;
        tilt.pitch += (pitch - tilt.pitch) * k;
        if (yaw - tilt.yaw).abs() < 1e-4 && (pitch - tilt.pitch).abs() < 1e-4 {
            tilt.yaw = yaw;
            tilt.pitch = pitch;
        }
    }
}

/// `SpriteTilt.set(e, field, value)`: any field by name, through serde.
pub fn set_field(world: &mut World, e: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut tilt = world.get::<&mut SpriteTilt>(e).map_err(|_| "no sprite_tilt on that entity".to_string())?;
    let mut v = serde_json::to_value(&*tilt).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("not an object")?;
    if !obj.contains_key(field) {
        return Err(format!("sprite_tilt has no field '{field}'"));
    }
    obj.insert(field.to_string(), value.clone());
    *tilt = serde_json::from_value(v).map_err(|e| format!("sprite_tilt.{field}: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> SpriteComponent {
        SpriteComponent { size: Some([100.0, 140.0]), ..Default::default() }
    }

    fn at(x: f32, y: f32) -> WorldTransform {
        WorldTransform { position: Vec2::new(x, y), scale: Vec2::ONE, ..Default::default() }
    }

    const UV: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    #[test]
    fn untilted_it_is_the_sprite() {
        let q = quad(&card(), &SpriteTilt::default(), &at(10.0, 20.0), Vec2::new(100.0, 140.0), UV);
        let want = [Vec2::new(-40.0, -50.0), Vec2::new(60.0, -50.0), Vec2::new(60.0, 90.0), Vec2::new(-40.0, 90.0)];
        for (p, w) in q.positions.iter().zip(want) {
            assert!(p.distance(w) < 1e-4, "{p} vs {w}");
        }
        assert!(q.uvq.iter().all(|c| (c[2] - 1.0).abs() < 1e-6), "no perspective: {:?}", q.uvq);
        assert_eq!(q.uvq[0], [0.0, 1.0, 1.0], "bottom left is the texture's bottom left");
        assert!(!q.back);
    }

    #[test]
    fn a_turned_card_is_nearer_on_one_side() {
        let tilt = SpriteTilt { yaw: 0.6, distance: 300.0, ..Default::default() };
        let q = quad(&card(), &tilt, &at(0.0, 0.0), Vec2::new(100.0, 140.0), UV);
        let left = q.positions[3].y - q.positions[0].y;
        let right = q.positions[2].y - q.positions[1].y;
        assert!(left > 140.0 && right < 140.0, "the left edge swings near and grows: {left} vs {right}");
        let width = q.positions[1].x - q.positions[0].x;
        assert!(width < 100.0, "turned, it is narrower: {width}");
        assert!(q.uvq[0][2] > 1.0 && q.uvq[1][2] < 1.0, "nearer corners weigh more: {:?}", q.uvq);
        // Its middle stays put: the perspective-correct midpoint of the
        // bottom edge (u = 0.5) lands at x = 0, not halfway along the edge.
        let (a, b) = (q.uvq[0], q.uvq[1]);
        let t = 0.5 * a[2] / (0.5 * a[2] + 0.5 * b[2]);
        let x = q.positions[0].x + (q.positions[1].x - q.positions[0].x) * t;
        let u = (a[0] + (b[0] - a[0]) * t) / (a[2] + (b[2] - a[2]) * t);
        assert!(x.abs() < 1e-3 && (u - 0.5).abs() < 1e-4, "the middle of the texture at the middle: x {x}, u {u}");
    }

    #[test]
    fn a_tipped_card_leans_its_top_away() {
        let tilt = SpriteTilt { pitch: 0.5, distance: 300.0, ..Default::default() };
        let q = quad(&card(), &tilt, &at(0.0, 0.0), Vec2::new(100.0, 140.0), UV);
        let bottom = q.positions[1].x - q.positions[0].x;
        let top = q.positions[2].x - q.positions[3].x;
        assert!(bottom > top, "the top is farther, so narrower: {bottom} vs {top}");
    }

    #[test]
    fn turned_over_it_shows_its_back_mirrored() {
        let tilt = SpriteTilt { yaw: std::f32::consts::PI, back_texture: "back.png".into(), ..Default::default() };
        let q = quad(&card(), &tilt, &at(0.0, 0.0), Vec2::new(100.0, 140.0), [0.25, 0.0, 0.5, 1.0]);
        assert!(q.back);
        // The card's left corner has swung to the screen's right, and the
        // back texture - whole - reads the right way round: its left (u 0)
        // on the corner now on the screen's left.
        assert!((q.positions[0].x - 50.0).abs() < 1e-3, "the left corner has swung right: {}", q.positions[0].x);
        assert!((q.positions[1].x + 50.0).abs() < 1e-3);
        assert!(q.uvq[1][0].abs() < 1e-4 && (q.uvq[0][0] - 1.0).abs() < 1e-4, "{:?}", q.uvq);
        // With no back, the front is seen through from behind: its own
        // rectangle, mirrored - its left (0.25) now on the screen's right.
        let front = SpriteTilt { yaw: std::f32::consts::PI, ..Default::default() };
        let q = quad(&card(), &front, &at(0.0, 0.0), Vec2::new(100.0, 140.0), [0.25, 0.0, 0.5, 1.0]);
        assert!((q.uvq[0][0] - 0.25).abs() < 1e-4 && (q.uvq[1][0] - 0.5).abs() < 1e-4, "{:?}", q.uvq);
    }

    #[test]
    fn shine_brightens_the_near_side() {
        let tilt = SpriteTilt { yaw: 0.5, shine: 0.5, ..Default::default() };
        let q = quad(&card(), &tilt, &at(0.0, 0.0), Vec2::new(100.0, 140.0), UV);
        assert!(q.colors[0][0] > 1.0 && q.colors[1][0] < 1.0, "{:?}", q.colors);
        assert_eq!(q.colors[0][3], 1.0, "alpha is left alone");
    }

    #[test]
    fn a_target_is_eased_to_and_reached() {
        let mut world = World::new();
        let e = world.spawn((SpriteTilt { target: Some([1.0, -0.5]), speed: 8.0, ..Default::default() },));
        tick_tilts(&mut world, 0.05);
        let yaw = world.get::<&SpriteTilt>(e).unwrap().yaw;
        assert!(yaw > 0.2 && yaw < 0.5, "part of the way: {yaw}");
        for _ in 0..200 {
            tick_tilts(&mut world, 0.05);
        }
        let t = (*world.get::<&SpriteTilt>(e).unwrap()).clone();
        assert_eq!((t.yaw, t.pitch), (1.0, -0.5), "and then exactly there");
        set_field(&mut world, e, "speed", &serde_json::json!(0.0)).unwrap();
        set_field(&mut world, e, "target", &serde_json::json!([0.0, 0.0])).unwrap();
        tick_tilts(&mut world, 0.01);
        assert_eq!(world.get::<&SpriteTilt>(e).unwrap().yaw, 0.0, "speed 0 snaps");
        assert!(set_field(&mut world, e, "nonsense", &serde_json::json!(1)).is_err());
    }
}
