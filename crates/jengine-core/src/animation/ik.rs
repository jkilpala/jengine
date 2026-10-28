//! Slice 3.5.F — inverse kinematics solvers.
//!
//! Two-bone IK (`IkTwoBone`) attached to the **end** bone of a
//! `root → mid → end` chain rotates `root` + `mid` local rotations so
//! the chain tip reaches a world-space `target`. Bone lengths come
//! from `Bone.length`.
//!
//! Look-at IK (`IkLookAt`) attached to any bone rotates that bone's
//! local rotation so its world `+X` axis points at a world-space
//! `target` (plus an optional `rotation_offset`).
//!
//! `solve_ik` must run AFTER `update_skeletal_players` (so animation
//! has set the base pose) and BEFORE the final
//! `resolve_world_transforms` call that downstream consumers
//! (rendering, skinning) read from. The solver itself calls
//! `resolve_world_transforms` once internally to read up-to-date
//! parent rotations + joint positions before computing the IK delta.

use crate::detmath::DetMath;
use glam::Vec2;
use hecs::{Entity, World};

use crate::ecs::components::{Bone, Parent, SkeletonRoot, Transform};
use serde::{Deserialize, Serialize};
use crate::ecs::outlines::ColliderOutlines;

/// Slice 3.5.F — two-bone IK constraint attached to the *end* (effector)
/// bone of a 3-bone chain `root → mid → end`. The solver walks
/// `Parent` twice from the entity carrying this component to find the
/// `mid` and `root` bones, then rotates `root.Transform.rotation` and
/// `mid.Transform.rotation` (in local space) so that the chain's tip
/// reaches `target` (world-space). Bone lengths come from the `Bone`
/// component's `length` field.
///
/// `flip_bend` selects the "elbow up" vs "elbow down" solution.
///
/// Scaled and mirrored rigs work (Sep 2026, BUGS #152): the lengths are
/// scaled by each bone's world `scale.x`, and under a mirrored parent
/// (`scale.x * scale.y < 0`, the usual way a 2D character turns round)
/// `flip_bend` keeps its meaning on screen - the knee goes to the mirrored
/// side, as the rest of the character does.
///
/// `weight` (Sep 2026) blends the solve over the animated pose: 0 leaves the
/// clip's leg alone, 1 is all IK - so a foot can be planted and released.
/// `ground` plants an **animated** foot on what is under it: the target is
/// then not a script's point but the foot where the clip put it, moved up
/// or down onto the colliders beneath (`ecs::outlines`) by as much as the
/// ground there is above or below the clip's own floor. A walk authored on
/// flat ground walks up a slope; the body is the game's to raise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IkTwoBone {
    pub target: [f32; 2],
    #[serde(default)]
    pub flip_bend: bool,
    /// How much of the solve to apply, 0..1.
    #[serde(default = "default_ik_weight")]
    pub weight: f32,
    /// Plant the animated foot on the ground under it (see [`IkGround`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground: Option<IkGround>,
}

fn default_ik_weight() -> f32 {
    1.0
}

impl Default for IkTwoBone {
    fn default() -> Self {
        Self { target: [0.0, 0.0], flip_bend: false, weight: 1.0, ground: None }
    }
}

/// A chain of any number of bones that bends to reach a target (Oct 2026;
/// FABRIK - Godot's `SkeletonModification2DFABRIK`, Unity's 2D IK Chain).
/// On the chain's **end** bone: `bones` counts up the parents from it, so 4
/// bends this bone and the three above it. The tip - the end bone's far
/// end - goes to `target`, or to the entity named `target_name` when that
/// is set. `max_bend` limits each joint after the first to that many
/// radians either way from where the animation put it (pi is free), which
/// is what keeps a tail from folding back on itself. `weight` blends the
/// solve over the animated pose, as two-bone IK's does.
/// `Skeleton.set_ik_target` / `set_ik_weight` reach it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct IkChain {
    pub target: [f32; 2],
    #[serde(skip_serializing_if = "String::is_empty")]
    pub target_name: String,
    pub bones: u32,
    pub iterations: u32,
    /// Close enough, in world units.
    pub tolerance: f32,
    pub max_bend: f32,
    pub weight: f32,
    /// Per bone, root first: the pose the solve bends from and the angle it
    /// last wrote. A bone still at what was written was posed by nothing
    /// else, so its rest stands; one moved by a clip takes the clip's. Never
    /// saved.
    #[serde(skip)]
    pub state: Vec<(f32, f32)>,
}

