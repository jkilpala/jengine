//! The script mirror both browser hosts publish into `window.__jengine_play`
//! every frame, and read scripts' writes back from - one implementation.
//!
//! The editor viewport and the web runtime each carried a copy of all of
//! this: the transform mirror's typed arrays, the per-frame publish of the
//! world (names, tags, cameras, input, custom components, `.gui` state, ...),
//! the writeback of what scripts set, and the physics state. Measured in Oct
//! 2026 the two copies were the same code formatted differently, with one
//! real difference - only the editor published a `.gui` node's `scroll_x` /
//! `scroll_y`, so `GUI.get(.., "scroll_y")` answered nothing in a web build.
//! Every per-frame fix (the names and velocity work of the same month) had to
//! be made twice. See `docs/architecture-debt.md`, #2.
//!
//! A host lends what the publish reads as a [`PlayWorld`]: the fields carry
//! the hosts' own field names, so the bodies read as they did in the hosts.

use std::cell::RefCell;

use hecs::{Entity, World};
use wasm_bindgen::{JsCast, JsValue};

use crate::ecs::components::{Camera2DComponent, CustomComponents, GuiComponent, SpriteComponent, Transform};
use crate::scripting::spawn_map::SpawnMap;

/// What the per-frame publish reads from a host.
pub struct PlayWorld<'a> {
    pub world: &'a World,
    pub assets: &'a crate::asset::AssetManager,
    pub camera: &'a crate::renderer::camera::Camera2D,
    pub canvas_size: glam::UVec2,
    /// The frame counters `Debug.get_fps` and friends read - only these,
    /// since the editor viewport keeps its own `FrameStats`.
    pub frame_stats: PlayStats,
    pub input: &'a crate::input::InputState,
    pub mixer: &'a crate::audio::AudioMixer,
    /// The host's script-spawned entities (spawn handle -> entity).
    pub spawns: &'a SpawnMap,
    /// Live particles, for `Particles.get_count()`.
    pub particle_count: u32,
}

/// The frame counters a script can read, named as both hosts' stats name them.
pub struct PlayStats {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub entity_count: u32,
    pub draw_calls: u32,
}

/// What the writeback of scripts' transform writes changes.
pub struct PlayWorldMut<'a> {
    pub world: &'a mut World,
}

/// Per-frame transform mirror for browser scripts, as flat typed arrays.
///
/// Replaces the old per-entity JS objects. Building those cost a heap `String`
/// for the id, a JS string, two JS arrays, four pushes and three
/// `Reflect::set` calls *per entity per frame* - and the writeback then walked
/// every Transform in the world doing six `Reflect::get`s looking for changes
/// that were almost never there. At 8,000 boids that was 31.4 ms of a 52 ms
/// frame for entities no script touched.
///
/// Now: fill plain Rust `Vec<f32>`s indexed by entity id, copy each into a
/// persistent `Float32Array` with one bulk copy, and let scripts record what
/// they wrote so the writeback costs one entry per actual write.
///
/// The arrays are indexed by `Entity::id()`. hecs reuses ids, so the highest
/// live id bounds the array rather than a monotonically growing counter.
#[derive(Default)]
pub struct ScriptMirror {
    /// Scratch buffers, reused between frames so a steady-state frame does no
    /// allocation at all.
    pos: Vec<f32>,
    rot: Vec<f32>,
    scale: Vec<f32>,
    color: Vec<f32>,
    live: Vec<u32>,
    /// `Entity` handle for each id, filled by the same pass that fills the
    /// float buffers. The writeback needs id -> Entity, and building a
    /// `HashMap` over the whole world for it — which is what this replaced —
    /// cost an allocation and an O(all entities) pass on every frame in which
    /// a script moved anything, i.e. most frames of a real game.
    entity_by_id: Vec<Option<Entity>>,
    /// The JS-side arrays, reallocated only when the entity count grows.
    js_pos: Option<js_sys::Float32Array>,
    js_rot: Option<js_sys::Float32Array>,
    js_scale: Option<js_sys::Float32Array>,
    js_color: Option<js_sys::Float32Array>,
    /// Live entity ids, reused between frames.
    ///
    /// Allocating a fresh `Uint32Array` here each frame was 32 KB of garbage
    /// per frame at 8,000 entities - about a megabyte a second, which showed
    /// up as a regular sawtooth in the profiler and a heap oscillating
    /// between 3.5 and 8.6 MB. The array is grown, never reallocated per
    /// frame; `live_count` says how much of it is valid.
    js_live: Option<js_sys::Uint32Array>,
    /// The four dirty lists, reused rather than replaced each frame.
    js_dirty: Option<[js_sys::Array; 4]>,
    /// Script-spawned entity aliases, rebuilt only when the set changes.
    js_alias: Option<js_sys::Object>,
    alias_len: usize,
    capacity: u32,
}

impl ScriptMirror {
    /// Grow the JS arrays to hold `max_id + 1` entities, if needed.
    ///
    /// Grown in steps rather than to the exact size: a flock that gains one
    /// entity a frame would otherwise reallocate every frame.
    pub fn ensure_capacity(&mut self, needed: u32, play: &js_sys::Object) {
        if self.capacity >= needed && self.js_pos.is_some() {
            return;
        }
        let cap = (needed.max(64)).next_power_of_two();
        let pos = js_sys::Float32Array::new_with_length(cap * 2);
        let rot = js_sys::Float32Array::new_with_length(cap);
        let scale = js_sys::Float32Array::new_with_length(cap * 2);
        let color = js_sys::Float32Array::new_with_length(cap * 4);
        let live = js_sys::Uint32Array::new_with_length(cap);
        let _ = js_sys::Reflect::set(play, &JsValue::from_str("positions"), &pos);
        let _ = js_sys::Reflect::set(play, &JsValue::from_str("rotations"), &rot);
        let _ = js_sys::Reflect::set(play, &JsValue::from_str("scales"), &scale);
        let _ = js_sys::Reflect::set(play, &JsValue::from_str("sprite_colors"), &color);
        let _ = js_sys::Reflect::set(play, &JsValue::from_str("live_ids"), &live);
        self.js_pos = Some(pos);
        self.js_rot = Some(rot);
        self.js_scale = Some(scale);
        self.js_color = Some(color);
        self.js_live = Some(live);
        if self.js_dirty.is_none() {
            let lists = [
                js_sys::Array::new(),
                js_sys::Array::new(),
                js_sys::Array::new(),
                js_sys::Array::new(),
            ];
            for (name, a) in [
                "dirty_positions",
                "dirty_rotations",
                "dirty_scales",
                "dirty_colors",
            ]
            .iter()
            .zip(lists.iter())
            {
                let _ = js_sys::Reflect::set(play, &JsValue::from_str(name), a);
            }
            self.js_dirty = Some(lists);
        }
        self.capacity = cap;
    }
}

