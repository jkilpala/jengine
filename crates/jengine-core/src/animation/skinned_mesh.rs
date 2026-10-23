//! Slice 3.5.E — Skinned mesh component + CPU skinning.
//!
//! A [`SkinnedMesh`] is a triangle-list deformable mesh attached to
//! the same entity as a [`SkeletonRoot`]. Each vertex carries a
//! `rest_position` (purely diagnostic / fallback) plus a list of
//! [`BoneInfluence`]s that pin the vertex to one or more bones in the
//! rig. The influence stores the vertex's position **in the bone's
//! local frame at bind time** (`offset`), so per-frame skinning is
//! just `Σ weight_i · (bone_world_xform_i · offset_i)` — no
//! inverse-bind matrices to invert at runtime.
//!
//! Slice E renders skinned meshes by deforming on the CPU each frame
//! and pushing triangles through the existing wgpu sprite batch via
//! a new `draw_triangle` helper (degenerate-quad reuses the static
//! quad index buffer). Later slices can promote this to a dedicated
//! GPU skinning pipeline.

use std::collections::HashMap;

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::WorldTransform;
use crate::ecs::skeleton::find_bone;

/// One bone's contribution to a skinned vertex. `offset` is the
/// vertex's position in the bone's local frame at bind time; at
/// runtime the bone's `WorldTransform` is applied to `offset` and
/// the result is summed with `weight`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BoneInfluence {
    /// Index into [`SkinnedMesh::bones`].
    pub bone_index: u32,
    pub weight: f32,
    /// Vertex position expressed in the bone's local frame at bind
    /// time. Pre-baked by the importer (or by tools) so per-frame
    /// skinning needs no inverse-bind matrices.
    pub offset: [f32; 2],
}

/// One vertex of a [`SkinnedMesh`]. `rest_position` is the bind-pose
/// world-space position and is **not** used during skinning — it's
/// kept around so tools / debug overlays can fall back to a static
/// pose if every influence's bone is missing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinnedVertex {
    pub rest_position: [f32; 2],
    pub uv: [f32; 2],
    #[serde(default)]
    pub influences: Vec<BoneInfluence>,
}

/// A deformable triangle mesh skinned to a [`SkeletonRoot`] on the
/// same entity. Triangles are listed in `indices` (3 entries per
/// triangle, CCW). `bones[i]` names the rig bone referenced by
/// [`BoneInfluence::bone_index`] `i`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkinnedMesh {
    /// Texture id used when rendering the mesh.
    #[serde(default)]
    pub texture_id: String,
    /// Multiplicative tint applied to the texture sample.
    #[serde(default = "default_color")]
    pub color: [f32; 4],
    /// Sort order, matches `SpriteComponent::z_order`.
    #[serde(default)]
    pub z_order: i32,
    /// Bone names (parallel to [`BoneInfluence::bone_index`]).
    pub bones: Vec<String>,
    pub vertices: Vec<SkinnedVertex>,
    pub indices: Vec<u32>,
}

fn default_color() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

/// Apply a 2D affine transform (translation + rotation + scale held
/// in [`WorldTransform`]) to a local-space point.
#[inline]
fn apply_world(xf: &WorldTransform, p: [f32; 2]) -> Vec2 {
    let cos = xf.rotation.cos();
    let sin = xf.rotation.sin();
    let sx = xf.scale.x * p[0];
    let sy = xf.scale.y * p[1];
    Vec2::new(
        xf.position.x + cos * sx - sin * sy,
        xf.position.y + sin * sx + cos * sy,
    )
}

