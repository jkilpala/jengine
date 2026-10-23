//! Jiggle bones (Oct 2026; Godot's `SkeletonModification2DJiggle`): a bone
//! whose tip lags behind the pose and springs back - a tail that trails a
//! running creature, ears that flop, a tentacle that streams behind a
//! swimming jellyfish, a feather on a hat.
//!
//! Each `jiggle` bone's tip is a point mass (Verlet, so its velocity is how
//! it moved last frame) pulled toward where the pose puts it - the clip's
//! pose, or the authored one when nothing animates the bone - by
//! `stiffness`, losing speed by `damping`, pulled by `gravity`, and kept at
//! the bone's length from its joint and within `max_angle` of the pose. The
//! bone is then turned to point at it. Bones are solved parent first, so a
//! chain of them - a tail - whips: each link's pose follows the link before
//! it as it is this frame.
//!
//! [`tick_jiggle`] runs at the end of `skeletal::update_skeletal_players`,
//! which every host already calls each frame after animation and before IK,
//! so nothing was wired. It needs a `Bone` (for the length) and is free when
//! no entity jiggles.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Bone, Parent, Transform};

fn default_stiffness() -> f32 {
    0.12
}
fn default_damping() -> f32 {
    0.88
}
fn default_gravity() -> [f32; 2] {
    [0.0, -200.0]
}
fn default_max_angle() -> f32 {
    1.3
}

/// `jiggle`, on a bone: its tip lags and springs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jiggle {
    /// How hard the tip is pulled back to the pose, a fraction of the way
    /// per frame at 60 fps: 1 is rigid, near 0 a wet noodle.
    #[serde(default = "default_stiffness")]
    pub stiffness: f32,
    /// How much of its speed the tip keeps from frame to frame at 60 fps:
    /// 1 swings for ever, 0 does not swing at all.
    #[serde(default = "default_damping")]
    pub damping: f32,
    /// A steady pull on the tip, in world units a second squared - down for
    /// a hanging tail, up for seaweed's buoyancy, sideways for a current.
    #[serde(default = "default_gravity")]
    pub gravity: [f32; 2],
    /// The furthest the bone may swing from its pose, in radians.
    #[serde(default = "default_max_angle")]
    pub max_angle: f32,
    /// The tip's simulation. Never saved.
    #[serde(skip)]
    pub state: Option<JiggleState>,
}

impl Default for Jiggle {
    fn default() -> Self {
        Self {
            stiffness: default_stiffness(),
            damping: default_damping(),
            gravity: default_gravity(),
            max_angle: default_max_angle(),
            state: None,
        }
    }
}

/// Where a jiggle bone's tip is, and was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JiggleState {
    pub tip: Vec2,
    pub prev: Vec2,
    /// The pose's local rotation, kept so a bone nothing animates swings
    /// about where it was authored rather than about where it swung to.
    pub rest: f32,
    /// The rotation this wrote last frame: if the bone still has it, nothing
    /// else posed the bone, and `rest` stands.
    pub written: f32,
}

fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

fn depth(world: &World, mut e: Entity) -> usize {
    let mut d = 0;
    while let Ok(p) = world.get::<&Parent>(e) {
        e = p.0;
        d += 1;
        if d > 64 {
            break;
        }
    }
    d
}

