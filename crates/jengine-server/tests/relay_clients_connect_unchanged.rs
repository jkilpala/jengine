//! **A game written for `jengine-relay` plays on `jengine-server` as it is.**
//!
//! These are the relay crate's own socket tests, frame for frame, pointed at
//! a server with [`Server::relay`]. If they pass, a game written for the relay does not know
//! which of the two it is talking to.

mod common;

use common::Client;
use jengine_server::Server;
use serde_json::{Value, json};

#[test]
fn two_clients_join_a_room_and_see_one_ordered_log() {
    let server = Server::new().relay().start("127.0.0.1:0").unwrap();
    let mut a = Client::connect(&server.url());
    let mut b = Client::connect(&server.url());

    a.send(json!({ "t": "join", "room": "shore", "name": "Ada", "seats": 2 }));
    assert_eq!(a.wait("joined")["seat"], 0);
    b.send(json!({ "t": "join", "room": "shore", "name": "Bo", "seats": 2 }));
    let joined = b.wait("joined");
    assert_eq!(joined["seat"], 1);
    assert_eq!(joined["peers"], json!([{ "seat": 0, "name": "Ada" }, { "seat": 1, "name": "Bo" }]));
    assert_eq!(a.wait("peer_joined")["name"], "Bo");
    assert_eq!(a.count("welcome"), 0, "a relay client is sent nothing it does not know");

    // Both send at once. Whatever order the server picks, both must see it.
    a.send(json!({ "t": "send", "body": { "move": 19 } }));
    b.send(json!({ "t": "send", "body": { "move": 44 } }));
    a.until("two moves", |f| f.iter().filter(|x| x["t"] == "msg").count() == 2);
    b.until("two moves", |f| f.iter().filter(|x| x["t"] == "msg").count() == 2);
    let order = |f: &[Value]| -> Vec<(i64, i64)> {
        f.iter()
            .filter(|x| x["t"] == "msg")
            .map(|x| (x["seq"].as_i64().unwrap(), x["body"]["move"].as_i64().unwrap()))
            .collect()
    };
    assert_eq!(order(&a.frames), order(&b.frames));
    assert_eq!(order(&a.frames).iter().map(|(s, _)| *s).collect::<Vec<_>>(), vec![0, 1]);
    server.stop();
}

#[test]
fn a_client_that_drops_rejoins_to_the_whole_log() {
    let server = Server::new().relay().start("127.0.0.1:0").unwrap();
    let mut a = Client::connect(&server.url());
    a.send(json!({ "t": "join", "room": "shore", "name": "Ada", "seats": 2 }));
    a.wait("joined");
    {
        let mut b = Client::connect(&server.url());
        b.send(json!({ "t": "join", "room": "shore", "name": "Bo", "seats": 2 }));
        b.wait("joined");
        for m in [19, 20, 21] {
            b.send(json!({ "t": "send", "body": { "move": m } }));
        }
        b.until("three moves", |f| f.iter().filter(|x| x["t"] == "msg").count() == 3);
        b.conn.disconnect();
    }
    a.wait("peer_left");

    let mut b = Client::connect(&server.url());
    b.send(json!({ "t": "join", "room": "shore", "name": "Bo", "seats": 2 }));
    let joined = b.wait("joined");
    assert_eq!(joined["seat"], 1, "the seat they left is theirs again");
    let moves: Vec<i64> =
        joined["log"].as_array().unwrap().iter().map(|m| m["body"]["move"].as_i64().unwrap()).collect();
    assert_eq!(moves, vec![19, 20, 21], "and the log rebuilds the game to the move");
    server.stop();
}

#[test]
fn a_third_player_and_a_stranger_are_told_why() {
    let server = Server::new().relay().start("127.0.0.1:0").unwrap();
    let mut clients: Vec<Client> = (0..3).map(|_| Client::connect(&server.url())).collect();
    clients[2].send(json!({ "t": "send", "body": 1 }));
    assert_eq!(clients[2].wait("error")["why"], "join a room first");
    for c in &mut clients[..2] {
        c.send(json!({ "t": "join", "room": "shore", "seats": 2 }));
        c.wait("joined");
    }
    clients[2].frames.clear();
    clients[2].send(json!({ "t": "join", "room": "shore", "seats": 2 }));
    assert_eq!(clients[2].wait("error")["why"], "the room is full");
    server.stop();
}
