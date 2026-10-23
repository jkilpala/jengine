//! A frame that fires a trigger reaches something.
//!
//! `AnimationFrame::triggers` has been authorable in the editor's animation
//! editor all along, and `tick_sprite_animations` has always returned what
//! fired - but every runtime wrote `let _sprite_events = ...` and threw them
//! away. So a sound placed on a frame was silent, and `on_animation_event`
//! was a handler name the metadata accepted, the subscription index tracked,
//! and nothing in the engine ever called.
//!
//! That matters most for frame data: "the hitbox is live on frames 4 to 7"
//! is how a fighting game is written, and it needs the frame to say so.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::animation::{
    AnimationClip, AnimationFrame, FrameTrigger, SpriteAnimator, split_triggers,
};
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::{
    CustomComponents, Name, ScriptComponent, ScriptLanguage, SpriteComponent, Transform,
};
use jengine_core::ecs::systems::{ScriptSystem, tick_sprite_animations};
use jengine_core::input::InputState;

const SRC: &str = r#"
script = { events = { "on_animation_event" } }

function update(entity, dt)
end

function on_animation_event(entity, ev)
    set_custom(entity, "Probe", "last", ev.name)
    set_custom(entity, "Probe", "count", (get_custom(entity, "Probe", "count") or 0) + 1)
    if ev.data ~= nil then
        set_custom(entity, "Probe", "frame", ev.data.frame)
    end
end
"#;

fn frame(triggers: Vec<FrameTrigger>) -> AnimationFrame {
    AnimationFrame { uv_rect: [0.0, 0.0, 1.0, 1.0], region: None, texture_id: None, triggers }
}

/// Three frames: nothing, a sound, an event carrying data.
fn punch() -> SpriteAnimator {
    let clip = AnimationClip {
        name: "punch".into(),
        frames: vec![
            frame(vec![]),
            frame(vec![FrameTrigger::PlaySound { clip_id: "sounds/hit.wav".into(), volume: 0.8 }]),
            frame(vec![FrameTrigger::EmitEvent {
                event_name: "hitbox_on".into(),
                data: Some(serde_json::json!({ "frame": 2 })),
            }]),
        ],
        frame_duration: 0.1,
        looping: false,
        property_tracks: vec![],
    };
    let mut animator = SpriteAnimator::default();
    animator.clips.insert("punch".into(), clip);
    animator.play("punch");
    animator
}

#[test]
fn the_split_separates_sounds_from_events() {
    let mut world = World::new();
    world.spawn((Transform::default(), SpriteComponent::default(), punch()));
    let assets = AssetManager::new();

    // Frame 0 fires nothing.
    let fired = tick_sprite_animations(&mut world, &assets, 0.05);
    assert!(fired.is_empty(), "{fired:?}");

    // Frame 1: the sound.
    let fired = tick_sprite_animations(&mut world, &assets, 0.1);
    let (sounds, events) = split_triggers(&fired);
    assert_eq!(sounds.len(), 1, "{fired:?}");
    assert_eq!(sounds[0].1, "sounds/hit.wav");
    assert!((sounds[0].2 - 0.8).abs() < 1e-6);
    assert!(events.is_empty());

    // Frame 2: the event, with its data.
    let fired = tick_sprite_animations(&mut world, &assets, 0.1);
    let (sounds, events) = split_triggers(&fired);
    assert!(sounds.is_empty());
    assert_eq!(events.len(), 1, "{fired:?}");
    assert_eq!(events[0].1, "hitbox_on");
    assert_eq!(events[0].2.as_ref().and_then(|d| d.get("frame")).and_then(|f| f.as_i64()), Some(2));
}

/// A frame's event is also an ordinary engine event, so a controller script
/// on *another* entity can hear it with `on_event`. Most games animate their
/// sprites and keep their logic on one entity; without this the trigger
/// would only ever reach a script sitting on the animated entity itself.
#[test]
fn a_frames_event_also_reaches_listeners_on_other_entities() {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_animation_events_test",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let watcher_path = dir.join("watcher.lua");
    std::fs::write(
        &watcher_path,
        r#"
function init(entity)
    -- The handler is looked up by convention: on_<event_name>.
    on_event("hitbox_on", entity)
end

function update(entity, dt)
end

function on_hitbox_on(entity, data)
    set_custom(entity, "Probe", "heard", (get_custom(entity, "Probe", "heard") or 0) + 1)
    if data ~= nil and data.frame ~= nil then
        set_custom(entity, "Probe", "frame", data.frame)
    end
end
"#,
    )
    .unwrap();

    let mut world = World::new();
    // The clip is on one entity; the script that cares is on another.
    world.spawn((Name("Puppet".into()), Transform::default(), SpriteComponent::default(), punch()));
    let watcher = world.spawn((
        Name("Controller".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: watcher_path.to_string_lossy().into(),
            cull_distance: None,
        },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let assets = AssetManager::new();
    let input = InputState::new();

    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    for step in 0..3 {
        let fired = tick_sprite_animations(&mut world, &assets, if step == 0 { 0.05 } else { 0.1 });
        system.queue_animation_events(&fired);
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    system.run(&mut world, 1.0 / 60.0, &input, None, None);

    let custom = world.get::<&CustomComponents>(watcher).unwrap();
    let heard = custom.0.get("Probe").and_then(|p| p.get("heard")).and_then(|v| v.as_f64());
    assert_eq!(heard, Some(1.0), "a listener on another entity heard the frame's event");
    let frame = custom.0.get("Probe").and_then(|p| p.get("frame")).and_then(|v| v.as_f64());
    assert_eq!(frame, Some(2.0), "and the frame's data came with it");
}

#[test]
fn a_frames_event_reaches_the_scripts_handler() {
    let dir = std::env::temp_dir().join("jengine_animation_events_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fighter.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let e = world.spawn((
        Name("Fighter".into()),
        Transform::default(),
        SpriteComponent::default(),
        CustomComponents(Default::default()),
        punch(),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_string_lossy().into(),
            cull_distance: None,
        },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let assets = AssetManager::new();
    let input = InputState::new();
    let read = |world: &World, field: &str| -> Option<serde_json::Value> {
        let custom = world.get::<&CustomComponents>(e).unwrap();
        custom.0.get("Probe").and_then(|p| p.get(field)).cloned()
    };

    // Let the script load, then run the clip to the frame that fires.
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    for step in 0..3 {
        let fired = tick_sprite_animations(&mut world, &assets, if step == 0 { 0.05 } else { 0.1 });
        system.queue_animation_events(&fired);
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }

    // One more frame: a handler's commands are collected during dispatch and
    // applied with the next run's, the same as a collision handler's.
    system.run(&mut world, 1.0 / 60.0, &input, None, None);

    assert_eq!(read(&world, "last").and_then(|v| v.as_str().map(String::from)).as_deref(), Some("hitbox_on"));
    assert_eq!(read(&world, "count").and_then(|v| v.as_f64()), Some(1.0), "fired exactly once");
    assert_eq!(read(&world, "frame").and_then(|v| v.as_f64()), Some(2.0), "the frame's data came with it");

    // The sound on frame 1 is the runtime's to play, and must not have been
    // handed to the script as an event.
    assert_eq!(read(&world, "count").and_then(|v| v.as_f64()), Some(1.0));
}
