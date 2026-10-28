//! A room for **input lockstep**: every machine runs the same deterministic
//! simulation in fixed steps, and all this room does is collect each seat's
//! input for a step and send the step out once every seat's has arrived.
//!
//! Its client is a `lockstep_session` component, which holds the
//! engine's fixed-step clock (`ecs::fixed_step::set_gate`) at the last step
//! it has from here - so no machine runs a step another has not got the same
//! input for, and two machines run the same game from the same inputs
//! (docs/determinism-proposal.md, phase 5).
//!
//! The relay room keeps every message ever sent for a late joiner; this one
//! keeps every *step* - one small array per step, an hour of them at sixty
//! a second is a few megabytes - and that history is what lets a dropped
//! player come back and a spectator watch: each replays the match from its
//! own freshly loaded scene, fast (`ecs::fixed_step::set_catch_up`), and then
//! plays on with everyone.
//!
//! Nobody is waited on for ever. A seat whose player has left - or who has
//! sent nothing for `silent_after` seconds while the others wait - is played
//! as "nothing held" (an input of `0`) until they are back, and everyone is
//! told (`peer_left` / `silent`, then `peer_joined` / `back`).
//!
//! The protocol, beside the server's own `join` / `leave`:
//!
//! - join options: `{ "seats": N, "name": .., "code": .., "silent_after": s }`,
//!   the first joiner sizing the room as the relay's does; `{ "watch": true }`
//!   to spectate. The `joined` answer carries `seat` (`null` for a
//!   spectator), `seats`, `peers`, `started` and, once started, `history`:
//!   every step so far, `[[seat 0's, seat 1's, ..], ..]` from step 1.
//! - `{"t":"start","seats":N,"peers":[..]}` to everyone when the last seat
//!   fills: step 1 is the first step.
//! - a seated client sends `{"t":"in","s":S,"i":[..]}` - its inputs for steps
//!   `S, S+1, ..` (whatever the client and its game agree an input is; the
//!   room never reads one) - optionally with `"hash":{"s":S,"h":".."}`.
//! - the room sends `{"t":"steps","s":S,"i":[[..], ..]}`: every step from
//!   `S` it now has, in order, each step once.
//! - `{"t":"silent","seat":k}` / `{"t":"back","seat":k}` around a silence.
//! - `{"t":"desync","s":S,"hashes":[..]}` when the seats' hashes of step `S`
//!   differ - the games have come apart, and the match stops.
//! - `{"t":"ended","why":..}` when every seated player has left.
//!
//! A match is joined until it has started and rejoined (into a seat that is
//! empty, its own name first) after; a rejoin or a spectator later than
//! [`MAX_HISTORY`] steps is refused, the history being full.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::room::{Player, PlayerId, Players, Room, RoomCtx};
use crate::server::ROOM_FULL;

/// Most seats a lockstep room may ask for.
pub const MAX_SEATS: usize = 8;
/// Most spectators a room takes.
pub const MAX_WATCHERS: usize = 16;
/// How far past the last step sent a seat's input may be: a client cannot
/// fill the room's memory by sending far into the future.
pub const MAX_AHEAD: u64 = 600;
/// Inputs one message may carry.
pub const MAX_BATCH: usize = 64;
/// Steps a room keeps for those who join late: an hour at 60 a second.
pub const MAX_HISTORY: usize = 60 * 60 * 60;
/// Seconds a seat may send nothing while the others wait before it is played
/// as "nothing held"; a join can ask for another (`silent_after`, 0 never).
pub const SILENT_AFTER: f64 = 3.0;
/// How often the room checks for a silent seat.
pub const TICK_HZ: f64 = 10.0;

/// One seat.
#[derive(Debug, Clone)]
struct Seat {
    name: String,
    /// Who is in it, or `None` while its player is away.
    player: Option<PlayerId>,
    /// Played as "nothing held" until it sends again.
    silent: bool,
    /// Room time this seat last sent an input.
    heard: f64,
    /// step -> input, for steps not sent yet.
    inputs: BTreeMap<u64, Value>,
}

