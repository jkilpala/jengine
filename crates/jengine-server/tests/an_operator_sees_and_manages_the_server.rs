//! **The admin API, over real HTTP against a server with players on it** -
//! what the dashboard at `/admin` is built on, called the way it calls it.

mod common;
#[path = "../examples/zones/world.rs"]
mod world;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use common::{Client, http_get};
use jengine_net::{HttpMethod, HttpRequest, http_client};
use jengine_server::{KindOptions, MemoryStore, Role, Running, Server, Store};
use serde_json::{Value, json};

const TOKEN: &str = "s3cret";

fn zones() -> Running {
    Server::new()
        .relay()
        .admin_token(TOKEN)
        // These tests scrape `/metrics` as Prometheus would, with no token.
        .public_stats()
        .room("zone", KindOptions::ticking(30.0).max_players(500).persistent(), world::Zone::new)
        .start("127.0.0.1:0")
        .unwrap()
}

/// One request as an operator would make it. Returns the status and the body.
fn call(addr: SocketAddr, method: HttpMethod, path: &str, token: &str, body: Option<Value>) -> (u16, Value) {
    let mut http = http_client();
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    if !token.is_empty() {
        headers.push(("Authorization".to_string(), format!("Bearer {token}")));
    }
    http.send(HttpRequest {
        id: "admin".into(),
        method,
        url: format!("http://{addr}{path}"),
        headers,
        body: body.map(|b| b.to_string()),
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        http.pump();
        if let Some(r) = http.drain().pop() {
            assert!(r.error.is_none(), "{path} never reached the server: {:?}", r.error);
            return (r.status, serde_json::from_str(&r.body).unwrap_or(Value::String(r.body)));
        }
        assert!(Instant::now() < deadline, "no answer from {path}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn get(addr: SocketAddr, path: &str) -> Value {
    let (status, body) = call(addr, HttpMethod::Get, path, TOKEN, None);
    assert_eq!(status, 200, "{path}: {body}");
    body
}

fn post(addr: SocketAddr, path: &str, body: Value) -> Value {
    let (status, answer) = call(addr, HttpMethod::Post, path, TOKEN, Some(body));
    assert_eq!(status, 200, "{path}: {answer}");
    answer
}

/// Ada, signed in and standing in the meadow.
fn ada_in_the_meadow(server: &Running) -> (Client, String, String) {
    let mut ada = Client::connect(&server.url());
    let welcome = ada.hello("Ada", "");
    let (id, token) = (welcome["player"].as_str().unwrap(), welcome["token"].as_str().unwrap());
    ada.send(json!({ "t": "join", "kind": "zone", "room": "meadow" }));
    ada.wait("joined");
    (ada, id.to_string(), token.to_string())
}

#[test]
fn nothing_is_shown_without_the_token() {
    let server = zones();
    for token in ["", "wrong", "s3cre", "s3cret-and-more"] {
        let (status, body) = call(server.addr, HttpMethod::Get, "/admin/api/overview", token, None);
        assert_eq!(status, 401, "token {token:?} got {body}");
        let (status, _) =
            call(server.addr, HttpMethod::Post, "/admin/api/drain", token, Some(json!({ "on": true })));
        assert_eq!(status, 401);
    }
    assert_eq!(http_get(server.addr, "/healthz"), "ok", "and the refused drain did nothing");
    assert!(http_get(server.addr, "/admin").contains("<title>jengine-server</title>"), "the page itself is static");
    server.stop();
}

#[test]
fn with_no_token_configured_there_is_no_admin_at_all() {
    let server = Server::new().relay().start("127.0.0.1:0").unwrap();
    for path in ["/admin", "/admin/api/overview"] {
        let (status, _) = call(server.addr, HttpMethod::Get, path, "", None);
        assert_eq!(status, 404, "{path}");
    }
    server.stop();
}

#[test]
fn an_operator_sees_rooms_players_and_their_state() {
    let server = zones();
    let (mut ada, id, _) = ada_in_the_meadow(&server);

    // The machine's figures, from the operating system: what is measured
    // is a number, and a disk is never empty. Waited for before Ada takes
    // her coin - the first reading can take seconds (on Windows about
    // eight), and standing on the coin's spot that long she takes it again
    // when it comes back (`COIN_RESPAWN`, five seconds).
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut measured = get(server.addr, "/admin/api/overview")["resources"].clone();
    while measured["disk_total_mb"].as_u64().unwrap_or(0) == 0 {
        assert!(Instant::now() < deadline, "no disk figure: {measured}");
        std::thread::sleep(Duration::from_millis(200));
        measured = get(server.addr, "/admin/api/overview")["resources"].clone();
    }

    ada.send(json!({ "t": "send", "body": { "go": [560, 300] } }));
    ada.wait("coins");

    let overview = get(server.addr, "/admin/api/overview");
    assert!(measured["disk_free_mb"].as_u64().unwrap() <= measured["disk_total_mb"].as_u64().unwrap());
    assert!(measured["net_rx_kb_per_sec"].is_number() && measured["net_tx_kb_per_sec"].is_number());
    assert_eq!(overview["connections"], 1);
    assert_eq!(overview["players"], 1);
    assert_eq!(overview["kinds"], json!(["relay", "zone"]));
    let room = &overview["rooms"][0];
    assert_eq!((&room["kind"], &room["id"], &room["players"]), (&json!("zone"), &json!("meadow"), &json!(1)));
    assert_eq!(room["max_players"], 500);
    assert!(overview["totals"]["messages_in"].as_u64().unwrap() >= 1);
    assert!(overview["totals"]["bytes_out"].as_u64().unwrap() > 0);

    // A ticking room reports what its ticks cost.
    let deadline = Instant::now() + Duration::from_secs(5);
    while get(server.addr, "/admin/api/overview")["rooms"][0]["tick_ms_max"].as_f64().unwrap() <= 0.0 {
        assert!(Instant::now() < deadline, "the zone never reported a tick time");
    }

    let players = get(server.addr, "/admin/api/players");
    assert_eq!(players["players"][0]["id"], id.as_str());
    assert_eq!(players["players"][0]["name"], "Ada");
    assert_eq!(players["players"][0]["room"], json!({ "kind": "zone", "id": "meadow" }));
    assert_eq!(players["players"][0]["address"], "127.0.0.1", "where she connects from");

    // The room's own page: its players' state as it is now, and what the
    // room says about itself.
    let detail = get(server.addr, "/admin/api/room/zone/meadow");
    assert_eq!(detail["players"][0]["state"]["coins"], 1.0);
    assert_eq!(detail["players"][0]["state"]["zone"], "meadow");
    assert!(detail["info"]["entities"].as_u64().unwrap() >= 3, "{detail}");
    assert_eq!(detail["info"]["coins_coming_back"], 1);

    let events = get(server.addr, "/admin/api/events");
    let texts: Vec<&str> = events["events"].as_array().unwrap().iter().filter_map(|e| e["text"].as_str()).collect();
    assert!(texts.contains(&"zone/meadow opened"), "{texts:?}");
    assert!(texts.iter().any(|t| t.ends_with("signed in from 127.0.0.1") && t.contains("Ada")), "{texts:?}");

    let (status, _) = call(server.addr, HttpMethod::Get, "/admin/api/room/zone/atlantis", TOKEN, None);
    assert_eq!(status, 404);
    server.stop();
}

#[test]
fn a_room_takes_its_own_commands_and_a_notice() {
    let server = zones();
    let (mut ada, _, _) = ada_in_the_meadow(&server);
    ada.until("the meadow's three coins", |f| {
        f.iter().flat_map(|x| x["enter"].as_array().into_iter().flatten()).filter(|e| e["kind"] == "coin").count() == 3
    });

    let answer = post(server.addr, "/admin/api/room/zone/meadow/command", json!({ "coin": [520, 320] }));
    assert_eq!(answer["ok"], true);
    let coin = answer["id"].clone();
    ada.until("the operator's coin to appear", |f| {
        f.iter().flat_map(|x| x["enter"].as_array().into_iter().flatten()).any(|e| e["id"] == coin && e["x"] == 520.0)
    });
    let refused = post(server.addr, "/admin/api/room/zone/meadow/command", json!({ "dance": true }));
    assert!(refused["error"].is_string());

    // A notice reaches everyone connected - Bo is signed in and in no room.
    let mut bo = Client::connect(&server.url());
    bo.hello("Bo", "");
    assert_eq!(post(server.addr, "/admin/api/notice", json!({ "text": "restart at noon" }))["connections"], 2);
    assert_eq!(ada.wait("notice")["text"], "restart at noon");
    assert_eq!(bo.wait("notice")["text"], "restart at noon");
    // Aimed at a room, it is that room's players alone.
    bo.frames.clear();
    assert_eq!(post(server.addr, "/admin/api/notice", json!({ "text": "meadow only", "room": "meadow" }))["rooms"], 1);
    ada.until("the meadow's notice", |f| f.iter().any(|x| x["text"] == "meadow only"));
    assert_eq!(bo.count("notice"), 0);
    server.stop();
}

#[test]
fn an_operator_kicks_disconnects_and_closes() {
    let server = zones();
    let (mut ada, id, token) = ada_in_the_meadow(&server);

    post(server.addr, "/admin/api/kick", json!({ "player": id, "why": "be nice" }));
    assert_eq!(ada.wait("kicked")["why"], "be nice");
    assert_eq!(get(server.addr, "/admin/api/players")["players"][0]["room"], Value::Null, "out of the room, still online");
    let (status, _) = call(server.addr, HttpMethod::Post, "/admin/api/kick", TOKEN, Some(json!({ "player": id })));
    assert_eq!(status, 409, "there is no room left to kick them from");

    // Back in, and the room is closed around her.
    ada.send(json!({ "t": "join", "kind": "zone", "room": "meadow" }));
    ada.until("the meadow again", |f| f.iter().filter(|x| x["t"] == "joined").count() == 2);
    post(server.addr, "/admin/api/room/zone/meadow/close", json!({}));
    ada.wait("closed");

    // Signed in and in no room: an edit goes through her connection, and
    // she is told.
    let path = format!("/admin/api/player/{id}");
    let (status, body) = call(server.addr, HttpMethod::Put, &path, TOKEN, Some(json!({ "state": { "coins": 7 } })));
    assert_eq!((status, &body["where"]), (200, &json!("connection")));
    assert_eq!(ada.wait("state")["state"], json!({ "coins": 7 }));

    // Off the server, edited again, and it is what she comes back to.
    post(server.addr, "/admin/api/kick", json!({ "player": id, "why": "maintenance", "disconnect": true }));
    assert_eq!(ada.wait("error")["why"], "maintenance");
    let deadline = Instant::now() + Duration::from_secs(5);
    while get(server.addr, "/admin/api/overview")["players"] != 0 {
        assert!(Instant::now() < deadline, "she is still online");
    }
    assert_eq!(get(server.addr, &path)["online"], Value::Null);
    let (status, body) = call(server.addr, HttpMethod::Put, &path, TOKEN, Some(json!({ "state": { "coins": 99 } })));
    assert_eq!(status, 200, "{body}");
    let mut again = Client::connect(&server.url());
    assert_eq!(again.hello("", &token)["state"], json!({ "coins": 99 }));

    let events = get(server.addr, "/admin/api/events");
    let admin: Vec<&str> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "admin")
        .filter_map(|e| e["text"].as_str())
        .collect();
    assert_eq!(admin.len(), 5, "what an operator did is on the record: {admin:?}");
    server.stop();
}

#[test]
fn draining_turns_new_players_away_and_keeps_the_old() {
    let server = zones();
    let (mut ada, _, _) = ada_in_the_meadow(&server);
    assert_eq!(post(server.addr, "/admin/api/drain", json!({ "on": true }))["draining"], true);

    let (status, body) = call(server.addr, HttpMethod::Get, "/healthz", "", None);
    assert_eq!((status, body), (503, json!("draining")), "what a load balancer reads");
    let mut late = jengine_net::NetConnection::new();
    jengine_net::connect(&mut late, &server.url());
    let deadline = Instant::now() + Duration::from_secs(5);
    while matches!(late.state(), jengine_net::NetState::Connecting) {
        late.pump();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!late.is_connected(), "a new connection got in while draining");

    // Ada is untouched.
    ada.send(json!({ "t": "send", "body": { "go": [560, 300] } }));
    ada.wait("coins");

    post(server.addr, "/admin/api/drain", json!({ "on": false }));
    assert_eq!(http_get(server.addr, "/healthz"), "ok");
    Client::connect(&server.url());
    server.stop();
}

#[test]
fn stats_can_be_kept_for_those_with_the_token() {
    let server = Server::new().relay().admin_token(TOKEN).private_stats().start("127.0.0.1:0").unwrap();
    for path in ["/stats", "/metrics"] {
        let (status, _) = call(server.addr, HttpMethod::Get, path, "", None);
        assert_eq!(status, 401, "{path} without the token");
        let (status, _) = call(server.addr, HttpMethod::Get, path, TOKEN, None);
        assert_eq!(status, 200, "{path} with it");
    }
    assert_eq!(http_get(server.addr, "/healthz"), "ok", "a load balancer needs no token");
    server.stop();
}

#[test]
fn metrics_are_prometheus_text_per_kind() {
    let server = zones();
    let (_ada, _, _) = ada_in_the_meadow(&server);
    let text = http_get(server.addr, "/metrics");
    for line in [
        "# TYPE jengine_connections gauge",
        "jengine_connections 1",
        "jengine_players_online 1",
        "jengine_rooms{kind=\"zone\"} 1",
        "jengine_rooms{kind=\"relay\"} 0",
        "jengine_room_players{kind=\"zone\"} 1",
        "# TYPE jengine_frames_out_total counter",
        "jengine_room_panics_total 0",
    ] {
        assert!(text.lines().any(|l| l == line), "missing {line:?} in:\n{text}");
    }
    assert!(!text.contains("meadow"), "no room or player is ever a label");
    server.stop();
}

/// A viewer's token opens the dashboard and nothing else; what an operator
/// does is recorded under their name.
#[test]
fn a_viewer_may_look_and_an_operator_is_named_in_the_record() {
    let server = Server::new()
        .relay()
        .operator("vera", "viewer-token", Role::Viewer)
        .operator("olaf", "operator-token", Role::Operator)
        .start("127.0.0.1:0")
        .unwrap();
    let (status, _) = call(server.addr, HttpMethod::Get, "/admin/api/overview", "viewer-token", None);
    assert_eq!(status, 200);
    let (status, body) =
        call(server.addr, HttpMethod::Post, "/admin/api/drain", "viewer-token", Some(json!({ "on": true })));
    assert_eq!((status, body["error"].as_str()), (403, Some("vera may look, not change things")));
    assert_eq!(http_get(server.addr, "/healthz"), "ok", "and nothing was changed");

    let (status, _) =
        call(server.addr, HttpMethod::Post, "/admin/api/notice", "operator-token", Some(json!({ "text": "hi" })));
    assert_eq!(status, 200);
    let (_, events) = call(server.addr, HttpMethod::Get, "/admin/api/events", "viewer-token", None);
    let texts: Vec<&str> = events["events"].as_array().unwrap().iter().filter_map(|e| e["text"].as_str()).collect();
    assert!(texts.iter().any(|t| t.starts_with("olaf: notice to")), "{texts:?}");
    server.stop();
}

/// The event log is not only memory: what an operator did, and that the
/// server stopped and started, is still there afterwards.
#[test]
fn what_happened_is_still_known_after_a_restart() {
    let store: std::sync::Arc<dyn Store> = std::sync::Arc::new(MemoryStore::new());
    let server = Server::new().relay().store(store.clone()).admin_token(TOKEN).start("127.0.0.1:0").unwrap();
    post(server.addr, "/admin/api/drain", json!({ "on": true }));
    server.stop();

    let server = Server::new().relay().store(store).admin_token(TOKEN).start("127.0.0.1:0").unwrap();
    let events = get(server.addr, "/admin/api/events");
    let texts: Vec<&str> = events["events"].as_array().unwrap().iter().filter_map(|e| e["text"].as_str()).collect();
    assert!(texts.contains(&"admin: draining: no new connections"), "{texts:?}");
    assert_eq!(texts.iter().filter(|t| **t == "started").count(), 2, "both starts are on record: {texts:?}");
    assert_eq!(http_get(server.addr, "/healthz"), "ok", "draining itself is not remembered");
    server.stop();
}

/// Somebody in a room is edited through the room, which holds the true copy.
#[test]
fn a_player_in_a_room_can_be_edited_where_they_stand() {
    let server = zones();
    let (mut ada, id, _) = ada_in_the_meadow(&server);
    let path = format!("/admin/api/player/{id}");
    let state = get(server.addr, "/admin/api/room/zone/meadow")["players"][0]["state"].clone();
    let mut richer = state.clone();
    richer["coins"] = json!(41);
    let (status, body) = call(server.addr, HttpMethod::Put, &path, TOKEN, Some(json!({ "state": richer })));
    assert_eq!((status, &body["where"]), (200, &json!("room")), "{body}");
    assert_eq!(ada.wait("state")["state"]["coins"], 41, "she is told");
    assert_eq!(get(server.addr, "/admin/api/room/zone/meadow")["players"][0]["state"]["coins"], 41);

    // And it is the room's copy that changed: the next coin counts from it.
    ada.send(json!({ "t": "send", "body": { "go": [560, 300] } }));
    assert_eq!(ada.wait("coins")["coins"], 42.0);
    server.stop();
}

/// The numbers are sampled, served for the charts, and kept across a restart.
#[test]
fn the_numbers_are_kept_over_time() {
    let store: std::sync::Arc<dyn Store> = std::sync::Arc::new(MemoryStore::new());
    let start = |store: std::sync::Arc<dyn Store>| {
        Server::new()
            .relay()
            .store(store)
            .admin_token(TOKEN)
            .config(jengine_server::Config { history_secs: 0.1, ..Default::default() })
            .start("127.0.0.1:0")
            .unwrap()
    };
    let server = start(store.clone());
    let mut ada = Client::connect(&server.url());
    ada.hello("Ada", "");
    let deadline = Instant::now() + Duration::from_secs(5);
    let samples = loop {
        let history = get(server.addr, "/admin/api/history");
        let samples = history["samples"].as_array().unwrap().clone();
        if samples.len() >= 4 && samples.last().unwrap()["players"] == 1 {
            assert_eq!(history["period"], 0.1);
            break samples;
        }
        assert!(Instant::now() < deadline, "no samples with a player in them: {samples:?}");
    };
    assert!(samples.windows(2).all(|w| w[0]["at"].as_f64() < w[1]["at"].as_f64()), "oldest first");
    assert!(samples[0].get("tick_ms").is_some() && samples[0].get("bytes_out").is_some());
    let before = samples.len();
    server.stop();

    let server = start(store);
    let after = get(server.addr, "/admin/api/history")["samples"].as_array().unwrap().len();
    assert!(after >= before, "the earlier samples are still there: {after} < {before}");
    server.stop();
}

/// What the process is costing the machine is on the dashboard, in the
/// history, and where a scraper reads it.
#[test]
fn memory_and_cpu_are_reported() {
    let server = Server::new()
        .relay()
        .admin_token(TOKEN)
        .config(jengine_server::Config { history_secs: 0.1, ..Default::default() })
        .public_stats()
        .start("127.0.0.1:0")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let used = loop {
        let used = get(server.addr, "/admin/api/overview")["resources"].clone();
        if used["memory_mb"].as_u64().unwrap_or(0) > 0 {
            break used;
        }
        assert!(Instant::now() < deadline, "no memory figure was ever measured: {used}");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(used["cores"].as_u64().unwrap() >= 1);
    assert!(used["machine_memory_total_mb"].as_u64().unwrap() > used["memory_mb"].as_u64().unwrap());
    assert!(used["cpu"].is_number() && used["machine_cpu"].is_number());

    let stats = common::stats(server.addr);
    assert!(stats["memory_mb"].as_u64().unwrap() > 0, "and in /stats, where the fleet reads it: {stats}");
    let deadline = Instant::now() + Duration::from_secs(5);
    while get(server.addr, "/admin/api/history")["samples"].as_array().unwrap().last().is_none_or(|s| s["memory_mb"] == 0) {
        assert!(Instant::now() < deadline, "the history never carried a memory figure");
    }
    let metrics = http_get(server.addr, "/metrics");
    for name in ["jengine_process_memory_bytes", "jengine_process_cpu_cores", "jengine_machine_memory_total_bytes"] {
        assert!(metrics.contains(&format!("# TYPE {name} gauge")), "{name} is not in /metrics");
    }
    server.stop();
}

/// An operator can give an account a new password, and delete one.
#[test]
fn an_operator_resets_and_deletes_accounts() {
    use jengine_server::PasswordAuth;
    let server = Server::new()
        .relay()
        .admin_token(TOKEN)
        .auth(PasswordAuth::new().iterations(50))
        .start("127.0.0.1:0")
        .unwrap();
    let mut ada = Client::connect(&server.url());
    ada.send(json!({ "t": "hello", "account": "Ada", "password": "correct horse", "register": true }));
    let player = ada.wait("welcome")["player"].clone();

    post(server.addr, "/admin/api/account/ada/reset", json!({ "password": "a fresh start" }));
    let mut back = Client::connect(&server.url());
    back.send(json!({ "t": "hello", "account": "ada", "password": "a fresh start" }));
    assert_eq!(back.wait("welcome")["player"], player);
    let (status, body) = call(server.addr, HttpMethod::Post, "/admin/api/account/nobody/reset", TOKEN, Some(json!({ "password": "long enough" })));
    assert_eq!((status, body["error"].as_str()), (409, Some("no such account")));

    let (status, body) = call(server.addr, HttpMethod::Delete, "/admin/api/account/ada", TOKEN, None);
    assert_eq!((status, &body["player"]), (200, &player));
    assert_eq!(back.wait("error")["why"], "signed in somewhere else", "whoever was signed in as it is put out");
    let (status, _) = call(server.addr, HttpMethod::Get, &format!("/admin/api/player/{}", player.as_str().unwrap()), TOKEN, None);
    assert_eq!(status, 404, "and the player's record is gone with it");
    let mut gone = Client::connect(&server.url());
    gone.send(json!({ "t": "hello", "account": "ada", "password": "a fresh start" }));
    assert_eq!(gone.wait("error")["why"], "wrong account or password");
    server.stop();
}

/// What is wrong is said on the dashboard and written in the event log when
/// it starts - here, a room whose tick takes longer than it has.
#[test]
fn a_room_over_its_budget_is_a_warning() {
    use jengine_server::{Player, Players, Room, RoomCtx};
    struct Slow;
    impl Room for Slow {
        fn on_join(&mut self, _: &mut RoomCtx, _: &mut Player, _: &Value) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn on_tick(&mut self, _: &mut RoomCtx, _: &mut Players, _: f64) {
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    // Somewhere for the warning to be sent: a socket that reads one request.
    let hook = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let hook_url = format!("http://{}/alerts", hook.local_addr().unwrap());
    let heard = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut socket, _) = hook.accept().unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut request = Vec::new();
        let mut chunk = [0u8; 2048];
        while !String::from_utf8_lossy(&request).contains("\"ended\"") {
            match socket.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => request.extend_from_slice(&chunk[..n]),
            }
        }
        let _ = socket.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        String::from_utf8_lossy(&request).to_string()
    });
    let server = Server::new()
        .admin_token(TOKEN)
        .config(jengine_server::Config { history_secs: 0.1, warn_webhook: Some(hook_url), ..Default::default() })
        // A hundred ticks a second is ten milliseconds each; this takes thirty.
        .room("slow", KindOptions::ticking(100.0), |_| Slow)
        .start("127.0.0.1:0")
        .unwrap();
    assert_eq!(get(server.addr, "/admin/api/overview")["warnings"], json!([]), "nothing is wrong yet");
    let mut ada = Client::connect(&server.url());
    ada.send(json!({ "t": "join", "kind": "slow", "room": "r1" }));
    ada.wait("joined");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let warnings = get(server.addr, "/admin/api/overview")["warnings"].clone();
        if warnings.as_array().unwrap().iter().any(|w| w.as_str().unwrap().starts_with("slow/r1 took")) {
            break;
        }
        assert!(Instant::now() < deadline, "no warning: {warnings}");
        std::thread::sleep(Duration::from_millis(100));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let events = get(server.addr, "/admin/api/events");
        if events["events"].as_array().unwrap().iter().any(|e| e["kind"] == "warn") {
            break;
        }
        assert!(Instant::now() < deadline, "the warning never reached the event log");
        std::thread::sleep(Duration::from_millis(100));
    }
    // And it was sent to whoever asked to be told.
    let request = heard.join().unwrap();
    assert!(request.starts_with("POST /alerts"), "{request}");
    assert!(request.contains("slow/r1 took") && request.contains("\"started\""), "{request}");
    // The room's own cost is on its row: thirty milliseconds a hundred times
    // a second is far more than one core has.
    let row = get(server.addr, "/admin/api/overview")["rooms"][0].clone();
    assert!(row["cpu"].as_f64().unwrap() > 100.0, "{row}");
    server.stop();
}

