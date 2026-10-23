//! **A relay for turn-based games** - rooms, seats, and one ordered log.
//!
//! `jengine-net` is a client and says so: "No server. The engine does not
//! listen for connections, and in a browser it could not." So no game in this
//! repo had ever sent a message to anyone. This is the smallest thing that
//! lets two copies of a game play each other.
//!
//! # What it decides, and what it does not
//!
//! **It decides order, not rules.** Every message a player sends is stamped
//! with a sequence number and broadcast to *everyone in the room, the sender
//! included*, and a client applies only what comes back. So both clients see
//! the same moves in the same order, and a game whose rules are deterministic
//! stays identical on both without the relay knowing what a move is. An
//! illegal move is refused by both clients the same way, because both run
//! the same rules over the same log.
//!
//! That is lockstep for turn-based games, and the trade is stated: a client
//! can see its own move only after a round trip. For a board game that is
//! invisible; for anything real-time it would not be.
//!
//! # The log is the game
//!
//! A room keeps every message it has relayed, and a player who joins - or
//! rejoins after a dropped connection - receives the whole log with its
//! seat. Replaying it rebuilds the game exactly, which is why reconnecting
//! needs no special case anywhere: a rejoin *is* a join.
//!
//! # Protocol
//!
//! JSON text frames. Client to relay:
//!
//! | Frame | Does |
//! |---|---|
//! | `{"t":"join","room":"TIDE","name":"Ada","seats":2}` | take the lowest free seat in a room, creating it |
//! | `{"t":"send","body":<any JSON>}` | append to the log and broadcast |
//! | `{"t":"leave"}` | give the seat up |
//!
//! Relay to client:
//!
//! | Frame | When |
//! |---|---|
//! | `{"t":"joined","room","seat","seats","peers":[{seat,name}],"log":[msg..]}` | to a joiner |
//! | `{"t":"peer_joined","seat","name"}` / `{"t":"peer_left","seat"}` | to the others |
//! | `{"t":"msg","seq","from","body"}` | to everyone, sender included |
//! | `{"t":"error","why"}` | to the one who asked |

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};

/// Largest body the relay will carry. A move is tens of bytes; this is room
/// for a chat line or a small snapshot, and a bound on what one client can
/// make every other client allocate.
pub const MAX_BODY_BYTES: usize = 8 * 1024;
/// Messages a room keeps. A long board game is a few hundred.
pub const MAX_LOG: usize = 10_000;
/// Rooms the relay holds at once.
pub const MAX_ROOMS: usize = 1_000;
/// Most seats a room may ask for.
pub const MAX_SEATS: usize = 16;

/// Which connection. The server numbers them; tests use literals.
pub type ConnId = u64;

#[derive(Debug, Default)]
struct Room {
    seats: usize,
    /// seat -> (connection, name)
    taken: BTreeMap<usize, (ConnId, String)>,
    log: Vec<Value>,
}

/// Rooms and who is in them - no sockets, so every rule here is a unit test.
#[derive(Debug, Default)]
pub struct Relay {
    rooms: HashMap<String, Room>,
    /// connection -> (room, seat)
    members: HashMap<ConnId, (String, usize)>,
}

/// What the relay wants sent, to whom.
pub type Outbox = Vec<(ConnId, Value)>;

fn error(to: ConnId, why: &str) -> Outbox {
    vec![(to, json!({ "t": "error", "why": why }))]
}

impl Relay {
    pub fn new() -> Self {
        Self::default()
    }

