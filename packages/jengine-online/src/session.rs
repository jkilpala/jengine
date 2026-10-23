//! One player's side of a `jengine-server` connection - pure, no sockets and
//! no world, so the whole protocol is unit tests.
//!
//! It is fed the frames the server sends and keeps what they add up to: who
//! we are, which room we are in, and **the entities in view** - `sync` frames
//! applied in order, so what is here is what `jengine_server::sync::Space`
//! says this player can see.
//!
//! # Between two ticks
//!
//! The server sends positions at its tick rate - ten a second, typically -
//! and a screen draws sixty or more. Drawn as they arrive, things move in
//! steps. So each entity is *walked* from where it is shown to where the
//! server last put it, over one tick's length, which the session measures
//! from the frames themselves rather than being told. The cost is honest:
//! everything is shown up to one tick late.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Status {
    /// No connection, or not yet signed in on it.
    #[default]
    Offline,
    /// Connected and waiting for `welcome`.
    SigningIn,
    /// Signed in, in no room.
    SignedIn,
    /// Asked to join and waiting for `joined`.
    Joining,
    Joined,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Offline => "offline",
            Status::SigningIn => "signing_in",
            Status::SignedIn => "signed_in",
            Status::Joining => "joining",
            Status::Joined => "joined",
        }
    }
}

/// Something the server has told us is in view.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetEntity {
    /// Its `kind` property, which picks the slot that shows it.
    pub kind: String,
    /// Everything the server said about it but its id and position.
    pub props: Map<String, Value>,
    /// Where it is shown now.
    pub x: f64,
    pub y: f64,
    from: (f64, f64),
    to: (f64, f64),
    /// 0 at `from`, 1 at `to`.
    t: f64,
    /// Its facing in radians, if the room gives it one
    /// (`Space::set_rotation`) - walked like the position, the short way
    /// round.
    pub rot: Option<f64>,
    rot_from: f64,
    rot_to: f64,
}

impl NetEntity {
    /// Where the server last said it is - not where it is being drawn.
    pub fn server(&self) -> (f64, f64) {
        self.to
    }

    /// A new facing to walk to, from where it is shown.
    fn turn_to(&mut self, rot: f64) {
        self.rot_from = self.rot.unwrap_or(rot);
        self.rot_to = rot;
        if self.rot.is_none() {
            self.rot = Some(rot);
        }
    }
}

/// From `a` to `b` by `t`, the short way round.
fn lerp_angle(a: f64, b: f64, t: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let d = (b - a).rem_euclid(tau);
    let d = if d > std::f64::consts::PI { d - tau } else { d };
    a + d * t
}

#[derive(Debug, Default)]
pub struct Session {
    pub status: Status,
    pub player: String,
    pub name: String,
    /// Kept and sent with the next `hello`, which is what makes a reconnect
    /// the same player.
    pub token: String,
    /// The player's saved state, as of signing in.
    pub state: Value,
    /// (kind, id) of the room we are in.
    pub room: Option<(String, String)>,
    /// The `joined` frame, for whatever the room put on it.
    pub joined: Value,
    /// The entity that is us, if the room said (`"you"` on `joined`, or a
    /// `you` frame mid-room when our avatar is replaced).
    pub you: Option<u64>,
    /// The options of the join that put us in this room - sent again when a
    /// dropped connection goes back to it, so a room reading a code or a
    /// seat count from them knows its own player. `Null` for a room we were
    /// transferred into, which we never asked to join.
    pub options: Value,
    /// The options of a join sent and not yet answered.
    asking: Value,
    pub error: String,
    pub entities: BTreeMap<u64, NetEntity>,
    /// Ids that came into view / went out of it since [`Session::take_changes`].
    entered: Vec<u64>,
    left: Vec<u64>,
    /// Ids that left with a room (a transfer, a leave). The next room
    /// numbers its entities afresh, so one of them may come back in the same
    /// frame as somebody else entirely - still `left`, or a slot bound to the
    /// old one would show the new one, whatever its kind.
    left_with_room: Vec<u64>,
    /// Seconds between the server's `sync` frames, measured.
    interval: f64,
    since_sync: f64,
    /// The server's tick, as of its last frame - what we are looking at.
    pub tick: u64,
    /// An acknowledgement not yet acted on: the last of our inputs the
    /// server applied, and where that left our entity.
    acked: Option<(u32, (f64, f64))>,
    /// How this room moves a steered avatar, if it said: the length of a
    /// step, and the model - with its walls, if it has them. What a
    /// prediction is made with.
    pub netcode: Option<(f64, jengine_netcode::BuiltIn)>,
    /// Counts the rooms we have been put in, so what belongs to one room is
    /// not carried into the next.
    pub joins: u32,
    /// Counts the times the room changed how we move (on `joined`, or a
    /// `netcode` frame mid-room), so a running prediction takes the new rule.
    pub netcode_rev: u32,
    /// Keep every `sync` frame as it arrived, in [`Session::syncs`] - for a
    /// test that must prove what a client was *sent* (Prop Hunt's seeker),
    /// since a `sync` never reaches the events a script reads. Off in a game.
    pub keep_syncs: bool,
    pub syncs: Vec<Value>,
}

