//! Telling each player about what is near them, and only what changed.
//!
//! A relay sends everything to everyone, which is right for four players and
//! ruinous for four hundred: the traffic goes with the square of the room. A
//! [`Space`] is the other shape. The room puts **entities** in it (a position
//! and a bag of properties), says who is **watching** from where, and once a
//! tick [`Space::flush`] works out one frame per watcher:
//!
//! ```json
//! {"t":"sync",
//!  "enter":[{"id":7,"x":120,"y":40,"kind":"wolf","hp":30}],
//!  "move":[[3,64.5,12],[5,80,12.5]],
//!  "update":[{"id":4,"hp":22}],
//!  "leave":[9]}
//! ```
//!
//! `enter` carries a whole entity the watcher has not seen; `move` is
//! `[id, x, y]` for each one that only moved; `update` is the changed fields
//! of one whose properties changed (with its position too, if that also
//! did); `leave` is the ids that went out of range or out of the world.
//!
//! **Movement is most of what a busy zone says**, which is why it has the
//! tersest form: three numbers, where an object naming its fields was
//! nearly twice the bytes - and bytes, not CPU, are a crowded zone's first
//! limit. A watcher near nothing that changed
//! gets no frame at all.
//!
//! **Frames come out as text, already serialised.** An entity that moved is
//! written once per tick and that text is shared by everyone who can see it,
//! so a crowd costs a copy per watcher rather than a serialisation - which is
//! most of what a tick of a busy zone would otherwise be (`examples/zone_cost`
//! measures it). Hand them to [`crate::RoomCtx::send_text`].
//!
//! **A crowd is worked out on several cores.** Each watcher's frame depends
//! only on the entities and its own view, so when a flush has a lot to do -
//! hundreds of watchers each seeing hundreds of things, the crowded zone
//! whose tick spiked past its budget in the load test (V4) - the watchers
//! are split across threads for that flush and their frames joined in the
//! same order. A quiet zone stays on its own task. [`Space::flush_threads`]
//! caps it (1 keeps every flush on one core).
//!
//! **Range has hysteresis.** Something comes into view at `radius` and stays
//! until `radius * 1.25`, so an entity pacing along the edge of someone's view
//! is not sent whole every other tick.
//!
//! # For prediction
//!
//! Every frame carries the `tick` it was made at, and a watcher whose input
//! the room has applied is told which one (`"ack": 41`, after
//! [`Space::ack`]) - with the position that gave, in the same frame. That
//! pair is what a predicting client reconciles against; see the
//! `jengine-netcode` crate (`crate::netcode`), which both ends link.
//! [`Space::remember`] keeps where everything was for the last so many
//! ticks, so a room can judge a hit at the tick the shooter saw
//! ([`Space::position_at`]).
//!
//! # Line of sight
//!
//! Range is what a field sees. Underground it is not: a monster round a
//! corner is three tiles away and hidden. [`Space::watch_sight`] gives one
//! watcher a [`Sight`] - which cells of a grid they can see, worked out by
//! the room (shadowcasting, a cone, whatever its game means by "seen") - and
//! from then on an entity is told to them only while it is in range **and**
//! in a cell they see. Out of sight is out of view: it leaves at the next
//! flush, with no hysteresis, because a wall is not an edge to pace along.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use serde_json::{Map, Value, json};

use crate::room::PlayerId;

pub type EntityId = u64;

/// Entity ids are small counters looked up several times per watcher per
/// tick; the standard hasher is built to resist chosen keys, which these are
/// not, and was most of a flush.
#[derive(Default)]
struct IdHasher(u64);

impl std::hash::Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = n.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

type IdMap<V> = HashMap<EntityId, V, std::hash::BuildHasherDefault<IdHasher>>;

/// How much farther than `radius` an entity stays known.
const LEAVE_FACTOR: f64 = 1.25;

/// Which cells of a grid one watcher can see - line of sight, worked out by
/// the room - for [`Space::watch_sight`].
///
/// The grid is laid out as `netcode::Walls` is, and as an engine tilemap
/// is: `origin` is the top-left corner, columns run right and rows run
/// **down** (towards smaller y), so a room can build its walls, its sight
/// and its client's tilemap on one set of numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Sight {
    pub cols: usize,
    pub rows: usize,
    pub cell: f64,
    pub origin: (f64, f64),
    /// One flag per cell, row by row from the top.
    pub visible: Vec<bool>,
}

impl Sight {
    /// A grid with nothing visible yet.
    pub fn new(cols: usize, rows: usize, cell: f64, origin: (f64, f64)) -> Self {
        Sight { cols, rows, cell, origin, visible: vec![false; cols * rows] }
    }

    /// Whether the cell holding a point is visible. Off the grid is not.
    pub fn sees(&self, x: f64, y: f64) -> bool {
        let col = ((x - self.origin.0) / self.cell).floor();
        let row = ((self.origin.1 - y) / self.cell).floor();
        if col < 0.0 || row < 0.0 || col >= self.cols as f64 || row >= self.rows as f64 {
            return false;
        }
        self.visible.get(row as usize * self.cols + col as usize).copied().unwrap_or(false)
    }
}

struct Entity {
    x: f64,
    y: f64,
    /// Which layers it is on: a bit each. A watcher sees it if they share one.
    layers: u32,
    /// Its facing, in radians, once the room has given it one - sent as a
    /// fourth number on every move, and walked between ticks like the
    /// position, so a client turns it smoothly.
    rot: Option<f64>,
    pos_rev: u64,
    /// name -> (value, the revision it was set at)
    props: BTreeMap<String, (Value, u64)>,
    cell: (i32, i32),
    /// Changed since the last flush, and so in `Space::dirty`.
    dirty: bool,
    /// The only watchers told of it, sorted - `None` is everyone its layers
    /// allow ([`Space::only_for`]).
    audience: Option<Vec<PlayerId>>,
}

