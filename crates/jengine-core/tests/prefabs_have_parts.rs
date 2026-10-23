//! **A prefab can be a hierarchy** (Oct 2026; Godot's nested scenes, Unity's
//! prefab hierarchies): `EntityData.children` hangs parts under a prefab - a
//! cart's wheels, a catapult's arm - each with parts of its own, and a part
//! may be a `prefab_instance`, so a prefab is built of prefabs. Every
//! shipped prefab was one entity before it, so a siege engine with turning
//! wheels was a scene's worth of entities the script spawned and parented
//! one at a time. Found by the showcase's Siege Workshop.
//!
//! The parts reach the world four ways, all checked here: a scene instance
//! (flattened into named entities at load), a native `spawn_prefab`, a
//! browser `spawn_prefab` (through the registry) and a behaviour graph's
//! spawn. `Entity.find_child` finds a part by name in every runtime.

use std::collections::HashMap;

use hecs::{Entity, World};
use jengine_core::ecs::components::{Name, Parent, Transform};
use jengine_core::ecs::hierarchy::{find_child, resolve_world_transforms, world_transform_of};
use jengine_core::scene::format::{EntityData, SceneData};
use jengine_core::scene::{expand_prefab_instances, resolve_prefab_variants};

fn prefab(json: serde_json::Value) -> EntityData {
    serde_json::from_value(json).expect("a prefab")
}

fn tr(x: f32, y: f32) -> serde_json::Value {
    serde_json::json!({ "position": [x, y], "rotation": 0, "scale": [1, 1] })
}

/// A cart of two wheels and a mast carrying a flag, the flag a prefab of
/// its own; and a variant of the cart that adds a lamp.
fn workshop() -> HashMap<String, EntityData> {
    let mut m = HashMap::new();
    m.insert("Flag".into(), prefab(serde_json::json!({ "name": "Flag", "components": {
        "transform": tr(0.0, 0.0),
        "sprite": { "texture_id": "flag.png", "color": [1, 0, 0, 1], "z_order": 4, "size": [20, 12] } } })));
    m.insert("Cart".into(), prefab(serde_json::json!({ "name": "Cart", "components": {
        "transform": tr(0.0, 0.0),
        "sprite": { "texture_id": "cart.png", "color": [1, 1, 1, 1], "z_order": 2, "size": [80, 30] } },
        "children": [
            { "name": "Wheel", "components": { "transform": tr(-30.0, -15.0),
                "sprite": { "texture_id": "wheel.png", "color": [1, 1, 1, 1], "z_order": 3, "size": [24, 24] } } },
            { "name": "Wheel", "components": { "transform": tr(30.0, -15.0),
                "sprite": { "texture_id": "wheel.png", "color": [1, 1, 1, 1], "z_order": 3, "size": [24, 24] } } },
            { "name": "Mast", "components": { "transform": tr(0.0, 40.0) }, "children": [
                { "name": "Flag", "components": { "transform": tr(10.0, 20.0),
                    "prefab_instance": { "prefab_id": "Flag", "overrides": { "sprite.color": [0, 0, 1, 1] } } } } ] } ] })));
    m.insert("LampCart".into(), prefab(serde_json::json!({ "name": "LampCart", "components": {
        "prefab_instance": { "prefab_id": "Cart" } },
        "children": [ { "name": "Lamp", "components": { "transform": tr(0.0, 20.0),
            "point_light": { "radius": 90, "color": [1, 0.8, 0.5], "intensity": 1.0, "falloff": 2.0 } } } ] })));
    m
}

fn named(scene: &SceneData, name: &str) -> EntityData {
    scene.entities.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("no '{name}'")).clone()
}


/// The per-frame publish both browser hosts share (`adapters::js::mirror`).
const MIRROR: &str = include_str!("../src/scripting/bridge/adapters/js/mirror.rs");

