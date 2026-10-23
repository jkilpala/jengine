//! A room, the player in it, and the rules for moving between them - no
//! sockets, so every rule here is a unit test.
//!
//! # A player is not a connection, and not a room's
//!
//! A [`Player`] is an id, a name and a **state document** that the server
//! loads when the player arrives and saves when they go. It is handed *to* a
//! room for as long as the player is in it, and taken back when they leave -
//! so walking from one zone into another, or from a lobby into a match, moves
//! the same document along. That is the thing a room-per-match framework
//! leaves to the game, and the reason this one exists.
//!
//! # A room records, it does not send
//!
//! A [`Room`]'s callbacks are plain synchronous functions. What they want
//! done - a frame to somebody, a player moved elsewhere, the room closed -
//! they say to the [`RoomCtx`], and the [`RoomRunner`] carries it out after
//! the callback returns. So a room never holds a socket, never awaits, and is
//! driven in a test exactly as it is by the server.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Map, Value, json};

use crate::store::Store;

pub type PlayerId = String;

/// A frame on its way to a player. Shared, so a broadcast to a room of five
/// hundred is one value - and one serialisation - not five hundred.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Json(Arc<Value>),
    /// Already serialised - see [`RoomCtx::send_text`].
    Text(Arc<str>),
}

impl Frame {
    /// The frame as JSON, parsing it if it was sent as text.
    pub fn to_json(&self) -> Value {
        match self {
            Frame::Json(value) => (**value).clone(),
            Frame::Text(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        }
    }
}

/// `frame["t"]`, for a test to read what a room said. A text frame has to be
/// parsed first, with [`Frame::to_json`].
impl std::ops::Index<&str> for Frame {
    type Output = Value;

    fn index(&self, key: &str) -> &Value {
        match self {
            Frame::Json(value) => &value[key],
            Frame::Text(_) => panic!("a text frame is not indexable; call to_json() first"),
        }
    }
}

/// Somebody playing. See the module doc: this travels from room to room.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    /// What is remembered about this player - always a JSON object. A room
    /// reads and writes it freely; the server saves it when the player leaves
    /// a room or disconnects, and every so often in between.
    pub state: Value,
}

impl Player {
    pub fn new(id: &str, name: &str) -> Self {
        Player { id: id.to_string(), name: name.to_string(), state: Value::Object(Map::new()) }
    }

    /// A number in the state, or `default`.
    pub fn num(&self, key: &str, default: f64) -> f64 {
        self.state.get(key).and_then(Value::as_f64).unwrap_or(default)
    }

    /// A string in the state, or `default`.
    pub fn text<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.state.get(key).and_then(Value::as_str).unwrap_or(default)
    }

    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        if !self.state.is_object() {
            self.state = Value::Object(Map::new());
        }
        self.state[key] = value.into();
    }
}

/// Which room somebody wants.
#[derive(Debug, Clone, PartialEq)]
pub struct JoinRequest {
    pub kind: String,
    /// A named room of that kind, made if it does not exist. `None` asks the
    /// server to pick one with space, or open another.
    pub room: Option<String>,
    pub options: Value,
}

impl JoinRequest {
    pub fn named(kind: &str, room: &str) -> Self {
        JoinRequest { kind: kind.to_string(), room: Some(room.to_string()), options: Value::Null }
    }

    pub fn any(kind: &str) -> Self {
        JoinRequest { kind: kind.to_string(), room: None, options: Value::Null }
    }

    pub fn with(mut self, options: Value) -> Self {
        self.options = options;
        self
    }
}

enum Out {
    To(PlayerId, Frame),
    All(Value),
    AllExcept(PlayerId, Value),
}

/// What a room callback is handed to say what it wants done.
pub struct RoomCtx {
    kind: String,
    id: String,
    store: Arc<dyn Store>,
    time: f64,
    player_count: usize,
    out: Vec<Out>,
    transfers: Vec<(PlayerId, JoinRequest)>,
    kicks: Vec<(PlayerId, String)>,
    close: bool,
    /// (channel, the `said` frame) a room asked to have said.
    said: Vec<(String, Value)>,
    /// How long ago the message being handled was sent, by the server's
    /// measure; zero outside `on_message`.
    delay: f64,
}

/// The furthest back a message is believed to have been sent: a connection
/// slower than this is late, and is judged as if it were not.
pub const MAX_DELAY: f64 = 1.0;

impl RoomCtx {
    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Seconds this room has been ticked for. Zero in a room with no tick.
    pub fn time(&self) -> f64 {
        self.time
    }

