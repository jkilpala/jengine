//! **A game's side of `jengine-server`** - sign in, join a room, and see what
//! the server says is near you as entities in the scene.
//!
//! `jengine-server` keeps a world and tells each player what is in view
//! (`sync` frames from a `Space`). This is the half that was missing: without
//! it a game could reach a server only by writing its own protocol code in
//! Rust, as the first relay games did.
//!
//! # What you author
//!
//! One entity is the connection:
//!
//! ```json
//! "online": { "server": "ws://127.0.0.1:9001", "name": "Ada", "kind": "zone", "room": "meadow" }
//! ```
//!
//! and a **pool of slots** is what the server's entities are shown with - any
//! entities you like (a sprite, a label child, a script), each saying which
//! `kind` it shows:
//!
//! ```json
//! { "name": "Player 1", "components": { "online_slot": { "kind": "player" }, "sprite": { .. } } }
//! { "name": "Coin 1",   "components": { "online_slot": { "kind": "coin" },   "sprite": { .. } } }
//! ```
//!
//! When the server says a `player` came into view, a free `player` slot is
//! **bound** to it: moved to its position every frame (walked smoothly
//! between the server's ticks - see [`session`]), and given a `net_entity`
//! bag holding the server's properties for it (`id`, `kind`, `mine`, and
//! whatever the room set - `name`, `hp`). An entity the room turns
//! (`Space::set_rotation`) turns its slot too, walked between ticks the short
//! way round, and the walked facing is `rot` in the bag; one it never turns
//! leaves the slot's rotation to the scripts. A slot's `turn` names what is
//! turned instead - a child (so a name label over a ship stays level) or
//! `none` - and the facing is written before the scripts run, so a script
//! may still turn its own avatar after it (a bow that answers the helm now).
//! When it leaves view the slot is freed. An unbound slot is scaled to
//! nothing, children and all; bound again it gets its authored scale back
//! once, and is then the scripts' to scale, and any trail on it or under it
//! is cleared - a slot handed to another entity draws no streak from where
//! the last one was.
//!
//! **Why a pool and not spawning.** A package cannot instantiate a prefab -
//! that is a runtime's job, and each runtime does it differently. A pool is
//! plain entities the scene already has, so it behaves the same natively, in
//! the editor and on the web, and a slot can be anything a scene entity can.
//! The price is that the pool has a size: an entity with no free slot of its
//! kind is not shown, and `overflow` in the `online` bag counts them.
//!
//! # It talks to `jengine-net` through the world
//!
//! As any package reaching the network does: the `Net` channel on the connection's entity
//! is read and written by field name, so `jengine-net` must be *listed by the
//! project* but is not linked. This runs at `ScriptExec` -1, between
//! `jengine-net`'s receive and its send, so a frame queued this frame leaves
//! this frame.
//!
//! # Script commands
//!
//! | Call | Does |
//! |---|---|
//! | `Online.connect(e, server, name, token)` | connect and sign in; empty arguments keep what the component says |
//! | `Online.join(e, kind, room, options)` | join a room (now, or as soon as signed in); empty `room` asks for any |
//! | `Online.send(e, body)` | a message to the room - `body` is a table: `Online.send(e, { go = { x, y } })`; one too big for a frame goes in parts, put back together by the server |
//! | `Online.fetch(e, id, path, body, method)` | an HTTP request to the game server's own routes (`Server::get`, `/api/..`), signed in as the player (the bearer token is the session's); the answer is a `fetched` event `{ id, status, ok, body, error }`, `body` decoded when it is JSON. `body` nil is a GET, a table a POST; `method` overrides |
//! | `Online.send_latest(e, key, body, per_second)` | the newest `body` under `key`, sent at most `per_second` times a second (default 10) - a helm, an aim, a stroke |
//! | `Online.input(e, x, y)` | steer the player's own avatar, predicted - see "Steering, predicted" |
//! | `Online.leave(e)` | come out of the room |
//! | `Online.subscribe(e, channel)` / `unsubscribe` | join or leave a channel - chat beside the room, kept from room to room |
//! | `Online.say(e, channel, body)` | speak in it; everyone in it gets a `said` event |
//! | `Online.account(e, request)` | `{ change_password = { old, new } }` or `{ sign_out_everywhere = true }`, on a server with accounts; the new token is kept |
//! | `Online.disconnect(e)` | close the connection and stay offline |
//!
//! # What scripts read
//!
//! The `online` bag on the connection's entity: `status` (`offline`,
//! `signing_in`, `signed_in`, `joining`, `joined`), `net`, `player`, `name`,
//! `token` (save it: it is who the player is), `state`, `room`, `you`,
//! `joined`, `error`, `count`, `overflow`, and `events` - every frame the
//! server sent this frame except `sync`, so a room's own frames (`coins`,
//! `notice`) arrive as tables.
//!
//! # Steering, predicted
//!
//! `Online.send(e, { go = { x, y } })` asks the server to walk the avatar
//! somewhere, and it moves when the server moves it - a round trip and a tick
//! later. For direct control that delay is what "laggy" means, so there is a
//! second way:
//!
//! ```lua
//! Online.input(conn, dx, dy)     -- the stick, each -1..1; (0, 0) to stop
//! ```
//!
//! While the stick is held the package sends one numbered input per fixed
//! step and **applies it to the player's own slot at once**, with the same
//! step function the server runs (the `jengine-netcode` crate, linked by
//! both; the room sends its numbers on `joined`). When the server's answer
//! comes - which input it reached, and where that left the avatar - the
//! package starts again from there and replays what the server has not seen.
//! With a shared step function that changes nothing, so the avatar simply
//! moves when the key is pressed; if the server knew better (a wall, a
//! shove) the difference is eased out over a few frames. Everybody else is
//! drawn as before, a tick in the past. It needs a room that takes `input`
//! messages and says `"netcode"` on `joined`, as the zones example does.
//!
//! # A server of several machines
//!
//! A room may be on another node of the server's cluster. Then the server
//! answers a join - or a walk through a door - with a `redirect`, and the
//! package follows it: connects to the node named, signs in with its token,
//! and joins. A script sees `status` pass through `offline` for a moment and
//! a `redirect` event; it has nothing to do.
//!
//! # A dropped connection
//!
//! The package reconnects on its own (`retry_secs`, 0 to never), signs in
//! with the token it was given, and goes back to the room it was in - with
//! the options it joined that room with, so a room that reads a code or a
//! seat count from them knows its own player. The server treats a drop as a
//! leave, so the others see the player go and come back - but it is the same
//! player, where they were.
//!
//! # A room that changes under us
//!
//! A room may send `{ "t": "netcode", "netcode": { .. } }` mid-stay - the
//! shape it sent on `joined`, a wall built or broken - and the prediction
//! takes the new rule at once; and `{ "t": "you", "you": id }` when the
//! player's avatar is another entity now. Both are the package's to read,
//! so a room should not use those names for frames of its own.
//!
//! Except when the server let this connection go because the same player
//! signed in somewhere else ([`REPLACED`]: another window, or a second copy
//! sharing the save). Then the package stays off and raises a `replaced`
//! event after the `error`: reconnecting would push the other window off,
//! and the two would trade the player back and forth.

