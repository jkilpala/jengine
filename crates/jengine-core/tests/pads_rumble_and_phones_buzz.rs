//! Rumble and vibration (`input::haptics`): a script's request goes into one
//! process-wide queue, and the gamepad poll each platform already runs plays
//! it - gilrs natively, the Gamepad API in the browsers, the controller's or
//! the phone's vibrator on Android.

use std::sync::Mutex;

use jengine_core::input::haptics::{self, Haptic, MAX_SECONDS};

/// The queue is the process's: tests that fill it take turns.
static QUEUE: Mutex<()> = Mutex::new(());

#[cfg(feature = "native")]
#[test]
fn a_script_asks_a_pad_to_rumble_and_the_phone_to_buzz() {
    use hecs::World;
    use jengine_core::ecs::components::{ScriptComponent, ScriptLanguage, Transform};
    use jengine_core::ecs::systems::ScriptSystem;
    let _g = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    let _ = haptics::take();
    let mut world = World::new();
    let dir = std::env::temp_dir().join(format!("jengine_haptics_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.lua");
    std::fs::write(
        &path,
        "function update(e, dt)\n  Input.rumble(1, 0.8, 0.3, 0.4)\n  Input.vibrate(0.2)\n  Input.rumble(0, 1, 1, 60)\nend\n",
    )
    .unwrap();
    world.spawn((
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    system.run(&mut world, 1.0 / 60.0, &jengine_core::input::InputState::new(), None, None);
    let got = haptics::take();
    assert_eq!(
        got,
        vec![
            Haptic::Rumble { pad: 1, strong: 0.8, weak: 0.3, seconds: 0.4 },
            Haptic::Vibrate { seconds: 0.2, strength: 1.0 },
            Haptic::Rumble { pad: 0, strong: 1.0, weak: 1.0, seconds: MAX_SECONDS },
        ],
        "full strength by default, a minute cut to the cap"
    );
    assert!(haptics::take().is_empty(), "drained once");
}

#[test]
fn a_runaway_script_does_not_grow_the_queue() {
    let _g = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    let _ = haptics::take();
    for i in 0..1000 {
        haptics::rumble(0, 1.0, 1.0, i as f32 * 0.001);
    }
    let got = haptics::take();
    assert_eq!(got.len(), 64);
    let Some(Haptic::Rumble { seconds, .. }) = got.last() else { panic!("{got:?}") };
    assert!((seconds - 0.999).abs() < 1e-5, "keeps the latest: {seconds}");
}

#[test]
fn every_platform_plays_what_is_queued() {
    let native = include_str!("../../jengine-runtime/src/gamepad_native.rs");
    assert!(native.contains("self.play_haptics();") && native.contains("haptics::take()"), "native never rumbles");
    let web = include_str!("../src/input/gamepad_web.rs");
    assert!(web.contains("play_haptics(&navigator, &pads);") && web.contains("haptics::take()"), "the browsers never rumble");
    assert!(web.contains("\"dual-rumble\"") && web.contains("\"vibrate\""));
    let android = include_str!("../../jengine-runtime/src/gamepad_android.rs");
    assert!(android.contains("play_haptics();") && android.contains("vibrate_phone("), "Android never buzzes");
    let manifest = include_str!("../../jengine-builder/src/android.rs");
    assert!(manifest.contains("android.permission.VIBRATE"), "an Android build may not vibrate the phone");
    let nav = include_str!("../src/scripting/bridge/adapters/js/nav.rs");
    assert!(nav.contains("install_haptics_callbacks(&state)"));
    let hooks = include_str!("../src/scripting/bridge/adapters/js/haptics.rs");
    assert!(hooks.contains("\"input_rumble\"") && hooks.contains("\"input_vibrate\""));
}