thread_local! {
    pub static SCRIPT_MIRROR: RefCell<ScriptMirror> = RefCell::new(ScriptMirror::default());
    static ID_STRINGS: RefCell<Vec<Option<JsValue>>> = const { RefCell::new(Vec::new()) };
    static PUBLISHED: RefCell<std::collections::HashMap<&'static str, (u64, JsValue)>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Whether `play[name]` is still the map published with fingerprint `sig`.
/// The object is compared too, so a namespace rebuilt for a new scene - which
/// drops the map, or replaces it - is republished whatever the fingerprint.
fn published_unchanged(play: &js_sys::Object, name: &'static str, sig: u64) -> bool {
    PUBLISHED.with(|p| {
        let p = p.borrow();
        let Some((last_sig, last_obj)) = p.get(name) else { return false };
        *last_sig == sig
            && js_sys::Reflect::get(play, &JsValue::from_str(name))
                .is_ok_and(|now| js_sys::Object::is(&now, last_obj))
    })
}

/// Records what `play[name]` was just published as (see [`published_unchanged`]).
fn remember_published(play: &js_sys::Object, name: &'static str, sig: u64) {
    let obj = js_sys::Reflect::get(play, &JsValue::from_str(name)).unwrap_or(JsValue::UNDEFINED);
    PUBLISHED.with(|p| {
        p.borrow_mut().insert(name, (sig, obj));
    });
}

/// An interned id as a JS string, made once per id for the life of the page.
/// `JsValue::from_str` copies the text out of wasm and decodes it every call,
/// and the mirror publishes an id per entity per frame: ten thousand lanterns
/// naming one material spent 11% of a Ten Thousand Lights frame in `decode`.
pub fn id_string(id: crate::asset::AssetId) -> JsValue {
    ID_STRINGS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let i = id.index() as usize;
        if cache.len() <= i {
            cache.resize(i + 1, None);
        }
        cache[i].get_or_insert_with(|| JsValue::from_str(id.as_str())).clone()
    })
}

/// Lazy-initialised `window.__jengine_play` accessor. Returns `None` when
/// no `window` is available (shouldn't happen in a real browser, but we
/// degrade rather than panic).
pub fn play_namespace() -> Option<js_sys::Object> {
    let window = web_sys::window()?;
    let key = JsValue::from_str("__jengine_play");
    let existing = js_sys::Reflect::get(&window, &key).ok()?;
    if existing.is_object() {
        return Some(existing.unchecked_into());
    }
    let obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&window, &key, &obj);
    Some(obj)
}

pub fn ensure_array(parent: &js_sys::Object, key: &str) -> js_sys::Array {
    let k = JsValue::from_str(key);
    if let Ok(v) = js_sys::Reflect::get(parent, &k) {
        if v.is_instance_of::<js_sys::Array>() {
            return v.unchecked_into();
        }
    }
    let arr = js_sys::Array::new();
    let _ = js_sys::Reflect::set(parent, &k, &arr);
    arr
}

/// Read a string field off a JS command object.
/// Convert a `serde_json::Value` to a JS value for hydration.
///
/// Via `JSON.parse` rather than serde-wasm-bindgen, which this crate does not
/// depend on — and the mirror of `js_value_json`, so a value written by
/// `set_ui_prop` and read back by `get_ui_prop` round-trips through the same
/// representation.
pub fn json_to_js(v: &serde_json::Value) -> wasm_bindgen::JsValue {
    serde_json::to_string(v)
        .ok()
        .and_then(|s| js_sys::JSON::parse(&s).ok())
        .unwrap_or(wasm_bindgen::JsValue::NULL)
}

/// Convert a serde_json value to a JsValue for namespace mirroring.
/// Matches the loose typing scripts already expect (numbers/strings/
/// bools/null/arrays/objects).
pub fn json_to_jsvalue(v: &serde_json::Value) -> JsValue {
    match v {
        serde_json::Value::Null => JsValue::NULL,
        serde_json::Value::Bool(b) => JsValue::from_bool(*b),
        serde_json::Value::Number(n) => n
            .as_f64()
            .map(JsValue::from_f64)
            .unwrap_or(JsValue::NULL),
        serde_json::Value::String(s) => JsValue::from_str(s),
        serde_json::Value::Array(arr) => {
            let out = js_sys::Array::new();
            for item in arr {
                out.push(&json_to_jsvalue(item));
            }
            out.into()
        }
        serde_json::Value::Object(map) => {
            let out = js_sys::Object::new();
            for (k, v) in map {
                let _ = js_sys::Reflect::set(
                    &out,
                    &JsValue::from_str(k),
                    &json_to_jsvalue(v),
                );
            }
            out.into()
        }
    }
}

/// Convert a JsValue back to serde_json. Used by the `sc` (set_custom)
/// command to persist script-side writes into ECS.
pub fn jsvalue_to_json(v: &JsValue) -> serde_json::Value {
    if v.is_null() || v.is_undefined() {
        serde_json::Value::Null
    } else if let Some(b) = v.as_bool() {
        serde_json::Value::Bool(b)
    } else if let Some(n) = v.as_f64() {
        serde_json::json!(n)
    } else if let Some(s) = v.as_string() {
        serde_json::Value::String(s)
    } else {
        serde_json::Value::Null
    }
}

/// Empty the dirty lists for this frame. Scripts push the ids they write.
///
/// Truncated rather than replaced: four fresh `Array`s a frame is four more
/// objects for the collector to walk, and the whole point of this rewrite was
/// to stop generating per-frame garbage.
pub fn reset_dirty_lists() {
    SCRIPT_MIRROR.with(|m| {
        if let Some(lists) = &m.borrow().js_dirty {
            for a in lists {
                a.set_length(0);
            }
        }
    });
}

/// Ids a script wrote to this frame, deduped.
pub fn drain_dirty(play: &js_sys::Object, key: &str) -> Vec<u32> {
    let Ok(v) = js_sys::Reflect::get(play, &JsValue::from_str(key)) else {
        return Vec::new();
    };
    if !v.is_instance_of::<js_sys::Array>() {
        return Vec::new();
    }
    let arr: js_sys::Array = v.unchecked_into();
    let mut out: Vec<u32> = Vec::with_capacity(arr.length() as usize);
    for i in 0..arr.length() {
        if let Some(n) = arr.get(i).as_f64() {
            out.push(n as u32);
        }
    }
    // A loop that sets the same entity every frame would otherwise apply it
    // once per call.
    out.sort_unstable();
    out.dedup();
    out
}

