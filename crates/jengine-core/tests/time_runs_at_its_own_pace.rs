//! **Time runs at its own pace** (Oct 2026; `time::world_scale`,
//! `ecs::time_scale`): with the world slowed to a quarter, a thing on the
//! world's time crawls, one with `time_scale.ignore_world` keeps real time
//! and one at `scale` 2 runs at half - and audio set to follow time bends
//! its music and one-shots down with it, and back. Found by the showcase's
//! Bullet Time. One test: the time scale is the process's.

use std::collections::HashMap;

use hecs::World;
use jengine_core::audio::music::{self, MusicOut};
use jengine_core::ecs::behaviors::tick_motion;
use jengine_core::ecs::components::{Transform, Velocity2D};
use jengine_core::ecs::time_scale::{TimeScale, factor};
use jengine_core::time;
use serde_json::json;

#[derive(Default)]
struct Out {
    pitch: HashMap<u64, f32>,
    next: u64,
}

impl MusicOut for Out {
    fn start(&mut self, _clip: &str, _volume: f32, _looping: bool, pitch: f32, _offset: f64) -> Option<u64> {
        self.next += 1;
        self.pitch.insert(self.next, pitch);
        Some(self.next)
    }
    fn set_volume(&mut self, _id: u64, _volume: f32) {}
    fn set_pitch(&mut self, id: u64, pitch: f32) {
        self.pitch.insert(id, pitch);
    }
    fn seek(&mut self, _id: u64, _seconds: f64) {}
    fn stop(&mut self, _id: u64) {}
    fn pause(&mut self, _id: u64) {}
    fn resume(&mut self, _id: u64) {}
    fn position(&self, _id: u64) -> Option<f64> {
        Some(0.0)
    }
    fn duration(&self, _clip: &str) -> Option<f64> {
        Some(60.0)
    }
    fn is_playing(&self, _id: u64) -> bool {
        true
    }
}

fn x(world: &World, e: hecs::Entity) -> f32 {
    world.get::<&Transform>(e).unwrap().position.x
}

#[test]
fn the_world_slows_the_hero_does_not_and_the_music_bends() {
    assert_eq!(time::world_scale(), 1.0, "the world runs at its own pace until told");
    let mut world = World::new();
    let walker = || (Transform::default(), Velocity2D { x: 100.0, ..Default::default() });
    let crowd = world.spawn(walker());
    let hero = world.spawn((walker().0, walker().1, TimeScale { ignore_world: true, ..Default::default() }));
    let quick = world.spawn((walker().0, walker().1, TimeScale { scale: 2.0, ..Default::default() }));

    // A second at a quarter speed: the host hands the world a quarter of
    // each frame.
    time::set_world_scale(0.25);
    for _ in 0..60 {
        tick_motion(&mut world, 0.25 / 60.0, None, false);
    }
    assert!((x(&world, crowd) - 25.0).abs() < 0.01, "the world crawls: {}", x(&world, crowd));
    assert!((x(&world, hero) - 100.0).abs() < 0.01, "the hero keeps real time: {}", x(&world, hero));
    assert!((x(&world, quick) - 50.0).abs() < 0.01, "twice the world's pace: {}", x(&world, quick));
    // A pause stops everything, the hero too.
    time::set_world_scale(0.0);
    assert_eq!(factor(Some(&TimeScale { ignore_world: true, ..Default::default() })), 0.0);

    // Music and one-shots, at half speed.
    time::set_world_scale(0.5);
    music::script_op("crossfade", &[json!("music/theme.ogg"), json!(0.0)]);
    let mut out = Out::default();
    music::drive_at(0.0, &mut out);
    assert!(!out.pitch.is_empty(), "the music started");
    assert!(out.pitch.values().all(|p| (*p - 1.0).abs() < 1e-6), "not following time: as written {:?}", out.pitch);
    assert_eq!(jengine_core::audio::variation::apply("sfx/shot.wav", 1.0, 1.0), (1.0, 1.0));

    time::set_audio_follows_time(true);
    music::drive_at(0.05, &mut out);
    assert!(out.pitch.values().all(|p| (*p - 0.5).abs() < 1e-6), "the music bends down with time: {:?}", out.pitch);
    assert_eq!(jengine_core::audio::variation::apply("sfx/shot.wav", 1.0, 1.0), (0.5, 1.0), "and a shot fired now");
    // A pause with audio following time slows it to a crawl, not silence.
    time::set_world_scale(0.0);
    music::drive_at(0.1, &mut out);
    assert!(out.pitch.values().all(|p| *p > 0.0), "never a rate of zero: {:?}", out.pitch);

    time::set_audio_follows_time(false);
    time::set_world_scale(1.0);
    music::drive_at(0.15, &mut out);
    assert!(out.pitch.values().all(|p| (*p - 1.0).abs() < 1e-6), "and back: {:?}", out.pitch);
}
