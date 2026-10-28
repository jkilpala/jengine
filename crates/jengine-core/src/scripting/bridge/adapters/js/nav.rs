//! Synchronous pathfinding for the browser runtimes.
//!
//! Most script functions in the editor and web builds are one-way: the script
//! pushes a command and the host applies it next tick. `tilemap_find_path`
//! cannot work that way — Lua has no await, so the call has to *answer*.
//!
//! Two routes were available and one of them is a trap:
//!
//! * Mirror the tile data into JS and run A* there. That means a second
//!   pathfinder, and two implementations of a pathfinder will not agree.
//! * Call back into Rust. The obvious version of this deadlocks: the hosts
//!   own their world in an `Rc<RefCell<…>>` and scripts run *inside* a
//!   `borrow_mut()`, so a callback that reaches for the app panics on a
//!   double borrow. (`raycast` was specified against a
//!   `__jengine_play.state.physics_raycast` hook that no host ever installed —
//!   almost certainly for this reason.)
//!
//! So the callback owns its own data. [`NavSnapshot`] holds cloned tilemaps,
//! refreshed by the host once per frame *before* scripts run, and the
//! installed closures borrow only that. One algorithm — the same
//! `tilemap::find_path` the native runtime uses — and no reentrancy.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use crate::ecs::components::Transform;
use crate::tilemap::{PathMode, Tilemap, find_path};

/// Tilemaps as of the start of this frame, keyed by entity id.
///
/// Cloned rather than borrowed: the whole point is that the pathfinding
/// callback must not touch the live world while scripts are running. Mirrors
/// `BridgeReadSnapshot::tilemap_snapshots` on native, which is refreshed the
/// same way and for the same reason.
#[derive(Default)]
pub struct NavSnapshot {
    maps: HashMap<u64, (Tilemap, (f32, f32))>,
    /// Every `nav_region`'s graph - shared, not copied: a graph is an `Arc`
    /// the region keeps until its colliders move.
    #[cfg(feature = "navigation")]
    graphs: Vec<std::sync::Arc<crate::navigation::NavGraph>>,
    /// The agents with somewhere to walk to, by entity id.
    moving: std::collections::HashSet<u64>,
}