impl Entity {
    /// Whether `watcher`, who sees `layers`, may be told of it at all.
    fn shown_to(&self, watcher: &str, layers: u32) -> bool {
        self.layers & layers != 0
            && self.audience.as_ref().is_none_or(|a| a.binary_search_by(|p| p.as_str().cmp(watcher)).is_ok())
    }
}

/// One entity's change, already text.
enum Change {
    Move(String),
    Update(String),
}

struct Watcher {
    x: f64,
    y: f64,
    /// Which layers this watcher sees - every one, unless told otherwise.
    sees: u32,
    follow: Option<EntityId>,
    /// How far this watcher sees, if not as far as the space allows.
    radius: Option<f64>,
    /// The last of this watcher's inputs the room applied, and whether they
    /// have been told.
    ack: Option<u32>,
    ack_sent: Option<u32>,
    /// The cells this watcher can see, if the room says; `None` sees
    /// everything in range.
    sight: Option<Sight>,
    /// What this watcher has been told of and not told has left, sorted.
    /// Every flush brings a watcher up to date with all of it, so one
    /// revision - the space's last flush - stands for the lot.
    known: Vec<EntityId>,
}

pub struct Space {
    radius: f64,
    cell: f64,
    /// Bumped by every change; an entity's fields remember the value it had.
    rev: u64,
    /// `rev` as of the last flush: what every watcher is up to date with.
    flushed: u64,
    next_id: EntityId,
    entities: IdMap<Entity>,
    dirty: Vec<EntityId>,
    grid: HashMap<(i32, i32), Vec<EntityId>>,
    watchers: BTreeMap<PlayerId, Watcher>,
    /// How many times this space has been flushed: the tick its frames carry.
    tick: u64,
    /// Where everything was, if the room asked for that to be kept.
    history: Option<crate::netcode::History>,
    /// Ids drawn at random, from this state, instead of counted
    /// ([`Space::opaque_ids`]).
    random_ids: Option<u64>,
    /// The most threads a flush may use; `None` is as many as are worth it.
    threads: Option<usize>,
}

/// Below this much work - watchers times what each knows - a flush stays on
/// one thread: starting threads costs more than it saves.
const PARALLEL_WORK: usize = 20_000;
/// Fewest watchers worth giving a thread of their own.
const WATCHERS_PER_THREAD: usize = 32;

/// Opaque ids stay below this: a client's numbers may be Lua's, which are
/// floats, exact only to 2^53.
const OPAQUE_ID_LIMIT: u64 = 1 << 48;

/// The layer everything is on unless put on another.
pub const DEFAULT_LAYER: u32 = 1;
/// Every layer: what a watcher sees unless told otherwise.
pub const EVERY_LAYER: u32 = u32::MAX;

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// A thousandth of a radian: finer than a pixel at the edge of anything a
/// screen shows.
fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

impl Space {
    /// A space whose watchers see `radius` units around them.
    pub fn new(radius: f64) -> Self {
        // One cell per leave-radius: a watcher's view is at most the nine
        // cells around it.
        Self::with_cell(radius, radius.max(1.0) * LEAVE_FACTOR)
    }

    /// A space whose watchers see `radius` units around them by default,
    /// with grid cells of `cell` units. A watcher scans the cells its own
    /// view covers, so in a world whose views differ widely (a small cell
    /// that sees 560 units beside a whale that sees 1200) cells sized for
    /// the common view keep the small ones from scanning the whale's nine.
    pub fn with_cell(radius: f64, cell: f64) -> Self {
        let radius = radius.max(1.0);
        Space {
            radius,
            cell: cell.max(1.0),
            rev: 0,
            flushed: 0,
            next_id: 1,
            entities: IdMap::default(),
            dirty: Vec::new(),
            grid: HashMap::new(),
            watchers: BTreeMap::new(),
            tick: 0,
            history: None,
            random_ids: None,
            threads: None,
        }
    }

    /// The most threads one flush may use. `1` keeps every flush on the
    /// room's own task; the default uses the machine's cores (up to eight)
    /// when a flush is big enough to be worth it.
    pub fn flush_threads(mut self, threads: usize) -> Self {
        self.threads = Some(threads.max(1));
        self
    }

    /// [`Space::flush_threads`], on a space already made.
    pub fn set_flush_threads(&mut self, threads: usize) {
        self.threads = Some(threads.max(1));
    }

    /// Hand out ids at random (from `seed`) rather than counting up from 1,
    /// so an id says nothing about when or in what order a thing was made -
    /// which of a round's props is a hider, which ship joined first. Under
    /// 2^48, so a client's Lua keeps them exact.
    pub fn opaque_ids(mut self, seed: u64) -> Self {
        self.random_ids = Some(seed | 1);
        self
    }

    fn take_id(&mut self) -> EntityId {
        let Some(state) = &mut self.random_ids else {
            let id = self.next_id;
            self.next_id += 1;
            return id;
        };
        loop {
            // xorshift64*: cheap, and a different id stream per seed.
            *state ^= *state >> 12;
            *state ^= *state << 25;
            *state ^= *state >> 27;
            let id = state.wrapping_mul(0x2545_f491_4f6c_dd1d) % OPAQUE_ID_LIMIT;
            if id != 0 && !self.entities.contains_key(&id) {
                return id;
            }
        }
    }

    fn cell_of(&self, x: f64, y: f64) -> (i32, i32) {
        ((x / self.cell).floor() as i32, (y / self.cell).floor() as i32)
    }

    pub fn spawn(&mut self, x: f64, y: f64, props: Value) -> EntityId {
        self.spawn_on(x, y, props, DEFAULT_LAYER)
    }

