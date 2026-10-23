//! The easiest server there is: a room that decides **order, not rules**.
//!
//! This is `jengine-relay` as one [`Room`] - seats, and a log every message is
//! stamped into and broadcast from, the sender included. It speaks that
//! crate's protocol frame for frame, so a game written against the relay
//! (a lockstep game, say, whose clients run the rules) connects to a
//! `jengine-server` without changing a line. See `jengine-relay`'s crate doc
//! for why a deterministic game needs nothing more.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::room::{Player, PlayerId, Room, RoomCtx};
use crate::server::ROOM_FULL;

/// Largest body a relay room will carry.
pub const MAX_BODY_BYTES: usize = 8 * 1024;
/// Messages a room keeps.
pub const MAX_LOG: usize = 10_000;
/// Most seats a room may ask for.
pub const MAX_SEATS: usize = 16;

#[derive(Default)]
pub struct RelayRoom {
    /// Decided by the first joiner.
    seats: usize,
    /// The room code the first joiner gave (`{"code": ..}`, upper-cased);
    /// `None` until somebody has joined. A room opened without one is a
    /// quick-match room.
    code: Option<Option<String>>,
    /// seat -> (player, name)
    taken: BTreeMap<usize, (PlayerId, String)>,
    log: Vec<Value>,
}

impl RelayRoom {
    pub fn new() -> Self {
        Self::default()
    }

    fn seat_of(&self, player: &str) -> Option<usize> {
        self.taken.iter().find(|(_, (id, _))| id == player).map(|(seat, _)| *seat)
    }
}

impl Room for RelayRoom {
    fn on_join(&mut self, ctx: &mut RoomCtx, player: &mut Player, options: &Value)
    -> Result<Value, String> {
        // A room code keeps strangers out: the first joiner's becomes the
        // room's, and a joiner with another (or none, to a coded room) is
        // refused with ROOM_FULL - which sends a matchmaking join on to the
        // next room, so a quick match never lands at a private table. Join
        // options of `null` are a reconnect by name (`jengine-online` keeps
        // no options for one), and are let in.
        let code = options
            .get("code")
            .and_then(Value::as_str)
            .map(|c| c.trim().to_uppercase())
            .filter(|c| !c.is_empty());
        match &self.code {
            Some(have) if !options.is_null() && *have != code => return Err(ROOM_FULL.into()),
            None => self.code = Some(code),
            _ => {}
        }
        // Lua sends every number as a float (`4.0`), so `as_u64` alone took
        // every scripted room for the default size. A join that names no
        // size takes the room as it was opened.
        let asked = options.get("seats").and_then(Value::as_f64).filter(|v| v.fract() == 0.0);
        let seats = match asked {
            Some(n) => n as usize,
            None if self.seats > 0 => self.seats,
            None => 2,
        };
        if !(1..=MAX_SEATS).contains(&seats) {
            return Err("seats must be between 1 and 16".into());
        }
        if self.seats == 0 {
            self.seats = seats;
        }
        // The first joiner decides the room's size; a later one asking for a
        // different size is told, rather than seated in a game it is not
        // expecting.
        if self.seats != seats {
            return Err("that room was opened for a different number of players".into());
        }
        let Some(seat) = (0..self.seats).find(|s| !self.taken.contains_key(s)) else {
            return Err("the room is full".into());
        };
        let asked = options.get("name").and_then(Value::as_str).unwrap_or(&player.name).trim();
        let name: String = if asked.is_empty() {
            format!("Player {}", seat + 1)
        } else {
            asked.chars().take(24).collect()
        };
        self.taken.insert(seat, (player.id.clone(), name.clone()));
        ctx.broadcast(json!({ "t": "peer_joined", "seat": seat, "name": name }));
        let peers: Vec<Value> =
            self.taken.iter().map(|(s, (_, n))| json!({ "seat": s, "name": n })).collect();
        Ok(json!({ "seat": seat, "seats": self.seats, "peers": peers, "log": self.log }))
    }

    fn on_message(&mut self, ctx: &mut RoomCtx, player: &mut Player, body: Value) {
        let Some(seat) = self.seat_of(&player.id) else { return };
        if body.to_string().len() > MAX_BODY_BYTES {
            return ctx.send(&player.id, json!({ "t": "error", "why": "that message is too large" }));
        }
        if self.log.len() >= MAX_LOG {
            return ctx.send(&player.id, json!({ "t": "error", "why": "this room's log is full" }));
        }
        let msg = json!({ "t": "msg", "seq": self.log.len(), "from": seat, "body": body });
        self.log.push(msg.clone());
        ctx.broadcast(msg);
    }

