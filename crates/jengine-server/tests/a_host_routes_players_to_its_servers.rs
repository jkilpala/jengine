//! **One port in front of several servers**: a gateway routing
//! `ws://<host>/<service>` to each server's own port - the engine's own
//! client over real sockets on both sides of it, and an agent hosting real
//! server processes behind its gateway, as a Raspberry Pi on a LAN does.

mod common;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use common::{Client, http_get};
use jengine_server::Server;
use jengine_server::fleet::agent;
use jengine_server::fleet::client::{self, Fleet};
use jengine_server::fleet::gateway::{self, StaticRoutes};
use jengine_server::fleet::platform;
use serde_json::{Value, json};

const SERVER: &str = env!("CARGO_BIN_EXE_jengine-server");
const TOKEN: &str = "host-test-token";

fn gateway_only() -> agent::HostOptions {
    agent::HostOptions { gateway: Some("127.0.0.1:0".into()), ..Default::default() }
}

fn directory(gateway: std::net::SocketAddr) -> Value {
    serde_json::from_str(&http_get(gateway, "/")).expect("the directory is JSON when HTML is not asked for")
}

#[test]
fn a_gateway_carries_sockets_and_requests_to_the_server_its_path_names() {
    let alpha = Server::new().relay().admin_token("alpha-admin").start("127.0.0.1:0").unwrap();
    let beta = Server::new().relay().start("127.0.0.1:0").unwrap();
    let routes = BTreeMap::from([("alpha".to_string(), alpha.addr.port()), ("beta".to_string(), beta.addr.port())]);
    let gw = gateway::start("127.0.0.1:0", StaticRoutes(routes)).unwrap();
    let url = |service: &str| format!("ws://{}/{service}", gw.addr);

    // Two players through the gateway share one room and one ordered log:
    // the socket is carried both ways, frame for frame.
    let mut a = Client::connect(&url("alpha"));
    let mut b = Client::connect(&format!("{}/ws", url("alpha")));
    a.send(json!({ "t": "join", "room": "shore", "name": "Ada", "seats": 2 }));
    assert_eq!(a.wait("joined")["seat"], 0);
    b.send(json!({ "t": "join", "room": "shore", "name": "Bo", "seats": 2 }));
    assert_eq!(b.wait("joined")["seat"], 1);
    a.send(json!({ "t": "send", "body": { "move": 19 } }));
    b.until("Ada's move", |f| f.iter().any(|x| x["t"] == "msg" && x["body"]["move"] == 19));

    // The other name is the other server: its room of the same name is empty.
    let mut c = Client::connect(&url("beta"));
    c.send(json!({ "t": "join", "room": "shore", "name": "Cy", "seats": 2 }));
    assert_eq!(c.wait("joined")["seat"], 0, "beta is a server of its own");

    // Plain requests reach the server's own paths, the prefix taken off -
    // the dashboard too, whose own requests are relative to where it is.
    assert_eq!(http_get(gw.addr, "/alpha/healthz"), "ok");
    assert!(http_get(gw.addr, "/alpha/admin").contains("<title>jengine-server</title>"));
    assert!(http_get(gw.addr, "/nowhere/ws").contains("no service named 'nowhere'"));

    // `/` says what is here, with the address to type.
    let listed = directory(gw.addr);
    let names: Vec<&str> = listed["services"].as_array().unwrap().iter().filter_map(|s| s["name"].as_str()).collect();
    assert_eq!(names, ["alpha", "beta"]);
    assert_eq!(listed["services"][0]["url"], format!("ws://{}/alpha", gw.addr));
    assert_eq!(listed["services"][0]["status"], "running");

    // A server that is gone is said to be, not hung on.
    beta.stop();
    assert!(http_get(gw.addr, "/beta/healthz").contains("not answering"));
    assert_eq!(directory(gw.addr)["services"][1]["status"], "down");

    gw.stop();
    alpha.stop();
}

/// Open a game socket through `gateway` as a proxy in front of it would -
/// with `X-Real-IP` and `X-Forwarded-For` naming a player - and sign in.
/// By hand, because the point is the headers, which a game client never
/// sends itself.
fn sign_in_as_a_proxy_would(gateway: std::net::SocketAddr, name: &str, real_ip: &str, forwarded: &str) -> std::net::TcpStream {
    use std::io::{Read, Write};
    let mut tcp = std::net::TcpStream::connect(gateway).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let request = [
        "GET /alpha/ws HTTP/1.1".to_string(),
        format!("Host: {gateway}"),
        "Upgrade: websocket".into(),
        "Connection: Upgrade".into(),
        "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==".into(),
        "Sec-WebSocket-Version: 13".into(),
        format!("X-Real-IP: {real_ip}"),
        format!("X-Forwarded-For: {forwarded}"),
        String::new(),
        String::new(),
    ]
    .join("\r\n");
    tcp.write_all(request.as_bytes()).unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        tcp.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
    }
    assert!(head.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&head));
    // One masked text frame: a client's frames are masked, by the protocol.
    let hello = json!({ "t": "hello", "name": name, "token": "" }).to_string();
    let mask = [7u8, 1, 9, 3];
    let mut frame = vec![0x81, 0x80 | hello.len() as u8];
    frame.extend_from_slice(&mask);
    frame.extend(hello.bytes().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    tcp.write_all(&frame).unwrap();
    tcp
}

