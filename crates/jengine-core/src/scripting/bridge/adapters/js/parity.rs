//! Nativeâ†”web bridge parity tests (Phase 4.6).
//!
//! Drives the same conceptual Luau script through:
//! 1. [`super::super::luau::LuauAdapter`] — native production path.
//! 2. [`super::JsAdapter`] with the host QuickJS path enabled — stand-in for
//!    the browser execution that runs in `jengine-web` / `jengine-editor-viewport`.
//!
//! The script is identical for the Luau side; for the JS side it goes through
//! the canonical [`super::transpiler`] before being eval'd in QuickJS. After
//! both sides run, we assert the emitted [`BridgeCommand`] streams are equal.

use super::super::super::adapter::ScriptAdapter;
use super::super::super::capability::Capabilities;
use super::super::super::command::BridgeCommand;
use super::super::super::context::{BridgeContext, BridgeReadSnapshot};
use super::super::luau::LuauAdapter;
use super::JsAdapter;

/// Run one fixture through both adapters and return `(luau_cmds, js_cmds)`.
fn run_both(luau_src: &str, snapshot: &BridgeReadSnapshot, entity_id: u64) -> (Vec<BridgeCommand>, Vec<BridgeCommand>) {
    // Luau side ------------------------------------------------------------
    let mut luau = LuauAdapter::new(Capabilities::empty()).expect("luau adapter");
    let luau_handle = luau.load("parity.luau", luau_src).expect("luau load");
    let mut luau_cmds: Vec<BridgeCommand> = Vec::new();
    {
        let mut ctx = BridgeContext::new(0.016, snapshot, Capabilities::empty(), &mut luau_cmds);
        luau.init(luau_handle, entity_id, &mut ctx).expect("luau init");
        luau.update(luau_handle, entity_id, &mut ctx).expect("luau update");
    }

    // JS side --------------------------------------------------------------
    let mut js = JsAdapter::new();
    // Note: JsAdapter::load runs the canonical transpile step, then forwards
    // to the host QuickJS engine because `js-adapter-host` is enabled in this
    // test build.
    let js_handle = js.load("parity.luau", luau_src).expect("js load");
    let mut js_cmds: Vec<BridgeCommand> = Vec::new();
    {
        let mut ctx = BridgeContext::new(0.016, snapshot, Capabilities::empty(), &mut js_cmds);
        js.init(js_handle, entity_id, &mut ctx).expect("js init");
        js.update(js_handle, entity_id, &mut ctx).expect("js update");
    }

    (luau_cmds, js_cmds)
}

#[test]
fn parity_log_only() {
    let src = r#"
        function init(entity)
            log("hello " .. entity)
        end
        function update(entity, dt)
            log("tick")
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 7);
    assert_eq!(luau, js, "log-only parity\nluau={luau:#?}\njs={js:#?}");
}

#[test]
fn parity_set_position_constant() {
    let src = r#"
        function update(entity, dt)
            set_position(entity, 1.5, 2.5)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 42);
    assert_eq!(luau, js);
    assert_eq!(luau, vec![BridgeCommand::SetPosition { entity_id: 42, x: 1.5, y: 2.5 }]);
}

#[test]
fn parity_read_then_write() {
    let src = r#"
        function update(entity, dt)
            local x, y = get_position(entity)
            set_position(entity, x + 1, y + 2)
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.positions.insert(3, (10.0, 20.0));
    let (luau, js) = run_both(src, &snap, 3);
    assert_eq!(luau, js);
    assert_eq!(luau, vec![BridgeCommand::SetPosition { entity_id: 3, x: 11.0, y: 22.0 }]);
}

#[test]
fn parity_rotation_and_scale() {
    let src = r#"
        function update(entity, dt)
            set_rotation(entity, 1.25)
            set_scale(entity, 2.0, 3.0)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 9);
    assert_eq!(luau, js);
}

#[test]
fn parity_init_and_update_ordering() {
    // Both sides must run init before update, and emit commands in the same
    // order across the two lifecycle calls.
    let src = r#"
        function init(entity)
            set_position(entity, 0.0, 0.0)
            log("init " .. entity)
        end
        function update(entity, dt)
            set_position(entity, 1.0, 1.0)
            log("update " .. entity)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 5);
    assert_eq!(luau, js);
    assert_eq!(luau.len(), 4, "expected init+log, update+log");
}

// --- Phase 5.1: entity spawn / destroy / find / tags ----------------------

#[test]
fn parity_spawn_entity_allocates_same_ids() {
    // Both adapters must allocate identical pre-allocated spawn IDs so a script
    // that spawns and immediately moves a freshly-created entity behaves the
    // same on native and web.
    let src = r#"
        function update(entity, dt)
            local a = spawn_entity("Alpha")
            local b = spawn_entity("Bravo")
            set_position(a, 1.0, 1.0)
            set_position(b, 2.0, 2.0)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js, "spawn-id parity\nluau={luau:#?}\njs={js:#?}");
    // Confirm IDs really were 1_000_000 and 1_000_001 (the documented base).
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SpawnEntity { name: "Alpha".into(), prefab: None, spawn_id: 1_000_000 },
            BridgeCommand::SpawnEntity { name: "Bravo".into(), prefab: None, spawn_id: 1_000_001 },
            BridgeCommand::SetPosition { entity_id: 1_000_000, x: 1.0, y: 1.0 },
            BridgeCommand::SetPosition { entity_id: 1_000_001, x: 2.0, y: 2.0 },
        ]
    );
}

#[test]
fn parity_spawn_prefab_carries_name() {
    let src = r#"
        function update(entity, dt)
            spawn_prefab("Bullet")
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![BridgeCommand::SpawnEntity {
            name: "Bullet".into(),
            prefab: Some("Bullet".into()),
            spawn_id: 1_000_000,
        }]
    );
}

#[test]
fn parity_destroy_entity() {
    let src = r#"
        function update(entity, dt)
            destroy_entity(entity)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 17);
    assert_eq!(luau, js);
    assert_eq!(luau, vec![BridgeCommand::DestroyEntity { entity_id: 17 }]);
}

