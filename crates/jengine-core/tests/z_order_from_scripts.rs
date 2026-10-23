//! `set_z_order(entity, z)` changes an entity's draw order at run time -
//! the sprite's and the text's, whichever it has. There was no way to do
//! this from a script: a hand of overlapping cards could not bring the one
//! under the mouse to the front. The browser hosts share one arm
//! (`host_commands`, tested there); this is the native apply.
#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{SpriteComponent, TextComponent};
use jengine_core::ecs::systems::apply_script_commands;
use jengine_core::scripting::api::ScriptCommand;

#[test]
fn native_set_z_order_reaches_sprite_and_text() {
    let mut world = World::new();
    let card = world.spawn((SpriteComponent::default(),));
    let label = world.spawn((TextComponent::default(),));
    apply_script_commands(
        &mut world,
        &[
            ScriptCommand::SetZOrder { entity_id: card.id() as u64, z: 90 },
            ScriptCommand::SetZOrder { entity_id: label.id() as u64, z: 91 },
        ],
        None,
        None,
    );
    assert_eq!(world.get::<&SpriteComponent>(card).unwrap().z_order, 90);
    assert_eq!(world.get::<&TextComponent>(label).unwrap().z_order, 91);
}
