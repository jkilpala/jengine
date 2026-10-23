//! `Lighting.set_color`, `set_intensity` and `set_height` reach whichever
//! light an entity carries - point, spot or sun - natively and through the
//! shared arm both browser hosts dispatch through.
//!
//! The colour and intensity setters were copied into each browser host and
//! knew point and spot lights only; a sun (`DirectionalLight2D`, Sep 2026)
//! would have ignored a script turning day to dusk. There was no height
//! setter at all, and a sun's height is how long its shadows are.

#![cfg(feature = "native")]

use hecs::{Entity, World};
use jengine_core::ecs::components::{
    DirectionalLight2D, Name, PointLight2D, ScriptComponent, ScriptLanguage, SpotLight2D, Transform,
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
        for _, name in ipairs({ "Sun", "Lamp" }) do
            local e = Entity.find_by_name(name)
            Lighting.set_color(e, 1.0, 0.5, 0.25)
            Lighting.set_intensity(e, 0.3)
            Lighting.set_height(e, 0.2)
        end
    end
end
"#;

#[test]
fn scripts_set_a_sun_natively() {
    let dir = std::env::temp_dir().join(format!(
        "jengine_light_setters_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("lights.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let sun = world.spawn((Name("Sun".into()), Transform::default(), DirectionalLight2D::default()));
    let lamp = world.spawn((Name("Lamp".into()), Transform::default(), PointLight2D::default()));
    world.spawn((
        Name("Driver".into()),
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..3 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    let s = world.get::<&DirectionalLight2D>(sun).unwrap();
    assert_eq!((s.color, s.intensity, s.height), ([1.0, 0.5, 0.25], 0.3, 0.2));
    let l = world.get::<&PointLight2D>(lamp).unwrap();
    assert_eq!((l.color, l.intensity, l.height), ([1.0, 0.5, 0.25], 0.3, 0.2));
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
fn the_browser_hosts_set_every_kind_of_light() {
    let mut host = Host(World::new());
    let sun = host.0.spawn((Transform::default(), DirectionalLight2D::default()));
    let spot = host.0.spawn((Transform::default(), SpotLight2D::default()));
    for e in [sun, spot] {
        assert!(handle_world_command(&mut host, "slc", &JsonArgs::new(json!({ "e": e.id(), "r": 0.1, "g": 0.2, "b": 0.3 }))));
        assert!(handle_world_command(&mut host, "sli", &JsonArgs::new(json!({ "e": e.id(), "i": 2.0 }))));
        assert!(handle_world_command(&mut host, "slh", &JsonArgs::new(json!({ "e": e.id(), "h": 0.25 }))));
        assert!(handle_world_command(&mut host, "slm", &JsonArgs::new(json!({ "e": e.id(), "m": 6 }))));
    }
    let s = host.0.get::<&DirectionalLight2D>(sun).unwrap();
    assert_eq!((s.color, s.intensity, s.height, s.light_mask), ([0.1, 0.2, 0.3], 2.0, 0.25, 6));
    let p = host.0.get::<&SpotLight2D>(spot).unwrap();
    assert_eq!((p.color, p.intensity, p.height, p.light_mask), ([0.1, 0.2, 0.3], 2.0, 0.25, 6));
}

#[test]
fn the_browser_hosts_set_the_colour_grade() {
    use jengine_core::ecs::components::ColorLut;
    let mut host = Host(World::new());
    // No grade yet: one is made.
    assert!(handle_world_command(&mut host, "slut", &JsonArgs::new(json!({ "id": "luts/warm.png", "s": 0.75 }))));
    let grades: Vec<ColorLut> = host.0.query::<&ColorLut>().iter().map(|(_, l)| l.clone()).collect();
    assert_eq!(grades, vec![ColorLut { texture_id: "luts/warm.png".into(), strength: 0.75 }]);
    // Then it is the one changed, and an empty id takes the grade away.
    assert!(handle_world_command(&mut host, "slut", &JsonArgs::new(json!({ "id": "", "s": 1.0 }))));
    let grades: Vec<ColorLut> = host.0.query::<&ColorLut>().iter().map(|(_, l)| l.clone()).collect();
    assert_eq!(grades.len(), 1);
    assert!(grades[0].texture_id.is_empty());
}
