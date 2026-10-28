//! What a browser host publishes into `window.__jengine_play` for the
//! preamble's getters to read, when publishing it is expensive enough to
//! want a cache.
//!
//! The editor viewport and the web runtime each mirrored every tilemap in
//! the world into `__jengine_play.tilemaps` **every frame**, pushing every
//! tile into a fresh `js_sys::Array` one `push` at a time - one wasm-to-JS
//! call per tile per frame, for the one getter (`get_tile`) that reads a
//! single cell. A 1000 x 1000 map was a million FFI calls a frame in the
//! runtime people develop in. Both copies of that loop were byte-identical,
//! which is the usual shape here: one implementation, kept in core, is the
//! only version there is.
//!
//! The cache is keyed by the map's [`Tilemap::revision_key`], so a map that
//! did not change hands back the JS object it built last time, and one that
//! did is rebuilt from a typed array in one call. The preamble indexes
//! `layer.tiles[y * width + x]` and treats `0` as empty, which is what the
//! typed array holds for an empty cell.

use std::cell::RefCell;
use std::collections::HashMap;

use hecs::{Entity, World};
use wasm_bindgen::JsValue;

use crate::tilemap::Tilemap;

thread_local! {
    /// The JS object published for each tilemap entity, and the revision it
    /// was built from.
    static PUBLISHED: RefCell<HashMap<Entity, (u64, js_sys::Object)>> = RefCell::new(HashMap::new());
}

/// Set `play.tilemaps` to an object mapping each tilemap entity's id (and its
/// script alias from `reverse_map`, when it has one) to the map's JS mirror,
/// rebuilding only the mirrors whose revision moved.
pub fn publish_tilemaps(world: &World, reverse_map: &HashMap<Entity, u32>, play: &js_sys::Object) {
    let tilemaps_obj = js_sys::Object::new();
    PUBLISHED.with(|cache| {
        let mut cache = cache.borrow_mut();
        let mut seen: Vec<Entity> = Vec::new();
        for (entity, tilemap) in world.query::<&Tilemap>().iter() {
            seen.push(entity);
            let rev = tilemap.revision_key();
            let tm_obj = match cache.get(&entity) {
                Some((built_from, obj)) if *built_from == rev => obj.clone(),
                _ => {
                    let obj = tilemap_to_js(tilemap);
                    cache.insert(entity, (rev, obj.clone()));
                    obj
                }
            };
            let _ = js_sys::Reflect::set(&tilemaps_obj, &JsValue::from_str(&entity.id().to_string()), &tm_obj);
            if let Some(idx) = reverse_map.get(&entity) {
                let _ = js_sys::Reflect::set(&tilemaps_obj, &JsValue::from_str(&idx.to_string()), &tm_obj);
            }
        }
        // A map whose entity is gone must not keep its mirror alive.
        cache.retain(|e, _| seen.contains(e));
    });
    let _ = js_sys::Reflect::set(play, &JsValue::from_str("tilemaps"), &tilemaps_obj);
}

thread_local! {
    /// The `kids` object last published, and the hierarchy it was built from.
    static KIDS: RefCell<(u64, Option<js_sys::Object>)> = const { RefCell::new((0, None)) };
}

/// Set `play.kids` - `{ parent: [child, part name, child, part name, ...] }`,
/// each parent under its id and its script alias, each child as the id a
/// script knows it by - for the preamble's `find_child`. Rebuilt only when
/// the hierarchy or a name in it changed (a hash of both), since most
/// frames move nothing.
pub fn publish_kids(world: &World, reverse_map: &HashMap<Entity, u32>, play: &js_sys::Object) {
    use std::hash::{Hash, Hasher};
    let mut links: Vec<(Entity, Entity, &str)> = Vec::new();
    let mut query = world.query::<(&crate::ecs::components::Parent, &crate::ecs::components::Name)>();
    for (e, (p, n)) in query.iter() {
        links.push((p.0, e, crate::ecs::hierarchy::part_name(&n.0)));
    }
    links.sort_by_key(|(p, e, _)| (p.id(), e.id()));
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (p, e, n) in &links {
        (p.to_bits(), e.to_bits(), n, reverse_map.get(p), reverse_map.get(e)).hash(&mut h);
    }
    let key = h.finish() | 1;
    let obj = KIDS.with(|k| {
        let mut k = k.borrow_mut();
        if k.0 == key
            && let Some(obj) = &k.1
        {
            return obj.clone();
        }
        let obj = js_sys::Object::new();
        let id_of = |e: &Entity| reverse_map.get(e).map(|i| *i as f64).unwrap_or(e.id() as f64);
        let mut i = 0;
        while i < links.len() {
            let parent = links[i].0;
            let list = js_sys::Array::new();
            while i < links.len() && links[i].0 == parent {
                list.push(&JsValue::from_f64(id_of(&links[i].1)));
                list.push(&JsValue::from_str(links[i].2));
                i += 1;
            }
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(&parent.id().to_string()), &list);
            if let Some(idx) = reverse_map.get(&parent) {
                let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(&idx.to_string()), &list);
            }
        }
        *k = (key, Some(obj.clone()));
        obj
    });
    let _ = js_sys::Reflect::set(play, &JsValue::from_str("kids"), &obj);
}