/// What a tick's worth of frames changed about which entities exist.
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    pub entered: Vec<u64>,
    pub left: Vec<u64>,
}

impl Session {
    pub fn new() -> Self {
        Session { interval: 0.1, ..Default::default() }
    }

    pub fn hello_frame(&self, name: &str) -> String {
        // `pings`: we answer them, so the server can measure our round trip
        // and tell a room how long ago each message was sent.
        json!({ "t": "hello", "name": name, "token": self.token, "pings": true }).to_string()
    }

    /// Ask to join: the frame to send, with its options remembered for
    /// the room it puts us in.
    pub fn join(&mut self, kind: &str, room: &str, options: &Value) -> String {
        self.asking = options.clone();
        self.status = Status::Joining;
        Self::join_frame(kind, room, options)
    }

    pub fn join_frame(kind: &str, room: &str, options: &Value) -> String {
        let mut frame = json!({ "t": "join", "kind": kind });
        if !room.is_empty() {
            frame["room"] = json!(room);
        }
        if !options.is_null() {
            frame["options"] = options.clone();
        }
        frame.to_string()
    }

    pub fn send_frame(body: &Value) -> String {
        json!({ "t": "send", "body": body }).to_string()
    }

    pub fn leave_frame() -> String {
        json!({ "t": "leave" }).to_string()
    }

    /// The connection is gone: nothing we knew about the room is true any
    /// more. Who we are (the token) is kept.
    pub fn dropped(&mut self) {
        self.status = Status::Offline;
        self.clear_room();
    }

    fn clear_room(&mut self) {
        self.left.extend(self.entities.keys());
        self.left_with_room.extend(self.entities.keys());
        self.entities.clear();
        self.room = None;
        self.you = None;
        self.options = Value::Null;
        self.joined = Value::Null;
    }

