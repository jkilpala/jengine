//! Integration tests for Slice 3.5.E — skinned-mesh round-trip and
//! end-to-end CPU skinning through the rig + resolver pipeline.
#![cfg(all(feature = "native", feature = "skinned_mesh"))]

use jengine_core::animation::rig::{spawn_rig, BoneDef, SkeletonRig};
use jengine_core::animation::skinned_mesh::{
    compute_skinned_positions, BoneInfluence, SkinnedMesh, SkinnedVertex,
};
use jengine_core::ecs::components::{SkeletonRoot, Transform};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::scene::format::{
    load_scene_with_mapping, save_scene, ComponentBundle, EntityData, SceneData,
};

fn quad_mesh() -> SkinnedMesh {
    SkinnedMesh {
        texture_id: "char.png".into(),
        color: [1.0, 0.5, 0.25, 1.0],
        z_order: 2,
        bones: vec!["root".into()],
        vertices: vec![
            SkinnedVertex {
                rest_position: [-1.0, -1.0],
                uv: [0.0, 1.0],
                influences: vec![BoneInfluence {
                    bone_index: 0,
                    weight: 1.0,
                    offset: [-1.0, -1.0],
                }],
            },
            SkinnedVertex {
                rest_position: [1.0, -1.0],
                uv: [1.0, 1.0],
                influences: vec![BoneInfluence {
                    bone_index: 0,
                    weight: 1.0,
                    offset: [1.0, -1.0],
                }],
            },
            SkinnedVertex {
                rest_position: [1.0, 1.0],
                uv: [1.0, 0.0],
                influences: vec![BoneInfluence {
                    bone_index: 0,
                    weight: 1.0,
                    offset: [1.0, 1.0],
                }],
            },
            SkinnedVertex {
                rest_position: [-1.0, 1.0],
                uv: [0.0, 0.0],
                influences: vec![BoneInfluence {
                    bone_index: 0,
                    weight: 1.0,
                    offset: [-1.0, 1.0],
                }],
            },
        ],
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}

#[test]
fn scene_round_trip_preserves_skinned_mesh() {
    let scene = SceneData {
        name: "skinned".into(),
        entities: vec![EntityData {
            children: Vec::new(),
            name: "Hero".into(),
            parent: None,
            components: ComponentBundle::new().with("transform", &Transform::default()).with("skeleton_root", &SkeletonRoot { name: "rig".into() }).with("skinned_mesh", &quad_mesh()),
        }],
        ..Default::default()
    };

    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);
    let loaded = world
        .get::<&SkinnedMesh>(mapping[0])
        .expect("SkinnedMesh present after load");
    assert_eq!(loaded.texture_id, "char.png");
    assert_eq!(loaded.color, [1.0, 0.5, 0.25, 1.0]);
    assert_eq!(loaded.z_order, 2);
    assert_eq!(loaded.bones, vec!["root".to_string()]);
    assert_eq!(loaded.vertices.len(), 4);
    assert_eq!(loaded.indices, vec![0, 1, 2, 0, 2, 3]);

    let saved = save_scene(&world);
    let bundle = &saved.entities[0].components;
    let saved_mesh = bundle
        .get::<jengine_core::animation::skinned_mesh::SkinnedMesh>("skinned_mesh")
        .expect("SkinnedMesh present after save");
    assert_eq!(saved_mesh.vertices.len(), 4);
    assert_eq!(saved_mesh.indices.len(), 6);
}

#[test]
fn skinning_against_translated_root_translates_quad() {
    // Spawn a rig with a single root bone offset by (5, 0). Skin a
    // quad pinned 100% to that bone — every vertex's deformed
    // position should be `offset + (5, 0)`.
    let rig = SkeletonRig {
        name: "rig".into(),
        bones: vec![BoneDef {
            name: "root".into(),
            parent: None,
            position: [5.0, 0.0],
            rotation: 0.0,
            scale: [1.0, 1.0],
            length: 1.0,
        }],
    };
    let mut world = hecs::World::new();
    let map = spawn_rig(&mut world, &rig);
    let root = map["root"];
    resolve_world_transforms(&mut world);

    let mesh = quad_mesh();
    let positions = compute_skinned_positions(&mesh, &world, root);
    assert_eq!(positions.len(), 4);
    // Bottom-left corner offset (-1,-1) + bone world translation (5,0).
    assert!((positions[0].x - 4.0).abs() < 1e-4);
    assert!((positions[0].y - -1.0).abs() < 1e-4);
    // Top-right corner offset (1,1) + bone world translation (5,0).
    assert!((positions[2].x - 6.0).abs() < 1e-4);
    assert!((positions[2].y - 1.0).abs() < 1e-4);
}