/// H3 and H8: behind the gateway a server sees every connection come from
/// this machine, so who a player is must be carried. Through a proxy the
/// gateway trusts, the player is the address that proxy names; through one
/// it does not, the proxy itself - never what a client says, which anybody
/// could.
#[test]
fn a_trusted_proxy_names_the_player_and_an_untrusted_one_does_not() {
    let alpha = Server::new().relay().admin_token("alpha-admin").start("127.0.0.1:0").unwrap();
    let routes = || StaticRoutes(BTreeMap::from([("alpha".to_string(), alpha.addr.port())]));
    let trusting = gateway::start_trusting("127.0.0.1:0", routes(), vec!["127.0.0.1".parse().unwrap()]).unwrap();
    let wary = gateway::start("127.0.0.1:0", routes()).unwrap();
    let _ada = sign_in_as_a_proxy_would(trusting.addr, "Ada", "203.0.113.7", "10.6.6.6");
    let _bo = sign_in_as_a_proxy_would(trusting.addr, "Bo", "203.0.113.8", "10.6.6.6, 198.51.100.4");
    let _cy = sign_in_as_a_proxy_would(wary.addr, "Cy", "203.0.113.9", "203.0.113.9");

    let deadline = Instant::now() + Duration::from_secs(5);
    let addresses = loop {
        let players: Value = ureq::get(&format!("http://{}/admin/api/players", alpha.addr))
            .set("Authorization", "Bearer alpha-admin")
            .call()
            .unwrap()
            .into_json()
            .unwrap();
        let rows = players["players"].as_array().cloned().unwrap_or_default();
        if rows.len() == 3 {
            break rows.iter().map(|p| (p["name"].as_str().unwrap().to_string(), p["address"].clone())).collect::<BTreeMap<_, _>>();
        }
        assert!(Instant::now() < deadline, "not all signed in: {players}");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(addresses["Ada"], "10.6.6.6", "the last forwarded entry is the one the proxy wrote");
    assert_eq!(addresses["Bo"], "198.51.100.4", "the first is whatever the client sent it");
    assert_eq!(addresses["Cy"], "127.0.0.1", "an untrusted proxy is itself, whatever it says");
    trusting.stop();
    wary.stop();
    alpha.stop();
}

#[test]
fn a_host_runs_its_servers_behind_one_port() {
    let dir = std::env::temp_dir().join(format!("jengine-host-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let host = agent::start_host("127.0.0.1:0", dir.join("data"), TOKEN, None, gateway_only()).unwrap();
    let gw = host.gateway.expect("a host has a gateway");

    // Two services of the same server binary, each on a port only the
    // machine itself can reach - the gateway is the way in.
    let port = || std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let service = |port: u16| {
        json!({
            "machines": ["pi"],
            "artifacts": { platform(): SERVER },
            "args": ["127.0.0.1:{port}", "--db", "game.db"],
            "port": port,
            "admin_token": "host-admin",
            "drain_secs": 1,
            "health_secs": 8,
        })
    };
    let file = json!({
        "machines": { "pi": { "url": host.url(), "token": TOKEN } },
        "services": { "town": service(port()), "arena": service(port()) },
    });
    let path = dir.join("fleet.json");
    std::fs::write(&path, file.to_string()).unwrap();
    let fleet = Fleet::load(&path).unwrap();
    for name in ["town", "arena"] {
        client::deploy(&fleet, name, None, &mut |_| {}).unwrap();
    }

    let mut ada = Client::connect(&format!("ws://{gw}/town"));
    assert_eq!(ada.hello("Ada", "")["name"], "Ada");

    // The directory shows both, and who is in each - from the servers, with
    // nothing of the operator's (versions, pids, errors) on an open port.
    let deadline = Instant::now() + Duration::from_secs(5);
    let listed = loop {
        let listed = directory(gw);
        if listed["services"].as_array().is_some_and(|s| s.iter().any(|s| s["name"] == "town" && s["players"] == 1)) {
            break listed;
        }
        assert!(Instant::now() < deadline, "the directory never showed Ada: {listed:#}");
        std::thread::sleep(Duration::from_millis(50));
    };
    let services = listed["services"].as_array().unwrap();
    assert_eq!(services.len(), 2);
    assert!(services.iter().all(|s| s["status"] == "running"), "{listed:#}");
    assert!(services.iter().all(|s| s.get("pid").is_none() && s.get("version").is_none()));

    // Stopped, a service is still routed to - and says it is not answering.
    client::act(&fleet, "arena", "stop", None, &mut |_| {}).unwrap();
    assert!(http_get(gw, "/arena/healthz").contains("not answering"));

    host.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
