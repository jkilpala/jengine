//! **Two engine clients, one real relay, a real socket.**
//!
//! `jengine-net` had a WebSocket client for native and browser and nothing to
//! connect it to: its only exercise was a Test Lab scene "where every
//! connection is a keypress and there is no server, so nothing has ever sent
//! a real message". This is the first time it does. It uses the engine's own
//! client - `jengine_net::connect`, `NetConnection::pump` - not a test
//! client, because the point is that the thing a game ships with works.

use std::time::{Duration, Instant};

use jengine_net::{NetConnection, connect};
use serde_json::{Value, json};

fn start_relay() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || jengine_relay::serve_on(listener));
    format!("ws://{addr}")
}

/// Pump until `pred` says the frames so far are enough, or give up.
fn until(conn: &mut NetConnection, got: &mut Vec<Value>, pred: impl Fn(&[Value]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        conn.pump();
        for m in conn.drain() {
            if let Some(t) = m.as_text() {
                got.push(serde_json::from_str(t).expect("the relay sends JSON"));
            }
        }
        if pred(got) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out; state {:?}, frames so far {got:#?}", conn.state());
}

fn connected(url: &str) -> NetConnection {
    let mut c = NetConnection::new();
    connect(&mut c, url);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !c.is_connected() {
        c.pump();
        assert!(Instant::now() < deadline, "never connected: {:?}", c.state());
        std::thread::sleep(Duration::from_millis(5));
    }
    c
}

fn has(frames: &[Value], t: &str) -> bool {
    frames.iter().any(|f| f["t"] == t)
}

#[test]
fn two_clients_join_a_room_and_see_one_ordered_log() {
    let url = start_relay();
    let mut a = connected(&url);
    let mut b = connected(&url);
    let (mut fa, mut fb) = (Vec::new(), Vec::new());

    a.send_text(&json!({ "t": "join", "room": "shore", "name": "Ada", "seats": 2 }).to_string())
        .unwrap();
    until(&mut a, &mut fa, |f| has(f, "joined"));
    b.send_text(&json!({ "t": "join", "room": "shore", "name": "Bo", "seats": 2 }).to_string())
        .unwrap();
    until(&mut b, &mut fb, |f| has(f, "joined"));
    until(&mut a, &mut fa, |f| has(f, "peer_joined"));

    // Both send at once. Whatever order the relay picks, both must see it.
    a.send_text(&json!({ "t": "send", "body": { "move": 19 } }).to_string()).unwrap();
    b.send_text(&json!({ "t": "send", "body": { "move": 44 } }).to_string()).unwrap();
    let two_msgs = |f: &[Value]| f.iter().filter(|x| x["t"] == "msg").count() == 2;
    until(&mut a, &mut fa, two_msgs);
    until(&mut b, &mut fb, two_msgs);

    let order = |f: &[Value]| -> Vec<(i64, i64)> {
        f.iter()
            .filter(|x| x["t"] == "msg")
            .map(|x| (x["seq"].as_i64().unwrap(), x["body"]["move"].as_i64().unwrap()))
            .collect()
    };
    assert_eq!(order(&fa), order(&fb), "both clients must see the same moves in the same order");
    assert_eq!(order(&fa).iter().map(|(s, _)| *s).collect::<Vec<_>>(), vec![0, 1]);
}

#[test]
fn a_client_that_drops_rejoins_to_the_whole_log() {
    let url = start_relay();
    let mut a = connected(&url);
    let mut fa = Vec::new();
    a.send_text(&json!({ "t": "join", "room": "shore", "name": "Ada", "seats": 2 }).to_string())
        .unwrap();
    until(&mut a, &mut fa, |f| has(f, "joined"));

    {
        let mut b = connected(&url);
        let mut fb = Vec::new();
        b.send_text(&json!({ "t": "join", "room": "shore", "name": "Bo", "seats": 2 }).to_string())
            .unwrap();
        until(&mut b, &mut fb, |f| has(f, "joined"));
        for m in [19, 20, 21] {
            b.send_text(&json!({ "t": "send", "body": { "move": m } }).to_string()).unwrap();
        }
        until(&mut b, &mut fb, |f| f.iter().filter(|x| x["t"] == "msg").count() == 3);
        b.disconnect();
    }
    until(&mut a, &mut fa, |f| has(f, "peer_left"));

    let mut b = connected(&url);
    let mut fb = Vec::new();
    b.send_text(&json!({ "t": "join", "room": "shore", "name": "Bo", "seats": 2 }).to_string())
        .unwrap();
    until(&mut b, &mut fb, |f| has(f, "joined"));
    let joined = fb.iter().find(|f| f["t"] == "joined").unwrap();
    assert_eq!(joined["seat"], 1, "the seat they left is theirs again");
    let moves: Vec<i64> = joined["log"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["body"]["move"].as_i64().unwrap())
        .collect();
    assert_eq!(moves, vec![19, 20, 21], "and the log rebuilds the game to the move");
}
