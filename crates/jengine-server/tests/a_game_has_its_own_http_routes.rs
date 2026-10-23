//! **A game's own HTTP routes, over real HTTP against a server with a player
//! on it** - the stats a profile page, a launcher or a web site asks for.

mod common;
#[path = "../examples/zones/world.rs"]
mod world;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Client, hello_when};
use jengine_net::{HttpMethod, HttpRequest, http_client};
use jengine_server::{ApiError, Config, KindOptions, MemoryStore, Running, Server, Store};
use serde_json::{Value, json};

fn game(store: Arc<dyn Store>) -> Running {
    Server::new()
        .store(store)
        // Its `/metrics` is read as a scraper would, with no token.
        .public_stats()
        .room("zone", KindOptions::ticking(30.0).max_players(500).persistent(), world::Zone::new)
        // Anyone may see a player's card.
        .get("/api/players/{id}", |req, api| {
            let player = api.player(req.param("id")).ok_or_else(ApiError::not_found)?;
            Ok(json!({
                "name": player["name"], "coins": player["state"]["coins"],
                "online": player["online"], "room": player["room"],
            }))
        })
        // Only the player themselves sees all of it.
        .get("/api/me", |_req, api| {
            let me = api.caller()?;
            api.player(&me).ok_or_else(ApiError::not_found)
        })
        .get("/api/status", |_req, api| Ok(json!({ "online": api.online(), "rooms": api.rooms() })))
        .get("/api/zones/{id}", |req, api| api.room("zone", req.param("id")).ok_or_else(ApiError::not_found))
        // Something a player writes about themselves, kept in the store.
        .post("/api/motto", |req, api| {
            let me = api.caller()?;
            let motto = req.body["motto"].as_str().ok_or_else(|| ApiError::bad("send a motto"))?;
            api.store().save("motto", &me, motto);
            Ok(json!({ "ok": true }))
        })
        .get("/api/motto/{id}", |req, api| {
            let motto = api.store().load("motto", req.param("id")).ok_or_else(ApiError::not_found)?;
            Ok(json!({ "motto": motto, "loud": req.query("loud").is_some() }))
        })
        // A gift, for whoever knows the shop's secret - given by the room,
        // which is what has the player while they play.
        .post("/api/gift/{id}", |req, api| {
            if req.query("key") != Some("shop-secret") {
                return Err(ApiError::forbidden());
            }
            let (kind, room) = api.room_of(req.param("id")).ok_or_else(|| ApiError::new(409, "not playing"))?;
            api.command(&kind, &room, json!({ "grant": req.param("id"), "coins": req.body["coins"] }))
                .ok_or_else(ApiError::not_found)
        })
        // The same gift, without asking where she is: in a room, signed in
        // but in none, or away, it is one step wherever her state lives.
        .post("/api/grant/{id}", |req, api| {
            if req.query("key") != Some("shop-secret") {
                return Err(ApiError::forbidden());
            }
            let coins = req.body["coins"].as_f64().ok_or_else(|| ApiError::bad("how many?"))?;
            let state = api.edit_player(req.param("id"), move |state| {
                state["coins"] = json!(state["coins"].as_f64().unwrap_or(0.0) + coins);
            })?;
            Ok(json!({ "coins": state["coins"] }))
        })
        .post("/api/ruin/{id}", |req, api| api.edit_player(req.param("id"), |state| *state = json!("not an object")))
        .get("/api/broken", |_req, _api| panic!("a bug in a route"))
        .start("127.0.0.1:0")
        .unwrap()
}

