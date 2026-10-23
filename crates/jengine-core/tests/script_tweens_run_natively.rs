//! A script's tweens move things on native.
//!
//! The Luau bridge turned `tween_position` / `tween_scale` / `tween_color` /
//! `tween_rotation` into `ScriptCommand::StartTween`, which
//! `apply_script_commands` treats as a no-op (tweens were meant to go
//! through `EngineApi` - which the bridge does not use), and nothing native
//! ticked a tween manager. So no script tween ever did anything natively,
//! while the editor and web builds, which tick their own, animated them
//! (BUGS #107). Found as the arcade monster RPG's battle: every monster slid
//! in with a tween, so on native none ever arrived.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{Name, ScriptComponent, ScriptLanguage, SpriteComponent, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

const SRC: &str = r#"
local started = false
local handle = nil
local cancel_me = nil
function update(entity, dt)
    if not started then
        started = true
        -- Placed first and tweened in the same frame: the tween starts
        -- from where the entity was put, not where it was.
        set_position(entity, 100, 0)
        tween_position(entity, 200, 50, 0.5, "linear")
        tween_scale(entity, 2, 3, 0.5, "linear")
        tween_color(entity, 1, 0, 0, 0.5, 0.5, "linear")
        -- A spawned entity, tweened through the handle spawn_prefab returned.
        handle = spawn_entity("Spawned")
        set_position(handle, 0, 0)
        tween_position(handle, -40, 0, 0.5, "linear")
        cancel_me = tween_rotation(entity, 3, 0.5, "linear")
        cancel_tween(cancel_me)
    end
end
"#;

#[test]
fn tweens_move_position_scale_colour_and_can_be_cancelled() {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_script_tweens_test",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tweens.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let e = world.spawn((
        Name("Mover".into()),
        Transform::default(),
        SpriteComponent::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..60 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }

    let t = (*world.get::<&Transform>(e).unwrap()).clone();
    assert!((t.position.x - 200.0).abs() < 0.5 && (t.position.y - 50.0).abs() < 0.5, "position tween: {:?}", t.position);
    assert!((t.scale.x - 2.0).abs() < 0.01 && (t.scale.y - 3.0).abs() < 0.01, "scale tween: {:?}", t.scale);
    assert_eq!(t.rotation, 0.0, "a cancelled tween does nothing");
    let c = world.get::<&SpriteComponent>(e).unwrap().color;
    assert!((c[1]).abs() < 0.01 && (c[3] - 0.5).abs() < 0.01, "colour tween: {c:?}");

    let spawned = world.query::<(&Name, &Transform)>().iter().find(|(_, (n, _))| n.0 == "Spawned").map(|(_, (_, t))| t.position);
    assert_eq!(spawned.map(|p| p.x.round()), Some(-40.0), "a spawn handle's tween");
}
