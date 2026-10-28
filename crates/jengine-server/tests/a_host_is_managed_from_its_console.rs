//! **A host managed from a browser**: `jengine-fleet host`'s console behind
//! its gateway at `/manage` - a build uploaded from the page, made a service,
//! deployed, played on through the gateway, its dashboard reached with the
//! console's one sign-in, and the jobs still there after the host restarts.
//! All over real sockets, with the real server executable.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::Client;
use jengine_server::fleet::agent::{self, HostOptions, RunningAgent};
use jengine_server::fleet::platform;
use serde_json::{Value, json};

const SERVER: &str = env!("CARGO_BIN_EXE_jengine-server");
const TOKEN: &str = "host-agent-token";
const CONSOLE: &str = "console-test-token";

fn start(data: &Path) -> RunningAgent {
    let host = HostOptions { gateway: Some("127.0.0.1:0".into()), console_token: Some(CONSOLE.into()), ..Default::default() };
    agent::start_host("127.0.0.1:0", data, TOKEN, None, host).unwrap()
}

/// One request; the status and the body, whatever the status.
fn call(method: &str, url: &str, token: Option<&str>, body: Option<(&str, &[u8])>) -> (u16, String) {
    let mut request = ureq::request(method, url).timeout(Duration::from_secs(30));
    if let Some(token) = token {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }
    let result = match body {
        Some((kind, bytes)) => request.set("Content-Type", kind).send_bytes(bytes),
        None if method == "GET" => request.call(),
        None => request.send_bytes(&[]),
    };
    match result {
        Ok(r) | Err(ureq::Error::Status(_, r)) => (r.status(), r.into_string().unwrap_or_default()),
        Err(e) => panic!("{method} {url}: {e}"),
    }
}

fn json_call(method: &str, url: &str, body: Value) -> (u16, Value) {
    let text = body.to_string();
    let (code, answer) = call(method, url, Some(CONSOLE), Some(("application/json", text.as_bytes())));
    (code, serde_json::from_str(&answer).unwrap_or(Value::String(answer)))
}

fn jobs(base: &str) -> Vec<Value> {
    let (code, text) = call("GET", &format!("{base}/api/jobs"), Some(CONSOLE), None);
    assert_eq!(code, 200, "{text}");
    serde_json::from_str::<Value>(&text).unwrap()["jobs"].as_array().unwrap().clone()
}

