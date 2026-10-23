//! **F2: the agent's token is not the keys to the machine** when the agent
//! has a release key: it runs only releases signed by the key's secret half,
//! which stays where builds are made. A token alone can start, stop and roll
//! back what is there - not put a program of its own on the machine, nor
//! reach past a signed release through a deploy's settings.

use std::path::{Path, PathBuf};
use std::time::Duration;

use jengine_server::fleet::agent::{self, HostOptions, SIGNATURE_HEADER};
use jengine_server::fleet::client::{self, Fleet};
use jengine_server::fleet::{ReleaseSigner, platform, version_of};
use serde_json::{Value, json};

const SERVER: &str = env!("CARGO_BIN_EXE_jengine-server");
const TOKEN: &str = "signed-fleet-test-token";

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn fleet_file(dir: &Path, agent_url: &str, artifact: &Path, port: u16, key_file: Option<&Path>) -> Fleet {
    let mut file = json!({
        "machines": { "m0": { "url": agent_url, "token": TOKEN } },
        "services": { "relay": {
            "machines": ["m0"],
            "artifacts": { platform(): artifact },
            "args": ["127.0.0.1:{port}"],
            "port": port,
            "admin_token": "relay-admin-token",
            "drain_secs": 0,
            "health_secs": 8,
        } },
    });
    if let Some(key) = key_file {
        file["release_key_file"] = json!(key);
    }
    let path = dir.join("fleet.json");
    std::fs::write(&path, file.to_string()).unwrap();
    Fleet::load(&path).unwrap()
}

/// A copy of the server, with bytes of its own so it is a release of its own.
fn build(dir: &Path, n: u8) -> PathBuf {
    let mut bytes = std::fs::read(SERVER).unwrap();
    bytes.extend_from_slice(&[b'#', n]);
    let path = dir.join(format!("build-{n}")).join(Path::new(SERVER).file_name().unwrap());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

fn healthy(port: u16) -> bool {
    ureq::get(&format!("http://127.0.0.1:{port}/healthz")).timeout(Duration::from_secs(3)).call().is_ok()
}

/// Straight to the agent with its token, as somebody who stole it would.
fn raw(agent_url: &str, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> (u16, Value) {
    let mut request =
        ureq::request(method, &format!("{agent_url}{path}")).set("Authorization", &format!("Bearer {TOKEN}"));
    for (k, v) in headers {
        request = request.set(k, v);
    }
    if body.starts_with(b"{") {
        request = request.set("Content-Type", "application/json");
    }
    let response = match request.send_bytes(body) {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("{path}: {e}"),
    };
    let status = response.status();
    (status, response.into_json().unwrap_or(Value::Null))
}

#[test]
fn an_agent_with_a_release_key_runs_only_what_it_signed() {
    let dir = std::env::temp_dir().join(format!("jengine-signed-fleet-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let key = ReleaseSigner::generate();
    let key_file = dir.join("release.key");
    std::fs::write(&key_file, key.to_text()).unwrap();
    let host = HostOptions { release_key: Some(key.public()), ..Default::default() };
    let agent = agent::start_host("127.0.0.1:0", dir.join("m0"), TOKEN, None, host).unwrap();
    let port = free_port();
    let mut said = |line: &str| eprintln!("{line}");

    // Unsigned: refused at the upload, before a byte is stored.
    let first = build(&dir, 1);
    let unsigned = fleet_file(&dir, &agent.url(), &first, port, None);
    let why = client::deploy(&unsigned, "relay", None, &mut said).unwrap_err();
    assert!(why.contains("only signed releases"), "{why}");
    assert!(!healthy(port));

    // Signed where it was built, and deployed from anywhere with the .sig.
    client::sign_artifact(&key, &first).unwrap();
    client::deploy(&unsigned, "relay", None, &mut said).unwrap();
    assert!(healthy(port), "the signed release runs");

    // Signed by the fleet's own key at deploy time.
    let second = build(&dir, 2);
    let signing = fleet_file(&dir, &agent.url(), &second, port, Some(&key_file));
    client::deploy(&signing, "relay", None, &mut said).unwrap();
    assert!(healthy(port));

    // Somebody with the token and a program of their own.
    let theirs = b"#!/bin/sh\necho owned\n".to_vec();
    let version = version_of(&theirs);
    let path = format!("/v1/releases/relay/{version}/run.sh");
    assert_eq!(raw(&agent.url(), "PUT", &path, &[], &theirs).0, 403, "no signature");
    let forged = ReleaseSigner::generate().sign(&theirs);
    assert_eq!(raw(&agent.url(), "PUT", &path, &[(SIGNATURE_HEADER, &forged)], &theirs).0, 403, "another key's");
    let stolen = key.sign(&std::fs::read(&second).unwrap());
    assert_eq!(raw(&agent.url(), "PUT", &path, &[(SIGNATURE_HEADER, &stolen)], &theirs).0, 403, "a real signature of other bytes");
    let deploy = json!({ "version": version, "file": "run.sh", "spec": { "port": port } });
    assert_eq!(raw(&agent.url(), "POST", "/v1/services/relay/deploy", &[], deploy.to_string().as_bytes()).0, 404);

    // Nor past a signed release through a deploy's settings.
    let signed = version_of(&std::fs::read(&second).unwrap());
    let file = second.file_name().unwrap().to_str().unwrap();
    let with = |spec: Value| json!({ "version": signed, "file": file, "spec": spec }).to_string();
    let (status, body) = raw(&agent.url(), "POST", "/v1/services/relay/deploy", &[], with(json!({ "port": port, "env": { "LD_PRELOAD": "/tmp/x.so" } })).as_bytes());
    assert_eq!(status, 403, "{body}");
    let (status, body) = raw(&agent.url(), "POST", "/v1/services/relay/deploy", &[], with(json!({ "port": port, "exe": "../../../bin/sh" })).as_bytes());
    assert_eq!(status, 400, "{body}");
    assert!(healthy(port), "and what ran still runs");

    // Stop, start and roll back still need only the token: the releases are signed.
    client::act(&signing, "relay", "rollback", None, &mut said).unwrap();
    assert!(healthy(port));
    agent.stop();

    // An agent anyone on the network could reach, with nothing but a token
    // between them and running programs: not without saying so.
    let why = agent::start_host("0.0.0.0:0", dir.join("m1"), TOKEN, None, HostOptions::default()).err().unwrap();
    assert!(why.to_string().contains("--release-key"), "{why}");
    let _ = std::fs::remove_dir_all(&dir);
}