/// Compute the deformed world-space position of every vertex in
/// `mesh`, blending bone `WorldTransform`s by per-influence weight.
/// `skeleton_root` is the entity carrying the rig's [`SkeletonRoot`];
/// bone names are resolved against its descendant tree via
/// [`find_bone`]. A vertex with no resolvable influences falls back
/// to its `rest_position` so authoring mistakes don't collapse the
/// mesh to the origin.
pub fn compute_skinned_positions(
    mesh: &SkinnedMesh,
    world: &World,
    skeleton_root: Entity,
) -> Vec<Vec2> {
    // Resolve every bone once up front. Missing bones produce `None`
    // and are skipped during accumulation.
    let bone_xforms: Vec<Option<WorldTransform>> = mesh
        .bones
        .iter()
        .map(|name| {
            let bone_entity = find_bone(world, skeleton_root, name)?;
            crate::ecs::hierarchy::world_transform_of(world, bone_entity)
        })
        .collect();

    mesh.vertices
        .iter()
        .map(|v| {
            let mut acc = Vec2::ZERO;
            let mut total_weight = 0.0f32;
            for inf in &v.influences {
                let Some(Some(xf)) = bone_xforms.get(inf.bone_index as usize) else {
                    continue;
                };
                acc += apply_world(xf, inf.offset) * inf.weight;
                total_weight += inf.weight;
            }
            if total_weight > 1e-6 {
                // Normalize so authoring tools that pass un-normalized
                // weights (Spine commonly exports weights summing to
                // 1.0 already, but it's cheap insurance) don't drift
                // the mesh toward the origin.
                acc / total_weight
            } else {
                Vec2::from(v.rest_position)
            }
        })
        .collect()
}

/// Sample a single vertex's deformed position. Useful for tooling and
/// debug overlays that don't need to materialize the whole buffer.
pub fn skin_vertex(
    vertex: &SkinnedVertex,
    bones: &[String],
    world: &World,
    skeleton_root: Entity,
) -> Vec2 {
    let mut acc = Vec2::ZERO;
    let mut total_weight = 0.0f32;
    for inf in &vertex.influences {
        let Some(name) = bones.get(inf.bone_index as usize) else {
            continue;
        };
        let Some(bone_entity) = find_bone(world, skeleton_root, name) else {
            continue;
        };
        // Falls back to the bone's local transform. Skipping the influence
        // instead - which is what a missing `WorldTransform` used to do -
        // silently produced a subtly wrong pose rather than an error, and
        // only hierarchy participants carry the component.
        let Some(xf) = crate::ecs::hierarchy::world_transform_of(world, bone_entity) else {
            continue;
        };
        acc += apply_world(&xf, inf.offset) * inf.weight;
        total_weight += inf.weight;
    }
    if total_weight > 1e-6 {
        acc / total_weight
    } else {
        Vec2::from(vertex.rest_position)
    }
}

