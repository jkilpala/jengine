//! **One world on several nodes** - a coordinator and two servers, each on
//! its own port, with players moved between them the way a client is: by a
//! `redirect` frame, a new connection, and the token.
//!
//! The nodes are in one process here; nothing they share is in memory. They
//! talk only through the coordinator, over HTTP, as two machines would.

mod common;
#[path = "../examples/zones/world.rs"]
mod world;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::Client;
use jengine_server::cluster::{self, RunningCoordinator};
use jengine_server::{ApiError, ClusterConfig, KindOptions, MemoryStore, Running, Server};
use serde_json::{Value, json};

const SECRET: &str = "cluster-test-secret";

/// One call to a coordinator, as a node makes it: signed as the node the
/// body names.
fn ask(coord: &RunningCoordinator, what: &str, body: Value) -> Value {
    let node = body["node"].as_str().unwrap_or("a-test").to_string();
    cluster::call_as(&coord.url(), SECRET, &node, what, body).unwrap()
}

fn coordinator() -> RunningCoordinator {
    cluster::coordinator("127.0.0.1:0", Arc::new(MemoryStore::new()), SECRET, 1.0).unwrap()
}

fn node(coord: &RunningCoordinator, name: &str) -> Running {
    let mut config = ClusterConfig::new(&coord.url(), SECRET, name);
    config.heartbeat_secs = 0.2;
    Server::new()
        .cluster(config)
        .open_channels()
        .admin_token("admin-token")
        .room("zone", KindOptions::ticking(30.0).max_players(100).persistent(), world::Zone::new)
        .room("held", KindOptions::ticking(30.0).persistent().grace(5.0), world::Zone::new)
        // The game's own routes, which see the whole cluster.
        .get("/api/players/{id}", |req, api| api.player(req.param("id")).ok_or_else(ApiError::not_found))
        .get("/api/status", |_req, api| Ok(json!({ "online": api.online(), "rooms": api.rooms() })))
        .get("/api/rooms/{kind}/{id}", |req, api| {
            api.room(req.param("kind"), req.param("id")).ok_or_else(ApiError::not_found)
        })
        .post("/api/grant/{id}", |req, api| {
            let state = api.edit_player(req.param("id"), |state| {
                state["coins"] = json!(state["coins"].as_f64().unwrap_or(0.0) + 5.0);
            })?;
            Ok(json!({ "coins": state["coins"] }))
        })
        .start("127.0.0.1:0")
        .unwrap()
}

/// A route on one node answers for a player and a room on another, and a
/// change made through it reaches the room that has the player.
#[test]
fn a_games_route_on_one_node_sees_a_player_on_another() {
    let coord = coordinator();
    let (a, b) = (node(&coord, "a"), node(&coord, "b"));
    let (mut cy, _, cy_player) = signed_in(&b, "Cy", "");
    join(&mut cy, "zone", "cave");
    cy.wait("joined");
    let player = cy_player.as_str().unwrap();
    let get = |path: &str| -> Result<Value, u16> {
        match ureq::get(&format!("http://{}{path}", a.addr)).call() {
            Ok(r) => Ok(r.into_json().unwrap()),
            Err(ureq::Error::Status(code, _)) => Err(code),
            Err(e) => panic!("{e}"),
        }
    };
    let card = get(&format!("/api/players/{player}")).unwrap();
    assert_eq!((card["online"].clone(), card["node"].clone(), card["room"]["id"].clone()), (json!(true), json!("b"), json!("cave")));
    assert_eq!(card["state"]["zone"], "cave", "the room's own copy of his state");
    let room = get("/api/rooms/zone/cave").unwrap();
    assert!(room["players"].as_array().unwrap().iter().any(|p| p["id"] == player), "{room}");
    assert_eq!(get("/api/rooms/zone/atlantis"), Err(404));
    let status = get("/api/status").unwrap();
    assert_eq!(status["online"], 1);
    assert!(status["rooms"].as_array().unwrap().iter().any(|r| r["id"] == "cave" && r["node"] == "b"), "{status}");

    // A grant from A: B's room has him, and tells him.
    let granted: Value = ureq::post(&format!("http://{}/api/grant/{player}", a.addr)).call().unwrap().into_json().unwrap();
    assert_eq!(granted["coins"], 5.0);
    assert_eq!(cy.wait("state")["state"]["coins"], 5.0);
    assert_eq!(get(&format!("/api/players/{player}")).unwrap()["state"]["coins"], 5.0);
    drop(cy);
    a.stop();
    b.stop();
}