#[test]
fn parity_tag_add_remove_has() {
    // `has_tag` reads the snapshot — we pre-seed entity 5 with the "enemy" tag
    // and assert the script branches the same way on both sides.
    let src = r#"
        function update(entity, dt)
            if has_tag(entity, "enemy") then
                add_tag(entity, "marked")
            else
                add_tag(entity, "neutral")
            end
            remove_tag(entity, "stale")
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    let mut tags = std::collections::HashSet::new();
    tags.insert("enemy".to_string());
    snap.tags.insert(5, tags);
    let (luau, js) = run_both(src, &snap, 5);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::AddTag { entity_id: 5, tag: "marked".into() },
            BridgeCommand::RemoveTag { entity_id: 5, tag: "stale".into() },
        ]
    );
}

#[test]
fn parity_find_by_name() {
    let src = r#"
        function update(entity, dt)
            local target = find_by_name("Player")
            if target then
                set_position(target, 9.0, 9.0)
            end
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.names.insert(42, "Player".to_string());
    snap.name_to_id.insert("Player".to_string(), 42);
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(luau, vec![BridgeCommand::SetPosition { entity_id: 42, x: 9.0, y: 9.0 }]);
}

// --- Phase 5.2: sprite + material -----------------------------------------

#[test]
fn parity_sprite_color_set_and_get() {
    // Read current color, brighten R, write back. Confirms get_sprite_color
    // returns matching tuple/array shape on both sides.
    let src = r#"
        function update(entity, dt)
            local r, g, b, a = get_sprite_color(entity)
            set_sprite_color(entity, r + 0.1, g, b, a)
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.sprite_colors.insert(2, [0.5, 0.6, 0.7, 1.0]);
    let (luau, js) = run_both(src, &snap, 2);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![BridgeCommand::SetSpriteColor {
            entity_id: 2,
            r: 0.6,
            g: 0.6,
            b: 0.7,
            a: 1.0,
        }]
    );
}

#[test]
fn parity_sprite_anchor_set_and_get() {
    let src = r#"
        function update(entity, dt)
            local ax, ay = get_sprite_anchor(entity)
            set_sprite_anchor(entity, ax, ay + 0.25)
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.sprite_anchors.insert(4, [0.5, 0.5]);
    let (luau, js) = run_both(src, &snap, 4);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![BridgeCommand::SetSpriteAnchor { entity_id: 4, x: 0.5, y: 0.75 }]
    );
}

#[test]
fn parity_material_assignment() {
    let src = r#"
        function update(entity, dt)
            set_material(entity, "additive")
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 8);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![BridgeCommand::SetMaterial {
            entity_id: 8,
            material_id: "additive".into(),
        }]
    );
}

#[test]
fn parity_material_uniform_float_and_vec4() {
    let src = r#"
        function update(entity, dt)
            set_material_uniform_float(entity, "intensity", 0.75)
            set_material_uniform_vec4(entity, "tint", 1.0, 0.5, 0.25, 1.0)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 11);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SetMaterialUniformFloat {
                entity_id: 11,
                name: "intensity".into(),
                value: 0.75,
            },
            BridgeCommand::SetMaterialUniformVec4 {
                entity_id: 11,
                name: "tint".into(),
                value: [1.0, 0.5, 0.25, 1.0],
            },
        ]
    );
}

// --- Phase 5.3: input + action bindings -----------------------------------

#[test]
fn parity_key_pressed_branches() {
    // The script branches on `is_key_pressed("ArrowRight")`. We seed the
    // snapshot so the branch is taken and confirm both adapters emit the
    // same SetPosition command.
    let src = r#"
        function update(entity, dt)
            if is_key_pressed("ArrowRight") then
                set_position(entity, 1.0, 0.0)
            else
                set_position(entity, -1.0, 0.0)
            end
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.keys_pressed.insert("ArrowRight".to_string());
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(luau, vec![BridgeCommand::SetPosition { entity_id: 1, x: 1.0, y: 0.0 }]);
}

#[test]
fn parity_key_just_pressed_only_fires_on_edge() {
    let src = r#"
        function update(entity, dt)
            if is_key_just_pressed("Space") then
                log("jump")
            end
        end
    "#;
    let mut held = BridgeReadSnapshot::default();
    held.keys_pressed.insert("Space".to_string()); // held, but not just-pressed
    let (luau_held, js_held) = run_both(src, &held, 1);
    assert_eq!(luau_held, js_held);
    assert!(luau_held.is_empty(), "no log when key is only held");

    let mut edge = BridgeReadSnapshot::default();
    edge.keys_pressed.insert("Space".to_string());
    edge.keys_just_pressed.insert("Space".to_string());
    let (luau_edge, js_edge) = run_both(src, &edge, 1);
    assert_eq!(luau_edge, js_edge);
    assert_eq!(luau_edge, vec![BridgeCommand::Log { message: "jump".into() }]);
}

#[test]
fn parity_action_bindings() {
    let src = r#"
        function update(entity, dt)
            if is_action_pressed("move_left") then
                set_position(entity, -1.0, 0.0)
            end
            if is_action_just_pressed("fire") then
                log("bang")
            end
            if is_action_just_released("fire") then
                log("release")
            end
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.actions_pressed.insert("move_left".to_string());
    snap.actions_just_pressed.insert("fire".to_string());
    // Note: NOT just_released — confirm that branch is skipped on both sides.
    let (luau, js) = run_both(src, &snap, 7);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SetPosition { entity_id: 7, x: -1.0, y: 0.0 },
            BridgeCommand::Log { message: "bang".into() },
        ]
    );
}

#[test]
fn parity_mouse_state_reads() {
    // Reads mouse_position tuple/array, mouse-wheel scalar, and the
    // just-pressed bool. Confirms shape parity for tuple-returning getters.
    let src = r#"
        function update(entity, dt)
            local mx, my = get_mouse_position()
            set_position(entity, mx, my)
            local w = get_mouse_wheel()
            set_rotation(entity, w)
            if is_mouse_left_just_pressed() then
                log("click")
            end
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.mouse_position = (123.5, -456.25);
    snap.mouse_wheel_delta = 0.5;
    snap.mouse_left_just_pressed = true;
    let (luau, js) = run_both(src, &snap, 3);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SetPosition { entity_id: 3, x: 123.5, y: -456.25 },
            BridgeCommand::SetRotation { entity_id: 3, radians: 0.5 },
            BridgeCommand::Log { message: "click".into() },
        ]
    );
}