impl Default for IkChain {
    fn default() -> Self {
        Self {
            target: [0.0, 0.0],
            target_name: String::new(),
            bones: 3,
            iterations: 12,
            tolerance: 0.5,
            max_bend: std::f32::consts::PI,
            weight: 1.0,
            state: Vec::new(),
        }
    }
}

/// Foot IK against the scene's colliders: where the clip's floor is, and
/// how far to look for ground above and below it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IkGround {
    /// The height, in the skeleton root's frame, of the floor the clip was
    /// authored on: a foot the clip puts here is standing on the ground.
    pub floor: f32,
    /// How far above and below that floor to look for ground.
    pub reach: f32,
    /// The collision layers that count as ground (a collider's
    /// `collision_membership`).
    pub mask: u16,
}

impl Default for IkGround {
    fn default() -> Self {
        Self { floor: 0.0, reach: 200.0, mask: u16::MAX }
    }
}

/// Slice 3.5.F — look-at IK constraint. Rotates the carrying entity's
/// local `Transform.rotation` so that its world `+X` axis (plus
/// `rotation_offset` radians) points at `target` (world-space).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IkLookAt {
    pub target: [f32; 2],
    #[serde(default)]
    pub rotation_offset: f32,
}

const EPS: f32 = 1.0e-4;

fn parent_world_rotation(world: &World, entity: Entity) -> f32 {
    world
        .get::<&Parent>(entity)
        .ok()
        .and_then(|p| {
            crate::ecs::hierarchy::world_transform_of(world, p.0).map(|w| w.rotation)
        })
        .unwrap_or(0.0)
}

/// The skeleton root a bone belongs to: the nearest ancestor carrying a
/// `SkeletonRoot`.
fn skeleton_root_of(world: &World, mut e: Entity) -> Option<Entity> {
    for _ in 0..64 {
        if world.get::<&SkeletonRoot>(e).is_ok() {
            return Some(e);
        }
        e = world.get::<&Parent>(e).ok()?.0;
    }
    None
}

/// Where `ground` plants an animated foot at `tip`: straight down (or up)
/// onto the first collider under it, keeping the height the clip gave it
/// above its own floor. `None` when there is no ground within reach.
fn ground_target(world: &World, outlines: &ColliderOutlines, end: Entity, tip: Vec2, g: &IkGround) -> Option<Vec2> {
    let root = skeleton_root_of(world, end)?;
    let root_w = crate::ecs::hierarchy::world_transform_of(world, root)?;
    // The foot in the skeleton's own frame, so a creature pitched up a
    // slope measures its lift from its own floor, not the world's level.
    let d = glam::Mat2::from_angle(-root_w.rotation) * (tip - root_w.position);
    let sy = if root_w.scale.y.abs() > EPS { root_w.scale.y } else { 1.0 };
    let local = Vec2::new(d.x / if root_w.scale.x.abs() > EPS { root_w.scale.x } else { 1.0 }, d.y / sy);
    let floor = root_w.position + glam::Mat2::from_angle(root_w.rotation) * Vec2::new(d.x, g.floor * sy);
    let lift = ((local.y - g.floor) * sy.abs()).max(0.0);
    let reach = g.reach.max(1.0);
    let hit = outlines.first_hit(Vec2::new(tip.x, floor.y + reach), Vec2::new(tip.x, floor.y - reach), g.mask)?;
    Some(Vec2::new(tip.x, hit.point.y + lift))
}