/// Sign in and return (client, token, player id).
fn signed_in(server: &Running, name: &str, token: &str) -> (Client, String, Value) {
    let mut client = Client::connect(&server.url());
    let welcome = client.hello(name, token);
    (client, welcome["token"].as_str().unwrap().to_string(), welcome["player"].clone())
}

fn join(client: &mut Client, kind: &str, room: &str) {
    client.send(json!({ "t": "join", "kind": kind, "room": room }));
}

#[test]
fn two_nodes_are_one_world() {
    let coord = coordinator();
    let (a, b) = (node(&coord, "a"), node(&coord, "b"));

    // Each node opens the zone it is first asked for.
    let (mut ada, ada_token, ada_player) = signed_in(&a, "Ada", "");
    join(&mut ada, "zone", "meadow");
    ada.wait("joined");
    let (mut cy, _, _) = signed_in(&b, "Cy", "");
    join(&mut cy, "zone", "cave");
    cy.wait("joined");

    // Bo is connected to B and asks for the meadow, which is on A.
    let (mut bo, bo_token, bo_player) = signed_in(&b, "Bo", "");
    join(&mut bo, "zone", "meadow");
    let redirect = bo.wait("redirect");
    assert_eq!(redirect, json!({ "t": "redirect", "url": a.url(), "kind": "zone", "room": "meadow" }));
    assert_eq!(bo.count("joined"), 0);
    // He does what a client does with that: goes there, with his token.
    let (mut bo, _, same) = signed_in(&a, "", &bo_token);
    assert_eq!(same, bo_player, "the same player on the other node");
    join(&mut bo, "zone", "meadow");
    let bo_id = bo.wait("joined")["you"].clone();
    ada.until("Bo to walk into Ada's view of the meadow", |f| {
        f.iter().flat_map(|x| x["enter"].as_array().into_iter().flatten()).any(|e| e["id"] == bo_id)
    });

    // Ada walks through the door. The cave is on B: she is saved, and sent.
    ada.send(json!({ "t": "send", "body": { "go": [1010, 300] } }));
    let redirect = ada.wait("redirect");
    assert_eq!((&redirect["url"], &redirect["room"]), (&json!(b.url()), &json!("cave")));
    let (mut ada, _, same) = signed_in(&b, "", &ada_token);
    assert_eq!(same, ada_player);
    let welcome = ada.last("welcome").unwrap();
    assert_eq!(welcome["state"]["zone"], "cave", "what node A saved is what node B loaded");
    assert_eq!(welcome["state"]["coins"], 1.0, "with the coin she crossed on the way");
    assert_eq!(welcome["room"], json!({ "kind": "zone", "id": "cave" }));
    join(&mut ada, "zone", "cave");
    assert_eq!(ada.wait("joined")["coins"], 1.0);

    // A channel reaches across: Bo is on A, Cy on B.
    for c in [&mut bo, &mut cy] {
        c.send(json!({ "t": "sub", "channel": "guild:7" }));
        c.wait("subbed");
    }
    cy.send(json!({ "t": "say", "channel": "guild:7", "body": "from the cave" }));
    assert_eq!(bo.wait("said")["body"], "from the cave");
    assert_eq!(bo.last("said").unwrap()["name"], "Cy");
    cy.wait("said");
    std::thread::sleep(Duration::from_millis(300));
    cy.pump();
    assert_eq!(cy.count("said"), 1, "and the speaker hears it once, not twice");

    // So does a notice from one node's dashboard.
    let sent = ureq::post(&format!("http://{}/admin/api/notice", a.addr))
        .set("Authorization", "Bearer admin-token")
        .set("Content-Type", "application/json")
        .send_string(r#"{"text":"restart at noon"}"#);
    assert!(sent.is_ok());
    assert_eq!(cy.wait("notice")["text"], "restart at noon", "heard on the other node");
    assert_eq!(bo.wait("notice")["text"], "restart at noon");

    // And one player is one connection, whichever node the second one is on.
    let (_other, _, _) = signed_in(&a, "", &ada_token);
    assert_eq!(ada.wait("error")["why"], "signed in somewhere else");

    // The dashboard of either node shows the cluster.
    let overview: Value = ureq::get(&format!("http://{}/admin/api/overview", a.addr))
        .set("Authorization", "Bearer admin-token")
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    let nodes = overview["cluster"]["nodes"].as_array().unwrap();
    assert_eq!(nodes.iter().map(|n| n["node"].as_str().unwrap()).collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(nodes[0]["rooms"], json!(["zone/meadow"]));
    assert_eq!(nodes[1]["rooms"], json!(["zone/cave"]));

    a.stop();
    b.stop();
    coord.stop();
}

#[test]
fn a_node_that_goes_gives_its_rooms_to_whoever_is_left() {
    let coord = coordinator();
    let (a, b) = (node(&coord, "a"), node(&coord, "b"));
    let (mut ada, _, _) = signed_in(&a, "Ada", "");
    join(&mut ada, "zone", "meadow");
    ada.wait("joined");
    let (mut bo, _, _) = signed_in(&b, "Bo", "");
    join(&mut bo, "zone", "meadow");
    assert_eq!(bo.wait("redirect")["url"], a.url());

    // A stops. Stopped cleanly it gives the name back at once; killed, the
    // coordinator would notice within its timeout. Either way B can open it.
    a.stop();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        bo.frames.clear();
        join(&mut bo, "zone", "meadow");
        bo.until("an answer", |f| f.iter().any(|x| x["t"] == "joined" || x["t"] == "redirect"));
        if bo.count("joined") == 1 {
            break;
        }
        assert!(Instant::now() < deadline, "B was never allowed to open the meadow");
        std::thread::sleep(Duration::from_millis(200));
    }
    b.stop();
    coord.stop();
}

/// A seat held on one node is still that player's seat when their new
/// connection lands on another: they are sent back to it.
#[test]
fn a_held_seat_brings_the_player_back_to_its_node() {
    let coord = coordinator();
    let (a, b) = (node(&coord, "a"), node(&coord, "b"));
    let (mut ada, token, _) = signed_in(&a, "Ada", "");
    join(&mut ada, "held", "meadow");
    let you = ada.wait("joined")["you"].clone();
    ada.conn.disconnect();
    std::thread::sleep(Duration::from_millis(300));

    // A load balancer puts her new connection on B.
    let mut wrong = Client::connect(&b.url());
    wrong.send(json!({ "t": "hello", "token": token }));
    assert_eq!(wrong.wait("redirect"), json!({ "t": "redirect", "url": a.url() }));
    assert_eq!(wrong.count("welcome"), 0, "B never loaded her: A has the only copy");

    let mut back = Client::connect(&a.url());
    back.hello("", &token);
    let joined = back.wait("joined");
    assert_eq!((&joined["resumed"], &joined["you"]), (&json!(true), &you));
    a.stop();
    b.stop();
    coord.stop();
}

/// A room is not placed on a machine that is nearly out of CPU or memory
/// while another is not - whatever their player counts say.
#[test]
fn a_room_is_not_placed_on_a_loaded_machine() {
    let coord = coordinator();
    // Three nodes, as they would report themselves: the asker busy with
    // players, one quiet but loaded, one with a few players and room to spare.
    let register = |name: &str, stats: Value| {
        ask(&coord, "register", json!({ "node": name, "public_url": format!("ws://{name}"), "internal_url": format!("http://{name}"), "kinds": ["zone"], "stats": stats }));
    };
    register("asker", json!({ "players": 100, "machine_cpu": 20, "memory_percent": 30 }));
    register("quiet-but-loaded", json!({ "players": 0, "machine_cpu": 95, "memory_percent": 30 }));
    register("calm", json!({ "players": 10, "machine_cpu": 20, "memory_percent": 30 }));
    let claim = ask(&coord, "claim", json!({ "node": "asker", "kind": "zone", "room": "cave" }));
    assert_eq!(claim["node"], "calm", "{claim}");
    // The asker's own machine loaded: the room goes elsewhere even with few players.
    register("asker", json!({ "players": 1, "machine_cpu": 20, "memory_percent": 97 }));
    let claim = ask(&coord, "claim", json!({ "node": "asker", "kind": "zone", "room": "meadow" }));
    assert_eq!(claim["node"], "calm", "{claim}");
}

#[test]
fn a_node_with_the_wrong_secret_gets_nothing() {
    let coord = coordinator();
    let refused = ureq::post(&format!("{}/c/store", coord.url()))
        .set("Authorization", "Bearer not-the-secret")
        .set("Content-Type", "application/json")
        .send_string(r#"{"op":"keys","ns":"player"}"#);
    assert!(matches!(refused, Err(ureq::Error::Status(401, _))));
    // K6: the secret itself is no longer a way in - only a signature is.
    let bearer = ureq::post(&format!("{}/c/store", coord.url()))
        .set("Authorization", &format!("Bearer {SECRET}"))
        .set("Content-Type", "application/json")
        .send_string(r#"{"op":"keys","ns":"player"}"#);
    assert!(matches!(bearer, Err(ureq::Error::Status(401, _))), "the secret sent as a bearer opens nothing");
    // A node signing with another secret's key is nobody.
    assert!(cluster::call_as(&coord.url(), "another-cluster-secret", "a", "nodes", json!({})).is_err());
    assert!(cluster::call_as(&coord.url(), SECRET, "a", "nodes", json!({})).is_ok());
    assert!(cluster::coordinator("127.0.0.1:0", Arc::new(MemoryStore::new()), "short", 1.0).is_err());
    coord.stop();
}

/// The coordinator is one process everybody depends on, so it has to be
/// able to stop and come back without costing anyone anything.
#[test]
fn the_coordinator_can_stop_and_come_back() {
    let store: Arc<dyn jengine_server::Store> = Arc::new(MemoryStore::new());
    let coord = cluster::coordinator("127.0.0.1:0", store.clone(), SECRET, 1.0).unwrap();
    let addr = coord.addr.to_string();
    let (a, b) = (node(&coord, "a"), node(&coord, "b"));
    let (mut ada, ada_token, _) = signed_in(&a, "Ada", "");
    join(&mut ada, "zone", "meadow");
    let me = ada.wait("joined")["you"].clone();
    let (bo, bo_token, bo_player) = signed_in(&b, "Bo", "");
    drop(bo);

    coord.stop();

    // The players who are on, play on: the room is on its node.
    ada.send(json!({ "t": "send", "body": { "go": [560, 300] } }));
    assert_eq!(ada.wait("coins")["coins"], 1.0);
    // The coin is picked up within reach of it; the walk ends a tick later,
    // and the save made when she goes is of where she stood then.
    ada.until("her to arrive on the coin", |f| {
        f.iter().any(|x| x["move"].as_array().is_some_and(|m| m.iter().any(|m| m[0] == me && m[1] == 560.0)))
    });

    // Nobody can be signed in - and is told why, not told they are unknown,
    // and certainly not let in as a new player with nothing.
    let mut bo = Client::connect(&b.url());
    bo.send(json!({ "t": "hello", "token": bo_token }));
    assert_eq!(bo.wait("error")["why"], "the server cannot reach its store; try again shortly");
    assert_eq!(bo.count("welcome"), 0);

    // Ada leaves while it is down. Her save has nowhere to go: it waits.
    ada.conn.disconnect();
    std::thread::sleep(Duration::from_millis(600));

    // A new coordinator at the same address, knowing nothing but the store.
    let coord = cluster::coordinator(&addr, store, SECRET, 1.0).unwrap();

    // The nodes tell it what they hold, and send what was waiting.
    let deadline = Instant::now() + Duration::from_secs(10);
    let welcome = loop {
        let mut again = Client::connect(&a.url());
        again.send(json!({ "t": "hello", "token": ada_token }));
        again.until("an answer", |f| f.iter().any(|x| x["t"] == "welcome" || x["t"] == "error"));
        if let Some(welcome) = again.last("welcome").filter(|w| w["state"]["coins"] == 1.0) {
            break welcome;
        }
        assert!(Instant::now() < deadline, "her save never arrived: {:?}", again.frames);
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(welcome["state"]["x"], 560.0, "what she did while the coordinator was away was not lost");

    // And the directory is back: the meadow is still A's, so B redirects -
    // once A's next heartbeat has told the new coordinator what it holds,
    // which on a fast machine is after her sign-in above.
    let deadline = Instant::now() + Duration::from_secs(5);
    while ask(&coord, "owner", json!({ "node": "b", "kind": "zone", "room": "meadow" }))["internal_url"].is_null() {
        assert!(Instant::now() < deadline, "A never told the new coordinator it has the meadow");
        std::thread::sleep(Duration::from_millis(100));
    }
    let (mut bo, _, same) = signed_in(&b, "", &bo_token);
    assert_eq!(same, bo_player);
    join(&mut bo, "zone", "meadow");
    assert_eq!(bo.wait("redirect")["url"], a.url());
    a.stop();
    b.stop();
    coord.stop();
}

/// A new room goes to the node that asks for it - unless that node is far
/// busier than another that could run it.
#[test]
fn a_new_room_goes_to_a_quieter_node_when_the_asker_is_busy() {
    let coord = coordinator();
    let register = |name: &str, players: u64| {
        ask(&coord, "register", json!({
            "node": name, "public_url": format!("ws://{name}"), "internal_url": format!("http://{name}"),
            "kinds": ["zone"], "stats": { "players": players },
        }))
    };
    assert_eq!(register("busy", 400)["known"], false, "a node it has not heard of is told so");
    assert_eq!(register("busy", 400)["known"], true);
    register("quiet", 12);

    let owner = ask(&coord, "claim", json!({ "node": "busy", "kind": "zone", "room": "meadow" }));
    assert_eq!((&owner["node"], &owner["url"]), (&json!("quiet"), &json!("ws://quiet")));
    // Once it has an owner, it has that owner.
    assert_eq!(ask(&coord, "claim", json!({ "node": "busy", "kind": "zone", "room": "meadow" }))["node"], "quiet");
    // A little busier is not worth sending a player across for.
    register("busy", 40);
    assert_eq!(ask(&coord, "claim", json!({ "node": "busy", "kind": "zone", "room": "cave" }))["node"], "busy");
    coord.stop();
}

/// An operator looking at one node can still act on a player who is on
/// another: the node that has them does it.
#[test]
fn an_operator_on_one_node_reaches_a_player_on_another() {
    let coord = coordinator();
    let (a, b) = (node(&coord, "a"), node(&coord, "b"));
    let (mut cy, _, cy_player) = signed_in(&b, "Cy", "");
    join(&mut cy, "zone", "cave");
    cy.wait("joined");
    let admin = |path: &str, method: &str, body: Value| -> Result<Value, u16> {
        let request = ureq::request(method, &format!("http://{}{path}", a.addr))
            .set("Authorization", "Bearer admin-token")
            .set("Content-Type", "application/json");
        match request.send_string(&body.to_string()) {
            Ok(r) => Ok(r.into_json().unwrap()),
            Err(ureq::Error::Status(code, _)) => Err(code),
            Err(e) => panic!("{e}"),
        }
    };

    // Edited from A; it is B's room that holds the true copy, and tells him.
    let player = cy_player.as_str().unwrap();
    let edited = admin(&format!("/admin/api/player/{player}"), "PUT", json!({ "state": { "zone": "cave", "coins": 12 } }));
    assert_eq!(edited.unwrap()["where"], "room");
    assert_eq!(cy.wait("state")["state"]["coins"], 12);

    // Kicked from A.
    admin("/admin/api/kick", "POST", json!({ "player": player, "why": "from the other node" })).unwrap();
    assert_eq!(cy.wait("kicked")["why"], "from the other node");

    // Somebody who is nowhere is still nowhere.
    assert_eq!(admin("/admin/api/kick", "POST", json!({ "player": "p_nobody" })), Err(404));

    // And A's dashboard shows the whole cluster: B's room and B's player,
    // each marked with the node it is on.
    let (mut ada, _, _) = signed_in(&a, "Ada", "");
    join(&mut ada, "zone", "meadow");
    ada.wait("joined");
    let overview: Value = ureq::get(&format!("http://{}/admin/api/overview", a.addr))
        .set("Authorization", "Bearer admin-token").call().unwrap().into_json().unwrap();
    let rooms: Vec<(&str, &str)> = overview["rooms"].as_array().unwrap().iter()
        .map(|r| (r["node"].as_str().unwrap(), r["id"].as_str().unwrap())).collect();
    assert!(rooms.contains(&("a", "meadow")) && rooms.contains(&("b", "cave")), "{rooms:?}");
    let players: Value = ureq::get(&format!("http://{}/admin/api/players", a.addr))
        .set("Authorization", "Bearer admin-token").call().unwrap().into_json().unwrap();
    let on: Vec<(&str, &str)> = players["players"].as_array().unwrap().iter()
        .map(|p| (p["node"].as_str().unwrap(), p["name"].as_str().unwrap())).collect();
    assert!(on.contains(&("a", "Ada")) && on.contains(&("b", "Cy")), "{on:?}");

    // A room on B is looked into, commanded and closed from A.
    join(&mut cy, "zone", "cave");
    cy.until("the cave again", |f| f.iter().filter(|x| x["t"] == "joined").count() == 2);
    let seen = admin("/admin/api/room/zone/cave", "GET", json!({})).unwrap();
    assert_eq!(seen["players"][0]["name"], "Cy");
    let answer = admin("/admin/api/room/zone/cave/command", "POST", json!({ "coin": [100, 100] })).unwrap();
    assert_eq!(answer["ok"], true);
    admin("/admin/api/room/zone/cave/close", "POST", json!({})).unwrap();
    cy.wait("closed");
    assert_eq!(admin("/admin/api/room/zone/atlantis", "GET", json!({})), Err(404));
    a.stop();
    b.stop();
    coord.stop();
}

/// The cluster's secret, and every player's state, go to the coordinator -
/// over HTTPS when it has a certificate, which the nodes are told to trust.
#[test]
fn a_node_reaches_a_coordinator_over_https() {
    use jengine_server::{Tls, listen};
    let dir = std::env::temp_dir().join(format!("jengine-cluster-tls-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (cert, key) = listen::self_signed(&["127.0.0.1".to_string()]).unwrap();
    std::fs::write(dir.join("cert.pem"), cert).unwrap();
    std::fs::write(dir.join("key.pem"), key).unwrap();
    let tls = Tls::new(dir.join("cert.pem"), dir.join("key.pem"));
    let coord = cluster::coordinator_with("127.0.0.1:0", Arc::new(MemoryStore::new()), SECRET, 1.0, Some(tls)).unwrap();
    assert!(coord.url().starts_with("https://"));

    let start = |name: &str, ca: Option<std::path::PathBuf>| {
        let mut config = ClusterConfig::new(&coord.url(), SECRET, name);
        config.heartbeat_secs = 0.2;
        config.ca = ca;
        Server::new()
            .cluster(config)
            .room("zone", KindOptions::ticking(30.0).persistent(), world::Zone::new)
            .start("127.0.0.1:0")
            .unwrap()
    };
    // A node that trusts the certificate signs players in: its store is the
    // coordinator's, reached over HTTPS.
    let a = start("a", Some(dir.join("cert.pem")));
    let (mut ada, token, player) = signed_in(&a, "Ada", "");
    join(&mut ada, "zone", "meadow");
    ada.wait("joined");
    ada.conn.disconnect();
    std::thread::sleep(Duration::from_millis(300));
    let (_again, _, same) = signed_in(&a, "", &token);
    assert_eq!(same, player, "saved to, and loaded from, the coordinator");

    // A node that does not trust it reaches nothing - and says so, rather
    // than letting anyone in with no store behind them.
    let b = start("b", None);
    let mut bo = Client::connect(&b.url());
    bo.send(json!({ "t": "hello", "token": token }));
    assert_eq!(bo.wait("error")["why"], "the server cannot reach its store; try again shortly");
    a.stop();
    b.stop();
    coord.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// K6: a node can be given its own key and nothing else - no cluster
/// secret on that machine at all - and plays its whole part: signs players
/// in through the coordinator, keeps them, and hands one to the other node.
#[test]
fn a_node_holding_only_its_own_key_plays_its_part() {
    let coord = coordinator();
    let mut config = ClusterConfig::new(&coord.url(), "", "keyed");
    config.node_key = Some(cluster::node_key_hex(SECRET, "keyed"));
    config.heartbeat_secs = 0.2;
    let keyed = Server::new()
        .cluster(config)
        .room("zone", KindOptions::ticking(30.0).max_players(100).persistent(), world::Zone::new)
        .start("127.0.0.1:0")
        .unwrap();
    let other = node(&coord, "other");
    let (mut ada, ada_token, ada_player) = signed_in(&keyed, "Ada", "");
    join(&mut ada, "zone", "meadow");
    ada.wait("joined");
    let (mut bo, _, _) = signed_in(&other, "Bo", "");
    join(&mut bo, "zone", "cave");
    bo.wait("joined");
    // Ada walks into the cave, which is the other node's: saved through the
    // coordinator by a node holding only its own key, and loaded there.
    ada.send(json!({ "t": "send", "body": { "go": [1010, 300] } }));
    assert_eq!(ada.wait("redirect")["url"], json!(other.url()));
    let (_ada, _, same) = signed_in(&other, "", &ada_token);
    assert_eq!(same, ada_player);
    keyed.stop();
    other.stop();
    coord.stop();
}

/// S8/K2: nodes whose store writes behind - every save taken at once and
/// sent to the coordinator on a thread of its own - still hand a player
/// across with everything they had: the server flushes before it sends
/// them on.
#[test]
fn a_node_that_writes_behind_hands_a_player_on_whole() {
    let coord = coordinator();
    let behind = |name: &str| {
        let mut config = ClusterConfig::new(&coord.url(), SECRET, name);
        config.heartbeat_secs = 0.2;
        Server::new()
            .cluster(config)
            .write_behind()
            .room("zone", KindOptions::ticking(30.0).max_players(100).persistent(), world::Zone::new)
            .post("/api/grant/{id}", |req, api| {
                let state = api.edit_player(req.param("id"), |state| {
                    state["coins"] = json!(state["coins"].as_f64().unwrap_or(0.0) + 5.0);
                })?;
                Ok(json!({ "coins": state["coins"] }))
            })
            .start("127.0.0.1:0")
            .unwrap()
    };
    let (a, b) = (behind("wa"), behind("wb"));
    let (mut cy, _, _) = signed_in(&b, "Cy", "");
    join(&mut cy, "zone", "cave");
    cy.wait("joined");
    let (mut ada, ada_token, player) = signed_in(&a, "Ada", "");
    join(&mut ada, "zone", "meadow");
    ada.wait("joined");
    let granted: Value =
        ureq::post(&format!("http://{}/api/grant/{}", a.addr, player.as_str().unwrap())).call().unwrap().into_json().unwrap();
    assert_eq!(granted["coins"], 5.0);
    // Through the door, into the cave on the other node: saved, flushed, sent.
    ada.send(json!({ "t": "send", "body": { "go": [1010, 300] } }));
    assert_eq!(ada.wait("redirect")["url"], json!(b.url()));
    let (_ada, _, same) = signed_in(&b, "", &ada_token);
    assert_eq!(same, player);
    let welcome = _ada.last("welcome").unwrap();
    // Five granted, and any she picked up on her way to the door.
    assert!(welcome["state"]["coins"].as_f64().unwrap() >= 5.0, "the coins came too: {welcome}");
    assert_eq!(welcome["state"]["zone"], "cave");
    a.stop();
    b.stop();
    coord.stop();
}
