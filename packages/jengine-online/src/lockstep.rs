//! **Input lockstep in fixed steps**: two to eight machines run one game,
//! sending each other nothing but what their players pressed.
//!
//! A `lockstep_session` on an entity joins a `lockstep` room on a
//! `jengine-server` (`Server::lockstep`, `jengine_server::lockstep`) through
//! the entity's `Net` channel, and from then on the engine's fixed-step clock
//! (`settings.simulation_hz` or the scene's own, `ecs::fixed_step`) runs a
//! step only when every player's input for it has come back from the room -
//! `ecs::fixed_step::set_gate`. So every machine runs every step with the
//! same inputs, and a deterministic game (phases 1 - 4 of
//! docs/determinism-proposal.md) plays out the same everywhere.
//!
//! What a player sends for a step is which of the project's input actions
//! they hold, as one number (bit `k` is the `k`-th action in sorted order -
//! the same list on every machine that loaded the project), sent `delay`
//! steps ahead so it is back before it is needed. A session can send more:
//! how far each of its `analog` actions is pushed (a stick, a trigger, in
//! thousandths) and, with `pointer`, where the mouse is in the world (in
//! hundredths of a unit) and which of its buttons are down. A game reads
//! **every** player's input from the session entity's `Lockstep` custom
//! component - never from `Input.*`, which is only this machine's player,
//! and now:
//!
//! - `held`, `pressed`, `released`: one list of action names per seat, in
//!   seat order (`held[1]` is seat 0's in Lua);
//! - `axes`: per seat, `{ action = 0..1 }` for each `analog` action;
//! - `pointer`: per seat, `{ x, y, left, right }` with `pointer` on;
//! - `away` and `silent`: per seat, whether it is played as "nothing held" -
//!   its player has gone, or has sent nothing for a while (the room's
//!   `silent_after`); `names`: per seat;
//! - `seat` (this machine's, from 0; `nil` for a spectator), `seats`, `step`
//!   (the one running), `behind` (steps still to replay), `state`
//!   (`connecting`, `waiting`, `catching_up`, `running`, `reconnecting`,
//!   `ended`, `desync`, `left`).
//!
//! Every `hash_every` steps the session sends a hash of the world
//! (`ecs::fixed_step::world_hash`: every root entity's transform), and the
//! room ends a match whose machines disagree (`state = "desync"`) rather than
//! let them play on apart.
//!
//! The session gates from the moment its scene loads, so every machine starts
//! the match at step 0 from the same freshly loaded scene: put the lobby in a
//! scene before it. Until the room starts the match nothing in the scene
//! steps; a "waiting for the other player" text the scene shows, which a
//! script hides on its first `update`, is the whole waiting screen. No
//! `update` runs while it waits, so the session itself watches its `leave`
//! actions then: one pressed gives the seat up (`state = "left"`). Once the
//! match is over - `ended`, `desync`, `left` - the scene steps freely again
//! with nothing held, so its script can say how it ended and move on.
//!
//! **Joining late is replaying.** The room keeps every step, so a machine
//! that joins a started match - a player back after closing the game, a
//! `spectate` session - loads the scene, takes the match so far from the
//! room and runs it fast (`ecs::fixed_step::set_catch_up`, `catch_up` steps a
//! frame) until it is level, then plays on with everyone. A session whose
//! connection drops reconnects by itself every `reconnect_every` seconds and
//! takes its seat back the same way, from where it was; meanwhile the others
//! play on with it as "nothing held".

use std::collections::BTreeMap;

use hecs::{Entity, World};
use jengine_core::ecs::camera_rig::mouse_world_position;
use jengine_core::ecs::components::CustomComponents;
use jengine_core::ecs::fixed_step::{set_catch_up, set_gate, world_hash};
use jengine_core::input::InputState;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{push_outbox, set_net, take_inbox};

/// The component scripts read every player's input from.
pub const COMPONENT: &str = "Lockstep";
/// Most input actions a step's number can carry.
pub const MAX_ACTIONS: usize = 32;
/// Most `analog` actions a session sends.
pub const MAX_ANALOG: usize = 8;
/// How often a session sends at most: a message a frame at 165 Hz would be
/// over a server's per-connection limit (`jengine-server`, 60 a second).
pub const SEND_EVERY: f32 = 1.0 / 30.0;
/// How long with no step back before this player's unconfirmed inputs are
/// sent again.
pub const RESEND_AFTER: f32 = 0.3;
/// Inputs one message carries at most - the room's `MAX_BATCH`.
pub const MAX_BATCH: usize = 64;

