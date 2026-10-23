//! Slice 3.5.D — Skeleton rig data + importers.
//!
//! A [`SkeletonRig`] is an authored description of a rig (one
//! [`BoneDef`] per bone, plus parent links and a bind-pose
//! `Transform`). It is the data side of slice B's runtime components.
//! Rigs are spawned into a hecs world via [`spawn_rig`], which creates
//! one entity per bone, wires up `Parent` links from slice A, and
//! tags the root with a `SkeletonRoot` from slice B. From that point
//! on, slice C's [`crate::animation::skeletal::update_skeletal_players`]
//! drives the bone Transforms and slice A's resolver composes them
//! into `WorldTransform`s.
//!
//! Two import paths are supported:
//!
//! * **`.rig` JSON** — homegrown serde format that maps 1:1 onto
//!   [`SkeletonRig`]. Every runtime loads every `.rig` under `assets/`,
//!   keyed by its path there (`rigs/arm.rig`), for `Skeleton.spawn_rig`:
//!   natively [`crate::asset::AssetManager::load_skeleton_rigs_from_dir`],
//!   on the web from the builder's `skelanim_manifest.json`, in the editor
//!   pushed by `viewport_load_skeleton_rig`.
//! * **Spine JSON (subset)** — parsed by [`parse_spine_json`].
//!   Handles `bones` (name / parent / x / y / rotation /
//!   scaleX / scaleY / length) and `animations.bones`
//!   (`translate` / `rotate` / `scale` keyframes). Rotation values
//!   are converted from Spine's degrees to our radians. Anything
//!   else in the file (slots, skins, IK, mesh deform, events, …) is
//!   silently ignored — those are higher slices in the skeletal
//!   stack.

use std::collections::HashMap;

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::animation::skeletal::{BoneTrack, ScalarKey, SkeletalClip, Vec2Key};
use crate::ecs::components::{Bone, Parent, SkeletonRoot, Transform};

const DEG2RAD: f32 = std::f32::consts::PI / 180.0;

fn one_vec() -> [f32; 2] {
    [1.0, 1.0]
}
fn one_f32() -> f32 {
    1.0
}

/// One bone in a [`SkeletonRig`]. Position / rotation / scale form the
/// bind pose written into the spawned entity's `Transform`. `length`
/// goes into the `Bone` component (purely cosmetic for editor gizmos
/// today, used by IK / mesh deform in later slices).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoneDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default)]
    pub position: [f32; 2],
    /// Bind-pose rotation in **radians**.
    #[serde(default)]
    pub rotation: f32,
    #[serde(default = "one_vec")]
    pub scale: [f32; 2],
    #[serde(default = "one_f32")]
    pub length: f32,
}

/// Authored rig definition. Bones are listed in any order — parent
/// links are resolved by name in [`spawn_rig`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkeletonRig {
    pub name: String,
    #[serde(default)]
    pub bones: Vec<BoneDef>,
}

/// Spawn `rig` and return its root bone - the entity carrying
/// [`SkeletonRoot`], which is what a script's `spawn_rig` handle names in
/// every runtime. `None` for a rig with no bones.
pub fn spawn_rig_root(world: &mut World, rig: &SkeletonRig) -> Option<Entity> {
    let map = spawn_rig(world, rig);
    map.values().copied().find(|&e| world.get::<&SkeletonRoot>(e).is_ok())
}

/// Spawn every bone in `rig` as a fresh entity. Returns a `name →
/// Entity` map so callers can attach sprites or read back specific
/// bones. The first parent-less bone (or the first bone overall, if
/// every bone has a parent) becomes the skeleton root and receives
/// the [`SkeletonRoot`] component carrying `rig.name`. Every other
/// bone is wired to its parent via slice-A `Parent`.
pub fn spawn_rig(world: &mut World, rig: &SkeletonRig) -> HashMap<String, Entity> {
    let mut by_name: HashMap<String, Entity> = HashMap::with_capacity(rig.bones.len());

    // Pass 1: spawn each bone with its bind-pose Transform + Bone.
    for def in &rig.bones {
        let transform = Transform {
            position: Vec2::new(def.position[0], def.position[1]),
            rotation: def.rotation,
            scale: Vec2::new(def.scale[0], def.scale[1]),
        };
        let bone = Bone {
            name: def.name.clone(),
            length: def.length,
        };
        let e = world.spawn((transform, bone));
        by_name.insert(def.name.clone(), e);
    }

    // Pass 2: parent links + pick the root.
    let mut root_assigned = false;
    for def in &rig.bones {
        let e = by_name[&def.name];
        match &def.parent {
            Some(p) => {
                if let Some(&parent_e) = by_name.get(p) {
                    let _ = world.insert_one(e, Parent(parent_e));
                }
            }
            None => {
                if !root_assigned {
                    let _ = world.insert_one(e, SkeletonRoot { name: rig.name.clone() });
                    root_assigned = true;
                }
            }
        }
    }
    if !root_assigned
        && let Some(first) = rig.bones.first()
    {
        let e = by_name[&first.name];
        let _ = world.insert_one(e, SkeletonRoot { name: rig.name.clone() });
    }

    by_name
}