/// Step every jiggle bone by `dt`. See the module docs.
pub fn tick_jiggle(world: &mut World, dt: f32) {
    let mut bones: Vec<(usize, Entity)> = world
        .query::<(&Jiggle, &Bone)>()
        .iter()
        .map(|(e, _)| e)
        .collect::<Vec<_>>()
        .into_iter()
        .map(|e| (depth(world, e), e))
        .collect();
    if bones.is_empty() || dt <= 0.0 {
        return;
    }
    // A limp skeleton is posed by physics (`ecs::ragdoll`).
    if crate::ecs::ragdoll::any_limp(world) {
        bones.retain(|(_, e)| !crate::ecs::ragdoll::is_limp(world, *e));
    }
    bones.sort_by_key(|(d, e)| (*d, e.id()));
    let steps = dt * 60.0;
    let mut level = usize::MAX;
    for (d, e) in bones {
        // A link's pose follows its parent as the parent is *now*: resolve
        // the hierarchy before each depth.
        if d != level {
            crate::ecs::hierarchy::resolve_world_transforms(world);
            level = d;
        }
        let Some(parent_w) = world
            .get::<&Parent>(e)
            .ok()
            .and_then(|p| crate::ecs::hierarchy::world_transform_of(world, p.0))
        else {
            continue;
        };
        let length = world.get::<&Bone>(e).map(|b| b.length).unwrap_or(0.0);
        let Ok(local) = world.get::<&Transform>(e).map(|t| (*t).clone()) else { continue };
        let Ok(mut jig) = world.get::<&mut Jiggle>(e) else { continue };

        let rest = match jig.state {
            Some(s) if (local.rotation - s.written).abs() < 1e-6 => s.rest,
            _ => local.rotation,
        };
        let joint = parent_w.apply(local.position);
        let sx = parent_w.scale.x * local.scale.x;
        let reach = length * sx.abs();
        if reach <= 1e-4 {
            continue;
        }
        // A bone with a negative world `scale.x` points back along its
        // rotation (as IK has it).
        let turn = if sx < 0.0 { std::f32::consts::PI } else { 0.0 };
        let rest_angle = parent_w.rotation + rest + turn;
        let rest_tip = joint + Vec2::from_angle(rest_angle) * reach;

        let mut s = match jig.state {
            Some(s) if s.tip.distance(rest_tip) < reach * 4.0 => s,
            // First frame, or teleported: start at rest.
            _ => JiggleState { tip: rest_tip, prev: rest_tip, rest, written: local.rotation },
        };
        let keep = jig.damping.clamp(0.0, 1.0).powf(steps);
        let velocity = (s.tip - s.prev) * keep;
        s.prev = s.tip;
        s.tip += velocity + Vec2::from(jig.gravity) * dt * dt;
        let pull = 1.0 - (1.0 - jig.stiffness.clamp(0.0, 1.0)).powf(steps);
        s.tip += (rest_tip - s.tip) * pull;
        let along = s.tip - joint;
        let angle = if along.length_squared() > 1e-8 { along.y.atan2(along.x) } else { rest_angle };
        let limit = jig.max_angle.max(0.0);
        let angle = rest_angle + wrap(angle - rest_angle).clamp(-limit, limit);
        s.tip = joint + Vec2::from_angle(angle) * reach;
        let rotation = angle - turn - parent_w.rotation;
        s.rest = rest;
        s.written = rotation;
        jig.state = Some(s);
        drop(jig);
        if let Ok(mut t) = world.get::<&mut Transform>(e) {
            t.rotation = rotation;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::SkeletonRoot;

    #[test]
    fn a_hanging_bone_trails_its_root_and_settles_back() {
        let mut world = World::new();
        let root = world.spawn((Transform::default(), SkeletonRoot { name: "r".into() }));
        let tail = world.spawn((
            Transform { rotation: -std::f32::consts::FRAC_PI_2, ..Default::default() },
            Parent(root),
            Bone { name: "tail".into(), length: 50.0 },
            Jiggle::default(),
        ));
        let rot = |w: &World| w.get::<&Transform>(tail).unwrap().rotation;
        for _ in 0..30 {
            tick_jiggle(&mut world, 1.0 / 60.0);
        }
        let hang = rot(&world);
        assert!((hang + std::f32::consts::FRAC_PI_2).abs() < 0.05, "hangs at its pose: {hang}");
        // Move the root right quickly: the tip lags behind, to the left -
        // so the bone, pointing down, swings clockwise (its angle drops).
        for _ in 0..8 {
            world.get::<&mut Transform>(root).unwrap().position.x += 12.0;
            tick_jiggle(&mut world, 1.0 / 60.0);
        }
        assert!(rot(&world) < hang - 0.15, "trails: {} vs {hang}", rot(&world));
        for _ in 0..240 {
            tick_jiggle(&mut world, 1.0 / 60.0);
        }
        assert!((rot(&world) - hang).abs() < 0.02, "settles back: {}", rot(&world));
    }
}