/// Run all IK constraints in the world. Resolves world transforms
/// internally before solving so the solver sees up-to-date joint
/// positions; the caller is still expected to run
/// `resolve_world_transforms` again afterwards so downstream systems
/// see the IK-modified pose.
pub fn solve_ik(world: &mut World) {
    let _span = crate::profiling::trace::span("animation.ik");
    crate::ecs::hierarchy::resolve_world_transforms(world);

    // --- Two-bone pass --------------------------------------------------
    // A limp skeleton is posed by physics (`ecs::ragdoll`).
    let limp = crate::ecs::skeleton::any_limp(world);
    let two_bone: Vec<(Entity, IkTwoBone)> = world
        .query::<&IkTwoBone>()
        .iter()
        .filter(|(e, _)| !limp || !crate::ecs::skeleton::is_limp(world, *e))
        .map(|(e, c)| (e, c.clone()))
        .collect();
    // The ground, gathered once, only when some foot looks for it.
    let outlines = if two_bone.iter().any(|(_, ik)| ik.ground.is_some()) {
        ColliderOutlines::gather(world)
    } else {
        ColliderOutlines::default()
    };
    for (end_e, ik) in two_bone {
        let weight = ik.weight.clamp(0.0, 1.0);
        if weight <= 0.0 {
            continue;
        }
        let Some(mid_e) = world.get::<&Parent>(end_e).ok().map(|p| p.0) else { continue };
        let Some(root_e) = world.get::<&Parent>(mid_e).ok().map(|p| p.0) else { continue };

        let l1 = world.get::<&Bone>(root_e).map(|b| b.length).unwrap_or(0.0);
        let l2 = world.get::<&Bone>(mid_e).map(|b| b.length).unwrap_or(0.0);
        if l1 < EPS || l2 < EPS {
            continue;
        }

        // Falls back to the local transform: only hierarchy participants
        // carry a `WorldTransform`, and a single-bone rig has none.
        let Some(root_w) = crate::ecs::hierarchy::world_transform_of(world, root_e) else {
            continue;
        };
        let mid_scale_x = crate::ecs::hierarchy::world_transform_of(world, mid_e)
            .map(|w| w.scale.x)
            .unwrap_or(root_w.scale.x);
        let root_world_pos = root_w.position;
        let parent_rot = parent_world_rotation(world, root_e);
        // `bone.length` is in the bone's own units, and the hierarchy scales
        // it: the next joint sits at `scale.x * length` along the bone. With
        // raw lengths a rig under a scaled parent put its knee in the wrong
        // place and its tip short of the target (or past it).
        let (l1, l2) = (l1 * root_w.scale.x.abs(), l2 * mid_scale_x.abs());
        if l1 < EPS || l2 < EPS {
            continue;
        }

        // Where the clip put the foot: what `ground` moves and `weight`
        // blends from.
        let tip = crate::ecs::hierarchy::world_transform_of(world, end_e).map(|w| w.position);
        let mut target = Vec2::from_array(ik.target);
        if let Some(g) = &ik.ground {
            let Some(tip) = tip else { continue };
            target = ground_target(world, &outlines, end_e, tip, g).unwrap_or(tip);
        }
        if weight < 1.0
            && let Some(tip) = tip
        {
            target = tip + (target - tip) * weight;
        }
        let d_vec = target - root_world_pos;
        let d_raw = d_vec.length();
        if d_raw < EPS {
            continue;
        }
        let d_min = (l1 - l2).abs() + EPS;
        let d_max = (l1 + l2) - EPS;
        let d = d_raw.clamp(d_min, d_max);

        let theta = d_vec.y.datan2(d_vec.x);
        let cos_a = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0);
        let a = cos_a.dacos();
        let cos_b = ((l1 * l1 + l2 * l2 - d * d) / (2.0 * l1 * l2)).clamp(-1.0, 1.0);
        let b = cos_b.dacos();

        // A mirrored rig (`scale.x * scale.y < 0`, the usual way a 2D
        // character turns round) keeps `flip_bend`'s meaning on screen: the
        // knee goes to the mirrored side, as the rest of the character does.
        let mirrored = root_w.scale.x * root_w.scale.y < 0.0;
        let sign = if ik.flip_bend ^ mirrored { -1.0 } else { 1.0 };
        // The directions the two bones must point, in the world.
        let root_dir = theta - sign * a;
        // A bone whose world `scale.x` is negative points *backwards* along
        // its rotation - its child sits at `scale.x * length` - so its
        // rotation is its direction turned half round.
        let turn = |sx: f32| if sx < 0.0 { std::f32::consts::PI } else { 0.0 };
        let root_world_rot = root_dir - turn(root_w.scale.x);
        let mid_local_rot = sign * (std::f32::consts::PI - b) + turn(root_w.scale.x) - turn(mid_scale_x);

        if let Ok(mut t) = world.get::<&mut Transform>(root_e) {
            t.rotation = root_world_rot - parent_rot;
        }
        if let Ok(mut t) = world.get::<&mut Transform>(mid_e) {
            t.rotation = mid_local_rot;
        }
    }

    // --- Chains (FABRIK) ------------------------------------------------
    solve_chains(world);

    // --- Look-at pass ---------------------------------------------------
    // Re-resolve so look-at sees the post-two-bone world rotations of
    // any ancestors it depends on.
    crate::ecs::hierarchy::resolve_world_transforms(world);
    let look: Vec<(Entity, IkLookAt)> = world
        .query::<&IkLookAt>()
        .iter()
        .filter(|(e, _)| !limp || !crate::ecs::skeleton::is_limp(world, *e))
        .map(|(e, c)| (e, c.clone()))
        .collect();
    for (bone_e, ik) in look {
        let Some(bone_w) = crate::ecs::hierarchy::world_transform_of(world, bone_e) else {
            continue;
        };
        let pos = bone_w.position;
        let parent_rot = parent_world_rotation(world, bone_e);
        let d = Vec2::from_array(ik.target) - pos;
        if d.length_squared() < EPS * EPS {
            continue;
        }
        // As in the two-bone pass: a bone with a negative world `scale.x`
        // points back along its rotation.
        let desired = d.y.datan2(d.x) - if bone_w.scale.x < 0.0 { std::f32::consts::PI } else { 0.0 };
        if let Ok(mut tr) = world.get::<&mut Transform>(bone_e) {
            tr.rotation = desired - parent_rot + ik.rotation_offset;
        }
    }
}