pub fn sync_world_to_play_namespace(app: &PlayWorld<'_>) {
    let Some(play) = play_namespace() else { return };
    // Reset per-frame collections every tick to drop stale entity ids
    // (e.g. despawned entities). Cheap — just allocates fresh objects.
    // `get_health(entity)` reads this. Health lives on a handful of entities,
    // so the loop below only adds the ones that have it.
    let ui_props = js_sys::Object::new();
    let healths = js_sys::Object::new();
    // Stage 2.9e read-side fix: build reverse map (Entity -> script idx)
    // so we can ALSO mirror entries under the script-side idx for any
    // entity that was spawned via `se`/`spf`. Scripts hold the idx (not
    // the real `Entity::id()`), so without this their reads of
    // `get_position(spawned_idx)` would never resolve.
    let reverse_map = app.spawns.by_entity();
    // Numeric keys, not strings. `JsValue::from_str(&id.to_string())` cost a
    // heap `String` plus a fresh JS string per entity per map, and this loop
    // runs over every entity every frame — at 8,000 boids that was the single
    // largest item in the frame. A JS object indexed by an integer uses the
    // engine's fast element store, and `obj[123]` from the preamble reads the
    // same slot as `obj["123"]` did, so nothing on the script side changes.
    let alias_keys = |entity: Entity| -> (JsValue, Option<JsValue>) {
        let real = JsValue::from_f64(entity.id() as f64);
        let alias = reverse_map
            .get(&entity)
            .map(|idx| JsValue::from_f64(*idx as f64));
        (real, alias)
    };
    // `without::<BulkEntity>` is the whole point: this loop is O(all
    // entities) every frame regardless of what any script reads, so an entity
    // that scripts never address should not be paying for it. See
    // `BulkEntity`.
    // Transforms go out as flat typed arrays indexed by entity id - see
    // `ScriptMirror`. `BulkEntity` still skips an entity, but the cost of an
    // unmarked one is now three float writes rather than a JS object.
    let max_id = app
        .world
        .iter()
        .map(|e| e.entity().id())
        .max()
        .unwrap_or(0);
    SCRIPT_MIRROR.with(|m| {
        let m = &mut *m.borrow_mut();
        m.ensure_capacity(max_id + 1, &play);
        let cap = m.capacity as usize;
        m.pos.clear();
        // NaN is "no entity here" to the preamble's getters: a slot left
        // unfilled below reads as nil, not as the origin.
        m.pos.resize(cap * 2, f32::NAN);
        m.rot.clear();
        m.rot.resize(cap, f32::NAN);
        m.scale.clear();
        m.scale.resize(cap * 2, f32::NAN);
        m.color.clear();
        m.color.resize(cap * 4, 1.0);
        m.live.clear();
        m.entity_by_id.clear();
        m.entity_by_id.resize(cap, None);

        for (entity, t) in app
            .world
            .query::<&Transform>()
            .without::<&crate::ecs::components::BulkEntity>()
            .iter()
        {
            let i = entity.id() as usize;
            if i >= cap {
                continue;
            }
            m.pos[i * 2] = t.position.x;
            m.pos[i * 2 + 1] = t.position.y;
            m.rot[i] = t.rotation;
            m.scale[i * 2] = t.scale.x;
            m.scale[i * 2 + 1] = t.scale.y;
            m.live.push(entity.id());
            m.entity_by_id[i] = Some(entity);
        }
        for (entity, sp) in app
            .world
            .query::<&SpriteComponent>()
            .without::<&crate::ecs::components::BulkEntity>()
            .iter()
        {
            let i = entity.id() as usize;
            if i >= cap {
                continue;
            }
            m.color[i * 4..i * 4 + 4].copy_from_slice(&sp.color);
            // A sprite with no Transform is still addressable for a colour
            // write, so record it here too.
            m.entity_by_id[i] = Some(entity);
        }
        // A mesh is tinted by the same call - `Sprite.set_color` on an entity
        // whose picture is a `mesh` (Oct 2026; it reached sprites only, so a
        // territory recoloured by owner had to resend every vertex).
        for (entity, mesh) in app
            .world
            .query::<&crate::ecs::components::Mesh2D>()
            .without::<&SpriteComponent>()
            .iter()
        {
            let i = entity.id() as usize;
            if i >= cap {
                continue;
            }
            m.color[i * 4..i * 4 + 4].copy_from_slice(&mesh.color);
            m.entity_by_id[i] = Some(entity);
        }

        // Four bulk copies across the boundary, whatever the entity count.
        if let Some(a) = &m.js_pos {
            a.copy_from(&m.pos);
        }
        if let Some(a) = &m.js_rot {
            a.copy_from(&m.rot);
        }
        if let Some(a) = &m.js_scale {
            a.copy_from(&m.scale);
        }
        if let Some(a) = &m.js_color {
            a.copy_from(&m.color);
        }
        // `find_in_radius` walks this instead of enumerating the old object.
        // Written into the persistent array with a separate count, so no
        // allocation happens here at all.
        if let Some(a) = &m.js_live {
            a.subarray(0, m.live.len() as u32).copy_from(&m.live);
        }
        let _ = js_sys::Reflect::set(
            &play,
            &JsValue::from_str("live_count"),
            &JsValue::from_f64(m.live.len() as f64),
        );
    });

    // Script-spawned entities are addressed by an allocated index, so the
    // preamble resolves those through a small alias map rather than the id
    // namespace sharing a key space with real entities.
    // Rebuilt only when the set of script-spawned entities changes. It is
    // usually empty and almost always tiny, so allocating a fresh object every
    // frame was pure garbage for the collector to walk.
    SCRIPT_MIRROR.with(|m| {
        let mut m = m.borrow_mut();
        if m.alias_len != reverse_map.len() || m.js_alias.is_none() {
            let alias_obj = js_sys::Object::new();
            for (entity, idx) in reverse_map.iter() {
                let _ = js_sys::Reflect::set(
                    &alias_obj,
                    &JsValue::from_f64(*idx as f64),
                    &JsValue::from_f64(entity.id() as f64),
                );
            }
            let _ = js_sys::Reflect::set(&play, &JsValue::from_str("alias"), &alias_obj);
            m.js_alias = Some(alias_obj);
            m.alias_len = reverse_map.len();
        }
    });
    reset_dirty_lists();

    // `name -> entity id`, or the spawn handle a script holds for it, for
    // `get_entity_by_name`; built only when a name changes.
    crate::scripting::bridge::adapters::js::publish::publish_names(
        &app.world,
        &reverse_map,
        app.spawns.generation(),
        &play,
    );
    // Frame counters the stats globals read. Without this `get_fps()` and
    // friends answer 0 in the editor, the runtime you develop in.
    for (key, value) in [
        ("fps", app.frame_stats.fps as f64),
        ("music_position", crate::audio::music_position().unwrap_or(0.0)),
        ("frame_time", app.frame_stats.frame_time_ms as f64),
        ("entity_count", app.frame_stats.entity_count as f64),
        ("draw_calls", app.frame_stats.draw_calls as f64),
    ] {
        let _ = js_sys::Reflect::set(&play, &JsValue::from_str(key), &JsValue::from_f64(value));
    }

    // 3.5.A — resolved world transforms, `[x, y, rotation, sx, sy]`, for the
    // `get_world_*` getters. Only parented entities carry a `WorldTransform`;
    // for the rest the preamble falls back to the local transform, so there
    // is nothing to publish.
    let world_transforms = js_sys::Object::new();
    for (entity, wt) in app
        .world
        .query::<&crate::ecs::components::WorldTransform>()
        .iter()
    {
        let (key, alias) = alias_keys(entity);
        let arr = js_sys::Array::new();
        arr.push(&JsValue::from_f64(wt.position.x as f64));
        arr.push(&JsValue::from_f64(wt.position.y as f64));
        arr.push(&JsValue::from_f64(wt.rotation as f64));
        arr.push(&JsValue::from_f64(wt.scale.x as f64));
        arr.push(&JsValue::from_f64(wt.scale.y as f64));
        let _ = js_sys::Reflect::set(&world_transforms, &key, &arr);
        if let Some(a) = alias {
            let _ = js_sys::Reflect::set(&world_transforms, &a, &arr);
        }
    }
    let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("world_transforms"),
        &world_transforms,
    );
    // 3.5.B/C — skeleton roots by name, bones per root, and skeletal
    // playback times. Native answers these by querying the live world through
    // a raw pointer, which a script call cannot do here.
    {
        use crate::animation::skeletal::SkeletalPlayer;
        use crate::ecs::components::SkeletonRoot;
        let skeletons = js_sys::Object::new();
        for (entity, root) in app.world.query::<&SkeletonRoot>().iter() {
            let _ = js_sys::Reflect::set(
                &skeletons,
                &JsValue::from_str(&root.name),
                &JsValue::from_f64(entity.id() as f64),
            );
        }
        let _ = js_sys::Reflect::set(&play, &JsValue::from_str("skeletons"), &skeletons);

        // Bones per skeleton root, via the engine's own `collect_bones` —
        // the same descendant walk native's `find_bone` does.
        let bones = js_sys::Object::new();
        let roots: Vec<(hecs::Entity, String)> = app
            .world
            .query::<&SkeletonRoot>()
            .iter()
            .map(|(e, r)| (e, r.name.clone()))
            .collect();
        for (root, _) in roots {
            let entry = js_sys::Object::new();
            for (bone_entity, bone_name) in
                crate::ecs::skeleton::collect_bones(&app.world, root)
            {
                let _ = js_sys::Reflect::set(
                    &entry,
                    &JsValue::from_str(&bone_name),
                    &JsValue::from_f64(bone_entity.id() as f64),
                );
            }
            let _ = js_sys::Reflect::set(
                &bones,
                &JsValue::from_str(&root.id().to_string()),
                &entry,
            );
        }
        let _ = js_sys::Reflect::set(&play, &JsValue::from_str("bones"), &bones);

        let times = js_sys::Object::new();
        for (entity, player) in app.world.query::<&SkeletalPlayer>().iter() {
            let _ = js_sys::Reflect::set(
                &times,
                &JsValue::from_str(&entity.id().to_string()),
                &JsValue::from_f64(player.time as f64),
            );
        }
        let _ = js_sys::Reflect::set(&play, &JsValue::from_str("animation_times"), &times);
    }
    // 2.4 — bus state for `audio_get_bus_volume` / `audio_is_bus_muted`.
    let audio_buses = js_sys::Object::new();
    for (name, bus) in app.mixer.buses() {
        let entry = js_sys::Object::new();
        let _ = js_sys::Reflect::set(
            &entry,
            &JsValue::from_str("volume"),
            &JsValue::from_f64(bus.volume as f64),
        );
        let _ = js_sys::Reflect::set(
            &entry,
            &JsValue::from_str("muted"),
            &JsValue::from_bool(bus.muted),
        );
        let _ = js_sys::Reflect::set(
            &entry,
            &JsValue::from_str("solo"),
            &JsValue::from_bool(bus.solo),
        );
        let _ = js_sys::Reflect::set(&audio_buses, &JsValue::from_str(name), &entry);
    }
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("audio_buses"), &audio_buses);
    // 2.5 — action bindings for `input_get_bindings` / `input_get_actions` /
    // `input_get_dead_zone`. Republished each frame so a rebinding menu sees
    // its own writes on the next one.
    let input_actions = js_sys::Object::new();
    for (name, keys) in app.input.get_actions() {
        let entry = js_sys::Object::new();
        let key_list = js_sys::Array::new();
        for k in keys {
            key_list.push(&JsValue::from_str(k));
        }
        let _ = js_sys::Reflect::set(&entry, &JsValue::from_str("keys"), &key_list);
        let _ = js_sys::Reflect::set(
            &entry,
            &JsValue::from_str("dead_zone"),
            &JsValue::from_f64(app.input.get_dead_zone(name) as f64),
        );
        let _ = js_sys::Reflect::set(&input_actions, &JsValue::from_str(name), &entry);
    }
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("input_actions"), &input_actions);
    // What went down this frame, by binding name (`input_get_just_pressed`).
    let just = js_sys::Array::new();
    for name in app.input.just_pressed_bindings() {
        just.push(&JsValue::from_str(&name));
    }
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("just_pressed_bindings"), &just);
    // The characters typed this frame (`input_get_typed`).
    let typed: String = app.input.typed_chars().iter().collect();
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("typed"), &JsValue::from_str(&typed));
    // Named atlas points per entity, for `get_sprite_point`. The preamble
    // has always read this map and nothing ever wrote it, so the getter
    // answered [0, 0] whatever the atlas said.
    let sprite_points = js_sys::Object::new();
    for (id, points) in crate::scripting::api::compute_sprite_points(&app.world, &app.assets)
    {
        let entry = js_sys::Object::new();
        for (name, (nx, ny)) in points {
            let pair = js_sys::Array::new();
            pair.push(&JsValue::from_f64(nx as f64));
            pair.push(&JsValue::from_f64(ny as f64));
            let _ = js_sys::Reflect::set(&entry, &JsValue::from_str(&name), &pair);
        }
        let _ = js_sys::Reflect::set(
            &sprite_points,
            &JsValue::from_str(&id.to_string()),
            &entry,
        );
    }
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("sprite_points"), &sprite_points);
    for (entity, h) in app.world.query::<&crate::ecs::components::Health>().iter() {
        let arr = js_sys::Array::new();
        arr.push(&JsValue::from_f64(h.current as f64));
        arr.push(&JsValue::from_f64(h.max as f64));
        let _ = js_sys::Reflect::set(
            &healths,
            &JsValue::from_str(&entity.id().to_string()),
            &arr,
        );
    }
    // Entity-UI widget state for `get_ui_prop`. Covers the button's `pressed`
    // / `hovered`, which the UI system rewrites every frame and which a script
    // has no other way to see — polling them is the entity-UI answer to
    // `gui_on_click`.
    {
        use crate::ui as jui;
        // The one list, beside the reader it describes.
        const UI_PROPS: &[&str] = jui::READABLE_UI_PROPS;
        let ui_entities: Vec<hecs::Entity> = app
            .world
            .query::<&jui::UIElement>()
            .iter()
            .map(|(e, _)| e)
            .collect();
        for entity in ui_entities {
            let entry = js_sys::Object::new();
            let mut any = false;
            for prop in UI_PROPS {
                if let Some(v) = jui::read_ui_prop(&app.world, entity, prop) {
                    let _ = js_sys::Reflect::set(
                        &entry,
                        &JsValue::from_str(prop),
                        &json_to_js(&v),
                    );
                    any = true;
                }
            }
            if any {
                let _ = js_sys::Reflect::set(
                    &ui_props,
                    &JsValue::from_str(&entity.id().to_string()),
                    &entry,
                );
            }
        }
    }
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("ui_props"), &ui_props);
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("healths"), &healths);
    // NOTE: `positions` / `rotations` / `scales` / `sprite_colors` are set by
    // `ScriptMirror::ensure_capacity` as Float32Arrays. Re-setting them here
    // from the old per-entity objects would silently overwrite the typed
    // arrays with empty ones, and every accessor would read zeros.

    // Mirror entity Tags so the wrapper's `find_by_tag` /
    // `get_entities_by_tag` / `has_tag` see the scene-loaded tags
    // (e.g. the Brotato player carries `tags: ["player"]` in its
    // scene entry — without this sync `find_by_tag("player")`
    // returns [] and the HUD reports GAME OVER immediately).
    let tags_obj = js_sys::Object::new();
    for (entity, tags) in app
        .world
        .query::<&crate::ecs::components::Tags>()
        .iter()
    {
        let arr = js_sys::Array::new();
        for t in tags.0.iter() {
            arr.push(&JsValue::from_str(t));
        }
        let (key, alias) = alias_keys(entity);
        let _ = js_sys::Reflect::set(&tags_obj, &key, &arr);
        if let Some(a) = alias {
            let _ = js_sys::Reflect::set(&tags_obj, &a, &arr);
        }
    }
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("tags"), &tags_obj);

    // Per-entity answers the preamble's getters read. None was published
    // until Oct 2026, so `get_material`, `Animation.get_current` / `get_state`
    // / `is_playing`, `Sprite.get_anchor` and `gui_on_click`'s short form
    // answered their defaults in every browser runtime, in silence. Each map
    // holds only the entities that carry the component (and, for anchors,
    // only those off-centre, the getter's default), so a world of plain
    // sprites costs a query and nothing more.
    let publish_map = |name: &str, entries: Vec<(Entity, JsValue)>| {
        let obj = js_sys::Object::new();
        for (entity, v) in entries {
            let (key, alias) = alias_keys(entity);
            let _ = js_sys::Reflect::set(&obj, &key, &v);
            if let Some(a) = alias {
                let _ = js_sys::Reflect::set(&obj, &a, &v);
            }
        }
        let _ = js_sys::Reflect::set(&play, &JsValue::from_str(name), &obj);
    };
    let w = app.world;
    // Materials are on every entity of a crowd and change almost never, so
    // the map is rebuilt only when its fingerprint does: ten thousand lanterns
    // spent a fifth of a Ten Thousand Lights frame in `Reflect::set` writing
    // the same `default_sprite` into a fresh object. Interned ids make the
    // fingerprint a few integer mixes per entity.
    let mut sig = 0xcbf2_9ce4_8422_2325u64 ^ app.spawns.generation();
    for (e, m) in w.query::<&crate::ecs::components::MaterialComponent>().iter() {
        for v in [e.to_bits().get(), m.material_id.index() as u64] {
            sig = (sig ^ v).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    if !published_unchanged(&play, "materials", sig) {
        publish_map(
            "materials",
            w.query::<&crate::ecs::components::MaterialComponent>()
                .iter()
                .map(|(e, m)| (e, id_string(m.material_id)))
                .collect(),
        );
        remember_published(&play, "materials", sig);
    }
    publish_map(
        "gui_ids",
        w.query::<&crate::ecs::components::GuiComponent>()
            .iter()
            .map(|(e, g)| (e, JsValue::from_str(&g.gui_id)))
            .collect(),
    );
    publish_map(
        "sprite_anchors",
        w.query::<&crate::ecs::components::SpriteComponent>()
            .iter()
            .filter(|(_, s)| s.anchor != [0.5, 0.5])
            .map(|(e, s)| {
                let a = js_sys::Array::new();
                a.push(&JsValue::from_f64(s.anchor[0] as f64));
                a.push(&JsValue::from_f64(s.anchor[1] as f64));
                (e, a.into())
            })
            .collect(),
    );
    // The animation answers, by the rules native reads (`animation::*`).
    let animated: Vec<Entity> = w
        .query::<&crate::animation::SpriteAnimator>()
        .iter()
        .map(|(e, _)| e)
        .chain(w.query::<&crate::animation::skeletal::SkeletalPlayer>().iter().map(|(e, _)| e))
        .collect();
    publish_map(
        "animation_clip_names",
        animated
            .iter()
            .filter_map(|&e| crate::animation::current_clip_name(w, e).map(|c| (e, JsValue::from_str(&c))))
            .collect(),
    );
    publish_map(
        "animation_states",
        animated
            .iter()
            .filter(|&&e| crate::animation::is_playing(w, e))
            .map(|&e| (e, JsValue::TRUE))
            .collect(),
    );
    publish_map(
        "anim_states",
        w.query::<&crate::animation::AnimStateMachine>()
            .iter()
            .filter_map(|(e, m)| m.current_state.as_ref().map(|s| (e, JsValue::from_str(s))))
            .collect(),
    );
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("scene_name"), &JsValue::from_str(&crate::scene::current_name()));
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("particle_count"), &JsValue::from_f64(app.particle_count as f64));

    // Stage 2.9b — mirror input snapshot. `pressed_keys`/`just_pressed_keys`/
    // `just_released_keys` are `{key_name: true}` maps keyed by
    // `KeyCode::name()` (canonical engine names). Unset keys read as
    // `undefined` which the wrapper coerces via `!!`. action_bindings
    // is the editor-supplied action map.
    let pressed_keys = js_sys::Object::new();
    for k in app.input.pressed_iter() {
        let _ = js_sys::Reflect::set(
            &pressed_keys,
            &JsValue::from_str(k.name()),
            &JsValue::TRUE,
        );
    }
    let just_pressed = js_sys::Object::new();
    for k in app.input.just_pressed_iter() {
        let _ = js_sys::Reflect::set(
            &just_pressed,
            &JsValue::from_str(k.name()),
            &JsValue::TRUE,
        );
    }
    let just_released = js_sys::Object::new();
    for k in app.input.just_released_iter() {
        let _ = js_sys::Reflect::set(
            &just_released,
            &JsValue::from_str(k.name()),
            &JsValue::TRUE,
        );
    }
    let action_bindings = js_sys::Object::new();
    for (name, keys) in app.input.get_actions() {
        let arr = js_sys::Array::new();
        for key in keys {
            arr.push(&JsValue::from_str(key));
        }
        let _ = js_sys::Reflect::set(&action_bindings, &JsValue::from_str(name), &arr);
    }
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("pressed_keys"), &pressed_keys);
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("just_pressed_keys"), &just_pressed);
    let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("just_released_keys"),
        &just_released,
    );
    let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("action_bindings"),
        &action_bindings,
    );
    // Resolved actions, action axes and raw pad state. Published from Rust
    // rather than re-derived in JS: the preamble's old binding scan only knew
    // about `pressed_keys`, so a gamepad binding was never found and pad
    // actions were dead in this runtime (BUGS #9).
    crate::scripting::bridge::adapters::js::input_mirror::publish(&play, &app.input);
    let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("mouse_x"),
        &JsValue::from_f64(app.input.mouse_position.0 as f64),
    );
    let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("mouse_y"),
        &JsValue::from_f64(app.input.mouse_position.1 as f64),
    );
    // Stage 2.9h — expose canvas pixel size so scripts can convert
    // mouse coords to world space without baking dimensions in.
    let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("screen_w"),
        &JsValue::from_f64(app.canvas_size.x as f64),
    );
    let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("screen_h"),
        &JsValue::from_f64(app.canvas_size.y as f64),
    );
    // Expose the active game camera so scripts can map screen → world
    // correctly even when the camera's authored viewport size differs
    // from the canvas pixel size (the common case: a fixed-resolution
    // 1280×720 camera in a resizable editor viewport). Falls back to
    // the editor camera if no scene Camera2DComponent is active.
    {
        // Position, zoom and turn are the **renderer's** camera - what is on
        // screen - as native's `camera_view` is. These were read off the
        // camera entity, so while `CameraFollow` moved or framed the view
        // the editor and web reported the entity's authored (0, 0) and zoom
        // 1: `get_camera_zoom` and `get_mouse_world_position` were wrong for
        // every following camera (test-lab scene 93, BUGS #163). The
        // viewport size stays the component's authored one.
        let cam_x = app.camera.position.x;
        let cam_y = app.camera.position.y;
        let cam_zoom = app.camera.zoom;
        let cam_rot = app.camera.rotation;
        let mut cam_vw = app.camera.width();
        let mut cam_vh = app.camera.height();
        let main = crate::ecs::camera_rig::main_camera(&app.world);
        for (e, cam) in app
            .world
            .query::<&Camera2DComponent>()
            .iter()
        {
            if Some(e) != main {
                continue;
            }
            // The world the canvas shows, as the projection has it, so
            // `get_mouse_world_position` picks what is under the cursor.
            let view = cam.view_on(glam::Vec2::new(app.canvas_size.x as f32, app.canvas_size.y as f32));
            cam_vw = view.x;
            cam_vh = view.y;
            break;
        }
        let _ = js_sys::Reflect::set(&play, &JsValue::from_str("camera_x"), &JsValue::from_f64(cam_x as f64));
        let _ = js_sys::Reflect::set(&play, &JsValue::from_str("camera_y"), &JsValue::from_f64(cam_y as f64));
        let _ = js_sys::Reflect::set(&play, &JsValue::from_str("camera_zoom"), &JsValue::from_f64(cam_zoom as f64));
        let _ = js_sys::Reflect::set(&play, &JsValue::from_str("camera_vw"), &JsValue::from_f64(cam_vw as f64));
        let _ = js_sys::Reflect::set(&play, &JsValue::from_str("camera_vh"), &JsValue::from_f64(cam_vh as f64));
        // The camera's own turn, which `get_mouse_world_position` needs.
        let _ = js_sys::Reflect::set(&play, &JsValue::from_str("camera_rot"), &JsValue::from_f64(cam_rot as f64));
    }
    let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("mouse_just_pressed"),
        &JsValue::from_bool(app.input.mouse_left_just_pressed),
    );
    let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("mouse_down"),
        &JsValue::from_bool(app.input.mouse_left),
    );
    let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("mouse_just_released"),
        &JsValue::from_bool(app.input.mouse_left_just_released),
    );
        let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("mouse_right_just_pressed"),
        &JsValue::from_bool(app.input.mouse_right_just_pressed),
    );
        let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("mouse_right_down"),
        &JsValue::from_bool(app.input.mouse_right),
    );
        let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("mouse_right_just_released"),
        &JsValue::from_bool(app.input.mouse_right_just_released),
    );
    for (key, value) in [
        ("mouse_middle_just_pressed", app.input.mouse_middle_just_pressed),
        ("mouse_middle_down", app.input.mouse_middle),
        ("mouse_middle_just_released", app.input.mouse_middle_just_released),
    ] {
        let _ = js_sys::Reflect::set(&play, &JsValue::from_str(key), &JsValue::from_bool(value));
    }
    let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("mouse_wheel_delta"),
        &JsValue::from_f64(app.input.mouse_wheel_delta as f64),
    );
    // This frame's mouse motion, for `get_mouse_delta` (a spinner, a
    // trackball, a locked pointer's look).
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("mouse_dx"), &JsValue::from_f64(app.input.mouse_delta.0 as f64));
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("mouse_dy"), &JsValue::from_f64(app.input.mouse_delta.1 as f64));

    // Stage 2.9b — hydrate `custom_components[entity_id][component][field]`
    // from the world's CustomComponents bag. Entries the script writes
    // via set_custom/set_fields stay in this object across ticks (since
    // we rebuild the OUTER object every tick but scripts may add new
    // keys after this returns; those survive only the current tick —
    // we do not mirror back into ECS yet, see Stage 2.9c).
    let custom_obj = js_sys::Object::new();
    for (entity, custom) in app.world.query::<&CustomComponents>().iter() {
        let entity_obj = js_sys::Object::new();
        for (comp_name, value) in custom.0.iter() {
            let comp_obj = js_sys::Object::new();
            if let serde_json::Value::Object(map) = value {
                for (field, fv) in map {
                    let js_val = json_to_jsvalue(fv);
                    let _ = js_sys::Reflect::set(&comp_obj, &JsValue::from_str(field), &js_val);
                }
            }
            let _ = js_sys::Reflect::set(
                &entity_obj,
                &JsValue::from_str(comp_name),
                &comp_obj,
            );
        }
        let _ = js_sys::Reflect::set(
            &custom_obj,
            &JsValue::from_str(&entity.id().to_string()),
            &entity_obj,
        );
        // Stage 2.9e read-side fix: mirror under script idx too.
        if let Some(idx) = reverse_map.get(&entity) {
            let _ = js_sys::Reflect::set(
                &custom_obj,
                &JsValue::from_str(&idx.to_string()),
                &entity_obj,
            );
        }
    }
    let _ = js_sys::Reflect::set(
        &play,
        &JsValue::from_str("custom_components"),
        &custom_obj,
    );

    // Stage 2.9l — mirror live GUI document state into
    // `__jengine_play.gui_state[gui_id][node_id]` so the wrapper's
    // `gui_get(gid, nid, prop)` can read it. We walk every
    // `GuiComponent` in the world (deduplicated by `gui_id`), look up
    // the loaded document, and serialize each node's current values:
    // `text` (Text/TextInput/Button.label), `value` (Slider/
    // ProgressBar), `checked` (Toggle), plus `visible` / `focusable` /
    // `tab_order` from `style`, and `focused` (set when this node is
    // the GuiComponent's focused_node_id). Rebuilt every tick so
    // updates from `gsp` / focus changes / native handlers all show
    // up immediately.
    let gui_state = js_sys::Object::new();
    let mut seen_guis: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for (_, gui_comp) in app.world.query::<&GuiComponent>().iter() {
        if !seen_guis.insert(gui_comp.gui_id.clone()) {
            continue;
        }
        let Some(doc) = app.assets.get_gui(&gui_comp.gui_id) else {
            continue;
        };
        let doc_obj = js_sys::Object::new();
        let focused_id = gui_comp.focused_node_id.as_deref().unwrap_or("");
        // Depth-first walk via an explicit stack to avoid recursion.
        let mut stack: Vec<&crate::ui::gui_document::GuiNode> = vec![&doc.root];
        while let Some(node) = stack.pop() {
            let node_obj = js_sys::Object::new();
            // Style fields available on every node.
            let _ = js_sys::Reflect::set(
                &node_obj,
                &JsValue::from_str("visible"),
                &JsValue::from_bool(node.style.visible),
            );
            let _ = js_sys::Reflect::set(
                &node_obj,
                &JsValue::from_str("focusable"),
                &JsValue::from_bool(node.style.focusable),
            );
            let _ = js_sys::Reflect::set(
                &node_obj,
                &JsValue::from_str("tab_order"),
                &JsValue::from_f64(node.style.tab_order as f64),
            );
            let _ = js_sys::Reflect::set(
                &node_obj,
                &JsValue::from_str("focused"),
                &JsValue::from_bool(!focused_id.is_empty() && node.id == focused_id),
            );
            // Scroll offsets, so a script can drive a scrollbar thumb from
            // the position the engine's wheel handling landed on.
            let _ = js_sys::Reflect::set(
                &node_obj,
                &JsValue::from_str("scroll_x"),
                &JsValue::from_f64(node.style.scroll_x as f64),
            );
            let _ = js_sys::Reflect::set(
                &node_obj,
                &JsValue::from_str("scroll_y"),
                &JsValue::from_f64(node.style.scroll_y as f64),
            );
            // Type-specific live state.
            use crate::ui::gui_document::GuiNodeType::*;
            match &node.node_type {
                Text { text, .. } | TextInput { text, .. } => {
                    let _ = js_sys::Reflect::set(
                        &node_obj,
                        &JsValue::from_str("text"),
                        &JsValue::from_str(text),
                    );
                }
                Button { label, .. } => {
                    for key in ["text", "label"] {
                        let _ = js_sys::Reflect::set(
                            &node_obj,
                            &JsValue::from_str(key),
                            &JsValue::from_str(label),
                        );
                    }
                }
                Slider { value, .. } | ProgressBar { value, .. } => {
                    let _ = js_sys::Reflect::set(
                        &node_obj,
                        &JsValue::from_str("value"),
                        &JsValue::from_f64(*value as f64),
                    );
                }
                Toggle { checked, .. } => {
                    let _ = js_sys::Reflect::set(
                        &node_obj,
                        &JsValue::from_str("checked"),
                        &JsValue::from_bool(*checked),
                    );
                }
                _ => {}
            }
            // A field's editing state, through native's own mapping. It was
            // never published here, so `gui_get(.., "submitted")` answered ""
            // in the browsers and Enter sent no chat line in any web game.
            if matches!(node.node_type, TextInput { .. }) {
                for key in ["submitted", "caret", "selected"] {
                    if let Some(v) = crate::scripting::bridge::snapshot::gui_node_prop(node, key) {
                        let _ = js_sys::Reflect::set(&node_obj, &JsValue::from_str(key), &JsValue::from_str(&v));
                    }
                }
            }
            let _ = js_sys::Reflect::set(
                &doc_obj,
                &JsValue::from_str(&node.id),
                &node_obj,
            );
            for child in &node.children {
                stack.push(child);
            }
        }
        let _ = js_sys::Reflect::set(
            &gui_state,
            &JsValue::from_str(&gui_comp.gui_id),
            &doc_obj,
        );
    }
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("gui_state"), &gui_state);

    // Mirror tilemap components into `__jengine_play.tilemaps` so
    // `get_tile` / `get_tile_at_world` / `world_to_tile` etc. work in
    // browser-script mode. Structure mirrors what browser.rs expects:
    //   tilemaps[entity_id] = { tile_size:[w,h], layers:[{name,width,height,tiles}] }
    // One implementation in core, cached by the map's revision: this used
    // to push every tile of every tilemap into a fresh JS array, one FFI
    // call per tile, every frame.
    crate::scripting::bridge::adapters::js::publish::publish_tilemaps(&app.world, &reverse_map, &play);
    // `find_child`: each parent's parts by name.
    crate::scripting::bridge::adapters::js::publish::publish_kids(&app.world, &reverse_map, &play);

    // Stage 2.9a: wrapper expects `commands` to exist (and we drain in
    // place by setting length=0 each tick).
    let _ = ensure_array(&play, "commands");
}