use std::collections::{BTreeMap, HashSet};

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::components::{CustomComponents, Name, Parent, Trail, Transform};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::script_api::ScriptApiRegistry;
use jengine_core::package::script_commands::PackageCommandRegistry;
use jengine_core::package::system_schedule::{SystemPhase, SystemSchedule};
use jengine_core::package::{JEnginePackage, PackageMetadata};
use jengine_core::scripting::host_commands::{CommandArgs, ScriptWorldHost};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub mod lockstep;
pub mod session;

use session::{Session, Status};

pub const PACKAGE: &str = "jengine-online";
/// The bag on the connection's entity that scripts read.
pub const BAG: &str = "online";
/// The bag on a bound slot.
pub const ENTITY_BAG: &str = "net_entity";
/// The server's refusal to a connection whose player signed in elsewhere
/// (`jengine-server`'s `run_conn`).
pub const REPLACED: &str = "signed in somewhere else";

/// The fields of `jengine-net`'s `Net` channel this package touches. Pinned
/// against `jengine_net::channel` by `tests/net_channel_contract.rs`.
pub mod net {
    pub const COMPONENT: &str = "Net";
    pub const URL: &str = "url";
    /// A connection of this entity's own, so two sessions in one scene do
    /// not share a socket.
    pub const OWN: &str = "own";
    pub const OUTBOX: &str = "outbox";
    /// HTTP requests for `jengine-net` to make, and their answers.
    pub const REQUESTS: &str = "requests";
    pub const RESPONSES: &str = "responses";
    pub const INBOX: &str = "inbox";
    pub const STATE: &str = "state";
}

/// A `send` frame longer than this goes in `part` frames. The server takes
/// frames of up to 64 KB, and a part's text is escaped once more inside it -
/// a quote becomes two characters - so a piece is under half of that.
pub const PART_BYTES: usize = 24 * 1024;

/// What the ids of `Online.fetch`'s requests start with on the `Net`
/// channel, so its answers are told from a script's own.
const FETCH_ID: &str = "online:";

#[derive(Debug, Clone)]
pub enum Request {
    Connect { server: String, name: String, token: String },
    Join { kind: String, room: String, options: Value },
    Send(Value),
    /// An HTTP request to the server's own routes, as the player.
    Fetch { id: String, path: String, method: String, body: Value },
    /// The newest body under a key, sent at most `per_second` times a second:
    /// a helm, a stroke, an aim - whatever only the latest of matters.
    SendLatest { key: String, body: Value, per_second: f64 },
    /// The stick: the direction to steer in, held until changed.
    Input(f64, f64),
    /// A raw frame for a channel: `sub`, `unsub` or `say`.
    Channel(Value),
    Leave,
    Disconnect,
}

/// `online`: the connection to a server.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Online {
    pub server: String,
    /// The name to sign in with. The server keeps the one a player first gave.
    pub name: String,
    /// Who the player is, from an earlier `welcome`. Empty makes a new player.
    pub token: String,
    /// A room to join as soon as signed in. Empty `kind` joins nothing.
    pub kind: String,
    pub room: String,
    /// Connect when the scene starts, without a script asking.
    pub auto_connect: bool,
    /// Seconds a position takes to be walked to. Zero is one server tick, as
    /// measured - the right answer unless the game wants it snappier or softer.
    pub smoothing: f32,
    /// Seconds before a dropped connection is tried again. Zero never does.
    pub retry_secs: f32,

    #[serde(skip)]
    pub session: Session,
    /// Whether we want to be connected at all.
    #[serde(skip)]
    wanted: bool,
    #[serde(skip)]
    hello_sent: bool,
    /// A redirect read this frame: the socket is the old node's until
    /// `jengine-net` next runs, so no `hello` goes down it now.
    #[serde(skip)]
    redirected: bool,
    /// Seconds since a sign-in was refused, on a connection that is still up.
    #[serde(skip)]
    refused_for: f32,
    /// The room to join once signed in: asked for, or the one a drop took us from.
    #[serde(skip)]
    pending_join: Option<(String, String, Value)>,
    #[serde(skip)]
    retry_in: Option<f32>,
    /// The direction being steered in.
    #[serde(skip)]
    stick: (f64, f64),
    /// Our own avatar, predicted - while it is being steered.
    #[serde(skip)]
    predictor: Option<jengine_netcode::Reconciler<jengine_netcode::BuiltIn>>,
    /// Time into the current fixed step.
    #[serde(skip)]
    step_time: f64,
    /// The number the next steering input carries. Kept across predictors:
    /// the room drops an input not newer than the last it applied, so a new
    /// spell of steering numbered from 1 again would be ignored.
    #[serde(skip)]
    next_seq: u32,
    /// Seconds the stick has been at rest with nothing unacknowledged.
    #[serde(skip)]
    at_rest: f32,
    /// The room the predictor belongs to.
    #[serde(skip)]
    predicted_join: u32,
    /// The rule it predicts with (the session's `netcode_rev`).
    #[serde(skip)]
    predicted_rule: u32,
    /// The entity it predicts.
    #[serde(skip)]
    predicted_you: Option<u64>,
    /// The channels we are in. A channel belongs to a connection, so after
    /// a reconnect - or a redirect to another node - they are joined again.
    #[serde(skip)]
    channels: std::collections::BTreeSet<String>,
    #[serde(skip)]
    started: bool,
    #[serde(skip)]
    net_state: String,
    #[serde(skip)]
    pub queue: Vec<Request>,
    #[serde(skip)]
    events: Vec<Value>,
    #[serde(skip)]
    overflow: usize,
    /// Numbers a message sent in parts.
    #[serde(skip)]
    next_part: u64,
    /// key -> (the newest body not yet sent, seconds between sends, seconds
    /// since the last).
    #[serde(skip)]
    latest: BTreeMap<String, (Option<Value>, f64, f64)>,
}

