//! A match: two players, three in a row, and a server that referees.
//!
//! This is the difference from a relay. A relay would pass a move on and
//! leave it to the clients to agree it was legal; here the room holds the
//! board, and a move that is out of turn or on a taken cell never happens.
//! A client can only *ask* (`{"place": 4}`).
//!
//! What the room sends:
//!
//! | Frame | When |
//! |---|---|
//! | `joined` with `seat`, `board`, `turn` | to a player who sits down (or comes back to a held seat) |
//! | `{"t":"start","names":[x, o]}` | both seats are taken |
//! | `{"t":"placed","seat","cell","board","turn"}` | a legal move |
//! | `{"t":"over","winner":seat or null,"forfeit":bool}` | three in a row, a full board, or somebody left |
//! | `{"t":"error","why"}` | to whoever asked for something that is not allowed |
//!
//! A win is written to the winner's state (`wins`), which the server saves:
//! it is still there in their next match, and the one after a restart.

use jengine_server::{Player, PlayerId, Room, RoomCtx};
use serde_json::{Value, json};

const LINES: [[usize; 3]; 8] =
    [[0, 1, 2], [3, 4, 5], [6, 7, 8], [0, 3, 6], [1, 4, 7], [2, 5, 8], [0, 4, 8], [2, 4, 6]];

#[derive(Default)]
pub struct Match {
    /// Who sits where, and what they are called.
    seats: [Option<(PlayerId, String)>; 2],
    /// 0 empty, 1 the first seat's mark, 2 the second's.
    board: [u8; 9],
    /// The seat to move.
    turn: usize,
    started: bool,
    over: bool,
}

impl Match {
    pub fn new() -> Self {
        Self::default()
    }

    fn seat_of(&self, player: &str) -> Option<usize> {
        self.seats.iter().position(|s| s.as_ref().is_some_and(|(id, _)| id == player))
    }

    fn view(&self, seat: usize) -> Value {
        json!({ "seat": seat, "board": self.board, "turn": self.turn, "started": self.started })
    }

    fn winner(&self) -> Option<usize> {
        LINES.iter().find_map(|l| {
            let mark = self.board[l[0]];
            (mark != 0 && l.iter().all(|c| self.board[*c] == mark)).then(|| mark as usize - 1)
        })
    }
}

impl Room for Match {
    fn on_join(&mut self, ctx: &mut RoomCtx, player: &mut Player, _options: &Value)
    -> Result<Value, String> {
        if self.over {
            return Err("that match is over".into());
        }
        let Some(seat) = self.seats.iter().position(Option::is_none) else {
            return Err("the match is full".into());
        };
        self.seats[seat] = Some((player.id.clone(), player.name.clone()));
        if self.seats.iter().all(Option::is_some) {
            self.started = true;
            let names: Vec<&str> = self.seats.iter().flatten().map(|(_, name)| name.as_str()).collect();
            // To the one already seated now, and to the joiner after `joined`.
            let start = json!({ "t": "start", "names": names });
            ctx.broadcast(start.clone());
            ctx.send(&player.id, start);
        }
        Ok(self.view(seat))
    }

    /// Back in a held seat: the board as it stands.
    fn on_rejoin(&mut self, _ctx: &mut RoomCtx, player: &mut Player) -> Value {
        self.seat_of(&player.id).map(|seat| self.view(seat)).unwrap_or(Value::Null)
    }

    fn on_message(&mut self, ctx: &mut RoomCtx, player: &mut Player, body: Value) {
        let Some(seat) = self.seat_of(&player.id) else { return };
        let refusal = match body["place"].as_u64().map(|c| c as usize).filter(|c| *c < 9) {
            None => Some("say {\"place\": 0..8}"),
            Some(_) if !self.started || self.over => Some("there is no game on"),
            Some(_) if seat != self.turn => Some("it is not your turn"),
            Some(cell) if self.board[cell] != 0 => Some("that cell is taken"),
            Some(_) => None,
        };
        if let Some(why) = refusal {
            return ctx.send(&player.id, json!({ "t": "error", "why": why }));
        }
        let cell = body["place"].as_u64().unwrap_or(0) as usize;
        self.board[cell] = seat as u8 + 1;
        self.turn = 1 - seat;
        ctx.broadcast(json!({ "t": "placed", "seat": seat, "cell": cell, "board": self.board, "turn": self.turn }));

        let winner = self.winner();
        if winner.is_some() || self.board.iter().all(|c| *c != 0) {
            self.over = true;
            if winner == Some(seat) {
                // The mover is the only one who can have just won, and theirs
                // is the state in hand.
                let wins = player.num("wins", 0.0) + 1.0;
                player.set("wins", wins);
            }
            ctx.broadcast(json!({ "t": "over", "winner": winner, "forfeit": false }));
        }
    }