/// A market that trades with nobody in port: it ticks on its own.
struct Market {
    ticks: u64,
}

impl jengine_server::Room for Market {
    fn on_join(
        &mut self,
        _: &mut jengine_server::RoomCtx,
        _: &mut jengine_server::Player,
        _: &Value,
    ) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn on_tick(&mut self, _: &mut jengine_server::RoomCtx, _: &mut jengine_server::Players, _: f64) {
        self.ticks += 1;
    }
    fn describe(&self) -> Value {
        json!({ "ticks": self.ticks })
    }
}

/// S24: a room opened at start is up and ticking before anybody comes, and
/// stays when the first who comes leaves - even of a kind that is not
/// persistent. One not asked for is not opened.
#[test]
fn a_room_opened_at_start_runs_with_nobody_in_it() {
    let server = Server::new()
        .admin_token(TOKEN)
        .room("market", KindOptions::ticking(50.0), |_| Market { ticks: 0 })
        .open_at_start("market", "harbour")
        .start("127.0.0.1:0")
        .unwrap();
    let ticks = |server: &Running| get(server.addr, "/admin/api/room/market/harbour")["info"]["ticks"].as_u64();
    let deadline = Instant::now() + Duration::from_secs(5);
    while ticks(&server).unwrap_or(0) < 5 {
        assert!(Instant::now() < deadline, "the harbour never ticked: {:?}", ticks(&server));
        std::thread::sleep(Duration::from_millis(20));
    }
    let rooms = get(server.addr, "/admin/api/overview")["rooms"].clone();
    assert_eq!(rooms.as_array().unwrap().len(), 1, "only the harbour: {rooms}");

    let mut ada = Client::connect(&server.url());
    ada.hello("Ada", "");
    ada.send(json!({ "t": "join", "kind": "market", "room": "harbour" }));
    ada.wait("joined");
    ada.send(json!({ "t": "leave" }));
    ada.wait("left");
    let before = ticks(&server).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(ticks(&server).unwrap() > before, "still open, still trading, with her gone");
    server.stop();
}

