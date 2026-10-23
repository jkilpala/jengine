//! **Sprite stacking** (Oct 2026): pseudo-3D from slices of a model.
//!
//! A [`SpriteStack`] draws its texture - a strip of `slices` equal pictures
//! left to right, the model cut horizontally from its bottom (the first) to
//! its top - as one picture a slice, each turned by the entity's own rotation
//! and raised `spacing` world units **up the screen** from the last. So a
//! house drawn as twenty slices turns in place like a model on a turntable,
//! and when the camera turns the slices still rise up the screen: the town
//! keeps standing up while it swings round under the view. `fill` draws each
//! slice that many times between itself and the next, so a stack spaced wider
//! than a texel has no gaps. `spin` turns it on its own (radians a second).
//!
//! Stacks sort among themselves by where their foot is **on the screen**
//! (`y_sort`, on by default) - which is what a turned camera needs and what a
//! sprite's `y_sort` cannot give, since that sorts by world y. Put everything
//! that stands up in a turning scene in stacks, and the ground below them in
//! a lower `z_order`.
//!
//! Drawn by `render_world_items` as one `Skinned` item a stack (lit unless
//! `unlit`), and `spin` ticked in `tick_motion` - so no host was wired.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::ecs::components::WorldTransform;

/// A model drawn as stacked slices.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpriteStack {
    /// A strip of `slices` pictures, bottom slice first (leftmost).
    pub texture_id: String,
    pub slices: u32,
    /// A slice's size in world units (before the transform's scale).
    pub size: [f32; 2],
    /// World units each slice rises up the screen from the one below.
    pub spacing: f32,
    /// Copies of each slice drawn up to the next (1: just the slices).
    pub fill: u32,
    /// Linear colour multiplied in, alpha included.
    pub color: [f32; 4],
    pub z_order: i32,
    /// Sort stacks in a band by where their foot is on screen.
    pub y_sort: bool,
    /// Radians a second it turns on its own.
    pub spin: f32,
}

impl Default for SpriteStack {
    fn default() -> Self {
        Self {
            texture_id: String::new(),
            slices: 1,
            size: [32.0, 32.0],
            spacing: 2.0,
            fill: 1,
            color: [1.0, 1.0, 1.0, 1.0],
            z_order: 0,
            y_sort: true,
            spin: 0.0,
        }
    }
}

/// The screen's up direction in the world, from a view's corners (top left,
/// top right, bottom right, bottom left); straight up with none.
pub fn screen_up(corners: Option<[Vec2; 4]>) -> Vec2 {
    corners.map(|c| (c[0] - c[3]).normalize_or_zero()).filter(|u| *u != Vec2::ZERO).unwrap_or(Vec2::Y)
}

