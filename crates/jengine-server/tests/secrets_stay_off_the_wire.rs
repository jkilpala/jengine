//! What the server does with a secret that would cross a network in plain
//! text - a password, an admin token - and the other things an operator
//! needs to keep a server's secrets few and short-lived: tokens added and
//! taken away at runtime, players signed out everywhere, and an `/api/`
//! budget per caller and per route.
//!
//! "Crossed a network" is told by [`jengine_server::listen::arrived_in_clear`]:
//! from this machine with nobody named is local; through the gateway (which
//! is on this machine, and believed) for a client elsewhere it is what the
//! gateway says in `X-Forwarded-Proto`. So these tests stand in for a player
//! on another machine by sending, from loopback, the headers the gateway
//! sends for one.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use jengine_server::{Config, PASSWORD_NEEDS_TLS, PasswordAuth, Role, Server, SignedAuth};
use serde_json::{Value, json};

const TOKEN: &str = "an-admin-token-long-enough";

/// A WebSocket written by hand, so a test can send the headers a gateway
/// would - the engine's own client sends none of its own.
struct RawWs(TcpStream);

impl RawWs {
    fn open(addr: SocketAddr, headers: &[(&str, &str)]) -> RawWs {
        let mut socket = TcpStream::connect(addr).unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut head = format!(
            "GET /ws HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n"
        );
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        socket.write_all(head.as_bytes()).unwrap();
        let mut answer = Vec::new();
        let mut byte = [0u8; 1];
        while !answer.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).unwrap();
            answer.push(byte[0]);
        }
        let answer = String::from_utf8_lossy(&answer);
        assert!(answer.starts_with("HTTP/1.1 101"), "not upgraded: {answer}");
        RawWs(socket)
    }

    fn send(&mut self, frame: Value) {
        let payload = frame.to_string().into_bytes();
        let mask = [0x11u8, 0x22, 0x33, 0x44];
        let mut out = vec![0x81u8];
        match payload.len() {
            n if n < 126 => out.push(0x80 | n as u8),
            n => {
                out.push(0x80 | 126);
                out.extend_from_slice(&(n as u16).to_be_bytes());
            }
        }
        out.extend_from_slice(&mask);
        out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.0.write_all(&out).unwrap();
    }

    /// The next text frame from the server, as JSON.
    fn recv(&mut self) -> Value {
        loop {
            let mut head = [0u8; 2];
            self.0.read_exact(&mut head).unwrap();
            let mut len = u64::from(head[1] & 0x7f);
            if len == 126 {
                let mut ext = [0u8; 2];
                self.0.read_exact(&mut ext).unwrap();
                len = u64::from(u16::from_be_bytes(ext));
            } else if len == 127 {
                let mut ext = [0u8; 8];
                self.0.read_exact(&mut ext).unwrap();
                len = u64::from_be_bytes(ext);
            }
            let mut payload = vec![0u8; len as usize];
            self.0.read_exact(&mut payload).unwrap();
            if head[0] & 0x0f == 1 {
                return serde_json::from_slice(&payload).unwrap();
            }
        }
    }
}

/// The headers the gateway on this machine sends for a player on the LAN
/// reaching it over plain `ws://` - and over `wss://`.
const FROM_THE_LAN: &[(&str, &str)] = &[("X-Forwarded-For", "203.0.113.40"), ("X-Forwarded-Proto", "http")];
const FROM_THE_LAN_OVER_TLS: &[(&str, &str)] = &[("X-Forwarded-For", "203.0.113.40"), ("X-Forwarded-Proto", "https")];

fn accounts() -> Server {
    Server::new().relay().auth(PasswordAuth::new().iterations(50).allow_guests())
}

/// S13: a password on a socket that crossed the network unencrypted is
/// turned away before it is checked; over TLS, or from this machine, it is
/// taken - and a server told the LAN is trusted takes it there too.
#[test]
fn a_password_is_never_taken_in_plain_text_from_another_machine() {
    let server = accounts().start("127.0.0.1:0").unwrap();
    let register = json!({ "t": "hello", "account": "ada", "password": "correct horse", "register": true });

    let mut far = RawWs::open(server.addr, FROM_THE_LAN);
    far.send(register.clone());
    assert_eq!(far.recv(), json!({ "t": "error", "why": PASSWORD_NEEDS_TLS }));
    // A guest has no password to give away, so it is let in the same way.
    far.send(json!({ "t": "hello", "name": "visitor" }));
    assert_eq!(far.recv()["t"], "welcome");

    let mut secure = RawWs::open(server.addr, FROM_THE_LAN_OVER_TLS);
    secure.send(register.clone());
    let welcome = secure.recv();
    assert_eq!(welcome["t"], "welcome", "{welcome}");

    // Changing a password says two of them: the same rule.
    let mut plain = RawWs::open(server.addr, FROM_THE_LAN);
    plain.send(json!({ "t": "hello", "token": welcome["token"] }));
    assert_eq!(plain.recv()["t"], "welcome", "a session token is not a password");
    plain.send(json!({ "t": "account", "change_password": { "old": "correct horse", "new": "battery staple" } }));
    assert_eq!(plain.recv()["why"], PASSWORD_NEEDS_TLS);

    // From this very machine, with nobody named: it never crossed a wire.
    let mut local = RawWs::open(server.addr, &[]);
    local.send(json!({ "t": "hello", "account": "ada", "password": "correct horse" }));
    assert_eq!(local.recv()["t"], "welcome");
    server.stop();

    let lan = accounts().allow_plain_passwords().start("127.0.0.1:0").unwrap();
    let mut far = RawWs::open(lan.addr, FROM_THE_LAN);
    far.send(register);
    assert_eq!(far.recv()["t"], "welcome", "a LAN the operator trusts");
    lan.stop();
}