    /// Spawn on particular layers (a bit each): a ghost on layer 2 is seen
    /// only by a watcher told to see layer 2 ([`Space::watch_layers`]) - a
    /// medium, a player on the same floor, a scout with the right item.
    /// Layers are about *who is told*; the grid and [`Space::near`] see
    /// everything.
    pub fn spawn_on(&mut self, x: f64, y: f64, props: Value, layers: u32) -> EntityId {
        let id = self.take_id();
        self.rev += 1;
        let cell = self.cell_of(x, y);
        let props = match props {
            Value::Object(map) => map.into_iter().map(|(k, v)| (k, (v, self.rev))).collect(),
            _ => BTreeMap::new(),
        };
        self.entities.insert(id, Entity { x, y, layers, rot: None, pos_rev: self.rev, props, cell, dirty: false, audience: None });
        self.grid.entry(cell).or_default().push(id);
        id
    }

    pub fn despawn(&mut self, id: EntityId) {
        if let Some(e) = self.entities.remove(&id)
            && let Some(ids) = self.grid.get_mut(&e.cell)
        {
            ids.retain(|i| *i != id);
        }
    }

    pub fn position(&self, id: EntityId) -> Option<(f64, f64)> {
        self.entities.get(&id).map(|e| (e.x, e.y))
    }

    pub fn layers(&self, id: EntityId) -> Option<u32> {
        self.entities.get(&id).map(|e| e.layers)
    }

    /// Move an entity between layers. Whoever can no longer see it is told
    /// it left, whoever now can is told it entered, at the next flush.
    pub fn set_layers(&mut self, id: EntityId, layers: u32) {
        if let Some(e) = self.entities.get_mut(&id) {
            e.layers = layers;
        }
    }

    /// Only these watchers are told of `id` - a hider's own body, a hand
    /// of cards, a note passed to one player - whatever its layers say. The
    /// others are told it left at the next flush. Unlike a layer a seat it
    /// costs nothing, so there is no limit to how many players can each
    /// have their own.
    pub fn only_for<P: AsRef<str>>(&mut self, id: EntityId, players: impl IntoIterator<Item = P>) {
        if let Some(e) = self.entities.get_mut(&id) {
            let mut audience: Vec<PlayerId> = players.into_iter().map(|p| p.as_ref().to_string()).collect();
            audience.sort_unstable();
            audience.dedup();
            e.audience = Some(audience);
        }
    }

    /// `id` is told to whoever its layers allow again.
    pub fn for_everyone(&mut self, id: EntityId) {
        if let Some(e) = self.entities.get_mut(&id) {
            e.audience = None;
        }
    }

    /// Which layers a watcher sees. `0` sees nothing at all.
    pub fn watch_layers(&mut self, player: &str, sees: u32) {
        if let Some(w) = self.watchers.get_mut(player) {
            w.sees = sees;
        }
    }

