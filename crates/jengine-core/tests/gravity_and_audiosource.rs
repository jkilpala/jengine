//! `Gravity2D` and `AudioSource` — two components that deserialized, appeared
//! in the editor's component list, and were read by nothing.
//!
//! `Gravity2D`'s only reader was `parallel::update_gravity_par`, which no
//! runtime calls and which is rayon-gated to native. `AudioSource` had no
//! reader at all: audio was script-driven in every runtime.

use hecs::World;
use jengine_core::ecs::behaviors::{collect_autoplay_audio, update_gravity};
use jengine_core::ecs::components::{AudioSource, Gravity2D, Velocity2D};

#[test]
fn gravity_accumulates_into_velocity() {
    let mut world = World::new();
    let e = world.spawn((
        Velocity2D { x: 0.0, y: 0.0, ..Default::default() },
        Gravity2D { x: 10.0, y: -20.0 },
    ));

    update_gravity(&mut world, 0.5);

    let v = world.get::<&Velocity2D>(e).unwrap();
    assert!((v.x - 5.0).abs() < 1e-5, "vx = {}", v.x);
    assert!((v.y + 10.0).abs() < 1e-5, "vy = {}", v.y);
}

#[test]
fn gravity_is_a_vector_and_composes_over_frames() {
    // The point of Gravity2D over Velocity2D.gravity: it has a direction.
    let mut world = World::new();
    let e = world.spawn((
        Velocity2D { x: 0.0, y: 0.0, ..Default::default() },
        Gravity2D { x: -3.0, y: 0.0 },
    ));
    for _ in 0..4 {
        update_gravity(&mut world, 0.25);
    }
    let v = world.get::<&Velocity2D>(e).unwrap();
    assert!((v.x + 3.0).abs() < 1e-5, "four quarter-seconds of -3 = -3, got {}", v.x);
    assert_eq!(v.y, 0.0, "a sideways gravity must not pull down");
}

#[test]
fn autoplay_fires_once_and_only_for_auto_play_sources() {
    let mut world = World::new();
    world.spawn((AudioSource {
        clip_id: "sounds/hum.wav".into(),
        volume: 0.5,
        pitch: 1.0,
        looping: true,
        auto_play: true,
        started: false,
        bus_id: None,
        ..Default::default()
    },));
    // auto_play off — the entity carries a sound something else will trigger.
    world.spawn((AudioSource {
        clip_id: "sounds/blip.wav".into(),
        auto_play: false,
        ..Default::default()
    },));
    // auto_play on but no clip: nothing to play, and it must not be requested.
    world.spawn((AudioSource { clip_id: String::new(), auto_play: true, ..Default::default() },));

    let first = collect_autoplay_audio(&mut world);
    assert_eq!(first.len(), 1, "only the armed source with a clip should fire");
    assert_eq!(first[0].clip_id, "sounds/hum.wav");
    assert!(first[0].looping);
    assert!((first[0].volume - 0.5).abs() < 1e-6);

    // Exactly once: the second frame must be silent, or an autoplay source
    // would retrigger every frame forever.
    assert!(
        collect_autoplay_audio(&mut world).is_empty(),
        "autoplay must not repeat on later frames"
    );
}

/// A browser decodes clips asynchronously, so a scene's first frame can find
/// its ambience not ready. The host hands the source back
/// (`retry_autoplay`) and it is collected again next frame - it used to be
/// marked started before the play was tried, and the sound was lost.
#[test]
fn a_source_handed_back_is_collected_again() {
    let mut world = World::new();
    let e = world.spawn((AudioSource { clip_id: "sounds/hum.wav".into(), auto_play: true, ..Default::default() },));
    assert_eq!(collect_autoplay_audio(&mut world).len(), 1);
    jengine_core::ecs::behaviors::retry_autoplay(&mut world, e);
    let again = collect_autoplay_audio(&mut world);
    assert_eq!(again.len(), 1, "a source handed back is tried again");
    assert_eq!(again[0].entity, e);
    assert!(collect_autoplay_audio(&mut world).is_empty(), "and then not again");
}

#[test]
fn started_is_runtime_state_and_never_serialized() {
    // A scene that round-trips a started source must come back re-armed,
    // otherwise reloading a scene would leave its ambience silent.
    let src = AudioSource { clip_id: "a.wav".into(), auto_play: true, started: true, ..Default::default() };
    let json = serde_json::to_string(&src).unwrap();
    assert!(!json.contains("started"), "started leaked into the scene format: {json}");
    let back: AudioSource = serde_json::from_str(&json).unwrap();
    assert!(!back.started);
}
