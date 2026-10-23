//! **A script can give a sprite a normal map, and take it away.**
//!
//! `MaterialComponent::normal_map_id` had no setter, so a game could not turn
//! normal maps off for a quality setting, or swap a surface's bumps - the
//! showcase's Crypt authored every surface twice and faded one copy out to
//! compare them. `Material.set_normal_map(e, id)` sets it; `nil` clears it.
//!
//! This runs a Luau script through the real native script system. The
//! browser hosts go through the shared `snm` arm in `host_commands`, tested
//! there with `JsonArgs`; the name reaching the editor is
//! `script_global_parity`'s job.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{MaterialComponent, ScriptComponent, ScriptLanguage, SpriteComponent};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

const SRC: &str = r#"
local frame = 0
function update(entity, dt)
    frame = frame + 1
    if frame == 1 then Material.set_normal_map(entity, "sprites/bricks_n.png") end
    if frame == 2 then Material.set_normal_map(entity, nil) end
end
"#;

#[test]
fn a_script_sets_and_clears_a_normal_map() {
    let dir = std::env::temp_dir().join(format!("jengine_normal_maps_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bumps.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let e = world.spawn((
        SpriteComponent::default(),
        MaterialComponent { material_id: "lit_sprite".into(), ..Default::default() },
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut scripts = ScriptSystem::new().expect("script system");
    let mut input = InputState::new();
    let normal = |world: &World| world.get::<&MaterialComponent>(e).unwrap().normal_map_id.clone();

    input.begin_frame();
    scripts.run(&mut world, 1.0 / 60.0, &input, None, None);
    assert_eq!(normal(&world).as_deref(), Some("sprites/bricks_n.png"));
    assert_eq!(world.get::<&MaterialComponent>(e).unwrap().material_id, "lit_sprite", "the material was replaced");

    input.begin_frame();
    scripts.run(&mut world, 1.0 / 60.0, &input, None, None);
    assert_eq!(normal(&world), None, "nil should take the normal map away");
}