/// `lockstep_session`: this entity is a seat in a lockstep match.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StepSession {
    /// The `jengine-server` to join, `ws://..`.
    pub url: String,
    /// The room: every machine naming it plays in the one match. Empty takes
    /// the room a script chose before the scene loaded ([`set_next_match`],
    /// `Online.lockstep_match`), else `lockstep`.
    pub room: String,
    /// Players in the match; it starts when they have all joined.
    pub seats: u32,
    /// This player's name, for the others - and the seat a rejoin asks for.
    /// Empty takes the name given with [`set_next_match`].
    pub name: String,
    /// The input actions sent, in this order (32 at most); empty sends every
    /// action the project has, sorted - which in a project with more than 32
    /// is not the ones a game wants.
    pub actions: Vec<String>,
    /// Steps between a press and the step it acts on - enough for it to go
    /// to the room and back. 3 at 60 Hz is 50 ms.
    pub delay: u32,
    /// Steps between world hashes; 0 sends none.
    pub hash_every: u32,
    /// Watch the match rather than play in it: no seat, no input.
    pub spectate: bool,
    /// Actions whose strength is sent as well as whether they are held - a
    /// stick, a trigger. Read from `axes`.
    pub analog: Vec<String>,
    /// Send where this player's mouse is in the world, and its buttons.
    pub pointer: bool,
    /// Seconds a room may wait on a silent seat before playing it as
    /// "nothing held"; sent by the session that opens the room. 0 waits for
    /// ever.
    pub silent_after: f32,
    /// Steps a frame a late joiner replays at most while catching up.
    pub catch_up: u32,
    /// Seconds between attempts to reconnect after the connection drops.
    pub reconnect_every: f32,
    /// Actions that give the seat up while the match has not started (or is
    /// reconnecting): the scene steps no `update` then, so a script could
    /// not react to them itself. The state becomes `left`, and the scene
    /// steps again for its script to go back to the lobby.
    pub leave: Vec<String>,
    #[serde(skip)]
    run: Run,
}

impl Default for StepSession {
    fn default() -> Self {
        Self {
            url: String::new(),
            room: "lockstep".into(),
            seats: 2,
            name: String::new(),
            actions: Vec::new(),
            delay: 3,
            hash_every: 30,
            spectate: false,
            analog: Vec::new(),
            pointer: false,
            silent_after: 3.0,
            catch_up: 32,
            reconnect_every: 2.0,
            leave: Vec::new(),
            run: Run::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Connecting,
    Waiting,
    Running,
    Reconnecting,
    Ended,
    Desync,
    Left,
}

impl Phase {
    /// The match is over for this machine: its scene steps freely again, so
    /// its script can say so and move on.
    fn over(self) -> bool {
        matches!(self, Phase::Ended | Phase::Desync | Phase::Left)
    }
}

/// One seat's input for a step: the room sends whatever the clients sent,
/// or `0` for a seat played as "nothing held".
#[derive(Debug, Clone, Default, PartialEq)]
struct SeatInput {
    mask: u32,
    /// Thousandths, one per `analog` action.
    axes: Vec<i64>,
    /// Hundredths of a world unit, and the buttons (1 left, 2 right).
    pointer: (i64, i64, u32),
}

impl SeatInput {
    fn nothing(s: &StepSession) -> SeatInput {
        SeatInput { axes: vec![0; s.analog.len().min(MAX_ANALOG)], ..SeatInput::default() }
    }

    /// A plain number when there is nothing but the actions, so a session
    /// with neither `analog` nor `pointer` sends what it always sent.
    fn encode(&self, s: &StepSession) -> Value {
        if s.analog.is_empty() && !s.pointer {
            return json!(self.mask);
        }
        let mut v = vec![json!(self.mask)];
        v.extend(self.axes.iter().map(|a| json!(a)));
        if s.pointer {
            v.extend([json!(self.pointer.0), json!(self.pointer.1), json!(self.pointer.2)]);
        }
        Value::Array(v)
    }

    fn decode(v: &Value, s: &StepSession) -> SeatInput {
        let int = |v: Option<&Value>| v.and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))).unwrap_or(0);
        let analog = s.analog.len().min(MAX_ANALOG);
        match v.as_array() {
            Some(a) => SeatInput {
                mask: int(a.first()) as u32,
                axes: (0..analog).map(|k| int(a.get(1 + k)).clamp(0, 1000)).collect(),
                pointer: if s.pointer {
                    (int(a.get(1 + analog)), int(a.get(2 + analog)), int(a.get(3 + analog)) as u32 & 3)
                } else {
                    (0, 0, 0)
                },
            },
            None => SeatInput { mask: int(Some(v)) as u32, ..SeatInput::nothing(s) },
        }
    }
}