impl NavSnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-clone every tilemap in the world. Call once per frame before
    /// scripts run.
    pub fn refresh(&mut self, world: &hecs::World) {
        self.refresh_named(world, &HashMap::new());
    }

    /// [`Self::refresh`], with the host's spawn map (entity id to the handle
    /// `spawn_prefab` gave the script), so `Nav.is_moving` knows a spawned
    /// agent by the handle the script holds - as the ray snapshot does.
    pub fn refresh_named(&mut self, world: &hecs::World, names: &HashMap<u64, u64>) {
        self.moving.clear();
        #[cfg(not(feature = "navigation"))]
        let _ = names;
        crate::feature!("navigation", {
            self.graphs = crate::navigation::graphs(world);
            if !self.graphs.is_empty() {
                for (e, a) in world.query::<&crate::navigation::NavAgent>().iter() {
                    if a.is_moving() {
                        let id = e.id() as u64;
                        self.moving.insert(id);
                        if let Some(&handle) = names.get(&id) {
                            self.moving.insert(handle);
                        }
                    }
                }
            }
        });
        self.maps.clear();
        for (entity, (tilemap, transform)) in world.query::<(&Tilemap, &Transform)>().iter() {
            let origin = crate::tilemap::world_origin(world, entity, transform);
            self.maps.insert(entity.id() as u64, (tilemap.clone(), (origin.x, origin.y)));
        }
    }

    /// True when no tilemap exists — the callbacks then answer `null`, which
    /// is what the native snapshot does for an unknown entity.
    pub fn is_empty(&self) -> bool {
        #[cfg(feature = "navigation")]
        return self.maps.is_empty() && self.graphs.is_empty();
        #[cfg(not(feature = "navigation"))]
        return self.maps.is_empty();
    }

    /// `Nav.find_path` from the frame's graphs.
    pub fn nav_path(&self, from: (f32, f32), to: (f32, f32)) -> Option<Vec<(f32, f32)>> {
        #[cfg(not(feature = "navigation"))]
        {
            let _ = (from, to);
            crate::features::missing("navigation", "Nav.find_path");
            return None;
        }
        #[cfg(feature = "navigation")]
        {
            let path = crate::navigation::find_path_in(&self.graphs, from.into(), to.into())?;
            Some(path.into_iter().map(|p| (p.x, p.y)).collect())
        }
    }

    /// `Nav.closest_point` from the frame's graphs.
    pub fn nav_closest(&self, p: (f32, f32)) -> Option<(f32, f32)> {
        #[cfg(not(feature = "navigation"))]
        {
            let _ = p;
            crate::features::missing("navigation", "Nav.closest_point");
            return None;
        }
        #[cfg(feature = "navigation")]
        crate::navigation::closest_point_in(&self.graphs, p.into()).map(|q| (q.x, q.y))
    }

    fn path_tiles(
        &self,
        entity_id: u64,
        layer: &str,
        start: (i32, i32),
        goal: (i32, i32),
        mode: &str,
    ) -> Option<Vec<(i32, i32)>> {
        let (tilemap, _) = self.maps.get(&entity_id)?;
        find_path(tilemap, layer, start, goal, PathMode::from_str_lossy(mode))
    }

    fn path_world(
        &self,
        entity_id: u64,
        layer: &str,
        start: (f32, f32),
        goal: (f32, f32),
        mode: &str,
    ) -> Option<Vec<(f32, f32)>> {
        let (tilemap, &(ex, ey)) = self.maps.get(&entity_id).map(|(t, o)| (t, o))?;
        let start = tilemap.world_to_tile(ex, ey, start.0, start.1);
        let goal = tilemap.world_to_tile(ex, ey, goal.0, goal.1);
        let tiles = find_path(tilemap, layer, start, goal, PathMode::from_str_lossy(mode))?;
        Some(
            tiles
                .into_iter()
                .map(|(tx, ty)| tilemap.tile_to_world(ex, ey, tx, ty))
                .collect(),
        )
    }
}

impl NavSnapshot {
    fn neighbors(&self, entity_id: u64, tx: i32, ty: i32) -> Vec<(i32, i32)> {
        self.maps.get(&entity_id).map(|(t, _)| t.neighbours_on_map(tx, ty)).unwrap_or_default()
    }

    fn distance(&self, entity_id: u64, a: (i32, i32), b: (i32, i32)) -> i32 {
        self.maps.get(&entity_id).map(|(t, _)| t.cell_distance(a, b) as i32).unwrap_or(-1)
    }

    /// A hexagonal map's cell picking is not two divisions, so the preamble
    /// asks here rather than keep a second copy of it in JavaScript.
    fn world_to_tile(&self, entity_id: u64, wx: f32, wy: f32) -> Option<(i32, i32)> {
        let (t, &(ex, ey)) = self.maps.get(&entity_id).map(|(t, o)| (t, o))?;
        Some(t.world_to_tile(ex, ey, wx, wy))
    }

    fn tile_to_world(&self, entity_id: u64, tx: i32, ty: i32) -> Option<(f32, f32)> {
        let (t, &(ex, ey)) = self.maps.get(&entity_id).map(|(t, o)| (t, o))?;
        Some(t.tile_to_world(ex, ey, tx, ty))
    }
}

/// Build the `[[x, y], …]` array scripts receive, or `null` for no path.
fn path_to_js<T: Into<f64> + Copy>(path: Option<Vec<(T, T)>>) -> JsValue {
    let Some(path) = path else {
        return JsValue::NULL;
    };
    let out = js_sys::Array::new();
    for (a, b) in path {
        let pair = js_sys::Array::new();
        pair.push(&JsValue::from_f64(a.into()));
        pair.push(&JsValue::from_f64(b.into()));
        out.push(&pair);
    }
    out.into()
}