thread_local! {
    /// The `names` object last built, and the hash of what it was built from.
    static NAMES: RefCell<(u64, Option<js_sys::Object>)> = const { RefCell::new((0, None)) };
}

/// Set `play.names` - every `Name` to its entity's id, or the spawn handle a
/// script holds for it - for `Entity.find_by_name`, building the object only
/// when a name or what it names changed.
///
/// Both hosts built it every frame, one JS string per named entity, and a
/// prefab's instances all carry its name: Ten Thousand Lights' lanterns made
/// ten thousand string conversions a frame, 13% of the frame in a web build.
///
/// The check is cheap on purpose - hashing the names, or looking each entity
/// up for its handle, was still 7%. It mixes each entity's bits with its
/// name's buffer and length, and `spawn_generation`
/// ([`SpawnMap::generation`](crate::scripting::spawn_map::SpawnMap::generation))
/// stands for every handle at once. A renamed entity gets a new buffer,
/// allocated while the old one is still held, so its address moves. As
/// before, a name carried by several entities answers the last one.
pub fn publish_names(world: &World, reverse_map: &HashMap<Entity, u32>, spawn_generation: u64, play: &js_sys::Object) {
    let mut key = 0xcbf2_9ce4_8422_2325u64;
    let mut mix = |v: u64| {
        key ^= v;
        key = key.wrapping_mul(0x0000_0100_0000_01b3);
    };
    mix(spawn_generation);
    let mut query = world.query::<&crate::ecs::components::Name>();
    for (e, n) in query.iter() {
        mix(e.to_bits().get());
        mix(n.0.as_ptr() as u64);
        mix(n.0.len() as u64);
    }
    let key = key | 1;
    let obj = NAMES.with(|c| {
        let mut c = c.borrow_mut();
        if c.0 == key
            && let Some(obj) = &c.1
        {
            return obj.clone();
        }
        let obj = js_sys::Object::new();
        for (e, n) in query.iter() {
            let id = reverse_map.get(&e).map(|i| *i as f64).unwrap_or(e.id() as f64);
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(&n.0), &JsValue::from_f64(id));
        }
        *c = (key, Some(obj.clone()));
        obj
    });
    let _ = js_sys::Reflect::set(play, &JsValue::from_str("names"), &obj);
}

/// `state.velocities` and `state.angular_velocities` for `get_velocity` /
/// `get_angular_velocity`: `Float32Array`s indexed by entity id, as the
/// mirror's positions are - `[vx, vy]` at `id * 2` and `omega` at `id`, NaN
/// where an entity has none - so the getter resolves a spawn handle with
/// `__ent` and reads one slot. Each body reads as native's snapshot does: its
/// `Velocity2D`, with rapier's value over it for a rigid body.
///
/// They were a JS array per entity, then flat `[id, vx, vy, ...]` rows built
/// from a `HashMap` of every velocity each frame and scanned row by row by
/// every call - with ten thousand moving lanterns the map alone was 6% of a
/// frame, and a script asking each lantern its speed was quadratic. Now the
/// buffers are reused and filled straight from the world, and the JS arrays
/// are refilled in place unless the highest id grew.
pub fn publish_velocities(
    state: &JsValue,
    world: &World,
    #[cfg_attr(not(feature = "physics"), allow(unused_variables))] physics: Option<&crate::physics::PhysicsWorld>,
) {
    VELOCITIES.with(|v| {
        let v = &mut *v.borrow_mut();
        let max_id = world
            .query::<&crate::ecs::components::Velocity2D>()
            .iter()
            .map(|(e, _)| e.id())
            .chain(world.query::<&crate::ecs::components::RigidBody2D>().iter().map(|(e, _)| e.id()))
            .max();
        let slots = max_id.map_or(0, |m| m as usize + 1);
        v.linear.clear();
        v.linear.resize(slots * 2, f32::NAN);
        v.angular.clear();
        v.angular.resize(slots, f32::NAN);
        for (e, vel) in world.query::<&crate::ecs::components::Velocity2D>().iter() {
            let i = e.id() as usize;
            v.linear[i * 2] = vel.x;
            v.linear[i * 2 + 1] = vel.y;
        }
        #[cfg(feature = "physics")]
        if let Some(physics) = physics {
            for (e, _) in world.query::<&crate::ecs::components::RigidBody2D>().iter() {
                let (id, i) = (e.id() as u64, e.id() as usize);
                if let Some((vx, vy)) = physics.get_velocity(id) {
                    v.linear[i * 2] = vx;
                    v.linear[i * 2 + 1] = vy;
                }
                if let Some(omega) = physics.get_angular_velocity(id) {
                    v.angular[i] = omega;
                }
            }
        }
        let linear = refill(&mut v.js_linear, &v.linear);
        let angular = refill(&mut v.js_angular, &v.angular);
        let _ = js_sys::Reflect::set(state, &JsValue::from_str("velocities"), &linear);
        let _ = js_sys::Reflect::set(state, &JsValue::from_str("angular_velocities"), &angular);
    });
}