    /// Handle one frame from the server. Returns it as an event for the
    /// game, unless it is a `sync`: that is the package's to apply, not a
    /// script's to parse sixty times a second.
    pub fn receive(&mut self, text: &str) -> Option<Value> {
        let frame: Value = serde_json::from_str(text).ok()?;
        match frame["t"].as_str() {
            Some("sync") => {
                self.sync(&frame);
                if self.keep_syncs {
                    self.syncs.push(frame);
                }
                return None;
            }
            Some("welcome") => {
                self.player = frame["player"].as_str().unwrap_or("").to_string();
                self.name = frame["name"].as_str().unwrap_or("").to_string();
                if let Some(token) = frame["token"].as_str() {
                    self.token = token.to_string();
                }
                self.state = frame["state"].clone();
                self.status = Status::SignedIn;
                self.error.clear();
            }
            Some("joined") => {
                // The answer to our join carries its options; a transfer
                // (nothing but this frame, while we were not joining) has none.
                let options = if self.status == Status::Joining {
                    std::mem::take(&mut self.asking)
                } else {
                    Value::Null
                };
                // A new room is a new world - also after a transfer, which
                // arrives as nothing but this frame.
                self.clear_room();
                self.options = options;
                self.room = Some((
                    frame["kind"].as_str().unwrap_or("").to_string(),
                    frame["room"].as_str().unwrap_or("").to_string(),
                ));
                self.you = frame["you"].as_u64();
                self.joins += 1;
                self.acked = None;
                // The room's own numbers, so the prediction is the room's rule
                // and not a copy of it that could be out of date.
                self.netcode = parse_netcode(&frame["netcode"]);
                self.netcode_rev += 1;
                self.joined = frame.clone();
                self.status = Status::Joined;
                self.error.clear();
            }
            // A new session after a password change or a sign-out: the old
            // token is dead, and this is who we are now.
            Some("account") => {
                if let Some(token) = frame["token"].as_str() {
                    self.token = token.to_string();
                }
            }
            // The room changed how we move while we are in it - a wall built
            // or broken. Same shape as `netcode` on `joined`.
            Some("netcode") => {
                self.netcode = parse_netcode(&frame["netcode"]);
                self.netcode_rev += 1;
            }
            // Our avatar is another entity now (a respawn as something else).
            Some("you") => {
                self.you = frame["you"].as_u64();
                self.acked = None;
            }
            // An operator changed what the server remembers of us - the
            // server's own `state`, marked `sys`. A room's frame of the same
            // name is the room's, and only an event (a MUD met the clash: its board read as our saved state after a drop).
            Some("state") if frame["sys"] == true => self.state = frame["state"].clone(),
            Some("left" | "kicked" | "closed") => {
                self.clear_room();
                self.status = Status::SignedIn;
            }
            Some("error") => {
                self.error = frame["why"].as_str().unwrap_or("").to_string();
                // A refused join leaves us where we were: in no room.
                if self.status == Status::Joining {
                    self.status = Status::SignedIn;
                    self.asking = Value::Null;
                }
                if self.status == Status::SigningIn {
                    // Only a token the server says it has never seen is
                    // thrown away - then the next hello makes a new player
                    // instead of failing the same way. Any other refusal
                    // (the server cannot reach its store, say) is not about
                    // who we are, and the token is all that we are.
                    if self.error == "unknown token" {
                        self.token.clear();
                    }
                    self.status = Status::Offline;
                }
            }
            _ => {}
        }
        Some(frame)
    }

    fn sync(&mut self, frame: &Value) {
        // A moving average of how far apart these arrive: the length of the
        // walk between two positions.
        if self.since_sync > 0.0 {
            self.interval = (self.interval * 0.8 + self.since_sync * 0.2).clamp(0.02, 1.0);
        }
        self.since_sync = 0.0;

        for id in frame["leave"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
            if self.entities.remove(&id).is_some() {
                self.left.push(id);
            }
        }
        for e in frame["enter"].as_array().into_iter().flatten() {
            let Some(id) = e["id"].as_u64() else { continue };
            let (x, y) = (e["x"].as_f64().unwrap_or(0.0), e["y"].as_f64().unwrap_or(0.0));
            let mut entity = NetEntity { x, y, from: (x, y), to: (x, y), t: 1.0, ..Default::default() };
            if let Some(rot) = e["rot"].as_f64() {
                entity.turn_to(rot);
            }
            apply_props(&mut entity, e);
            if self.entities.insert(id, entity).is_none() {
                self.entered.push(id);
            }
        }
        // Movement alone is `[id, x, y]`: most of what a busy zone says.
        for m in frame["move"].as_array().into_iter().flatten() {
            let Some(entity) = m[0].as_u64().and_then(|id| self.entities.get_mut(&id)) else { continue };
            entity.from = (entity.x, entity.y);
            entity.to = (m[1].as_f64().unwrap_or(entity.to.0), m[2].as_f64().unwrap_or(entity.to.1));
            if let Some(rot) = m[3].as_f64() {
                entity.turn_to(rot);
            }
            entity.t = 0.0;
        }
        for e in frame["update"].as_array().into_iter().flatten() {
            let Some(entity) = e["id"].as_u64().and_then(|id| self.entities.get_mut(&id)) else { continue };
            if e.get("x").is_some() || e.get("y").is_some() {
                entity.from = (entity.x, entity.y);
                entity.to = (e["x"].as_f64().unwrap_or(entity.to.0), e["y"].as_f64().unwrap_or(entity.to.1));
                if let Some(rot) = e["rot"].as_f64() {
                    entity.turn_to(rot);
                }
                entity.t = 0.0;
            }
            apply_props(entity, e);
        }
        if let Some(tick) = frame["tick"].as_u64() {
            self.tick = tick;
        }
        // Read after the moves above: the position in this frame is the one
        // the acknowledged input left us at.
        if let (Some(ack), Some(own)) =
            (frame["ack"].as_u64(), self.you.and_then(|you| self.entities.get(&you)))
        {
            self.acked = Some((ack as u32, own.to));
        }
    }