#[test]
fn parity_mouse_hold_and_release_reads() {
    // The drag trio: press edge, sustained hold, release edge. Each has to read
    // the same on native (Luau) and in the browser IIFE, because a drag that
    // starts in one runtime and never ends in the other is the exact failure
    // this harness exists to catch.
    let src = r#"
        function update(entity, dt)
            if is_mouse_left_pressed() then
                log("held")
            end
            if is_mouse_left_just_released() then
                log("dropped")
            end
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.mouse_left = true;
    snap.mouse_left_just_released = true;
    let (luau, js) = run_both(src, &snap, 3);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::Log { message: "held".into() },
            BridgeCommand::Log { message: "dropped".into() },
        ]
    );
}

#[test]
fn parity_mouse_hold_and_release_default_false() {
    // Nothing pressed: neither branch fires, on either runtime.
    let src = r#"
        function update(entity, dt)
            if is_mouse_left_pressed() then log("held") end
            if is_mouse_left_just_released() then log("dropped") end
            log("tick")
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 3);
    assert_eq!(luau, js);
    assert_eq!(luau, vec![BridgeCommand::Log { message: "tick".into() }]);
}

#[test]
fn parity_quit_game() {
    let src = r#"
        function update(entity, dt)
            quit_game()
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(luau, vec![BridgeCommand::QuitGame]);
}

// --- Phase 5.4: physics ---------------------------------------------------

#[test]
fn parity_apply_force_impulse_and_set_velocity() {
    let src = r#"
        function update(entity, dt)
            apply_force(entity, 10.0, -5.0)
            apply_impulse(entity, 0.5, 0.0)
            set_velocity(entity, 2.0, 3.0)
            set_angular_velocity(entity, 1.5)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 4);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::ApplyForce { entity_id: 4, fx: 10.0, fy: -5.0 },
            BridgeCommand::ApplyImpulse { entity_id: 4, ix: 0.5, iy: 0.0 },
            BridgeCommand::SetVelocity { entity_id: 4, vx: 2.0, vy: 3.0 },
            BridgeCommand::SetAngularVelocity { entity_id: 4, omega: 1.5 },
        ]
    );
}

#[test]
fn parity_get_velocity_read_then_dampen() {
    // Read current velocity, halve it, write back. Confirms tuple-returning
    // get_velocity destructures identically on both sides.
    let src = r#"
        function update(entity, dt)
            local vx, vy = get_velocity(entity)
            set_velocity(entity, vx * 0.5, vy * 0.5)
            local omega = get_angular_velocity(entity)
            set_angular_velocity(entity, omega * 0.5)
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.velocities.insert(2, (4.0, -8.0));
    snap.angular_velocities.insert(2, 2.0);
    let (luau, js) = run_both(src, &snap, 2);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SetVelocity { entity_id: 2, vx: 2.0, vy: -4.0 },
            BridgeCommand::SetAngularVelocity { entity_id: 2, omega: 1.0 },
        ]
    );
}

#[test]
fn parity_contact_iteration() {
    // Iterate over contacts and log their participants. Confirms the discrete
    // contact_count + get_contact(i) accessors agree across adapters,
    // including the 1-based index convention.
    let src = r#"
        function update(entity, dt)
            local n = contact_count()
            for i = 1, n do
                local a, b, started = get_contact(i)
                if started then
                    log("hit " .. a .. "-" .. b)
                end
            end
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.contacts.push((10, 20, true, 4.5, 1.0, 2.0));
    snap.contacts.push((30, 40, false, 0.0, 0.0, 0.0));
    snap.contacts.push((50, 60, true, 0.25, -3.0, 7.0));
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::Log { message: "hit 10-20".into() },
            BridgeCommand::Log { message: "hit 50-60".into() },
        ]
    );
}

// --- Phase 5.5: audio -----------------------------------------------------

#[test]
fn parity_play_sound_returns_handle_and_stops() {
    // Confirms `play_sound` returns the same pre-allocated handle on both
    // adapters (sound_counter base = 2_000_000) and that the script can pass
    // the captured handle straight back into `stop_sound`.
    let src = r#"
        function update(entity, dt)
            local h = play_sound("hit", 0.5, 1.25)
            stop_sound(h)
            set_sound_volume(h, 0.25)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 7);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::PlaySound {
                clip_id: "hit".into(),
                volume: 0.5,
                pitch: 1.25,
                // Centred: both adapters must default pan identically, which
                // is the point of a parity fixture.
                pan: 0.0,
                sound_id: 2_000_000,
                looping: false,
            },
            BridgeCommand::StopSound { handle_id: 2_000_000 },
            BridgeCommand::SetSoundVolume { handle_id: 2_000_000, volume: 0.25 },
        ]
    );
}

#[test]
fn parity_play_sound_defaults_apply_when_omitted() {
    // Both adapters must default volume → 1.0 and pitch → 1.0 when the
    // script omits the optional arguments.
    let src = r#"
        function update(entity, dt)
            play_sound("ambient")
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![BridgeCommand::PlaySound {
            clip_id: "ambient".into(),
            volume: 1.0,
            pitch: 1.0,
            pan: 0.0,
            sound_id: 2_000_000,
                looping: false,
        }]
    );
}

#[test]
fn parity_music_lifecycle_and_volumes() {
    let src = r#"
        function update(entity, dt)
            play_music("theme", 0.75, true)
            pause_music()
            resume_music()
            set_music_volume(0.5)
            set_master_volume(0.8)
            stop_music()
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::PlayMusic { clip_id: "theme".into(), volume: 0.75, looping: true },
            BridgeCommand::PauseMusic,
            BridgeCommand::ResumeMusic,
            BridgeCommand::SetMusicVolume { volume: 0.5 },
            BridgeCommand::SetMasterVolume { volume: 0.8 },
            BridgeCommand::StopMusic,
        ]
    );
}

