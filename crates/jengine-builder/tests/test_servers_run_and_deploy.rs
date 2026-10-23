//! A project's test server, the whole way: built, run on this machine and
//! asked how it is, stopped cleanly - then deployed through a fleet agent
//! on this machine and asked again where it was deployed. The server is
//! the engine's stock `jengine-server` binary, built from the workspace the
//! way a game's own server crate is; nothing here is a stand-in.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jengine_builder::secret_store::MemoryStore;
use jengine_builder::servers::{self, LocalServer, Secrets, ServerConfig};
use serde_json::json;

fn engine() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A scratch project whose server is the stock `jengine-server`, on free ports.
fn project() -> (PathBuf, ServerConfig) {
    let dir = std::env::temp_dir().join(format!("jengine-test-servers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let doc = json!({
        "name": "t",
        "server": {
            "package": "jengine-server",
            "address": format!("127.0.0.1:{}", free_port()),
            "args": ["{address}", "--db", "u.db"],
            "fleet": "fleet.json",
            "service": "game",
            "local_agent_port": free_port(),
        }
    });
    std::fs::write(dir.join("project.jengine"), doc.to_string()).unwrap();
    let cfg = ServerConfig::load(&dir).unwrap().unwrap();
    (dir, cfg)
}

fn until(what: &str, secs: u64, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn a_test_server_runs_here_and_deploys_through_a_local_agent() {
    let engine = engine();
    let (dir, cfg) = project();
    // Tests never touch this machine's keychain.
    let (secrets, _) = Secrets::for_project_with(&dir, &MemoryStore::new()).unwrap();
    let exe = servers::build(&engine, &cfg).expect("the server builds");

    // Run here: its log reaches the sink, it answers, its rooms are seen.
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = lines.clone();
    let server = LocalServer::spawn(&exe, &cfg, &secrets.admin_token, Arc::new(move |l: &str| sink.lock().unwrap().push(l.to_string())))
        .expect("the server starts");
    until("the local server's health check", 20, || servers::probe(&cfg.address, None).up);
    let probe = servers::probe(&cfg.address, Some(&secrets.admin_token));
    assert!(probe.ms.is_some());
    let overview = probe.overview.expect("the admin token is the server's");
    assert_eq!(overview["kinds"], json!(["relay"]));
    servers::admin_post(&cfg.address, &secrets.admin_token, "/admin/api/notice", &json!({ "text": "hello" }))
        .expect("a notice is accepted");
    until("its log", 5, || lines.lock().unwrap().iter().any(|l| l.contains("listening")));
    assert_eq!(server.stop(Duration::from_secs(10)), "shut down cleanly");
    assert!(!servers::probe(&cfg.address, None).up, "stopped means stopped");

    // Deploy: a fleet agent here, the default fleet file, the same build.
    let agent = servers::start_local_agent(&cfg, &secrets).expect("the agent starts");
    let fleet_path = cfg.fleet.clone().unwrap();
    std::fs::write(&fleet_path, servers::default_fleet(&engine, &cfg, &fleet_path).to_string()).unwrap();
    // On a free port rather than the default fleet's 9101, which may be busy.
    let port = free_port();
    let mut doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&fleet_path).unwrap()).unwrap();
    doc["services"]["game"]["port"] = json!(port);
    std::fs::write(&fleet_path, doc.to_string()).unwrap();

    let mut said = Vec::new();
    servers::deploy(&engine, &cfg, &secrets, None, &mut |l| said.push(l.to_string())).expect("the deploy goes through");
    assert!(said.iter().any(|l| l.contains("uploading")), "{said:?}");

    let status = servers::fleet_status(&cfg, &secrets).unwrap();
    let machine = &status["machines"][0];
    assert_eq!(machine["name"], "local", "{status:#}");
    let svc = &machine["info"]["services"][0];
    assert_eq!(svc["name"], "game", "{status:#}");
    assert_eq!(svc["probe"]["up"], true, "{status:#}");
    // The deployed server was handed the project's admin token: its rooms show.
    assert_eq!(svc["probe"]["overview"]["kinds"], json!(["relay"]), "{status:#}");

    let logs = servers::logs(&cfg, &secrets, "local", 50).unwrap();
    assert!(logs.contains("listening"), "{logs}");
    servers::act(&cfg, &secrets, "stop", None, &mut |_| {}).unwrap();
    agent.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
