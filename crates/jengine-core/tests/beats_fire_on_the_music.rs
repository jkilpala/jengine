//! **Beat events** (Oct 2026; `ecs::beat_clock`, `BeatClock.set`): a
//! `beat_clock` turns the music's tempo into `on_beat` / `on_bar` callbacks
//! on its own script, timed by the music's playhead - which a host publishes
//! a frame late and in buffer steps, so the clock runs on `dt` and is pulled
//! toward it. It is ticked inside `tick_motion`, the step every host makes,
//! and its callbacks go out through `ecs::callbacks`, so no host was wired.
//! Found by the showcase's Beat Hall.
//!
//! The music's playhead is process-wide, so every test that sets it is in
//! this one function's binary, run one after another.
#![cfg(feature = "beat_clock")]

use hecs::World;
use jengine_core::ecs::beat_clock::BeatClock;
use jengine_core::ecs::components::CustomComponents;

fn read(world: &World, e: hecs::Entity, field: &str) -> f64 {
    world.get::<&CustomComponents>(e).unwrap().0["BeatClock"][field].as_f64().unwrap()
}

/// Fire times of each handler, in seconds of true music time.
fn run(world: &mut World, seconds: f64, playhead: impl Fn(f64) -> Option<f64>) -> Vec<(f64, &'static str)> {
    let mut fired = Vec::new();
    let mut t = 0.0;
    let mut frame = 0;
    while t < seconds {
        // An uneven frame: 15 and 18 ms in turn, as a real display hands out.
        let dt = if frame % 2 == 0 { 0.015 } else { 0.018 };
        t += dt;
        frame += 1;
        // The device reports in 10 ms buffer steps, a frame late.
        jengine_core::audio::publish_music_position(playhead(t - dt).map(|p| (p / 0.01).floor() * 0.01));
        jengine_core::ecs::behaviors::tick_motion(world, dt as f32, None, false);
        for c in jengine_core::ecs::callbacks::take() {
            fired.push((t, c.handler));
        }
    }
    fired
}

#[test]
fn beats_and_bars_fire_on_the_music() {
    // 120 bpm, four to a bar: a beat every half second - of the music,
    // which starts 0.3 s late, so a clock that ran on frames alone would
    // fire every beat 0.3 s early.
    let mut world = World::new();
    let clock = world.spawn((BeatClock { bpm: 120.0, beats_per_bar: 4, ..Default::default() },));
    let fired = run(&mut world, 4.5, |t| (t >= 0.3).then(|| t - 0.3));
    let beats: Vec<f64> = fired.iter().filter(|f| f.1 == "on_beat").map(|f| f.0).collect();
    let bars: Vec<f64> = fired.iter().filter(|f| f.1 == "on_bar").map(|f| f.0).collect();
    assert_eq!(beats.len(), 8, "eight beats by 4.5 s (the first downbeat is where it starts): {beats:?}");
    for (k, at) in beats.iter().enumerate() {
        let due = 0.3 + 0.5 * (k + 1) as f64;
        // Within a couple of frames: the playhead arrives late and coarse.
        assert!((at - due).abs() < 0.05, "beat {} fired at {at:.3}, due {due:.3}", k + 1);
    }
    assert_eq!(bars.len(), 2, "a bar on beats 4 and 8: {bars:?}");
    assert!((bars[0] - 2.3).abs() < 0.05 && (bars[1] - 4.3).abs() < 0.05, "{bars:?}");
    assert_eq!(read(&world, clock, "count"), 8.0);
    assert_eq!(read(&world, clock, "bar"), 2.0);
    assert_eq!(read(&world, clock, "beat_in_bar"), 0.0);
    let phase = read(&world, clock, "phase");
    assert!((0.0..1.0).contains(&phase), "{phase}");

    // A one-bar loop: the playhead jumps back to 0 every 2 s, and the beat
    // with it - so it is a downbeat each time, and nothing is skipped or
    // doubled.
    let mut world = World::new();
    world.spawn((BeatClock { bpm: 120.0, beats_per_bar: 4, ..Default::default() },));
    let fired = run(&mut world, 6.2, |t| Some(t % 2.0));
    let beats = fired.iter().filter(|f| f.1 == "on_beat").count();
    let bars: Vec<f64> = fired.iter().filter(|f| f.1 == "on_bar").map(|f| f.0).collect();
    assert_eq!(beats, 12, "{fired:?}");
    assert_eq!(bars.len(), 3, "a bar each time the loop comes round: {bars:?}");

    // No music: a metronome on dt alone, and `running` false holds it.
    jengine_core::audio::publish_music_position(None);
    let mut world = World::new();
    let metronome = world.spawn((BeatClock { bpm: 60.0, beats_per_bar: 3, follow_music: false, ..Default::default() },));
    let fired = run(&mut world, 3.1, |_| None);
    assert_eq!(fired.iter().filter(|f| f.1 == "on_beat").count(), 3);
    assert_eq!(fired.iter().filter(|f| f.1 == "on_bar").count(), 1, "the third beat starts bar 1");
    jengine_core::ecs::beat_clock::set_beat_clock_field(&world, metronome, "running", &serde_json::json!(false)).unwrap();
    assert!(run(&mut world, 2.0, |_| None).is_empty(), "a stopped clock fires nothing");
    // Setting the tempo keeps the clock where it is.
    jengine_core::ecs::beat_clock::set_beat_clock_field(&world, metronome, "bpm", &serde_json::json!(120.0)).unwrap();
    let c = world.get::<&BeatClock>(metronome).unwrap();
    assert_eq!(c.bpm, 120.0);
    assert!(c.state.time > 3.0, "{}", c.state.time);
}