/// A host's source as far as what it publishes goes: its own, plus the shared
/// mirror - but only if it calls it, so a host that dropped the call fails.
fn with_mirror(src: &str) -> String {
    let mut out = src.to_string();
    if src.contains("mirror::sync_world_to_play_namespace(") {
        out = format!("{out}
{MIRROR}");
    }
    // `spawn_prefab`'s arm, shared since Oct 2026 (debt #2).
    if src.contains("handle_host_command(") {
        out = format!("{out}
{DISPATCH}");
    }
    out
}

/// The command arms both browser hosts share (`adapters::js::host_dispatch`).
const DISPATCH: &str = include_str!("../src/scripting/bridge/adapters/js/host_dispatch.rs");

#[test]
fn a_scene_instance_brings_its_parts_named_under_it() {
    let prefabs = resolve_prefab_variants(&workshop());
    let mut scene: SceneData = serde_json::from_value(serde_json::json!({ "name": "yard", "entities": [
        { "name": "A", "components": { "prefab_instance": { "prefab_id": "Cart",
            "overrides": { "transform.position": [100, 0] } } } },
        { "name": "B", "components": { "prefab_instance": { "prefab_id": "LampCart" } } } ] }))
    .unwrap();
    let report = expand_prefab_instances(&mut scene, &prefabs);
    assert!(report.missing.is_empty(), "{:?}", report.missing);
    // Two wheels of one name under one parent are told apart.
    for n in ["A/Wheel", "A/Wheel#2", "A/Mast", "A/Mast/Flag", "B/Wheel", "B/Wheel#2", "B/Mast/Flag", "B/Lamp"] {
        named(&scene, n);
    }
    assert_eq!(named(&scene, "A/Wheel").parent.as_deref(), Some("A"));
    assert_eq!(named(&scene, "A/Mast/Flag").parent.as_deref(), Some("A/Mast"));
    // The flag is the Flag prefab with the cart's override over it.
    let flag = named(&scene, "A/Mast/Flag").components;
    let sprite = flag.sprite.expect("the Flag prefab's sprite");
    assert_eq!(sprite.texture_id, "flag.png");
    assert_eq!(sprite.color, [0.0, 0.0, 1.0, 1.0], "the part's override");
    assert!(named(&scene, "B/Lamp").components.point_light.is_some(), "the variant's own part");
    assert!(scene.entities.iter().all(|e| e.children.is_empty()), "every part hung out");

    // Loaded, the parts sit in their parent's frame.
    let mut world = World::new();
    let json = serde_json::to_string(&scene).unwrap();
    let loaded: SceneData = serde_json::from_str(&json).unwrap();
    jengine_core::scene::load_scene(&loaded, &mut world);
    resolve_world_transforms(&mut world);
    let a = by_name(&world, "A");
    let wheel = find_child(&world, a, "Wheel").expect("Entity.find_child(A, 'Wheel')");
    assert_eq!(world_transform_of(&world, wheel).unwrap().position.to_array(), [70.0, -15.0]);
    let flag = find_child(&world, a, "Flag").expect("a grandchild, breadth first");
    assert_eq!(find_child(&world, a, "Mast/Flag"), Some(flag), "a path");
    assert_eq!(world_transform_of(&world, flag).unwrap().position.to_array(), [110.0, 60.0]);
    assert_eq!(find_child(&world, a, "Lamp"), None, "B's lamp is not A's");
}

#[test]
fn a_scene_can_override_one_part_without_doubling_it() {
    let prefabs = resolve_prefab_variants(&workshop());
    let mut scene: SceneData = serde_json::from_value(serde_json::json!({ "name": "yard", "entities": [
        { "name": "A", "components": { "prefab_instance": { "prefab_id": "Cart" } } },
        { "name": "A/Wheel#2", "parent": "A", "components": {
            "sprite": { "texture_id": "spoked.png", "color": [1, 1, 1, 1], "z_order": 3, "size": [30, 30] } } } ] }))
    .unwrap();
    expand_prefab_instances(&mut scene, &prefabs);
    let twos: Vec<_> = scene.entities.iter().filter(|e| e.name == "A/Wheel#2").collect();
    assert_eq!(twos.len(), 1, "one second wheel");
    let c = &twos[0].components;
    assert_eq!(c.sprite.as_ref().unwrap().texture_id, "spoked.png", "the scene's sprite");
    assert_eq!(c.transform.as_ref().unwrap().position.to_array(), [30.0, -15.0], "the part's transform");
    // Saved with its parts written out and loaded again: still once each.
    let again = serde_json::to_string(&scene).unwrap();
    let mut scene: SceneData = serde_json::from_str(&again).unwrap();
    expand_prefab_instances(&mut scene, &prefabs);
    for n in ["A/Wheel", "A/Wheel#2", "A/Mast", "A/Mast/Flag"] {
        assert_eq!(scene.entities.iter().filter(|e| e.name == n).count(), 1, "{n} once");
    }
}

/// Each cart named `name` in `world` has two wheels, a mast and a flag of
/// its own.
fn every_cart_has_its_own_parts(world: &World, name_prefix: &str, carts: usize) {
    let roots: Vec<Entity> = world
        .query::<(&Name, Option<&Parent>)>()
        .iter()
        .filter(|(_, (n, p))| p.is_none() && n.0.starts_with(name_prefix))
        .map(|(e, _)| e)
        .collect();
    assert_eq!(roots.len(), carts, "{carts} carts");
    for root in roots {
        let wheels = world.query::<&Parent>().iter().filter(|(e, p)| {
            p.0 == root && world.get::<&Name>(*e).is_ok_and(|n| n.0.ends_with("Wheel") || n.0.ends_with("Wheel#2"))
        }).count();
        assert_eq!(wheels, 2, "two wheels each");
        assert!(find_child(world, root, "Mast/Flag").is_some(), "a flag each");
    }
}

#[test]
fn two_instances_of_one_name_keep_their_own_parts() {
    // Both unnamed, so both take the prefab's name.
    let prefabs = resolve_prefab_variants(&workshop());
    let mut scene: SceneData = serde_json::from_value(serde_json::json!({ "name": "yard", "entities": [
        { "name": "", "components": { "prefab_instance": { "prefab_id": "Cart" } } },
        { "name": "", "components": { "prefab_instance": { "prefab_id": "Cart" } } } ] }))
    .unwrap();
    expand_prefab_instances(&mut scene, &prefabs);
    let mut world = World::new();
    jengine_core::scene::load_scene(&scene, &mut world);
    every_cart_has_its_own_parts(&world, "Cart", 2);
}

#[test]
fn a_v2_scene_expands_its_instances_and_their_parts() {
    // Until Oct 2026 a v2 scene skipped `prefab_instance` altogether.
    let prefabs = resolve_prefab_variants(&workshop());
    let json = serde_json::json!({ "version": 2, "name": "yard", "entities": [
        { "name": "Cart", "components": { "prefab_instance": { "prefab_id": "Cart",
            "overrides": { "transform.position": [100, 0] } } } },
        { "name": "Cart", "components": { "prefab_instance": { "prefab_id": "LampCart" } } },
        { "name": "Cart/Wheel#2", "parent_index": 0, "components": {
            "sprite": { "texture_id": "spoked.png", "color": [1, 1, 1, 1], "z_order": 3, "size": [30, 30] } } } ] })
    .to_string();
    let out = jengine_core::scene::expand_prefab_instances_in_json(&json, &prefabs, "test");
    let registry = core_registry();
    let mut world = World::new();
    jengine_core::scene::load_scene_auto(&out, &mut world, &registry).expect("loads");
    every_cart_has_its_own_parts(&world, "Cart", 2);
    resolve_world_transforms(&mut world);
    let carts: Vec<Entity> = world
        .query::<(&Name, Option<&Parent>)>()
        .iter()
        .filter(|(_, (n, p))| p.is_none() && n.0 == "Cart")
        .map(|(e, _)| e)
        .collect();
    let first = carts.iter().copied().min_by_key(|e| e.id()).unwrap();
    let sprite = world.get::<&jengine_core::ecs::components::SpriteComponent>(first).expect("the prefab's sprite");
    assert_eq!(sprite.texture_id, "cart.png");
    drop(sprite);
    let wheel = find_child(&world, first, "Wheel").unwrap();
    assert_eq!(world_transform_of(&world, wheel).unwrap().position.to_array(), [70.0, -15.0], "in the cart's frame");
    // The authored second wheel was laid over the first cart's, not added.
    let seconds: Vec<Entity> = world
        .query::<(&Name, &Parent)>()
        .iter()
        .filter(|(_, (n, p))| n.0 == "Cart/Wheel#2" && p.0 == first)
        .map(|(e, _)| e)
        .collect();
    assert_eq!(seconds.len(), 1);
    let s = world.get::<&jengine_core::ecs::components::SpriteComponent>(seconds[0]).unwrap();
    assert_eq!(s.texture_id, "spoked.png");
    // The variant's own part.
    let second = carts.iter().copied().max_by_key(|e| e.id()).unwrap();
    assert!(find_child(&world, second, "Lamp").is_some());
}

#[test]
fn resolved_prefabs_carry_their_parts_expanded() {
    let r = resolve_prefab_variants(&workshop());
    let cart = &r["Cart"];
    let flag = &cart.children[2].children[0];
    assert_eq!(flag.components.sprite.as_ref().unwrap().texture_id, "flag.png", "the part is the Flag prefab");
    let lamp = &r["LampCart"];
    let names: Vec<&str> = lamp.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Wheel", "Wheel", "Mast", "Lamp"], "the base's parts, then its own");
}

fn core_registry() -> jengine_core::package::component_registry::ComponentRegistry {
    let mut registry = jengine_core::package::component_registry::ComponentRegistry::new();
    jengine_core::package::core_registration::register_core_components(&mut registry);
    registry
}

fn by_name(world: &World, name: &str) -> Entity {
    world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e).unwrap_or_else(|| panic!("no {name}"))
}

fn parent_of(world: &World, e: Entity) -> Option<Entity> {
    world.get::<&Parent>(e).ok().map(|p| p.0)
}

/// What every spawn path must produce: a cart with two wheels and a mast,
/// the mast carrying a blue flag.
fn check_tree(world: &World, cart: Entity) {
    let wheels: Vec<Entity> = world
        .query::<(&Name, &Parent)>()
        .iter()
        .filter(|(_, (n, p))| n.0 == "Wheel" && p.0 == cart)
        .map(|(e, _)| e)
        .collect();
    assert_eq!(wheels.len(), 2, "two wheels under the cart");
    let mast = find_child(world, cart, "Mast").expect("a mast");
    let flag = find_child(world, mast, "Flag").expect("a flag on it");
    assert_eq!(parent_of(world, flag), Some(mast));
    let sprite = world.get::<&jengine_core::ecs::components::SpriteComponent>(flag).expect("the Flag prefab's sprite");
    assert_eq!(sprite.color, [0.0, 0.0, 1.0, 1.0]);
    assert!(world.get::<&Transform>(mast).is_ok(), "a part with no sprite still has a transform");
}

#[cfg(feature = "native")]
#[test]
fn a_native_spawn_builds_the_whole_tree() {
    use jengine_core::scripting::api::ScriptCommand;
    let prefabs = resolve_prefab_variants(&workshop());
    let cart = &prefabs["Cart"];
    let mut world = World::new();
    jengine_core::ecs::systems::apply_script_commands(
        &mut world,
        &[ScriptCommand::SpawnEntity {
            name: "Cart".into(),
            components: Box::new(Some(cart.components.clone())),
            spawn_id: Some(jengine_core::scripting::spawn_aliases::FIRST_SPAWN_ID + 7),
            children: cart.children.clone(),
        }],
        None,
        None,
    );
    check_tree(&world, by_name(&world, "Cart"));
}

#[test]
fn a_browser_spawn_builds_the_whole_tree_through_the_registry() {
    let prefabs = resolve_prefab_variants(&workshop());
    let cart = &prefabs["Cart"];
    let registry = core_registry();
    let mut world = World::new();
    let root = jengine_core::behavior::command::spawn_entity_data(&mut world, &registry, cart);
    jengine_core::scene::prefab::spawn_children_with_registry(&mut world, &registry, root, &cart.children);
    check_tree(&world, root);
}

#[test]
fn a_behaviour_graph_spawn_builds_the_whole_tree() {
    use jengine_core::behavior::command::{BehaviorCommand, PrefabSource, route_behavior_commands_with};
    let prefabs = resolve_prefab_variants(&workshop());
    let registry = core_registry();
    let mut world = World::new();
    let source = PrefabSource { prefabs: &prefabs, registry: &registry };
    route_behavior_commands_with(
        vec![BehaviorCommand::Spawn {
            prefab_id: "Cart".into(),
            at: glam::Vec2::new(5.0, 5.0),
            rotation: 0.0,
            scale: glam::Vec2::ONE,
            with_velocity: None,
        }],
        &mut world,
        Some(&source),
    );
    check_tree(&world, by_name(&world, "Cart"));
}

#[cfg(feature = "native")]
#[test]
fn a_script_spawns_a_cart_and_finds_its_parts() {
    use jengine_core::ecs::components::{CustomComponents, ScriptComponent, ScriptLanguage};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::input::InputState;
    const SRC: &str = r#"
local cart
local frame = 0
function update(entity, dt)
    frame = frame + 1
    if frame == 1 then
        cart = Entity.spawn_prefab("Cart")
    elseif frame == 2 then
        local flag = Entity.find_child(cart, "Mast/Flag")
        local wheel = Entity.find_child(cart, "Wheel")
        Entity.set_custom(entity, "Probe", "flag", flag ~= nil)
        Entity.set_custom(entity, "Probe", "wheel", wheel ~= nil and wheel ~= cart)
        Entity.set_custom(entity, "Probe", "none", Entity.find_child(cart, "Sail") == nil)
    end
end
"#;
    let dir = std::env::temp_dir().join(format!("jengine_parts_{}_{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("parts.lua");
    std::fs::write(&path, SRC).unwrap();
    let mut world = World::new();
    let e = world.spawn((
        Name("Probe".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    system.api.services.prefabs = resolve_prefab_variants(&workshop());
    let input = InputState::new();
    for _ in 0..3 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    let read = |key: &str| {
        world.get::<&CustomComponents>(e).ok().and_then(|c| c.0.get("Probe").and_then(|p| p.get(key)).cloned())
    };
    assert_eq!(read("flag"), Some(serde_json::json!(true)), "Entity.find_child(cart, 'Mast/Flag')");
    assert_eq!(read("wheel"), Some(serde_json::json!(true)));
    assert_eq!(read("none"), Some(serde_json::json!(true)), "nil for a part it has not");
}

#[test]
fn every_runtime_spawns_the_parts_and_finds_them() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |p: &str| std::fs::read_to_string(root.join(p)).unwrap();
    let preamble = read("crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs");
    assert!(preamble.contains("kids: kids"), "the preamble's spawn_prefab sends the parts");
    assert!(preamble.contains("function find_child("), "the preamble has find_child");
    for host in ["crates/jengine-web/src/script_host.rs", "crates/jengine-editor-viewport/src/lib.rs"] {
        let src = with_mirror(&read(host));
        assert!(src.contains("spawn_prefab_parts("), "{host} drops a prefab's parts");
        assert!(src.contains("publish_kids("), "{host} never publishes the parts for find_child");
    }
    assert!(read("crates/jengine-core/src/scripting/bridge/adapters/luau.rs").contains("\"find_child\""));
    assert!(read("editor/src/lib/lua-runtime.ts").contains("'find_child'"));
}
