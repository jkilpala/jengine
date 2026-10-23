//! More bus effects (Oct 2026): an echo, a soft-clip distortion, a
//! compressor and a three-band EQ beside the low-pass and reverb every bus
//! already had - set by `Audio.set_bus_effect`, or by an `audio_zone` (an
//! echo for a canyon). The numbers each one turns into are worked out once,
//! on `BusEffects`, and read by both backends: kira's chain natively, the Web
//! Audio nodes in the browsers (whose distortion curve is `distort` itself).

use glam::Vec2;
use jengine_core::audio::zones::AudioZone;
use jengine_core::audio::{AudioMixer, BusEffects, SFX_BUS};

#[test]
fn every_new_effect_is_set_by_name_and_held_in_range() {
    let mut mixer = AudioMixer::new();
    for (name, value) in [
        ("echo", 0.6),
        ("echo_time", 0.5),
        ("echo_feedback", 0.7),
        ("distortion", 0.4),
        ("compress", 0.8),
        ("eq_low", 6.0),
        ("eq_mid", -3.0),
        ("eq_high", 9.0),
    ] {
        mixer.set_effect(SFX_BUS, name, value).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    let fx = mixer.effects(SFX_BUS);
    assert_eq!((fx.echo, fx.echo_time, fx.echo_feedback), (0.6, 0.5, 0.7));
    assert_eq!((fx.distortion, fx.compress), (0.4, 0.8));
    assert_eq!(fx.eq_gains(), [6.0, -3.0, 9.0]);

    mixer.set_effect(SFX_BUS, "echo_time", 9.0).unwrap();
    mixer.set_effect(SFX_BUS, "echo_feedback", 3.0).unwrap();
    mixer.set_effect(SFX_BUS, "eq_low", -90.0).unwrap();
    let fx = mixer.effects(SFX_BUS);
    assert_eq!(fx.echo_delay(), BusEffects::MAX_ECHO_TIME, "no longer than a delay line holds");
    assert!(fx.echo_return() <= 0.95, "an echo that returned whole would ring for ever");
    assert_eq!(fx.eq_low, -BusEffects::EQ_RANGE);
    let err = mixer.set_effect(SFX_BUS, "flanger", 1.0).unwrap_err();
    assert!(err.contains("echo") && err.contains("eq_high"), "the refusal names what there is: {err}");
}

#[test]
fn a_bus_with_no_new_effects_is_left_as_it_was() {
    let fx = BusEffects::default();
    assert_eq!(fx.echo_gains(), (1.0, 0.0), "no echo: the dry signal whole");
    for x in [-1.0, -0.3, 0.0, 0.25, 0.9] {
        assert!((fx.distort(x) - x).abs() < 1e-6, "no distortion bends nothing");
    }
    let c = fx.compressor();
    assert_eq!((c.threshold_db, c.ratio, c.makeup_db), (0.0, 1.0, 0.0), "no compression is unity");
    assert_eq!(fx.eq_gains(), [0.0; 3]);
}

#[test]
fn distortion_crackles_and_keeps_full_scale() {
    let fx = BusEffects { distortion: 1.0, ..Default::default() };
    let curve: Vec<f32> = (0..=40).map(|i| fx.distort(i as f32 / 20.0 - 1.0)).collect();
    assert!(curve.windows(2).all(|w| w[1] >= w[0]), "it never folds back");
    assert!((fx.distort(0.5) + fx.distort(-0.5)).abs() < 1e-6, "it is symmetric");
    assert!((fx.distort(1.0) - 1.0).abs() < 1e-5, "full scale stays full scale");
    // Fully driven, a loud and a quiet sample come out much closer than
    // they went in - that is the crackle.
    assert!(fx.distort(1.0) / fx.distort(0.1) < 1.5);
    for a in [0.2, 0.5, 0.8] {
        let fx = BusEffects { distortion: a, ..Default::default() };
        assert!((-20..=20).all(|i| fx.distort(i as f32 / 20.0).abs() <= 1.0 + 1e-6), "never past full scale at {a}");
    }
}

#[test]
fn compression_lowers_the_threshold_and_makes_some_back() {
    let half = BusEffects { compress: 0.5, ..Default::default() }.compressor();
    let full = BusEffects { compress: 1.0, ..Default::default() }.compressor();
    assert_eq!((full.threshold_db, full.ratio), (-30.0, 8.0));
    assert!(half.threshold_db > full.threshold_db && half.ratio < full.ratio);
    assert!(full.makeup_db > 0.0 && full.makeup_db < 30.0);
}

#[test]
fn a_zone_lends_its_echo_and_scripts_and_zones_combine() {
    let canyon = AudioZone { echo: 0.6, echo_time: 0.45, ..Default::default() };
    let half = canyon.effects_at(0.5);
    assert!((half.echo - 0.3).abs() < 1e-6 && half.echo_time == 0.45, "the echo grows into the zone");
    assert_eq!(canyon.effects_at(0.0).echo, 0.0);
    assert!(canyon.weight(Vec2::ZERO, 0.0, Vec2::ONE, Vec2::ZERO) > 0.0);

    let script = BusEffects { distortion: 0.5, eq_low: 4.0, echo: 0.1, echo_time: 0.2, ..Default::default() };
    let zone = BusEffects { distortion: 0.2, eq_low: 3.0, echo: 0.4, echo_time: 0.6, ..Default::default() };
    let both = script.combine(zone);
    assert_eq!(both.distortion, 0.5, "the stronger distortion");
    assert_eq!(both.eq_low, 7.0, "EQs add");
    assert_eq!((both.echo, both.echo_time), (0.4, 0.6), "the wetter echo, with its own timing");
}

#[test]
fn the_backend_hears_when_only_an_echo_changes() {
    let mut mixer = AudioMixer::new();
    let _ = mixer.take_bus_plan();
    mixer.set_effect(SFX_BUS, "echo_time", 0.8).unwrap();
    let plan = mixer.take_bus_plan().expect("an echo time is a change");
    assert!(plan.buses.iter().any(|(n, _, fx)| n == SFX_BUS && fx.echo_time == 0.8));
    assert!(mixer.take_bus_plan().is_none(), "and only once");
    mixer.set_effect(SFX_BUS, "eq_high", -6.0).unwrap();
    assert!(mixer.take_bus_plan().is_some());
}
