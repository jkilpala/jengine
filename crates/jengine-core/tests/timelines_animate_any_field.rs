//! Keyframe timelines (Oct 2026): a clip's tracks move any component's
//! fields, its event keys call the timeline entity's script, and it ends,
//! loops and switches clips as told - ticked by `tick_motion`, which every
//! host runs.
#![cfg(feature = "timeline")]

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::behaviors::tick_motion;
use jengine_core::ecs::callbacks;
use jengine_core::ecs::components::{Name, Parent, SpriteComponent, Transform};
use jengine_core::ecs::timeline::{Ease, Timeline, TimelineClip, Track, set_timeline_field};
use serde_json::json;

fn track(target: &str, component: &str, field: &str, keys: Vec<(f32, serde_json::Value)>) -> Track {
    Track { target: target.into(), component: component.into(), field: field.into(), keys, ease: Ease::Linear }
}

fn run(world: &mut World, seconds: f32) {
    let dt = 1.0 / 60.0;
    for _ in 0..(seconds / dt).round() as usize {
        tick_motion(world, dt, None, false);
    }
}

fn handlers(owner: Entity) -> Vec<&'static str> {
    callbacks::take().into_iter().filter(|c| c.entity == owner).map(|c| c.handler).collect()
}

fn stage() -> (World, Entity, Entity) {
    let mut world = World::new();
    let actor = world.spawn((
        Name("Actor".into()),
        Transform::default(),
        SpriteComponent { texture_id: "sprites/a.png".into(), ..Default::default() },
    ));
    let clip = TimelineClip {
        duration: 0.0,
        tracks: vec![
            track("Actor", "transform", "position", vec![(0.0, json!([0.0, 0.0])), (2.0, json!([200.0, 100.0]))]),
            track("Actor", "sprite", "color.3", vec![(0.0, json!(1.0)), (2.0, json!(0.0))]),
            track("Actor", "sprite", "texture_id", vec![(0.0, json!("sprites/a.png")), (1.0, json!("sprites/b.png"))]),
            track("Actor", "sprite", "z_order", vec![(0.0, json!(0)), (2.0, json!(10))]),
        ],
        events: vec![(0.0, "curtain_up".into()), (1.5, "bow".into())],
    };
    let mut tl = Timeline::default();
    tl.clips.insert("act1".into(), clip);
    let owner = world.spawn((tl,));
    (world, owner, actor)
}

#[test]
fn tracks_move_any_field_and_events_call_the_script() {
    let (mut world, owner, actor) = stage();
    let _ = callbacks::take();

    run(&mut world, 1.0);
    {
        let t = world.get::<&Transform>(actor).unwrap();
        assert!((t.position - Vec2::new(100.0, 50.0)).length() < 2.0, "{:?}", t.position);
        let s = world.get::<&SpriteComponent>(actor).unwrap();
        assert!((s.color[3] - 0.5).abs() < 0.02, "alpha {}", s.color[3]);
        assert_eq!(s.texture_id, "sprites/a.png", "a string holds until its key");
        assert_eq!(s.z_order, 5, "an integer field takes the rounded value");
    }
    assert_eq!(handlers(owner), vec!["on_curtain_up"], "the key at 0 fires on the first tick");
    // Sixty frames of a sixtieth fall a hair short of 1.0; a few more pass it.
    run(&mut world, 0.05);
    assert_eq!(world.get::<&SpriteComponent>(actor).unwrap().texture_id, "sprites/b.png", "then changes on it");

    run(&mut world, 1.5);
    let t = world.get::<&Transform>(actor).unwrap().position;
    assert_eq!(t, Vec2::new(200.0, 100.0), "it ends on its last key");
    assert_eq!(handlers(owner), vec!["on_bow", "on_timeline_end"]);
    let tl = world.get::<&Timeline>(owner).unwrap();
    assert!(!tl.playing && (tl.time - 2.0).abs() < 1e-4);
}

#[test]
fn a_looping_clip_starts_over_and_fires_again() {
    let (mut world, owner, actor) = stage();
    world.get::<&mut Timeline>(owner).unwrap().looping = true;
    let _ = callbacks::take();
    run(&mut world, 2.5);
    let fired = handlers(owner);
    assert_eq!(fired.iter().filter(|h| **h == "on_curtain_up").count(), 2, "{fired:?}");
    assert!(!fired.contains(&"on_timeline_end"));
    let x = world.get::<&Transform>(actor).unwrap().position.x;
    assert!((x - 50.0).abs() < 2.0, "half a second into the second time round: {x}");
}

#[test]
fn a_track_prefers_a_child_of_the_timeline() {
    let mut world = World::new();
    let elsewhere = world.spawn((Name("Lamp".into()), Transform::default()));
    let mut tl = Timeline::default();
    tl.clips.insert(
        "a".into(),
        TimelineClip {
            tracks: vec![track("Lamp", "transform", "rotation", vec![(0.0, json!(1.0)), (1.0, json!(1.0))])],
            ..Default::default()
        },
    );
    let owner = world.spawn((tl, Transform::default()));
    let mine = world.spawn((Name("Lamp".into()), Transform::default(), Parent(owner)));
    run(&mut world, 0.1);
    assert_eq!(world.get::<&Transform>(mine).unwrap().rotation, 1.0);
    assert_eq!(world.get::<&Transform>(elsewhere).unwrap().rotation, 0.0);
}

#[test]
fn scripts_switch_clips_and_seek() {
    use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
    struct Host(World);
    impl ScriptWorldHost for Host {
        fn world(&mut self) -> &mut World {
            &mut self.0
        }
        fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
            self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
        }
    }
    let (mut world, owner, actor) = stage();
    let mut tl = world.get::<&mut Timeline>(owner).unwrap().clone();
    tl.clips.insert(
        "act2".into(),
        TimelineClip {
            tracks: vec![track("Actor", "transform", "rotation", vec![(0.0, json!(0.0)), (1.0, json!(2.0))])],
            ..Default::default()
        },
    );
    *world.get::<&mut Timeline>(owner).unwrap() = tl;
    run(&mut world, 0.5);
    // Seeking does not fire the events it skips.
    let _ = callbacks::take();
    set_timeline_field(&world, owner, "time", &json!(1.9)).unwrap();
    run(&mut world, 0.05);
    assert!(!handlers(owner).contains(&"on_bow"));
    // A clip set by a script plays from its start (the browsers' shared arm).
    let mut host = Host(world);
    assert!(handle_world_command(&mut host, "stlf", &JsonArgs::new(json!({ "e": owner.id(), "f": "clip", "v": "act2" }))));
    run(&mut host.0, 0.5);
    let r = host.0.get::<&Transform>(actor).unwrap().rotation;
    assert!((r - 1.0).abs() < 0.05, "half way through act two: {r}");
    assert!(set_timeline_field(&host.0, owner, "clips_", &json!(1)).is_err(), "a misspelt field is an error");
}

#[test]
fn a_timeline_loads_from_a_scene() {
    let json = json!({
        "clips": { "open": {
            "tracks": [{ "target": "Curtain", "component": "transform", "field": "position.0",
                         "keys": [[0.0, -300.0], [2.0, -700.0]], "ease": "in_out" }],
            "events": [[2.0, "opened"]]
        } },
        "looping": false
    });
    let tl: Timeline = serde_json::from_value(json).unwrap();
    let (name, clip) = tl.current().unwrap();
    assert_eq!(name, "open");
    assert_eq!(clip.length(), 2.0);
    assert_eq!(clip.tracks[0].ease, Ease::InOut);
    assert!(tl.playing, "a timeline plays unless told not to");
}
