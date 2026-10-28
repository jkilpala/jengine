//! **What the framework is for**, played over a real socket against the
//! `zones` example: one player, through a door into another room, across a
//! dropped connection, and across a restart of the server - the same player
//! each time.

mod common;
#[path = "../examples/zones/world.rs"]
mod world;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Client, hello_when, http_get, stats};
use jengine_server::{
    Config, JoinRequest, KindOptions, MemoryStore, Player, Room, RoomCtx, Running, Server,
    SqliteStore, Store,
};
use serde_json::{Value, json};

fn zones(store: Arc<dyn Store>) -> Running {
    Server::new()
        .store(store)
        .relay()
        .config(Config { autosave_secs: 0.2, ..Default::default() })
        // Its `/stats` is read with no token, as an autoscaler might.
        .public_stats()
        .room("zone", KindOptions::ticking(30.0).max_players(500).persistent(), world::Zone::new)
        // The same zones, holding a dropped player's seat for a moment.
        .room("held", KindOptions::ticking(30.0).persistent().grace(1.5), world::Zone::new)
        .channels(|ask| !ask.channel.starts_with("staff"))
        .room("bomb", KindOptions::default(), |_| Bomb)
        .room("gate", KindOptions::default(), |_| Gate)
        .start("127.0.0.1:0")
        .unwrap()
}

fn memory() -> Arc<dyn Store> {
    Arc::new(MemoryStore::new())
}

/// Everything a client has been told is in view, by entity id - what a game
/// client would be drawing.
fn seen(frames: &[Value]) -> BTreeMap<u64, Value> {
    let mut world = BTreeMap::new();
    for f in frames {
        if f["t"] == "joined" {
            world.clear(); // a new room is a new world
        }
        if f["t"] != "sync" {
            continue;
        }
        for e in f["enter"].as_array().into_iter().flatten() {
            world.insert(e["id"].as_u64().unwrap(), e.clone());
        }
        for m in f["move"].as_array().into_iter().flatten() {
            if let Some(known) = world.get_mut(&m[0].as_u64().unwrap()) {
                known["x"] = m[1].clone();
                known["y"] = m[2].clone();
            }
        }
        for e in f["update"].as_array().into_iter().flatten() {
            if let Some(known) = world.get_mut(&e["id"].as_u64().unwrap()) {
                for (k, v) in e.as_object().unwrap() {
                    known[k] = v.clone();
                }
            }
        }
        for id in f["leave"].as_array().into_iter().flatten() {
            world.remove(&id.as_u64().unwrap());
        }
    }
    world
}

#[test]
fn two_players_in_a_zone_see_each_other_move() {
    let server = zones(memory());
    let mut ada = Client::connect(&server.url());
    let mut bo = Client::connect(&server.url());
    ada.hello("Ada", "");
    bo.hello("Bo", "");
    ada.send(json!({ "t": "join", "kind": "zone", "room": "meadow" }));
    let ada_id = ada.wait("joined")["you"].as_u64().unwrap();
    bo.send(json!({ "t": "join", "kind": "zone", "room": "meadow" }));
    bo.wait("joined");
    bo.until("ada and the coins in view", |f| {
        let w = seen(f);
        w.get(&ada_id).is_some_and(|e| e["name"] == "Ada")
            && w.values().filter(|e| e["kind"] == "coin").count() == 3
    });

    ada.send(json!({ "t": "send", "body": { "go": [500, 100] } }));
    bo.until("ada to arrive", |f| seen(f)[&ada_id]["y"] == 100.0);
    assert_eq!(seen(&bo.frames)[&ada_id]["x"], 500.0);
    let steps = bo
        .frames
        .iter()
        .filter(|f| f["move"].as_array().is_some_and(|u| u.iter().any(|m| m[0] == ada_id)))
        .count();
    assert!(steps > 5, "the server walked her there ({steps} steps); nobody teleported");

    ada.conn.disconnect();
    bo.until("ada to leave bo's view", |f| !seen(f).contains_key(&ada_id));
    server.stop();
}