    /// The server's latest word on our own entity, once: the input it had
    /// reached, and the position that gave.
    pub fn take_ack(&mut self) -> Option<(u32, (f64, f64))> {
        self.acked.take()
    }

    /// Move every entity along its walk. `smoothing` is how long a walk
    /// takes; zero means "one server tick, as measured".
    pub fn advance(&mut self, dt: f64, smoothing: f64) {
        self.since_sync += dt;
        let length = if smoothing > 0.0 { smoothing } else { self.interval };
        for entity in self.entities.values_mut() {
            if entity.t < 1.0 {
                entity.t = (entity.t + dt / length).min(1.0);
                entity.x = entity.from.0 + (entity.to.0 - entity.from.0) * entity.t;
                entity.y = entity.from.1 + (entity.to.1 - entity.from.1) * entity.t;
                if entity.rot.is_some() {
                    entity.rot = Some(lerp_angle(entity.rot_from, entity.rot_to, entity.t));
                }
            }
        }
    }

    /// Which entities appeared and disappeared since the last call. One that
    /// did both in between is in neither list.
    pub fn take_changes(&mut self) -> Changes {
        let mut entered = std::mem::take(&mut self.entered);
        let mut left = std::mem::take(&mut self.left);
        // Came and went unseen: nobody was ever shown it, so nobody is told.
        let unseen: Vec<u64> = entered.iter().copied().filter(|id| !self.entities.contains_key(id)).collect();
        entered.retain(|id| self.entities.contains_key(id));
        let with_room = std::mem::take(&mut self.left_with_room);
        left.retain(|id| with_room.contains(id) || (!self.entities.contains_key(id) && !unseen.contains(id)));
        left.sort_unstable();
        left.dedup();
        Changes { entered, left }
    }
}

/// How a room says we move: `netcode` on `joined`, or in a `netcode` frame.
/// Anything malformed is no model at all - better than a wrong one.
fn parse_netcode(n: &Value) -> Option<(f64, jengine_netcode::BuiltIn)> {
    let pair = |v: &Value| Some((v[0].as_f64()?, v[1].as_f64()?));
    let field = jengine_netcode::TopDown { speed: n["speed"].as_f64()?, min: pair(&n["min"])?, max: pair(&n["max"])? };
    let step = n["step"].as_f64().filter(|s| *s > 0.0)?;
    // Walls make it the tile model: the same cells the server moves us and
    // its monsters through.
    let w = &n["walls"];
    if w.is_object() {
        let walls = jengine_netcode::Walls::unpack(
            w["cols"].as_u64()? as usize,
            w["rows"].as_u64()? as usize,
            w["cell"].as_f64()?,
            pair(&w["origin"])?,
            w["solid"].as_str()?,
        )?;
        let tiles = jengine_netcode::TileMotion {
            speed: field.speed,
            radius: n["radius"].as_f64().unwrap_or(0.0),
            min: field.min,
            max: field.max,
            walls,
        };
        return Some((step, jengine_netcode::BuiltIn::Tiles(tiles)));
    }
    Some((step, jengine_netcode::BuiltIn::TopDown(field)))
}

