//! `tick_sprite_animations` against a real world.
//!
//! UI property tracks were authorable in the animation editor and broken two
//! ways: an animator on a UI entity with no sprite was never ticked, so its
//! clip sat on frame 0; and a separate UI pass re-applied the tracks every
//! frame whether the clip was playing or not, so once a fade finished no script
//! could change that property again.

use hecs::World;
use jengine_core::animation::{
    AnimatableProperty, AnimationClip, AnimationFrame, Keyframe, PropertyTrack, SpriteAnimator,
};
use jengine_core::ecs::components::{SpriteComponent, Transform};
use jengine_core::ecs::systems::tick_sprite_animations;
use jengine_core::asset::AssetManager;
use jengine_core::ui::{UIElement, UIPanel};

fn assets() -> AssetManager {
    AssetManager::new()
}

/// A 3-frame, 0.1 s/frame, non-looping clip fading panel alpha 0 -> 1.
fn fade_in() -> SpriteAnimator {
    let frame = AnimationFrame { uv_rect: [0.0, 0.0, 1.0, 1.0], region: None, texture_id: None, triggers: Vec::new() };
    let clip = AnimationClip {
        name: "fade".into(),
        frames: vec![frame.clone(), frame.clone(), frame],
        frame_duration: 0.1,
        looping: false,
        property_tracks: vec![PropertyTrack {
            property: AnimatableProperty::UiPanelColorA,
            keyframes: vec![Keyframe { frame: 0, value: 0.0 }, Keyframe { frame: 2, value: 1.0 }],
        }],
    };
    let mut animator = SpriteAnimator::default();
    animator.clips.insert("fade".into(), clip);
    animator.play("fade");
    animator
}

fn alpha(world: &World, e: hecs::Entity) -> f32 {
    world.get::<&UIPanel>(e).unwrap().bg_color[3]
}

#[test]
fn a_ui_only_animator_advances() {
    let mut world = World::new();
    // No Transform, no SpriteComponent - just the widget.
    let panel = world.spawn((UIElement::default(), UIPanel::default(), fade_in()));

    tick_sprite_animations(&mut world, &assets(), 0.1 + 1e-4);
    let a = alpha(&world, panel);
    assert!(a > 0.4 && a < 0.6, "half way through the fade, got {a}");

    tick_sprite_animations(&mut world, &assets(), 0.1);
    assert!((alpha(&world, panel) - 1.0).abs() < 1e-3);
    assert_eq!(world.get::<&SpriteAnimator>(panel).unwrap().current_frame, 2);
}

#[test]
fn a_finished_clip_stops_writing_its_property() {
    let mut world = World::new();
    let panel = world.spawn((UIElement::default(), UIPanel::default(), fade_in()));
    for _ in 0..6 {
        tick_sprite_animations(&mut world, &assets(), 0.1);
    }
    assert!(!world.get::<&SpriteAnimator>(panel).unwrap().is_playing());

    // A script hides the panel after the fade.
    world.get::<&mut UIPanel>(panel).unwrap().bg_color[3] = 0.0;
    tick_sprite_animations(&mut world, &assets(), 0.1);
    assert_eq!(alpha(&world, panel), 0.0, "the stopped clip wrote its last value back");
}

#[test]
fn sprite_entities_still_get_uv_and_transform_tracks() {
    let mut world = World::new();
    let mut animator = fade_in();
    let clip = animator.clips.get_mut("fade").unwrap();
    clip.frames[1].uv_rect = [0.5, 0.0, 1.0, 1.0];
    clip.property_tracks.push(PropertyTrack {
        property: AnimatableProperty::PositionX,
        keyframes: vec![Keyframe { frame: 0, value: 0.0 }, Keyframe { frame: 2, value: 100.0 }],
    });
    let e = world.spawn((Transform::default(), SpriteComponent::default(), animator));

    tick_sprite_animations(&mut world, &assets(), 0.1 + 1e-4);
    assert_eq!(world.get::<&SpriteComponent>(e).unwrap().uv_rect, Some([0.5, 0.0, 1.0, 1.0]));
    let x = world.get::<&Transform>(e).unwrap().position.x;
    assert!(x > 40.0 && x < 60.0, "got {x}");
}