// --- Phase 5.6: animation -------------------------------------------------

#[test]
fn parity_play_stop_set_speed_animation() {
    let src = r#"
        function update(entity, dt)
            play_animation(entity, "walk")
            set_animation_speed(entity, 1.5)
            stop_animation(entity)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 8);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::PlayAnimation { entity_id: 8, clip_name: "walk".into() },
            BridgeCommand::SetAnimationSpeed { entity_id: 8, speed: 1.5 },
            BridgeCommand::StopAnimation { entity_id: 8 },
        ]
    );
}

#[test]
fn parity_anim_state_machine_params_and_reads() {
    // Confirms the snapshot reads (`is_animation_playing`, `get_current_animation`,
    // `get_anim_state`) drive identical branching across adapters and that
    // the parameter setters queue identical commands.
    let src = r#"
        function update(entity, dt)
            if is_animation_playing(entity) then
                log("playing " .. get_current_animation(entity))
            end
            log("state=" .. get_anim_state(entity))
            set_anim_bool(entity, "moving", true)
            set_anim_float(entity, "speed", 4.5)
            set_anim_trigger(entity, "jump")
            start_state_machine(entity, "Idle")
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.anim_playing.insert(3, true);
    snap.anim_clip_names.insert(3, "run".into());
    snap.anim_state_names.insert(3, "Locomotion".into());
    let (luau, js) = run_both(src, &snap, 3);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::Log { message: "playing run".into() },
            BridgeCommand::Log { message: "state=Locomotion".into() },
            BridgeCommand::SetAnimBool { entity_id: 3, param: "moving".into(), value: true },
            BridgeCommand::SetAnimFloat { entity_id: 3, param: "speed".into(), value: 4.5 },
            BridgeCommand::SetAnimTrigger { entity_id: 3, param: "jump".into() },
            BridgeCommand::StartStateMachine { entity_id: 3, state_name: "Idle".into() },
        ]
    );
}

#[test]
fn parity_tweens_allocate_matching_ids_and_cancel() {
    // Both adapters must allocate identical tween ids from the shared
    // tween_counter (base 3_000_000) and the script captures them in source
    // order so cancel_tween targets the right one.
    let src = r#"
        function update(entity, dt)
            local p = tween_position(entity, 100, 200, 1.0)
            local r = tween_rotation(entity, 1.5, 0.5, "ease_out_quad")
            local s = tween_scale(entity, 2, 2, 0.25)
            local c = tween_color(entity, 1, 0, 0, 1, 0.5)
            cancel_tween(p)
            cancel_tween(r)
            cancel_tween(s)
            cancel_tween(c)
            cancel_all_tweens(entity)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 9);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::StartTween {
                entity_id: 9,
                target: "position".into(),
                end_value: [100.0, 200.0, 0.0, 0.0],
                duration: 1.0,
                easing: "linear".into(),
                tween_id: 3_000_000,
            },
            BridgeCommand::StartTween {
                entity_id: 9,
                target: "rotation".into(),
                end_value: [1.5, 0.0, 0.0, 0.0],
                duration: 0.5,
                easing: "ease_out_quad".into(),
                tween_id: 3_000_001,
            },
            BridgeCommand::StartTween {
                entity_id: 9,
                target: "scale".into(),
                end_value: [2.0, 2.0, 0.0, 0.0],
                duration: 0.25,
                easing: "linear".into(),
                tween_id: 3_000_002,
            },
            BridgeCommand::StartTween {
                entity_id: 9,
                target: "sprite_color".into(),
                end_value: [1.0, 0.0, 0.0, 1.0],
                duration: 0.5,
                easing: "linear".into(),
                tween_id: 3_000_003,
            },
            BridgeCommand::CancelTween { tween_id: 3_000_000 },
            BridgeCommand::CancelTween { tween_id: 3_000_001 },
            BridgeCommand::CancelTween { tween_id: 3_000_002 },
            BridgeCommand::CancelTween { tween_id: 3_000_003 },
            BridgeCommand::CancelAllTweens { entity_id: 9 },
        ]
    );
}

// --- Phase 5.7: scene -----------------------------------------------------

#[test]
fn parity_scene_load_push_pop_and_read_name() {
    let src = r#"
        function update(entity, dt)
            log("scene=" .. get_current_scene_name())
            push_scene("scenes/pause.scene")
            pop_scene()
            load_scene("scenes/level2.scene")
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    crate::scene::set_current_name("Level 1");
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::Log { message: "scene=Level 1".into() },
            BridgeCommand::PushScene { path: "scenes/pause.scene".into() },
            BridgeCommand::PopScene,
            BridgeCommand::LoadScene { path: "scenes/level2.scene".into() },
        ]
    );
}

#[test]
fn parity_scene_name_empty_when_unset() {
    // No scene recorded: both adapters must produce the same `""` sentinel
    // (no nil/undefined leaking through).
    let src = r#"
        function update(entity, dt)
            local name = get_current_scene_name()
            if name == "" then
                log("no scene")
            else
                log("active=" .. name)
            end
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    crate::scene::set_current_name("");
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(luau, vec![BridgeCommand::Log { message: "no scene".into() }]);
}

// --- Phase 5.8: events & timers -------------------------------------------

#[test]
fn parity_send_event_with_and_without_data() {
    let src = r#"
        function update(entity, dt)
            send_event("game_over", entity, "score=10")
            send_event("ping", entity)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 7);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SendEvent { name: "game_over".into(), sender: 7, data: Some("score=10".into()) },
            BridgeCommand::SendEvent { name: "ping".into(), sender: 7, data: None },
        ]
    );
}

#[test]
fn parity_event_listener_register_and_remove_allocate_matching_ids() {
    let src = r#"
        function update(entity, dt)
            local a = on_event("hit", entity)
            local b = on_event("collect", entity)
            log("a=" .. tostring(a) .. " b=" .. tostring(b))
            remove_event_listener("hit", entity)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 3);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::RegisterEventListener { event_name: "hit".into(), entity_id: 3, listener_id: 4_000_000 },
            BridgeCommand::RegisterEventListener { event_name: "collect".into(), entity_id: 3, listener_id: 4_000_001 },
            BridgeCommand::Log { message: "a=4000000 b=4000001".into() },
            BridgeCommand::RemoveEventListener { event_name: "hit".into(), entity_id: 3 },
        ]
    );
}

