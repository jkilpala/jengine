//! `get_health` reads back what `set_health` writes.
//!
//! `set_health` shipped without a reader: `Health` was absent from the bridge
//! snapshot, and `get_custom` only reaches the CustomComponents bag, not
//! registered components. A script could damage an entity and never show its
//! HP.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{Health, Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

/// Reads its own health and stores it where the test can see it.
const SRC: &str = r#"
function init(entity)
    local hp, max_hp = get_health(entity)
    if hp ~= nil then
        set_custom(entity, "Probe", "hp", hp)
        set_custom(entity, "Probe", "max", max_hp)
    end
end

local damaged = false

function update(entity, dt)
    -- Damage exactly once, then read the new value back on a later frame.
    if not damaged then
        damaged = true
        local hp = get_health(entity)
        set_health(entity, (hp or 0) - 30, 100)
    else
        set_custom(entity, "Probe", "after", get_health(entity) or -1)
    end
end
"#;

#[test]
fn get_health_round_trips_through_the_bridge() {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_health_test",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("health.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let e = world.spawn((
        Name("Victim".into()),
        Transform::default(),
        Health { current: 100.0, max: 100.0, invuln_time: 0.0, invuln_timer: 0.0 },
        // set_custom writes into an existing bag rather than creating one.
        jengine_core::ecs::components::CustomComponents(Default::default()),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_string_lossy().into(),
            cull_distance: None,
        },
    ));

    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..4 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }

    let custom = world
        .get::<&jengine_core::ecs::components::CustomComponents>(e)
        .expect("script should have written its readings");
    let probe = custom.0.get("Probe").expect("Probe bag");
    let num = |k: &str| probe.get(k).and_then(|v| v.as_f64()).unwrap_or(f64::NAN);

    assert_eq!(num("hp"), 100.0, "init should read the authored current");
    assert_eq!(num("max"), 100.0, "init should read the authored max");
    // set_health(70) landed and was read back on a later frame.
    assert_eq!(num("after"), 70.0, "get_health should see what set_health wrote");

    // ...and the world agrees.
    let h = world.get::<&Health>(e).unwrap();
    assert_eq!(h.current, 70.0);
}

#[test]
fn get_health_is_nil_without_a_health_component() {
    // "no Health component" must stay distinguishable from "dead at 0 HP".
    let dir = std::env::temp_dir().join("jengine_health_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("nohealth.lua");
    std::fs::write(
        &path,
        "function init(entity)\n  if get_health(entity) == nil then add_tag(entity, \"nil_health\") end\nend\nfunction update(e, dt) end\n",
    )
    .unwrap();

    let mut world = World::new();
    let e = world.spawn((
        Name("Rock".into()),
        Transform::default(),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_string_lossy().into(),
            cull_distance: None,
        },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    system.run(&mut world, 1.0 / 60.0, &input, None, None);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);

    let tags = world
        .get::<&jengine_core::ecs::components::Tags>(e)
        .expect("Tags");
    assert!(tags.0.iter().any(|t| t == "nil_health"));
}