#[test]
fn a_player_walks_through_a_door_and_is_remembered_after_a_drop() {
    let server = zones(memory());
    let mut ada = Client::connect(&server.url());
    let token = ada.hello("Ada", "")["token"].as_str().unwrap().to_string();
    ada.send(json!({ "t": "join", "kind": "zone", "room": "meadow" }));
    ada.wait("joined");

    // East along the middle: over a coin, then through the door.
    ada.send(json!({ "t": "send", "body": { "go": [1010, 300] } }));
    ada.until("the next zone", |f| f.iter().any(|x| x["t"] == "joined" && x["room"] == "cave"));
    let joined = ada.last("joined").unwrap();
    assert_eq!(joined["coins"], 1.0, "the coin picked up in the meadow came through the door");
    let me = joined["you"].as_u64().unwrap();
    ada.until("herself, in the cave", |f| seen(f).contains_key(&me));
    assert_eq!(seen(&ada.frames)[&me]["x"], 40.0, "just inside the cave's west door");

    ada.send(json!({ "t": "send", "body": { "go": [100, 300] } }));
    ada.until("to have walked", |f| seen(f).get(&me).is_some_and(|e| e["x"] == 100.0));
    ada.conn.disconnect();

    // A new connection, the same token: the same player, where she was.
    let (_again, welcome) = hello_when(&server.url(), &token, |w| w["state"]["x"] == 100.0);
    assert_eq!(welcome["name"], "Ada");
    assert_eq!(welcome["state"]["zone"], "cave");
    assert_eq!(welcome["state"]["coins"], 1.0);
    assert_eq!(welcome["state"]["x"], 100.0);
    assert_eq!(welcome["room"], json!({ "kind": "zone", "id": "cave" }), "and where to go back to");
    server.stop();
}