/// Solve every [`IkChain`]: FABRIK over the chain's joints, then each bone
/// turned to point along its solved segment. See the component.
fn solve_chains(world: &mut World) {
    let chains: Vec<(Entity, IkChain)> = world.query::<&IkChain>().iter().map(|(e, c)| (e, c.clone())).collect();
    if chains.is_empty() {
        return;
    }
    let limp = crate::ecs::skeleton::any_limp(world);
    for (end_e, ik) in chains {
        if limp && crate::ecs::skeleton::is_limp(world, end_e) {
            continue;
        }
        let weight = ik.weight.clamp(0.0, 1.0);
        if weight <= 0.0 {
            continue;
        }
        // The chain, root first.
        let mut bones = vec![end_e];
        while bones.len() < ik.bones.max(1) as usize {
            match world.get::<&Parent>(*bones.last().expect("non-empty")) {
                Ok(p) if world.get::<&Bone>(p.0).is_ok() => bones.push(p.0),
                _ => break,
            }
        }
        bones.reverse();
        crate::ecs::hierarchy::resolve_world_transforms(world);
        let Some(world_of) = bones
            .iter()
            .map(|b| crate::ecs::hierarchy::world_transform_of(world, *b))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let lengths: Vec<f32> = bones
            .iter()
            .zip(&world_of)
            .map(|(b, w)| world.get::<&Bone>(*b).map(|bone| bone.length).unwrap_or(0.0) * w.scale.x.abs())
            .collect();
        let reach: f32 = lengths.iter().sum();
        if reach < EPS {
            continue;
        }
        let target = if ik.target_name.is_empty() {
            Vec2::from_array(ik.target)
        } else {
            let named = world.query::<(&crate::ecs::components::Name,)>().iter().find(|(_, (n,))| n.0 == ik.target_name).map(|(e, _)| e);
            match named.and_then(|e| crate::ecs::hierarchy::world_transform_of(world, e)) {
                Some(w) => w.position,
                None => continue,
            }
        };
        // A bone with a negative world scale.x points back along its turn.
        let turn = |sx: f32| if sx < 0.0 { std::f32::consts::PI } else { 0.0 };
        let dir = |w: &crate::ecs::components::WorldTransform| Vec2::from_angle(w.rotation + turn(w.scale.x));
        // The joints as the animation left them, and the tip.
        let mut p: Vec<Vec2> = world_of.iter().map(|w| w.position).collect();
        let last = world_of.last().expect("non-empty");
        p.push(last.position + dir(last) * lengths[lengths.len() - 1]);
        let root = p[0];
        if (target - root).length() >= reach {
            // Out of reach: the chain straight at it.
            let d = (target - root).normalize_or_zero();
            for i in 0..lengths.len() {
                p[i + 1] = p[i] + d * lengths[i];
            }
        } else {
            for _ in 0..ik.iterations.max(1) {
                if (p[p.len() - 1] - target).length() <= ik.tolerance {
                    break;
                }
                // Backward: the tip on the target, each joint dragged after it.
                let n = p.len() - 1;
                p[n] = target;
                for i in (0..n).rev() {
                    let d = (p[i] - p[i + 1]).normalize_or_zero();
                    p[i] = p[i + 1] + d * lengths[i];
                }
                // Forward: the root back on its anchor.
                p[0] = root;
                for i in 0..n {
                    let d = (p[i + 1] - p[i]).normalize_or_zero();
                    p[i + 1] = p[i] + d * lengths[i];
                }
            }
        }
        // Each bone turned to its segment, root first, as local rotations.
        let mut state = ik.state.clone();
        state.resize(bones.len(), (f32::NAN, f32::NAN));
        let mut parent_rot = parent_world_rotation(world, bones[0]);
        for (i, b) in bones.iter().enumerate() {
            let Ok(local) = world.get::<&Transform>(*b).map(|t| t.rotation) else { continue };
            // The pose: the authored or animated angle, not what this wrote.
            let rest = if (local - state[i].1).abs() < 1e-6 { state[i].0 } else { local };
            let seg = p[i + 1] - p[i];
            let mut want = if seg.length_squared() < EPS * EPS {
                rest
            } else {
                rest + wrap_angle(seg.y.datan2(seg.x) - turn(world_of[i].scale.x) - parent_rot - rest)
            };
            // A joint bends at most `max_bend` either way from its pose.
            if i > 0 && ik.max_bend < std::f32::consts::PI {
                want = rest + wrap_angle(want - rest).clamp(-ik.max_bend, ik.max_bend);
            }
            let rot = rest + wrap_angle(want - rest) * weight;
            if let Ok(mut t) = world.get::<&mut Transform>(*b) {
                t.rotation = rot;
            }
            state[i] = (rest, rot);
            parent_rot += rot;
        }
        if let Ok(mut c) = world.get::<&mut IkChain>(end_e) {
            c.state = state;
        }
    }
}