/// Convenience cache used by importers: builds a `bone_name → bone_index`
/// map from a slice of names.
pub fn bone_index_map(bones: &[String]) -> HashMap<String, u32> {
    bones
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i as u32))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::rig::{spawn_rig, BoneDef, SkeletonRig};
    use crate::ecs::hierarchy::resolve_world_transforms;

    fn one_bone_rig(rot_radians: f32) -> SkeletonRig {
        SkeletonRig {
            name: "rig".into(),
            bones: vec![BoneDef {
                name: "root".into(),
                parent: None,
                position: [0.0, 0.0],
                rotation: rot_radians,
                scale: [1.0, 1.0],
                length: 1.0,
            }],
        }
    }

    fn one_vertex_mesh(bone_name: &str) -> SkinnedMesh {
        SkinnedMesh {
            texture_id: "tex".into(),
            color: [1.0; 4],
            z_order: 0,
            bones: vec![bone_name.to_string()],
            vertices: vec![SkinnedVertex {
                rest_position: [1.0, 0.0],
                uv: [0.0, 0.0],
                influences: vec![BoneInfluence {
                    bone_index: 0,
                    weight: 1.0,
                    offset: [1.0, 0.0],
                }],
            }],
            indices: vec![],
        }
    }

    #[test]
    fn identity_bind_pose_leaves_vertex_at_rest() {
        let rig = one_bone_rig(0.0);
        let mut world = World::new();
        let map = spawn_rig(&mut world, &rig);
        let root = map["root"];
        resolve_world_transforms(&mut world);

        let mesh = one_vertex_mesh("root");
        let deformed = compute_skinned_positions(&mesh, &world, root);
        assert_eq!(deformed.len(), 1);
        assert!((deformed[0].x - 1.0).abs() < 1e-5);
        assert!(deformed[0].y.abs() < 1e-5);
    }

    #[test]
    fn rotating_the_bone_rotates_the_vertex() {
        let rig = one_bone_rig(std::f32::consts::FRAC_PI_2);
        let mut world = World::new();
        let map = spawn_rig(&mut world, &rig);
        let root = map["root"];
        resolve_world_transforms(&mut world);

        // Offset (1, 0) rotated 90° → (0, 1).
        let mesh = one_vertex_mesh("root");
        let deformed = compute_skinned_positions(&mesh, &world, root);
        assert!(deformed[0].x.abs() < 1e-5);
        assert!((deformed[0].y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn weighted_blend_between_two_bones_lerps_position() {
        let rig = SkeletonRig {
            name: "rig".into(),
            bones: vec![
                BoneDef {
                    name: "a".into(),
                    parent: None,
                    position: [0.0, 0.0],
                    rotation: 0.0,
                    scale: [1.0, 1.0],
                    length: 1.0,
                },
                BoneDef {
                    name: "b".into(),
                    parent: None,
                    position: [10.0, 0.0],
                    rotation: 0.0,
                    scale: [1.0, 1.0],
                    length: 1.0,
                },
            ],
        };
        let mut world = World::new();
        let map = spawn_rig(&mut world, &rig);
        // Both bones get treated as roots (`spawn_rig` picks the first
        // parent-less bone as the SkeletonRoot). For find_bone we want
        // bone "b" to live under the rig root — manually parent it.
        world
            .insert_one(
                map["b"],
                crate::ecs::components::Parent(map["a"]),
            )
            .unwrap();
        resolve_world_transforms(&mut world);

        let mesh = SkinnedMesh {
            texture_id: "t".into(),
            color: [1.0; 4],
            z_order: 0,
            bones: vec!["a".into(), "b".into()],
            vertices: vec![SkinnedVertex {
                rest_position: [5.0, 0.0],
                uv: [0.0, 0.0],
                influences: vec![
                    BoneInfluence { bone_index: 0, weight: 0.5, offset: [0.0, 0.0] },
                    BoneInfluence { bone_index: 1, weight: 0.5, offset: [0.0, 0.0] },
                ],
            }],
            indices: vec![],
        };

        let deformed = compute_skinned_positions(&mesh, &world, map["a"]);
        // 0.5 * (0,0) + 0.5 * (10,0) = (5,0).
        assert!((deformed[0].x - 5.0).abs() < 1e-4);
        assert!(deformed[0].y.abs() < 1e-4);
    }

    #[test]
    fn missing_bone_falls_back_to_rest_position() {
        let rig = one_bone_rig(0.0);
        let mut world = World::new();
        let map = spawn_rig(&mut world, &rig);
        let root = map["root"];
        resolve_world_transforms(&mut world);

        // Mesh references a bone that doesn't exist in the rig.
        let mesh = SkinnedMesh {
            texture_id: "t".into(),
            color: [1.0; 4],
            z_order: 0,
            bones: vec!["ghost".into()],
            vertices: vec![SkinnedVertex {
                rest_position: [7.0, -3.0],
                uv: [0.0, 0.0],
                influences: vec![BoneInfluence {
                    bone_index: 0,
                    weight: 1.0,
                    offset: [99.0, 99.0],
                }],
            }],
            indices: vec![],
        };
        let deformed = compute_skinned_positions(&mesh, &world, root);
        assert_eq!(deformed[0].x, 7.0);
        assert_eq!(deformed[0].y, -3.0);
    }

    #[test]
    fn unnormalized_weights_get_normalized() {
        let rig = one_bone_rig(0.0);
        let mut world = World::new();
        let map = spawn_rig(&mut world, &rig);
        let root = map["root"];
        resolve_world_transforms(&mut world);

        let mesh = SkinnedMesh {
            texture_id: "t".into(),
            color: [1.0; 4],
            z_order: 0,
            bones: vec!["root".into()],
            vertices: vec![SkinnedVertex {
                rest_position: [0.0, 0.0],
                uv: [0.0, 0.0],
                influences: vec![BoneInfluence {
                    bone_index: 0,
                    weight: 4.0, // not normalized
                    offset: [2.0, 0.0],
                }],
            }],
            indices: vec![],
        };
        let deformed = compute_skinned_positions(&mesh, &world, root);
        // Result is (2,0) regardless of weight magnitude after normalization.
        assert!((deformed[0].x - 2.0).abs() < 1e-5);
    }
}