fn apply_props(entity: &mut NetEntity, frame: &Value) {
    for (key, value) in frame.as_object().into_iter().flatten() {
        match key.as_str() {
            "id" | "x" | "y" | "rot" => {}
            "kind" => {
                entity.kind = value.as_str().unwrap_or("").to_string();
                entity.props.insert(key.clone(), value.clone());
            }
            _ => {
                entity.props.insert(key.clone(), value.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined() -> Session {
        let mut s = Session::new();
        s.receive(r#"{"t":"welcome","player":"p_1","token":"tok","name":"Ada","state":{"coins":2}}"#);
        s.receive(r#"{"t":"joined","kind":"zone","room":"meadow","you":4}"#);
        s
    }

    #[test]
    fn signing_in_keeps_the_token_and_the_state() {
        let mut s = Session::new();
        assert!(s.hello_frame("Ada").contains(r#""token":"""#), "a first hello has no token");
        let event = s.receive(r#"{"t":"welcome","player":"p_1","token":"tok","name":"Ada","state":{"coins":2}}"#);
        assert_eq!(event.unwrap()["t"], "welcome", "a script hears it too");
        assert_eq!((s.status, s.player.as_str(), &s.state["coins"]), (Status::SignedIn, "p_1", &json!(2)));
        assert!(s.hello_frame("").contains(r#""token":"tok""#), "and the next hello is the same player");
    }

    #[test]
    fn a_session_keeps_its_syncs_only_when_asked() {
        let mut s = Session::new();
        s.receive(r#"{"t":"sync","enter":[{"id":4,"x":0,"y":0}]}"#);
        assert!(s.syncs.is_empty());
        s.keep_syncs = true;
        s.receive(r#"{"t":"sync","move":[[4,5,0]]}"#);
        assert_eq!(s.syncs.len(), 1);
        assert_eq!(s.syncs[0]["move"][0][1], 5);
    }

    #[test]
    fn sync_frames_add_up_to_what_is_in_view() {
        let mut s = joined();
        assert_eq!(s.you, Some(4));
        assert!(s.receive(r#"{"t":"sync","enter":[{"id":4,"x":10,"y":20,"kind":"player","name":"Ada"},{"id":1,"x":0,"y":0,"kind":"coin"}]}"#).is_none());
        assert_eq!(s.take_changes(), Changes { entered: vec![4, 1], left: vec![] });
        assert_eq!(s.entities[&4].kind, "player");
        assert_eq!(s.entities[&4].props["name"], "Ada");

        s.receive(r#"{"t":"sync","update":[{"id":4,"hp":7}],"leave":[1]}"#);
        assert_eq!(s.entities[&4].props["hp"], 7);
        assert_eq!((s.entities[&4].x, s.entities[&4].y), (10.0, 20.0), "a prop change moves nothing");
        assert_eq!(s.take_changes(), Changes { entered: vec![], left: vec![1] });
    }

    /// A position arrives ten times a second and is drawn sixty: it is
    /// walked to, not jumped to - and arrives exactly.
    #[test]
    fn a_move_is_walked_over_one_tick() {
        let mut s = joined();
        s.receive(r#"{"t":"sync","enter":[{"id":4,"x":0,"y":0}]}"#);
        s.advance(0.1, 0.0);
        s.receive(r#"{"t":"sync","move":[[4,30,0]]}"#);
        assert_eq!(s.entities[&4].x, 0.0, "not yet");
        s.advance(0.05, 0.0);
        assert!((s.entities[&4].x - 15.0).abs() < 0.01, "half a tick, half way: {}", s.entities[&4].x);
        s.advance(0.05, 0.0);
        s.advance(0.05, 0.0);
        assert_eq!(s.entities[&4].x, 30.0, "and it stops where the server said");

        // A new target mid-walk starts from where it is shown, so it never snaps back.
        s.receive(r#"{"t":"sync","update":[{"id":4,"x":60,"y":0}]}"#);
        s.advance(0.05, 0.0);
        s.receive(r#"{"t":"sync","update":[{"id":4,"x":0,"y":0}]}"#);
        let turned_at = s.entities[&4].x;
        s.advance(0.01, 0.0);
        assert!(s.entities[&4].x < turned_at && s.entities[&4].x > turned_at - 10.0);
    }

    #[test]
    fn the_rooms_numbers_and_the_servers_word_are_kept_for_a_prediction() {
        let mut s = Session::new();
        s.receive(r#"{"t":"welcome","player":"p_1","token":"tok","name":"Ada","state":{}}"#);
        s.receive(r#"{"t":"joined","kind":"zone","room":"meadow","you":4,"netcode":{"step":0.1,"speed":300,"min":[-10,0],"max":[1010,600]}}"#);
        let (step, field) = s.netcode.clone().unwrap();
        let jengine_netcode::BuiltIn::TopDown(field) = field else { panic!("no walls were sent") };
        assert_eq!((step, field.speed, field.max), (0.1, 300.0, (1010.0, 600.0)));
        // With walls, the tile model - and a malformed grid is no model at all.
        s.receive(r#"{"t":"joined","kind":"zone","room":"cave","you":4,"netcode":{"step":0.1,"speed":300,"radius":10,"min":[-10,0],"max":[1010,600],"walls":{"cols":2,"rows":1,"cell":50,"origin":[0,600],"solid":"01"}}}"#);
        let Some((_, jengine_netcode::BuiltIn::Tiles(tiles))) = s.netcode.clone() else { panic!("walls make the tile model") };
        assert!(tiles.walls.is_solid(1, 0) && !tiles.walls.is_solid(0, 0));
        assert_eq!(tiles.radius, 10.0);
        s.receive(r#"{"t":"joined","kind":"zone","room":"cave","you":4,"netcode":{"step":0.1,"speed":300,"min":[-10,0],"max":[1010,600],"walls":{"cols":2,"rows":1,"cell":50,"origin":[0,600],"solid":"0"}}}"#);
        assert!(s.netcode.is_none());
        s.receive(r#"{"t":"sync","tick":1,"enter":[{"id":4,"x":500,"y":300}]}"#);
        assert_eq!(s.take_ack(), None, "no input has been applied yet");
        s.receive(r#"{"t":"sync","tick":9,"ack":3,"move":[[4,590,300]]}"#);
        assert_eq!(s.tick, 9);
        assert_eq!(s.take_ack(), Some((3, (590.0, 300.0))), "the input reached, and where that left us");
        assert_eq!(s.take_ack(), None, "told once");
        assert_eq!(s.entities[&4].server(), (590.0, 300.0));
        // A room that says nothing of how it moves cannot be predicted.
        s.receive(r#"{"t":"joined","kind":"relay","room":"x"}"#);
        assert!(s.netcode.is_none());
    }

    #[test]
    fn a_new_room_is_a_new_world() {
        let mut s = joined();
        s.receive(r#"{"t":"sync","enter":[{"id":4,"x":0,"y":0},{"id":9,"x":0,"y":0}]}"#);
        s.take_changes();
        // A transfer: nothing but a new `joined`.
        s.receive(r#"{"t":"joined","kind":"zone","room":"cave","you":2}"#);
        assert!(s.entities.is_empty());
        assert_eq!(s.take_changes().left, vec![4, 9]);
        assert_eq!(s.room, Some(("zone".into(), "cave".into())));
        assert_eq!(s.you, Some(2));
    }

    /// Zones number their entities each from 1: the player who was entity 2
    /// in one room may be followed by a goblin that is entity 2 in the next,
    /// in the same frame. That id must still be reported as gone (and as
    /// come), or the slot that showed the player shows the goblin.
    #[test]
    fn an_id_the_next_room_reuses_is_still_reported_gone() {
        let mut s = joined();
        s.receive(r#"{"t":"sync","enter":[{"id":2,"kind":"player","x":0,"y":0}]}"#);
        s.take_changes();
        s.receive(r#"{"t":"joined","kind":"zone","room":"cave","you":7}"#);
        s.receive(r#"{"t":"sync","enter":[{"id":2,"kind":"mob","x":5,"y":5},{"id":7,"kind":"player","x":0,"y":0}]}"#);
        let changes = s.take_changes();
        assert_eq!(changes.left, vec![2], "the old entity 2 went");
        assert!(changes.entered.contains(&2), "and a new entity 2 came");
        assert_eq!(s.entities[&2].kind, "mob");
        // Only that once: an ordinary frame after it reports nothing gone.
        s.receive(r#"{"t":"sync","update":[{"id":2,"hp":3}]}"#);
        assert!(s.take_changes().left.is_empty());
    }

    /// A facing is walked like a position, and the short way round: from
    /// just under a whole turn to just over none is a small step, not a spin.
    #[test]
    fn a_facing_is_walked_the_short_way_round() {
        let mut s = joined();
        s.receive(r#"{"t":"sync","enter":[{"id":4,"x":0,"y":0,"rot":6.2},{"id":5,"x":0,"y":0}]}"#);
        assert_eq!(s.entities[&4].rot, Some(6.2));
        assert_eq!(s.entities[&5].rot, None, "no facing, none made up");
        assert!(!s.entities[&4].props.contains_key("rot"), "a channel, not a property");
        s.advance(0.1, 0.0);
        s.receive(r#"{"t":"sync","move":[[4,0,0,0.1]]}"#);
        s.advance(0.05, 0.0);
        let tau = std::f64::consts::TAU;
        let half = s.entities[&4].rot.unwrap().rem_euclid(tau);
        let want = (6.2 + (0.1 + tau - 6.2) / 2.0).rem_euclid(tau);
        assert!((half - want).abs() < 0.01, "half way, the short way: {half} against {want}");
        s.advance(0.1, 0.0);
        assert!((s.entities[&4].rot.unwrap().rem_euclid(tau) - 0.1).abs() < 1e-9);
        // A three-number move keeps the facing it had.
        s.receive(r#"{"t":"sync","move":[[4,5,0]]}"#);
        s.advance(0.2, 0.0);
        assert!((s.entities[&4].rot.unwrap().rem_euclid(tau) - 0.1).abs() < 1e-9);
    }

    /// C11: a reconnect goes back with the options it joined with, and a
    /// room we were moved into (never asked for) has none.
    #[test]
    fn the_options_of_a_join_are_kept_for_its_room() {
        let mut s = Session::new();
        s.receive(r#"{"t":"welcome","player":"p_1","token":"tok","name":"Ada","state":{}}"#);
        let frame = s.join("duel", "", &json!({ "code": "ABCD" }));
        assert!(frame.contains("ABCD") && s.status == Status::Joining);
        s.receive(r#"{"t":"joined","kind":"duel","room":"r7"}"#);
        assert_eq!(s.options, json!({ "code": "ABCD" }));
        s.receive(r#"{"t":"joined","kind":"zone","room":"cave"}"#);
        assert_eq!(s.options, Value::Null, "a transfer was not our join");
        // A refused join forgets what it asked with.
        s.join("duel", "r8", &json!({ "code": "X" }));
        s.receive(r#"{"t":"error","why":"the room is full"}"#);
        s.receive(r#"{"t":"joined","kind":"zone","room":"meadow"}"#);
        assert_eq!(s.options, Value::Null);
    }

    /// C14 and C19: mid-room, the room can change how we move and which
    /// entity we are.
    #[test]
    fn a_room_can_change_our_walls_and_our_avatar_mid_stay() {
        let mut s = Session::new();
        s.receive(r#"{"t":"welcome","player":"p_1","token":"tok","name":"Ada","state":{}}"#);
        s.receive(r#"{"t":"joined","kind":"zone","room":"meadow","you":4,"netcode":{"step":0.1,"speed":300,"min":[0,0],"max":[100,50],"walls":{"cols":2,"rows":1,"cell":50,"origin":[0,50],"solid":"01"}}}"#);
        let rev = s.netcode_rev;
        s.receive(r#"{"t":"netcode","netcode":{"step":0.1,"speed":300,"min":[0,0],"max":[100,50],"walls":{"cols":2,"rows":1,"cell":50,"origin":[0,50],"solid":"00"}}}"#);
        let Some((_, jengine_netcode::BuiltIn::Tiles(tiles))) = s.netcode.clone() else { panic!("still the tile model") };
        assert!(!tiles.walls.is_solid(1, 0), "the wall is gone");
        assert_eq!(s.netcode_rev, rev + 1);
        s.receive(r#"{"t":"sync","tick":3,"ack":2,"enter":[{"id":4,"x":0,"y":0}]}"#);
        s.receive(r#"{"t":"you","you":9}"#);
        assert_eq!((s.you, s.take_ack()), (Some(9), None), "an ack about the old avatar is not about the new");
    }

    /// S18: the server's `state` is ours to keep; a room's frame called
    /// `state` is an event and nothing else.
    #[test]
    fn only_the_servers_own_state_frame_is_our_saved_state() {
        let mut s = joined();
        let event = s.receive(r#"{"t":"state","board":[1,2,3]}"#).unwrap();
        assert_eq!(event["board"][1], 2, "a room's frame reaches the scripts");
        assert_eq!(s.state["coins"], 2, "and leaves what we saved alone");
        s.receive(r#"{"t":"state","sys":true,"state":{"coins":9}}"#);
        assert_eq!(s.state["coins"], 9);
    }

    #[test]
    fn a_dropped_connection_forgets_the_room_and_keeps_who_we_are() {
        let mut s = joined();
        s.receive(r#"{"t":"sync","enter":[{"id":4,"x":0,"y":0}]}"#);
        s.dropped();
        assert_eq!((s.status, s.room.is_none(), s.entities.is_empty()), (Status::Offline, true, true));
        assert_eq!(s.token, "tok");
    }

    #[test]
    fn a_refused_join_and_an_unknown_token_are_said_and_recovered_from() {
        let mut s = Session::new();
        s.token = "stale".into();
        s.status = Status::SigningIn;
        s.receive(r#"{"t":"error","why":"unknown token"}"#);
        assert_eq!((s.status, s.token.as_str(), s.error.as_str()), (Status::Offline, "", "unknown token"));

        let mut s = Session::new();
        s.token = "mine".into();
        s.status = Status::SigningIn;
        s.receive(r#"{"t":"error","why":"the server cannot reach its store; try again shortly"}"#);
        assert_eq!((s.status, s.token.as_str()), (Status::Offline, "mine"), "a server in trouble does not cost us who we are");

        let mut s = Session::new();
        s.receive(r#"{"t":"welcome","player":"p","token":"t","name":"","state":{}}"#);
        s.status = Status::Joining;
        s.receive(r#"{"t":"error","why":"the room is full"}"#);
        assert_eq!((s.status, s.error.as_str()), (Status::SignedIn, "the room is full"));
    }

    #[test]
    fn an_entity_that_comes_and_goes_between_two_looks_is_in_neither_list() {
        let mut s = joined();
        s.receive(r#"{"t":"sync","enter":[{"id":7,"x":0,"y":0}]}"#);
        s.receive(r#"{"t":"sync","leave":[7]}"#);
        assert_eq!(s.take_changes(), Changes::default());
    }
}
