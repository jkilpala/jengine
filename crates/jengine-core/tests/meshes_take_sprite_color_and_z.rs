//! `Sprite.set_color`, `Sprite.get_color` and `Sprite.set_z_order` reach an
//! entity whose picture is a `mesh` (Oct 2026).
//!
//! They wrote `SpriteComponent` (and `TextComponent` for the z) only, so a
//! mesh-only entity ignored both: World Table recoloured a territory by owner
//! by resending every vertex through `Mesh.set`, and Clay Gods kept a pool of
//! 31 mesh entities authored at fixed z because a row's z could not move.
//! The browser hosts' half (the colour mirror and the shared `szo` arm) is in
//! `scripting/bridge/adapters/js/mirror.rs` and `host_commands.rs`.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{CustomComponents, Mesh2D, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

const SRC: &str = r#"
local frame = 0
function update(entity, dt)
    frame = frame + 1
    if frame == 1 then
        Sprite.set_color(entity, 0.25, 0.5, 0.75, 1)
        Sprite.set_z_order(entity, 12)
    elseif frame == 2 then
        local r, g, b, a = Sprite.get_color(entity)
        Entity.set_custom(entity, "Read", "g", g)
    end
end
"#;

#[test]
fn a_mesh_takes_the_sprite_colour_and_z() {
    let dir = std::env::temp_dir().join(format!("jengine_mesh_color_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mesh.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        Mesh2D::default(),
        CustomComponents::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);

    let mesh = world.get::<&Mesh2D>(e).unwrap();
    assert_eq!(mesh.color, [0.25, 0.5, 0.75, 1.0], "set_color tints the mesh");
    assert_eq!(mesh.z_order, 12, "set_z_order moves the mesh");
    let custom = world.get::<&CustomComponents>(e).unwrap();
    let g = custom.0.get("Read").and_then(|r| r.get("g")).and_then(|v| v.as_f64());
    assert_eq!(g, Some(0.5), "get_color reads the mesh's tint back");
}