impl Default for Online {
    fn default() -> Self {
        Online {
            server: "ws://127.0.0.1:9001".into(),
            name: String::new(),
            token: String::new(),
            kind: String::new(),
            room: String::new(),
            auto_connect: true,
            smoothing: 0.0,
            retry_secs: 3.0,
            session: Session::new(),
            wanted: false,
            hello_sent: false,
            redirected: false,
            refused_for: 0.0,
            pending_join: None,
            retry_in: None,
            stick: (0.0, 0.0),
            predictor: None,
            step_time: 0.0,
            next_seq: 1,
            at_rest: 0.0,
            predicted_join: 0,
            predicted_rule: 0,
            predicted_you: None,
            channels: Default::default(),
            started: false,
            net_state: String::new(),
            queue: Vec::new(),
            events: Vec::new(),
            overflow: 0,
            next_part: 0,
            latest: BTreeMap::new(),
        }
    }
}

impl Clone for Online {
    /// The authored fields only: an inspector clone must not carry a live
    /// session, which would be a second connection as the same player.
    fn clone(&self) -> Self {
        Online {
            server: self.server.clone(),
            name: self.name.clone(),
            token: self.token.clone(),
            kind: self.kind.clone(),
            room: self.room.clone(),
            auto_connect: self.auto_connect,
            smoothing: self.smoothing,
            retry_secs: self.retry_secs,
            ..Default::default()
        }
    }
}

/// `online_slot`: an entity that shows one of the server's.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OnlineSlot {
    /// The `kind` of server entity this shows. Empty shows any kind that has
    /// no slot of its own left.
    pub kind: String,
    /// The **name** of the `online` entity this slot shows for, when a scene
    /// has more than one connection (a split screen, two accounts). Empty is
    /// the first `online` entity, which is every slot in a scene with one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session: String,
    /// What the server's facing turns: empty is the slot itself, a name is
    /// the entity of that name under it (a ship's hull, leaving its label
    /// level), and `none` is nothing - the facing is only `rot` in the bag.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub turn: String,
    /// The server entity it is showing.
    #[serde(skip)]
    pub bound: Option<u64>,
    /// The scale it was authored with, put back when it is bound.
    #[serde(skip)]
    scale: Option<[f32; 2]>,
    /// Bound this frame and not yet shown: its scale is put back once.
    #[serde(skip)]
    fresh: bool,
}

fn set_net(custom: &mut CustomComponents, field: &str, value: Value) {
    let entry = custom.0.entry(net::COMPONENT.to_string()).or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    // This entity's socket is its own: a second `online` entity gets another.
    entry[net::OWN] = json!(true);
    entry[field] = value;
}

fn push_outbox(custom: &mut CustomComponents, frame: String) {
    let entry = custom.0.entry(net::COMPONENT.to_string()).or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    if !entry[net::OUTBOX].is_array() {
        entry[net::OUTBOX] = json!([]);
    }
    entry[net::OUTBOX].as_array_mut().unwrap().push(Value::String(frame));
}

fn take_inbox(custom: &mut CustomComponents) -> Vec<String> {
    match custom.0.get_mut(net::COMPONENT).and_then(|c| c.get_mut(net::INBOX)) {
        Some(Value::Array(items)) => {
            std::mem::take(items).into_iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
        }
        _ => Vec::new(),
    }
}

impl Online {
    fn view(&self) -> Value {
        let s = &self.session;
        json!({
            "status": s.status.as_str(),
            "net": self.net_state,
            "player": s.player,
            "name": s.name,
            "token": s.token,
            "state": s.state,
            "room": s.room.as_ref().map(|(kind, id)| json!({ "kind": kind, "id": id })),
            "you": s.you,
            "joined": s.joined,
            "error": s.error,
            "count": s.entities.len(),
            "overflow": self.overflow,
            "tick": s.tick,
            "predicting": self.predictor.is_some(),
            "unacknowledged": self.predictor.as_ref().map(|p| p.pending()).unwrap_or(0),
        })
    }