#[derive(Default)]
pub struct LockstepRoom {
    /// Decided by the first joiner.
    seats: usize,
    /// The first joiner's room code, as the relay room keeps it.
    code: Option<Option<String>>,
    seat: BTreeMap<usize, Seat>,
    watchers: BTreeSet<PlayerId>,
    started: bool,
    ended: bool,
    /// The next step to send out; step 1 is the first.
    next: u64,
    /// Every step sent, from step 1.
    history: Vec<Value>,
    /// step -> each seat's hash of it, until every seat that is playing has one.
    hashes: BTreeMap<u64, BTreeMap<usize, String>>,
    /// The last step whose hashes were compared: one reported later (by a
    /// machine catching up) is not waited on.
    compared: u64,
    silent_after: f64,
    /// Room time the step `next` began to be waited for.
    waiting_since: Option<f64>,
}

impl LockstepRoom {
    pub fn new() -> Self {
        Self { next: 1, silent_after: SILENT_AFTER, ..Self::default() }
    }

    fn seat_of(&self, player: &str) -> Option<usize> {
        self.seat.iter().find(|(_, s)| s.player.as_deref() == Some(player)).map(|(k, _)| *k)
    }

    fn peers(&self) -> Vec<Value> {
        self.seat
            .iter()
            .map(|(k, s)| json!({ "seat": k, "name": s.name, "here": s.player.is_some(), "silent": s.silent }))
            .collect()
    }

    /// A seat whose input is not waited for: away, or silent.
    fn played_for(&self, seat: usize) -> bool {
        self.seat.get(&seat).is_none_or(|s| s.player.is_none() || s.silent)
    }

    /// Every step that can go, from `next` on, as one message: each seat's
    /// input, or `0` for a seat nobody is waited on for.
    fn send_ready(&mut self, ctx: &mut RoomCtx) {
        if !self.started || self.ended {
            return;
        }
        // A step needs someone playing it: with every seat away or silent the
        // match waits, rather than run on by itself.
        if (0..self.seats).all(|k| self.played_for(k)) {
            return;
        }
        let from = self.next;
        let mut steps = Vec::new();
        loop {
            let ready = (0..self.seats).all(|k| self.played_for(k) || self.seat[&k].inputs.contains_key(&self.next));
            if !ready {
                break;
            }
            let step: Vec<Value> = (0..self.seats)
                .map(|k| {
                    let s = self.seat.get_mut(&k).unwrap();
                    let input = s.inputs.remove(&self.next);
                    if s.player.is_none() || s.silent { json!(0) } else { input.unwrap_or(json!(0)) }
                })
                .collect();
            let step = Value::Array(step);
            if self.history.len() < MAX_HISTORY {
                self.history.push(step.clone());
            }
            steps.push(step);
            self.next += 1;
            if steps.len() >= MAX_AHEAD as usize {
                break;
            }
        }
        if !steps.is_empty() {
            self.waiting_since = None;
            ctx.broadcast(json!({ "t": "steps", "s": from, "i": steps }));
        }
    }

    fn end(&mut self, ctx: &mut RoomCtx, why: &str) {
        if !self.ended {
            self.ended = true;
            ctx.broadcast(json!({ "t": "ended", "why": why }));
        }
    }
}