    /// Handle one text frame from `from`.
    pub fn handle(&mut self, from: ConnId, text: &str) -> Outbox {
        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return error(from, "not JSON");
        };
        match frame.get("t").and_then(Value::as_str) {
            Some("join") => {
                let room = frame.get("room").and_then(Value::as_str).unwrap_or("");
                let name = frame.get("name").and_then(Value::as_str).unwrap_or("");
                let seats = frame.get("seats").and_then(Value::as_u64).unwrap_or(2) as usize;
                self.join(from, room, name, seats)
            }
            Some("send") => match frame.get("body") {
                Some(body) => self.send(from, body.clone()),
                None => error(from, "send needs a body"),
            },
            Some("leave") => self.leave(from),
            _ => error(from, "unknown frame"),
        }
    }

    /// Take the lowest free seat in `room`.
    ///
    /// **A rejoin is a join.** A player whose connection dropped left their
    /// seat free; joining again takes the lowest free one, which is theirs
    /// unless someone else has sat down, and the log they are handed rebuilds
    /// the game to the move.
    pub fn join(&mut self, from: ConnId, room: &str, name: &str, seats: usize) -> Outbox {
        let room = room.trim();
        if room.is_empty() || room.len() > 32 {
            return error(from, "a room needs a name of 1 to 32 characters");
        }
        if !(1..=MAX_SEATS).contains(&seats) {
            return error(from, "seats must be between 1 and 16");
        }
        if self.members.contains_key(&from) {
            // One room per connection. Leaving first keeps the bookkeeping
            // one-to-one, which is what makes `leave` on disconnect correct.
            let mut out = self.leave(from);
            out.extend(self.join(from, room, name, seats));
            return out;
        }
        if !self.rooms.contains_key(room) && self.rooms.len() >= MAX_ROOMS {
            return error(from, "the relay is full");
        }
        let r = self
            .rooms
            .entry(room.to_string())
            .or_insert_with(|| Room { seats, ..Default::default() });
        // The first joiner decides the room's size; a later one asking for a
        // different size is told, rather than silently seated in a room that
        // is not the game it thinks it is.
        if r.seats != seats {
            return error(from, "that room was opened for a different number of players");
        }
        let Some(seat) = (0..r.seats).find(|s| !r.taken.contains_key(s)) else {
            return error(from, "the room is full");
        };
        let name = if name.trim().is_empty() {
            format!("Player {}", seat + 1)
        } else {
            name.chars().take(24).collect()
        };
        r.taken.insert(seat, (from, name.clone()));
        let peers: Vec<Value> = r
            .taken
            .iter()
            .map(|(s, (_, n))| json!({ "seat": s, "name": n }))
            .collect();
        let mut out: Outbox = r
            .taken
            .iter()
            .filter(|(s, _)| **s != seat)
            .map(|(_, (c, _))| (*c, json!({ "t": "peer_joined", "seat": seat, "name": name })))
            .collect();
        out.push((
            from,
            json!({
                "t": "joined", "room": room, "seat": seat, "seats": r.seats,
                "peers": peers, "log": r.log,
            }),
        ));
        self.members.insert(from, (room.to_string(), seat));
        out
    }

    /// Stamp and broadcast - to everyone, the sender included, because what
    /// comes back is the order.
    pub fn send(&mut self, from: ConnId, body: Value) -> Outbox {
        let Some((room, seat)) = self.members.get(&from).cloned() else {
            return error(from, "join a room first");
        };
        if body.to_string().len() > MAX_BODY_BYTES {
            return error(from, "that message is too large");
        }
        let Some(r) = self.rooms.get_mut(&room) else {
            return error(from, "join a room first");
        };
        if r.log.len() >= MAX_LOG {
            return error(from, "this room's log is full");
        }
        let msg = json!({ "t": "msg", "seq": r.log.len(), "from": seat, "body": body });
        r.log.push(msg.clone());
        r.taken.values().map(|(c, _)| (*c, msg.clone())).collect()
    }

    /// Give up a seat - by asking, or by the connection closing.
    ///
    /// The room outlives its players while anyone is in it; once it is empty
    /// it is dropped, log and all. A relay that kept every finished game
    /// would grow without bound, and one that dropped a room the moment a
    /// single player's wifi hiccuped would lose the game they came back to.
    pub fn leave(&mut self, from: ConnId) -> Outbox {
        let Some((room, seat)) = self.members.remove(&from) else {
            return Vec::new();
        };
        let Some(r) = self.rooms.get_mut(&room) else {
            return Vec::new();
        };
        r.taken.remove(&seat);
        if r.taken.is_empty() {
            self.rooms.remove(&room);
            return Vec::new();
        }
        r.taken
            .values()
            .map(|(c, _)| (*c, json!({ "t": "peer_left", "seat": seat })))
            .collect()
    }

    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }
}

// ---- the socket shell ---------------------------------------------------------

/// Serve on `addr` until the process ends. Thread per connection.
///
/// Each connection thread reads with a short timeout and, between reads,
/// writes whatever the relay queued for it - which keeps the sync
/// `tungstenite` socket on one thread without an async runtime.
pub fn serve(addr: &str) -> std::io::Result<()> {
    let listener = std::net::TcpListener::bind(addr)?;
    log::info!("[relay] listening on ws://{addr}");
    serve_on(listener);
    Ok(())
}

