//! Keyframe timelines (Oct 2026; Godot's `AnimationPlayer`, Unity's
//! Timeline): clips of **tracks** that animate any field of any component,
//! and **event keys** that call the timeline entity's own script.
//!
//! A `timeline` holds its clips inline, so a scene or a prefab carries them
//! and no host delivers an asset. A track names an entity (by `Name`, a
//! descendant of the timeline's entity first; empty for the timeline's own),
//! a component by its scene key (`"transform"`, `"sprite"`, `"point_light"`,
//! a package's) and a field path inside it (`"position"`, `"color"`,
//! `"color.3"`). Numbers and same-length lists of numbers are interpolated
//! by the track's `ease`; anything else - a texture id, a flag, a string -
//! holds until the next key. A track writes only when its value changes, so
//! a hold costs nothing and a script may move the field between keys.
//!
//! Writing goes through the component registry's own serde (read the
//! component, set the field, write it back), which is what lets a track
//! reach every component without a setter each - and why a component that
//! keeps unsaved runtime state (a cloth's points, a jiggle's swing) restarts
//! that state when a track writes it. Animate those through an event key and
//! the component's own setter.
//!
//! An event key `[t, "name"]` calls `on_<name>(entity)` on the timeline
//! entity's script as the playhead passes it, and a clip that does not loop
//! calls `on_timeline_end(entity)` when it finishes - both through
//! `ecs::callbacks`, so no host is wired. Ticked inside `tick_motion`.

use std::collections::BTreeMap;

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ecs::components::{Name, Parent};

/// How a track moves between two keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Ease {
    #[default]
    Linear,
    /// Hold each key until the next.
    Step,
    /// Slow out of the key.
    In,
    /// Slow into the next.
    Out,
    /// Both.
    InOut,
}

impl Ease {
    pub fn apply(self, f: f32) -> f32 {
        let f = f.clamp(0.0, 1.0);
        match self {
            Ease::Linear => f,
            Ease::Step => 0.0,
            Ease::In => f * f,
            Ease::Out => 1.0 - (1.0 - f) * (1.0 - f),
            Ease::InOut => f * f * (3.0 - 2.0 * f),
        }
    }
}

/// One animated field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Track {
    /// The entity's `Name`; empty for the timeline's own entity.
    pub target: String,
    /// The component's scene key.
    pub component: String,
    /// The field, dotted into objects and lists: `"position"`, `"color.3"`.
    pub field: String,
    /// `[time, value]` pairs, in time order.
    pub keys: Vec<(f32, Value)>,
    pub ease: Ease,
}

/// One clip: tracks, event keys and a length.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TimelineClip {
    /// Seconds; 0 takes the last key's time.
    pub duration: f32,
    pub tracks: Vec<Track>,
    /// `[time, name]`: calls `on_<name>(entity)` on the timeline's script.
    pub events: Vec<(f32, String)>,
}

impl TimelineClip {
    /// The clip's length: `duration`, or its last key.
    pub fn length(&self) -> f32 {
        if self.duration > 0.0 {
            return self.duration;
        }
        let tracks = self.tracks.iter().flat_map(|t| t.keys.iter().map(|k| k.0));
        let events = self.events.iter().map(|e| e.0);
        tracks.chain(events).fold(0.0, f32::max)
    }
}

/// A timeline: named clips, and which one plays.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Timeline {
    pub clips: BTreeMap<String, TimelineClip>,
    /// The clip playing; empty for the first by name.
    pub clip: String,
    pub playing: bool,
    /// The playhead, in seconds.
    pub time: f32,
    pub speed: f32,
    pub looping: bool,
    #[serde(skip)]
    pub state: TimelineState,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            clips: BTreeMap::new(),
            clip: String::new(),
            playing: true,
            time: 0.0,
            speed: 1.0,
            looping: false,
            state: TimelineState::default(),
        }
    }
}

impl PartialEq for Timeline {
    fn eq(&self, o: &Self) -> bool {
        self.clips == o.clips
            && self.clip == o.clip
            && self.playing == o.playing
            && self.time == o.time
            && self.speed == o.speed
            && self.looping == o.looping
    }
}

/// What a timeline remembers between frames. Never saved.
#[derive(Debug, Clone, Default)]
pub struct TimelineState {
    /// The clip the rest was made for.
    clip: Option<String>,
    /// Where the playhead was after the last tick (`None` before the first,
    /// so events at 0 fire).
    last: Option<f32>,
    /// Per track: its entity, and the value it last wrote.
    tracks: Vec<(Option<Entity>, Option<Value>)>,
}

