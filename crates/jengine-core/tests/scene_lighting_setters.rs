//! `Lighting.set_toon(bands, softness, dither)` and
//! `Lighting.set_occlusion(radius, strength)` - the scene-wide lighting
//! settings, which had no setter at all: the showcase's Clay and Toon turned
//! them off by destroying their entity and on by spawning a prefab.
//!
//! Each writes the first component of its kind or spawns one, and "off"
//! (`bands` or `strength` of 0) takes it off every entity, so no second copy
//! is left to take over. Natively through a script, and through the shared
//! arm both browser hosts dispatch through.

#![cfg(feature = "native")]

use hecs::{Entity, World};
use jengine_core::ecs::components::{
    AmbientOcclusion, Name, ScriptComponent, ScriptLanguage, ToonLighting, Transform,
};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
use serde_json::json;

const SRC: &str = r#"
local frames = 0
function update(entity, dt)
    frames = frames + 1
    if frames == 1 then
        Lighting.set_toon(4, 0.5, 2)
        Lighting.set_occlusion(30, 0.4)
    elseif frames == 2 then
        Lighting.set_toon(0)
        Lighting.set_occlusion(nil, 0)
    elseif frames == 3 then
        Lighting.set_toon(2)
        Lighting.set_occlusion()
    end
end
"#;

fn count<T: hecs::Component>(world: &World) -> usize {
    world.query::<&T>().iter().count()
}

fn first<T: hecs::Component + Copy>(world: &World) -> Option<T> {
    world.query::<&T>().iter().next().map(|(_, t)| *t)
}

#[test]
fn scripts_turn_toon_and_occlusion_on_and_off() {
    let dir = std::env::temp_dir().join(format!(
        "jengine_scene_lighting_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("scene_lighting.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    // Two authored copies: "off" must take both, or the second takes over.
    world.spawn((ToonLighting::default(),));
    world.spawn((ToonLighting::default(),));
    world.spawn((
        Name("Driver".into()),
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    let mut step = |world: &mut World| system.run(world, 1.0 / 60.0, &input, None, None);

    step(&mut world);
    assert_eq!(first::<ToonLighting>(&world), Some(ToonLighting { bands: 4, softness: 0.5, dither: 2.0 }));
    assert_eq!(first::<AmbientOcclusion>(&world).map(|a| (a.radius, a.strength)), Some((30.0, 0.4)), "spawned");

    step(&mut world);
    assert_eq!(count::<ToonLighting>(&world), 0, "bands 0 takes every copy off");
    assert_eq!(count::<AmbientOcclusion>(&world), 0, "strength 0 too");

    step(&mut world);
    assert_eq!(first::<ToonLighting>(&world), Some(ToonLighting { bands: 2, ..ToonLighting::default() }), "nil is the default");
    let d = AmbientOcclusion::default();
    assert_eq!(first::<AmbientOcclusion>(&world).map(|a| (a.radius, a.strength)), Some((d.radius, d.strength)));
}

struct Host(World);

impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

#[test]
fn the_browser_hosts_set_them_too() {
    let mut host = Host(World::new());
    // As the preamble sends them: a nil argument is simply absent.
    assert!(handle_world_command(&mut host, "stoon", &JsonArgs::new(json!({ "b": 3, "s": 0.2, "d": 4 }))));
    assert!(handle_world_command(&mut host, "sao", &JsonArgs::new(json!({ "r": 64 }))));
    assert_eq!(first::<ToonLighting>(&host.0), Some(ToonLighting { bands: 3, softness: 0.2, dither: 4.0 }));
    assert_eq!(first::<AmbientOcclusion>(&host.0).map(|a| a.radius), Some(64.0));
    assert_eq!(count::<ToonLighting>(&host.0), 1, "written, not spawned twice");

    assert!(handle_world_command(&mut host, "stoon", &JsonArgs::new(json!({ "b": 0 }))));
    assert!(handle_world_command(&mut host, "sao", &JsonArgs::new(json!({ "s": 0 }))));
    assert_eq!((count::<ToonLighting>(&host.0), count::<AmbientOcclusion>(&host.0)), (0, 0));
}