impl Room for LockstepRoom {
    fn on_join(&mut self, ctx: &mut RoomCtx, player: &mut Player, options: &Value) -> Result<Value, String> {
        let code = options
            .get("code")
            .and_then(Value::as_str)
            .map(|c| c.trim().to_uppercase())
            .filter(|c| !c.is_empty());
        match &self.code {
            Some(have) if *have != code => return Err(ROOM_FULL.into()),
            None => self.code = Some(code),
            _ => {}
        }
        let late = self.started && self.history.len() >= MAX_HISTORY;
        let history = || if self.started { json!(self.history) } else { json!([]) };

        // A spectator: every step so far, and every one from now, no seat.
        if options.get("watch").and_then(Value::as_bool) == Some(true) {
            if late || self.ended {
                return Err("that match is too far along to watch".into());
            }
            if self.watchers.len() >= MAX_WATCHERS {
                return Err(ROOM_FULL.into());
            }
            self.watchers.insert(player.id.clone());
            return Ok(json!({
                "seat": Value::Null, "seats": self.seats, "peers": self.peers(),
                "started": self.started, "history": history(),
            }));
        }

        // Cut as a seat's name is, or a long name would not find its own seat
        // on a rejoin.
        let asked: String = options.get("name").and_then(Value::as_str).unwrap_or(&player.name).trim().chars().take(24).collect();
        if self.started {
            // A seat whose player has gone, theirs first.
            if late || self.ended {
                return Err("that match can no longer be rejoined".into());
            }
            let empty: Vec<usize> = self.seat.iter().filter(|(_, s)| s.player.is_none()).map(|(k, _)| *k).collect();
            let Some(&k) = empty.iter().find(|k| self.seat[k].name == asked).or(empty.first()) else {
                return Err(ROOM_FULL.into());
            };
            // Silent until it speaks: it has the match so far to replay
            // first, and nobody waits for that.
            let s = self.seat.get_mut(&k).unwrap();
            s.player = Some(player.id.clone());
            s.silent = true;
            s.heard = ctx.time();
            s.inputs.clear();
            let name = s.name.clone();
            ctx.broadcast(json!({ "t": "peer_joined", "seat": k, "name": name, "back": true, "silent": true }));
            return Ok(json!({
                "seat": k, "seats": self.seats, "peers": self.peers(), "started": true, "history": history(),
            }));
        }

        // Lua sends numbers as floats; a join that names no size takes the
        // room as it was opened.
        let size = options.get("seats").and_then(Value::as_f64).filter(|v| v.fract() == 0.0);
        let seats = match size {
            Some(n) => n as usize,
            None if self.seats > 0 => self.seats,
            None => 2,
        };
        if !(1..=MAX_SEATS).contains(&seats) {
            return Err(format!("seats must be between 1 and {MAX_SEATS}"));
        }
        if self.seats == 0 {
            self.seats = seats;
            if let Some(s) = options.get("silent_after").and_then(Value::as_f64).filter(|s| *s >= 0.0) {
                self.silent_after = s;
            }
        }
        if self.seats != seats {
            return Err("that room was opened for a different number of players".into());
        }
        let Some(k) = (0..self.seats).find(|k| !self.seat.contains_key(k)) else {
            return Err(ROOM_FULL.into());
        };
        let name = if asked.is_empty() { format!("Player {}", k + 1) } else { asked };
        self.seat.insert(
            k,
            Seat { name: name.clone(), player: Some(player.id.clone()), silent: false, heard: ctx.time(), inputs: BTreeMap::new() },
        );
        ctx.broadcast(json!({ "t": "peer_joined", "seat": k, "name": name }));
        if self.seat.len() == self.seats {
            self.started = true;
            ctx.broadcast(json!({ "t": "start", "seats": self.seats, "peers": self.peers() }));
        }
        Ok(json!({ "seat": k, "seats": self.seats, "peers": self.peers(), "started": self.started, "history": [] }))
    }

    fn on_message(&mut self, ctx: &mut RoomCtx, player: &mut Player, body: Value) {
        let Some(k) = self.seat_of(&player.id) else { return };
        if !self.started || self.ended || body.get("t").and_then(Value::as_str) != Some("in") {
            return;
        }
        let Some(from) = body.get("s").and_then(Value::as_f64).filter(|s| *s >= 1.0 && s.fract() == 0.0) else {
            return ctx.send(&player.id, json!({ "t": "error", "why": "an input needs its step, `s`" }));
        };
        let from = from as u64;
        let now = ctx.time();
        if let Some(inputs) = body.get("i").and_then(Value::as_array) {
            if inputs.len() > MAX_BATCH {
                return ctx.send(&player.id, json!({ "t": "error", "why": "too many inputs in one message" }));
            }
            let next = self.next;
            let s = self.seat.get_mut(&k).unwrap();
            let mut fresh = false;
            for (n, input) in inputs.iter().enumerate() {
                let step = from + n as u64;
                // A step already sent is history; one too far ahead is refused.
                if step >= next && step < next + MAX_AHEAD {
                    s.inputs.entry(step).or_insert_with(|| input.clone());
                    fresh = true;
                }
            }
            if fresh {
                s.heard = now;
                if s.silent {
                    s.silent = false;
                    ctx.broadcast(json!({ "t": "back", "seat": k }));
                }
            }
        }
        if let Some(hash) = body.get("hash") {
            if let (Some(step), Some(h)) = (hash.get("s").and_then(Value::as_f64), hash.get("h").and_then(Value::as_str)) {
                let step = step as u64;
                // A hash is of a step this machine has run, so never past the
                // inputs the room would take: one further on is refused, or a
                // client could fill the room's memory with them.
                if step > self.compared && step < self.next + MAX_AHEAD {
                    self.hashes.entry(step).or_default().insert(k, h.to_string());
                }
            }
        }
        // Compare every step whose hashes are all in from the seats playing.
        let complete: Vec<u64> = self
            .hashes
            .iter()
            .filter(|(_, row)| (0..self.seats).all(|s| self.played_for(s) || row.contains_key(&s)))
            .map(|(step, _)| *step)
            .collect();
        for step in complete {
            let row = self.hashes.remove(&step).unwrap_or_default();
            self.compared = self.compared.max(step);
            let mut values = row.values();
            if let Some(first) = values.next()
                && values.any(|h| h != first)
            {
                let hashes: Vec<Value> = (0..self.seats).map(|s| json!(row.get(&s))).collect();
                self.ended = true;
                ctx.broadcast(json!({ "t": "desync", "s": step, "hashes": hashes }));
                return;
            }
        }
        self.send_ready(ctx);
    }

