//! A callback dispatched after the world has been stepped can still *do*
//! something.
//!
//! Three handlers are dispatched at the end of a script frame, because what
//! they report is only known once the world has moved: `on_entered_trigger`
//! / `on_exited_trigger`, `on_collision_enter` / `on_collision_exit`, and
//! (since Sep 2026) `on_animation_event`. Every command they issued was
//! routed into `api.commands` *after* that frame's apply and then wiped by
//! the next frame's `api.clear()` - so a collision handler that moved
//! something, spawned something or played a sound did nothing at all, while
//! its `log` output appeared exactly as expected, because logging is not a
//! deferred command.
//!
//! The same bug was found and fixed for `on_event` listeners once, by moving
//! their dispatch before the apply. These three cannot move, so the frame
//! applies what they issued at the end instead.
//!
//! Two things had to be true for a payload-bearing callback to work at all,
//! and neither was: the payload also arrived as **nil** in Luau, because the
//! adapter turned every `Map` and `Array` into `LuaValue::Nil`.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{
    CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform,
};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;
use jengine_core::physics::{ContactEvent, ContactType};

const SRC: &str = r#"
function update(entity, dt)
end

function on_collision_enter(entity, ev)
    -- Both halves of the fix: the payload is a table (it was nil), and a
    -- command issued here reaches the world (it was dropped).
    set_custom(entity, "Hit", "other", ev.other)
    set_position(entity, 40, 7)
end
"#;

#[test]
fn a_collision_handler_sees_its_payload_and_can_move_the_entity() {
    let dir = std::env::temp_dir().join(format!(
        // Unique per run: a fixed name here is a race between parallel
        // test binaries and anything still holding the last run's file.
        "{}_{}_{}",
        "jengine_late_callbacks_test",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hit.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let a = world.spawn((
        Name("A".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_string_lossy().into(),
            cull_distance: None,
        },
    ));
    let b = world.spawn((Name("B".into()), Transform::default()));

    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    system.run(&mut world, 1.0 / 60.0, &input, None, None);

    system.queue_contact_events(&[ContactEvent {
        entity_a: a.id() as u64,
        entity_b: b.id() as u64,
        event_type: ContactType::Started,
        impulse: 12.5,
        point: (3.0, 4.0),
    }]);
    system.run(&mut world, 1.0 / 60.0, &input, None, None);

    let moved = world.get::<&Transform>(a).unwrap().position;
    assert_eq!(
        (moved.x, moved.y),
        (40.0, 7.0),
        "the handler's set_position was dropped - late callbacks cannot act"
    );

    let custom = world.get::<&CustomComponents>(a).unwrap();
    let other = custom.0.get("Hit").and_then(|h| h.get("other")).and_then(|v| v.as_f64());
    assert_eq!(other, Some(b.id() as f64), "the payload table was nil in Luau");
}