#[test]
fn a_host_is_managed_from_its_console() {
    let dir = std::env::temp_dir().join(format!("jengine-console-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let host = start(&dir.join("data"));
    let gw = host.gateway.unwrap();
    let base = format!("http://{gw}/manage");

    // The page comes through the gateway without its token: a request the
    // gateway carried is from loopback, but not from this machine.
    let (code, page) = call("GET", &base, None, None);
    assert_eq!(code, 200);
    assert!(page.contains("let token = \"\" ||"), "the console's token is never served in its page");
    assert!(!page.contains(CONSOLE) && !page.contains(TOKEN));
    assert!(page.contains("const HOSTED = \"true\""));
    // The gateway's directory points at it.
    let (_, listed) = call("GET", &format!("http://{gw}/"), None, None);
    assert_eq!(serde_json::from_str::<Value>(&listed).unwrap()["console"], base);

    // Nothing without the console's token; with it, the host is a machine.
    assert_eq!(call("GET", &format!("{base}/api/fleet"), None, None).0, 401);
    assert_eq!(call("GET", &format!("{base}/api/fleet"), Some(TOKEN.get(..4).unwrap()), None).0, 401);
    let (code, fleet) = json_call("GET", &format!("{base}/api/fleet"), json!(null));
    assert_eq!(code, 200, "{fleet}");
    assert_eq!(fleet["machines"][0]["name"], "host");
    assert!(!fleet.to_string().contains(TOKEN), "the agent's token stays in the console");

    // A build from "this computer", uploaded, made a service, deployed.
    let bytes = std::fs::read(SERVER).unwrap();
    let file = Path::new(SERVER).file_name().unwrap().to_str().unwrap();
    let (code, up) = call(
        "PUT",
        &format!("{base}/api/build/town/{}?file={file}", platform()),
        Some(CONSOLE),
        Some(("application/octet-stream", &bytes)),
    );
    assert_eq!(code, 200, "{up}");
    let up: Value = serde_json::from_str(&up).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let service = json!({
        "name": "town",
        "machines": ["host"],
        "artifacts": { platform(): up["path"] },
        "args": ["127.0.0.1:{port}", "--db", "town.db", "--liveops", "ops"],
        "port": port,
        "drain_secs": 1,
        "health_secs": 8,
    });
    let (code, answer) = json_call("PUT", &format!("{base}/api/fleet/service"), service.clone());
    assert_eq!(code, 200, "{answer}");
    let file_on_host: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("data/console/fleet.json")).unwrap()).unwrap();
    let admin = file_on_host["services"]["town"]["admin_token"].as_str().expect("the console makes an admin token").to_string();
    // Edited again from the form (which knows no token), it keeps the one it has.
    json_call("PUT", &format!("{base}/api/fleet/service"), service);
    let again: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("data/console/fleet.json")).unwrap()).unwrap();
    assert_eq!(again["services"]["town"]["admin_token"], admin);

    let (code, answer) = json_call("POST", &format!("{base}/api/deploy"), json!({ "service": "town" }));
    assert_eq!(code, 200, "{answer}");
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        let job = jobs(&base).into_iter().find(|j| j["id"] == answer["job"]).unwrap();
        if job["state"] == "ok" {
            break;
        }
        assert_ne!(job["state"], "failed", "{job:#}");
        assert!(Instant::now() < deadline, "the deploy never finished: {job:#}");
        std::thread::sleep(Duration::from_millis(100));
    }

    // The directory links its dashboard through the console - whose token
    // somebody knows - not straight to the server, whose token nobody does.
    let (_, listed) = call("GET", &format!("http://{gw}/"), None, None);
    let listed: Value = serde_json::from_str(&listed).unwrap();
    assert_eq!(listed["services"][0]["dashboard"], "/manage/dash/host/town/admin");

    // Played on through the gateway.
    let mut ada = Client::connect(&format!("ws://{gw}/town"));
    assert_eq!(ada.hello("Ada", "")["name"], "Ada");

    // The overview's numbers: the console samples the machine and every
    // server, and the server counts what Ada sent it and what it sent her.
    ada.send(json!({ "t": "ping" }));
    let deadline = Instant::now() + Duration::from_secs(20);
    let sample = loop {
        let (code, text) = call("GET", &format!("{base}/api/history?range=hour"), Some(CONSOLE), None);
        assert_eq!(code, 200, "{text}");
        let history: Value = serde_json::from_str(&text).unwrap();
        let last = history["samples"].as_array().and_then(|s| s.last()).cloned().unwrap_or(Value::Null);
        let town = &last["services"]["host/town"];
        // The machine is measured on a thread of its own, and its first
        // reading can come after the first traffic, so wait for both.
        let measured = last["machines"]["host"]["mem_total"].is_u64();
        if measured && town["out"].as_u64().unwrap_or(0) > 0 && town["in"].as_u64().unwrap_or(0) > 0 && town["players"] == 1 {
            break last;
        }
        assert!(Instant::now() < deadline, "the overview never saw Ada's traffic and the machine: {last:#}");
        std::thread::sleep(Duration::from_millis(250));
    };
    let machine = &sample["machines"]["host"];
    assert_eq!(machine["up"], true);
    assert!(machine["mem_total"].as_u64().unwrap() > machine["mem"].as_u64().unwrap(), "{machine}");
    assert!(machine["cpu"].is_number() && machine["disk_total"].as_u64().unwrap() > 0, "{machine}");
    assert!(sample["services"]["host/town"]["errors"].is_u64(), "the log is counted: {sample:#}");

    // Its dashboard, through the console: the page is open, its API takes
    // the console's token - and the server's own admin token is never asked.
    let dash = format!("{base}/dash/host/town/admin");
    let (code, page) = call("GET", &dash, None, None);
    assert_eq!(code, 200);
    assert!(page.contains("<title>jengine-server</title>"));
    assert_eq!(call("GET", &format!("{dash}/api/overview"), None, None).0, 401);
    assert_eq!(call("GET", &format!("{dash}/api/overview"), Some(&admin), None).0, 401, "only the console's token");
    let (code, overview) = call("GET", &format!("{dash}/api/overview"), Some(CONSOLE), None);
    assert_eq!(code, 200, "{overview}");
    let (code, players) = call("GET", &format!("{dash}/api/players"), Some(CONSOLE), None);
    assert_eq!(code, 200, "{players}");
    assert!(players.contains("Ada"), "the server's own view, with its player in it: {players}");

    // Live ops, which this server also runs: its page and API through the
    // console, signed in by the console's token; and what a game asks for
    // through the gateway.
    let (code, page) = call("GET", &format!("{dash}/liveops"), None, None);
    assert_eq!(code, 200);
    assert!(page.contains("<title>"), "the live ops page");
    let (code, ops) = call("GET", &format!("{dash}/api/liveops"), Some(CONSOLE), None);
    assert_eq!(code, 200, "{ops}");
    assert!(ops.contains("jlo-public-"), "{ops}");
    // It describes itself where a game reaches it - the gateway, under its
    // path - not at the loopback address the agent asked it on.
    let ops: Value = serde_json::from_str(&ops).unwrap();
    assert_eq!(ops["base_url"], format!("http://{gw}/town"), "{ops:#}");
    assert_eq!(ops["packs_url"], format!("http://{gw}/town/liveops/v1/packs/"));
    let (code, key) = call("GET", &format!("http://{gw}/town/liveops/v1/key"), None, None);
    assert_eq!(code, 200, "{key}");
    assert!(key.contains("jlo-public-"), "a game gets the key through the gateway: {key}");

    // The host restarts (a reboot): what the console did is still listed,
    // and the service is running again.
    drop(ada);
    host.stop();
    let host = start(&dir.join("data"));
    let base = format!("http://{}/manage", host.gateway.unwrap());
    let kept = jobs(&base);
    assert!(kept.iter().any(|j| j["title"] == "deploy town" && j["state"] == "ok"), "{kept:#?}");
    let deadline = Instant::now() + Duration::from_secs(20);
    while call("GET", &format!("http://{}/town/healthz", host.gateway.unwrap()), None, None).1 != "ok" {
        assert!(Instant::now() < deadline, "town never came back");
        std::thread::sleep(Duration::from_millis(100));
    }

    // `manage` is the console's; no service may take it.
    let (code, _) = json_call(
        "PUT",
        &format!("{base}/api/fleet/service"),
        json!({ "name": "manage", "machines": ["host"], "artifacts": { platform(): up["path"] }, "port": port }),
    );
    assert_eq!(code, 200, "the file may say it");
    let (_, refused) = json_call("POST", &format!("{base}/api/deploy"), json!({ "service": "manage" }));
    let deadline = Instant::now() + Duration::from_secs(20);
    let job = loop {
        let job = jobs(&base).into_iter().find(|j| j["id"] == refused["job"]).unwrap();
        if job["state"] != "running" {
            break job;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(job["state"], "failed");
    assert!(job["error"].as_str().unwrap().contains("console's path"), "{job:#}");

    host.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
