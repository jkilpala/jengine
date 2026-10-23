//! **H2, H7, U4: a host's gateway and console keep their secrets.** The
//! gateway can speak TLS and says to the servers behind it how each request
//! reached it; a server and the console refuse a token that crossed the
//! network in plain text; and the console has people of its own, each with
//! their own token and role.
//!
//! A player on another machine is stood in for the way a proxy would carry
//! one: from loopback, with `X-Forwarded-For` naming them, to a gateway told
//! to believe loopback (`--trust-proxy`).

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use jengine_server::fleet::agent::{self, HostOptions};
use jengine_server::fleet::gateway::{self, StaticRoutes};
use jengine_server::fleet::ui::Users;
use jengine_server::{Server, Tls};
use serde_json::{Value, json};

const ADMIN: &str = "a-servers-admin-token";
const TOKEN: &str = "host-secrets-agent-token";
const CONSOLE: &str = "host-secrets-console-token";
const FAR: (&str, &str) = ("X-Forwarded-For", "203.0.113.55");

fn loopback() -> Vec<std::net::IpAddr> {
    vec!["127.0.0.1".parse().unwrap()]
}

fn call(http: &ureq::Agent, method: &str, url: &str, token: Option<&str>, headers: &[(&str, &str)], body: Option<Value>) -> (u16, Value) {
    let mut request = http.request(method, url).timeout(Duration::from_secs(20));
    if let Some(token) = token {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }
    for (k, v) in headers {
        request = request.set(k, v);
    }
    let result = match body {
        Some(body) => request.send_json(body),
        None if method == "GET" => request.call(),
        None => request.send_bytes(&[]),
    };
    let response = match result {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("{method} {url}: {e}"),
    };
    let status = response.status();
    let text = response.into_string().unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

fn certificate(dir: &Path) -> Tls {
    std::fs::create_dir_all(dir).unwrap();
    let (cert, key) = jengine_server::listen::self_signed(&["127.0.0.1".into(), "localhost".into()]).unwrap();
    std::fs::write(dir.join("cert.pem"), cert).unwrap();
    std::fs::write(dir.join("key.pem"), key).unwrap();
    Tls::new(dir.join("cert.pem"), dir.join("key.pem"))
}

/// H2 and H7: given a certificate the gateway speaks TLS, lists `wss://`
/// addresses, and tells the server behind it so - which is what lets a token
/// from another machine through; the same token over the plain gateway is
/// refused by the server.
#[test]
fn a_gateway_with_a_certificate_speaks_tls_and_says_so() {
    let dir = std::env::temp_dir().join(format!("jengine-gateway-tls-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let tls = certificate(&dir);
    let server = Server::new().relay().admin_token(ADMIN).start("127.0.0.1:0").unwrap();
    let routes = || StaticRoutes(BTreeMap::from([("alpha".to_string(), server.addr.port())]));
    let secure = gateway::start_with("127.0.0.1:0", routes(), loopback(), Some(tls.clone())).unwrap();
    let plain = gateway::start_with("127.0.0.1:0", routes(), loopback(), None).unwrap();
    assert!(secure.url().starts_with("wss://") && plain.url().starts_with("ws://"));
    let https = jengine_server::listen::client(Some(&tls.cert)).unwrap();
    let http = ureq::agent();

    let (status, body) = call(&https, "GET", &format!("https://{}/alpha/healthz", secure.addr), None, &[], None);
    assert_eq!((status, body), (200, json!("ok")));
    let (_, listed) = call(&https, "GET", &format!("https://{}/", secure.addr), None, &[], None);
    assert_eq!(listed["services"][0]["url"], format!("wss://{}/alpha", secure.addr), "{listed}");
    // Plain HTTP to the TLS gateway is not answered as HTTP.
    let refused = http.get(&format!("http://{}/alpha/healthz", secure.addr)).timeout(Duration::from_secs(5)).call();
    assert!(refused.is_err(), "a TLS port answered plain HTTP");

    let overview = "admin/api/overview";
    let (status, _) = call(&https, "GET", &format!("https://{}/alpha/{overview}", secure.addr), Some(ADMIN), &[FAR], None);
    assert_eq!(status, 200, "over the gateway's TLS from another machine: taken");
    let (status, body) = call(&http, "GET", &format!("http://{}/alpha/{overview}", plain.addr), Some(ADMIN), &[FAR], None);
    assert_eq!(status, 403, "over the plain gateway from another machine: refused - {body}");
    // A proxy in front saying the player reached *it* over TLS is believed.
    let (status, _) = call(&http, "GET", &format!("http://{}/alpha/{overview}", plain.addr), Some(ADMIN), &[FAR, ("X-Forwarded-Proto", "https")], None);
    assert_eq!(status, 200);
    // A stranger saying so is not: the gateway that does not trust them
    // writes its own, and names the stranger as who they are.
    let wary = gateway::start_with("127.0.0.1:0", routes(), Vec::new(), None).unwrap();
    let (status, _) = call(&http, "GET", &format!("http://{}/alpha/{overview}", wary.addr), Some(ADMIN), &[FAR, ("X-Forwarded-Proto", "https")], None);
    assert_eq!(status, 200, "from this machine it never crossed a wire, whatever it claims");

    for gw in [secure, plain, wary] {
        gw.stop();
    }
    server.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// U4 and H7 at the console: a token from another machine over the plain
/// gateway is refused there, and people sign in with tokens of their own -
/// a viewer looks, an operator acts, and what they did says who.
#[test]
fn a_console_takes_tokens_only_over_tls_and_knows_its_people() {
    let dir = std::env::temp_dir().join(format!("jengine-console-secrets-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let start = |allow_plain: bool| {
        let host = HostOptions {
            gateway: Some("127.0.0.1:0".into()),
            console_token: Some(CONSOLE.into()),
            trust_proxies: loopback(),
            allow_plain_console: allow_plain,
            ..Default::default()
        };
        agent::start_host("127.0.0.1:0", dir.join("data"), TOKEN, None, host).unwrap()
    };
    let host = start(false);
    let base = format!("http://{}/manage", host.gateway.unwrap());
    let http = ureq::agent();

    let (status, body) = call(&http, "GET", &format!("{base}/api/status"), Some(CONSOLE), &[FAR], None);
    assert_eq!(status, 403, "{body}");
    assert!(body["error"].as_str().unwrap().contains("--gateway-tls-cert"), "the refusal says what to do: {body}");
    assert_eq!(call(&http, "GET", &format!("{base}/"), None, &[FAR], None).0, 200, "the page itself carries nothing");
    assert_eq!(call(&http, "GET", &format!("{base}/api/status"), Some(CONSOLE), &[], None).0, 200, "from this machine");

    let users = dir.join("data").join("console").join("users.json");
    let eve = Users::add(&users, "eve", "viewer").unwrap();
    let bo = Users::add(&users, "bo", "operator").unwrap();
    assert!(!std::fs::read_to_string(&users).unwrap().contains(&eve), "only a hash is kept");
    assert_eq!(call(&http, "GET", &format!("{base}/api/status"), Some(&eve), &[], None).0, 200, "a viewer looks");
    let act = json!({ "service": "relay", "action": "restart" });
    let (status, body) = call(&http, "POST", &format!("{base}/api/act"), Some(&eve), &[], Some(act.clone()));
    assert_eq!(status, 403, "and does not act: {body}");
    let (status, body) = call(&http, "POST", &format!("{base}/api/act"), Some(&bo), &[], Some(act));
    assert_eq!(status, 200, "an operator acts: {body}");
    let (_, jobs) = call(&http, "GET", &format!("{base}/api/jobs"), Some(&eve), &[], None);
    assert_eq!(jobs["jobs"][0]["by"], "bo", "and the job says who: {jobs}");
    Users::remove(&users, "bo").unwrap();
    assert_eq!(call(&http, "GET", &format!("{base}/api/status"), Some(&bo), &[], None).0, 401, "gone at once");
    host.stop();

    // A LAN the operator trusts, said so.
    let host = start(true);
    let base = format!("http://{}/manage", host.gateway.unwrap());
    assert_eq!(call(&http, "GET", &format!("{base}/api/status"), Some(CONSOLE), &[FAR], None).0, 200);
    host.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// U1: two people's pages over one fleet file share their jobs through a
/// jobs file, and a page started again still has them.
#[test]
fn pages_share_their_jobs_through_a_file() {
    use jengine_server::fleet::ui;
    let dir = std::env::temp_dir().join(format!("jengine-ui-jobs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fleet = dir.join("fleet.json");
    std::fs::write(&fleet, r#"{ "machines": {}, "services": {} }"#).unwrap();
    let options = || ui::Options { jobs: Some(dir.join("jobs.json")), ..Default::default() };
    let http = ureq::agent();
    let page = ui::start_options("127.0.0.1:0", &fleet, None, None, options()).unwrap();
    let (status, _) = call(&http, "POST", &format!("{}/api/act", page.url()), Some(page.token()), &[], Some(json!({ "service": "relay", "action": "stop" })));
    assert_eq!(status, 200);
    // The job runs on a thread of its own; it fails (no such service) at once.
    std::thread::sleep(Duration::from_millis(300));
    page.stop();
    let again = ui::start_options("127.0.0.1:0", &fleet, None, None, options()).unwrap();
    let (_, jobs) = call(&http, "GET", &format!("{}/api/jobs", again.url()), Some(again.token()), &[], None);
    assert_eq!(jobs["jobs"][0]["title"], "stop relay", "the earlier page's job: {jobs}");
    assert_eq!(jobs["jobs"][0]["by"], "console");
    again.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
