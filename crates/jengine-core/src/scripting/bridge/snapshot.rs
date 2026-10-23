//! Build a [`BridgeReadSnapshot`] from the live ECS world.
//!
//! This isolates the bridge from `EngineApi`'s wide HashMap surface. During Phase 2 we
//! still walk the world directly; later phases may share or replace this with the
//! existing snapshot machinery.

use hecs::{Entity, World};
use std::collections::HashMap;

use crate::ecs::components::{
    MaterialComponent, Name, SpriteComponent, Tags, Transform, Velocity2D,
};
use crate::input::{InputState, ALL_KEYS};

use super::context::BridgeReadSnapshot;

/// Snapshot the subset of world state the bridge currently exposes.
///
/// Phase 5.2 ships transform + name + tag + sprite + material reads; the
/// snapshot grows as more API surface migrates onto the bridge.
pub fn snapshot_world(world: &World) -> BridgeReadSnapshot {
    let mut snap = BridgeReadSnapshot::default();
    for (entity, transform) in world.query::<&Transform>().iter() {
        let id = entity.id() as u64;
        snap.positions.insert(id, (transform.position.x, transform.position.y));
        snap.rotations.insert(id, transform.rotation);
        snap.scales.insert(id, (transform.scale.x, transform.scale.y));
    }
    for (entity, name) in world.query::<&Name>().iter() {
        let id = entity.id() as u64;
        snap.names.insert(id, name.0.clone());
        // Reverse lookup keeps the lowest entity id when names collide.
        snap.name_to_id.entry(name.0.clone()).or_insert(id);
    }
    for (entity, tags) in world.query::<&Tags>().iter() {
        snap.tags.insert(entity.id() as u64, tags.0.clone());
    }
    for (entity, sprite) in world.query::<&SpriteComponent>().iter() {
        let id = entity.id() as u64;
        snap.sprite_colors.insert(id, sprite.color);
        snap.sprite_anchors.insert(id, sprite.anchor);
    }
    // `Sprite.get_color` on a mesh reads its tint, as `set_color` writes it.
    for (entity, mesh) in world.query::<&crate::ecs::components::Mesh2D>().without::<&SpriteComponent>().iter() {
        snap.sprite_colors.insert(entity.id() as u64, mesh.color);
    }
    for (entity, mat) in world.query::<&MaterialComponent>().iter() {
        snap.materials.insert(entity.id() as u64, mat.material_id.to_string());
    }
    snapshot_velocities(&mut snap, world);
    snapshot_characters(&mut snap, world);
    snap
}

/// Fill `snap.characters` from every `CharacterController2D`. The browser
/// hosts publish the same map, so `get_character` answers alike everywhere.
pub fn snapshot_characters(snap: &mut BridgeReadSnapshot, world: &World) {
    for (entity, cc) in world.query::<&crate::ecs::components::CharacterController2D>().iter() {
        snap.characters.insert(entity.id() as u64, cc.clone());
    }
}

/// Fill `snap.velocities` from each entity's `Velocity2D`.
///
/// Split out of [`snapshot_world`] so the browser hosts can publish exactly
/// what native scripts read without building the whole snapshot: call this,
/// then [`snapshot_physics`] to overlay rapier's authoritative velocities for
/// rigid bodies. Until Sep 2026 neither browser host published velocities at
/// all, so `get_velocity` and `get_angular_velocity` answered `(0, 0)` and `0`
/// for every entity in the editor and on the web (BUGS #91).
pub fn snapshot_velocities(snap: &mut BridgeReadSnapshot, world: &World) {
    let mut query = world.query::<&Velocity2D>();
    let iter = query.iter();
    // Reserved up front: grown from empty, ten thousand moving lanterns
    // rehashed the table a dozen times a frame.
    snap.velocities.reserve(iter.len());
    for (entity, vel) in iter {
        snap.velocities.insert(entity.id() as u64, (vel.x, vel.y));
    }
}

/// Overwrite the linear/angular velocity entries on `snap` from the live
/// physics world. Used when the `physics` feature is active so scripts see
/// rapier's authoritative velocities instead of the cached `Velocity2D`
/// component (which only mirrors them after `sync_transforms_to_ecs`).
#[cfg(feature = "physics")]
pub fn snapshot_physics(
    snap: &mut BridgeReadSnapshot,
    world: &World,
    physics: &crate::physics::PhysicsWorld,
) {
    use crate::ecs::components::RigidBody2D;
    for (entity, _rb) in world.query::<&RigidBody2D>().iter() {
        let id = entity.id() as u64;
        if let Some((vx, vy)) = physics.get_velocity(id) {
            snap.velocities.insert(id, (vx, vy));
        }
        if let Some(omega) = physics.get_angular_velocity(id) {
            snap.angular_velocities.insert(id, omega);
        }
    }
}