/// After scripts run, copy `__jengine_play.positions/rotations/scales` back
/// to the ECS for entities the script may have mutated via the wrapper's
/// `set_position` / `set_rotation` / `set_scale` setters.
pub fn sync_play_namespace_to_world(app: &mut PlayWorldMut<'_>) {
    let Some(play) = play_namespace() else { return };

    // Which entities did scripts actually write this frame? Usually none, and
    // never more than the number of `set_position`-style calls made. This used
    // to be a pass over every Transform in the world doing six `Reflect::get`s
    // apiece, looking for changes that were not there - see BUGS.md #30.
    let dirty_pos = drain_dirty(&play, "dirty_positions");
    let dirty_rot = drain_dirty(&play, "dirty_rotations");
    let dirty_scale = drain_dirty(&play, "dirty_scales");
    let dirty_color = drain_dirty(&play, "dirty_colors");
    if dirty_pos.is_empty()
        && dirty_rot.is_empty()
        && dirty_scale.is_empty()
        && dirty_color.is_empty()
    {
        return;
    }

    // Ids are real `Entity::id()`s: the preamble resolves script-side handles
    // through the alias map before writing, so nothing has to be un-aliased
    // here. That also removes the old collision hazard, where a script index
    // and a real entity id could be the same number and share a key.
    SCRIPT_MIRROR.with(|m| {
        let m = &*m.borrow();
        let (Some(js_pos), Some(js_rot), Some(js_scale), Some(js_color)) =
            (&m.js_pos, &m.js_rot, &m.js_scale, &m.js_color)
        else {
            return;
        };
        // O(1) per write, from the table the fill pass already built.
        let lookup = |id: u32| -> Option<Entity> {
            m.entity_by_id.get(id as usize).copied().flatten()
        };
        for id in dirty_pos {
            let Some(entity) = lookup(id) else { continue };
            if let Ok(mut t) = app.world.get::<&mut Transform>(entity) {
                t.position.x = js_pos.get_index(id * 2);
                t.position.y = js_pos.get_index(id * 2 + 1);
            }
        }
        for id in dirty_rot {
            let Some(entity) = lookup(id) else { continue };
            if let Ok(mut t) = app.world.get::<&mut Transform>(entity) {
                t.rotation = js_rot.get_index(id);
            }
        }
        for id in dirty_scale {
            let Some(entity) = lookup(id) else { continue };
            if let Ok(mut t) = app.world.get::<&mut Transform>(entity) {
                t.scale.x = js_scale.get_index(id * 2);
                t.scale.y = js_scale.get_index(id * 2 + 1);
            }
        }
        for id in dirty_color {
            let Some(entity) = lookup(id) else { continue };
            if let Ok(mut sp) = app.world.get::<&mut SpriteComponent>(entity) {
                for (c, slot) in sp.color.iter_mut().enumerate() {
                    *slot = js_color.get_index(id * 4 + c as u32);
                }
            } else if let Ok(mut mesh) = app.world.get::<&mut crate::ecs::components::Mesh2D>(entity) {
                for (c, slot) in mesh.color.iter_mut().enumerate() {
                    *slot = js_color.get_index(id * 4 + c as u32);
                }
            }
        }
    });
}