/// [`serve`] on a listener the caller bound - which is how a test gets a free
/// port (`127.0.0.1:0`) and learns what it was.
pub fn serve_on(listener: std::net::TcpListener) {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::{Sender, channel};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tungstenite::{Message, accept};

    let relay = Arc::new(Mutex::new(Relay::new()));
    let outboxes: Arc<Mutex<HashMap<ConnId, Sender<String>>>> = Arc::default();
    let next = AtomicU64::new(1);

    let deliver = {
        let outboxes = outboxes.clone();
        move |out: Outbox| {
            let map = outboxes.lock().unwrap();
            for (to, frame) in out {
                if let Some(tx) = map.get(&to) {
                    let _ = tx.send(frame.to_string());
                }
            }
        }
    };
    let deliver = Arc::new(deliver);

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let id = next.fetch_add(1, Ordering::Relaxed);
        let relay = relay.clone();
        let outboxes = outboxes.clone();
        let deliver = deliver.clone();
        std::thread::spawn(move || {
            let Ok(mut ws) = accept(stream) else { return };
            let _ = ws.get_ref().set_read_timeout(Some(Duration::from_millis(20)));
            let (tx, rx) = channel::<String>();
            outboxes.lock().unwrap().insert(id, tx);
            log::info!("[relay] connection {id} opened");
            loop {
                match ws.read() {
                    Ok(Message::Text(text)) => {
                        let out = relay.lock().unwrap().handle(id, &text);
                        deliver(out);
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => break,
                }
                let mut broken = false;
                while let Ok(frame) = rx.try_recv() {
                    if ws.send(Message::Text(frame)).is_err() {
                        broken = true;
                        break;
                    }
                }
                if broken {
                    break;
                }
            }
            outboxes.lock().unwrap().remove(&id);
            let out = relay.lock().unwrap().leave(id);
            deliver(out);
            log::info!("[relay] connection {id} closed");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames_to(out: &Outbox, to: ConnId) -> Vec<&Value> {
        out.iter().filter(|(c, _)| *c == to).map(|(_, f)| f).collect()
    }

    #[test]
    fn two_players_take_two_seats_and_a_third_is_turned_away() {
        let mut r = Relay::new();
        let a = r.join(1, "tide", "Ada", 2);
        assert_eq!(frames_to(&a, 1)[0]["seat"], 0);
        let b = r.join(2, "tide", "Bo", 2);
        assert_eq!(frames_to(&b, 2)[0]["seat"], 1);
        assert_eq!(frames_to(&b, 1)[0]["t"], "peer_joined", "the first player hears the second");
        let c = r.join(3, "tide", "Cy", 2);
        assert_eq!(frames_to(&c, 3)[0]["t"], "error");
    }

    /// The property everything else rests on: what a sender gets back is the
    /// same frame, with the same sequence number, as everyone else.
    #[test]
    fn a_message_comes_back_to_everyone_with_one_sequence_number() {
        let mut r = Relay::new();
        r.join(1, "tide", "Ada", 2);
        r.join(2, "tide", "Bo", 2);
        let out = r.send(1, json!({ "move": 19 }));
        assert_eq!(out.len(), 2, "the sender and the other player");
        assert_eq!(frames_to(&out, 1), frames_to(&out, 2));
        assert_eq!(frames_to(&out, 1)[0]["seq"], 0);
        assert_eq!(frames_to(&out, 1)[0]["from"], 0);
        let out = r.send(2, json!({ "move": 20 }));
        assert_eq!(frames_to(&out, 1)[0]["seq"], 1);
    }

    #[test]
    fn a_player_who_drops_and_rejoins_gets_their_seat_and_the_whole_game() {
        let mut r = Relay::new();
        r.join(1, "tide", "Ada", 2);
        r.join(2, "tide", "Bo", 2);
        r.send(1, json!({ "move": 19 }));
        r.send(2, json!({ "move": 20 }));
        let left = r.leave(2);
        assert_eq!(frames_to(&left, 1)[0]["t"], "peer_left");

        let back = r.join(7, "tide", "Bo", 2);
        let joined = frames_to(&back, 7)[0];
        assert_eq!(joined["seat"], 1, "the free seat is theirs again");
        assert_eq!(joined["log"].as_array().unwrap().len(), 2, "with every move so far");
    }

    #[test]
    fn an_empty_room_is_forgotten() {
        let mut r = Relay::new();
        r.join(1, "tide", "Ada", 2);
        r.leave(1);
        assert_eq!(r.room_count(), 0);
        let again = r.join(2, "tide", "Bo", 2);
        assert!(frames_to(&again, 2)[0]["log"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_room_keeps_the_size_it_was_opened_with() {
        let mut r = Relay::new();
        r.join(1, "tide", "Ada", 2);
        let out = r.join(2, "tide", "Bo", 4);
        assert_eq!(frames_to(&out, 2)[0]["t"], "error");
    }

    #[test]
    fn nothing_is_relayed_for_someone_not_in_a_room() {
        let mut r = Relay::new();
        let out = r.send(9, json!({}));
        assert_eq!(frames_to(&out, 9)[0]["t"], "error");
    }

    #[test]
    fn frames_that_are_not_the_protocol_are_refused_by_name() {
        let mut r = Relay::new();
        assert_eq!(r.handle(1, "hello")[0].1["t"], "error");
        assert_eq!(r.handle(1, r#"{"t":"dance"}"#)[0].1["t"], "error");
        let big = "x".repeat(MAX_BODY_BYTES + 10);
        r.join(1, "tide", "Ada", 1);
        assert_eq!(r.send(1, json!(big))[0].1["t"], "error");
    }
}