/// One `[x, y]` pair, or `null`.
fn pair_to_js<T: Into<f64> + Copy>(p: Option<(T, T)>) -> JsValue {
    match p {
        Some((a, b)) => {
            let pair = js_sys::Array::new();
            pair.push(&JsValue::from_f64(a.into()));
            pair.push(&JsValue::from_f64(b.into()));
            pair.into()
        }
        None => JsValue::NULL,
    }
}

/// Install `tilemap_find_path` / `tilemap_find_path_world` (and the cell
/// queries `tilemap_neighbors`, `tilemap_distance`, `tilemap_world_to_tile`,
/// `tilemap_tile_to_world`) on `window.__jengine_play.state`, backed by
/// `snapshot`.
///
/// Idempotent per host: calling it again replaces the closures, which is
/// harmless. The closures are leaked deliberately (`into_js_value`) — they
/// live as long as the page, and a dropped `Closure` would leave JS holding a
/// dangling function that traps on call.
pub fn install_nav_callbacks(snapshot: Rc<RefCell<NavSnapshot>>) -> Result<(), JsValue> {
    let win = web_sys::window().ok_or(JsValue::NULL)?;
    let play_key = JsValue::from_str("__jengine_play");
    let play = js_sys::Reflect::get(&win, &play_key)?;
    let play: JsValue = if play.is_object() {
        play
    } else {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&win, &play_key, &obj)?;
        obj.into()
    };

    // The preamble reads every synchronous host hook off `state`; nothing
    // else creates it.
    let state_key = JsValue::from_str("state");
    let state = js_sys::Reflect::get(&play, &state_key)?;
    let state: JsValue = if state.is_object() {
        state
    } else {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&play, &state_key, &obj)?;
        obj.into()
    };

    {
        let snap = Rc::clone(&snapshot);
        let f = Closure::wrap(Box::new(
            move |entity: f64, layer: JsValue, sx: f64, sy: f64, gx: f64, gy: f64, mode: JsValue| {
                let layer = layer.as_string().unwrap_or_default();
                let mode = mode.as_string().unwrap_or_else(|| "manhattan".to_string());
                let snap = snap.borrow();
                path_to_js(snap.path_tiles(
                    entity as u64,
                    &layer,
                    (sx as i32, sy as i32),
                    (gx as i32, gy as i32),
                    &mode,
                ))
            },
        )
            as Box<dyn Fn(f64, JsValue, f64, f64, f64, f64, JsValue) -> JsValue>);
        js_sys::Reflect::set(
            &state,
            &JsValue::from_str("tilemap_find_path"),
            &f.into_js_value(),
        )?;
    }

    {
        let snap = Rc::clone(&snapshot);
        let f = Closure::wrap(Box::new(
            move |entity: f64, layer: JsValue, sx: f64, sy: f64, gx: f64, gy: f64, mode: JsValue| {
                let layer = layer.as_string().unwrap_or_default();
                let mode = mode.as_string().unwrap_or_else(|| "manhattan".to_string());
                let snap = snap.borrow();
                path_to_js(snap.path_world(
                    entity as u64,
                    &layer,
                    (sx as f32, sy as f32),
                    (gx as f32, gy as f32),
                    &mode,
                ))
            },
        )
            as Box<dyn Fn(f64, JsValue, f64, f64, f64, f64, JsValue) -> JsValue>);
        js_sys::Reflect::set(
            &state,
            &JsValue::from_str("tilemap_find_path_world"),
            &f.into_js_value(),
        )?;
    }

    {
        let snap = Rc::clone(&snapshot);
        let f = Closure::wrap(Box::new(move |entity: f64, tx: f64, ty: f64| {
            let cells = snap.borrow().neighbors(entity as u64, tx as i32, ty as i32);
            path_to_js(Some(cells))
        }) as Box<dyn Fn(f64, f64, f64) -> JsValue>);
        js_sys::Reflect::set(&state, &JsValue::from_str("tilemap_neighbors"), &f.into_js_value())?;
    }
    {
        let snap = Rc::clone(&snapshot);
        let f = Closure::wrap(Box::new(move |entity: f64, ax: f64, ay: f64, bx: f64, by: f64| {
            snap.borrow().distance(entity as u64, (ax as i32, ay as i32), (bx as i32, by as i32)) as f64
        }) as Box<dyn Fn(f64, f64, f64, f64, f64) -> f64>);
        js_sys::Reflect::set(&state, &JsValue::from_str("tilemap_distance"), &f.into_js_value())?;
    }
    {
        let snap = Rc::clone(&snapshot);
        let f = Closure::wrap(Box::new(move |entity: f64, wx: f64, wy: f64| {
            pair_to_js(snap.borrow().world_to_tile(entity as u64, wx as f32, wy as f32))
        }) as Box<dyn Fn(f64, f64, f64) -> JsValue>);
        js_sys::Reflect::set(&state, &JsValue::from_str("tilemap_world_to_tile"), &f.into_js_value())?;
    }
    {
        let snap = Rc::clone(&snapshot);
        let f = Closure::wrap(Box::new(move |entity: f64, tx: f64, ty: f64| {
            pair_to_js(snap.borrow().tile_to_world(entity as u64, tx as i32, ty as i32))
        }) as Box<dyn Fn(f64, f64, f64) -> JsValue>);
        js_sys::Reflect::set(&state, &JsValue::from_str("tilemap_tile_to_world"), &f.into_js_value())?;
    }
    {
        let snap = Rc::clone(&snapshot);
        let f = Closure::wrap(Box::new(move |x0: f64, y0: f64, x1: f64, y1: f64| {
            path_to_js(snap.borrow().nav_path((x0 as f32, y0 as f32), (x1 as f32, y1 as f32)))
        }) as Box<dyn Fn(f64, f64, f64, f64) -> JsValue>);
        js_sys::Reflect::set(&state, &JsValue::from_str("nav_find_path"), &f.into_js_value())?;
    }
    {
        let snap = Rc::clone(&snapshot);
        let f = Closure::wrap(Box::new(move |x: f64, y: f64| {
            pair_to_js(snap.borrow().nav_closest((x as f32, y as f32)))
        }) as Box<dyn Fn(f64, f64) -> JsValue>);
        js_sys::Reflect::set(&state, &JsValue::from_str("nav_closest_point"), &f.into_js_value())?;
    }
    {
        let snap = Rc::clone(&snapshot);
        let f = Closure::wrap(Box::new(move |entity: f64| snap.borrow().moving.contains(&(entity as u64)))
            as Box<dyn Fn(f64) -> bool>);
        js_sys::Reflect::set(&state, &JsValue::from_str("nav_is_moving"), &f.into_js_value())?;
    }
    // The language's hooks need no snapshot; they ride along here because
    // both hosts call this once per Play.
    super::locale::install_locale_callbacks(&state)?;
    super::haptics::install_haptics_callbacks(&state)?;
    super::display::install_display_callbacks(&state)?;
    super::streaming::install_streaming_callbacks(&state)?;
    super::packs::install_pack_callbacks(&state)?;
    super::liveops::install_liveops_callbacks(&state)?;
    super::input_query::install_input_query_callbacks(&state)?;
    super::music::install_music_callbacks(&state)?;
    super::noise::install_noise_callbacks(&state)?;
    super::script_math::install_script_math()?;
    super::analysis::install_analysis_callbacks(&state)?;
    super::screenshot::install_screenshot_callbacks(&state)?;
    super::accessibility::install_accessibility_callbacks(&state)?;

    Ok(())
}