    pub fn player_count(&self) -> usize {
        self.player_count
    }

    /// In `on_message`: how long ago, in seconds, the player sent it - half
    /// their connection's round trip, which the server measures by pinging
    /// clients that answer (`jengine-online` does), at most [`MAX_DELAY`].
    /// Zero for a client that does not, and outside `on_message`. Measured,
    /// not claimed: a client cannot buy itself time.
    pub fn delay(&self) -> f64 {
        self.delay
    }

    /// In `on_message`: when the player sent it, on this room's clock
    /// ([`RoomCtx::time`]) - what a timing rule should judge ("released when
    /// the timer showed a 4"), not when it arrived.
    pub fn sent_at(&self) -> f64 {
        self.time - self.delay
    }

    /// The tick a claim about the past may be judged at - a client saying
    /// "I hit them at tick n" - held to what the player could have been
    /// looking at: no older than their **measured** round trip (twice
    /// [`RoomCtx::delay`]) plus the `slack` ticks a client draws others
    /// behind by (its interpolation), and no newer than now. `tick_secs` is
    /// the length of one of the claim's ticks. A claim from further back is
    /// judged at the oldest tick allowed, so a cheat lying about the tick
    /// buys no more rewind than their own lag already gives them.
    pub fn clamp_rewind(&self, claimed: u64, now_tick: u64, tick_secs: f64, slack: u64) -> u64 {
        let behind = if tick_secs > 0.0 { (2.0 * self.delay / tick_secs).ceil() as u64 } else { 0 };
        claimed.clamp(now_tick.saturating_sub(behind + slack + 1), now_tick)
    }

    /// The server's store, for what the *room* wants to keep - a world's
    /// chests, a leaderboard. A player's own state needs none of this.
    pub fn store(&self) -> &dyn Store {
        &*self.store
    }

    pub fn send(&mut self, to: &str, frame: Value) {
        self.out.push(Out::To(to.to_string(), Frame::Json(Arc::new(frame))));
    }

    /// Send a frame that is already JSON text, as it is. For a frame built
    /// from shared pieces - [`crate::sync::Space::flush`] makes them - where
    /// going through a [`Value`] would serialise the same thing per player.
    pub fn send_text(&mut self, to: &str, frame: impl Into<Arc<str>>) {
        self.out.push(Out::To(to.to_string(), Frame::Text(frame.into())));
    }

    /// To everyone in the room. In `on_join` the joiner is not in it yet, and
    /// in `on_leave` the leaver no longer is.
    pub fn broadcast(&mut self, frame: Value) {
        self.out.push(Out::All(frame));
    }

    pub fn broadcast_except(&mut self, except: &str, frame: Value) {
        self.out.push(Out::AllExcept(except.to_string(), frame));
    }

    /// Say something in a channel, as the room: everyone in it hears
    /// `{"t":"said","channel",..,"from":"","room":{"kind","id"},"body"}` -
    /// a boss that roars across the zone's chat, a match that announces its
    /// result to the lobby. The server must have channels on; said into one
    /// nobody is in, it still goes into its backlog for whoever joins next.
    pub fn say(&mut self, channel: &str, body: Value) {
        let frame = json!({
            "t": "said", "channel": channel, "from": "", "name": format!("{}/{}", self.kind, self.id),
            "room": { "kind": self.kind, "id": self.id }, "body": body,
        });
        self.said.push((channel.to_string(), frame));
    }

    /// Move a player to another room once this callback returns: `on_leave`
    /// runs here, their state is saved, and they join `to` on the same
    /// connection - the client sees a new `joined` frame and nothing else.
    pub fn transfer(&mut self, player: &str, to: JoinRequest) {
        self.transfers.push((player.to_string(), to));
    }

    /// Put a player out of the room, telling them why.
    pub fn kick(&mut self, player: &str, why: &str) {
        self.kicks.push((player.to_string(), why.to_string()));
    }

    /// Close the room once this callback returns. Everyone in it is released
    /// with their state, as if each had left.
    pub fn close(&mut self) {
        self.close = true;
    }
}

/// The players in a room, as `on_tick` sees them.
pub struct Players<'a>(&'a mut BTreeMap<PlayerId, Player>);

