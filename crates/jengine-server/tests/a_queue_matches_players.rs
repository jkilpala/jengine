//! **S14: a queue in front of a match**, over real sockets: two players of
//! close rating are sent from the queue into a duel of their own on the
//! connections they queued on; a third, far off, waits on in the queue.

mod common;

use common::Client;
use jengine_server::matchmaking::Matchmaker;
use jengine_server::{KindOptions, Player, Room, RoomCtx, Server};
use serde_json::{Value, json};

/// A duel that seats whoever the queue sends, and says who they meet.
struct Duel;

impl Room for Duel {
    fn on_join(&mut self, _: &mut RoomCtx, _: &mut Player, options: &Value) -> Result<Value, String> {
        Ok(json!({ "match": options["match"], "players": options["players"] }))
    }
}

#[test]
fn a_queue_sends_close_ratings_into_a_duel_of_their_own() {
    let server = Server::new()
        .room("duel", KindOptions::default().max_players(2), |_| Duel)
        .room("queue", KindOptions::ticking(10.0).max_players(1000).persistent(), |_| {
            Matchmaker::by_rating("duel", 2).window(100.0, 0.0)
        })
        .start("127.0.0.1:0")
        .unwrap();
    let queue = |name: &str, rating: u32| {
        let mut c = Client::connect(&server.url());
        c.hello(name, "");
        c.send(json!({ "t": "join", "kind": "queue", "room": "ranked", "options": { "rating": rating } }));
        c
    };
    let (mut ada, mut bo, mut cy) = (queue("Ada", 1500), queue("Bo", 1550), queue("Cy", 2400));
    let a = ada.wait("joined");
    assert_eq!(a["kind"], "queue", "first, the queue");
    ada.until("the duel", |f| f.iter().any(|x| x["t"] == "joined" && x["kind"] == "duel"));
    bo.until("the duel", |f| f.iter().any(|x| x["t"] == "joined" && x["kind"] == "duel"));
    let duel = |c: &Client| c.frames.iter().rev().find(|x| x["t"] == "joined" && x["kind"] == "duel").cloned().unwrap();
    let (da, db) = (duel(&ada), duel(&bo));
    assert_eq!(da["room"], db["room"], "one room for both");
    assert_eq!(da["match"], da["room"]);
    cy.until("the queue to say one waits", |f| f.iter().any(|x| x["t"] == "queued" && x["waiting"] == 1));
    assert!(cy.frames.iter().all(|x| !(x["t"] == "joined" && x["kind"] == "duel")), "Cy is nobody's match yet");
    server.stop();
}
