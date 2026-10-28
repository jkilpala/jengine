//! **Post volumes** (Oct 2026; `ecs::post_volume`): a box whose post-effect
//! values the camera takes on as it goes in, fading over `blend` outside -
//! so each room of a manor has its own grade with no script. The blend is
//! handed to the screen's stack as a copy each frame; the scene's settings,
//! and what scripts set on them, stay as they were. Found by the showcase's
//! Haunted Manor. The blending itself is tested in the module.
#![cfg(feature = "post_volume")]

mod support;

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::Transform;
use jengine_core::ecs::post_volume::{PostVolume, VolumeEffect, blended};
use jengine_core::renderer::material::UniformValue;
use jengine_core::renderer::postprocess::PostProcessSettings;

#[test]
fn every_host_blends_the_volumes_into_the_screens_chain() {
    for (name, src) in [
        ("native", support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs"))),
        ("editor", support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs"))),
        ("web", support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs"))),
    ] {
        assert!(src.contains("post_volume::blended("), "{name} never blends post volumes");
    }
}

#[test]
fn a_volume_from_a_scene_switches_on_an_effect_and_adds_exposure() {
    let volume: PostVolume = serde_json::from_value(serde_json::json!({
        "size": [400, 300], "blend": 100, "exposure": 1.6,
        "effects": [{ "name": "vignette", "enabled": true, "params": { "intensity": { "Float": 0.8 } } }]
    }))
    .unwrap();
    let mut world = World::new();
    world.spawn((Transform { position: Vec2::new(1000.0, 0.0), ..Default::default() }, volume));
    let base = PostProcessSettings { hdr: true, ..Default::default() };
    assert!(blended(&world, &base, Vec2::ZERO).is_none(), "the camera is in another room");
    let inside = blended(&world, &base, Vec2::new(1000.0, 0.0)).unwrap();
    assert!((inside.exposure() - 1.6).abs() < 1e-5);
    let v = inside.effects.iter().find(|e| e.name == "vignette").expect("added");
    assert!(v.enabled);
    assert!(matches!(v.params["intensity"], UniformValue::Float(x) if (x - 0.8).abs() < 1e-6));
    let _ = VolumeEffect::default();
}