#[test]
fn parity_timer_set_interval_and_cancel_allocate_matching_ids() {
    let src = r#"
        function update(entity, dt)
            local t1 = set_timer(entity, 1.5, "on_done")
            local t2 = set_interval(entity, 0.25, "tick")
            log("t1=" .. tostring(t1) .. " t2=" .. tostring(t2))
            cancel_timer(t1)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 9);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SetTimer {
                entity_id: 9,
                delay: 1.5,
                callback_name: "on_done".into(),
                repeating: false,
                timer_id: 5_000_000,
            },
            BridgeCommand::SetTimer {
                entity_id: 9,
                delay: 0.25,
                callback_name: "tick".into(),
                repeating: true,
                timer_id: 5_000_001,
            },
            BridgeCommand::Log { message: "t1=5000000 t2=5000001".into() },
            BridgeCommand::CancelTimer { timer_id: 5_000_000 },
        ]
    );
}

// --- Phase 5.9: UI / GUI --------------------------------------------------

#[test]
fn parity_ui_create_text_and_panel_share_spawn_counter() {
    let src = r#"
        function update(entity, dt)
            local t = create_ui_text("Hello", "top_center", 0, -8)
            local p = create_ui_panel("bottom_left", 4, 4, 100, 32, 0.1, 0.2, 0.3, 0.8)
            log("t=" .. tostring(t) .. " p=" .. tostring(p))
            set_ui_text(t, "World")
            set_ui_visible(p, false)
            set_ui_position(t, "center", 1, 2)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::CreateUIText {
                text: "Hello".into(),
                anchor: "top_center".into(),
                offset_x: 0.0,
                offset_y: -8.0,
                color: [1.0, 1.0, 1.0, 1.0],
                font_size: 16.0,
                alignment: "left".into(),
                entity_id: 1_000_000,
            },
            BridgeCommand::CreateUIPanel {
                anchor: "bottom_left".into(),
                offset_x: 4.0,
                offset_y: 4.0,
                width: 100.0,
                height: 32.0,
                bg_color: [0.1, 0.2, 0.3, 0.8],
                entity_id: 1_000_001,
            },
            BridgeCommand::Log { message: "t=1000000 p=1000001".into() },
            BridgeCommand::SetUIText { entity_id: 1_000_000, text: "World".into() },
            BridgeCommand::SetUIVisible { entity_id: 1_000_001, visible: false },
            BridgeCommand::SetUIPosition {
                entity_id: 1_000_000,
                anchor: "center".into(),
                offset_x: 1.0,
                offset_y: 2.0,
            },
        ]
    );
}

#[test]
fn parity_gui_show_hide_set_focus_and_on_click() {
    let src = r#"
        function update(entity, dt)
            gui_show("hud")
            gui_set("hud", "score", "text", "42")
            gui_on_click(entity, "hud", "btn_pause", "on_pause")
            gui_focus("hud", "btn_pause")
            gui_hide("hud")
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 5);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::GuiShow { gui_id: "hud".into() },
            BridgeCommand::GuiSetNodeProp {
                gui_id: "hud".into(),
                node_id: "score".into(),
                property: "text".into(),
                value: "42".into(),
            },
            BridgeCommand::GuiOnClick {
                entity_id: 5,
                gui_id: "hud".into(),
                node_id: "btn_pause".into(),
                callback_name: "on_pause".into(),
            },
            BridgeCommand::GuiFocus { gui_id: "hud".into(), node_id: "btn_pause".into() },
            BridgeCommand::GuiHide { gui_id: "hud".into() },
        ]
    );
}

#[test]
fn parity_gui_get_reads_snapshot_with_empty_sentinel() {
    let src = r#"
        function update(entity, dt)
            local v = gui_get("hud", "score", "text")
            local missing = gui_get("hud", "nope", "text")
            log("v=" .. v .. " missing=[" .. missing .. "]")
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.gui_node_props.insert(
        ("hud".into(), "score".into(), "text".into()),
        "1337".into(),
    );
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![BridgeCommand::Log { message: "v=1337 missing=[]".into() }]
    );
}

// --- Phase 5.10: particles -----------------------------------------------

#[test]
fn parity_particle_emit_burst_set_active_and_set_rate() {
    let src = r#"
        function update(entity, dt)
            emit_burst(entity, 12)
            set_emitter_active(entity, false)
            set_emitter_rate(entity, 25.5)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 9);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::EmitBurst { entity_id: 9, count: 12 },
            BridgeCommand::SetEmitterActive { entity_id: 9, active: false },
            BridgeCommand::SetEmitterRate { entity_id: 9, rate: 25.5 },
        ]
    );
}

#[test]
fn parity_get_particle_count_reads_snapshot_with_zero_sentinel() {
    let src = r#"
        function update(entity, dt)
            local n = get_particle_count()
            log("n=" .. tostring(n))
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.particle_count = 7;
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(luau, vec![BridgeCommand::Log { message: "n=7".into() }]);

    let zero = BridgeReadSnapshot::default();
    let (luau_zero, js_zero) = run_both(src, &zero, 1);
    assert_eq!(luau_zero, js_zero);
    assert_eq!(luau_zero, vec![BridgeCommand::Log { message: "n=0".into() }]);
}

// --- Phase 5.11: tilemap -------------------------------------------------

fn build_test_tilemap_snapshot(entity_id: u64, transform: (f32, f32)) -> BridgeReadSnapshot {
    use crate::tilemap::{Tilemap, TilemapLayer};
    let layer = TilemapLayer {
        name: "ground".into(),
        // 3x2 grid: row 0 = [1, 2, empty], row 1 = [empty, 3, 4]
        tiles: vec![1, 2, 0, 0, 3, 4],
        width: 3,
        height: 2,
        visible: true,
        collision: false,
        ..Default::default()
    };
    let tilemap = Tilemap {
        tile_size: [32.0, 32.0],
        layers: vec![layer],
        tilesets: vec![],
        source: None,
        z_order: 0,
        sort_layer: None,
        ..Default::default()
    };
    let mut snap = BridgeReadSnapshot::default();
    snap.tilemap_snapshots.insert(entity_id, tilemap);
    snap.tilemap_transforms.insert(entity_id, transform);
    snap
}