    /// One frame of the connection: requests, the socket's state, the inbox.
    fn tick(&mut self, custom: &mut CustomComponents, dt: f32) {
        if !self.started {
            self.started = true;
            self.session.token = self.token.clone();
            if !self.kind.is_empty() {
                self.pending_join = Some((self.kind.clone(), self.room.clone(), Value::Null));
            }
            if self.auto_connect && !self.server.is_empty() {
                self.wanted = true;
                set_net(custom, net::URL, json!(self.server));
            }
        }
        self.net_state = custom
            .0
            .get(net::COMPONENT)
            .and_then(|c| c[net::STATE].as_str())
            .unwrap_or("")
            .to_string();
        let connected = self.net_state == "connected";

        // A disconnect and a connect in one frame (a "sign in again" button,
        // a new name) must not share a socket: `jengine-net` only sees the
        // URL as it is when it next runs, so "" then the same server would
        // be no change at all - the old socket stays up and a second `hello`
        // goes down it, which the server refuses ("already signed in"). So
        // whatever follows a disconnect waits for the next frame, by which
        // time the socket has been closed.
        let mut disconnected_now = false;
        let mut connect_now = false;
        let mut later = Vec::new();
        for request in std::mem::take(&mut self.queue) {
            if disconnected_now && !matches!(request, Request::Disconnect) || !later.is_empty() {
                later.push(request);
                continue;
            }
            match request {
                Request::Connect { server, name, token } => {
                    connect_now = true;
                    if !server.is_empty() {
                        self.server = server;
                    }
                    if !name.is_empty() {
                        self.name = name;
                    }
                    if !token.is_empty() {
                        self.session.token = token;
                    }
                    self.wanted = true;
                    self.retry_in = None;
                    set_net(custom, net::URL, json!(self.server));
                }
                Request::Join { kind, room, options } => {
                    if self.session.status == Status::Offline || self.session.status == Status::SigningIn {
                        self.pending_join = Some((kind, room, options));
                    } else if connected {
                        let frame = self.session.join(&kind, &room, &options);
                        push_outbox(custom, frame);
                    }
                }
                Request::Send(body) => {
                    if connected && self.session.status == Status::Joined {
                        self.send(custom, &body);
                    }
                }
                Request::Fetch { id, path, method, body } => self.fetch(custom, &id, &path, &method, &body),
                Request::SendLatest { key, body, per_second } => {
                    let interval = if per_second > 0.0 { 1.0 / per_second } else { 0.0 };
                    let entry = self.latest.entry(key).or_insert((None, interval, f64::INFINITY));
                    (entry.0, entry.1) = (Some(body), interval);
                }
                Request::Input(x, y) => self.stick = (x, y),
                Request::Channel(frame) => {
                    // Remembered whether or not it can be sent now: what we
                    // are in is what we ask for again on the next sign-in.
                    if let Some(channel) = frame["channel"].as_str() {
                        match frame["t"].as_str() {
                            Some("sub") => {
                                self.channels.insert(channel.to_string());
                            }
                            Some("unsub") => {
                                self.channels.remove(channel);
                            }
                            _ => {}
                        }
                    }
                    if connected && self.session.status != Status::Offline && self.session.status != Status::SigningIn {
                        push_outbox(custom, frame.to_string());
                    }
                }
                Request::Leave => {
                    self.pending_join = None;
                    if connected && self.session.room.is_some() {
                        push_outbox(custom, Session::leave_frame());
                    }
                }
                Request::Disconnect => {
                    disconnected_now = true;
                    self.wanted = false;
                    self.pending_join = None;
                    self.retry_in = None;
                    self.hello_sent = false;
                    self.session.dropped();
                    // Empty is "disconnect" to `jengine-net`, distinct from absent.
                    set_net(custom, net::URL, json!(""));
                }
            }
        }
        // Ahead of anything a script asks for before the next frame.
        later.append(&mut self.queue);
        self.queue = later;

        if std::mem::take(&mut self.redirected) {
            // Leaving for another node: nothing more on this socket.
        } else if connected && !disconnected_now {
            // (`connected` was read before this frame's requests: a socket
            // disconnected just now is going, and a `hello` down it marked
            // us signed in on it - so the next frame, finding it closed,
            // raised `dropped` for a disconnect the script asked for. Every
            // "sign in again" flashed "Connection lost".)
            self.retry_in = None;
            if !self.hello_sent {
                self.hello_sent = true;
                self.refused_for = 0.0;
                self.session.status = Status::SigningIn;
                push_outbox(custom, self.session.hello_frame(&self.name));
            } else if self.session.status == Status::Offline && self.wanted {
                // Connected, and the server would not sign us in - it could
                // not reach its store, or did not know the token. Ask again
                // in a while; the socket is fine.
                self.refused_for += dt;
                if self.refused_for >= self.retry_secs.max(0.5) {
                    self.hello_sent = false;
                }
            }
        } else if self.hello_sent {
            // The connection went away under us. Remember where we were, so
            // coming back is coming back to the same place.
            self.hello_sent = false;
            if let Some((kind, id)) = self.session.room.clone() {
                self.pending_join.get_or_insert((kind, id, self.session.options.clone()));
            }
            self.session.dropped();
            self.events.push(json!({ "t": "dropped" }));
        }

        // `jengine-net` does not reconnect: a failed connection stays failed
        // until the URL changes. So clear it, wait, and state it again.
        // Not on the frame a connect was asked for: the socket state read
        // above is the old one's ("closed"), and the retry would undo the
        // URL just set.
        if self.wanted && !connected && !connect_now {
            match &mut self.retry_in {
                Some(left) => {
                    *left -= dt;
                    if *left <= 0.0 {
                        self.retry_in = None;
                        set_net(custom, net::URL, json!(self.server));
                    }
                }
                None if matches!(self.net_state.as_str(), "failed" | "closed") && self.retry_secs > 0.0 => {
                    self.retry_in = Some(self.retry_secs);
                    set_net(custom, net::URL, json!(""));
                }
                None => {}
            }
        }

        // The newest of each kept message, when its time comes. Only in a
        // room: one held over a room change belonged to the room left.
        let joined = connected && self.session.status == Status::Joined;
        if !joined {
            self.latest.clear();
        }
        let mut due = Vec::new();
        for (body, interval, since) in self.latest.values_mut() {
            *since += f64::from(dt);
            if body.is_some() && *since >= *interval {
                *since = 0.0;
                due.extend(body.take());
            }
        }
        for body in due {
            self.send(custom, &body);
        }

        self.predict(custom, f64::from(dt));
    }

    /// The game server's own HTTP address: the socket's, as `http(s)`, with
    /// no `/ws` - where `Server::get` routes and a gateway's prefix live.
    fn http_base(&self) -> String {
        let base = self.server.trim_end_matches('/');
        let base = base.strip_suffix("/ws").unwrap_or(base);
        if let Some(rest) = base.strip_prefix("wss://") {
            format!("https://{rest}")
        } else if let Some(rest) = base.strip_prefix("ws://") {
            format!("http://{rest}")
        } else {
            base.to_string()
        }
    }

    /// Queue an HTTP request for `jengine-net` - which answers on every
    /// `Net` channel, so the id says it is ours.
    fn fetch(&mut self, custom: &mut CustomComponents, id: &str, path: &str, method: &str, body: &Value) {
        let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
        let method = if !method.is_empty() { method.to_uppercase() } else if body.is_null() { "GET".into() } else { "POST".into() };
        let mut headers = serde_json::Map::new();
        if !self.session.token.is_empty() {
            headers.insert("Authorization".into(), json!(format!("Bearer {}", self.session.token)));
        }
        let mut request = json!({ "id": format!("{FETCH_ID}{id}"), "url": format!("{}{path}", self.http_base()), "method": method });
        if !body.is_null() {
            headers.insert("Content-Type".into(), json!("application/json"));
            request["body"] = json!(body.to_string());
        }
        request["headers"] = Value::Object(headers);
        let entry = custom.0.entry(net::COMPONENT.to_string()).or_insert_with(|| json!({}));
        if !entry[net::REQUESTS].is_array() {
            entry[net::REQUESTS] = json!([]);
        }
        entry[net::REQUESTS].as_array_mut().unwrap().push(request);
    }