// ---------------------------------------------------------------------
// Spine subset importer
// ---------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct SpineFile {
    #[serde(default)]
    skeleton: SpineSkeletonMeta,
    #[serde(default)]
    bones: Vec<SpineBone>,
    #[serde(default)]
    animations: HashMap<String, SpineAnimation>,
}

#[derive(Debug, Default, Deserialize)]
struct SpineSkeletonMeta {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpineBone {
    name: String,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    /// Spine stores rotation in degrees.
    #[serde(default)]
    rotation: f32,
    #[serde(default = "one_f32", rename = "scaleX")]
    scale_x: f32,
    #[serde(default = "one_f32", rename = "scaleY")]
    scale_y: f32,
    #[serde(default)]
    length: f32,
}

#[derive(Debug, Default, Deserialize)]
struct SpineAnimation {
    #[serde(default)]
    bones: HashMap<String, SpineBoneAnim>,
}

#[derive(Debug, Default, Deserialize)]
struct SpineBoneAnim {
    #[serde(default)]
    translate: Vec<SpineVec2Key>,
    #[serde(default)]
    rotate: Vec<SpineRotKey>,
    #[serde(default)]
    scale: Vec<SpineVec2Key>,
}

#[derive(Debug, Deserialize)]
struct SpineVec2Key {
    #[serde(default)]
    time: f32,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
}

#[derive(Debug, Deserialize)]
struct SpineRotKey {
    #[serde(default)]
    time: f32,
    // Spine 4.x uses "value"; older exports used "angle".
    #[serde(default, alias = "angle")]
    value: f32,
}

/// Parse a Spine-format JSON document into a [`SkeletonRig`] plus one
/// [`SkeletalClip`] per top-level animation entry. The returned
/// clips' `duration` is the maximum keyframe time across all tracks
/// in that animation. Rotation values are converted from degrees to
/// radians on the way in so they plug straight into our `Transform`.
pub fn parse_spine_json(json: &str) -> anyhow::Result<(SkeletonRig, Vec<SkeletalClip>)> {
    let file: SpineFile = serde_json::from_str(json)?;

    let bones = file
        .bones
        .into_iter()
        .map(|b| BoneDef {
            name: b.name,
            parent: b.parent,
            position: [b.x, b.y],
            rotation: b.rotation * DEG2RAD,
            scale: [b.scale_x, b.scale_y],
            length: b.length,
        })
        .collect();

    let rig = SkeletonRig {
        name: file.skeleton.name.unwrap_or_else(|| "spine".to_string()),
        bones,
    };

    let mut clips = Vec::with_capacity(file.animations.len());
    for (anim_name, anim) in file.animations {
        let mut tracks = Vec::with_capacity(anim.bones.len());
        let mut duration = 0.0f32;
        for (bone_name, b) in anim.bones {
            let position: Vec<Vec2Key> = b
                .translate
                .into_iter()
                .map(|k| {
                    duration = duration.max(k.time);
                    Vec2Key {
                        time: k.time,
                        value: [k.x, k.y],
                    }
                })
                .collect();
            let rotation: Vec<ScalarKey> = b
                .rotate
                .into_iter()
                .map(|k| {
                    duration = duration.max(k.time);
                    ScalarKey {
                        time: k.time,
                        value: k.value * DEG2RAD,
                    }
                })
                .collect();
            let scale: Vec<Vec2Key> = b
                .scale
                .into_iter()
                .map(|k| {
                    duration = duration.max(k.time);
                    Vec2Key {
                        time: k.time,
                        value: [k.x, k.y],
                    }
                })
                .collect();
            tracks.push(BoneTrack {
                bone_name,
                position,
                rotation,
                scale,
            });
        }
        clips.push(SkeletalClip {
            name: anim_name,
            duration,
            tracks,
        });
    }

    Ok((rig, clips))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::skeleton::find_bone;

    #[test]
    fn spawn_rig_builds_hierarchy_and_picks_root() {
        let rig = SkeletonRig {
            name: "hero".into(),
            bones: vec![
                BoneDef {
                    name: "root".into(),
                    parent: None,
                    position: [10.0, 20.0],
                    rotation: 0.0,
                    scale: [1.0, 1.0],
                    length: 1.0,
                },
                BoneDef {
                    name: "torso".into(),
                    parent: Some("root".into()),
                    position: [0.0, 5.0],
                    rotation: 0.0,
                    scale: [1.0, 1.0],
                    length: 1.0,
                },
                BoneDef {
                    name: "arm".into(),
                    parent: Some("torso".into()),
                    position: [3.0, 0.0],
                    rotation: 0.0,
                    scale: [1.0, 1.0],
                    length: 1.0,
                },
            ],
        };

        let mut world = World::new();
        let map = spawn_rig(&mut world, &rig);

        // Root carries SkeletonRoot named after the rig.
        let root = map["root"];
        let sr = world.get::<&SkeletonRoot>(root).unwrap();
        assert_eq!(sr.name, "hero");

        // Children carry Parent links.
        let torso = map["torso"];
        let arm = map["arm"];
        assert_eq!(world.get::<&Parent>(torso).unwrap().0, root);
        assert_eq!(world.get::<&Parent>(arm).unwrap().0, torso);

        // find_bone from slice B can reach every bone via the rig root.
        assert_eq!(find_bone(&world, root, "arm"), Some(arm));
        assert_eq!(find_bone(&world, root, "torso"), Some(torso));

        // Bind-pose Transforms made it through.
        let torso_t = world.get::<&Transform>(torso).unwrap();
        assert_eq!(torso_t.position.x, 0.0);
        assert_eq!(torso_t.position.y, 5.0);
    }

    #[test]
    fn parse_spine_subset_extracts_bones_and_animation() {
        let json = r#"{
            "skeleton": { "name": "demo" },
            "bones": [
                { "name": "root" },
                { "name": "arm", "parent": "root", "x": 5, "y": 0, "rotation": 90, "length": 12 }
            ],
            "animations": {
                "wave": {
                    "bones": {
                        "arm": {
                            "translate": [
                                { "time": 0, "x": 0, "y": 0 },
                                { "time": 0.5, "x": 10, "y": 0 }
                            ],
                            "rotate": [
                                { "time": 0, "value": 0 },
                                { "time": 1.0, "value": 180 }
                            ]
                        }
                    }
                }
            }
        }"#;

        let (rig, clips) = parse_spine_json(json).expect("spine parse");
        assert_eq!(rig.name, "demo");
        assert_eq!(rig.bones.len(), 2);

        let arm = rig.bones.iter().find(|b| b.name == "arm").unwrap();
        assert_eq!(arm.parent.as_deref(), Some("root"));
        assert_eq!(arm.position, [5.0, 0.0]);
        // 90 degrees → π/2 radians.
        assert!((arm.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert_eq!(arm.length, 12.0);

        assert_eq!(clips.len(), 1);
        let wave = &clips[0];
        assert_eq!(wave.name, "wave");
        // Max keyframe time across translate+rotate is 1.0.
        assert!((wave.duration - 1.0).abs() < 1e-5);
        let arm_track = wave
            .tracks
            .iter()
            .find(|t| t.bone_name == "arm")
            .expect("arm track");
        assert_eq!(arm_track.position.len(), 2);
        assert_eq!(arm_track.position[1].value, [10.0, 0.0]);
        // Last rotate key 180° → π radians.
        assert_eq!(arm_track.rotation.len(), 2);
        assert!((arm_track.rotation[1].value - std::f32::consts::PI).abs() < 1e-5);
    }

    #[test]
    fn parse_spine_accepts_legacy_angle_field() {
        let json = r#"{
            "bones": [{ "name": "root" }],
            "animations": {
                "spin": {
                    "bones": {
                        "root": { "rotate": [{ "time": 0, "angle": 360 }] }
                    }
                }
            }
        }"#;

        let (_, clips) = parse_spine_json(json).expect("spine parse");
        let rot = &clips[0].tracks[0].rotation;
        assert!((rot[0].value - std::f32::consts::TAU).abs() < 1e-5);
    }

    #[test]
    fn spawn_rig_falls_back_to_first_bone_when_no_parentless_bone() {
        // Cycle: every bone claims a parent. Implementation should
        // still pick a deterministic root rather than panic.
        let rig = SkeletonRig {
            name: "loop".into(),
            bones: vec![
                BoneDef {
                    name: "a".into(),
                    parent: Some("b".into()),
                    position: [0.0, 0.0],
                    rotation: 0.0,
                    scale: [1.0, 1.0],
                    length: 1.0,
                },
                BoneDef {
                    name: "b".into(),
                    parent: Some("a".into()),
                    position: [0.0, 0.0],
                    rotation: 0.0,
                    scale: [1.0, 1.0],
                    length: 1.0,
                },
            ],
        };
        let mut world = World::new();
        let map = spawn_rig(&mut world, &rig);
        // First bone listed is promoted to root.
        assert!(world.get::<&SkeletonRoot>(map["a"]).is_ok());
    }
}