    pub fn get(&self, id: EntityId, key: &str) -> Option<&Value> {
        self.entities.get(&id)?.props.get(key).map(|(v, _)| v)
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Move an entity. Moving it to where it is costs nothing and sends nothing.
    pub fn set_position(&mut self, id: EntityId, x: f64, y: f64) {
        let cell = self.cell_of(x, y);
        let Some(e) = self.entities.get_mut(&id) else { return };
        if e.x == x && e.y == y {
            return;
        }
        self.rev += 1;
        (e.x, e.y, e.pos_rev) = (x, y, self.rev);
        if !std::mem::replace(&mut e.dirty, true) {
            self.dirty.push(id);
        }
        if e.cell != cell {
            let old = std::mem::replace(&mut e.cell, cell);
            if let Some(ids) = self.grid.get_mut(&old) {
                ids.retain(|i| *i != id);
            }
            self.grid.entry(cell).or_default().push(id);
        }
    }

    /// Turn an entity to face `radians`. It goes out with the position (as
    /// `[id, x, y, rot]`) rather than as a property, so a ship that turns
    /// as it sails costs four numbers a tick, and a client walks the turn
    /// between ticks the short way round. Turning it to where it faces
    /// sends nothing.
    pub fn set_rotation(&mut self, id: EntityId, radians: f64) {
        let Some(e) = self.entities.get_mut(&id) else { return };
        if e.rot.is_some_and(|r| round3(r) == round3(radians)) {
            return;
        }
        self.rev += 1;
        (e.rot, e.pos_rev) = (Some(radians), self.rev);
        if !std::mem::replace(&mut e.dirty, true) {
            self.dirty.push(id);
        }
    }

    pub fn rotation(&self, id: EntityId) -> Option<f64> {
        self.entities.get(&id)?.rot
    }

    /// Set a property. Setting it to what it is sends nothing.
    pub fn set(&mut self, id: EntityId, key: &str, value: impl Into<Value>) {
        let value = value.into();
        let Some(e) = self.entities.get_mut(&id) else { return };
        if e.props.get(key).is_some_and(|(v, _)| *v == value) {
            return;
        }
        self.rev += 1;
        e.props.insert(key.to_string(), (value, self.rev));
        if !std::mem::replace(&mut e.dirty, true) {
            self.dirty.push(id);
        }
    }

    /// Entities within `radius` of a point - for the room's own rules (who a
    /// swing hits, what a wolf can smell), on the grid the sync already keeps.
    pub fn near(&self, x: f64, y: f64, radius: f64) -> Vec<EntityId> {
        let (c0, c1) = (self.cell_of(x - radius, y - radius), self.cell_of(x + radius, y + radius));
        let mut found = Vec::new();
        for cx in c0.0..=c1.0 {
            for cy in c0.1..=c1.1 {
                for id in self.grid.get(&(cx, cy)).into_iter().flatten() {
                    let e = &self.entities[id];
                    if (e.x - x).hypot(e.y - y) <= radius {
                        found.push(*id);
                    }
                }
            }
        }
        found.sort_unstable();
        found
    }

    /// A player starts watching from a point.
    pub fn watch(&mut self, player: &str, x: f64, y: f64) {
        self.watchers.insert(
            player.to_string(),
            Watcher {
                x,
                y,
                sees: EVERY_LAYER,
                follow: None,
                radius: None,
                ack: None,
                ack_sent: None,
                sight: None,
                known: Vec::new(),
            },
        );
    }

    /// A player starts watching from wherever an entity is - their avatar.
    pub fn watch_from(&mut self, player: &str, entity: EntityId) {
        self.watchers.insert(
            player.to_string(),
            Watcher {
                x: 0.0,
                y: 0.0,
                sees: EVERY_LAYER,
                follow: Some(entity),
                radius: None,
                ack: None,
                ack_sent: None,
                sight: None,
                known: Vec::new(),
            },
        );
    }

    pub fn move_watcher(&mut self, player: &str, x: f64, y: f64) {
        if let Some(w) = self.watchers.get_mut(player) {
            (w.x, w.y, w.follow) = (x, y, None);
        }
    }

    /// The room has applied this watcher's input number `seq`. They are told
    /// in their next frame, beside the position it left them at - and get a
    /// frame for it even if nothing else changed.
    pub fn ack(&mut self, player: &str, seq: u32) {
        if let Some(w) = self.watchers.get_mut(player) {
            w.ack = Some(seq);
        }
    }

    /// The tick the next frames will carry.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Keep where every entity was for the last `ticks` flushes, so that
    /// [`Space::position_at`] can answer. Costs a copy of every position
    /// that **moved** each tick, so it is asked for rather than always on.
    pub fn remember(&mut self, ticks: usize) {
        self.history = Some(crate::netcode::History::new(ticks));
    }

    /// Where an entity was at an earlier tick - the one a player says they
    /// were looking at when they fired. `None` without [`Space::remember`],
    /// for an entity that was not there, and for a tick older than is
    /// remembered - the room decides what a too-old claim is worth (judged
    /// where the target is now, with `.or_else(|| space.position(id))`, or
    /// not at all).
    pub fn position_at(&self, id: EntityId, tick: u64) -> Option<(f64, f64)> {
        self.history.as_ref()?.at(id, tick)
    }

    /// Let one watcher see less far - or farther - than the rest: a player
    /// in fog, a lookout in a crow's nest. A watcher scans the grid cells its
    /// own view covers, so any radius works; one much larger than the cells
    /// scans many of them.
    pub fn watch_radius(&mut self, player: &str, radius: f64) {
        if let Some(w) = self.watchers.get_mut(player) {
            w.radius = Some(radius.max(1.0));
        }
    }

    /// What one watcher can see, cell by cell - their line of sight, as the
    /// room works it out each time it changes. `None` goes back to seeing
    /// everything in range. Range still applies: a sight wider than the
    /// space's radius sees no farther.
    pub fn watch_sight(&mut self, player: &str, sight: Option<Sight>) {
        if let Some(w) = self.watchers.get_mut(player) {
            w.sight = sight;
        }
    }

    pub fn unwatch(&mut self, player: &str) {
        self.watchers.remove(player);
    }

    fn whole(id: EntityId, e: &Entity) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), json!(id));
        m.insert("x".into(), json!(round2(e.x)));
        m.insert("y".into(), json!(round2(e.y)));
        if let Some(rot) = e.rot {
            m.insert("rot".into(), json!(round3(rot)));
        }
        for (k, (v, _)) in &e.props {
            m.insert(k.clone(), v.clone());
        }
        Value::Object(m)
    }

    /// What changed about an entity since `since`, written out: a `move`
    /// triple if it only moved, an `update` object if a property changed.
    fn changes(id: EntityId, e: &Entity, since: u64) -> Option<Change> {
        let moved = e.pos_rev > since;
        let mut m = Map::new();
        for (k, (v, rev)) in &e.props {
            if *rev > since {
                m.insert(k.clone(), v.clone());
            }
        }
        if m.is_empty() {
            return moved.then(|| match e.rot {
                Some(rot) => Change::Move(format!("[{id},{},{},{}]", round2(e.x), round2(e.y), round3(rot))),
                None => Change::Move(format!("[{id},{},{}]", round2(e.x), round2(e.y))),
            });
        }
        if moved {
            m.insert("x".into(), json!(round2(e.x)));
            m.insert("y".into(), json!(round2(e.y)));
            if let Some(rot) = e.rot {
                m.insert("rot".into(), json!(round3(rot)));
            }
        }
        m.insert("id".into(), json!(id));
        Some(Change::Update(Value::Object(m).to_string()))
    }

    /// One `sync` frame, as text, for each watcher with something to hear.
    pub fn flush(&mut self) -> Vec<(PlayerId, String)> {
        self.tick += 1;
        let tick = self.tick;
        if let Some(history) = &mut self.history {
            history.record(tick, self.entities.iter().map(|(id, e)| (*id, (e.x, e.y))));
        }
        // What changed since the last flush, written once for everyone.
        let mut changed: IdMap<Change> = IdMap::default();
        for id in self.dirty.drain(..) {
            if let Some(e) = self.entities.get_mut(&id) {
                e.dirty = false;
                if let Some(change) = Self::changes(id, e, self.flushed) {
                    changed.insert(id, change);
                }
            }
        }
        let world = World { entities: &self.entities, grid: &self.grid, cell: self.cell, radius: self.radius, changed: &changed, tick };
        let mut watchers: Vec<(&PlayerId, &mut Watcher)> = self.watchers.iter_mut().collect();
        let work: usize = watchers.iter().map(|(_, w)| w.known.len() + 1).sum();
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8);
        let threads = self
            .threads
            .unwrap_or(cores)
            .min(watchers.len() / WATCHERS_PER_THREAD)
            .max(1);
        let frames = if threads <= 1 || work < PARALLEL_WORK {
            world.frames(&mut watchers)
        } else {
            // Every watcher's frame is its own: split them across threads,
            // and join what comes back in the order they were split.
            let size = watchers.len().div_ceil(threads);
            std::thread::scope(|scope| {
                let world = &world;
                let parts: Vec<_> =
                    watchers.chunks_mut(size).map(|part| scope.spawn(move || world.frames(part))).collect();
                parts.into_iter().flat_map(|part| part.join().expect("a flush thread panicked")).collect()
            })
        };
        self.flushed = self.rev;
        frames
    }
}

