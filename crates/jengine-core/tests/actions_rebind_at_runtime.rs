//! **Rebinding at runtime** (Oct 2026): a settings screen asks "press a key
//! for Jump" with `Input.get_just_pressed()` - every input that went down
//! this frame, by the name an action binds it by - and hands the answer to
//! `Input.set_bindings`. With `Save.set` / `Save.commit` it holds after a
//! restart (the showcase's The Controls Room shows the round trip). The
//! setters existed; nothing could say *which* key was pressed, short of
//! asking about every key there is one by one.

use std::collections::HashSet;

use jengine_core::input::{InputAction, InputState, KeyCode};

#[test]
fn keys_and_pad_inputs_are_named_as_bindings_on_the_frame_they_go_down() {
    let mut input = InputState::new();
    input.begin_frame();
    input.key_down(KeyCode::Space);
    input.key_down(KeyCode::A);
    input.update_gamepad(0, HashSet::from([0usize]), vec![0.9, -0.95, 0.0, 0.0]);
    let names = input.just_pressed_bindings();
    for want in ["A", "Space", "Pad0_A", "Pad0_LeftStickX+", "Pad0_LeftStickY-"] {
        assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
    }
    // Every name is one an action can be bound to and then reads as pressed.
    for name in &names {
        input.load_actions(&[InputAction { name: "probe".into(), keys: vec![name.clone()], dead_zone: 0.0 }]);
        assert!(input.is_action_pressed("probe"), "`{name}` binds");
    }
    // Held into the next frame: nothing new went down.
    input.begin_frame();
    input.update_gamepad(0, HashSet::from([0usize]), vec![0.9, -0.95, 0.0, 0.0]);
    assert!(input.just_pressed_bindings().is_empty(), "{:?}", input.just_pressed_bindings());
}

#[test]
fn the_names_come_back_in_one_order() {
    let mut a = InputState::new();
    let mut b = InputState::new();
    for k in [KeyCode::Z, KeyCode::B, KeyCode::Enter, KeyCode::Digit1] {
        a.key_down(k);
    }
    for k in [KeyCode::Digit1, KeyCode::Enter, KeyCode::B, KeyCode::Z] {
        b.key_down(k);
    }
    assert_eq!(a.just_pressed_bindings(), b.just_pressed_bindings());
}

#[cfg(feature = "native")]
#[test]
fn a_script_captures_a_key_and_rebinds_an_action_to_it() {
    use hecs::World;
    use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
    use jengine_core::ecs::systems::ScriptSystem;
    const SRC: &str = r#"
local capturing = true
function update(entity, dt)
    if capturing then
        local pressed = Input.get_just_pressed()
        if #pressed > 0 then
            Input.set_bindings("jump", { pressed[1] })
            Entity.set_custom(entity, "Probe", "bound", pressed[1])
            capturing = false
        end
    elseif Input.is_action_just_pressed("jump") then
        Entity.set_custom(entity, "Probe", "jumps", (Entity.get_custom(entity, "Probe", "jumps") or 0) + 1)
    end
end
"#;
    let dir = std::env::temp_dir().join(format!("jengine_rebind_{}_{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rebind.lua");
    std::fs::write(&path, SRC).unwrap();
    let mut world = World::new();
    let e = world.spawn((
        Name("Probe".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let mut input = InputState::new();
    input.load_actions(&[InputAction { name: "jump".into(), keys: vec!["Space".into()], dead_zone: 0.0 }]);
    let mut frame = |input: &mut InputState, world: &mut World, down: Option<KeyCode>, up: Option<KeyCode>| {
        input.begin_frame();
        if let Some(k) = down {
            input.key_down(k);
        }
        if let Some(k) = up {
            input.key_up(k);
        }
        system.run(world, 1.0 / 60.0, input, None, None);
        // As the native runner does: remaps queued by the script reach the
        // live input after the run.
        for cmd in system.api.services.pending_input_remaps.drain(..).collect::<Vec<_>>() {
            jengine_core::input::apply_input_remap(input, &cmd);
        }
    };
    frame(&mut input, &mut world, None, None);
    frame(&mut input, &mut world, Some(KeyCode::J), None);
    frame(&mut input, &mut world, None, Some(KeyCode::J));
    assert_eq!(input.get_bindings("jump").cloned(), Some(vec!["J".to_string()]), "jump is J now");
    // Space no longer jumps; J does.
    frame(&mut input, &mut world, Some(KeyCode::Space), None);
    frame(&mut input, &mut world, None, Some(KeyCode::Space));
    frame(&mut input, &mut world, Some(KeyCode::J), None);
    let read = |key: &str| {
        world.get::<&CustomComponents>(e).ok().and_then(|c| c.0.get("Probe").and_then(|p| p.get(key)).cloned())
    };
    assert_eq!(read("bound").and_then(|v| v.as_str().map(str::to_string)).as_deref(), Some("J"));
    assert_eq!(read("jumps").and_then(|v| v.as_f64()), Some(1.0), "one jump, by J");
}