    /// Our answers from the `Net` channel, as `fetched` events; anybody
    /// else's are left where they are.
    fn take_fetched(&mut self, custom: &mut CustomComponents) {
        let Some(Value::Array(items)) = custom.0.get_mut(net::COMPONENT).and_then(|c| c.get_mut(net::RESPONSES)) else {
            return;
        };
        let (ours, theirs): (Vec<Value>, Vec<Value>) = std::mem::take(items)
            .into_iter()
            .partition(|r| r["id"].as_str().is_some_and(|id| id.starts_with(FETCH_ID)));
        *items = theirs;
        for r in ours {
            let id = r["id"].as_str().unwrap_or("").trim_start_matches(FETCH_ID).to_string();
            let text = r["body"].as_str().unwrap_or("");
            let body = serde_json::from_str::<Value>(text).unwrap_or_else(|_| json!(text));
            self.events.push(json!({
                "t": "fetched", "id": id, "status": r["status"], "ok": r["ok"], "body": body, "error": r["error"],
            }));
        }
    }

    /// A message to the room: one `send` frame, or - too big for one - the
    /// same frame's text in `part` frames the server puts back together.
    fn send(&mut self, custom: &mut CustomComponents, body: &Value) {
        let frame = Session::send_frame(body);
        if frame.len() <= PART_BYTES {
            return push_outbox(custom, frame);
        }
        let mut pieces = Vec::new();
        let mut rest = frame.as_str();
        while !rest.is_empty() {
            let mut cut = rest.len().min(PART_BYTES);
            while !rest.is_char_boundary(cut) {
                cut -= 1;
            }
            pieces.push(&rest[..cut]);
            rest = &rest[cut..];
        }
        self.next_part += 1;
        let of = pieces.len();
        for (i, text) in pieces.into_iter().enumerate() {
            let part = json!({ "t": "part", "id": self.next_part, "i": i, "of": of, "text": text });
            push_outbox(custom, part.to_string());
        }
    }

    /// What arrived this frame: the server's frames read into events and
    /// entities, and the entities walked on by `dt`. Run right after
    /// `jengine-net` reads the socket, before the scripts - so a script sees
    /// a frame on the frame it arrives, not the one after (every answer in
    /// the three MMOs was a frame later than it needed to be, Oct 2026).
    fn receive(&mut self, custom: &mut CustomComponents, dt: f32) {
        self.take_fetched(custom);
        for frame in take_inbox(custom) {
            let Some(event) = self.session.receive(&frame) else { continue };
            // The server measuring our round trip: answered at once, and no
            // script's business.
            if event["t"] == "ping" {
                push_outbox(custom, json!({ "t": "pong", "n": event["n"] }).to_string());
                continue;
            }
            // This player signed in somewhere else - another window, another
            // copy sharing the save - and the server let this connection go.
            // That is not a lost connection: coming back would push the other
            // one off in turn, and the two traded the player for as long as
            // both ran, each showing "Connection lost" while it played on. So
            // stay off, and say what happened.
            if event["t"] == "error" && event["why"] == REPLACED {
                self.wanted = false;
                self.retry_in = None;
                self.pending_join = None;
                self.hello_sent = false;
                self.session.dropped();
                set_net(custom, net::URL, json!(""));
                self.events.push(event);
                self.events.push(json!({ "t": "replaced" }));
                break;
            }
            // The room is on another machine of the server's cluster: go
            // there. Who we are - the token - comes along; the join follows
            // the `welcome` there, as it would after any reconnect.
            if event["t"] == "redirect"
                && let Some(url) = event["url"].as_str()
            {
                self.server = url.to_string();
                if let Some(kind) = event["kind"].as_str() {
                    let room = event["room"].as_str().unwrap_or("").to_string();
                    self.pending_join = Some((kind.to_string(), room, event["options"].clone()));
                }
                self.hello_sent = false;
                self.redirected = true;
                self.session.dropped();
                set_net(custom, net::URL, json!(""));
                self.retry_in = Some(0.05);
                self.events.push(event);
                break;
            }
            // Signed in - for the first time or again. The channels we were
            // in are the old connection's; join them on this one.
            if event["t"] == "welcome" {
                for channel in &self.channels {
                    push_outbox(custom, json!({ "t": "sub", "channel": channel }).to_string());
                }
            }
            // Signed in: now the room that was asked for, or that we were in.
            if event["t"] == "welcome"
                && let Some((kind, room, options)) = self.pending_join.take()
            {
                let frame = self.session.join(&kind, &room, &options);
                push_outbox(custom, frame);
            }
            self.events.push(event);
        }
        self.session.advance(f64::from(dt), f64::from(self.smoothing));
    }

    /// Steer our own avatar: one numbered input per fixed step, applied
    /// here at once and sent; the server's answers reconciled against.
    fn predict(&mut self, custom: &mut CustomComponents, dt: f64) {
        let ready = self.session.status == Status::Joined && self.net_state == "connected";
        let (Some((step, field)), Some(you), true) = (self.session.netcode.clone(), self.session.you, ready) else {
            self.predictor = None;
            return;
        };
        // A new room is a new place, and a new avatar a new thing to steer:
        // nothing predicted for the last one holds.
        if self.predicted_join != self.session.joins || self.predicted_you != Some(you) {
            self.predicted_join = self.session.joins;
            self.predicted_you = Some(you);
            self.predicted_rule = self.session.netcode_rev;
            self.predictor = None;
        }
        // The room's walls changed: steer by the new ones from the next step.
        if self.predicted_rule != self.session.netcode_rev {
            self.predicted_rule = self.session.netcode_rev;
            if let Some(predictor) = self.predictor.as_mut() {
                predictor.set_motion(field.clone());
            }
        }
        let Some(server_at) = self.session.entities.get(&you).map(|e| e.server()) else { return };
        let steering = self.stick != (0.0, 0.0);
        if self.predictor.is_none() {
            if !steering {
                // Not steering: the server walks the avatar, as it does everyone.
                let _ = self.session.take_ack();
                return;
            }
            // Positions cross the wire to two decimals; half a hundredth
            // each way is the rounding, not a disagreement.
            self.predictor = Some(
                jengine_netcode::Reconciler::new(field, step, server_at).with_slack(0.02).starting_at(self.next_seq),
            );
            // The first step is taken on this frame, not a whole step
            // from now: started at zero, the walk began a step (100 ms in
            // every zone so far) after the key went down - the largest
            // single delay all three MMOs measured, network included.
            self.step_time = step;
            self.at_rest = 0.0;
        }
        let Some(predictor) = self.predictor.as_mut() else { return };
        if let Some((ack, at)) = self.session.take_ack() {
            predictor.server(at, ack);
        }
        self.step_time += dt;
        // A hitch must not turn into a burst of inputs the server would
        // only queue and drop.
        self.step_time = self.step_time.min(step * 4.0);
        while self.step_time >= step {
            self.step_time -= step;
            if steering {
                let input = predictor.input(self.stick.0, self.stick.1);
                self.next_seq = predictor.next_seq();
                let body = json!({ "input": { "seq": input.seq, "x": input.x, "y": input.y } });
                push_outbox(custom, Session::send_frame(&body));
            } else {
                predictor.rest();
            }
        }
        predictor.ease(dt);
        // Let go and all caught up: hand the avatar back to the server's
        // own telling of it, which by now is the same place.
        if steering || predictor.pending() > 0 {
            self.at_rest = 0.0;
        } else {
            self.at_rest += dt as f32;
            if self.at_rest > 0.5 && predictor.error() == 0.0 {
                self.predictor = None;
            }
        }
    }