/// What a session knows that is not authored.
#[derive(Debug, Clone, Default)]
struct Run {
    phase: Phase,
    joined: bool,
    seat: Option<usize>,
    seats: usize,
    /// step -> every seat's input, as the room sent it.
    steps: BTreeMap<u64, Vec<Value>>,
    /// The last step every seat's input has come back for.
    through: u64,
    /// The last step run.
    ran: u64,
    /// The first step this seat has not queued an input for.
    send_next: u64,
    /// This player's inputs not sent yet, for the steps before `send_next`.
    pending: Vec<Value>,
    /// This player's inputs the room has not sent back in a step yet, by
    /// step - sent again if nothing comes back, since a lost one would hold
    /// every machine until the room calls the seat silent.
    unconfirmed: BTreeMap<u64, Value>,
    /// Seconds since this session last sent, and since a step last came back.
    since_send: f32,
    since_progress: f32,
    /// Seconds until the next reconnect, and whether the url is blanked for
    /// this frame to make the `Net` channel open a fresh connection.
    reconnect_in: f32,
    blanked: bool,
    hash: Option<(u64, String)>,
    /// The project's actions, sorted: bit `k` is `actions[k]`.
    actions: Vec<String>,
    /// Each seat's input last step, for `pressed` / `released`.
    last: Vec<u32>,
    away: Vec<bool>,
    silent: Vec<bool>,
    names: Vec<String>,
    why: String,
    /// A script asked to give the seat up ([`StepSession::leave`]).
    leave_asked: bool,
}

impl Run {
    fn seat_flags(&mut self, seats: usize) {
        let n = seats.max(self.away.len());
        self.away.resize(n, false);
        self.silent.resize(n, false);
        self.names.resize(n, String::new());
    }
}

thread_local! {
    /// The room and name a lobby chose for the next match scene.
    static NEXT_MATCH: std::cell::RefCell<Option<(String, String)>> = const { std::cell::RefCell::new(None) };
}

/// The room (and player name) a session with none of its own joins: what a
/// lobby scene sets before loading the match scene, whose script cannot run
/// until the match starts. Kept for every later session too, so loading the
/// match scene again rejoins the same room.
pub fn set_next_match(room: &str, name: &str) {
    NEXT_MATCH.with(|n| *n.borrow_mut() = Some((room.to_string(), name.to_string())));
}

impl StepSession {
    /// Give the seat up, now or whenever the match is: the room is told and
    /// the connection closed on the next frame, and the scene steps freely
    /// with the state `left` - how a match scene's script leaves before
    /// loading another (`Online.lockstep_leave`).
    pub fn leave(&mut self) {
        self.run.leave_asked = true;
    }
}

/// What [`set_next_match`] last set, if anything.
pub fn next_match() -> Option<(String, String)> {
    NEXT_MATCH.with(|n| n.borrow().clone())
}

/// Once a frame, before any step (`SystemPhase::Frame`): the network, the
/// gate and what has to go out.
pub fn frame(world: &mut World, dt: f32, input: Option<&InputState>) {
    let sessions: Vec<Entity> = world.query::<&StepSession>().iter().map(|(e, _)| e).collect();
    if sessions.is_empty() {
        return;
    }
    for e in sessions {
        if world.get::<&CustomComponents>(e).is_err() {
            let _ = world.insert_one(e, CustomComponents::default());
        }
        let mut q = world.query_one::<(&mut StepSession, &mut CustomComponents)>(e).unwrap();
        let Some((s, custom)) = q.get() else { continue };
        let leaving = input.is_some_and(|i| s.leave.iter().any(|a| i.is_action_just_pressed(a)));
        frame_one(s, custom, dt, leaving);
    }
}