/// Gravity, this frame's contacts and every body's velocity, for the scripts
/// about to run. Velocities go through the functions native's snapshot uses,
/// so `get_velocity` means one thing in every runtime: the component, with
/// rapier's value over it for a rigid body. Both browser hosts call this
/// before their scripts and before each `fixed_update`; each carried the
/// block until Oct 2026.
pub fn publish_physics_for_scripts(
    world: &World,
    physics: Option<&crate::physics::PhysicsWorld>,
    contacts: &[(u32, u32, bool, f32, f32, f32)],
) {
    #[cfg(feature = "physics")]
    let gravity = physics.map(|p| p.get_gravity()).unwrap_or((0.0, 0.0));
    #[cfg(not(feature = "physics"))]
    let gravity = {
        let _ = physics;
        (0.0, 0.0)
    };
    let mut snap = crate::scripting::bridge::context::BridgeReadSnapshot::default();
    crate::scripting::bridge::snapshot_characters(&mut snap, world);
    publish_physics_state(gravity, contacts, &snap.characters, &super::scripts::script_names());
    if let Some(state) = play_namespace().and_then(|play| js_sys::Reflect::get(&play, &JsValue::from_str("state")).ok())
        && state.is_object()
    {
        crate::scripting::bridge::adapters::js::publish::publish_velocities(&state, world, physics);
    }
}

