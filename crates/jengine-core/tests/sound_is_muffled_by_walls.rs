//! **Walls muffle sound, and a room has its own reverb** (Oct 2026;
//! `audio::spatial`, `audio::zones`). A spatial `audio_source` with
//! `occlusion` loses that share of its volume to each wall between it and the
//! listener, and with `muffle` is low-passed behind them; an `audio_listener`
//! with `room_reverb` gives that bus a reverb from how enclosed it is. Found
//! by the showcase's Muffled Halls.

use glam::Vec2;
use hecs::World;
use jengine_core::audio::spatial::{AudioListener, SpatialAudio, occlude};
use jengine_core::audio::zones::room_effects;
use jengine_core::ecs::components::{AudioSource, Collider2D, ColliderShape, Transform};

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

fn wall(world: &mut World, x: f32, y: f32, w: f32, h: f32) -> hecs::Entity {
    world.spawn((at(x, y), Collider2D { shape: ColliderShape::Box { width: w, height: h }, ..Default::default() }))
}

/// A listener at the origin and a muffled source 600 to its right; returns
/// what the source plays at this frame.
fn hear(world: &mut World) -> (f32, f32) {
    let src = world.spawn((
        at(600.0, 0.0),
        AudioSource {
            clip_id: "lute.wav".into(),
            spatial: true,
            min_distance: 1000.0,
            max_distance: 2000.0,
            occlusion: 0.5,
            muffle: 800.0,
            ..Default::default()
        },
    ));
    let mut spatial = SpatialAudio::new();
    spatial.started(src, 7, "lute.wav");
    let frame = spatial.update(world, |_| true);
    let m = frame.mix[0];
    (m.volume, m.cutoff)
}

#[test]
fn each_wall_takes_volume_and_lowers_the_cutoff() {
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), AudioListener::default(), Collider2D::default()));
    assert_eq!(hear(&mut world), (1.0, 0.0), "nothing between them: full and open");

    let mut world = World::new();
    world.spawn((at(0.0, 0.0), AudioListener::default()));
    wall(&mut world, 300.0, 0.0, 40.0, 400.0);
    let (v, c) = hear(&mut world);
    assert!((v - 0.5).abs() < 1e-5 && (c - 800.0).abs() < 1e-3, "one wall: half, 800 Hz: {v} {c}");

    let mut world = World::new();
    world.spawn((at(0.0, 0.0), AudioListener::default()));
    wall(&mut world, 200.0, 0.0, 40.0, 400.0);
    wall(&mut world, 400.0, 0.0, 40.0, 400.0);
    let (v, c) = hear(&mut world);
    assert!((v - 0.25).abs() < 1e-5 && (c - 400.0).abs() < 1e-3, "two walls: a quarter, 400 Hz: {v} {c}");

    // A wall with a doorway the line passes through: no wall.
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), AudioListener::default()));
    wall(&mut world, 300.0, 250.0, 40.0, 400.0);
    wall(&mut world, 300.0, -250.0, 40.0, 400.0);
    assert_eq!(hear(&mut world), (1.0, 0.0), "through the doorway");
    assert_eq!(occlude(3, 0.0, 1000.0), (1.0, 250.0));
}

#[test]
fn a_closed_room_has_a_reverb_and_the_open_has_none() {
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), AudioListener { room_reverb: "sfx".into(), ..Default::default() }));
    assert!(room_effects(&world).is_none(), "the open: no walls in reach");
    // A hall 800 by 600, walled all round.
    wall(&mut world, -400.0, 0.0, 40.0, 640.0);
    wall(&mut world, 400.0, 0.0, 40.0, 640.0);
    wall(&mut world, 0.0, 300.0, 840.0, 40.0);
    wall(&mut world, 0.0, -300.0, 840.0, 40.0);
    let (bus, hall) = room_effects(&world).expect("a room");
    assert_eq!(bus, "sfx");
    assert!((hall.reverb - 0.5).abs() < 1e-3, "closed all round: the whole mix: {}", hall.reverb);
    // A closet: the same walls, the room smaller.
    let mut small = World::new();
    small.spawn((at(0.0, 0.0), AudioListener { room_reverb: "sfx".into(), ..Default::default() }));
    wall(&mut small, -100.0, 0.0, 20.0, 220.0);
    wall(&mut small, 100.0, 0.0, 20.0, 220.0);
    wall(&mut small, 0.0, 100.0, 220.0, 20.0);
    wall(&mut small, 0.0, -100.0, 220.0, 20.0);
    let (_, closet) = room_effects(&small).unwrap();
    assert!(closet.room < hall.room, "a closet is a smaller room: {} < {}", closet.room, hall.room);
}