    /// Where our own avatar is drawn while it is being predicted.
    fn predicted_position(&self) -> Option<(f64, f64)> {
        let (step, _) = self.session.netcode.as_ref()?;
        Some(self.predictor.as_ref()?.position(self.step_time / step))
    }
}

/// Forget the trail on a slot and everything under it: the slot is about to
/// show something else, somewhere else, and a ribbon from where the last one
/// was would streak across the map to it.
fn clear_trails(world: &mut World, slot: Entity) {
    for e in std::iter::once(slot).chain(descendants(world, slot)) {
        if let Ok(mut trail) = world.get::<&mut Trail>(e) {
            trail.points.clear();
        }
    }
}

/// Hide a slot nothing is bound to: scaled to nothing, so its children and
/// its collider go with it.
fn park(slot: &mut OnlineSlot, transform: &mut Transform, custom: Option<&mut CustomComponents>) {
    if slot.scale.is_none() {
        slot.scale = Some(transform.scale.to_array());
    }
    slot.bound = None;
    transform.scale = Vec2::ZERO;
    if let Some(custom) = custom {
        custom.0.remove(ENTITY_BAG);
    }
}

/// What one connection shows this frame.
#[derive(Default)]
struct View {
    /// What the server says is in view, by id.
    shown: BTreeMap<u64, Shown>,
    entered: Vec<u64>,
    left: HashSet<u64>,
    overflow: usize,
}

struct Shown {
    kind: String,
    x: f64,
    y: f64,
    /// `None`: the room gives it no facing, so the slot's is left alone.
    rot: Option<f64>,
    bag: Value,
}

/// Which half of the frame [`step_pass`] is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// After the socket is read, before the scripts: what arrived becomes
    /// events and entities, slots move, events are published.
    Receive,
    /// After the scripts, before the socket is written: what they asked for
    /// goes out, our own avatar is predicted, slots move again. Events are
    /// left as published: those raised here wait for the next `Receive`.
    Send,
}

/// Both halves of a frame, for a caller with one system to run. A host runs
/// them apart (`online_receive` at `PreUpdate`, `online_step` at
/// `ScriptExec`), so scripts read this frame's frames.
pub fn step(world: &mut World, dt: f32) {
    step_pass(world, dt, Pass::Receive);
    step_pass(world, dt, Pass::Send);
}

/// The receiving half: run right after `jengine-net`'s `net_receive`.
pub fn receive(world: &mut World, dt: f32) {
    step_pass(world, dt, Pass::Receive);
}

/// The sending half: run after the scripts and before `jengine-net`'s `net_send`.
pub fn send(world: &mut World, dt: f32) {
    step_pass(world, dt, Pass::Send);
}

fn step_pass(world: &mut World, dt: f32, pass: Pass) {
    let mut sessions: Vec<Entity> = world.query::<&Online>().iter().map(|(e, _)| e).collect();
    sessions.sort_by_key(|e| e.id());
    let names: Vec<String> =
        sessions.iter().map(|e| world.get::<&Name>(*e).map(|n| n.0.clone()).unwrap_or_default()).collect();
    let mut views: Vec<View> = Vec::with_capacity(sessions.len());
    for e in &sessions {
        if world.get::<&CustomComponents>(*e).is_err() {
            let _ = world.insert_one(*e, CustomComponents::default());
        }
        let mut query = world.query_one::<(&mut Online, &mut CustomComponents)>(*e).unwrap();
        let Some((online, custom)) = query.get() else { continue };
        match pass {
            Pass::Receive => online.receive(custom, dt),
            Pass::Send => online.tick(custom, dt),
        }
        let mut view = View::default();
        let changes = online.session.take_changes();
        view.entered = changes.entered;
        view.left = changes.left.into_iter().collect();
        let predicted = online.predicted_position();
        for (id, entity) in &online.session.entities {
            let mut bag = Value::Object(entity.props.clone());
            let mine = online.session.you == Some(*id);
            bag["id"] = json!(id);
            bag["mine"] = json!(mine);
            if let Some(rot) = entity.rot {
                bag["rot"] = json!(rot);
            }
            // Our own avatar, while steered, is where we reckon it is -
            // not where the server last said, a round trip ago.
            let (x, y) = predicted.filter(|_| mine).unwrap_or((entity.x, entity.y));
            if mine {
                let (sx, sy) = entity.server();
                bag["predicted"] = json!(predicted.is_some());
                bag["server_x"] = json!(sx);
                bag["server_y"] = json!(sy);
            }
            view.shown.insert(*id, Shown { kind: entity.kind.clone(), x, y, rot: entity.rot, bag });
        }
        views.push(view);
    }

    // Each slot shows for one session: the one it names, else the first.
    let mut slots: Vec<Entity> = world.query::<&OnlineSlot>().iter().map(|(e, _)| e).collect();
    slots.sort_by_key(|e| e.id());
    let owner_of = |slot: &OnlineSlot| -> usize {
        if slot.session.is_empty() {
            return 0;
        }
        names.iter().position(|n| *n == slot.session).unwrap_or(0)
    };
    for (which, view) in views.iter_mut().enumerate() {
        let mine: Vec<Entity> = slots
            .iter()
            .copied()
            .filter(|e| world.get::<&OnlineSlot>(*e).is_ok_and(|slot| owner_of(&slot) == which))
            .collect();
        view.overflow = bind_slots(world, &mine, view, pass);
    }

    // Publish what scripts read - after the slots, so `overflow` is this frame's.
    for (e, view) in sessions.iter().zip(views) {
        let Ok(mut query) = world.query_one::<(&mut Online, &mut CustomComponents)>(*e) else { continue };
        let Some((online, custom)) = query.get() else { continue };
        online.overflow = view.overflow;
        let mut view = online.view();
        view["events"] = match pass {
            Pass::Receive => Value::Array(std::mem::take(&mut online.events)),
            // Keep what `Receive` published for this frame's readers.
            Pass::Send => custom.0.get(BAG).map_or_else(|| json!([]), |b| b["events"].clone()),
        };
        if view["events"].is_null() {
            view["events"] = json!([]);
        }
        custom.0.insert(BAG.to_string(), view);
    }
}