/// Append `events` (typically `physics.contact_events()` from the last step)
/// to the snapshot's `contacts` field. Kept separate from `snapshot_world` so
/// the engine can own the contact buffer.
pub fn snapshot_contacts(snap: &mut BridgeReadSnapshot, events: &[crate::physics::ContactEvent]) {
    snap.contacts.clear();
    snap.contacts.extend(events.iter().map(|e| {
        (
            e.entity_a,
            e.entity_b,
            matches!(e.event_type, crate::physics::ContactType::Started),
            e.impulse,
            e.point.0,
            e.point.1,
        )
    }));
}

/// Populate the input-related fields on `snap` from the given [`InputState`].
///
/// Phase 5.3: callers (the engine frame loop) invoke `snapshot_world` then
/// `snapshot_input` so the bridge sees a consistent read view for the frame.
pub fn snapshot_input(snap: &mut BridgeReadSnapshot, input: &InputState) {
    snap.keys_pressed.clear();
    snap.keys_just_pressed.clear();
    for &key in ALL_KEYS {
        if input.is_pressed(key) {
            snap.keys_pressed.insert(key.name().to_string());
        }
        if input.is_just_pressed(key) {
            snap.keys_just_pressed.insert(key.name().to_string());
        }
    }
    snap.actions_pressed.clear();
    snap.actions_just_pressed.clear();
    snap.actions_just_released.clear();
    snap.action_bindings.clear();
    snap.action_dead_zones.clear();
    snap.action_axes.clear();
    for (action_name, keys) in input.get_actions() {
        if input.is_action_pressed(action_name) {
            snap.actions_pressed.insert(action_name.clone());
        }
        if input.is_action_just_pressed(action_name) {
            snap.actions_just_pressed.insert(action_name.clone());
        }
        if input.is_action_just_released(action_name) {
            snap.actions_just_released.insert(action_name.clone());
        }
        // 2.5: surface current bindings + dead-zones so scripts can
        // build remap UIs without re-borrowing the live InputState.
        snap.action_bindings.insert(action_name.clone(), keys.clone());
        let dz = input.get_dead_zone(action_name);
        if dz != 0.0 {
            snap.action_dead_zones.insert(action_name.clone(), dz);
        }
        // Every action, including the ones reading zero: a script asking for an
        // axis wants a number back, and an action released this frame has to
        // stop reporting the value it had.
        snap.action_axes
            .insert(action_name.clone(), input.action_axis_value(action_name));
    }
    // Gamepads. Disconnected slots are kept so a script's pad index stays
    // stable across a reconnect.
    snap.pads.clear();
    for index in 0..input.pad_count() {
        let mut pad = crate::scripting::bridge::context::PadSnapshot {
            connected: input.is_pad_connected(index),
            ..Default::default()
        };
        if let Some(state) = input.pad_state(index) {
            pad.axes = state.axes.clone();
            pad.buttons = state.pressed.clone();
        }
        snap.pads.push(pad);
    }
    snap.mouse_position = input.mouse_position;
    snap.screen_size = input.screen_size;
    snap.mouse_left_just_pressed = input.mouse_left_just_pressed;
    snap.mouse_left = input.mouse_left;
    snap.mouse_left_just_released = input.mouse_left_just_released;
    snap.mouse_right_just_pressed = input.mouse_right_just_pressed;
    snap.mouse_right = input.mouse_right;
    snap.mouse_right_just_released = input.mouse_right_just_released;
    snap.mouse_middle_just_pressed = input.mouse_middle_just_pressed;
    snap.mouse_middle = input.mouse_middle;
    snap.mouse_middle_just_released = input.mouse_middle_just_released;
    snap.just_pressed_bindings = input.just_pressed_bindings();
    snap.typed = input.typed_chars().iter().collect();
    snap.input_view = crate::input::interactions::InputView::of(input);
    snap.mouse_wheel_delta = input.get_mouse_wheel();
    snap.mouse_delta = input.mouse_delta;
    snap.touches.clear();
    snap.touches.extend_from_slice(&input.touches);
    snap.gestures.clear();
    snap.gestures.extend_from_slice(&input.gestures);
}