fn call(addr: SocketAddr, method: &str, path: &str, token: Option<&str>, headers: &[(&str, &str)], body: Option<Value>) -> (u16, Value) {
    let mut request = ureq::request(method, &format!("http://{addr}{path}")).timeout(Duration::from_secs(10));
    if let Some(token) = token {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }
    for (k, v) in headers {
        request = request.set(k, v);
    }
    let result = match body {
        Some(body) => request.send_json(body),
        None if method == "GET" => request.call(),
        None => request.send_string(""),
    };
    let response = match result {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("{method} {path}: {e}"),
    };
    let status = response.status();
    let text = response.into_string().unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// H7, M4: the admin token is refused when it crossed a network in plain
/// text - the dashboard page still loads, to say why - and taken over TLS
/// or from this machine. `/stats` and `/metrics` want the token by default.
#[test]
fn the_admin_token_and_the_numbers_stay_off_a_plain_network() {
    let server = Server::new().relay().admin_token(TOKEN).start("127.0.0.1:0").unwrap();
    let (status, body) = call(server.addr, "GET", "/admin/api/overview", Some(TOKEN), FROM_THE_LAN, None);
    assert_eq!(status, 403, "{body}");
    assert!(body["error"].as_str().unwrap().contains("encrypted"), "{body}");
    assert_eq!(call(server.addr, "GET", "/admin", None, FROM_THE_LAN, None).0, 200, "the page carries nothing");
    assert_eq!(call(server.addr, "GET", "/admin/api/overview", Some(TOKEN), FROM_THE_LAN_OVER_TLS, None).0, 200);
    assert_eq!(call(server.addr, "GET", "/admin/api/overview", Some(TOKEN), &[], None).0, 200, "from this machine");

    for path in ["/stats", "/metrics"] {
        assert_eq!(call(server.addr, "GET", path, None, &[], None).0, 401, "{path} is private by default");
        assert_eq!(call(server.addr, "GET", path, Some(TOKEN), &[], None).0, 200, "{path} with the token");
        assert_eq!(call(server.addr, "GET", path, Some(TOKEN), FROM_THE_LAN, None).0, 403, "{path}'s token in plain text");
    }
    assert_eq!(call(server.addr, "GET", "/healthz", None, FROM_THE_LAN, None).0, 200, "a load balancer needs nothing");
    server.stop();

    let open = Server::new().relay().public_stats().admin_token(TOKEN).admin_over_plain().start("127.0.0.1:0").unwrap();
    assert_eq!(call(open.addr, "GET", "/stats", None, FROM_THE_LAN, None).0, 200, "asked to be public");
    assert_eq!(call(open.addr, "GET", "/admin/api/overview", Some(TOKEN), FROM_THE_LAN, None).0, 200, "a LAN the operator trusts");
    open.stop();
}

/// U4/M2: operators are added and taken away while the server runs, each
/// with their own token and role, kept in the store only as a hash - so a
/// token is rotated by giving the name a new one, and a leaked one is ended
/// without a restart.
#[test]
fn operators_come_and_go_while_the_server_runs() {
    let store = std::sync::Arc::new(jengine_server::MemoryStore::new());
    let server = Server::new().relay().store(store.clone()).admin_token(TOKEN).start("127.0.0.1:0").unwrap();
    let add = |name: &str, role: &str| call(server.addr, "POST", "/admin/api/operators", Some(TOKEN), &[], Some(json!({ "name": name, "role": role })));
    let (status, made) = add("eve", "viewer");
    assert_eq!(status, 200, "{made}");
    let eve = made["token"].as_str().unwrap().to_string();
    let (_, made) = add("bo", "operator");
    let bo = made["token"].as_str().unwrap().to_string();

    let kept = jengine_server::Store::load(&*store, "operator", "eve").unwrap();
    assert!(!kept.contains(&eve), "the store keeps a hash, not the token: {kept}");

    assert_eq!(call(server.addr, "GET", "/admin/api/overview", Some(&eve), &[], None).0, 200, "a viewer looks");
    let (status, _) = call(server.addr, "POST", "/admin/api/notice", Some(&eve), &[], Some(json!({ "text": "hi" })));
    assert_eq!(status, 403, "and changes nothing");
    assert_eq!(call(server.addr, "POST", "/admin/api/notice", Some(&bo), &[], Some(json!({ "text": "hi" }))).0, 200);
    let (_, events) = call(server.addr, "GET", "/admin/api/events", Some(TOKEN), &[], None);
    assert!(events.to_string().contains("bo: notice"), "what bo did is under bo's name");

    let listed = call(server.addr, "GET", "/admin/api/operators", Some(&eve), &[], None).1;
    let mut names: Vec<&str> = listed["operators"].as_array().unwrap().iter().filter_map(|o| o["name"].as_str()).collect();
    names.sort();
    assert_eq!(names, ["admin", "bo", "eve"]);
    assert!(!listed.to_string().contains(&eve), "no token is ever listed");

    // Rotated: the old token stops at once, the new one works.
    let (_, again) = add("eve", "viewer");
    assert_eq!(call(server.addr, "GET", "/admin/api/overview", Some(&eve), &[], None).0, 401);
    let eve = again["token"].as_str().unwrap().to_string();
    assert_eq!(call(server.addr, "GET", "/admin/api/overview", Some(&eve), &[], None).0, 200);
    // Taken away.
    assert_eq!(call(server.addr, "DELETE", "/admin/api/operators/eve", Some(&eve), &[], None).0, 403, "a viewer cannot");
    assert_eq!(call(server.addr, "DELETE", "/admin/api/operators/eve", Some(&bo), &[], None).0, 200);
    assert_eq!(call(server.addr, "GET", "/admin/api/overview", Some(&eve), &[], None).0, 401);
    assert_eq!(call(server.addr, "DELETE", "/admin/api/operators/admin", Some(&bo), &[], None).0, 409, "one given in code stays");
    assert_eq!(add("admin", "viewer").0, 409);
    server.stop();
}

/// S13: an operator ends every session of one player - their open
/// connection, and every ticket issued to them until now.
#[test]
fn a_player_is_signed_out_everywhere_by_an_operator() {
    let server = Server::new().relay().auth(SignedAuth::new("shared-secret")).admin_token(TOKEN).start("127.0.0.1:0").unwrap();
    let ticket = SignedAuth::jwt("shared-secret", "user-42", "Ada", 3600);
    let mut ada = RawWs::open(server.addr, &[]);
    ada.send(json!({ "t": "hello", "token": ticket }));
    assert_eq!(ada.recv()["t"], "welcome");
    let (status, body) = call(server.addr, "POST", "/admin/api/player/user-42/sign_out", Some(TOKEN), &[], None);
    assert_eq!((status, body["was_online"].clone()), (200, json!(true)), "{body}");
    assert_eq!(ada.recv()["why"], "signed out by an operator");
    let mut again = RawWs::open(server.addr, &[]);
    again.send(json!({ "t": "hello", "token": ticket }));
    assert_eq!(again.recv()["why"], "that ticket was revoked");
    server.stop();
}

/// A1: a caller with a token has a budget of their own, whatever address
/// they share; an expensive route has one too; and a body past the limit
/// never reaches the route.
#[test]
fn the_api_budget_is_per_caller_and_per_route() {
    let server = Server::new()
        .config(Config { api_requests_per_second: 5.0, api_token_requests_per_second: 5.0, api_max_body_bytes: 1000, ..Config::default() })
        .get("/api/cheap", |_req, _api| Ok(json!({ "ok": true })))
        .post("/api/search", |req, _api| Ok(json!({ "got": req.body.to_string().len() })))
        .route_limit("POST", "/api/search", 1.0)
        .start("127.0.0.1:0")
        .unwrap();
    // Somebody at a shared address spends the address's budget...
    let anon: Vec<u16> = (0..12).map(|_| call(server.addr, "GET", "/api/cheap", None, &[], None).0).collect();
    assert!(anon.contains(&429), "{anon:?}");
    // ...and two signed-in callers there are each let through on their own.
    for token in ["token-of-ada", "token-of-bo"] {
        let theirs: Vec<u16> = (0..10).map(|_| call(server.addr, "GET", "/api/cheap", Some(token), &[], None).0).collect();
        assert_eq!(theirs, [200; 10], "{token}");
        assert_eq!(call(server.addr, "GET", "/api/cheap", Some(token), &[], None).0, 429, "but not past their own");
    }
    // The search's own budget: a burst of two.
    let searches: Vec<u16> =
        (0..3).map(|_| call(server.addr, "POST", "/api/search", Some("token-of-cy"), &[], Some(json!({ "q": "x" }))).0).collect();
    assert_eq!(searches, [200, 200, 429]);
    std::thread::sleep(Duration::from_millis(1100));
    let big = json!({ "q": "x".repeat(2000) });
    assert_eq!(call(server.addr, "POST", "/api/search", Some("token-of-dee"), &[], Some(big)).0, 413);
    server.stop();
}

/// The roles a token can carry, by name, for the `Role` re-export.
#[test]
fn operators_named_in_code_keep_working() {
    let server = Server::new().relay().operator("watcher", "a-watchers-token", Role::Viewer).start("127.0.0.1:0").unwrap();
    assert_eq!(call(server.addr, "GET", "/admin/api/overview", Some("a-watchers-token"), &[], None).0, 200);
    server.stop();
}