/// Slots: free the ones whose entity left, bind the ones that entered, and
/// move every bound one to where its entity is shown. Returns how many
/// entities had no slot.
fn bind_slots(world: &mut World, slots: &[Entity], view: &View, pass: Pass) -> usize {
    let (shown, left) = (&view.shown, &view.left);
    let mut bound: HashSet<u64> = HashSet::new();
    for e in slots {
        let Ok(mut query) = world.query_one::<(&mut OnlineSlot, &mut Transform, Option<&mut CustomComponents>)>(*e)
        else {
            continue;
        };
        let Some((slot, transform, custom)) = query.get() else { continue };
        match slot.bound {
            Some(id) if !left.contains(&id) && shown.contains_key(&id) => {
                bound.insert(id);
            }
            // Bound to something gone, or never yet looked at.
            _ if slot.bound.is_some() || slot.scale.is_none() => park(slot, transform, custom),
            _ => {}
        }
    }
    // Entered, plus anything shown that no slot has (a slot freed this frame
    // can take an entity that had none).
    let mut waiting: Vec<u64> = view.entered.iter().copied().filter(|id| !bound.contains(id)).collect();
    waiting.extend(shown.keys().copied().filter(|id| !bound.contains(id)));
    let mut overflow = 0;
    let mut seen = HashSet::new();
    for id in waiting {
        if !seen.insert(id) || bound.contains(&id) {
            continue;
        }
        let Some(Shown { kind, .. }) = shown.get(&id) else { continue };
        // A slot of its own kind first; then one that takes any.
        let free = |wanted: &str| {
            slots.iter().copied().find(|e| {
                world.get::<&OnlineSlot>(*e).is_ok_and(|slot| slot.bound.is_none() && slot.kind == wanted)
            })
        };
        match free(kind).or_else(|| free("")) {
            Some(e) => {
                if let Ok(mut slot) = world.get::<&mut OnlineSlot>(e) {
                    slot.bound = Some(id);
                    slot.fresh = true;
                }
                clear_trails(world, e);
                bound.insert(id);
            }
            None => overflow += 1,
        }
    }
    // Facings to write: (the entity turned, radians). Only before the
    // scripts, so what a script turns after it stands for the frame.
    let mut turns: Vec<(Entity, String, f32)> = Vec::new();
    for e in slots {
        if world.get::<&CustomComponents>(*e).is_err() {
            let _ = world.insert_one(*e, CustomComponents::default());
        }
        let Ok(mut query) = world.query_one::<(&mut OnlineSlot, &mut Transform, &mut CustomComponents)>(*e) else {
            continue;
        };
        let Some((slot, transform, custom)) = query.get() else { continue };
        let Some(Shown { x, y, rot, bag, .. }) = slot.bound.and_then(|id| shown.get(&id)) else { continue };
        transform.position = Vec2::new(*x as f32, *y as f32);
        if let (Some(rot), Pass::Receive) = (rot, pass) {
            match slot.turn.as_str() {
                "" => transform.rotation = *rot as f32,
                "none" => {}
                child => turns.push((*e, child.to_string(), *rot as f32)),
            }
        }
        // Its authored size back once, as it is shown; after that the
        // scale is the scripts' (a cell sized by how much it has eaten).
        if std::mem::take(&mut slot.fresh) {
            transform.scale = Vec2::from(slot.scale.unwrap_or([1.0, 1.0]));
        }
        custom.0.insert(ENTITY_BAG.to_string(), bag.clone());
    }
    for (slot, child, rot) in turns {
        let Some(target) = descendant(world, slot, &child) else { continue };
        if let Ok(mut t) = world.get::<&mut Transform>(target) {
            t.rotation = rot;
        }
    }
    overflow
}

/// Everything under `root`, its children first.
fn descendants(world: &World, root: Entity) -> Vec<Entity> {
    let mut under = vec![root];
    let mut i = 0;
    while i < under.len() {
        let parent = under[i];
        under.extend(world.query::<&Parent>().iter().filter(|(_, p)| p.0 == parent).map(|(e, _)| e));
        i += 1;
    }
    under.remove(0);
    under
}

/// The entity named `name` under `root`.
fn descendant(world: &World, root: Entity, name: &str) -> Option<Entity> {
    descendants(world, root).into_iter().find(|e| world.get::<&Name>(*e).is_ok_and(|n| n.0 == name))
}

fn with_online(host: &mut dyn ScriptWorldHost, args: &dyn CommandArgs, cmd: &str, f: impl FnOnce(&mut Online)) {
    let Some(id) = args.number("e").filter(|v| *v >= 0.0) else { return };
    let Some(e) = host.resolve_entity(id as u32) else {
        log::warn!("[online] {cmd}: no entity {id}");
        return;
    };
    match host.world().get::<&mut Online>(e) {
        Ok(mut online) => f(&mut online),
        Err(_) => log::warn!("[online] {cmd}: entity has no `online` component"),
    }
}