    fn on_leave(&mut self, ctx: &mut RoomCtx, player: &mut Player) {
        if let Some(seat) = self.seat_of(&player.id) {
            self.taken.remove(&seat);
            ctx.broadcast(json!({ "t": "peer_left", "seat": seat }));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::room::{Effects, RoomRunner};
    use crate::store::MemoryStore;

    fn room() -> RoomRunner {
        RoomRunner::new("relay", "tide", Box::new(RelayRoom::new()), Arc::new(MemoryStore::new()), 64).0
    }

    fn join(r: &mut RoomRunner, id: &str, name: &str, seats: u64) -> Result<Effects, String> {
        r.join(Player::new(id, ""), &json!({ "name": name, "seats": seats })).map_err(|(_, why)| why)
    }

    fn frames(e: &Effects, to: &str) -> Vec<Value> {
        e.frames.iter().filter(|(id, _)| id == to).map(|(_, f)| f.to_json()).collect()
    }

    // The relay crate's own properties, held by the room that replaces it.

    #[test]
    fn two_players_take_two_seats_and_a_third_is_turned_away() {
        let mut r = room();
        assert_eq!(frames(&join(&mut r, "a", "Ada", 2).unwrap(), "a")[0]["seat"], 0);
        let b = join(&mut r, "b", "Bo", 2).unwrap();
        assert_eq!(frames(&b, "b")[0]["seat"], 1);
        assert_eq!(frames(&b, "a")[0]["t"], "peer_joined");
        assert_eq!(join(&mut r, "c", "Cy", 2).unwrap_err(), "the room is full");
    }

    #[test]
    fn a_message_comes_back_to_everyone_with_one_sequence_number() {
        let mut r = room();
        join(&mut r, "a", "Ada", 2).unwrap();
        join(&mut r, "b", "Bo", 2).unwrap();
        let out = r.message("a", json!({ "move": 19 }));
        assert_eq!(frames(&out, "a"), frames(&out, "b"));
        assert_eq!(frames(&out, "a")[0]["seq"], 0);
        assert_eq!(frames(&out, "a")[0]["from"], 0);
        assert_eq!(frames(&r.message("b", json!({ "move": 20 })), "a")[0]["seq"], 1);
    }

    #[test]
    fn a_player_who_drops_and_rejoins_gets_their_seat_and_the_whole_game() {
        let mut r = room();
        join(&mut r, "a", "Ada", 2).unwrap();
        join(&mut r, "b", "Bo", 2).unwrap();
        r.message("a", json!({ "move": 19 }));
        r.message("b", json!({ "move": 20 }));
        assert_eq!(frames(&r.leave("b"), "a")[0]["t"], "peer_left");
        let back = join(&mut r, "b2", "Bo", 2).unwrap();
        let joined = &frames(&back, "b2")[0];
        assert_eq!(joined["seat"], 1);
        assert_eq!(joined["log"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_room_keeps_the_size_it_was_opened_with() {
        let mut r = room();
        join(&mut r, "a", "Ada", 2).unwrap();
        assert!(join(&mut r, "b", "Bo", 4).is_err());
    }

    #[test]
    fn a_size_sent_as_a_float_is_the_size_and_none_takes_the_room_as_it_is() {
        let mut r = room();
        // What a Lua client sends: every number a float.
        r.join(Player::new("a", ""), &json!({ "seats": 4.0 })).unwrap();
        for id in ["b", "c"] {
            r.join(Player::new(id, ""), &Value::Null).unwrap();
        }
        assert!(r.join(Player::new("d", ""), &json!({ "seats": 4 })).is_ok(), "four seats, as asked");
        assert_eq!(r.join(Player::new("e", ""), &Value::Null).unwrap_err().1, "the room is full");
    }

    #[test]
    fn a_room_code_keeps_strangers_out_and_lets_a_reconnect_in() {
        let mut r = room();
        r.join(Player::new("a", ""), &json!({ "seats": 3, "code": "pals" })).unwrap();
        let quick = r.join(Player::new("b", ""), &json!({ "seats": 3 }));
        assert_eq!(quick.unwrap_err().1, ROOM_FULL, "a quick match is sent on");
        assert!(r.join(Player::new("c", ""), &json!({ "seats": 3, "code": " Pals" })).is_ok());
        assert!(r.join(Player::new("d", ""), &Value::Null).is_ok(), "a reconnect, by name");

        let mut q = room();
        q.join(Player::new("a", ""), &json!({ "seats": 2 })).unwrap();
        let coded = q.join(Player::new("b", ""), &json!({ "seats": 2, "code": "PALS" }));
        assert_eq!(coded.unwrap_err().1, ROOM_FULL, "a code does not open a quick-match room");
    }

    #[test]
    fn an_oversized_body_is_refused_to_its_sender_alone() {
        let mut r = room();
        join(&mut r, "a", "Ada", 2).unwrap();
        join(&mut r, "b", "Bo", 2).unwrap();
        let out = r.message("a", json!("x".repeat(MAX_BODY_BYTES + 10)));
        assert_eq!(frames(&out, "a")[0]["t"], "error");
        assert!(frames(&out, "b").is_empty());
    }
}