/// More than this far behind the room, a machine is replaying the past.
fn catching_up(s: &StepSession) -> bool {
    matches!(s.run.phase, Phase::Running | Phase::Reconnecting)
        && s.run.through.saturating_sub(s.run.ran) > (s.delay as u64).max(8)
}

fn frame_one(s: &mut StepSession, custom: &mut CustomComponents, dt: f32, leaving: bool) {
    let asked = std::mem::take(&mut s.run.leave_asked);
    if (asked && !s.run.phase.over())
        || (leaving && matches!(s.run.phase, Phase::Connecting | Phase::Waiting | Phase::Reconnecting))
    {
        log::info!("[lockstep] leaving the match");
        if s.run.joined {
            push_outbox(custom, json!({ "t": "leave" }).to_string());
        }
        s.run.phase = Phase::Left;
        s.run.why = "left".into();
    }
    let state = custom
        .0
        .get(crate::net::COMPONENT)
        .and_then(|n| n.get(crate::net::STATE))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let connected = state == "connected";
    // A dropped connection: the others play on with this seat as "nothing
    // held", and this one tries again now and then, taking its seat back
    // from where it was. Joined, anything but "connected" is a drop - a
    // "connecting" too: a changed URL replaces the socket without one frame
    // of "closed", and the new socket has joined nothing. (On Windows a
    // refused connect is "connecting" for two seconds, so a URL put back
    // within that never saw "closed", and the seat sat in "running" for ever.)
    if s.run.joined && !connected && matches!(s.run.phase, Phase::Waiting | Phase::Running) {
        log::info!("[lockstep] the connection dropped ({state}); reconnecting");
        s.run.phase = Phase::Reconnecting;
        s.run.joined = false;
        s.run.reconnect_in = 0.0;
    }
    if s.run.phase == Phase::Reconnecting && !connected {
        s.run.reconnect_in -= dt;
        if s.run.reconnect_in <= 0.0 && state != "connecting" {
            s.run.reconnect_in = s.reconnect_every.max(0.1);
            s.run.blanked = true;
        }
    }
    let url = if std::mem::take(&mut s.run.blanked) || s.run.phase == Phase::Left { String::new() } else { s.url.clone() };
    set_net(custom, crate::net::URL, json!(url));
    if connected && !s.run.joined && !s.run.phase.over() {
        s.run.joined = true;
        if s.room.is_empty() || s.name.is_empty() {
            let (room, name) = NEXT_MATCH.with(|n| n.borrow().clone()).unwrap_or_default();
            if s.room.is_empty() {
                s.room = if room.is_empty() { "lockstep".into() } else { room };
            }
            if s.name.is_empty() {
                s.name = name;
            }
        }
        let mut join = json!({
            "t": "join", "kind": "lockstep", "room": s.room, "seats": s.seats, "name": s.name,
            "silent_after": s.silent_after,
        });
        if s.spectate {
            join["watch"] = json!(true);
        }
        push_outbox(custom, join.to_string());
    }
    if !connected && s.run.phase == Phase::Connecting {
        s.run.joined = false;
    }
    let before = s.run.through;
    for text in take_inbox(custom) {
        let Ok(msg) = serde_json::from_str::<Value>(&text) else { continue };
        receive(s, &msg);
    }
    s.run.since_send += dt;
    s.run.since_progress = if s.run.through > before { 0.0 } else { s.run.since_progress + dt };
    let through = s.run.through;
    s.run.unconfirmed.retain(|step, _| *step > through);
    let playing = matches!(s.run.phase, Phase::Running | Phase::Reconnecting);
    // Run no step past the last one every player's input is in for, and
    // none before the match starts. After it the scene steps freely again,
    // so its script can show how it ended and move on.
    set_gate(if s.run.phase.over() {
        None
    } else {
        Some(if playing { s.run.through } else { s.run.ran })
    });
    // Behind the room (a late join): replay fast, and owe the room "nothing
    // held" for the steps it is playing now rather than make it wait while
    // this machine is still in the past.
    let behind = catching_up(s);
    set_catch_up(if behind { s.catch_up.max(1) } else { 0 });
    if s.run.phase == Phase::Running && s.run.seat.is_some() {
        // Held by the gate with nothing owed in flight - the match's first
        // steps, or level again after a late join: "nothing held" for the
        // steps no press can reach in time, which is also how a seat the
        // room plays for (away, silent) says it is back.
        if !behind && s.run.ran >= s.run.through {
            owe_through(s, s.run.through + s.delay as u64);
        }
        // New inputs go out at most every `SEND_EVERY` - at once if the
        // room is waiting on them - and if nothing has come back for a while,
        // everything not yet confirmed goes again (the room keeps the first
        // copy of each).
        let waited_on = s.run.through <= s.run.ran;
        let resend = s.run.since_progress >= RESEND_AFTER && !s.run.unconfirmed.is_empty();
        let send = (!s.run.pending.is_empty() || s.run.hash.is_some()) && (s.run.since_send >= SEND_EVERY || waited_on);
        if send || resend {
            let from = s.run.send_next - s.run.pending.len() as u64;
            for (k, input) in s.run.pending.drain(..).enumerate() {
                s.run.unconfirmed.insert(from + k as u64, input);
            }
            // In runs the room can take: consecutive steps, a batch at a time.
            let mut batch: Vec<(u64, Value)> = Vec::new();
            for (step, input) in &s.run.unconfirmed {
                if batch.last().is_some_and(|(last, _)| *last + 1 != *step) || batch.len() == MAX_BATCH {
                    send_inputs(custom, &mut batch, None);
                }
                batch.push((*step, input.clone()));
            }
            let hash = s.run.hash.take();
            if !batch.is_empty() || hash.is_some() {
                send_inputs(custom, &mut batch, hash);
            }
            s.run.since_send = 0.0;
            if resend {
                s.run.since_progress = 0.0;
            }
        }
    }
    publish_state(s, custom);
}