fn register_commands(c: &mut PackageCommandRegistry) -> Result<(), String> {
    c.register(PACKAGE, "online_connect", &["e", "server", "name", "token"], |host, args| {
        let request = Request::Connect {
            server: args.text("server").unwrap_or_default(),
            name: args.text("name").unwrap_or_default(),
            token: args.text("token").unwrap_or_default(),
        };
        with_online(host, args, "online_connect", |o| o.queue.push(request));
    })?;
    c.register(PACKAGE, "online_join", &["e", "kind", "room", "options"], |host, args| {
        let Some(kind) = args.text("kind").filter(|k| !k.is_empty()) else {
            log::warn!("[online] online_join needs a kind of room");
            return;
        };
        let request =
            Request::Join { kind, room: args.text("room").unwrap_or_default(), options: args.json("options") };
        with_online(host, args, "online_join", |o| o.queue.push(request));
    })?;
    c.register(PACKAGE, "online_send", &["e", "body"], |host, args| {
        let body = args.json("body");
        with_online(host, args, "online_send", |o| o.queue.push(Request::Send(body)));
    })?;
    c.register(PACKAGE, "online_fetch", &["e", "id", "path", "body", "method"], |host, args| {
        let request = Request::Fetch {
            id: args.text("id").unwrap_or_default(),
            path: args.text("path").unwrap_or_default(),
            method: args.text("method").unwrap_or_default(),
            body: args.json("body"),
        };
        with_online(host, args, "online_fetch", |o| o.queue.push(request));
    })?;
    c.register(PACKAGE, "online_send_latest", &["e", "key", "body", "per_second"], |host, args| {
        let request = Request::SendLatest {
            key: args.text("key").unwrap_or_default(),
            body: args.json("body"),
            per_second: args.number("per_second").unwrap_or(10.0),
        };
        with_online(host, args, "online_send_latest", |o| o.queue.push(request));
    })?;
    c.register(PACKAGE, "online_subscribe", &["e", "channel"], |host, args| {
        let frame = json!({ "t": "sub", "channel": args.text("channel").unwrap_or_default() });
        with_online(host, args, "online_subscribe", |o| o.queue.push(Request::Channel(frame)));
    })?;
    c.register(PACKAGE, "online_unsubscribe", &["e", "channel"], |host, args| {
        let frame = json!({ "t": "unsub", "channel": args.text("channel").unwrap_or_default() });
        with_online(host, args, "online_unsubscribe", |o| o.queue.push(Request::Channel(frame)));
    })?;
    c.register(PACKAGE, "online_say", &["e", "channel", "body"], |host, args| {
        let frame =
            json!({ "t": "say", "channel": args.text("channel").unwrap_or_default(), "body": args.json("body") });
        with_online(host, args, "online_say", |o| o.queue.push(Request::Channel(frame)));
    })?;
    c.register(PACKAGE, "online_account", &["e", "request"], |host, args| {
        // `{ change_password = { old = .., new = .. } }` or
        // `{ sign_out_everywhere = true }`; the answer is an `account` event.
        let mut frame = args.json("request");
        if !frame.is_object() {
            log::warn!("[online] online_account needs a table");
            return;
        }
        frame["t"] = json!("account");
        with_online(host, args, "online_account", |o| o.queue.push(Request::Channel(frame)));
    })?;
    c.register(PACKAGE, "online_input", &["e", "x", "y"], |host, args| {
        let (x, y) = (args.number("x").unwrap_or(0.0), args.number("y").unwrap_or(0.0));
        with_online(host, args, "online_input", |o| o.queue.push(Request::Input(x, y)));
    })?;
    c.register(PACKAGE, "online_leave", &["e"], |host, args| {
        with_online(host, args, "online_leave", |o| o.queue.push(Request::Leave));
    })?;
    // A lockstep session's room and name, chosen by a lobby before it loads
    // the match scene, whose script cannot run until the match starts.
    c.register(PACKAGE, "online_lockstep_match", &["room", "name"], |_host, args| {
        let room = args.text("room").unwrap_or_default().trim().to_uppercase();
        let name = args.text("name").unwrap_or_default();
        lockstep::set_next_match(&room, name.trim());
    })?;
    c.register(PACKAGE, "online_lockstep_leave", &["e"], |host, args| {
        let Some(id) = args.number("e").filter(|v| *v >= 0.0) else { return };
        let Some(e) = host.resolve_entity(id as u32) else { return };
        match host.world().get::<&mut lockstep::StepSession>(e) {
            Ok(mut s) => s.leave(),
            Err(_) => log::warn!("[online] online_lockstep_leave: entity has no `lockstep_session`"),
        }
    })?;
    c.register(PACKAGE, "online_disconnect", &["e"], |host, args| {
        with_online(host, args, "online_disconnect", |o| o.queue.push(Request::Disconnect));
    })?;
    Ok(())
}

pub struct OnlinePackage;

impl JEnginePackage for OnlinePackage {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: PACKAGE.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "A game's side of jengine-server: sign in, join rooms, and show synced entities".into(),
            author: String::new(),
            engine_version: Some("0.1.0".into()),
            // A runtime dependency, not a crate one: the `Net` channel is
            // `jengine-net`'s, and only its systems move the bytes.
            dependencies: vec!["jengine-net".into()],
        }
    }

    fn register_components(&self, registry: &mut ComponentRegistry) {
        registry.register::<Online>("online", "Online", "Networking");
        registry.register::<OnlineSlot>("online_slot", "Online Slot", "Networking");
        registry.register_defaulted::<lockstep::StepSession>("lockstep_session", "Lockstep Session", "Networking");
    }

    fn register_systems(&self, schedule: &mut SystemSchedule) {
        // Right after `jengine-net`'s receive (PreUpdate 0), before the scripts.
        schedule.add_system("online_receive", SystemPhase::PreUpdate, 1, |ctx| {
            let (world, dt, _) = ctx.split();
            receive(world, dt);
        });
        // After the scripts, before `jengine-net`'s send (ScriptExec 0).
        schedule.add_system("online_step", SystemPhase::ScriptExec, -1, |ctx| {
            let (world, dt, _) = ctx.split();
            send(world, dt);
        });
        // Fixed-step lockstep sessions (`lockstep`): the network and the step
        // gate once a frame, between `jengine-net`'s receive (Frame 0) and
        // send (Frame 20); every player's input first thing in each step,
        // before any game reads it.
        schedule.add_system("online_lockstep_frame", SystemPhase::Frame, 10, |ctx| {
            let (world, dt, resources) = ctx.split();
            lockstep::frame(world, dt, resources.get::<jengine_core::input::InputState>());
        });
        schedule.add_system("online_lockstep_step", SystemPhase::PreUpdate, -100, |ctx| {
            let (world, _dt, resources) = ctx.split();
            lockstep::step(world, resources.get::<jengine_core::input::InputState>());
        });
    }

    fn register_script_api(&self, _api: &mut ScriptApiRegistry) {}

    fn register_script_commands(&self, commands: &mut PackageCommandRegistry) {
        if let Err(e) = register_commands(commands) {
            log::error!("[online] script commands not registered: {e}");
        }
    }
}
