//! **A fog of war can be reset and reconfigured from a script** (Oct 2026;
//! `FogOfWar.reset` / `FogOfWar.set`, `ecs::fog_of_war::reset_fog` /
//! `set_fog_of_war_field`). Before them a game playing a second run on the
//! same scene kept the first run's explored map, and two RPG classics wrote
//! their own fog rather than use the component. Both go through the shared
//! browser command (`fwrs` / `fwsf`) and the native apply arm; this drives
//! each.

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::{CustomComponents, SpriteComponent, Transform};
use jengine_core::ecs::fog_of_war::{FogOfWar, Sight};
use jengine_core::ecs::systems::apply_script_commands;
use jengine_core::scripting::api::ScriptCommand;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};

struct Host(World);
impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

fn explored(world: &World, fog: Entity) -> f64 {
    world.get::<&CustomComponents>(fog).unwrap().0["FogOfWar"]["explored"].as_f64().unwrap()
}

fn explored_cells(world: &World, fog: Entity) -> usize {
    world.get::<&FogOfWar>(fog).unwrap().state.explored.iter().filter(|&&e| e > 0.0).count()
}

/// A fog with a scout that has walked about in it, so part of it is explored.
fn scouted() -> (World, Entity) {
    let mut world = World::new();
    let fog = world.spawn((
        Transform::default(),
        FogOfWar { size: [400.0, 200.0], cell: 20.0, reveal_speed: 10.0, ..Default::default() },
        SpriteComponent { texture_id: "".into(), size: Some([400.0, 200.0]), ..Default::default() },
    ));
    world.spawn((Transform { position: Vec2::new(-120.0, 0.0), ..Default::default() }, Sight { radius: 70.0, soft: 20.0 }));
    for _ in 0..60 {
        jengine_core::ecs::behaviors::tick_motion(&mut world, 1.0 / 60.0, None, false);
    }
    assert!(explored_cells(&world, fog) > 0 && explored(&world, fog) > 0.0, "the scout explored something");
    (world, fog)
}

#[test]
fn fog_of_war_resets_through_the_shared_command() {
    let (world, fog) = scouted();
    let id = fog.id() as u64;
    let mut host = Host(world);
    // A picture made before the reset, to see it remade after.
    let mut assets = AssetManager::new();
    jengine_core::ecs::fog_of_war::sync(&mut host.0, &mut assets);
    let tex_id = host.0.get::<&SpriteComponent>(fog).unwrap().texture_id.as_str().to_string();
    let clear_before = assets.get_texture_bytes(&tex_id).unwrap().rgba.chunks(4).filter(|p| p[3] < 40).count();
    assert!(clear_before > 0, "a clear patch where the scout is");

    // Move the scout off the map, so nothing reveals it again this frame.
    let scouts: Vec<Entity> = host.0.query::<&Sight>().iter().map(|(e, _)| e).collect();
    for s in scouts {
        host.0.get::<&mut Transform>(s).unwrap().position.x = 10_000.0;
    }
    assert!(handle_world_command(&mut host, "fwrs", &JsonArgs::new(serde_json::json!({ "e": id }))));
    assert_eq!(explored_cells(&host.0, fog), 0, "nothing explored after a reset");
    let f = host.0.get::<&FogOfWar>(fog).unwrap().state.visible.iter().copied().fold(0.0f32, f32::max);
    assert_eq!(f, 0.0, "nothing visible after a reset");
    assert_eq!(explored(&host.0, fog), 0.0, "the script's explored share reads 0 at once");

    // The picture is remade at once, whatever max_rate says: dark everywhere.
    jengine_core::ecs::fog_of_war::sync(&mut host.0, &mut assets);
    let clear_after = assets.get_texture_bytes(&tex_id).unwrap().rgba.chunks(4).filter(|p| p[3] < 200).count();
    assert_eq!(clear_after, 0, "the whole picture dark again");

    // And it stays unseen as the world ticks on.
    for _ in 0..10 {
        jengine_core::ecs::behaviors::tick_motion(&mut host.0, 1.0 / 60.0, None, false);
    }
    assert_eq!(explored_cells(&host.0, fog), 0);
}

#[test]
fn fog_of_war_fields_are_set_by_name() {
    let (world, fog) = scouted();
    let id = fog.id() as u64;
    let cells_before = explored_cells(&world, fog);
    let mut host = Host(world);

    // Through the shared command: a field that keeps the grid keeps what was seen.
    assert!(handle_world_command(
        &mut host,
        "fwsf",
        &JsonArgs::new(serde_json::json!({ "e": id, "f": "explored_alpha", "v": 0.2 }))
    ));
    {
        let f = host.0.get::<&FogOfWar>(fog).unwrap();
        assert!((f.explored_alpha - 0.2).abs() < 1e-6, "explored_alpha set");
    }
    assert_eq!(explored_cells(&host.0, fog), cells_before, "what was seen is kept");
    handle_world_command(
        &mut host,
        "fwsf",
        &JsonArgs::new(serde_json::json!({ "e": id, "f": "unseen_color", "v": [0.5, 0.0, 0.0, 1.0] })),
    );
    assert_eq!(host.0.get::<&FogOfWar>(fog).unwrap().unseen_color, [0.5, 0.0, 0.0, 1.0]);

    // An unknown field or a wrong type changes nothing.
    handle_world_command(&mut host, "fwsf", &JsonArgs::new(serde_json::json!({ "e": id, "f": "tide", "v": 1 })));
    handle_world_command(&mut host, "fwsf", &JsonArgs::new(serde_json::json!({ "e": id, "f": "cell", "v": "big" })));
    assert_eq!(host.0.get::<&FogOfWar>(fog).unwrap().cell, 20.0);
    assert!(jengine_core::ecs::fog_of_war::set_fog_of_war_field(&host.0, fog, "tide", &serde_json::json!(1)).is_err());

    // Natively, through the apply arm: a new cell size is a new, unseen grid.
    let mut world = host.0;
    apply_script_commands(
        &mut world,
        &[ScriptCommand::FogOfWarSet { entity_id: id, field: "cell".into(), value: serde_json::json!(40.0) }],
        None,
        None,
    );
    {
        let f = world.get::<&FogOfWar>(fog).unwrap();
        assert_eq!(f.cell, 40.0);
        assert_eq!(f.state.dims, (10, 5), "the grid follows the new cell");
    }
    assert_eq!(explored_cells(&world, fog), 0, "a new grid is unseen");

    // And the native reset arm.
    for _ in 0..30 {
        jengine_core::ecs::behaviors::tick_motion(&mut world, 1.0 / 60.0, None, false);
    }
    assert!(explored_cells(&world, fog) > 0);
    let scouts: Vec<Entity> = world.query::<&Sight>().iter().map(|(e, _)| e).collect();
    for s in scouts {
        world.get::<&mut Transform>(s).unwrap().position.x = 10_000.0;
    }
    apply_script_commands(&mut world, &[ScriptCommand::FogOfWarReset { entity_id: id }], None, None);
    assert_eq!(explored_cells(&world, fog), 0, "reset natively too");
}