pub fn publish_physics_state(
    gravity: (f32, f32),
    contacts: &[(u32, u32, bool, f32, f32, f32)],
    characters: &std::collections::HashMap<u64, crate::ecs::components::CharacterController2D>,
    // Entity id -> the spawn handle a script holds for it (`SpawnMap::by_id`).
    handles: &std::collections::HashMap<u64, u64>,
) {
    let Some(win) = web_sys::window() else { return };
    let Ok(play) = js_sys::Reflect::get(&win, &JsValue::from_str("__jengine_play")) else {
        return;
    };
    if !play.is_object() {
        return;
    }
    let state_key = JsValue::from_str("state");
    let state = match js_sys::Reflect::get(&play, &state_key) {
        Ok(v) if v.is_object() => v,
        _ => {
            let obj = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&play, &state_key, &obj);
            obj.into()
        }
    };
    let g = js_sys::Array::new();
    g.push(&JsValue::from_f64(gravity.0 as f64));
    g.push(&JsValue::from_f64(gravity.1 as f64));
    let _ = js_sys::Reflect::set(&state, &JsValue::from_str("gravity"), &g);

    // A spawned entity is known to scripts by its spawn handle, so every row
    // names it by that: `get_velocity(handle)` finds it, and a contact names
    // the bomb the script is holding (native: `spawn_aliases::script_id`).
    let sid = |id: u64| handles.get(&id).copied().unwrap_or(id) as f64;

    // Linear and angular velocities are published by the caller, by entity
    // id (`publish::publish_velocities`). Until Oct 2026 nothing published
    // them, so both answered zero in every browser runtime - a script that
    // read a body's velocity and wrote it back (scene 17's player keeps
    // gravity that way) cancelled its own fall every frame.
    // Character controllers for `get_character`, as rows `[id, grounded,
    // ground_nx, ground_ny, ground_entity (-1 for none), wall, ceiling,
    // sliding, vx, vy, [touching]]`, the shape the preamble scans.
    let chars = js_sys::Array::new();
    for (id, c) in characters {
        let row = js_sys::Array::new();
        row.push(&JsValue::from_f64(sid(*id)));
        row.push(&JsValue::from_bool(c.grounded));
        row.push(&JsValue::from_f64(c.ground_normal[0] as f64));
        row.push(&JsValue::from_f64(c.ground_normal[1] as f64));
        row.push(&JsValue::from_f64(c.ground_entity.map_or(-1.0, |g| g as f64)));
        row.push(&JsValue::from_f64(c.wall as f64));
        row.push(&JsValue::from_bool(c.ceiling));
        row.push(&JsValue::from_bool(c.sliding));
        row.push(&JsValue::from_f64(c.moved[0] as f64));
        row.push(&JsValue::from_f64(c.moved[1] as f64));
        let touching = js_sys::Array::new();
        for t in &c.touching {
            touching.push(&JsValue::from_f64(*t as f64));
        }
        row.push(&touching);
        chars.push(&row);
    }
    let _ = js_sys::Reflect::set(&state, &JsValue::from_str("characters"), &chars);
    // The engine's persistent store, for the preamble's mirror of it:
    // `persistent_*` read the mirror, and this is how a package's writes
    // (Rust, not script) reach it. Published every frame, before scripts.
    let _ = js_sys::Reflect::set(
        &state,
        &JsValue::from_str("persistent_json"),
        &JsValue::from_str(&crate::scripting::persistent::to_json()),
    );

    let list = js_sys::Array::new();
    for (a, b, started, impulse, x, y) in contacts {
        // The first three are what this always published; how hard and where
        // are appended, so a script reading three is unaffected.
        let row = js_sys::Array::new();
        row.push(&JsValue::from_f64(sid(*a as u64)));
        row.push(&JsValue::from_f64(sid(*b as u64)));
        row.push(&JsValue::from_bool(*started));
        row.push(&JsValue::from_f64(*impulse as f64));
        row.push(&JsValue::from_f64(*x as f64));
        row.push(&JsValue::from_f64(*y as f64));
        list.push(&row);
    }
    let _ = js_sys::Reflect::set(&state, &JsValue::from_str("contacts"), &list);
}