#[test]
fn parity_get_tile_in_bounds_empty_and_out_of_bounds() {
    let src = r#"
        function update(entity, dt)
            local hit = get_tile(entity, "ground", 1, 0)       -- 2
            local empty = get_tile(entity, "ground", 2, 0)     -- 0 (in-bounds None)
            local oob = get_tile(entity, "ground", 99, 0)      -- -1
            local missing = get_tile(entity, "void", 0, 0)     -- -1
            log("h=" .. tostring(hit) .. " e=" .. tostring(empty)
                .. " o=" .. tostring(oob) .. " m=" .. tostring(missing))
        end
    "#;
    let snap = build_test_tilemap_snapshot(11, (0.0, 0.0));
    let (luau, js) = run_both(src, &snap, 11);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![BridgeCommand::Log { message: "h=2 e=0 o=-1 m=-1".into() }]
    );
}

#[test]
fn parity_set_tile_with_clear_via_negative_id() {
    let src = r#"
        function update(entity, dt)
            set_tile(entity, "ground", 0, 0, 7)
            set_tile(entity, "ground", 1, 1, -1)   -- negative -> clear (None)
        end
    "#;
    let snap = build_test_tilemap_snapshot(2, (0.0, 0.0));
    let (luau, js) = run_both(src, &snap, 2);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SetTile {
                entity_id: 2,
                layer_name: "ground".into(),
                x: 0,
                y: 0,
                tile_id: Some(7),
            },
            BridgeCommand::SetTile {
                entity_id: 2,
                layer_name: "ground".into(),
                x: 1,
                y: 1,
                tile_id: None,
            },
        ]
    );
}

#[test]
fn parity_world_tile_round_trip_and_get_at_world() {
    // Tilemap at origin (0,0), tile_size 32, 3x2 grid.
    // World (40, -50) -> tile (1, 1) (Y inverted: world +Y is up)
    // tile (1, 1) -> world (1*32 + 16, -1*32 - 16) = (48, -48)
    let src = r#"
        function update(entity, dt)
            local tx, ty = world_to_tile(entity, 40.0, -50.0)
            local wx, wy = tile_to_world(entity, tx, ty)
            set_position(entity, wx, wy)
            local at = get_tile_at_world(entity, "ground", 40.0, -50.0)   -- tile (1,1) = 3
            log("t=" .. tostring(tx) .. "," .. tostring(ty) .. " at=" .. tostring(at))
        end
    "#;
    let snap = build_test_tilemap_snapshot(5, (0.0, 0.0));
    let (luau, js) = run_both(src, &snap, 5);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SetPosition { entity_id: 5, x: 48.0, y: -48.0 },
            BridgeCommand::Log { message: "t=1,1 at=3".into() },
        ]
    );
}

#[test]
fn parity_world_to_tile_missing_entity_returns_zero_sentinel() {
    let src = r#"
        function update(entity, dt)
            local tx, ty = world_to_tile(entity, 40.0, -50.0)
            local wx, wy = tile_to_world(entity, 3, 4)
            log("t=" .. tostring(tx) .. "," .. tostring(ty)
                .. " w=" .. tostring(wx) .. "," .. tostring(wy))
        end
    "#;
    // No tilemap snapshot for entity 8 -> both helpers fall back to (0,0)/(0.0,0.0).
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 8);
    assert_eq!(luau, js);
    assert_eq!(luau, vec![BridgeCommand::Log { message: "t=0,0 w=0,0".into() }]);
}

// --- Phase 5.12: lighting ------------------------------------------------

#[test]
fn parity_lighting_set_color_intensity_radius_and_ambient() {
    let src = r#"
        function update(entity, dt)
            set_light_color(entity, 1.0, 0.5, 0.25)
            set_light_intensity(entity, 2.5)
            set_light_radius(entity, 128.0)
            set_ambient_light(0.1, 0.2, 0.3, 0.75)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 6);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SetLightColor { entity_id: 6, r: 1.0, g: 0.5, b: 0.25 },
            BridgeCommand::SetLightIntensity { entity_id: 6, intensity: 2.5 },
            BridgeCommand::SetLightRadius { entity_id: 6, radius: 128.0 },
            BridgeCommand::SetAmbientLight { r: 0.1, g: 0.2, b: 0.3, intensity: 0.75 },
        ]
    );
}

// --- Phase 5.12: post-process --------------------------------------------

#[test]
fn parity_postprocess_enabled_float_and_vec4() {
    let src = r#"
        function update(entity, dt)
            set_post_process_enabled("bloom", true)
            set_post_process_float("bloom", "threshold", 0.8)
            set_post_process_vec4("vignette", "tint", 0.0, 0.0, 0.0, 0.7)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SetPostProcessEnabled {
                effect_name: "bloom".into(),
                enabled: true,
            },
            BridgeCommand::SetPostProcessFloat {
                effect_name: "bloom".into(),
                param_name: "threshold".into(),
                value: 0.8,
            },
            BridgeCommand::SetPostProcessVec4 {
                effect_name: "vignette".into(),
                param_name: "tint".into(),
                value: [0.0, 0.0, 0.0, 0.7],
            },
        ]
    );
}

// --- Phase 5.12: debug drawing & overlay ---------------------------------