#[test]
fn a_sprite_animates_through_named_atlas_regions() {
    // Frames name regions instead of carrying coordinates; the system fills
    // them in from the atlas registered under the sprite's texture.
    let mut assets = AssetManager::new();
    let atlas: jengine_core::renderer::atlas::AtlasData = serde_json::from_value(serde_json::json!({
        "width": 128, "height": 32,
        "regions": [
            { "name": "walk_0", "x": 0,  "y": 0, "width": 32, "height": 32 },
            { "name": "walk_1", "x": 64, "y": 0, "width": 32, "height": 32 }
        ]
    }))
    .unwrap();
    assets.insert_atlas_data("sprites/sheet.png".into(), atlas);

    let clip: AnimationClip = serde_json::from_value(serde_json::json!({
        "name": "walk", "frame_duration": 0.1, "looping": true,
        "frames": [ { "region": "walk_0" }, { "region": "walk_1" } ]
    }))
    .unwrap();
    let mut animator = SpriteAnimator::default();
    animator.clips.insert("walk".into(), clip);
    animator.play("walk");

    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "sprites/sheet.png".into(), ..Default::default() },
        animator,
    ));

    tick_sprite_animations(&mut world, &assets, 0.01);
    assert_eq!(world.get::<&SpriteComponent>(e).unwrap().uv_rect, Some([0.0, 0.0, 0.25, 1.0]));
    tick_sprite_animations(&mut world, &assets, 0.1);
    assert_eq!(world.get::<&SpriteComponent>(e).unwrap().uv_rect, Some([0.5, 0.0, 0.75, 1.0]));
}

#[test]
fn a_sprite_can_name_its_atlas_region() {
    use jengine_core::ecs::systems::resolve_sprite_regions;

    let mut assets = AssetManager::new();
    let atlas: jengine_core::renderer::atlas::AtlasData = serde_json::from_value(serde_json::json!({
        "width": 64, "height": 64,
        "regions": [ { "name": "idle_0", "x": 32, "y": 0, "width": 32, "height": 32 } ]
    }))
    .unwrap();

    let mut world = World::new();
    let sprite: SpriteComponent = serde_json::from_value(serde_json::json!({
        "texture_id": "sprites/sheet.png", "region": "idle_0",
        "color": [1.0, 1.0, 1.0, 1.0], "z_order": 0
    }))
    .unwrap();
    let e = world.spawn((Transform::default(), sprite));

    // The atlas is not loaded yet: the sprite waits rather than resolving wrong.
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(world.get::<&SpriteComponent>(e).unwrap().uv_rect, None);

    assets.insert_atlas_data("sprites/sheet.png".into(), atlas);
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(world.get::<&SpriteComponent>(e).unwrap().uv_rect, Some([0.5, 0.0, 1.0, 0.5]));

    // Resolved once: a later uv_rect write is not undone.
    world.get::<&mut SpriteComponent>(e).unwrap().uv_rect = Some([0.0, 0.0, 1.0, 1.0]);
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(world.get::<&SpriteComponent>(e).unwrap().uv_rect, Some([0.0, 0.0, 1.0, 1.0]));
}

#[test]
fn an_unknown_sprite_region_leaves_the_authored_rect() {
    use jengine_core::ecs::systems::resolve_sprite_regions;

    let mut assets = AssetManager::new();
    let atlas: jengine_core::renderer::atlas::AtlasData = serde_json::from_value(serde_json::json!({
        "width": 32, "height": 32, "regions": []
    }))
    .unwrap();
    assets.insert_atlas_data("sprites/sheet.png".into(), atlas);

    let mut world = World::new();
    let sprite = SpriteComponent {
        texture_id: "sprites/sheet.png".into(),
        region: Some("nope".into()),
        uv_rect: Some([0.25, 0.25, 0.5, 0.5]),
        ..Default::default()
    };
    let e = world.spawn((Transform::default(), sprite));
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(world.get::<&SpriteComponent>(e).unwrap().uv_rect, Some([0.25, 0.25, 0.5, 0.5]));
}