/// What the room already played without this seat is not owed: inputs for
/// those steps, sent or not, are moot.
fn skip_played(s: &mut StepSession) {
    if s.run.send_next <= s.run.through {
        s.run.pending.clear();
        s.run.send_next = s.run.through + 1;
    }
}

/// "Nothing held" for every step up to `last` this seat has not queued.
fn owe_through(s: &mut StepSession, last: u64) {
    skip_played(s);
    if s.run.send_next <= last {
        let nothing = SeatInput::nothing(s).encode(s);
        while s.run.send_next <= last {
            s.run.pending.push(nothing.clone());
            s.run.send_next += 1;
        }
    }
}

fn send_inputs(custom: &mut CustomComponents, batch: &mut Vec<(u64, Value)>, hash: Option<(u64, String)>) {
    let from = batch.first().map(|(s, _)| *s).unwrap_or(1);
    let inputs: Vec<Value> = batch.drain(..).map(|(_, v)| v).collect();
    let mut body = json!({ "t": "in", "s": from, "i": inputs });
    if let Some((step, h)) = hash {
        body["hash"] = json!({ "s": step, "h": h });
    }
    push_outbox(custom, json!({ "t": "send", "body": body }).to_string());
}

fn seat_of(msg: &Value) -> Option<usize> {
    msg.get("seat").and_then(Value::as_u64).map(|v| v as usize)
}