    fn on_tick(&mut self, ctx: &mut RoomCtx, _players: &mut Players, _dt: f64) {
        if !self.started || self.ended || self.silent_after <= 0.0 {
            return;
        }
        let now = ctx.time();
        // Is anyone holding the next step up?
        let missing: Vec<usize> =
            (0..self.seats).filter(|k| !self.played_for(*k) && !self.seat[k].inputs.contains_key(&self.next)).collect();
        if missing.is_empty() {
            self.waiting_since = None;
            return;
        }
        let since = *self.waiting_since.get_or_insert(now);
        if now - since < self.silent_after {
            return;
        }
        for k in missing {
            let s = self.seat.get_mut(&k).unwrap();
            if now - s.heard >= self.silent_after {
                s.silent = true;
                ctx.broadcast(json!({ "t": "silent", "seat": k }));
            }
        }
        self.send_ready(ctx);
    }

    fn on_leave(&mut self, ctx: &mut RoomCtx, player: &mut Player) {
        if self.watchers.remove(&player.id) {
            return;
        }
        let Some(k) = self.seat_of(&player.id) else { return };
        if !self.started {
            self.seat.remove(&k);
            ctx.broadcast(json!({ "t": "peer_left", "seat": k }));
            return;
        }
        // Played as "nothing held" until they are back.
        if let Some(s) = self.seat.get_mut(&k) {
            s.player = None;
            s.inputs.clear();
        }
        ctx.broadcast(json!({ "t": "peer_left", "seat": k }));
        if self.seat.values().all(|s| s.player.is_none()) {
            self.end(ctx, "every player left");
        } else {
            self.send_ready(ctx);
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
        RoomRunner::new("lockstep", "match", Box::new(LockstepRoom::new()), Arc::new(MemoryStore::new()), 64).0
    }

    fn join(r: &mut RoomRunner, id: &str, seats: u64) -> Result<Effects, String> {
        r.join(Player::new(id, ""), &json!({ "name": id, "seats": seats })).map_err(|(_, why)| why)
    }

    fn frames(e: &Effects, to: &str) -> Vec<Value> {
        e.frames.iter().filter(|(id, _)| id == to).map(|(_, f)| f.to_json()).collect()
    }

    fn of(e: &Effects, to: &str, t: &str) -> Vec<Value> {
        frames(e, to).into_iter().filter(|f| f["t"] == t).collect()
    }

    fn started() -> RoomRunner {
        let mut r = room();
        join(&mut r, "a", 2).unwrap();
        join(&mut r, "b", 2).unwrap();
        r
    }

    #[test]
    fn the_match_starts_when_the_last_seat_fills() {
        let mut r = room();
        let a = join(&mut r, "a", 2).unwrap();
        assert_eq!(frames(&a, "a")[0]["started"], false);
        let b = join(&mut r, "b", 2).unwrap();
        assert_eq!(of(&b, "a", "start").len(), 1, "the waiting seat is told");
        assert!(frames(&b, "b").iter().any(|f| f["started"] == true || f["t"] == "start"), "and so is the last");
        assert!(join(&mut r, "c", 2).is_err(), "nobody takes a seat in a started match");
    }

    #[test]
    fn a_step_goes_out_once_every_seat_has_sent_it() {
        let mut r = started();
        // Seat 0 is ahead: nothing is sent until seat 1 catches up.
        assert!(of(&r.message("a", json!({ "t": "in", "s": 1, "i": [5, 6, 7] })), "a", "steps").is_empty());
        let out = r.message("b", json!({ "t": "in", "s": 1, "i": [9, 9] }));
        let steps = of(&out, "a", "steps");
        assert_eq!(steps[0]["s"], 1);
        assert_eq!(steps[0]["i"], json!([[5, 9], [6, 9]]), "steps 1 and 2, seat by seat");
        assert_eq!(of(&out, "b", "steps"), steps, "everyone gets the same");
        // Step 3 goes when seat 1 sends it; resent inputs are ignored.
        let out = r.message("b", json!({ "t": "in", "s": 2, "i": [1, 8] }));
        assert_eq!(of(&out, "a", "steps")[0], json!({ "t": "steps", "s": 3, "i": [[7, 8]] }));
    }

    #[test]
    fn differing_hashes_are_a_desync() {
        let mut r = started();
        r.message("a", json!({ "t": "in", "s": 1, "i": [0], "hash": { "s": 30, "h": "ab" } }));
        let same = r.message("b", json!({ "t": "in", "s": 1, "i": [0], "hash": { "s": 30, "h": "ab" } }));
        assert!(of(&same, "a", "desync").is_empty());
        r.message("a", json!({ "t": "in", "s": 2, "i": [0], "hash": { "s": 60, "h": "ab" } }));
        let apart = r.message("b", json!({ "t": "in", "s": 2, "i": [0], "hash": { "s": 60, "h": "cd" } }));
        assert_eq!(of(&apart, "a", "desync")[0]["s"], 60);
        assert!(of(&r.message("a", json!({ "t": "in", "s": 3, "i": [0] })), "a", "steps").is_empty(), "and it stops");
    }

    /// A player who leaves is played as "nothing held" - the match goes on -
    /// and coming back by name takes the seat with every step so far.
    #[test]
    fn a_player_who_leaves_is_played_for_and_comes_back() {
        let mut r = started();
        r.message("a", json!({ "t": "in", "s": 1, "i": [3, 3] }));
        r.message("b", json!({ "t": "in", "s": 1, "i": [4, 4] }));
        let left = r.leave("b");
        assert!(of(&left, "a", "ended").is_empty(), "the match goes on");
        let out = r.message("a", json!({ "t": "in", "s": 3, "i": [5] }));
        assert_eq!(of(&out, "a", "steps")[0]["i"], json!([[5, 0]]), "the empty seat holds nothing");
        // Back, under another connection, by name.
        let back = r.join(Player::new("b2", ""), &json!({ "name": "b" })).unwrap();
        let joined = &frames(&back, "b2")[0];
        assert_eq!(joined["seat"], 1);
        assert_eq!(joined["history"], json!([[3, 4], [3, 4], [5, 0]]), "every step so far");
        assert_eq!(of(&back, "a", "peer_joined")[0]["back"], true);
        // Silent until it speaks - it has the match to replay first - so
        // nobody waits for it yet.
        let out = r.message("a", json!({ "t": "in", "s": 4, "i": [6] }));
        assert_eq!(of(&out, "a", "steps")[0]["i"], json!([[6, 0]]));
        // Then it is back, and waited for.
        let out = r.message("b2", json!({ "t": "in", "s": 5, "i": [7] }));
        assert_eq!(of(&out, "a", "back")[0]["seat"], 1);
        assert!(of(&r.message("a", json!({ "t": "in", "s": 6, "i": [8] })), "a", "steps").is_empty());
        let out = r.message("a", json!({ "t": "in", "s": 5, "i": [7] }));
        assert_eq!(of(&out, "a", "steps")[0]["i"], json!([[7, 7]]), "{out:?}");
        assert!(of(&r.message("b2", json!({ "t": "in", "s": 6, "i": [9] })), "a", "steps").len() == 1);
        // Everyone gone ends it.
        r.leave("a");
        assert_eq!(of(&r.leave("b2"), "b2", "ended").len(), 0, "the last one out is not told");
    }

    /// A seat that sends nothing holds nobody up for long: after
    /// `silent_after` it is played as "nothing held" until it sends again.
    #[test]
    fn a_silent_seat_is_played_for_until_it_speaks() {
        let mut r = room();
        r.join(Player::new("a", ""), &json!({ "name": "a", "seats": 2, "silent_after": 1.0 })).unwrap();
        join(&mut r, "b", 2).unwrap();
        r.message("a", json!({ "t": "in", "s": 1, "i": [1, 1, 1] }));
        assert!(of(&r.tick(0.5), "a", "steps").is_empty(), "waited for, at first");
        let quiet = r.tick(1.0);
        assert_eq!(of(&quiet, "a", "silent")[0]["seat"], 1);
        assert_eq!(of(&quiet, "a", "steps")[0]["i"], json!([[1, 0], [1, 0], [1, 0]]));
        // It speaks: back, and waited for again.
        let back = r.message("b", json!({ "t": "in", "s": 4, "i": [2] }));
        assert_eq!(of(&back, "a", "back")[0]["seat"], 1);
        assert!(of(&back, "a", "steps").is_empty(), "seat 0 has not sent step 4");
        let out = r.message("a", json!({ "t": "in", "s": 4, "i": [9] }));
        assert_eq!(of(&out, "a", "steps")[0]["i"], json!([[9, 2]]));
    }

    #[test]
    fn a_spectator_gets_the_match_so_far_and_every_step_after() {
        let mut r = started();
        r.message("a", json!({ "t": "in", "s": 1, "i": [1] }));
        r.message("b", json!({ "t": "in", "s": 1, "i": [2] }));
        let watch = r.join(Player::new("w", ""), &json!({ "watch": true })).unwrap();
        let joined = &frames(&watch, "w")[0];
        assert_eq!(joined["seat"], Value::Null);
        assert_eq!(joined["history"], json!([[1, 2]]));
        r.message("a", json!({ "t": "in", "s": 2, "i": [3] }));
        let out = r.message("b", json!({ "t": "in", "s": 2, "i": [4] }));
        assert_eq!(of(&out, "w", "steps")[0]["i"], json!([[3, 4]]));
        // A spectator's input is nobody's.
        assert!(of(&r.message("w", json!({ "t": "in", "s": 3, "i": [9] })), "a", "steps").is_empty());
    }

    #[test]
    fn nobody_playing_is_no_step() {
        let mut r = room();
        r.join(Player::new("a", ""), &json!({ "name": "a", "seats": 2, "silent_after": 1.0 })).unwrap();
        join(&mut r, "b", 2).unwrap();
        r.leave("b");
        // Seat 0 goes quiet too: the match waits instead of running on.
        assert!(of(&r.tick(2.0), "a", "steps").is_empty());
        assert!(of(&r.tick(2.0), "a", "steps").is_empty());
    }

    /// A name longer than a seat keeps still finds that seat again.
    #[test]
    fn a_long_name_rejoins_its_own_seat() {
        let mut r = room();
        let long = "a".repeat(30);
        join(&mut r, "q", 3).unwrap();
        r.join(Player::new("p", ""), &json!({ "name": long, "seats": 3 })).unwrap();
        join(&mut r, "s", 3).unwrap();
        r.leave("q");
        r.leave("p");
        let back = r.join(Player::new("p2", ""), &json!({ "name": long })).unwrap();
        assert_eq!(frames(&back, "p2")[0]["seat"], 1, "its own seat, not the first empty one");
    }

    #[test]
    fn hashes_far_in_the_future_are_not_kept() {
        let mut r = started();
        r.message("a", json!({ "t": "in", "s": 1, "i": [0], "hash": { "s": 1 + MAX_AHEAD, "h": "ab" } }));
        assert!(r.room::<LockstepRoom>().unwrap().hashes.is_empty());
    }

    #[test]
    fn inputs_far_in_the_future_are_refused() {
        let mut r = room();
        join(&mut r, "a", 1).unwrap();
        let out = r.message("a", json!({ "t": "in", "s": 1 + MAX_AHEAD, "i": [1] }));
        assert!(of(&out, "a", "steps").is_empty());
        let out = r.message("a", json!({ "t": "in", "s": 1, "i": [1] }));
        assert_eq!(of(&out, "a", "steps")[0]["i"], json!([[1]]));
    }
}