/// Phase 5.13: copy each entity's `CustomComponents` data into the
/// snapshot so scripts can call `get_custom(eid, comp, field)` against the
/// per-frame read view. The data is a deep clone of the JSON map; cheap for
/// the small handful of fields a typical script uses.
///
/// **Hot-path callers** should prefer [`snapshot_custom_components_borrow`]
/// instead — it sets `world_ptr` + `entity_handles` so reads borrow the live
/// world (no per-frame JSON cloning). This owned-clone form is retained for
/// tests and the parity harness.
pub fn snapshot_custom_components(snap: &mut BridgeReadSnapshot, world: &World) {
    use crate::ecs::components::CustomComponents;
    snap.custom_components.clear();
    for (entity, comps) in world.query::<&CustomComponents>().iter() {
        snap.custom_components
            .insert(entity.id() as u64, comps.0.clone());
    }
}

/// Hot-path equivalent of [`snapshot_custom_components`] that avoids cloning.
/// Sets `snap.world_ptr` + populates `snap.entity_handles` so the bridge's
/// `get_custom_field` can borrow component data directly from the live ECS
/// world. Caller is responsible for calling this BEFORE the bridge runs and
/// clearing `world_ptr` after the world is mutated.
pub fn snapshot_custom_components_borrow(snap: &mut BridgeReadSnapshot, world: &World) {
    snap.custom_components.clear();
    snap.entity_handles.clear();
    snap.world_ptr = Some(world as *const _);
    for entity in world.iter().map(|e| e.entity()) {
        snap.entity_handles.insert(entity.id() as u64, entity);
    }
    snap.entity_handles.extend(crate::scripting::spawn_aliases::live(world));
}

/// Hot-path replacement for [`snapshot_world`] that avoids the per-entity
/// HashMap fills for transforms, sprites, materials, velocities, names, and
/// tags. Instead it sets `world_ptr` + `entity_handles` once and lets the
/// `BridgeReadSnapshot::get_*` accessors read live components from the world.
///
/// Still populates `name_to_id` (small reverse-lookup map used by
/// `find_by_name`) and the borrow handles. The returned snapshot is the same
/// shape as [`snapshot_world`] — just with the per-entity bulk maps left
/// empty. Callers should follow up with `snapshot_input` and any other
/// per-frame populators (physics, contacts, etc.) as before.
///
/// Used by the production engine frame loop. Tests that build snapshots by
/// hand still call [`snapshot_world`] (and other eager populators) so they
/// don't need a live world.
pub fn snapshot_world_borrow(world: &World) -> BridgeReadSnapshot {
    snapshot_world_borrow_into(world, HashMap::new())
}

/// [`snapshot_world_borrow`] filling a map the caller kept from last frame.
///
/// The `id -> Entity` index is the one thing every consumer of a frame
/// needs - the bridge accessors, the command applier, the tween ticker, the
/// despawn diff - and each used to walk the world and build its own. This is
/// the single build: `ScriptSystem` hands the map back in at the start of a
/// frame and takes it out at the end, so the allocation lives for the
/// session and the walk happens once.
pub fn snapshot_world_borrow_into(
    world: &World,
    mut entity_handles: HashMap<u64, Entity>,
) -> BridgeReadSnapshot {
    entity_handles.clear();
    entity_handles.reserve(world.len() as usize);
    let mut snap = BridgeReadSnapshot {
        world_ptr: Some(world as *const _),
        entity_handles,
        ..Default::default()
    };
    for entity in world.iter().map(|e| e.entity()) {
        snap.entity_handles.insert(entity.id() as u64, entity);
    }
    // A handle `spawn_prefab` returned in an earlier frame reads the entity
    // it became (BUGS #105).
    snap.entity_handles.extend(crate::scripting::spawn_aliases::live(world));
    // Names are read live through `entity_by_name`; the `name_to_id` map is
    // no longer filled here (it cloned every name every frame).
    snapshot_characters(&mut snap, world);
    snap
}