fn wrap_angle(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::{Bone, SkeletonRoot, Transform};
    use crate::ecs::hierarchy::resolve_world_transforms;

    fn spawn_chain(world: &mut World) -> (Entity, Entity, Entity, Entity) {
        // root_pivot (SkeletonRoot, no Parent) — at origin.
        // root_bone (Parent=root_pivot, Bone len=2, rest pos (0,0), rot=0)
        // mid_bone  (Parent=root_bone, Bone len=2, rest pos (2,0), rot=0)
        // end_bone  (Parent=mid_bone, rest pos (2,0))
        let pivot = world.spawn((Transform::default(), SkeletonRoot { name: "rig".into() }));
        let root = world.spawn((
            Transform::default(),
            Parent(pivot),
            Bone { name: "root".into(), length: 2.0 },
        ));
        let mid = world.spawn((
            Transform { position: Vec2::new(2.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
            Parent(root),
            Bone { name: "mid".into(), length: 2.0 },
        ));
        let end = world.spawn((
            Transform { position: Vec2::new(2.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
            Parent(mid),
            Bone { name: "end".into(), length: 0.0 },
        ));
        (pivot, root, mid, end)
    }

    #[test]
    fn two_bone_reaches_target_within_range() {
        let mut world = World::new();
        let (_p, _r, _m, end) = spawn_chain(&mut world);
        // Target at (2, 2): reachable (within l1+l2 = 4). End should
        // end up at the target.
        world.insert_one(end, IkTwoBone { target: [2.0, 2.0], flip_bend: false, ..Default::default() }).unwrap();
        solve_ik(&mut world);
        resolve_world_transforms(&mut world);
        let end_world = crate::ecs::hierarchy::world_transform_of(&world, end).unwrap();
        let dx = end_world.position.x - 2.0;
        let dy = end_world.position.y - 2.0;
        assert!((dx * dx + dy * dy).sqrt() < 1.0e-3, "end at {:?}", end_world.position);
    }

    #[test]
    fn two_bone_clamps_when_target_out_of_reach() {
        let mut world = World::new();
        let (_p, _r, _m, end) = spawn_chain(&mut world);
        // Target way out at (100, 0): unreachable, chain should
        // straighten along the +X axis. End position = (l1+l2, 0).
        world.insert_one(end, IkTwoBone { target: [100.0, 0.0], flip_bend: false, ..Default::default() }).unwrap();
        solve_ik(&mut world);
        resolve_world_transforms(&mut world);
        let end_world = crate::ecs::hierarchy::world_transform_of(&world, end).unwrap();
        assert!((end_world.position.x - 4.0).abs() < 1.0e-2, "end x={}", end_world.position.x);
        assert!(end_world.position.y.abs() < 1.0e-2, "end y={}", end_world.position.y);
    }

    #[test]
    fn two_bone_flip_bend_mirrors_solution() {
        let mut world_a = World::new();
        let (_p, _r, mid_a, end_a) = spawn_chain(&mut world_a);
        world_a.insert_one(end_a, IkTwoBone { target: [2.0, 2.0], flip_bend: false, ..Default::default() }).unwrap();
        solve_ik(&mut world_a);
        let rot_a = world_a.get::<&Transform>(mid_a).unwrap().rotation;

        let mut world_b = World::new();
        let (_p, _r, mid_b, end_b) = spawn_chain(&mut world_b);
        world_b.insert_one(end_b, IkTwoBone { target: [2.0, 2.0], flip_bend: true, ..Default::default() }).unwrap();
        solve_ik(&mut world_b);
        let rot_b = world_b.get::<&Transform>(mid_b).unwrap().rotation;

        assert!((rot_a + rot_b).abs() < 1.0e-4, "rot_a={}, rot_b={}", rot_a, rot_b);
    }

    #[test]
    fn look_at_points_bone_at_target() {
        let mut world = World::new();
        let pivot = world.spawn((Transform::default(), SkeletonRoot { name: "rig".into() }));
        let bone = world.spawn((
            Transform::default(),
            Parent(pivot),
            Bone { name: "aim".into(), length: 1.0 },
            IkLookAt { target: [0.0, 5.0], rotation_offset: 0.0 },
        ));
        solve_ik(&mut world);
        let tr = world.get::<&Transform>(bone).unwrap();
        // Target straight up → rotation = π/2.
        assert!((tr.rotation - std::f32::consts::FRAC_PI_2).abs() < 1.0e-4, "rot={}", tr.rotation);
    }

    #[test]
    fn look_at_subtracts_parent_world_rotation() {
        let mut world = World::new();
        // Parent rotated 90° → child's local rotation must subtract
        // that to land on world rotation π/2 (target straight up).
        let pivot = world.spawn((
            Transform { position: Vec2::ZERO, rotation: std::f32::consts::FRAC_PI_2, scale: Vec2::ONE },
            SkeletonRoot { name: "rig".into() },
        ));
        let bone = world.spawn((
            Transform::default(),
            Parent(pivot),
            Bone { name: "aim".into(), length: 1.0 },
            IkLookAt { target: [0.0, 5.0], rotation_offset: 0.0 },
        ));
        solve_ik(&mut world);
        let tr = world.get::<&Transform>(bone).unwrap();
        assert!(tr.rotation.abs() < 1.0e-4, "rot={}", tr.rotation);
    }

    // --- weight and ground (Sep 2026) -----------------------------------

    /// A leg hanging straight down from (0, 4): thigh and shin of 2, the
    /// foot at (0, 0) - the floor.
    fn spawn_leg(world: &mut World, x: f32) -> (Entity, Entity) {
        let pivot = world.spawn((
            Transform { position: Vec2::new(x, 4.0), rotation: 0.0, scale: Vec2::ONE },
            SkeletonRoot { name: "leg".into() },
        ));
        let root = world.spawn((
            Transform { position: Vec2::ZERO, rotation: -std::f32::consts::FRAC_PI_2, scale: Vec2::ONE },
            Parent(pivot),
            Bone { name: "thigh".into(), length: 2.0 },
        ));
        let mid = world.spawn((
            Transform { position: Vec2::new(2.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
            Parent(root),
            Bone { name: "shin".into(), length: 2.0 },
        ));
        let end = world.spawn((
            Transform { position: Vec2::new(2.0, 0.0), rotation: 0.0, scale: Vec2::ONE },
            Parent(mid),
            Bone { name: "foot".into(), length: 0.0 },
        ));
        (pivot, end)
    }

    fn tip(world: &mut World, end: Entity) -> Vec2 {
        resolve_world_transforms(world);
        crate::ecs::hierarchy::world_transform_of(world, end).unwrap().position
    }

    fn slab(world: &mut World, top: f32) {
        let col: crate::ecs::components::Collider2D = serde_json::from_value(
            serde_json::json!({ "shape": { "Box": { "width": 100.0, "height": 2.0 } } }),
        )
        .unwrap();
        world.spawn((Transform { position: Vec2::new(0.0, top - 1.0), rotation: 0.0, scale: Vec2::ONE }, col));
    }

    #[test]
    fn ground_plants_the_animated_foot_on_what_is_under_it() {
        // The clip's floor is 4 below the pivot (`floor: -4`); the ground
        // here is 1 higher, so the foot is lifted 1 and the knee bends.
        let mut world = World::new();
        let (_pivot, end) = spawn_leg(&mut world, 0.0);
        slab(&mut world, 1.0);
        world
            .insert_one(end, IkTwoBone { ground: Some(IkGround { floor: -4.0, ..Default::default() }), ..Default::default() })
            .unwrap();
        solve_ik(&mut world);
        let t = tip(&mut world, end);
        assert!((t.y - 1.0).abs() < 1e-3 && t.x.abs() < 1e-3, "on the raised ground: {t:?}");

    }

    #[test]
    fn no_ground_under_the_foot_leaves_the_clip_alone() {
        let mut world = World::new();
        let (_pivot, end) = spawn_leg(&mut world, 500.0);
        slab(&mut world, 1.0);
        world
            .insert_one(end, IkTwoBone { ground: Some(IkGround { floor: -4.0, ..Default::default() }), ..Default::default() })
            .unwrap();
        solve_ik(&mut world);
        let t = tip(&mut world, end);
        assert!((t - Vec2::new(500.0, 0.0)).length() < 1e-3, "{t:?}");
    }

    #[test]
    fn weight_blends_the_solve_over_the_pose() {
        let mut world = World::new();
        let (_pivot, end) = spawn_leg(&mut world, 0.0);
        world.insert_one(end, IkTwoBone { target: [0.0, 2.0], weight: 0.0, ..Default::default() }).unwrap();
        solve_ik(&mut world);
        assert!((tip(&mut world, end) - Vec2::ZERO).length() < 1e-3, "weight 0 is the clip's pose");
        world.get::<&mut IkTwoBone>(end).unwrap().weight = 0.5;
        solve_ik(&mut world);
        let t = tip(&mut world, end);
        assert!((t - Vec2::new(0.0, 1.0)).length() < 1e-2, "half way to the target: {t:?}");
    }
}