impl Players<'_> {
    pub fn get(&self, id: &str) -> Option<&Player> {
        self.0.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Player> {
        self.0.get_mut(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Player> {
        self.0.values()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Player> {
        self.0.values_mut()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A game's rules for one kind of room. Every method has a default but
/// `on_join`, so a relay is twenty lines and a zone is what it needs to be.
pub trait Room: Any + Send + 'static {
    fn on_create(&mut self, _ctx: &mut RoomCtx) {}

    /// Somebody wants in. `Err(why)` turns them away; `Ok(extra)` lets them
    /// in, and `extra`'s fields are added to the `joined` frame they get -
    /// their seat, the map, the id of their avatar.
    fn on_join(&mut self, ctx: &mut RoomCtx, player: &mut Player, options: &Value)
    -> Result<Value, String>;

    /// The `body` of a `send` frame from a player in the room.
    fn on_message(&mut self, _ctx: &mut RoomCtx, _player: &mut Player, _body: Value) {}

    /// The player is going - by asking, by disconnecting, by being moved or
    /// put out. The last chance to write to their state.
    fn on_leave(&mut self, _ctx: &mut RoomCtx, _player: &mut Player) {}

    /// The player's connection dropped and the kind has a grace period: they
    /// are still in the room, with nobody at the other end. Frames sent to
    /// them go nowhere. A room might freeze their avatar, or skip their turn.
    fn on_disconnect(&mut self, _ctx: &mut RoomCtx, _player: &mut Player) {}

    /// They are back within the grace period, on a new connection that knows
    /// nothing. What this returns rides on the `joined` frame they are sent,
    /// as `on_join`'s does - and a room that syncs by difference must start
    /// that player's view again from nothing.
    fn on_rejoin(&mut self, _ctx: &mut RoomCtx, _player: &mut Player) -> Value {
        Value::Null
    }

    /// An operator replaced this player's state while they are in the room.
    /// A room that keeps a copy of something in it - a position, a health
    /// bar it syncs - takes the new value from `player.state` here.
    fn on_state_edited(&mut self, _ctx: &mut RoomCtx, _player: &mut Player) {}

    /// Called at the kind's tick rate, with the seconds since the last one.
    fn on_tick(&mut self, _ctx: &mut RoomCtx, _players: &mut Players, _dt: f64) {}

    /// The room is closing, with nobody in it.
    fn on_close(&mut self, _ctx: &mut RoomCtx) {}

    /// What an operator is shown for this room, beside its players - how many
    /// monsters are up, whose turn it is. Read on demand, never per tick.
    fn describe(&self) -> Value {
        Value::Null
    }

    /// A command from an operator, through the admin API - spawn the boss,
    /// end the round, open the gate. What it returns is the operator's answer.
    /// The commands are the game's own; a room that has none refuses.
    fn on_admin(&mut self, _ctx: &mut RoomCtx, _players: &mut Players, _command: Value) -> Value {
        json!({ "error": "this kind of room takes no commands" })
    }
}

/// Why a player came out of a room.
#[derive(Debug, Clone, PartialEq)]
pub enum Release {
    Left,
    Kicked(String),
    Transfer(JoinRequest),
    /// The room closed around them.
    Closed,
}

/// What one step of a room produced, for the caller to carry out.
#[derive(Debug, Default)]
pub struct Effects {
    /// Frames, in order, each to one player.
    pub frames: Vec<(PlayerId, Frame)>,
    /// Players the room no longer holds, with their state as the room left it.
    pub released: Vec<(Player, Release)>,
    /// The room asked to close; it has already released everyone.
    pub closed: bool,
    /// What the room said in channels: (channel, the `said` frame).
    pub said: Vec<(String, Value)>,
}

/// One room, run: the [`Room`], its players, and the bookkeeping between.
pub struct RoomRunner {
    room: Box<dyn Room>,
    ctx: RoomCtx,
    players: BTreeMap<PlayerId, Player>,
    max_players: usize,
}

impl RoomRunner {
    /// Make the room and run its `on_create`.
    pub fn new(
        kind: &str,
        id: &str,
        room: Box<dyn Room>,
        store: Arc<dyn Store>,
        max_players: usize,
    ) -> (Self, Effects) {
        let mut runner = RoomRunner {
            room,
            ctx: RoomCtx {
                kind: kind.to_string(),
                id: id.to_string(),
                store,
                time: 0.0,
                player_count: 0,
                out: Vec::new(),
                transfers: Vec::new(),
                kicks: Vec::new(),
                close: false,
                said: Vec::new(),
                delay: 0.0,
            },
            players: BTreeMap::new(),
            max_players,
        };
        runner.room.on_create(&mut runner.ctx);
        let effects = runner.settle(Effects::default());
        (runner, effects)
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    /// The room itself, as its own type - for a unit test that reads its
    /// rules' state, or sets a position up before the messages it means to
    /// test. `None` if the room is another type.
    pub fn room<T: Room>(&self) -> Option<&T> {
        (&*self.room as &dyn Any).downcast_ref::<T>()
    }

    /// The room itself, to change - a test placing a tomato on a counter
    /// before two cooks reach for it. Effects of a change made this way are
    /// not collected; send what should be said through the room's own code.
    pub fn room_mut<T: Room>(&mut self) -> Option<&mut T> {
        (&mut *self.room as &mut dyn Any).downcast_mut::<T>()
    }

    pub fn players(&self) -> impl Iterator<Item = &Player> {
        self.players.values()
    }

    /// Let a player in, or hand them back with the reason they were not.
    pub fn join(&mut self, mut player: Player, options: &Value) -> Result<Effects, (Player, String)> {
        if self.players.contains_key(&player.id) {
            return Err((player, "already in this room".to_string()));
        }
        if self.players.len() >= self.max_players {
            return Err((player, "the room is full".to_string()));
        }
        match self.room.on_join(&mut self.ctx, &mut player, options) {
            Err(why) => {
                // A refusal takes back anything said while deciding.
                self.ctx.out.clear();
                self.ctx.transfers.clear();
                self.ctx.kicks.clear();
                Err((player, why))
            }
            Ok(extra) => {
                let mut joined = json!({ "t": "joined", "kind": self.ctx.kind, "room": self.ctx.id });
                if let (Some(frame), Value::Object(extra)) = (joined.as_object_mut(), extra) {
                    for (k, v) in extra {
                        frame.entry(k).or_insert(v);
                    }
                }
                let id = player.id.clone();
                // Routed on arrival: `on_join` sent them on (a new soul to the
                // room it starts in, a returning one to where it was). Then
                // this room is one they pass through and never see - no
                // `joined` from it, and nothing it said to them - or a client
                // draws the wrong place for a frame and every routing world
                // writes the same guard against it.
                let passing = self.ctx.transfers.iter().any(|(p, _)| *p == id);
                // Broadcasts made in `on_join` reach who was there before;
                // the joiner hears `joined` first, then anything said to them.
                let mut effects = Effects::default();
                let mut to_joiner = Vec::new();
                for out in std::mem::take(&mut self.ctx.out) {
                    match out {
                        Out::To(to, frame) if to == id => to_joiner.push(frame),
                        other => self.resolve(other, &mut effects.frames),
                    }
                }
                if !passing {
                    effects.frames.push((id.clone(), Frame::Json(Arc::new(joined))));
                    effects.frames.extend(to_joiner.into_iter().map(|f| (id.clone(), f)));
                }
                self.players.insert(id.clone(), player);
                let mut effects = self.settle(effects);
                if passing {
                    effects.frames.retain(|(to, _)| *to != id);
                }
                Ok(effects)
            }
        }
    }

    /// A `send` from a player. Nothing happens if they are not in the room.
    pub fn message(&mut self, id: &str, body: Value) -> Effects {
        self.message_after(id, body, 0.0)
    }

    /// A `send` the player made `delay` seconds ago ([`RoomCtx::delay`]).
    pub fn message_after(&mut self, id: &str, body: Value, delay: f64) -> Effects {
        let Some(player) = self.players.get_mut(id) else {
            return Effects::default();
        };
        self.ctx.delay = delay.clamp(0.0, MAX_DELAY);
        self.room.on_message(&mut self.ctx, player, body);
        self.ctx.delay = 0.0;
        self.settle(Effects::default())
    }

    /// The player goes, by their own choice or their connection's.
    pub fn leave(&mut self, id: &str) -> Effects {
        let mut effects = Effects::default();
        self.release(id, Release::Left, &mut effects);
        self.settle(effects)
    }

    pub fn tick(&mut self, dt: f64) -> Effects {
        self.ctx.time += dt;
        self.room.on_tick(&mut self.ctx, &mut Players(&mut self.players), dt);
        self.settle(Effects::default())
    }

    /// The player's connection has gone and their seat is being held.
    pub fn disconnected(&mut self, id: &str) -> Effects {
        if let Some(player) = self.players.get_mut(id) {
            self.room.on_disconnect(&mut self.ctx, player);
        }
        self.settle(Effects::default())
    }

    /// The player is back on a new connection: they are sent `joined` again,
    /// marked `resumed`, and the room is told.
    pub fn rejoin(&mut self, id: &str) -> Effects {
        let Some(player) = self.players.get_mut(id) else {
            return Effects::default();
        };
        let extra = self.room.on_rejoin(&mut self.ctx, player);
        let mut joined =
            json!({ "t": "joined", "kind": self.ctx.kind, "room": self.ctx.id, "resumed": true });
        if let (Some(frame), Value::Object(extra)) = (joined.as_object_mut(), extra) {
            for (k, v) in extra {
                frame.entry(k).or_insert(v);
            }
        }
        // `welcome` carried what was last saved; the room has what is true.
        let state = json!({ "t": "state", "sys": true, "state": player.state });
        let mut effects = Effects::default();
        effects.frames.push((id.to_string(), Frame::Json(Arc::new(joined))));
        effects.frames.push((id.to_string(), Frame::Json(Arc::new(state))));
        self.settle(effects)
    }

    /// Replace a player's state from outside the room's own code. `None` if
    /// they are not here. They are told with a `state` frame.
    pub fn edit_state(&mut self, id: &str, state: Value) -> Option<Effects> {
        self.edit_state_with(id, |old| *old = state)
    }

    /// Change a player's state in place, between two of the room's own
    /// calls - so nothing the room does can fall between the read and the
    /// write, which replacing the whole state from outside cannot promise.
    pub fn edit_state_with(&mut self, id: &str, edit: impl FnOnce(&mut Value)) -> Option<Effects> {
        let player = self.players.get_mut(id)?;
        edit(&mut player.state);
        self.room.on_state_edited(&mut self.ctx, player);
        let frame = json!({ "t": "state", "sys": true, "state": player.state });
        let mut effects = Effects::default();
        effects.frames.push((id.to_string(), Frame::Json(Arc::new(frame))));
        Some(self.settle(effects))
    }

    /// Everything about the room an operator can look at.
    pub fn inspect(&self) -> Value {
        let players: Vec<Value> = self
            .players
            .values()
            .map(|p| json!({ "id": p.id, "name": p.name, "state": p.state }))
            .collect();
        json!({
            "kind": self.ctx.kind, "id": self.ctx.id, "time": self.ctx.time,
            "players": players, "info": self.room.describe(),
        })
    }

    /// An operator's command, and the room's answer to it.
    pub fn admin(&mut self, command: Value) -> (Value, Effects) {
        let answer = self.room.on_admin(&mut self.ctx, &mut Players(&mut self.players), command);
        (answer, self.settle(Effects::default()))
    }

    /// Put a player out from outside the room's own code.
    pub fn kick(&mut self, id: &str, why: &str) -> Effects {
        self.ctx.kick(id, why);
        self.settle(Effects::default())
    }

    /// Say something to everyone in the room that the room did not say.
    pub fn notice(&mut self, frame: Value) -> Effects {
        self.ctx.broadcast(frame);
        self.settle(Effects::default())
    }

    /// Close the room: everyone is released, then `on_close` runs.
    pub fn close(&mut self) -> Effects {
        self.ctx.close = true;
        self.settle(Effects::default())
    }

    fn release(&mut self, id: &str, why: Release, effects: &mut Effects) {
        let Some(mut player) = self.players.remove(id) else { return };
        self.ctx.player_count = self.players.len();
        self.room.on_leave(&mut self.ctx, &mut player);
        // Frames first, so a goodbye addressed to the leaver is ahead of
        // their release in the caller's hands.
        for out in std::mem::take(&mut self.ctx.out) {
            self.resolve(out, &mut effects.frames);
        }
        effects.released.push((player, why));
    }

    fn resolve(&self, out: Out, frames: &mut Vec<(PlayerId, Frame)>) {
        match out {
            Out::To(to, frame) => frames.push((to, frame)),
            Out::All(frame) => {
                let frame = Frame::Json(Arc::new(frame));
                frames.extend(self.players.keys().map(|id| (id.clone(), frame.clone())));
            }
            Out::AllExcept(except, frame) => {
                let frame = Frame::Json(Arc::new(frame));
                frames.extend(
                    self.players
                        .keys()
                        .filter(|id| **id != except)
                        .map(|id| (id.clone(), frame.clone())),
                );
            }
        }
    }

    /// Take every player out without asking the room anything - for a room
    /// whose own code has failed, so its players keep what they had.
    pub fn abandon(&mut self) -> Vec<Player> {
        std::mem::take(&mut self.players).into_values().collect()
    }

    /// Carry out what the last callback asked for - and what the callbacks
    /// that causes ask for, until nothing is pending.
    fn settle(&mut self, mut effects: Effects) -> Effects {
        loop {
            self.ctx.player_count = self.players.len();
            for out in std::mem::take(&mut self.ctx.out) {
                self.resolve(out, &mut effects.frames);
            }
            effects.said.append(&mut self.ctx.said);
            let kicks = std::mem::take(&mut self.ctx.kicks);
            let transfers = std::mem::take(&mut self.ctx.transfers);
            if kicks.is_empty() && transfers.is_empty() {
                break;
            }
            for (id, why) in kicks {
                self.release(&id, Release::Kicked(why), &mut effects);
            }
            for (id, to) in transfers {
                self.release(&id, Release::Transfer(to), &mut effects);
            }
        }
        if self.ctx.close && !effects.closed {
            for id in self.players.keys().cloned().collect::<Vec<_>>() {
                self.release(&id, Release::Closed, &mut effects);
            }
            self.room.on_close(&mut self.ctx);
            // Nobody is left to hear, and nothing can be asked of a closed room.
            self.ctx.out.clear();
            self.ctx.kicks.clear();
            self.ctx.transfers.clear();
            effects.closed = true;
        }
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;

    /// A room that counts visits in the player's state and obeys a few words.
    struct Hall;

    impl Room for Hall {
        fn on_join(&mut self, ctx: &mut RoomCtx, player: &mut Player, options: &Value)
        -> Result<Value, String> {
            if options["password"] == "wrong" {
                ctx.broadcast(json!({ "t": "never" }));
                return Err("wrong password".into());
            }
            let visits = player.num("visits", 0.0) + 1.0;
            player.set("visits", visits);
            ctx.broadcast(json!({ "t": "arrived", "name": player.name }));
            ctx.send(&player.id, json!({ "t": "hint" }));
            Ok(json!({ "visits": visits }))
        }

        fn on_message(&mut self, ctx: &mut RoomCtx, player: &mut Player, body: Value) {
            match body.as_str() {
                Some("door") => ctx.transfer(&player.id, JoinRequest::named("hall", "next")),
                Some("rude") => ctx.kick(&player.id, "manners"),
                Some("fire") => ctx.close(),
                _ => ctx.broadcast(json!({ "t": "said", "by": player.id, "body": body })),
            }
        }

        fn on_leave(&mut self, ctx: &mut RoomCtx, player: &mut Player) {
            player.set("left_at", ctx.time());
            ctx.broadcast(json!({ "t": "gone", "id": player.id }));
        }

        fn on_tick(&mut self, _ctx: &mut RoomCtx, players: &mut Players, _dt: f64) {
            for p in players.iter_mut() {
                let ticks = p.num("ticks", 0.0) + 1.0;
                p.set("ticks", ticks);
            }
        }
    }

    fn hall(max: usize) -> RoomRunner {
        RoomRunner::new("hall", "main", Box::new(Hall), Arc::new(MemoryStore::new()), max).0
    }

    fn kinds(effects: &Effects, to: &str) -> Vec<String> {
        effects
            .frames
            .iter()
            .filter(|(id, _)| id == to)
            .map(|(_, f)| f["t"].as_str().unwrap_or("").to_string())
            .collect()
    }

    #[test]
    fn a_joiner_hears_joined_first_and_the_others_hear_the_room() {
        let mut r = hall(8);
        r.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        let e = r.join(Player::new("b", "Bo"), &Value::Null).unwrap();
        assert_eq!(kinds(&e, "b"), ["joined", "hint"]);
        assert_eq!(kinds(&e, "a"), ["arrived"], "and not the joiner's own arrival twice");
        let joined = &e.frames.iter().find(|(id, f)| id == "b" && f["t"] == "joined").unwrap().1;
        assert_eq!(joined["room"], "main");
        assert_eq!(joined["visits"], 1.0, "what on_join returned rides on the frame");
    }

    /// S26: a room that sends a player on from `on_join` is passed through
    /// unseen - no `joined` from it, nothing it said to them - while the
    /// others still hear what it said to them, and the player is released
    /// to where they were sent.
    #[test]
    fn a_player_sent_on_from_on_join_never_hears_the_room_they_passed() {
        struct Gate;
        impl Room for Gate {
            fn on_join(&mut self, ctx: &mut RoomCtx, player: &mut Player, _: &Value) -> Result<Value, String> {
                if player.id == "stay" {
                    return Ok(Value::Null);
                }
                ctx.send(&player.id, json!({ "t": "welcome_to_the_gate" }));
                ctx.broadcast(json!({ "t": "somebody_passed" }));
                ctx.transfer(&player.id, JoinRequest::named("zone", "white_room"));
                Ok(json!({ "you": 4 }))
            }
            fn on_leave(&mut self, ctx: &mut RoomCtx, player: &mut Player) {
                ctx.send(&player.id, json!({ "t": "goodbye" }));
            }
        }
        let mut r = RoomRunner::new("gate", "main", Box::new(Gate), Arc::new(MemoryStore::new()), 8).0;
        r.join(Player::new("stay", "Ada"), &Value::Null).unwrap();
        let e = r.join(Player::new("soul", "Bo"), &Value::Null).unwrap();
        assert!(kinds(&e, "soul").is_empty(), "nothing from a room never joined: {:?}", kinds(&e, "soul"));
        assert_eq!(kinds(&e, "stay"), ["somebody_passed"]);
        let [(player, Release::Transfer(to))] = &e.released[..] else { panic!("sent on: {:?}", e.released.len()) };
        assert_eq!((player.id.as_str(), to.kind.as_str()), ("soul", "zone"));
        assert_eq!(r.player_count(), 1);
    }

    /// S19: a message knows how long ago it was sent, capped, and the room
    /// judges by when.
    #[test]
    fn a_message_knows_when_it_was_sent() {
        struct Clock(Vec<(f64, f64)>);
        impl Room for Clock {
            fn on_join(&mut self, _: &mut RoomCtx, _: &mut Player, _: &Value) -> Result<Value, String> {
                Ok(Value::Null)
            }
            fn on_message(&mut self, ctx: &mut RoomCtx, _: &mut Player, _: Value) {
                self.0.push((ctx.delay(), ctx.sent_at()));
            }
        }
        let mut r = RoomRunner::new("c", "1", Box::new(Clock(Vec::new())), Arc::new(MemoryStore::new()), 4).0;
        r.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        r.ctx.time = 10.0;
        r.message_after("a", json!(1), 0.08);
        r.message_after("a", json!(2), 9.0);
        r.message("a", json!(3));
        let seen = &r.room::<Clock>().unwrap().0;
        assert_eq!(seen[0], (0.08, 10.0 - 0.08));
        assert_eq!(seen[1], (MAX_DELAY, 10.0 - MAX_DELAY), "a slow line buys no more than a second");
        assert_eq!(seen[2], (0.0, 10.0));
        assert_eq!(r.ctx.delay, 0.0, "and none outside on_message");
    }

    /// S6: a claimed tick is held to the player's measured lag: one a long
    /// way back is judged at the oldest tick their round trip allows.
    #[test]
    fn a_claimed_tick_is_held_to_the_measured_lag() {
        struct Judge(Vec<u64>);
        impl Room for Judge {
            fn on_join(&mut self, _: &mut RoomCtx, _: &mut Player, _: &Value) -> Result<Value, String> {
                Ok(Value::Null)
            }
            fn on_message(&mut self, ctx: &mut RoomCtx, _: &mut Player, body: Value) {
                let claimed = body.as_u64().unwrap_or(u64::MAX);
                self.0.push(ctx.clamp_rewind(claimed, 100, 0.1, 2));
            }
        }
        let mut r = RoomRunner::new("j", "1", Box::new(Judge(Vec::new())), Arc::new(MemoryStore::new()), 4).0;
        r.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        r.message_after("a", json!(95), 0.15); // 300 ms round trip: three ticks, plus two, plus one
        r.message_after("a", json!(10), 0.15); // a claim from ten seconds ago
        r.message("a", json!(10)); // no measured lag at all
        r.message("a", json!(200)); // the future
        r.message_after("a", json!(0), 9.0); // a slow line: a second each way at most
        assert_eq!(r.room::<Judge>().unwrap().0, [95, 94, 97, 100, 77]);
    }

    #[test]
    fn a_refused_player_is_handed_back_and_nothing_is_said() {
        let mut r = hall(8);
        r.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        let (p, why) = r.join(Player::new("b", "Bo"), &json!({ "password": "wrong" })).unwrap_err();
        assert_eq!((p.id.as_str(), why.as_str()), ("b", "wrong password"));
        assert!(r.message("a", json!("hi")).frames.iter().all(|(_, f)| f["t"] != "never"));
        assert_eq!(r.player_count(), 1);
    }

    /// A test reaches the room as its own type - to read it, or to set a
    /// position up - and a room of another type is `None`.
    #[test]
    fn a_test_reaches_the_room_by_its_type() {
        struct Counter(u32);
        impl Room for Counter {
            fn on_join(&mut self, _: &mut RoomCtx, _: &mut Player, _: &Value) -> Result<Value, String> {
                self.0 += 1;
                Ok(Value::Null)
            }
        }
        let mut r = RoomRunner::new("c", "1", Box::new(Counter(0)), Arc::new(MemoryStore::new()), 4).0;
        r.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        assert_eq!(r.room::<Counter>().map(|c| c.0), Some(1));
        r.room_mut::<Counter>().unwrap().0 = 10;
        r.join(Player::new("b", "Bo"), &Value::Null).unwrap();
        assert_eq!(r.room::<Counter>().map(|c| c.0), Some(11));
        assert!(r.room::<Hall>().is_none());
    }

    #[test]
    fn a_full_room_turns_the_next_one_away() {
        let mut r = hall(1);
        r.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        let (_, why) = r.join(Player::new("b", "Bo"), &Value::Null).unwrap_err();
        assert_eq!(why, "the room is full");
    }

    /// The point of the framework: what a room wrote leaves with the player.
    #[test]
    fn state_written_in_a_room_leaves_with_the_player() {
        let mut r = hall(8);
        r.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        r.tick(0.5);
        r.tick(0.5);
        let e = r.leave("a");
        let (player, why) = &e.released[0];
        assert_eq!(*why, Release::Left);
        assert_eq!(player.num("ticks", 0.0), 2.0);
        assert_eq!(player.num("left_at", 0.0), 1.0);
        assert_eq!(player.num("visits", 0.0), 1.0);

        // And arrives in the next room.
        let mut next = hall(8);
        let e = next.join(player.clone(), &Value::Null).unwrap();
        assert_eq!(e.frames[0].1["visits"], 2.0);
    }

    #[test]
    fn a_transfer_runs_on_leave_and_names_where_to() {
        let mut r = hall(8);
        r.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        r.join(Player::new("b", "Bo"), &Value::Null).unwrap();
        let e = r.message("a", json!("door"));
        assert_eq!(e.released.len(), 1);
        assert_eq!(e.released[0].1, Release::Transfer(JoinRequest::named("hall", "next")));
        assert_eq!(kinds(&e, "b"), ["gone"], "the others hear the leaving");
        assert_eq!(r.player_count(), 1);
    }

    #[test]
    fn a_kick_says_why() {
        let mut r = hall(8);
        r.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        let e = r.message("a", json!("rude"));
        assert_eq!(e.released[0].1, Release::Kicked("manners".into()));
    }

    #[test]
    fn a_closing_room_releases_everyone_with_their_state() {
        let mut r = hall(8);
        r.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        r.join(Player::new("b", "Bo"), &Value::Null).unwrap();
        r.tick(1.0);
        let e = r.message("a", json!("fire"));
        assert!(e.closed);
        assert_eq!(e.released.len(), 2);
        assert!(e.released.iter().all(|(p, why)| *why == Release::Closed && p.num("ticks", 0.0) == 1.0));
    }

    #[test]
    fn an_operator_can_look_in_kick_and_speak() {
        let mut r = hall(8);
        r.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        r.join(Player::new("b", "Bo"), &Value::Null).unwrap();
        let seen = r.inspect();
        assert_eq!(seen["players"].as_array().unwrap().len(), 2);
        assert_eq!(seen["players"][0]["state"]["visits"], 1.0);

        let e = r.notice(json!({ "t": "notice", "text": "restart at noon" }));
        assert_eq!((kinds(&e, "a"), kinds(&e, "b")), (vec!["notice".to_string()], vec!["notice".to_string()]));

        let e = r.kick("a", "an operator said so");
        assert_eq!(e.released[0].1, Release::Kicked("an operator said so".into()));
        assert_eq!(kinds(&e, "b"), ["gone"], "on_leave ran, as for any leaving");

        let (answer, _) = r.admin(json!("anything"));
        assert!(answer["error"].is_string(), "a room with no commands says so");
    }

    #[test]
    fn a_held_seat_is_still_a_seat_and_coming_back_is_told_as_joined() {
        let mut r = hall(2);
        r.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        r.join(Player::new("b", "Bo"), &Value::Null).unwrap();
        let e = r.disconnected("a");
        assert!(e.released.is_empty() && kinds(&e, "b").is_empty(), "nobody left, so nobody is told");
        assert_eq!(r.player_count(), 2, "the seat is hers, so the room is still full");
        assert!(r.join(Player::new("c", "Cy"), &Value::Null).is_err());

        let e = r.rejoin("a");
        assert_eq!(kinds(&e, "a"), ["joined", "state"], "and what she has, as it is now");
        assert_eq!(e.frames[0].1["resumed"], true);
        assert!(r.rejoin("ghost").frames.is_empty());
    }

    #[test]
    fn a_message_from_a_stranger_does_nothing() {
        let mut r = hall(8);
        let e = r.message("ghost", json!("hi"));
        assert!(e.frames.is_empty() && e.released.is_empty());
    }
}