impl Timeline {
    /// The clip to play: `clip`, or the first by name.
    pub fn current(&self) -> Option<(&str, &TimelineClip)> {
        if self.clip.is_empty() {
            self.clips.iter().next().map(|(k, c)| (k.as_str(), c))
        } else {
            self.clips.get_key_value(&self.clip).map(|(k, c)| (k.as_str(), c))
        }
    }
}

/// A track's value at `t`: before the first key the first, after the last
/// the last, between two keys interpolated - numbers and same-length lists
/// of numbers by `ease`, anything else held.
pub fn sample(track: &Track, t: f32) -> Option<Value> {
    let keys = &track.keys;
    let first = keys.first()?;
    if t <= first.0 || keys.len() == 1 {
        return Some(first.1.clone());
    }
    let i = keys.iter().rposition(|k| k.0 <= t)?;
    let (t0, v0) = &keys[i];
    let Some((t1, v1)) = keys.get(i + 1) else { return Some(v0.clone()) };
    let f = if *t1 > *t0 { track.ease.apply((t - t0) / (t1 - t0)) } else { 1.0 };
    Some(lerp(v0, v1, f))
}

fn lerp(a: &Value, b: &Value, f: f32) -> Value {
    let f = f as f64;
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => serde_json::json!(x + (y - x) * f),
            _ => a.clone(),
        },
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() && x.iter().chain(y).all(Value::is_number) => {
            Value::Array(x.iter().zip(y).map(|(p, q)| lerp(p, q, f as f32)).collect())
        }
        _ => {
            if f >= 1.0 {
                b.clone()
            } else {
                a.clone()
            }
        }
    }
}


pub use crate::package::component_registry::{set_path, write_field};

/// The entity a track names: the timeline's own for an empty name, else one
/// so named under the timeline's entity, else any so named.
fn resolve(world: &World, owner: Entity, name: &str) -> Option<Entity> {
    if name.is_empty() {
        return Some(owner);
    }
    let named: Vec<Entity> = world.query::<&Name>().iter().filter(|(_, n)| n.0 == name).map(|(e, _)| e).collect();
    let under = |mut e: Entity| {
        for _ in 0..64 {
            match world.get::<&Parent>(e) {
                Ok(p) if p.0 == owner => return true,
                Ok(p) => e = p.0,
                Err(_) => return false,
            }
        }
        false
    };
    named.iter().copied().find(|e| under(*e)).or_else(|| named.first().copied())
}


pub(crate) use super::callbacks::handler_for;

/// Advance every playing timeline by `dt`, write what its tracks say and
/// call the event keys it passed. See the module docs.
pub fn tick_timelines(world: &mut World, dt: f32) {
    let owners: Vec<Entity> = world.query::<&Timeline>().iter().map(|(e, _)| e).collect();
    for owner in owners {
        let Some((writes, events, ended)) = advance(world, owner, dt) else { continue };
        for (entity, component, field, value) in writes {
            if let Err(e) = write_field(world, entity, &component, &field, &value) {
                log::warn!("timeline on entity {}: {component}.{field}: {e}", owner.id());
            }
        }
        for name in events {
            crate::ecs::callbacks::push(owner, handler_for(&name));
        }
        if ended {
            crate::ecs::callbacks::push(owner, "on_timeline_end");
        }
    }
}

type Writes = Vec<(Entity, String, String, Value)>;