/// Populate `tilemap_snapshots` and `tilemap_transforms` on `snap` from the
/// live ECS world. Must be called after [`snapshot_world_borrow`] so that
/// `tilemap_get_tile` / `tilemap_get_tile_at_world` work correctly.
pub fn snapshot_tilemaps(snap: &mut BridgeReadSnapshot, world: &World) {
    use crate::ecs::components::Transform;
    use crate::tilemap::Tilemap;
    snap.tilemap_snapshots.clear();
    snap.tilemap_transforms.clear();
    for (entity, (tilemap, transform)) in world.query::<(&Tilemap, &Transform)>().iter() {
        let id = entity.id() as u64;
        snap.tilemap_snapshots.insert(id, tilemap.clone());
        let origin = crate::tilemap::world_origin(world, entity, transform);
        snap.tilemap_transforms.insert(id, (origin.x, origin.y));
    }
}

/// Populate `gui_node_props` from the currently-loaded GUI documents
/// referenced by entities with a [`GuiComponent`]. Each readable widget
/// property (Text.text, Button.label, Slider.value, Toggle.checked,
/// ProgressBar.value, TextInput.text, NodeStyle.visible) is stored as a
/// stringified JSON scalar so `gui_get` returns a value usable from Lua/JS.
///
/// Must be called after `assets` has resolved any new GUI documents for the
/// frame so the snapshot reflects what the renderer will draw.
///
/// **Not called per frame any more.** `gui_get` reads the document live
/// through [`BridgeReadSnapshot::gui_prop`]; this fill built three `String`s
/// per property per node per frame for it. It stays for a test that builds a
/// snapshot by hand with no assets to lend.
pub fn snapshot_gui_props(
    snap: &mut BridgeReadSnapshot,
    world: &World,
    assets: &crate::asset::AssetManager,
) {
    use crate::ecs::components::GuiComponent;
    use crate::ui::gui_document::GuiNode;

    snap.gui_node_props.clear();

    fn walk(gui_id: &str, node: &GuiNode, out: &mut std::collections::HashMap<(String, String, String), String>) {
        for prop in GUI_NODE_PROPS {
            if let Some(value) = gui_node_prop(node, prop) {
                out.insert((gui_id.to_string(), node.id.clone(), prop.to_string()), value);
            }
        }
        for child in &node.children {
            walk(gui_id, child, out);
        }
    }

    // De-dup gui_ids: the same document can be referenced by many entities,
    // but we only need to walk it once per frame.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, gui) in world.query::<&GuiComponent>().iter() {
        if !seen.insert(gui.gui_id.clone()) {
            continue;
        }
        if let Some(doc) = assets.get_gui(&gui.gui_id) {
            walk(&gui.gui_id, &doc.root, &mut snap.gui_node_props);
        }
    }
}

/// The readable properties [`gui_node_prop`] answers.
pub const GUI_NODE_PROPS: &[&str] = &["visible", "scroll_x", "scroll_y", "text", "label", "value", "checked"];

/// One readable widget property of `node`, stringified the way `gui_get`
/// hands it to a script: the style's `visible` / `scroll_x` / `scroll_y`,
/// then the widget's own `text` (a Button's label answers to both `text`
/// and `label`), `value` or `checked`. `None` when the node has no such
/// property.
///
/// The one place the mapping lives: the live `gui_get` reads through it, and
/// so does the eager fill a hand-built test snapshot uses.
pub fn gui_node_prop(node: &crate::ui::gui_document::GuiNode, prop: &str) -> Option<String> {
    use crate::ui::gui_document::GuiNodeType;
    match prop {
        "visible" => return Some(node.style.visible.to_string()),
        "scroll_x" => return Some(node.style.scroll_x.to_string()),
        "scroll_y" => return Some(node.style.scroll_y.to_string()),
        _ => {}
    }
    match (&node.node_type, prop) {
        (GuiNodeType::Text { text, .. }, "text") => Some(text.clone()),
        (GuiNodeType::Button { label, .. }, "text" | "label") => Some(label.clone()),
        (GuiNodeType::ProgressBar { value, .. }, "value") => Some(value.to_string()),
        (GuiNodeType::Slider { value, .. }, "value") => Some(value.to_string()),
        (GuiNodeType::Toggle { checked, .. }, "checked") => Some(checked.to_string()),
        (GuiNodeType::TextInput { text, .. }, "text") => Some(text.clone()),
        // Enter pressed this frame, the caret, the selected text.
        (GuiNodeType::TextInput { edit, .. }, "submitted") => Some(edit.submitted.to_string()),
        (GuiNodeType::TextInput { edit, .. }, "caret") => Some(edit.caret.to_string()),
        (GuiNodeType::TextInput { text, edit, .. }, "selected") => Some(edit.selected(text).to_string()),
        _ => None,
    }
}