/// What every watcher's frame is worked out from, for one flush - shared,
/// read-only, by however many threads the flush uses.
struct World<'a> {
    entities: &'a IdMap<Entity>,
    grid: &'a HashMap<(i32, i32), Vec<EntityId>>,
    cell: f64,
    radius: f64,
    changed: &'a IdMap<Change>,
    tick: u64,
}

fn push(list: &mut String, item: &str) {
    if !list.is_empty() {
        list.push(',');
    }
    list.push_str(item);
}

impl World<'_> {
    /// The frames of these watchers, in their order: one each that has
    /// something to hear.
    fn frames(&self, watchers: &mut [(&PlayerId, &mut Watcher)]) -> Vec<(PlayerId, String)> {
        // Whole entities, written the first time somebody needs one.
        let mut whole: IdMap<String> = IdMap::default();
        let mut fresh = Vec::new();
        let mut frames = Vec::new();
        for (player, w) in watchers.iter_mut() {
            if let Some(frame) = self.frame(player, w, &mut whole, &mut fresh) {
                frames.push(((*player).clone(), frame));
            }
        }
        frames
    }

    fn frame(&self, player: &str, w: &mut Watcher, whole: &mut IdMap<String>, fresh: &mut Vec<EntityId>) -> Option<String> {
        let tick = self.tick;
        if let Some(e) = w.follow.and_then(|id| self.entities.get(&id)) {
            (w.x, w.y) = (e.x, e.y);
        }
        let radius = w.radius.unwrap_or(self.radius);
        let leave_radius = radius * LEAVE_FACTOR;
        let (mut enter, mut update, mut leave) = (String::new(), String::new(), String::new());
        let mut moves = String::new();
        let (wx, wy, sees) = (w.x, w.y, w.sees);
        let sight = &w.sight;

        w.known.retain(|id| {
            let near = self.entities.get(id).is_some_and(|e| {
                e.shown_to(player, sees)
                    && (e.x - wx).hypot(e.y - wy) <= leave_radius
                    && sight.as_ref().is_none_or(|s| s.sees(e.x, e.y))
            });
            if !near {
                if !leave.is_empty() {
                    leave.push(',');
                }
                let _ = write!(leave, "{id}");
            } else {
                match self.changed.get(id) {
                    Some(Change::Move(text)) => push(&mut moves, text),
                    Some(Change::Update(text)) => push(&mut update, text),
                    None => {}
                }
            }
            near
        });

        fresh.clear();
        // The cells this watcher's own view reaches - three by three at
        // most when the cell is the leave-radius of the widest view.
        let lo = (((w.x - radius) / self.cell).floor() as i32, ((w.y - radius) / self.cell).floor() as i32);
        let hi = (((w.x + radius) / self.cell).floor() as i32, ((w.y + radius) / self.cell).floor() as i32);
        for cx in lo.0..=hi.0 {
            for cy in lo.1..=hi.1 {
                for id in self.grid.get(&(cx, cy)).into_iter().flatten() {
                    let e = &self.entities[id];
                    if e.shown_to(player, w.sees)
                        && (e.x - w.x).hypot(e.y - w.y) <= radius
                        && w.sight.as_ref().is_none_or(|s| s.sees(e.x, e.y))
                        && w.known.binary_search(id).is_err()
                    {
                        fresh.push(*id);
                    }
                }
            }
        }
        // Sorted, so a frame is the same whatever order the maps iterate.
        fresh.sort_unstable();
        for id in fresh.iter() {
            let text = whole.entry(*id).or_insert_with(|| Space::whole(*id, &self.entities[id]).to_string());
            push(&mut enter, text);
        }
        if !fresh.is_empty() {
            w.known.extend_from_slice(fresh);
            w.known.sort_unstable();
        }

        let ack = w.ack.filter(|ack| w.ack_sent != Some(*ack));
        if enter.is_empty() && update.is_empty() && leave.is_empty() && moves.is_empty() && ack.is_none() {
            return None;
        }
        let mut frame = String::with_capacity(80 + enter.len() + moves.len() + update.len() + leave.len());
        let _ = write!(frame, r#"{{"t":"sync","tick":{tick}"#);
        if let Some(ack) = ack {
            let _ = write!(frame, r#","ack":{ack}"#);
            w.ack_sent = Some(ack);
        }
        for (key, list) in [("enter", &enter), ("move", &moves), ("update", &update), ("leave", &leave)] {
            if !list.is_empty() {
                let _ = write!(frame, r#","{key}":[{list}]"#);
            }
        }
        frame.push('}');
        Some(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_for(frames: &[(PlayerId, String)], who: &str) -> Option<Value> {
        frames.iter().find(|(p, _)| p == who).map(|(_, f)| serde_json::from_str(f).unwrap())
    }

    /// V4: a crowd big enough is worked out on several threads, and the
    /// frames are exactly - byte for byte, in order - what one thread makes.
    #[test]
    fn a_crowd_flushed_on_several_threads_says_what_one_thread_says() {
        let make = |threads: usize| {
            let mut space = Space::new(400.0).flush_threads(threads);
            let mut seed = 0x2545_f491_4f6c_dd1du64;
            let mut rnd = move || {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed >> 11) as f64 / (1u64 << 53) as f64
            };
            let ids: Vec<EntityId> = (0..400)
                .map(|i| {
                    let id = space.spawn(rnd() * 900.0, rnd() * 900.0, json!({ "hp": 100 }));
                    space.watch_from(&format!("p{i:03}"), id);
                    id
                })
                .collect();
            let mut all = Vec::new();
            for tick in 0..6 {
                for (n, id) in ids.iter().enumerate() {
                    let Some((x, y)) = space.position(*id) else { continue };
                    space.set_position(*id, x + rnd() * 6.0 - 3.0, y + rnd() * 6.0 - 3.0);
                    if (n + tick) % 37 == 0 {
                        space.set(*id, "hp", 100 - tick as i64);
                    }
                }
                if tick == 3 {
                    space.despawn(ids[7]);
                }
                all.push(space.flush());
            }
            all
        };
        let one = make(1);
        assert!(one[1].len() > 300, "a crowd: {} frames", one[1].len());
        assert_eq!(make(4), one, "four threads, the same frames in the same order");
    }

    /// A ghost on its own layer is told only to a watcher who sees that
    /// layer; moved onto the common one, everybody is told it arrived.
    #[test]
    fn layers_decide_who_is_told() {
        let mut s = Space::new(100.0);
        let ghost = s.spawn_on(10.0, 0.0, json!({ "kind": "ghost" }), 2);
        let rock = s.spawn(20.0, 0.0, json!({ "kind": "rock" }));
        // A watcher sees every layer unless told otherwise: Ada is an
        // ordinary player, told the common layer; the medium sees ghosts too.
        s.watch("ada", 0.0, 0.0);
        s.watch_layers("ada", DEFAULT_LAYER);
        s.watch("medium", 0.0, 0.0);
        s.watch_layers("medium", DEFAULT_LAYER | 2);
        let frames = s.flush();
        let ids = |f: &Value| -> Vec<u64> { f["enter"].as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap()).collect() };
        assert_eq!(ids(&frame_for(&frames, "ada").unwrap()), [rock]);
        assert_eq!(ids(&frame_for(&frames, "medium").unwrap()), [ghost, rock]);
        // The ghost moves: only the medium hears of it.
        s.set_position(ghost, 11.0, 0.0);
        let frames = s.flush();
        assert!(frame_for(&frames, "ada").is_none());
        assert_eq!(frame_for(&frames, "medium").unwrap()["move"][0][0], ghost);
        // It takes form on the common layer: Ada is told it entered.
        s.set_layers(ghost, DEFAULT_LAYER);
        let frames = s.flush();
        assert_eq!(ids(&frame_for(&frames, "ada").unwrap()), [ghost]);
        assert!(frame_for(&frames, "medium").is_none(), "the medium saw it all along");
        // And back into the dark: it leaves her view, and `near` still finds it.
        s.set_layers(ghost, 2);
        let frames = s.flush();
        assert_eq!(frame_for(&frames, "ada").unwrap()["leave"], json!([ghost]));
        assert_eq!(s.near(10.0, 0.0, 5.0), [ghost]);
        assert_eq!(s.layers(ghost), Some(2));
    }

    /// A wall between: the watcher's sight says which cells they see, and
    /// an entity in range but in an unseen cell is not told - until the
    /// sight changes, and then it is told it entered; hidden again, it left.
    #[test]
    fn sight_hides_what_is_in_range_but_not_in_view() {
        // A 4 x 1 strip of 10-unit cells, top-left corner at (0, 10): cell
        // centres at x = 5, 15, 25, 35, y = 5.
        let mut s = Space::new(100.0);
        let near = s.spawn(15.0, 5.0, json!({ "kind": "rat" }));
        let behind = s.spawn(35.0, 5.0, json!({ "kind": "orc" }));
        s.watch("ada", 5.0, 5.0);
        let mut sight = Sight::new(4, 1, 10.0, (0.0, 10.0));
        sight.visible = vec![true, true, false, false];
        assert!(sight.sees(15.0, 5.0) && !sight.sees(35.0, 5.0) && !sight.sees(-1.0, 5.0));
        s.watch_sight("ada", Some(sight.clone()));
        let ids = |f: &Value| -> Vec<u64> { f["enter"].as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap()).collect() };
        assert_eq!(ids(&frame_for(&s.flush(), "ada").unwrap()), [near]);
        // A door opens: the far cells come into sight.
        sight.visible = vec![true; 4];
        s.watch_sight("ada", Some(sight.clone()));
        assert_eq!(ids(&frame_for(&s.flush(), "ada").unwrap()), [behind]);
        // The orc steps behind the wall again (the room says that cell is
        // not seen): it leaves at once, though it is well within range.
        sight.visible[3] = false;
        s.watch_sight("ada", Some(sight));
        assert_eq!(frame_for(&s.flush(), "ada").unwrap()["leave"], json!([behind]));
        // No sight at all is range alone, as before.
        s.watch_sight("ada", None);
        assert_eq!(ids(&frame_for(&s.flush(), "ada").unwrap()), [behind]);
    }

    #[test]
    fn a_moved_entity_is_told_to_everyone_who_sees_it_and_once_each() {
        let mut s = Space::new(100.0);
        let wolf = s.spawn(10.0, 0.0, json!({ "hp": 30 }));
        s.watch("ada", 0.0, 0.0);
        s.watch("bo", 20.0, 0.0);
        s.flush();
        s.set_position(wolf, 11.0, 0.0);
        s.set(wolf, "hp", 29);
        let frames = s.flush();
        let expected = json!([{ "id": wolf, "x": 11.0, "y": 0.0, "hp": 29 }]);
        assert_eq!(frame_for(&frames, "ada").unwrap()["update"], expected);
        assert_eq!(frame_for(&frames, "bo").unwrap()["update"], expected);
        assert!(s.flush().is_empty(), "and not again next tick");
    }

    #[test]
    fn a_watcher_is_told_of_what_is_near_and_nothing_else() {
        let mut s = Space::new(100.0);
        let near = s.spawn(50.0, 0.0, json!({ "kind": "wolf" }));
        let _far = s.spawn(5000.0, 0.0, json!({ "kind": "bear" }));
        s.watch("ada", 0.0, 0.0);
        let f = frame_for(&s.flush(), "ada").unwrap();
        let enter = f["enter"].as_array().unwrap();
        assert_eq!(enter.len(), 1);
        assert_eq!((enter[0]["id"].as_u64(), &enter[0]["kind"]), (Some(near), &json!("wolf")));
        assert!(s.flush().is_empty(), "nothing changed, so nothing is sent");
    }

    #[test]
    fn an_update_carries_only_what_changed() {
        let mut s = Space::new(100.0);
        let wolf = s.spawn(10.0, 0.0, json!({ "kind": "wolf", "hp": 30 }));
        s.watch("ada", 0.0, 0.0);
        s.flush();
        s.set(wolf, "hp", 25);
        s.set(wolf, "kind", "wolf"); // unchanged
        let f = frame_for(&s.flush(), "ada").unwrap();
        assert_eq!(f["update"], json!([{ "id": wolf, "hp": 25 }]));
        s.set_position(wolf, 12.0, 0.0);
        let f = frame_for(&s.flush(), "ada").unwrap();
        assert_eq!(f["move"], json!([[wolf, 12, 0]]), "a move alone is three numbers, written as short as they go");
        assert!(f.get("update").is_none());
    }

    #[test]
    fn what_walks_away_or_dies_leaves() {
        let mut s = Space::new(100.0);
        let wolf = s.spawn(10.0, 0.0, json!({}));
        let hare = s.spawn(20.0, 0.0, json!({}));
        s.watch("ada", 0.0, 0.0);
        s.flush();
        s.set_position(wolf, 900.0, 0.0);
        s.despawn(hare);
        let f = frame_for(&s.flush(), "ada").unwrap();
        assert_eq!(f["leave"], json!([wolf, hare]));
        assert!(f.get("update").is_none(), "a thing that left is not also updated");
    }

    /// Pacing along the edge of a view must not re-send the entity each time.
    #[test]
    fn the_edge_of_view_has_hysteresis() {
        let mut s = Space::new(100.0);
        let wolf = s.spawn(99.0, 0.0, json!({}));
        s.watch("ada", 0.0, 0.0);
        s.flush();
        s.set_position(wolf, 110.0, 0.0);
        let f = frame_for(&s.flush(), "ada").unwrap();
        assert!(f.get("leave").is_none() && f.get("enter").is_none());
        assert!(f["move"].is_array(), "still known, so it only moved");
        s.set_position(wolf, 130.0, 0.0);
        assert_eq!(frame_for(&s.flush(), "ada").unwrap()["leave"], json!([wolf]));
    }

    #[test]
    fn a_watcher_following_an_avatar_sees_the_world_come_and_go() {
        let mut s = Space::new(100.0);
        let ada = s.spawn(0.0, 0.0, json!({ "kind": "player" }));
        let tree = s.spawn(400.0, 0.0, json!({ "kind": "tree" }));
        s.watch_from("ada", ada);
        let f = frame_for(&s.flush(), "ada").unwrap();
        assert_eq!(f["enter"].as_array().unwrap().len(), 1, "herself");
        s.set_position(ada, 350.0, 0.0);
        let f = frame_for(&s.flush(), "ada").unwrap();
        assert_eq!(f["enter"][0]["id"], tree);
        assert_eq!(f["move"][0][0], ada);
    }

    #[test]
    fn two_watchers_each_hear_their_own_surroundings() {
        let mut s = Space::new(100.0);
        let a = s.spawn(0.0, 0.0, json!({}));
        let b = s.spawn(3000.0, 0.0, json!({}));
        s.watch_from("ada", a);
        s.watch_from("bo", b);
        let frames = s.flush();
        assert_eq!(frame_for(&frames, "ada").unwrap()["enter"][0]["id"], a);
        assert_eq!(frame_for(&frames, "bo").unwrap()["enter"][0]["id"], b);
        s.set(a, "hp", 1);
        let frames = s.flush();
        assert!(frame_for(&frames, "bo").is_none(), "bo is nowhere near it");
    }

    #[test]
    fn a_watcher_can_see_less_far_than_the_rest() {
        let mut s = Space::new(100.0);
        let close = s.spawn(20.0, 0.0, json!({}));
        let farther = s.spawn(80.0, 0.0, json!({}));
        s.watch("ada", 0.0, 0.0);
        s.watch("bo", 0.0, 0.0);
        s.watch_radius("bo", 40.0);
        let frames = s.flush();
        assert_eq!(frame_for(&frames, "ada").unwrap()["enter"].as_array().unwrap().len(), 2);
        let bo = frame_for(&frames, "bo").unwrap();
        assert_eq!(bo["enter"], json!([{ "id": close, "x": 20.0, "y": 0.0 }]), "the lantern reaches forty");
        // What was near and walks out of his light leaves his view - not hers.
        s.set_position(close, 60.0, 0.0);
        let frames = s.flush();
        assert_eq!(frame_for(&frames, "bo").unwrap()["leave"], json!([close]));
        assert!(frame_for(&frames, "ada").unwrap().get("leave").is_none());
        let _ = farther;
    }

    #[test]
    fn a_watcher_is_told_which_input_was_applied_and_when() {
        let mut s = Space::new(100.0);
        let ada = s.spawn(0.0, 0.0, json!({}));
        s.watch_from("ada", ada);
        let first = frame_for(&s.flush(), "ada").unwrap();
        assert_eq!(first["tick"], 1);
        assert!(first.get("ack").is_none(), "nothing has been applied yet");

        s.set_position(ada, 30.0, 0.0);
        s.ack("ada", 7);
        let f = frame_for(&s.flush(), "ada").unwrap();
        assert_eq!((&f["tick"], &f["ack"], &f["move"]), (&json!(2), &json!(7), &json!([[ada, 30, 0]])));

        // An input that changed nothing is still acknowledged - once.
        s.ack("ada", 8);
        let f = frame_for(&s.flush(), "ada").unwrap();
        assert_eq!((&f["ack"], f.get("move")), (&json!(8), None));
        assert!(s.flush().is_empty(), "and not again");
    }

    /// A facing rides on the move: a fourth number, sent when it turns
    /// (even standing still), on entering, and never for a thing that has
    /// no facing.
    /// S28: an entity for one watcher only - as many as there are players,
    /// with no layer bit spent - and ids that say nothing of their order.
    #[test]
    fn an_entity_can_be_for_one_watcher_and_ids_can_be_opaque() {
        let mut s = Space::new(100.0).opaque_ids(42);
        let ids: Vec<EntityId> = (0..64).map(|i| s.spawn(f64::from(i), 0.0, json!({ "kind": "body" }))).collect();
        assert!(ids.iter().all(|id| *id > 0 && *id < OPAQUE_ID_LIMIT));
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 64, "unique");
        assert_ne!(sorted, ids, "and not in the order they were made");
        // Sixty-four players, each with a body only they are told of.
        for (i, id) in ids.iter().enumerate() {
            s.watch(&format!("p{i}"), 0.0, 0.0);
            s.only_for(*id, [format!("p{i}")]);
        }
        let frames = s.flush();
        for i in [0usize, 31, 63] {
            let f = frame_for(&frames, &format!("p{i}")).unwrap();
            assert_eq!(f["enter"].as_array().unwrap().len(), 1, "p{i} sees one body");
            assert_eq!(f["enter"][0]["id"], ids[i]);
        }
        // Shown to everyone again: the others are told it came.
        s.for_everyone(ids[5]);
        let frames = s.flush();
        assert_eq!(frame_for(&frames, "p0").unwrap()["enter"][0]["id"], ids[5]);
        assert!(frame_for(&frames, "p5").is_none(), "its owner knew it already");
        // And hidden again: the others are told it left.
        s.only_for(ids[5], ["p5"]);
        assert_eq!(frame_for(&s.flush(), "p0").unwrap()["leave"], json!([ids[5]]));
    }

    /// S15: a watcher scans the cells its own view covers, so cells smaller
    /// than the widest view still let a wide watcher see as far as it
    /// should, and a watcher may see past the space's own radius.
    #[test]
    fn a_view_wider_than_a_cell_still_sees_its_whole_radius() {
        let mut s = Space::with_cell(100.0, 50.0);
        let far = s.spawn(390.0, 0.0, json!({ "kind": "whale" }));
        let near = s.spawn(40.0, 0.0, json!({ "kind": "krill" }));
        s.watch("small", 0.0, 0.0);
        s.watch("lookout", 0.0, 0.0);
        s.watch_radius("lookout", 400.0);
        let frames = s.flush();
        let ids = |who: &str| -> Vec<u64> {
            frame_for(&frames, who).unwrap()["enter"].as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap()).collect()
        };
        assert_eq!(ids("small"), [near]);
        assert_eq!(ids("lookout"), [far, near], "eight cells out, and past the space's radius");
    }

    #[test]
    fn a_rotation_rides_on_the_move() {
        let mut s = Space::new(100.0);
        let ship = s.spawn(0.0, 0.0, json!({ "kind": "ship" }));
        let rock = s.spawn(5.0, 0.0, json!({ "kind": "rock" }));
        s.set_rotation(ship, 1.5);
        s.watch("ada", 0.0, 0.0);
        let f = frame_for(&s.flush(), "ada").unwrap();
        let entered = f["enter"].as_array().unwrap();
        assert_eq!(entered[0]["rot"], 1.5);
        assert!(entered[1].get("rot").is_none(), "a rock has no facing");
        s.set_rotation(ship, 1.75);
        assert_eq!(frame_for(&s.flush(), "ada").unwrap()["move"], json!([[ship, 0, 0, 1.75]]));
        s.set_rotation(ship, 1.7501);
        assert!(frame_for(&s.flush(), "ada").is_none(), "a turn finer than is sent sends nothing");
        s.set_position(rock, 6.0, 0.0);
        assert_eq!(frame_for(&s.flush(), "ada").unwrap()["move"], json!([[rock, 6, 0]]));
        s.set_position(ship, 1.0, 0.0);
        s.set(ship, "hp", 3);
        assert_eq!(frame_for(&s.flush(), "ada").unwrap()["update"][0]["rot"], 1.75);
        assert_eq!(s.rotation(ship), Some(1.75));
    }

    #[test]
    fn a_space_that_remembers_knows_where_things_were() {
        let mut s = Space::new(100.0);
        let wolf = s.spawn(0.0, 0.0, json!({}));
        assert_eq!(s.position_at(wolf, 1), None, "nothing is kept unless asked");
        s.remember(8);
        for step in 1..=5 {
            s.set_position(wolf, f64::from(step) * 10.0, 0.0);
            s.flush();
        }
        // A shot "seen" at tick 3 is judged against where the wolf was then.
        assert_eq!(s.position_at(wolf, 3), Some((30.0, 0.0)));
        assert_eq!(s.position(wolf), Some((50.0, 0.0)));
        for step in 6..=20 {
            s.set_position(wolf, f64::from(step) * 10.0, 0.0);
            s.flush();
        }
        assert_eq!(s.position_at(wolf, 3), None, "older than is kept is said, not answered with the oldest");
    }

    #[test]
    fn near_finds_by_distance_across_cells() {
        let mut s = Space::new(100.0);
        let a = s.spawn(120.0, 0.0, json!({}));
        let b = s.spawn(130.0, 0.0, json!({}));
        s.spawn(400.0, 0.0, json!({}));
        assert_eq!(s.near(125.0, 0.0, 10.0), vec![a, b]);
    }
}