#[test]
fn a_player_outlives_the_server() {
    let dir = std::env::temp_dir().join(format!("jengine-server-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("world.db");
    let _ = std::fs::remove_file(&db);

    let server = zones(Arc::new(SqliteStore::open(&db).unwrap()));
    let mut ada = Client::connect(&server.url());
    let token = ada.hello("Ada", "")["token"].as_str().unwrap().to_string();
    ada.send(json!({ "t": "join", "kind": "zone", "room": "meadow" }));
    ada.wait("joined");
    ada.send(json!({ "t": "send", "body": { "go": [560, 300] } }));
    ada.wait("coins");
    // Stopped with her still connected and in the room: a deploy.
    server.stop();

    let server = zones(Arc::new(SqliteStore::open(&db).unwrap()));
    let mut ada = Client::connect(&server.url());
    let welcome = ada.hello("", &token);
    assert_eq!(welcome["state"]["coins"], 1.0);
    assert_eq!(welcome["state"]["zone"], "meadow");
    assert_eq!(welcome["room"]["id"], "meadow");
    server.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn signing_in_twice_puts_the_first_connection_out() {
    let server = zones(memory());
    let mut first = Client::connect(&server.url());
    let token = first.hello("Ada", "")["token"].as_str().unwrap().to_string();
    first.send(json!({ "t": "join", "kind": "zone", "room": "meadow" }));
    first.wait("joined");
    first.send(json!({ "t": "send", "body": { "go": [560, 300] } }));
    first.wait("coins");

    let mut second = Client::connect(&server.url());
    let welcome = second.hello("", &token);
    assert_eq!(welcome["state"]["coins"], 1.0, "loaded after the first connection saved");
    assert_eq!(first.wait("error")["why"], "signed in somewhere else");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let s = stats(server.addr);
        if s["connections"] == 1 && s["players"] == 1 {
            break;
        }
        assert!(Instant::now() < deadline, "the first connection is still open: {s}");
    }
    server.stop();
}

#[test]
fn stats_and_health_answer_beside_the_socket() {
    let server = zones(memory());
    let mut ada = Client::connect(&server.url());
    ada.hello("Ada", "");
    ada.send(json!({ "t": "join", "kind": "zone", "room": "meadow" }));
    ada.wait("joined");
    let s = stats(server.addr);
    assert_eq!(s["connections"], 1);
    assert_eq!(s["kinds"]["zone"], json!({ "rooms": 1, "players": 1 }));
    assert_eq!(http_get(server.addr, "/healthz"), "ok");
    server.stop();
}

#[test]
fn a_zone_that_does_not_exist_is_refused_and_leaves_no_room_behind() {
    let server = zones(memory());
    let mut ada = Client::connect(&server.url());
    ada.hello("Ada", "");
    ada.send(json!({ "t": "join", "kind": "zone", "room": "atlantis" }));
    assert_eq!(ada.wait("error")["why"], "there is no zone called 'atlantis'");
    ada.frames.clear();
    ada.send(json!({ "t": "join", "kind": "castle" }));
    assert_eq!(ada.wait("error")["why"], "no room kind 'castle'");
    let deadline = Instant::now() + Duration::from_secs(5);
    while stats(server.addr)["rooms"] != 0 {
        assert!(Instant::now() < deadline, "the refused room is still open");
    }
    server.stop();
}

/// A room whose code is broken: it marks the player, then panics when spoken to.
struct Bomb;

impl Room for Bomb {
    fn on_join(&mut self, _: &mut RoomCtx, player: &mut Player, _: &Value) -> Result<Value, String> {
        player.set("armed", true);
        Ok(Value::Null)
    }

    fn on_message(&mut self, ctx: &mut RoomCtx, player: &mut Player, body: Value) {
        if body == "door" {
            return ctx.transfer(&player.id, JoinRequest::named("zone", "nowhere"));
        }
        panic!("a bug in a game's room (this panic is the test)");
    }
}

#[test]
fn a_room_that_panics_loses_nobody() {
    let server = zones(memory());
    let mut ada = Client::connect(&server.url());
    let token = ada.hello("Ada", "")["token"].as_str().unwrap().to_string();
    ada.send(json!({ "t": "join", "kind": "bomb", "room": "b1" }));
    ada.wait("joined");
    ada.send(json!({ "t": "send", "body": "boom" }));
    ada.wait("closed");

    // The connection is alive, the server is alive, and the player kept
    // what the room had written.
    ada.send(json!({ "t": "join", "kind": "zone", "room": "meadow" }));
    ada.until("the meadow", |f| f.iter().any(|x| x["t"] == "joined" && x["room"] == "meadow"));
    ada.conn.disconnect();
    let (_again, welcome) = hello_when(&server.url(), &token, |w| w["state"]["zone"] == "meadow");
    assert_eq!(welcome["state"]["armed"], true);
    assert_eq!(welcome["state"]["zone"], "meadow");
    server.stop();
}

/// A room that only routes: every arrival is sent on to the meadow.
struct Gate;

impl Room for Gate {
    fn on_join(&mut self, ctx: &mut RoomCtx, player: &mut Player, _: &Value) -> Result<Value, String> {
        ctx.send(&player.id, json!({ "t": "gate_says" }));
        ctx.transfer(&player.id, JoinRequest::named("zone", "meadow"));
        Ok(json!({ "passing": true }))
    }
}

/// S26: routing a player on arrival used to send the asked-for room's
/// `joined` first, then the one they landed in - and every routing world
/// marked the first and had its client ignore it. Now the room passed
/// through is never seen.
#[test]
fn a_player_routed_on_arrival_is_joined_once_where_they_land() {
    let server = zones(memory());
    let mut ada = Client::connect(&server.url());
    ada.hello("Ada", "");
    ada.send(json!({ "t": "join", "kind": "gate", "room": "front" }));
    ada.until("the meadow", |f| f.iter().any(|x| x["t"] == "joined" && x["room"] == "meadow"));
    // A little longer, for anything that might trail behind.
    let until = std::time::Instant::now() + std::time::Duration::from_millis(200);
    while std::time::Instant::now() < until {
        ada.pump();
    }
    let joined: Vec<&Value> = ada.frames.iter().filter(|f| f["t"] == "joined").collect();
    assert_eq!(joined.len(), 1, "one joined, for where she is: {joined:?}");
    assert_eq!(joined[0]["kind"], "zone");
    assert!(ada.frames.iter().all(|f| f["t"] != "gate_says" && f["t"] != "left"), "{:?}", ada.frames);
    server.stop();
}

#[test]
fn a_transfer_to_nowhere_leaves_the_player_in_no_room_and_says_so() {
    let server = zones(memory());
    let mut ada = Client::connect(&server.url());
    ada.hello("Ada", "");
    ada.send(json!({ "t": "join", "kind": "bomb", "room": "b2" }));
    ada.wait("joined");
    ada.send(json!({ "t": "send", "body": "door" }));
    assert_eq!(ada.wait("error")["why"], "there is no zone called 'nowhere'");
    ada.wait("left");
    ada.frames.clear();
    ada.send(json!({ "t": "send", "body": "hello?" }));
    assert_eq!(ada.wait("error")["why"], "join a room first");
    server.stop();
}

/// A connection that blinks is not a player who left: within the grace
/// period they are put back where they were, and nobody saw them go.
#[test]
fn a_dropped_player_keeps_their_seat_for_a_while() {
    let server = zones(memory());
    let mut ada = Client::connect(&server.url());
    let token = ada.hello("Ada", "")["token"].as_str().unwrap().to_string();
    ada.send(json!({ "t": "join", "kind": "held", "room": "meadow" }));
    let ada_id = ada.wait("joined")["you"].as_u64().unwrap();
    let mut bo = Client::connect(&server.url());
    bo.hello("Bo", "");
    bo.send(json!({ "t": "join", "kind": "held", "room": "meadow" }));
    let bo_id = bo.wait("joined")["you"].as_u64().unwrap();
    ada.send(json!({ "t": "send", "body": { "go": [500, 100] } }));
    bo.until("ada to arrive", |f| seen(f).get(&ada_id).is_some_and(|e| e["y"] == 100.0));

    ada.conn.disconnect();
    std::thread::sleep(Duration::from_millis(500));
    bo.pump();
    assert!(seen(&bo.frames).contains_key(&ada_id), "her seat is held: she is still standing there");

    // Back, on a new connection that knows nothing.
    let mut again = Client::connect(&server.url());
    again.hello("", &token);
    let joined = again.wait("joined");
    assert_eq!((&joined["resumed"], &joined["room"]), (&json!(true), &json!("meadow")));
    assert_eq!(again.wait("state")["state"]["y"], 100.0, "and her state as the room has it, not as it was last saved");
    assert_eq!(joined["you"], ada_id, "the same avatar, not a new one");
    let order: Vec<&str> = again.frames.iter().filter_map(|f| f["t"].as_str()).take(2).collect();
    assert_eq!(order, ["welcome", "joined"]);
    again.until("the whole view, sent afresh", |f| {
        let w = seen(f);
        w.get(&ada_id).is_some_and(|e| e["y"] == 100.0) && w.contains_key(&bo_id)
    });

    // A client that asks for its room again - as one that reconnects does -
    // is not thrown out and let back in.
    again.send(json!({ "t": "join", "kind": "held", "room": "meadow" }));
    again.send(json!({ "t": "send", "body": { "go": [500, 130] } }));
    again.until("to walk on", |f| seen(f).get(&ada_id).is_some_and(|e| e["y"] == 130.0));
    assert_eq!(again.count("joined"), 1);
    let ever_left = bo.frames.iter().any(|f| f["leave"].as_array().is_some_and(|l| l.contains(&json!(ada_id))));
    assert!(!ever_left, "Bo never saw her leave");

    // Gone for longer than the grace period: now she has left, and what she
    // had is saved, with the room to come back to.
    again.conn.disconnect();
    bo.until("her seat to be given up", |f| !seen(f).contains_key(&ada_id));
    let (_back, welcome) = hello_when(&server.url(), &token, |w| w["state"]["y"] == 130.0);
    assert_eq!(welcome["state"]["y"], 130.0);
    assert_eq!(welcome["room"], json!({ "kind": "held", "id": "meadow" }));
    server.stop();
}

/// A channel is beside the room: joined once, kept through every room.
#[test]
fn channels_carry_talk_beside_the_rooms() {
    let server = zones(memory());
    let mut ada = Client::connect(&server.url());
    let mut bo = Client::connect(&server.url());
    let ada_player = ada.hello("Ada", "")["player"].clone();
    bo.hello("Bo", "");
    ada.send(json!({ "t": "sub", "channel": "guild:7" }));
    let subbed = ada.wait("subbed");
    assert_eq!(subbed["channel"], "guild:7");
    assert_eq!(subbed["members"], json!([{ "player": ada_player, "name": "Ada" }]), "who is in it, herself included");
    // Something said before Bo arrives, which he should not be met without.
    ada.send(json!({ "t": "say", "channel": "guild:7", "body": "first!" }));
    ada.wait("said");
    ada.frames.retain(|f| f["t"] != "said");
    bo.send(json!({ "t": "sub", "channel": "guild:7" }));
    let subbed = bo.wait("subbed");
    assert_eq!(subbed["members"].as_array().unwrap().len(), 2);
    assert_eq!(subbed["recent"][0]["body"], "first!", "the last things said, for the one arriving");
    assert_eq!(ada.wait("presence")["joined"]["name"], "Bo", "and those already there are told who came");
    // Ada is in a zone; Bo is in no room at all. The channel does not care.
    ada.send(json!({ "t": "join", "kind": "zone", "room": "meadow" }));
    ada.wait("joined");
    ada.send(json!({ "t": "say", "channel": "guild:7", "body": "anyone near the cave?" }));
    let heard = bo.wait("said");
    assert_eq!(heard, json!({ "t": "said", "channel": "guild:7", "from": ada_player, "name": "Ada", "body": "anyone near the cave?" }));
    assert_eq!(ada.wait("said"), heard, "the speaker hears it too, in the same order as everyone");

    // Through a door, and still in the channel.
    ada.send(json!({ "t": "send", "body": { "go": [1010, 300] } }));
    ada.until("the cave", |f| f.iter().any(|x| x["t"] == "joined" && x["room"] == "cave"));
    bo.send(json!({ "t": "say", "channel": "guild:7", "body": { "emote": "wave" } }));
    ada.until("Bo's wave", |f| f.iter().any(|x| x["t"] == "said" && x["body"]["emote"] == "wave"));

    // What is refused, and why.
    let mut refusals = |frame: Value| {
        bo.frames.retain(|f| f["t"] != "error");
        bo.send(frame);
        bo.wait("error")["why"].as_str().unwrap().to_string()
    };
    assert_eq!(refusals(json!({ "t": "say", "channel": "lobby", "body": 1 })), "join that channel first");
    assert_eq!(refusals(json!({ "t": "sub", "channel": "staff-only" })), "you may not join that channel");
    assert_eq!(refusals(json!({ "t": "say", "channel": "guild:7", "body": "x".repeat(3000) })), "that message is too large");
    let mut guest = Client::connect(&server.url());
    guest.send(json!({ "t": "sub", "channel": "guild:7" }));
    assert_eq!(guest.wait("error")["why"], "sign in to use channels");

    // Somebody who has gone is not spoken to, and the others are told.
    bo.conn.disconnect();
    ada.until("to hear that Bo left", |f| f.iter().any(|x| x["t"] == "presence" && x["left"]["name"] == "Bo"));
    ada.frames.clear();
    ada.send(json!({ "t": "say", "channel": "guild:7", "body": "hello?" }));
    assert_eq!(ada.wait("said")["body"], "hello?");
    server.stop();

    let plain = Server::new().relay().start("127.0.0.1:0").unwrap();
    let mut c = Client::connect(&plain.url());
    c.hello("Cy", "");
    c.send(json!({ "t": "sub", "channel": "guild:7" }));
    assert_eq!(c.wait("error")["why"], "this server has no channels");
    plain.stop();
}

/// A room can speak in a channel, and what was said is still there after
/// the server comes back.
#[test]
fn a_room_speaks_in_a_channel_and_the_backlog_outlives_a_restart() {
    use jengine_server::{Player, Room, RoomCtx};
    struct Herald;
    impl Room for Herald {
        fn on_join(&mut self, ctx: &mut RoomCtx, player: &mut Player, _: &Value) -> Result<Value, String> {
            ctx.say("town", json!({ "arrived": player.name }));
            Ok(Value::Null)
        }
    }
    let store: Arc<dyn Store> = memory();
    let herald = |store: Arc<dyn Store>| {
        Server::new()
            .store(store)
            .open_channels()
            .room("square", KindOptions::default().persistent(), |_| Herald)
            .start("127.0.0.1:0")
            .unwrap()
    };
    let server = herald(store.clone());
    let mut ada = Client::connect(&server.url());
    ada.hello("Ada", "");
    ada.send(json!({ "t": "sub", "channel": "town" }));
    ada.wait("subbed");
    let mut bo = Client::connect(&server.url());
    bo.hello("Bo", "");
    bo.send(json!({ "t": "join", "kind": "square", "room": "main" }));
    bo.wait("joined");
    let heard = ada.wait("said");
    assert_eq!(heard["from"], "", "a room, not a player");
    assert_eq!((heard["room"].clone(), heard["body"].clone()), (json!({ "kind": "square", "id": "main" }), json!({ "arrived": "Bo" })));
    drop(ada);
    drop(bo);
    server.stop();

    // Back on the same store: the town remembers.
    let server = herald(store);
    let mut cy = Client::connect(&server.url());
    cy.hello("Cy", "");
    cy.send(json!({ "t": "sub", "channel": "town" }));
    let subbed = cy.wait("subbed");
    assert_eq!(subbed["recent"][0]["body"], json!({ "arrived": "Bo" }), "{subbed}");
    server.stop();
}

/// Accounts: a name and a password are the player, from any device - and a
/// socket is not a password-guessing machine.
#[test]
fn an_account_signs_in_from_anywhere_and_guessing_is_cut_short() {
    use jengine_server::PasswordAuth;
    let server = Server::new()
        .relay()
        .auth(PasswordAuth::new().iterations(50))
        .start("127.0.0.1:0")
        .unwrap();
    let mut first = Client::connect(&server.url());
    first.send(json!({ "t": "hello", "account": "Ada", "password": "correct horse", "register": true }));
    let welcome = first.wait("welcome");
    let player = welcome["player"].clone();
    assert_eq!(welcome["name"], "Ada");

    // Another device: no token, only what she remembers.
    let mut second = Client::connect(&server.url());
    second.send(json!({ "t": "hello", "account": "ada", "password": "correct horse" }));
    assert_eq!(second.wait("welcome")["player"], player);
    assert_eq!(first.wait("error")["why"], "signed in somewhere else", "one connection per player, still");

    // She changes her password, over the wire: the session she is on is
    // replaced, and the old password is no more.
    second.send(json!({ "t": "account", "change_password": { "old": "correct horse", "new": "battery staple" } }));
    let changed = second.wait("account");
    assert_eq!(changed["ok"], true);
    let mut third = Client::connect(&server.url());
    third.send(json!({ "t": "hello", "token": changed["token"] }));
    assert_eq!(third.wait("welcome")["player"], player);
    let mut stale = Client::connect(&server.url());
    stale.send(json!({ "t": "hello", "account": "ada", "password": "correct horse" }));
    assert_eq!(stale.wait("error")["why"], "wrong account or password");

    // A guesser gets five tries on a connection, each told nothing useful.
    let mut guesser = Client::connect(&server.url());
    for n in 0..5 {
        guesser.send(json!({ "t": "hello", "account": "ada", "password": format!("guess number {n}") }));
    }
    guesser.until("five refusals", |f| f.iter().filter(|x| x["t"] == "error").count() == 5);
    let deadline = Instant::now() + Duration::from_secs(5);
    while guesser.conn.is_connected() {
        guesser.pump();
        assert!(Instant::now() < deadline, "the guessing connection is still open");
        std::thread::sleep(Duration::from_millis(10));
    }
    server.stop();
}

/// A ticket signed by a game's own backend is all the server needs to see.
#[test]
fn a_ticket_from_another_backend_signs_a_player_in() {
    use jengine_server::SignedAuth;
    let server = Server::new().relay().auth(SignedAuth::new("shared-secret")).start("127.0.0.1:0").unwrap();
    let ticket = SignedAuth::ticket("shared-secret", "user-42", "Ada", 60);
    let mut ada = Client::connect(&server.url());
    let welcome = ada.hello("", &ticket);
    assert_eq!((&welcome["player"], &welcome["name"]), (&json!("user-42"), &json!("Ada")));

    let mut mallory = Client::connect(&server.url());
    mallory.send(json!({ "t": "hello", "token": SignedAuth::ticket("a-guess", "user-42", "Ada", 60) }));
    assert_eq!(mallory.wait("error")["why"], "that ticket is not ours");
    server.stop();
}

/// S11: a player a room lets go of at the very moment their connection
/// dies - a door walked through, then the line gone - keeps what they had:
/// the hand-back that finds no connection is saved by the room instead of
/// lost in a queue nobody reads.
#[test]
fn a_transfer_as_the_connection_dies_loses_nothing() {
    struct Hop;
    impl Room for Hop {
        fn on_join(&mut self, _: &mut jengine_server::RoomCtx, _: &mut Player, _: &Value) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn on_message(&mut self, ctx: &mut jengine_server::RoomCtx, player: &mut Player, _: Value) {
            let hops = player.num("hops", 0.0) + 1.0;
            player.set("hops", hops);
            let next = if ctx.id() == "a" { "b" } else { "a" };
            ctx.transfer(&player.id, jengine_server::JoinRequest::named("hop", next));
        }
    }
    let server = Server::new()
        .config(Config { autosave_secs: 60.0, ..Default::default() })
        .room("hop", KindOptions::default().persistent().grace(5.0), |_| Hop)
        .start("127.0.0.1:0")
        .unwrap();
    for round in 0..12 {
        let mut c = common::RawWs::open(server.addr, &[]);
        c.send(json!({ "t": "hello", "name": format!("hopper{round}") }));
        let token = c.wait("welcome")["token"].as_str().unwrap().to_string();
        c.send(json!({ "t": "join", "kind": "hop", "room": "a" }));
        c.wait("joined");
        // Through the door and gone, in one write.
        c.send_and_vanish(json!({ "t": "send", "body": "hop" }));
        // Back: the hop is in what the server has - the saved record, or
        // the room holding the seat says so after `joined`.
        let mut back = Client::connect(&server.url());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let hops = loop {
            let welcome = back.hello("", &token);
            back.pump();
            std::thread::sleep(std::time::Duration::from_millis(50));
            back.pump();
            let held = back.last("state").map(|s| s["state"]["hops"].clone());
            let hops = held.unwrap_or(welcome["state"]["hops"].clone());
            if hops == 1.0 || std::time::Instant::now() > deadline {
                break hops;
            }
            back = Client::connect(&server.url());
        };
        assert_eq!(hops, 1.0, "round {round}: the hop survived the dropped line");
    }
    server.stop();
}

/// A player who comes back while their room is busy - in the middle of a
/// long tick when the line drops - is put back in their held seat, not
/// refused as "already in this room". The old connection used to hand the
/// player on as soon as it had *asked* the room to hold the seat, so the new
/// one looked for a held seat before the room had got round to holding it.
/// A game's reconnect test met it under the load of the full suite.
#[test]
fn a_player_back_while_the_room_is_busy_gets_their_held_seat() {
    struct Slow;
    impl Room for Slow {
        fn on_join(&mut self, _: &mut RoomCtx, _: &mut Player, _: &Value) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn on_tick(&mut self, _: &mut RoomCtx, _: &mut jengine_server::Players, _: f64) {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    let server = Server::new()
        .room("slow", KindOptions::ticking(30.0).persistent().grace(5.0), |_| Slow)
        .start("127.0.0.1:0")
        .unwrap();
    for round in 0..3 {
        let mut ada = Client::connect(&server.url());
        let token = ada.hello(&format!("Ada{round}"), "")["token"].as_str().unwrap().to_string();
        ada.send(json!({ "t": "join", "kind": "slow", "room": "den" }));
        ada.wait("joined");
        ada.conn.disconnect();
        // Long enough for the server to see the line go and ask the room to
        // hold the seat; far shorter than the tick the room is in.
        std::thread::sleep(Duration::from_millis(60));
        let mut again = Client::connect(&server.url());
        again.hello("", &token);
        // As a reconnecting client does: ask for its room again.
        again.send(json!({ "t": "join", "kind": "slow", "room": "den" }));
        again.until("joined, or refused", |f| f.iter().any(|x| x["t"] == "joined" || x["t"] == "error"));
        assert_eq!(again.last("error"), None, "round {round}: refused");
        assert_eq!(again.wait("joined")["resumed"], true, "round {round}: back in the held seat");
    }
    server.stop();
}