#[test]
fn parity_debug_draws_and_overlay_toggle() {
    let src = r#"
        function update(entity, dt)
            draw_debug_line(0.0, 0.0, 10.0, 20.0, 1.0, 0.0, 0.0)
            draw_debug_rect(5.0, 5.0, 50.0, 25.0, 0.0, 1.0, 0.0)
            draw_debug_circle(100.0, 50.0, 16.0, 0.0, 0.0, 1.0)
            set_debug_overlay(true)
            set_debug_overlay(false)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::DrawDebugLine { x1: 0.0, y1: 0.0, x2: 10.0, y2: 20.0, r: 1.0, g: 0.0, b: 0.0 },
            BridgeCommand::DrawDebugRect { x: 5.0, y: 5.0, w: 50.0, h: 25.0, r: 0.0, g: 1.0, b: 0.0 },
            BridgeCommand::DrawDebugCircle { x: 100.0, y: 50.0, radius: 16.0, r: 0.0, g: 0.0, b: 1.0 },
            BridgeCommand::SetDebugOverlay { visible: true },
            BridgeCommand::SetDebugOverlay { visible: false },
        ]
    );
}

// ---- Phase 5.13: custom-component round-trip --------------------------------

#[test]
fn parity_custom_component_round_trip_numeric() {
    // Bunnymark-style: read two numeric fields, compute, write them back.
    // Host JS adapter only supports numeric custom-component values, so the
    // fixture sticks to numbers — Luau retains full Number/Bool/String fidelity
    // (covered by the dedicated luau test below).
    let src = r#"
        function update(entity, dt)
            local vx = get_custom(entity, "BunnyData", "velocity_x")
            local vy = get_custom(entity, "BunnyData", "velocity_y")
            set_custom(entity, "BunnyData", "velocity_x", vx + 1.0)
            set_custom(entity, "BunnyData", "velocity_y", vy - 2.0)
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    let mut comps = std::collections::HashMap::new();
    comps.insert(
        "BunnyData".to_string(),
        serde_json::json!({ "velocity_x": 10.0, "velocity_y": 5.0 }),
    );
    snap.custom_components.insert(7, comps);

    let (luau, js) = run_both(src, &snap, 7);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SetCustomField {
                entity_id: 7,
                component_name: std::sync::Arc::from("BunnyData"),
                field_name: std::sync::Arc::from("velocity_x"),
                value: serde_json::Value::from(11.0_f64),
            },
            BridgeCommand::SetCustomField {
                entity_id: 7,
                component_name: std::sync::Arc::from("BunnyData"),
                field_name: std::sync::Arc::from("velocity_y"),
                value: serde_json::Value::from(3.0_f64),
            },
        ]
    );
}

#[test]
fn parity_custom_component_missing_field_is_nil_zero_on_js() {
    // Luau returns nil → script falls back to 0; host JS returns 0 directly.
    // Both produce the same SetCustomField stream.
    let src = r#"
        function update(entity, dt)
            local n = get_custom(entity, "Health", "missing") or 0
            set_custom(entity, "Health", "missing", n + 1.0)
        end
    "#;
    let mut snap = BridgeReadSnapshot::default();
    snap.custom_components
        .insert(1, std::collections::HashMap::new());
    let (luau, js) = run_both(src, &snap, 1);
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![BridgeCommand::SetCustomField {
            entity_id: 1,
            component_name: std::sync::Arc::from("Health"),
            field_name: std::sync::Arc::from("missing"),
            value: serde_json::Value::from(1.0_f64),
        }]
    );
}

// --- Script-reuse Phase A: require() parity --------------------------------

/// Variant of [`run_both`] that wires the same module resolver into both
/// adapters before loading the entity script. The resolver receives the path
/// passed to `require`; both backends must agree on what they get back, modulo
/// the JS adapter running Lua-extension paths through the transpiler.
fn run_both_with_resolver<F>(
    luau_src: &str,
    snapshot: &BridgeReadSnapshot,
    entity_id: u64,
    resolver: F,
) -> (Vec<BridgeCommand>, Vec<BridgeCommand>)
where
    F: Fn(&str) -> Result<String, String> + 'static,
{
    use std::rc::Rc;
    let resolver: crate::scripting::bridge::ModuleResolver = Rc::new(resolver);

    // Luau side
    let mut luau = LuauAdapter::new(Capabilities::empty()).expect("luau adapter");
    luau.set_module_resolver(resolver.clone());
    let luau_handle = luau.load("parity.luau", luau_src).expect("luau load");
    let mut luau_cmds: Vec<BridgeCommand> = Vec::new();
    {
        let mut ctx =
            BridgeContext::new(0.016, snapshot, Capabilities::empty(), &mut luau_cmds);
        luau.init(luau_handle, entity_id, &mut ctx).expect("luau init");
        luau.update(luau_handle, entity_id, &mut ctx).expect("luau update");
    }

    // JS side — set resolver BEFORE first load so the lazily-built host
    // engine receives it via `pending_resolver`.
    let mut js = JsAdapter::new();
    js.set_module_resolver(resolver);
    let js_handle = js.load("parity.luau", luau_src).expect("js load");
    let mut js_cmds: Vec<BridgeCommand> = Vec::new();
    {
        let mut ctx = BridgeContext::new(0.016, snapshot, Capabilities::empty(), &mut js_cmds);
        js.init(js_handle, entity_id, &mut ctx).expect("js init");
        js.update(js_handle, entity_id, &mut ctx).expect("js update");
    }

    (luau_cmds, js_cmds)
}

#[test]
fn parity_require_basic_export() {
    // Library exports a single function; the entity script imports and calls it.
    // The library uses `local function`+`return { add = add }` rather than an
    // inline `function() ... end` table value because the canonical transpiler
    // doesn't yet handle inline anonymous-function expressions inside table
    // literals. (Phase D follow-up: extend the transpiler.)
    let src = r#"
        local m = require("lib/math")
        function update(entity, dt)
            set_position(entity, m.add(1, 2), m.add(10, 20))
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both_with_resolver(src, &snap, 1, |path| match path {
        "lib/math" => Ok(r#"
            local function add(x, y)
                return x + y
            end
            return { add = add }
        "#
        .into()),
        other => Err(format!("not found: {other}")),
    });
    assert_eq!(luau, js, "require parity\nluau={luau:#?}\njs={js:#?}");
    assert_eq!(
        luau,
        vec![BridgeCommand::SetPosition {
            entity_id: 1,
            x: 3.0,
            y: 30.0,
        }]
    );
}