/// Where a foot at `p` sorts: lower on the screen is later (in front), in
/// the units of `SpriteComponent::y_sort_key`.
pub fn screen_order(p: Vec2, up: Vec2) -> i32 {
    let y = p.dot(up);
    if y.is_nan() {
        return i32::MAX;
    }
    (-y * 16.0).clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

/// How far from its foot a stack can reach, for culling.
pub fn reach(stack: &SpriteStack, scale: Vec2) -> f32 {
    let s = Vec2::from(stack.size) * scale.abs();
    s.length() * 0.5 + stack.spacing.abs() * stack.slices as f32
}

/// The stack's quads in world space, bottom first: positions, `(u, v, q)`,
/// colours and indices, as a `Skinned` item takes them.
#[allow(clippy::type_complexity)]
pub fn quads(stack: &SpriteStack, wt: &WorldTransform, up: Vec2) -> (Vec<Vec2>, Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let n = stack.slices.max(1);
    let fill = stack.fill.max(1);
    let half = Vec2::from(stack.size) * wt.scale * 0.5;
    let (s, c) = wt.rotation.sin_cos();
    let turn = |v: Vec2| Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c);
    let corners = [turn(Vec2::new(-half.x, -half.y)), turn(Vec2::new(half.x, -half.y)), turn(Vec2::new(half.x, half.y)), turn(Vec2::new(-half.x, half.y))];
    let count = (n * fill) as usize;
    let (mut pos, mut uvq, mut col, mut idx) =
        (Vec::with_capacity(count * 4), Vec::with_capacity(count * 4), Vec::with_capacity(count * 4), Vec::with_capacity(count * 6));
    for i in 0..n {
        let (u0, u1) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        // The last slice is the top: no copies above it.
        let copies = if i + 1 == n { 1 } else { fill };
        for k in 0..copies {
            let rise = (i as f32 + k as f32 / fill as f32) * stack.spacing;
            let at = wt.position + up * rise;
            let base = pos.len() as u32;
            for corner in corners {
                pos.push(at + corner);
            }
            uvq.extend_from_slice(&[[u0, 1.0, 1.0], [u1, 1.0, 1.0], [u1, 0.0, 1.0], [u0, 0.0, 1.0]]);
            col.extend_from_slice(&[stack.color; 4]);
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    (pos, uvq, col, idx)
}

/// Turn every spinning stack. Called from `tick_motion`.
pub fn tick_stacks(world: &mut hecs::World, dt: f32) {
    for (_, (stack, t)) in world.query_mut::<(&SpriteStack, &mut crate::ecs::components::Transform)>() {
        if stack.spin != 0.0 {
            t.rotation = (t.rotation + stack.spin * dt).rem_euclid(std::f32::consts::TAU);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32, rot: f32) -> WorldTransform {
        WorldTransform { position: Vec2::new(x, y), rotation: rot, scale: Vec2::ONE, ..Default::default() }
    }

    #[test]
    fn slices_rise_up_the_screen_and_turn_with_the_model() {
        let stack = SpriteStack { slices: 4, size: [10.0, 10.0], spacing: 3.0, ..Default::default() };
        let (pos, uvq, _, idx) = quads(&stack, &at(100.0, 50.0, 0.0), Vec2::Y);
        assert_eq!((pos.len(), idx.len()), (16, 24), "a quad a slice");
        // The top slice sits three spacings up, its picture the last quarter.
        let top = (pos[12] + pos[14]) * 0.5;
        assert!(top.distance(Vec2::new(100.0, 59.0)) < 1e-4, "{top}");
        assert!((uvq[12][0] - 0.75).abs() < 1e-6 && (uvq[13][0] - 1.0).abs() < 1e-6);
        // A camera turned a quarter: the slices rise along its up, not world y.
        let up = Vec2::new(-1.0, 0.0);
        let (pos, ..) = quads(&stack, &at(0.0, 0.0, 0.0), up);
        let top = (pos[12] + pos[14]) * 0.5;
        assert!(top.distance(Vec2::new(-9.0, 0.0)) < 1e-4, "{top}");
        // The model's own turn turns each slice about its centre.
        let (pos, ..) = quads(&stack, &at(0.0, 0.0, std::f32::consts::FRAC_PI_2), Vec2::Y);
        assert!(pos[0].distance(Vec2::new(5.0, -5.0)) < 1e-4, "bottom-left corner turned: {}", pos[0]);
        // Filled: copies between slices, none above the top.
        let filled = SpriteStack { fill: 3, ..stack };
        let (pos, ..) = quads(&filled, &at(0.0, 0.0, 0.0), Vec2::Y);
        assert_eq!(pos.len(), (3 * 3 + 1) * 4);
    }

    #[test]
    fn lower_on_the_screen_sorts_in_front_whichever_way_it_turns() {
        assert!(screen_order(Vec2::new(0.0, -10.0), Vec2::Y) > screen_order(Vec2::new(0.0, 10.0), Vec2::Y));
        // Camera turned half round: what was above is now below.
        let up = screen_up(Some([Vec2::new(1.0, -1.0), Vec2::new(-1.0, -1.0), Vec2::new(-1.0, 1.0), Vec2::new(1.0, 1.0)]));
        assert!((up - Vec2::new(0.0, -1.0)).length() < 1e-6, "{up}");
        assert!(screen_order(Vec2::new(0.0, 10.0), up) > screen_order(Vec2::new(0.0, -10.0), up));
        assert_eq!(screen_up(None), Vec2::Y);
    }
}