/// One timeline's step: what to write, which events it passed, and whether
/// it finished.
fn advance(world: &mut World, owner: Entity, dt: f32) -> Option<(Writes, Vec<String>, bool)> {
    // Work on a copy of the state, so the world is free to resolve names.
    let (clip_name, clip, mut state, time, playing, speed, looping) = {
        let tl = world.get::<&Timeline>(owner).ok()?;
        let (name, clip) = tl.current()?;
        (name.to_string(), clip.clone(), tl.state.clone(), tl.time, tl.playing, tl.speed, tl.looping)
    };
    if state.clip.as_deref() != Some(clip_name.as_str()) || state.tracks.len() != clip.tracks.len() {
        state = TimelineState { clip: Some(clip_name), last: None, tracks: vec![(None, None); clip.tracks.len()] };
    }
    let length = clip.length();
    let mut now = time;
    let mut ended = false;
    let mut passed: Vec<String> = Vec::new();
    // Events in (from, to]; `None` takes the clip's start, so a key at 0 fires.
    let fire = |from: Option<f32>, to: f32, out: &mut Vec<String>| {
        for (t, name) in &clip.events {
            if from.is_none_or(|f| *t > f) && *t <= to {
                out.push(name.clone());
            }
        }
    };
    if playing && dt > 0.0 {
        now = time + dt * speed;
        if length > 0.0 && now >= length {
            if looping {
                fire(state.last, length, &mut passed);
                now %= length;
                state.last = None;
            } else {
                now = length;
                ended = true;
            }
        }
    }
    if playing || state.last.is_none() {
        fire(state.last, now, &mut passed);
    }
    state.last = Some(now);

    let mut writes = Vec::new();
    for (i, track) in clip.tracks.iter().enumerate() {
        let alive = state.tracks[i].0.is_some_and(|e| world.contains(e));
        if !alive {
            state.tracks[i].0 = resolve(world, owner, &track.target);
        }
        let (Some(target), Some(value)) = (state.tracks[i].0, sample(track, now)) else { continue };
        if state.tracks[i].1.as_ref() != Some(&value) {
            state.tracks[i].1 = Some(value.clone());
            writes.push((target, track.component.clone(), track.field.clone(), value));
        }
    }

    if let Ok(mut tl) = world.get::<&mut Timeline>(owner) {
        tl.time = now;
        if ended {
            tl.playing = false;
        }
        tl.state = state;
    }
    Some((writes, passed, ended))
}

/// `Timeline.set(e, field, value)`: write any field by name. `"clip"` starts
/// that clip from its beginning; `"time"` seeks without firing the events
/// skipped over.
pub fn set_timeline_field(world: &World, entity: Entity, field: &str, value: &Value) -> Result<(), String> {
    let Ok(mut tl) = world.get::<&mut Timeline>(entity) else {
        return Err("no timeline on that entity".into());
    };
    let mut whole = serde_json::to_value(&*tl).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a timeline field"));
    }
    let v = match (map.get(field), value) {
        (Some(Value::Bool(_)), Value::Number(n)) => Value::Bool(n.as_f64().unwrap_or(0.0) != 0.0),
        _ => value.clone(),
    };
    map.insert(field.to_string(), v);
    let mut new: Timeline = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    new.state = std::mem::take(&mut tl.state);
    match field {
        "clip" => {
            new.time = 0.0;
            new.playing = true;
            new.state = TimelineState::default();
        }
        "time" => new.state.last = Some(new.time),
        _ => {}
    }
    *tl = new;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn track(keys: Vec<(f32, Value)>, ease: Ease) -> Track {
        Track { target: String::new(), component: "transform".into(), field: "rotation".into(), keys, ease }
    }

    #[test]
    fn numbers_and_lists_interpolate_and_the_rest_holds() {
        let t = track(vec![(0.0, json!(0.0)), (2.0, json!(10.0))], Ease::Linear);
        assert_eq!(sample(&t, 1.0), Some(json!(5.0)));
        assert_eq!(sample(&t, -1.0), Some(json!(0.0)));
        assert_eq!(sample(&t, 9.0), Some(json!(10.0)));
        let list = track(vec![(0.0, json!([0.0, 2.0])), (1.0, json!([4.0, 6.0]))], Ease::Linear);
        assert_eq!(sample(&list, 0.5), Some(json!([2.0, 4.0])));
        let text = track(vec![(0.0, json!("a.png")), (1.0, json!("b.png"))], Ease::Linear);
        assert_eq!(sample(&text, 0.99), Some(json!("a.png")));
        assert_eq!(sample(&text, 1.0), Some(json!("b.png")));
        let step = track(vec![(0.0, json!(0.0)), (1.0, json!(8.0))], Ease::Step);
        assert_eq!(sample(&step, 0.9), Some(json!(0.0)));
        let ease = track(vec![(0.0, json!(0.0)), (1.0, json!(1.0))], Ease::InOut);
        assert!(sample(&ease, 0.25).unwrap().as_f64().unwrap() < 0.25);
    }

    #[test]
    fn a_path_reaches_inside_and_rounds_integers() {
        let mut v = json!({ "color": [1.0, 1.0, 1.0, 1.0], "z_order": 3 });
        set_path(&mut v, "color.3", &json!(0.5)).unwrap();
        set_path(&mut v, "z_order", &json!(6.6)).unwrap();
        assert_eq!(v, json!({ "color": [1.0, 1.0, 1.0, 0.5], "z_order": 7 }));
        assert!(set_path(&mut v, "colour", &json!(1)).is_err());
    }
}