/// One request. Returns the status, the body, and the CORS header.
fn call(addr: SocketAddr, method: HttpMethod, path: &str, token: &str, body: Option<Value>) -> (u16, Value) {
    let mut http = http_client();
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    if !token.is_empty() {
        headers.push(("Authorization".to_string(), format!("Bearer {token}")));
    }
    http.send(HttpRequest {
        id: "api".into(),
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

fn get(addr: SocketAddr, path: &str, token: &str) -> (u16, Value) {
    call(addr, HttpMethod::Get, path, token, None)
}

/// Ada, signed in, in the meadow, with a coin picked up.
fn ada_with_a_coin(server: &Running) -> (Client, String, String) {
    let mut ada = Client::connect(&server.url());
    let welcome = ada.hello("Ada", "");
    let (id, token) = (welcome["player"].as_str().unwrap().to_string(), welcome["token"].as_str().unwrap().to_string());
    ada.send(json!({ "t": "join", "kind": "zone", "room": "meadow" }));
    ada.wait("joined");
    ada.send(json!({ "t": "send", "body": { "go": [560, 300] } }));
    ada.wait("coins");
    (ada, id, token)
}

#[test]
fn a_players_stats_are_what_the_room_has_this_moment() {
    let server = game(Arc::new(MemoryStore::new()));
    let (ada, id, _) = ada_with_a_coin(&server);

    // The coin was picked up a moment ago and nothing has been saved since:
    // the answer comes from the room.
    let (status, card) = get(server.addr, &format!("/api/players/{id}"), "");
    assert_eq!(status, 200, "{card}");
    assert_eq!(
        card,
        json!({ "name": "Ada", "coins": 1.0, "online": true, "room": { "kind": "zone", "id": "meadow" } })
    );

    let (status, nobody) = get(server.addr, "/api/players/p_nobody", "");
    assert_eq!((status, nobody), (404, json!({ "error": "not found" })));

    let (_, status_page) = get(server.addr, "/api/status", "");
    assert_eq!(status_page, json!({ "online": 1, "rooms": [{ "kind": "zone", "id": "meadow", "players": 1 }] }));

    let (_, zone) = get(server.addr, "/api/zones/meadow", "");
    assert_eq!(zone["players"][0]["id"], id.as_str());
    assert_eq!(get(server.addr, "/api/zones/atlantis", "").0, 404, "a room that is not open is not opened by asking");
    drop(ada);
    server.stop();
}

#[test]
fn a_player_who_has_gone_is_answered_from_what_was_saved() {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let server = game(store);
    let (ada, id, token) = ada_with_a_coin(&server);
    drop(ada);
    // Gone, and saved: a fresh sign-in sees the coin.
    let (again, _) = hello_when(&server.url(), &token, |w| w["state"]["coins"] == 1.0);
    drop(again);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (_, card) = get(server.addr, &format!("/api/players/{id}"), "");
        if card["online"] == false {
            assert_eq!(card, json!({ "name": "Ada", "coins": 1.0, "online": false, "room": null }));
            break;
        }
        assert!(Instant::now() < deadline, "she never went offline: {card}");
        std::thread::sleep(Duration::from_millis(20));
    }
    server.stop();
}

#[test]
fn mine_means_the_token_says_so() {
    let server = game(Arc::new(MemoryStore::new()));
    let (ada, id, token) = ada_with_a_coin(&server);

    let (status, me) = get(server.addr, "/api/me", &token);
    assert_eq!(status, 200, "{me}");
    assert_eq!((me["id"].as_str(), me["state"]["coins"].as_f64()), (Some(id.as_str()), Some(1.0)));

    // No token is not a new guest, and a wrong one is nobody.
    for wrong in ["", "not-a-token"] {
        let (status, body) = get(server.addr, "/api/me", wrong);
        assert_eq!(status, 401, "token {wrong:?} got {body}");
    }
    let (_, status_page) = get(server.addr, "/api/status", "");
    assert_eq!(status_page["online"], 1, "and asking minted no player");

    // Writing: only as yourself, and only what makes sense.
    let (status, _) = call(server.addr, HttpMethod::Post, "/api/motto", "", Some(json!({ "motto": "hm" })));
    assert_eq!(status, 401);
    let (status, body) = call(server.addr, HttpMethod::Post, "/api/motto", &token, Some(json!({})));
    assert_eq!((status, body), (400, json!({ "error": "send a motto" })));
    let (status, _) = call(server.addr, HttpMethod::Post, "/api/motto", &token, Some(json!({ "motto": "onwards" })));
    assert_eq!(status, 200);
    let (_, motto) = get(server.addr, &format!("/api/motto/{id}?loud=1"), "");
    assert_eq!(motto, json!({ "motto": "onwards", "loud": true }));
    drop(ada);
    server.stop();
}

/// What writing the store could not do: the room has her, so the room is told.
#[test]
fn a_route_can_tell_a_room_to_change_somebody_who_is_playing() {
    let server = game(Arc::new(MemoryStore::new()));
    let (mut ada, id, _) = ada_with_a_coin(&server);
    let gift = format!("/api/gift/{id}");

    let (status, _) = call(server.addr, HttpMethod::Post, &gift, "", Some(json!({ "coins": 5 })));
    assert_eq!(status, 403, "not without the secret");
    let (status, answer) =
        call(server.addr, HttpMethod::Post, &format!("{gift}?key=shop-secret"), "", Some(json!({ "coins": 5 })));
    assert_eq!((status, answer), (200, json!({ "ok": true, "coins": 6.0 })));

    // She is told, on her socket, and her card says so at once.
    ada.until("the gift", |f| f.iter().any(|x| x["t"] == "coins" && x["coins"] == 6.0));
    assert_eq!(get(server.addr, &format!("/api/players/{id}"), "").1["coins"], 6.0);

    let (status, _) =
        call(server.addr, HttpMethod::Post, "/api/gift/p_nobody?key=shop-secret", "", Some(json!({ "coins": 5 })));
    assert_eq!(status, 409);
    server.stop();
}

#[test]
fn a_players_state_is_edited_wherever_it_lives() {
    let server = game(Arc::new(MemoryStore::new()));
    let (mut ada, id, token) = ada_with_a_coin(&server);
    let grant = format!("/api/grant/{id}?key=shop-secret");
    let give = |n: i64| call(server.addr, HttpMethod::Post, &grant, "", Some(json!({ "coins": n })));

    // In a room: the room's copy is changed, and she is told.
    assert_eq!(give(5), (200, json!({ "coins": 6.0 })));
    ada.until("her new state", |f| f.iter().any(|x| x["t"] == "state" && x["state"]["coins"] == 6.0));
    assert_eq!(get(server.addr, &format!("/api/players/{id}"), "").1["coins"], 6.0);

    // An edit that would leave something no room can read changes nothing.
    let (status, _) = call(server.addr, HttpMethod::Post, &format!("/api/ruin/{id}"), "", None);
    assert_eq!(status, 500);
    assert_eq!(get(server.addr, &format!("/api/players/{id}"), "").1["coins"], 6.0);

    // Signed in, in no room: her connection has it.
    ada.send(json!({ "t": "leave" }));
    ada.wait("left");
    assert_eq!(give(1), (200, json!({ "coins": 7.0 })));
    ada.until("her new state", |f| f.iter().any(|x| x["t"] == "state" && x["state"]["coins"] == 7.0));

    // Away: the saved record, which is what she signs in with next.
    drop(ada);
    let deadline = Instant::now() + Duration::from_secs(5);
    while get(server.addr, &format!("/api/players/{id}"), "").1["online"] != false {
        assert!(Instant::now() < deadline, "she never went offline");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(give(3), (200, json!({ "coins": 10.0 })));
    let (_, welcome) = hello_when(&server.url(), &token, |w| w["state"]["coins"] == 10.0);
    assert_eq!(welcome["state"]["coins"], 10.0);

    let nobody = call(server.addr, HttpMethod::Post, "/api/grant/p_nobody?key=shop-secret", "", Some(json!({ "coins": 1 })));
    assert_eq!(nobody.0, 404);
    server.stop();
}

/// Twenty grants racing a player who is picking coins up lose none of them.
#[test]
fn edits_made_at_once_are_all_kept() {
    let server = game(Arc::new(MemoryStore::new()));
    let (ada, id, _) = ada_with_a_coin(&server);
    let addr = server.addr;
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let grant = format!("/api/grant/{id}?key=shop-secret");
            std::thread::spawn(move || {
                for _ in 0..5 {
                    assert_eq!(call(addr, HttpMethod::Post, &grant, "", Some(json!({ "coins": 1 }))).0, 200);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(get(server.addr, &format!("/api/players/{id}"), "").1["coins"], 21.0);
    drop(ada);
    server.stop();
}

#[test]
fn one_address_cannot_flood_the_routes() {
    let server = Server::new()
        .config(Config { api_requests_per_second: 5.0, ..Config::default() })
        .public_stats()
        .get("/api/status", |_req, api| Ok(json!({ "online": api.online() })))
        .start("127.0.0.1:0")
        .unwrap();
    let answers: Vec<u16> = (0..14).map(|_| get(server.addr, "/api/status", "").0).collect();
    assert_eq!(answers[..10], [200; 10], "a burst of twice the rate is let through");
    assert!(answers[10..].contains(&429), "{answers:?}");
    std::thread::sleep(Duration::from_millis(450));
    assert_eq!(get(server.addr, "/api/status", "").0, 200, "and it is let back in as time passes");
    assert!(common::http_get(server.addr, "/metrics").contains("api_limited_total"));
    server.stop();
}

#[test]
fn a_route_that_panics_fails_one_request() {
    let server = game(Arc::new(MemoryStore::new()));
    let (status, body) = get(server.addr, "/api/broken", "");
    assert_eq!((status, body), (500, json!({ "error": "the route failed" })));
    assert_eq!(get(server.addr, "/api/status", "").0, 200, "and the next one is served");
    assert_eq!(call(server.addr, HttpMethod::Post, "/api/status", "", None).0, 405, "a method nobody registered");
    let metrics = common::http_get(server.addr, "/metrics");
    assert!(metrics.contains("api_requests_total 2"), "{metrics}");
    assert!(metrics.contains("api_failures_total 1"), "{metrics}");
    server.stop();
}

/// A page on another site may call the API: the browser asks first.
#[test]
fn a_browser_on_another_site_may_ask() {
    let server = game(Arc::new(MemoryStore::new()));
    let answer = ureq::request("OPTIONS", &format!("http://{}/api/me", server.addr)).call().unwrap();
    assert_eq!(answer.status(), 204);
    assert_eq!(answer.header("access-control-allow-origin"), Some("*"));
    assert!(answer.header("access-control-allow-headers").unwrap().contains("authorization"));
    let answer = ureq::get(&format!("http://{}/api/status", server.addr)).call().unwrap();
    assert_eq!(answer.header("access-control-allow-origin"), Some("*"));
    server.stop();

    // A game that names its own site: that origin is allowed back by name,
    // and another gets no allow header at all - which is a browser's no.
    let server = Server::new()
        .config(Config { api_allow_origins: Some(vec!["https://game.example".into()]), ..Config::default() })
        .get("/api/status", |_req, api| Ok(json!({ "online": api.online() })))
        .start("127.0.0.1:0")
        .unwrap();
    let url = format!("http://{}/api/status", server.addr);
    let ours = ureq::get(&url).set("Origin", "https://game.example").call().unwrap();
    assert_eq!(ours.header("access-control-allow-origin"), Some("https://game.example"));
    assert_eq!(ours.header("vary"), Some("Origin"));
    let theirs = ureq::get(&url).set("Origin", "https://evil.example").call().unwrap();
    assert_eq!(theirs.header("access-control-allow-origin"), None);
    assert_eq!(theirs.status(), 200, "the answer itself is served; the browser is what withholds it");
    let asked = ureq::request("OPTIONS", &url).set("Origin", "https://evil.example").call().unwrap();
    assert_eq!(asked.header("access-control-allow-origin"), None);
    server.stop();
}

/// Every route is counted and timed on its own, for the dashboard and for
/// `/metrics`; and who a token is remembered briefly, so a page polling
/// its own stats does not sign in for every request.
#[test]
fn routes_are_counted_each_and_a_caller_is_remembered() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counting = calls.clone();
    struct Counting(Arc<std::sync::atomic::AtomicUsize>);
    impl jengine_server::Authenticator for Counting {
        fn authenticate(&self, hello: &Value, store: &dyn Store) -> Result<jengine_server::Identity, String> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            jengine_server::GuestAuth.authenticate(hello, store)
        }
    }
    let server = Server::new()
        .auth(Counting(counting))
        .admin_token("s3cret")
        .public_stats()
        .room("zone", KindOptions::ticking(30.0).max_players(500).persistent(), world::Zone::new)
        .get("/api/me", |_req, api| Ok(json!({ "me": api.caller()? })))
        .get("/api/slow", |_req, _api| {
            std::thread::sleep(Duration::from_millis(30));
            Ok(json!({}))
        })
        .get("/api/broken", |_req, _api| panic!("a bug in a route"))
        .start("127.0.0.1:0")
        .unwrap();
    let (ada, id, token) = ada_with_a_coin(&server);
    let signed_in = calls.load(std::sync::atomic::Ordering::Relaxed);
    for _ in 0..5 {
        let (status, me) = get(server.addr, "/api/me", &token);
        assert_eq!((status, me["me"].as_str()), (200, Some(id.as_str())));
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), signed_in + 1, "five calls, one sign-in");
    get(server.addr, "/api/slow", "");
    get(server.addr, "/api/slow", "");
    get(server.addr, "/api/broken", "");

    let (status, overview) = call(server.addr, HttpMethod::Get, "/admin/api/overview", "s3cret", None);
    assert_eq!(status, 200);
    let rows = overview["api_routes"].as_array().unwrap();
    let row = |route: &str| rows.iter().find(|r| r["route"] == route).unwrap_or_else(|| panic!("{route} in {rows:?}")).clone();
    assert_eq!((row("GET /api/me")["requests"].as_u64(), row("GET /api/me")["failures"].as_u64()), (Some(5), Some(0)));
    assert_eq!((row("GET /api/broken")["requests"].as_u64(), row("GET /api/broken")["failures"].as_u64()), (Some(1), Some(1)));
    assert!(row("GET /api/slow")["avg_ms"].as_f64().unwrap() >= 30.0, "{}", row("GET /api/slow"));
    assert!(row("GET /api/me")["max_ms"].as_f64().unwrap() < 30.0);
    let metrics = common::http_get(server.addr, "/metrics");
    assert!(metrics.contains("jengine_api_route_requests_total{route=\"GET /api/slow\"} 2"), "{metrics}");
    assert!(metrics.contains("jengine_api_route_failures_total{route=\"GET /api/broken\"} 1"), "{metrics}");
    drop(ada);
    server.stop();
}

/// The whole port over TLS: the socket is `wss://`, the routes `https://`.
#[test]
fn the_game_port_can_be_tls() {
    use jengine_server::{Tls, listen};
    let dir = std::env::temp_dir().join(format!("jengine-game-tls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (cert, key) = listen::self_signed(&["localhost".into(), "127.0.0.1".into()]).unwrap();
    std::fs::write(dir.join("cert.pem"), &cert).unwrap();
    std::fs::write(dir.join("key.pem"), &key).unwrap();
    let server = Server::new()
        .tls(Tls::new(dir.join("cert.pem"), dir.join("key.pem")))
        .get("/api/status", |_req, api| Ok(json!({ "online": api.online() })))
        .start("127.0.0.1:0")
        .unwrap();
    assert!(server.url().starts_with("wss://"));
    assert!(server.http_url().starts_with("https://"));
    // A client that trusts the certificate - pinned, as a test must.
    let client = listen::client(Some(&dir.join("cert.pem"))).unwrap();
    let health = client.get(&format!("{}/healthz", server.http_url())).call().unwrap();
    assert_eq!(health.into_string().unwrap(), "ok");
    let status: Value = client.get(&format!("{}/api/status", server.http_url())).call().unwrap().into_json().unwrap();
    assert_eq!(status, json!({ "online": 0 }));
    // Plain HTTP at a TLS port is refused, not served.
    assert!(ureq::get(&format!("http://{}/healthz", server.addr)).call().is_err());
    server.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the typed routes cannot say, a game says in axum itself.
#[test]
fn a_game_can_merge_its_own_axum_router() {
    use axum::routing::get as axum_get;
    let server = Server::new()
        .routes(|router| router.route("/hello.txt", axum_get(|| async { ([("content-type", "text/plain")], "hello") })))
        .start("127.0.0.1:0")
        .unwrap();
    let answer = ureq::get(&format!("http://{}/hello.txt", server.addr)).call().unwrap();
    assert_eq!(answer.header("content-type"), Some("text/plain"));
    assert_eq!(answer.into_string().unwrap(), "hello");
    assert_eq!(common::http_get(server.addr, "/healthz"), "ok", "the server's own routes are untouched");
    server.stop();
}

#[test]
#[should_panic(expected = "starts with /api/")]
fn a_route_outside_api_is_refused_at_start_up() {
    let _ = Server::new().get("/healthz", |_req, _api| Ok(json!({})));
}