fn receive(s: &mut StepSession, msg: &Value) {
    match msg.get("t").and_then(Value::as_str) {
        Some("joined") => {
            s.run.seat = seat_of(msg);
            s.run.seats = msg.get("seats").and_then(Value::as_u64).unwrap_or(s.seats as u64) as usize;
            let seats = s.run.seats;
            s.run.seat_flags(seats);
            peers(s, msg);
            let started = msg.get("started").and_then(Value::as_bool) == Some(true);
            if !started {
                s.run.phase = Phase::Waiting;
                return;
            }
            // Every step so far: the ones this machine has not run are
            // replayed, and its own inputs are owed from the room's next step.
            let history = msg.get("history").and_then(Value::as_array).cloned().unwrap_or_default();
            for (k, step) in history.iter().enumerate() {
                let n = 1 + k as u64;
                if n > s.run.ran {
                    s.run.steps.insert(n, step.as_array().cloned().unwrap_or_default());
                }
            }
            s.run.through = s.run.through.max(history.len() as u64);
            // Back after a drop: what this seat had queued went with the old
            // connection, and the room played it as "nothing held".
            if s.run.phase == Phase::Reconnecting {
                s.run.phase = Phase::Waiting;
            }
            start(s, history.len() as u64 + 1);
        }
        Some("start") => {
            peers(s, msg);
            start(s, 1);
        }
        Some("steps") => {
            let from = msg.get("s").and_then(Value::as_u64).unwrap_or(0);
            if let Some(steps) = msg.get("i").and_then(Value::as_array) {
                for (k, step) in steps.iter().enumerate() {
                    let n = from + k as u64;
                    if n > s.run.ran {
                        s.run.steps.insert(n, step.as_array().cloned().unwrap_or_default());
                    }
                    s.run.through = s.run.through.max(n);
                }
            }
        }
        Some("peer_joined") => {
            if let Some(k) = seat_of(msg) {
                s.run.seat_flags(k + 1);
                s.run.away[k] = false;
                s.run.silent[k] = msg.get("silent") == Some(&json!(true));
                s.run.names[k] = msg.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            }
        }
        Some("peer_left") => {
            if let Some(k) = seat_of(msg) {
                s.run.seat_flags(k + 1);
                s.run.away[k] = true;
            }
        }
        Some(t @ ("silent" | "back")) => {
            if let Some(k) = seat_of(msg) {
                s.run.seat_flags(k + 1);
                s.run.silent[k] = t == "silent";
            }
        }
        Some("desync") => {
            s.run.phase = Phase::Desync;
            s.run.why = format!("the games came apart by step {}", msg.get("s").and_then(Value::as_u64).unwrap_or(0));
            log::warn!("[lockstep] {}", s.run.why);
        }
        Some("ended") => {
            s.run.phase = Phase::Ended;
            s.run.why = msg.get("why").and_then(Value::as_str).unwrap_or("ended").to_string();
        }
        Some("error") => {
            let why = msg.get("why").and_then(Value::as_str).unwrap_or("?").to_string();
            log::warn!("[lockstep] the room says: {why}");
            // Refused (the match is full, or too far along): nothing will
            // ever start, so say so rather than wait.
            if matches!(s.run.phase, Phase::Connecting | Phase::Reconnecting) {
                s.run.phase = Phase::Ended;
                s.run.why = why;
            }
        }
        _ => {}
    }
}

fn peers(s: &mut StepSession, msg: &Value) {
    for p in msg.get("peers").and_then(Value::as_array).into_iter().flatten() {
        if let Some(k) = seat_of(p) {
            s.run.seat_flags(k + 1);
            s.run.names[k] = p.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            s.run.away[k] = p.get("here") == Some(&json!(false));
            s.run.silent[k] = p.get("silent") == Some(&json!(true));
        }
    }
}

/// The match is on, and this seat owes the room its inputs from step
/// `first`. Steps `first .. first + delay` go as "nothing held" - no press
/// can reach them in time - once this machine has caught up with the room
/// (`frame_one`); its own first input goes with the step after.
fn start(s: &mut StepSession, first: u64) {
    if s.run.last.len() < s.run.seats.max(1) {
        s.run.last.resize(s.run.seats.max(1), 0);
    }
    if s.run.phase == Phase::Running {
        return;
    }
    s.run.phase = Phase::Running;
    s.run.pending.clear();
    s.run.unconfirmed.clear();
    s.run.send_next = first;
}