/// R3: a game adds a warning of its own; the webhook that refuses the first
/// POST is tried again and takes it; a warning that goes on is said again
/// (`still`); and its end is said too.
#[test]
fn warnings_are_retried_repeated_and_a_game_can_add_its_own() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    // A webhook that answers 500 to its first request and 200 after.
    let hook = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let hook_url = format!("http://{}/alerts", hook.local_addr().unwrap());
    let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let heard = bodies.clone();
    std::thread::spawn(move || {
        for (n, socket) in hook.incoming().enumerate() {
            let Ok(mut socket) = socket else { continue };
            socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            while !request.ends_with(b"}") {
                match socket.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(k) => request.extend_from_slice(&chunk[..k]),
                }
            }
            let text = String::from_utf8_lossy(&request).to_string();
            let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
            let status = if n == 0 { "500 Internal Server Error" } else { "200 OK" };
            let _ = write!(socket, "HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            if n > 0 {
                heard.lock().unwrap().push(body);
            }
        }
    });
    let unsold = std::sync::Arc::new(AtomicBool::new(true));
    let watched = unsold.clone();
    let server = Server::new()
        .admin_token(TOKEN)
        .config(jengine_server::Config {
            history_secs: 0.1,
            warn_webhook: Some(hook_url),
            warn_repeat_secs: 0.6,
            warn_retry_secs: vec![0.2, 0.2],
            ..Default::default()
        })
        .warn_when("market", move || watched.load(Ordering::Relaxed).then(|| "nobody is selling iron".to_string()))
        .start("127.0.0.1:0")
        .unwrap();
    let said = |what: &str| -> bool {
        bodies.lock().unwrap().iter().any(|b| {
            let v: Value = serde_json::from_str(b).unwrap_or(Value::Null);
            v[what].as_array().is_some_and(|l| l.iter().any(|t| t.as_str().is_some_and(|t| t.contains("iron") || t == "game:market")))
        })
    };
    let wait_for = |what: &str| {
        let deadline = Instant::now() + Duration::from_secs(8);
        while !said(what) {
            assert!(Instant::now() < deadline, "never told '{what}': {:?}", bodies.lock().unwrap());
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    wait_for("started");
    assert!(get(server.addr, "/admin/api/overview")["warnings"].to_string().contains("nobody is selling iron"));
    wait_for("still");
    unsold.store(false, Ordering::Relaxed);
    wait_for("ended");
    let events = get(server.addr, "/admin/api/events").to_string();
    assert!(events.contains("still: nobody is selling iron") && events.contains("no longer a problem: game:market"), "{events}");
    server.stop();
}

/// S12: an operator sees the channels, mutes somebody in one - who is
/// told why their words do not go out - lets them speak again, and clears
/// what a channel remembers for whoever joins next.
#[test]
fn an_operator_moderates_a_channel() {
    let server = Server::new().relay().open_channels().admin_token(TOKEN).start("127.0.0.1:0").unwrap();
    let sub = |name: &str| {
        let mut c = Client::connect(&server.url());
        let welcome = c.hello(name, "");
        c.send(json!({ "t": "sub", "channel": "lobby" }));
        c.wait("subbed");
        (c, welcome["player"].as_str().unwrap().to_string())
    };
    let (mut ada, _) = sub("Ada");
    let (mut bo, bo_id) = sub("Bo");
    bo.send(json!({ "t": "say", "channel": "lobby", "body": "something nobody should read" }));
    ada.until("Bo's words", |f| f.iter().any(|x| x["t"] == "said"));

    let listed = get(server.addr, "/admin/api/channels");
    assert_eq!(listed["channels"][0]["name"], "lobby");
    assert_eq!(listed["channels"][0]["members"].as_array().unwrap().len(), 2);
    post(server.addr, "/admin/api/channel/lobby/mute", json!({ "player": bo_id, "minutes": 10, "why": "spam" }));
    bo.send(json!({ "t": "say", "channel": "lobby", "body": "again" }));
    assert_eq!(bo.wait("error")["why"], "an operator has muted you in that channel");
    assert_eq!(get(server.addr, "/admin/api/channels")["channels"][0]["muted"][0]["player"], json!(bo_id));
    let events = get(server.addr, "/admin/api/events").to_string();
    assert!(events.contains("muted") && events.contains("spam"), "on record, with why");

    post(server.addr, "/admin/api/channel/lobby/mute", json!({ "player": bo_id, "minutes": 0 }));
    bo.send(json!({ "t": "say", "channel": "lobby", "body": "sorry" }));
    ada.until("Bo again", |f| f.iter().any(|x| x["t"] == "said" && x["body"] == "sorry"));

    post(server.addr, "/admin/api/channel/lobby/clear", json!({}));
    let (cy, _) = sub("Cy");
    assert_eq!(cy.last("subbed").unwrap()["recent"], json!([]), "a newcomer meets nothing of it");
    server.stop();
}