#[test]
fn parity_require_caches_module_body() {
    // The entity script requires the same module from two different bindings
    // and asserts that the returned tables are identity-equal. The assertion
    // lives inside the script so a cache miss raises a runtime error in
    // BOTH backends — the test then becomes "neither side panicked".
    let src = r#"
        local a = require("lib/once")
        local b = require("lib/once")
        if a ~= b then error("require returned distinct tables") end
        function update(entity, dt)
            log(a.tag)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both_with_resolver(src, &snap, 1, |_| {
        Ok("return { tag = 'ok' }".into())
    });
    assert_eq!(luau, js);
    assert_eq!(
        luau,
        vec![BridgeCommand::Log {
            message: "ok".into(),
        }]
    );
}

#[test]
fn parity_require_resolver_error_surfaces() {
    // When the resolver returns Err, both backends must fail the load (the
    // require call happens at module top-level, so the error propagates out
    // of `JsAdapter::load` / `LuauAdapter::load`). We don't compare command
    // streams because both sides errored before emitting anything.
    let src = r#"
        local m = require("lib/missing")
        function update(entity, dt) log("never") end
    "#;
    use std::rc::Rc;
    let resolver: crate::scripting::bridge::ModuleResolver =
        Rc::new(|path: &str| Err(format!("resolver-said-no: {path}")));

    let mut luau = LuauAdapter::new(Capabilities::empty()).unwrap();
    luau.set_module_resolver(resolver.clone());
    let luau_err = luau.load("parity.luau", src).unwrap_err();
    assert!(
        luau_err.message.contains("resolver-said-no"),
        "luau: {}",
        luau_err.message
    );

    let mut js = JsAdapter::new();
    js.set_module_resolver(resolver);
    let js_err = js.load("parity.luau", src).unwrap_err();
    assert!(
        js_err.message.contains("resolver-said-no"),
        "js: {}",
        js_err.message
    );
}


// --- Phase B: extends (prototype inheritance) parity ---------------------------

#[test]
fn parity_extends_inherits_parent_update() {
    let src = r#"
        script = { extends = "lib/parent" }
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both_with_resolver(src, &snap, 7, |path| match path {
        "lib/parent" => Ok(r#"
            local function update(entity, dt)
                set_position(entity, 11, 22)
            end
            return { update = update }
        "#
        .into()),
        other => Err(format!("not found: {other}")),
    });
    assert_eq!(luau, js, "extends parity\nluau={luau:#?}\njs={js:#?}");
    assert_eq!(
        luau,
        vec![BridgeCommand::SetPosition { entity_id: 7, x: 11.0, y: 22.0 }]
    );
}

#[test]
fn parity_extends_child_overrides_parent() {
    let src = r#"
        script = { extends = "lib/parent" }
        function update(entity, dt)
            set_position(entity, 99, 99)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both_with_resolver(src, &snap, 3, |path| match path {
        "lib/parent" => Ok(r#"
            local function update(entity, dt)
                set_position(entity, 1, 1)
            end
            return { update = update }
        "#
        .into()),
        other => Err(format!("not found: {other}")),
    });
    assert_eq!(luau, js, "override parity\nluau={luau:#?}\njs={js:#?}");
    assert_eq!(
        luau,
        vec![BridgeCommand::SetPosition { entity_id: 3, x: 99.0, y: 99.0 }]
    );
}

#[test]
fn parity_extends_multilevel_chain() {
    let src = r#"
        script = { extends = "lib/parent" }
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both_with_resolver(src, &snap, 5, |path| match path {
        "lib/grandparent" => Ok(r#"
            local function update(entity, dt)
                set_position(entity, 100, 200)
            end
            return { update = update }
        "#
        .into()),
        "lib/parent" => Ok(r#"
            return require("lib/grandparent")
        "#
        .into()),
        other => Err(format!("not found: {other}")),
    });
    assert_eq!(luau, js, "multilevel parity\nluau={luau:#?}\njs={js:#?}");
    assert_eq!(
        luau,
        vec![BridgeCommand::SetPosition { entity_id: 5, x: 100.0, y: 200.0 }]
    );
}
// --- Phase C: targeted cross-script comms parity ----------------------------

#[test]
fn parity_send_event_to_targeted() {
    let src = r#"
        function update(entity, dt)
            send_event_to("hit", entity, 99, "10")
            send_event_to("ping", entity, 99)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both_with_resolver(src, &snap, 7, |_| Err("none".into()));
    assert_eq!(luau, js, "send_event_to parity\nluau={luau:#?}\njs={js:#?}");
    assert_eq!(
        luau,
        vec![
            BridgeCommand::SendEventTo {
                name: "hit".into(),
                sender: 7,
                target: 99,
                data: Some("10".into()),
            },
            BridgeCommand::SendEventTo {
                name: "ping".into(),
                sender: 7,
                target: 99,
                data: None,
            },
        ]
    );
}

#[test]
fn parity_call_with_primitive_args() {
    let src = r#"
        function update(entity, dt)
            call(entity, 42, "damage", 10, "fire", true)
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both_with_resolver(src, &snap, 5, |_| Err("none".into()));
    assert_eq!(luau, js, "call parity\nluau={luau:#?}\njs={js:#?}");
    use crate::scripting::bridge::value::Value as V;
    assert_eq!(
        luau,
        vec![BridgeCommand::CallScriptMethod {
            caller: 5,
            target: 42,
            method: "damage".into(),
            args: vec![V::Number(10.0), V::String("fire".into()), V::Bool(true)],
        }]
    );
}

#[test]
fn parity_call_with_no_args() {
    let src = r#"
        function update(entity, dt)
            call(entity, 7, "tick")
        end
    "#;
    let snap = BridgeReadSnapshot::default();
    let (luau, js) = run_both_with_resolver(src, &snap, 3, |_| Err("none".into()));
    assert_eq!(luau, js, "call no-args parity\nluau={luau:#?}\njs={js:#?}");
    assert_eq!(
        luau,
        vec![BridgeCommand::CallScriptMethod {
            caller: 3,
            target: 7,
            method: "tick".into(),
            args: vec![],
        }]
    );
}