/// `data` in a JS `Float32Array`: `slot`'s, refilled in place when it is the
/// same length, else a new one kept in `slot`.
fn refill(slot: &mut Option<js_sys::Float32Array>, data: &[f32]) -> js_sys::Float32Array {
    match slot {
        Some(a) if a.length() as usize == data.len() => {
            a.copy_from(data);
            a.clone()
        }
        _ => {
            let a = js_sys::Float32Array::from(data);
            *slot = Some(a.clone());
            a
        }
    }
}

#[derive(Default)]
struct Velocities {
    linear: Vec<f32>,
    angular: Vec<f32>,
    js_linear: Option<js_sys::Float32Array>,
    js_angular: Option<js_sys::Float32Array>,
}

thread_local! {
    static VELOCITIES: RefCell<Velocities> = RefCell::new(Velocities::default());
}

/// The `kids` of a `spf` command - a prefab's parts as JSON
/// (`EntityData.children`) - spawned under `root` through the registry.
/// Both browser hosts' `spawn_prefab` arms call it.
pub fn spawn_prefab_parts(
    world: &mut World,
    registry: &crate::package::component_registry::ComponentRegistry,
    root: Entity,
    kids_json: Option<&str>,
) -> Result<(), String> {
    let Some(json) = kids_json.filter(|j| !j.is_empty()) else {
        return Ok(());
    };
    let kids: Vec<crate::scene::format::EntityData> = crate::scene::from_json_text(json).map_err(|e| e.to_string())?;
    crate::scene::prefab::spawn_children_with_registry(world, registry, root, &kids);
    Ok(())
}

/// Forget every cached mirror. Call when the world is replaced, so an entity
/// id reused by the next scene cannot be answered with the last scene's map.
pub fn forget_published_tilemaps() {
    PUBLISHED.with(|cache| cache.borrow_mut().clear());
}

/// `{ tile_size: [w, h], iso, hex, layers: [{ name, width, height, tiles }] }`,
/// with `tiles` a `Uint32Array` holding `0` for an empty cell, `iso` true
/// for an isometric map and `hex` for a hexagonal one (the preamble's
/// `world_to_tile` turns on them - a hexagonal map's is asked of Rust).
fn tilemap_to_js(tilemap: &Tilemap) -> js_sys::Object {
    let tm_obj = js_sys::Object::new();
    let ts = js_sys::Array::new();
    ts.push(&JsValue::from_f64(tilemap.tile_size[0] as f64));
    ts.push(&JsValue::from_f64(tilemap.tile_size[1] as f64));
    let _ = js_sys::Reflect::set(&tm_obj, &JsValue::from_str("tile_size"), &ts);
    let iso = tilemap.orientation == crate::tilemap::TileOrientation::Isometric;
    let _ = js_sys::Reflect::set(&tm_obj, &JsValue::from_str("iso"), &JsValue::from_bool(iso));
    let _ = js_sys::Reflect::set(&tm_obj, &JsValue::from_str("hex"), &JsValue::from_bool(tilemap.is_hex()));
    let layers_arr = js_sys::Array::new();
    for layer in &tilemap.layers {
        let layer_obj = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&layer_obj, &JsValue::from_str("name"), &JsValue::from_str(&layer.name));
        let _ = js_sys::Reflect::set(&layer_obj, &JsValue::from_str("width"), &JsValue::from_f64(layer.width as f64));
        let _ = js_sys::Reflect::set(&layer_obj, &JsValue::from_str("height"), &JsValue::from_f64(layer.height as f64));
        let tiles_arr = js_sys::Uint32Array::from(&layer.tiles[..]);
        let _ = js_sys::Reflect::set(&layer_obj, &JsValue::from_str("tiles"), &tiles_arr);
        layers_arr.push(&layer_obj);
    }
    let _ = js_sys::Reflect::set(&tm_obj, &JsValue::from_str("layers"), &layers_arr);
    // Each tile's custom data by global id (`TilesetDef::data`), for
    // `get_tile_data`.
    let data = js_sys::Object::new();
    for ts in &tilemap.tilesets {
        for (local, value) in &ts.data {
            let (Ok(local), Ok(text)) = (local.parse::<u32>(), serde_json::to_string(value)) else { continue };
            if let Ok(v) = js_sys::JSON::parse(&text) {
                let _ = js_sys::Reflect::set(&data, &JsValue::from_str(&(ts.first_gid + local).to_string()), &v);
            }
        }
    }
    let _ = js_sys::Reflect::set(&tm_obj, &JsValue::from_str("data"), &data);
    tm_obj
}