fn publish_state(s: &StepSession, custom: &mut CustomComponents) {
    let slot = custom.0.entry(COMPONENT.to_string()).or_insert_with(|| json!({}));
    if !slot.is_object() {
        *slot = json!({});
    }
    let state = match s.run.phase {
        Phase::Connecting => "connecting",
        Phase::Waiting => "waiting",
        Phase::Running if catching_up(s) => "catching_up",
        Phase::Running => "running",
        Phase::Reconnecting => "reconnecting",
        Phase::Ended => "ended",
        Phase::Desync => "desync",
        Phase::Left => "left",
    };
    let o = slot.as_object_mut().unwrap();
    o.insert("state".into(), json!(state));
    o.insert("seat".into(), json!(s.run.seat));
    o.insert("seats".into(), json!(s.run.seats));
    o.insert("step".into(), json!(s.run.ran));
    o.insert("behind".into(), json!(s.run.through.saturating_sub(s.run.ran)));
    o.insert("spectating".into(), json!(s.spectate));
    o.insert("away".into(), json!(s.run.away));
    o.insert("silent".into(), json!(s.run.silent));
    o.insert("names".into(), json!(s.run.names));
    let waiting = s.run.phase == Phase::Running && s.run.through <= s.run.ran;
    o.insert("waiting_on".into(), json!(if waiting { 1 } else { 0 }));
    if !s.run.why.is_empty() {
        o.insert("why".into(), json!(s.run.why));
    }
    // Nobody holds anything once the match is over.
    if s.run.phase.over() && o.get("held").and_then(Value::as_array).is_some_and(|h| h.iter().any(|a| a != &json!([]))) {
        let none = Value::Array(vec![json!([]); s.run.seats]);
        for k in ["held", "pressed", "released"] {
            o.insert(k.into(), none.clone());
        }
    }
}

/// At the start of each step (`SystemPhase::PreUpdate`, first): every
/// player's input for this step on the `Lockstep` component, this player's
/// for the step `delay` ahead queued, and now and then the world's hash.
pub fn step(world: &mut World, input: Option<&InputState>) {
    let sessions: Vec<Entity> = world.query::<&StepSession>().iter().map(|(e, _)| e).collect();
    for e in sessions {
        let (hash_now, wants_pointer) = {
            let s = world.get::<&StepSession>(e).unwrap();
            let next = s.run.ran + 1;
            (s.hash_every > 0 && next % s.hash_every as u64 == 0 && s.run.steps.contains_key(&next), s.pointer)
        };
        let hash = hash_now.then(|| format!("{:016x}", world_hash(world)));
        let pointer = match input {
            Some(i) if wants_pointer => {
                let p = mouse_world_position(world, i.mouse_position, i.screen_size);
                Some((p.x, p.y, i.mouse_left as u32 | (i.mouse_right as u32) << 1))
            }
            _ => None,
        };
        let mut q = world.query_one::<(&mut StepSession, &mut CustomComponents)>(e).unwrap();
        let Some((s, custom)) = q.get() else { continue };
        step_one(s, custom, input, pointer, hash);
    }
}