    fn on_leave(&mut self, ctx: &mut RoomCtx, player: &mut Player) {
        let Some(seat) = self.seat_of(&player.id) else { return };
        self.seats[seat] = None;
        if self.started && !self.over {
            self.over = true;
            ctx.broadcast(json!({ "t": "over", "winner": 1 - seat, "forfeit": true }));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use jengine_server::{Effects, MemoryStore, RoomRunner};

    use super::*;

    fn empty() -> RoomRunner {
        RoomRunner::new("match", "m1", Box::new(Match::new()), Arc::new(MemoryStore::new()), 2).0
    }

    fn seated() -> RoomRunner {
        let mut room = empty();
        room.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        room.join(Player::new("b", "Bo"), &Value::Null).unwrap();
        room
    }

    fn frames(effects: &Effects, to: &str, t: &str) -> Vec<Value> {
        effects.frames.iter().filter(|(id, f)| id == to && f.to_json()["t"] == t).map(|(_, f)| f.to_json()).collect()
    }

    #[test]
    fn two_players_sit_down_and_both_hear_the_start() {
        let mut room = empty();
        let first = room.join(Player::new("a", "Ada"), &Value::Null).unwrap();
        assert_eq!(frames(&first, "a", "joined")[0]["seat"], 0);
        assert!(frames(&first, "a", "start").is_empty(), "one player is not a game");
        let second = room.join(Player::new("b", "Bo"), &Value::Null).unwrap();
        assert_eq!(frames(&second, "b", "joined")[0]["seat"], 1);
        for who in ["a", "b"] {
            assert_eq!(frames(&second, who, "start")[0]["names"], json!(["Ada", "Bo"]));
        }
        assert!(room.join(Player::new("c", "Cy"), &Value::Null).is_err());
    }

    #[test]
    fn the_server_referees() {
        let mut room = seated();
        let why = |e: &Effects, to: &str| frames(e, to, "error")[0]["why"].as_str().unwrap().to_string();
        assert_eq!(why(&room.message("b", json!({ "place": 4 })), "b"), "it is not your turn");
        assert_eq!(frames(&room.message("a", json!({ "place": 4 })), "b", "placed")[0]["board"][4], 1);
        assert_eq!(why(&room.message("b", json!({ "place": 4 })), "b"), "that cell is taken");
        assert!(why(&room.message("b", json!({ "place": 99 })), "b").contains("place"));
    }

    #[test]
    fn three_in_a_row_wins_and_the_win_is_the_players_to_keep() {
        let mut room = seated();
        // Ada: 0 1 2. Bo: 3 4.
        for (who, cell) in [("a", 0), ("b", 3), ("a", 1), ("b", 4)] {
            room.message(who, json!({ "place": cell }));
        }
        let last = room.message("a", json!({ "place": 2 }));
        assert_eq!(frames(&last, "b", "over")[0], json!({ "t": "over", "winner": 0, "forfeit": false }));
        assert_eq!(frames(&room.message("b", json!({ "place": 5 })), "b", "error")[0]["why"], "there is no game on");
        let left = room.leave("a");
        assert_eq!(left.released[0].0.num("wins", 0.0), 1.0, "written to her state, which the server saves");
    }

    #[test]
    fn leaving_a_game_in_progress_gives_it_to_the_other() {
        let mut room = seated();
        room.message("a", json!({ "place": 0 }));
        let left = room.leave("a");
        assert_eq!(frames(&left, "b", "over")[0], json!({ "t": "over", "winner": 1, "forfeit": true }));
    }
}