fn step_one(
    s: &mut StepSession,
    custom: &mut CustomComponents,
    input: Option<&InputState>,
    pointer: Option<(f32, f32, u32)>,
    hash: Option<String>,
) {
    let n = s.run.ran + 1;
    let Some(inputs) = s.run.steps.remove(&n) else {
        // A step ran that the room had not sent: the clock is not gated -
        // fixed steps are off (`settings.simulation_hz`, or the scene's).
        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if s.run.phase == Phase::Running && !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log::warn!("[lockstep] a step ran without every player's input: a lockstep game needs fixed steps (`simulation_hz`)");
        }
        return;
    };
    s.run.ran = n;
    if !s.actions.is_empty() && s.run.actions.is_empty() {
        s.run.actions = s.actions.clone();
        s.run.actions.truncate(MAX_ACTIONS);
    }
    if let Some(input) = input
        && s.run.actions.is_empty()
    {
        s.run.actions = input.action_names();
        if s.run.actions.len() > MAX_ACTIONS {
            log::warn!("[lockstep] only the first {MAX_ACTIONS} of {} input actions are sent", s.run.actions.len());
            s.run.actions.truncate(MAX_ACTIONS);
        }
        if s.analog.len() > MAX_ANALOG {
            log::warn!("[lockstep] only the first {MAX_ANALOG} of {} analog actions are sent", s.analog.len());
        }
    }
    let inputs: Vec<SeatInput> = inputs.iter().map(|v| SeatInput::decode(v, s)).collect();
    let names = |mask: u32| -> Vec<Value> {
        s.run.actions.iter().enumerate().filter(|(k, _)| mask & (1 << k) != 0).map(|(_, a)| json!(a)).collect()
    };
    let masks: Vec<u32> = inputs.iter().map(|i| i.mask).collect();
    let last = std::mem::replace(&mut s.run.last, masks.clone());
    let held: Vec<Value> = masks.iter().map(|m| Value::Array(names(*m))).collect();
    let pressed: Vec<Value> =
        masks.iter().enumerate().map(|(k, m)| Value::Array(names(m & !last.get(k).copied().unwrap_or(0)))).collect();
    let released: Vec<Value> =
        masks.iter().enumerate().map(|(k, m)| Value::Array(names(!m & last.get(k).copied().unwrap_or(0)))).collect();
    let slot = custom.0.entry(COMPONENT.to_string()).or_insert_with(|| json!({}));
    if !slot.is_object() {
        *slot = json!({});
    }
    let o = slot.as_object_mut().unwrap();
    o.insert("held".into(), Value::Array(held));
    o.insert("pressed".into(), Value::Array(pressed));
    o.insert("released".into(), Value::Array(released));
    if !s.analog.is_empty() {
        let axes: Vec<Value> = inputs
            .iter()
            .map(|i| {
                let m: serde_json::Map<String, Value> =
                    s.analog.iter().zip(&i.axes).map(|(a, v)| (a.clone(), json!(*v as f64 / 1000.0))).collect();
                Value::Object(m)
            })
            .collect();
        o.insert("axes".into(), Value::Array(axes));
    }
    if s.pointer {
        let pointers: Vec<Value> = inputs
            .iter()
            .map(|i| {
                let (x, y, b) = i.pointer;
                json!({ "x": x as f64 / 100.0, "y": y as f64 / 100.0, "left": b & 1 != 0, "right": b & 2 != 0 })
            })
            .collect();
        o.insert("pointer".into(), Value::Array(pointers));
    }
    o.insert("step".into(), json!(n));
    // This player's input now, for the step `delay` ahead - unless this
    // machine is replaying the past, when the room has had "nothing held"
    // for those steps already.
    // Level with the room, this seat speaks: for the step `delay` ahead, or -
    // when that one is played already, which is how it trails a room that
    // played it as "nothing held" while it was away - the first step the room
    // has not played, so its player is waited for again. Still replaying the
    // past, it says nothing: its inputs there are moot.
    let speaking = s.run.seat.is_some() && s.run.phase == Phase::Running && !catching_up(s);
    let target = (n + s.delay as u64).max(s.run.through + 1);
    if speaking {
        owe_through(s, target - 1);
    }
    if speaking && s.run.send_next == target {
        let mut mine = SeatInput::nothing(s);
        if let Some(i) = input {
            mine.mask =
                s.run.actions.iter().enumerate().filter(|(_, a)| i.is_action_pressed(a)).fold(0u32, |m, (k, _)| m | 1 << k);
            mine.axes = s
                .analog
                .iter()
                .take(MAX_ANALOG)
                .map(|a| (i.action_axis_value(a).clamp(0.0, 1.0) * 1000.0).round() as i64)
                .collect();
        }
        if let Some((x, y, b)) = pointer {
            mine.pointer = ((x as f64 * 100.0).round() as i64, (y as f64 * 100.0).round() as i64, b);
        }
        s.run.pending.push(mine.encode(s));
        s.run.send_next += 1;
    }
    if let Some(h) = hash
        && s.run.seat.is_some()
    {
        s.run.hash = Some((n, h));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_session_sends_a_plain_number() {
        let s = StepSession::default();
        let i = SeatInput { mask: 5, ..SeatInput::default() };
        assert_eq!(i.encode(&s), json!(5));
        assert_eq!(SeatInput::decode(&json!(5), &s), SeatInput { mask: 5, ..SeatInput::default() });
    }

    #[test]
    fn analog_and_pointer_ride_beside_the_actions() {
        let s = StepSession { analog: vec!["throttle".into(), "steer".into()], pointer: true, ..StepSession::default() };
        let i = SeatInput { mask: 3, axes: vec![250, 1000], pointer: (-1050, 300, 1) };
        let v = i.encode(&s);
        assert_eq!(v, json!([3, 250, 1000, -1050, 300, 1]));
        assert_eq!(SeatInput::decode(&v, &s), i);
        // A seat played as "nothing held" is a plain 0 from the room.
        assert_eq!(SeatInput::decode(&json!(0), &s), SeatInput { mask: 0, axes: vec![0, 0], pointer: (0, 0, 0) });
        // Lua's numbers are floats on the wire.
        assert_eq!(SeatInput::decode(&json!([1.0, 500.0, 0.0, 2.0, 3.0, 2.0]), &s).pointer, (2, 3, 2));
    }
}
