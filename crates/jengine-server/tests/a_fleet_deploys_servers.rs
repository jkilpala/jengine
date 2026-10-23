//! **The deploy platform, with real processes**: agents listening on real
//! ports, the real `jengine-server` executable uploaded to them, started,
//! replaced, killed and put back - through the same functions
//! `jengine-fleet`'s command line calls.
//!
//! Two agents on one machine stand in for two machines; they differ only in
//! the port their service is given, which is what `overrides` is for.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::Client;
use jengine_server::fleet::agent::{self, RunningAgent};
use jengine_server::fleet::client::{self, Fleet};
use jengine_server::fleet::{platform, version_of};
use serde_json::{Value, json};

const SERVER: &str = env!("CARGO_BIN_EXE_jengine-server");
const TOKEN: &str = "fleet-test-token";

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A scratch directory holding the agents' data and the fleet file.
struct Bench {
    dir: PathBuf,
    agents: Vec<RunningAgent>,
    ports: Vec<u16>,
}

impl Bench {
    fn new(name: &str, machines: usize) -> Self {
        let dir = std::env::temp_dir().join(format!("jengine-fleet-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let agents = (0..machines).map(|i| agent::start("127.0.0.1:0", dir.join(format!("m{i}")), TOKEN).unwrap()).collect();
        Bench { dir, agents, ports: (0..machines).map(|_| free_port()).collect() }
    }

    /// Write a fleet file running `artifact` as the service `relay` on every
    /// machine, with `args`, and load it.
    fn fleet(&self, artifact: &Path, args: Value) -> Fleet {
        self.fleet_with(artifact, args, Some("server-admin-token"))
    }

    fn fleet_with(&self, artifact: &Path, args: Value, admin_token: Option<&str>) -> Fleet {
        let machines: serde_json::Map<String, Value> = self
            .agents
            .iter()
            .enumerate()
            .map(|(i, a)| (format!("m{i}"), json!({ "url": a.url(), "token": TOKEN })))
            .collect();
        let overrides: serde_json::Map<String, Value> =
            self.ports.iter().enumerate().map(|(i, p)| (format!("m{i}"), json!({ "port": p }))).collect();
        let names: Vec<String> = machines.keys().cloned().collect();
        let file = json!({
            "machines": machines,
            "services": { "relay": {
                "machines": names,
                "artifacts": { platform(): artifact },
                "args": args,
                "admin_token": admin_token,
                "drain_secs": 1,
                "health_secs": 8,
                "overrides": overrides,
            } },
        });
        let path = self.dir.join("fleet.json");
        std::fs::write(&path, file.to_string()).unwrap();
        Fleet::load(&path).unwrap()
    }

    fn good(&self, artifact: &Path) -> Fleet {
        self.fleet(artifact, json!(["127.0.0.1:{port}", "--db", "relay.db"]))
    }

    /// The same server with different bytes: a new build, as far as a
    /// release's name is concerned.
    fn next_build(&self) -> PathBuf {
        let mut bytes = std::fs::read(SERVER).unwrap();
        bytes.extend_from_slice(b"a newer build");
        let path = self.dir.join(Path::new(SERVER).file_name().unwrap());
        std::fs::write(&path, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    fn service(&self, fleet: &Fleet, machine: usize) -> Value {
        let (_, status) = client::status(fleet).into_iter().nth(machine).unwrap();
        status.unwrap()["services"][0].clone()
    }

    fn url(&self, machine: usize) -> String {
        format!("ws://127.0.0.1:{}", self.ports[machine])
    }

    fn healthy(&self, machine: usize) -> bool {
        let mut http = jengine_net::http_client();
        http.send(jengine_net::HttpRequest::get("h", format!("http://127.0.0.1:{}/healthz", self.ports[machine])));
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            http.pump();
            if let Some(r) = http.drain().pop() {
                return r.status == 200;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    fn finish(self) {
        for agent in self.agents {
            agent.stop();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn deploy(fleet: &Fleet) -> Result<Vec<String>, String> {
    let mut said = Vec::new();
    client::deploy(fleet, "relay", None, &mut |line| said.push(line.to_string()))?;
    Ok(said)
}

fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_build_goes_to_two_machines_and_the_next_one_keeps_the_players() {
    let bench = Bench::new("rolling", 2);
    let fleet = bench.good(Path::new(SERVER));
    let v1 = version_of(&std::fs::read(SERVER).unwrap());

    let said = deploy(&fleet).unwrap();
    assert!(said.iter().any(|l| l.starts_with("m0: uploading")) && said.iter().any(|l| l.starts_with("m1: uploading")));
    for machine in 0..2 {
        let service = bench.service(&fleet, machine);
        assert_eq!((service["status"].as_str(), service["version"].as_str()), (Some("running"), Some(v1.as_str())));
        assert!(bench.healthy(machine));
    }

    // Somebody plays on the first machine.
    let mut ada = Client::connect(&bench.url(0));
    let token = ada.hello("Ada", "")["token"].as_str().unwrap().to_string();
    assert_eq!(bench.service(&fleet, 0)["stats"]["players"], 1, "status shows what the server says of itself");

    // Deploying the same build again uploads nothing.
    assert!(deploy(&fleet).unwrap().iter().any(|l| l == &format!("m0: already has {v1}")));

    // A new build. Ada is still connected, so the drain runs out and the
    // server is stopped anyway - having saved her.
    let fleet = bench.good(&bench.next_build());
    deploy(&fleet).unwrap();
    let service = bench.service(&fleet, 0);
    assert_ne!(service["version"], v1.as_str());
    assert_eq!(service["previous"], v1.as_str());
    let mut again = Client::connect(&bench.url(0));
    assert_eq!(again.hello("", &token)["name"], "Ada", "the database is the service's, not the release's");

    // And back.
    client::act(&fleet, "relay", "rollback", None, &mut |_| {}).unwrap();
    assert_eq!(bench.service(&fleet, 0)["version"], v1.as_str());
    assert_eq!(bench.service(&fleet, 1)["version"], v1.as_str());
    assert!(bench.healthy(0) && bench.healthy(1));
    bench.finish();
}

#[test]
fn a_build_that_does_not_come_up_is_put_back_and_the_rollout_stops() {
    let bench = Bench::new("putback", 2);
    let fleet = bench.good(Path::new(SERVER));
    deploy(&fleet).unwrap();
    let v1 = bench.service(&fleet, 0)["version"].clone();

    // The next build cannot listen: it exits as soon as it starts.
    let broken = bench.fleet(&bench.next_build(), json!(["not-an-address"]));
    let why = deploy(&broken).unwrap_err();
    assert!(why.starts_with("m0: "), "{why}");
    assert!(why.contains("exited at once") && why.contains("put back"), "{why}");
    assert!(why.contains("1 machine(s) after it were not touched"), "{why}");

    for machine in 0..2 {
        let service = bench.service(&fleet, machine);
        assert_eq!((&service["status"], &service["version"]), (&json!("running"), &v1), "machine {machine}");
        assert!(bench.healthy(machine), "machine {machine} is serving again");
    }
    assert!(bench.service(&fleet, 0)["last_error"].as_str().unwrap().contains("failed"));
    let log = client::logs(&fleet, "relay", "m0", 50).unwrap();
    assert!(log.contains("could not listen on not-an-address"), "the reason is in the log: {log}");
    bench.finish();
}

#[test]
fn a_server_that_dies_is_started_again() {
    let bench = Bench::new("crash", 1);
    let fleet = bench.good(Path::new(SERVER));
    deploy(&fleet).unwrap();
    let pid = bench.service(&fleet, 0)["pid"].as_u64().unwrap();

    let killed = if cfg!(windows) {
        std::process::Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).output()
    } else {
        std::process::Command::new("kill").args(["-9", &pid.to_string()]).output()
    };
    assert!(killed.unwrap().status.success());

    eventually("the agent to start it again", || {
        let service = bench.service(&fleet, 0);
        service["status"] == "running" && service["restarts"] == 1 && service["pid"].as_u64() != Some(pid) && bench.healthy(0)
    });
    bench.finish();
}

#[test]
fn stopping_stays_stopped_and_an_agent_that_restarts_brings_back_what_ran() {
    let mut bench = Bench::new("restart", 1);
    let fleet = bench.good(Path::new(SERVER));
    deploy(&fleet).unwrap();

    client::act(&fleet, "relay", "stop", None, &mut |_| {}).unwrap();
    assert_eq!(bench.service(&fleet, 0)["status"], "stopped");
    assert!(!bench.healthy(0));
    client::act(&fleet, "relay", "start", None, &mut |_| {}).unwrap();
    assert!(bench.healthy(0));

    // The machine reboots: the agent goes, and takes its server with it...
    bench.agents.remove(0).stop();
    assert!(!bench.healthy(0), "an agent that stops, stops its servers");
    // ...and comes back on the same data directory.
    bench.agents.push(agent::start("127.0.0.1:0", bench.dir.join("m0"), TOKEN).unwrap());
    let fleet = bench.good(Path::new(SERVER));
    eventually("the service to be running again", || {
        bench.service(&fleet, 0)["status"] == "running" && bench.healthy(0)
    });
    bench.finish();
}

/// The fleet file is edited from the page: machines and services put and
/// removed, each edit checked before it is written.
#[test]
fn the_fleet_file_is_edited_from_the_page() {
    use jengine_server::fleet::ui;
    let bench = Bench::new("edit", 1);
    let _ = bench.good(Path::new(SERVER));
    let web = ui::start("127.0.0.1:0", bench.dir.join("fleet.json"), None).unwrap();
    let call = |method: &str, path: &str, body: Value| -> Result<Value, (u16, String)> {
        let request = ureq::request(method, &format!("{}{path}", web.url()))
            .set("Authorization", &format!("Bearer {}", web.token()))
            .set("Content-Type", "application/json");
        match request.send_string(&body.to_string()) {
            Ok(r) => Ok(r.into_json().unwrap()),
            Err(ureq::Error::Status(code, r)) => Err((code, r.into_string().unwrap_or_default())),
            Err(e) => panic!("{e}"),
        }
    };
    call("PUT", "/api/fleet/machine", json!({ "name": "eu-1", "url": "https://10.0.0.5:9900", "token_env": "FLEET_TOKEN", "ca": "eu-1.pem" })).unwrap();
    let file: Value = serde_json::from_str(&std::fs::read_to_string(bench.dir.join("fleet.json")).unwrap()).unwrap();
    assert_eq!(file["machines"]["eu-1"], json!({ "url": "https://10.0.0.5:9900", "token_env": "FLEET_TOKEN", "ca": "eu-1.pem" }));
    assert!(file["machines"]["m0"].is_object(), "what was there is kept");
    let refused = call("PUT", "/api/fleet/machine", json!({ "name": "bad", "url": "10.0.0.5" })).unwrap_err();
    assert_eq!(refused.0, 400, "{refused:?}");

    call("PUT", "/api/fleet/service", json!({ "name": "world", "machines": ["eu-1"], "artifacts": { "linux-x86_64": "target/release/world" }, "port": 9002, "admin_token_env": "WORLD_ADMIN" })).unwrap();
    let file: Value = serde_json::from_str(&std::fs::read_to_string(bench.dir.join("fleet.json")).unwrap()).unwrap();
    assert_eq!(file["services"]["world"]["port"], 9002);
    assert_eq!(file["services"]["world"]["admin_token_env"], "WORLD_ADMIN");
    let refused = call("PUT", "/api/fleet/service", json!({ "name": "lost", "machines": ["nowhere"], "artifacts": {} })).unwrap_err();
    assert!(refused.1.contains("nowhere"), "{refused:?}");
    let refused = call("DELETE", "/api/fleet/machine", json!({ "name": "eu-1" })).unwrap_err();
    assert!(refused.1.contains("world"), "a machine a service runs on stays: {refused:?}");
    call("DELETE", "/api/fleet/service", json!({ "name": "world" })).unwrap();
    call("DELETE", "/api/fleet/machine", json!({ "name": "eu-1" })).unwrap();
    let fleet = Fleet::load(&bench.dir.join("fleet.json")).unwrap();
    assert!(!fleet.machines.contains_key("eu-1") && !fleet.services.contains_key("world"));
    assert_eq!(fleet.services["relay"].machines, ["m0"], "and the relay is as it was");

    // Status comes pushed: the first event has every machine in it.
    let stream = ureq::get(&format!("{}/api/events?token={}", web.url(), web.token())).timeout(Duration::from_secs(15)).call().unwrap();
    assert_eq!(stream.header("content-type"), Some("text/event-stream"));
    let mut reader = std::io::BufReader::new(stream.into_reader());
    let mut line = String::new();
    std::io::BufRead::read_line(&mut reader, &mut line).unwrap();
    let event: Value = serde_json::from_str(line.trim_start_matches("data: ").trim()).unwrap();
    assert_eq!(event["machines"][0]["machine"], "m0", "{event}");
    assert!(event["jobs"].is_array());
    // Still listening while the page's server stops: the stream is ended
    // by the stop, not waited for.
    let stopped_at = Instant::now();
    web.stop();
    assert!(stopped_at.elapsed() < Duration::from_secs(10), "the open stream did not hold the stop");
    drop(reader);
    bench.finish();
}

/// A release can be an archive: the program with files beside it.
#[test]
fn a_release_can_be_an_archive_with_files_beside_the_program() {
    let bench = Bench::new("archive", 1);
    let exe_name = Path::new(SERVER).file_name().unwrap().to_str().unwrap().to_string();
    let archive = bench.dir.join("server.zip");
    {
        use std::io::Write;
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let plain = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(format!("bin/{exe_name}"), plain).unwrap();
        zip.write_all(&std::fs::read(SERVER).unwrap()).unwrap();
        zip.start_file("data/hello.txt", plain).unwrap();
        zip.write_all(b"hello from the archive").unwrap();
        zip.finish().unwrap();
    }
    // Without naming the program, the deploy refuses before uploading.
    let fleet = bench.fleet_with(&archive, json!(["127.0.0.1:{port}"]), Some("server-admin-token"));
    let refused = deploy(&fleet).unwrap_err();
    assert!(refused.contains("archive"), "{refused}");

    let mut fleet = fleet;
    let mut def = fleet.services.remove("relay").unwrap();
    def.spec.exe = Some(format!("bin/{exe_name}"));
    fleet.services.insert("relay".into(), def);
    deploy(&fleet).unwrap();
    assert!(bench.healthy(0), "the program inside the archive runs");
    let version = version_of(&std::fs::read(&archive).unwrap());
    let unpacked = bench.dir.join("m0").join("releases").join("relay").join(&version).join("unpacked");
    assert_eq!(std::fs::read_to_string(unpacked.join("data").join("hello.txt")).unwrap(), "hello from the archive");
    assert!(unpacked.join("bin").join(&exe_name).is_file());
    bench.finish();
}

/// A log is searched across its generations, newest last.
#[test]
fn logs_are_kept_in_generations_and_searched() {
    let bench = Bench::new("logs", 1);
    let fleet = bench.good(Path::new(SERVER));
    deploy(&fleet).unwrap();
    let dir = bench.dir.join("m0").join("services").join("relay");
    // Older generations, as the agent would have set them aside.
    std::fs::write(dir.join("service.log.2"), "oldest needle\nnoise\n").unwrap();
    std::fs::write(dir.join("service.log.1"), "older needle\nmore noise\n").unwrap();
    let found = client::logs_matching(&fleet, "relay", "m0", 100, "needle").unwrap();
    assert_eq!(found, "oldest needle\nolder needle", "oldest first, nothing else: {found}");
    let last = client::logs_matching(&fleet, "relay", "m0", 1, "needle").unwrap();
    assert_eq!(last, "older needle", "the last line is the newest match");
    let live = client::logs_matching(&fleet, "relay", "m0", 100, "listening").unwrap();
    assert!(live.contains("listening on"), "the live log is searched too: {live}");
    assert!(client::logs(&fleet, "relay", "m0", 100).unwrap().contains("listening on"), "and a plain tail still works");
    bench.finish();
}

/// A service given no admin token is still stopped gently - on Unix by
/// SIGTERM, on Windows by the stop file - so it saves its players; only a
/// program that is not a `jengine-server` is killed.
#[test]
fn a_service_without_a_token_is_still_stopped_gently() {
    let bench = Bench::new("gentle", 1);
    let fleet = bench.fleet_with(Path::new(SERVER), json!(["127.0.0.1:{port}"]), None);
    deploy(&fleet).unwrap();
    assert!(bench.healthy(0));
    let stopped_at = Instant::now();
    client::act(&fleet, "relay", "stop", None, &mut |_| {}).unwrap();
    assert_eq!(bench.service(&fleet, 0)["status"], "stopped");
    assert!(stopped_at.elapsed() < Duration::from_secs(10), "it stopped when asked, not at the kill timeout");
    let log = std::fs::read_to_string(bench.dir.join("m0").join("services").join("relay").join("service.log")).unwrap();
    assert!(log.contains("stopping: saving players"), "the server stopped itself:\n{log}");
    assert!(!bench.dir.join("m0").join("services").join("relay").join("stop").exists(), "the stop file is cleared");
    bench.finish();
}

/// The web interface, called the way its page calls it.
#[test]
fn the_web_interface_deploys_and_shows_what_the_command_line_would() {
    use jengine_server::fleet::ui;

    let bench = Bench::new("ui", 2);
    let fleet = bench.good(Path::new(SERVER));
    let v1 = version_of(&std::fs::read(SERVER).unwrap());
    let web = ui::start("127.0.0.1:0", bench.dir.join("fleet.json"), None).unwrap();
    let call = |method: &str, path: &str, body: Option<Value>| -> Result<Value, (u16, String)> {
        let request = ureq::request(method, &format!("{}{path}", web.url()))
            .set("Authorization", &format!("Bearer {}", web.token()));
        let result = match body {
            Some(body) => request.set("Content-Type", "application/json").send_string(&body.to_string()),
            None => request.call(),
        };
        match result {
            Ok(r) => {
                let text = r.into_string().unwrap();
                Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
            }
            Err(ureq::Error::Status(code, r)) => Err((code, r.into_string().unwrap_or_default())),
            Err(e) => panic!("{path}: {e}"),
        }
    };
    let job_done = |id: &Value| -> Value {
        let mut found = Value::Null;
        eventually("the job to finish", || {
            let jobs = call("GET", "/api/jobs", None).unwrap();
            found = jobs["jobs"].as_array().unwrap().iter().find(|j| j["id"] == *id).cloned().unwrap();
            found["state"] != "running"
        });
        found
    };

    // The page carries its token on loopback; without it, nothing answers.
    let page = ureq::get(&web.url()).call().unwrap().into_string().unwrap();
    assert!(page.contains(web.token()) && page.contains("<title>jengine-fleet</title>"));
    assert!(matches!(ureq::get(&format!("{}/api/status", web.url())).call(), Err(ureq::Error::Status(401, _))));
    assert!(ui::start("0.0.0.0:0", bench.dir.join("fleet.json"), None).is_err(), "beyond loopback needs a token");

    // What it shows of the fleet file: the build that would go out, and no secrets.
    let described = call("GET", "/api/fleet", None).unwrap();
    assert_eq!(described["services"][0]["artifacts"][0]["version"], v1.as_str());
    assert_eq!(described["services"][0]["machines"][1]["port"], bench.ports[1]);
    let text = described.to_string();
    assert!(!text.contains(TOKEN) && !text.contains("server-admin-token"), "a token reached the browser: {text}");

    // Deploy, from the button.
    let job = call("POST", "/api/deploy", Some(json!({ "service": "relay" }))).unwrap()["job"].clone();
    let done = job_done(&job);
    assert_eq!(done["state"], "ok", "{done}");
    let lines: Vec<&str> = done["lines"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(lines.contains(&format!("m1: running {v1}").as_str()), "the command line's own words: {lines:?}");

    let status = call("GET", "/api/status", None).unwrap();
    for machine in 0..2 {
        let service = &status["machines"][machine]["status"]["services"][0];
        assert_eq!((&service["status"], &service["version"]), (&json!("running"), &json!(v1)));
    }

    // One machine stopped, through the page; the other is untouched.
    let job = call("POST", "/api/act", Some(json!({ "service": "relay", "machine": "m0", "action": "stop" }))).unwrap();
    assert_eq!(job_done(&job["job"])["state"], "ok");
    assert_eq!(bench.service(&fleet, 0)["status"], "stopped");
    assert!(bench.healthy(1));

    // A build that cannot come up: the job fails, and says what the agent said.
    bench.fleet(&bench.next_build(), json!(["not-an-address"]));
    let job = call("POST", "/api/deploy", Some(json!({ "service": "relay", "machine": "m1" }))).unwrap();
    let failed = job_done(&job["job"]);
    assert_eq!(failed["state"], "failed");
    assert!(failed["error"].as_str().unwrap().contains("put back"), "{failed}");
    assert!(bench.healthy(1), "and the machine is serving the old build");

    let log = call("GET", "/api/logs?service=relay&machine=m1&lines=50", None).unwrap();
    assert!(log.as_str().unwrap().contains("could not listen on not-an-address"));
    assert_eq!(call("POST", "/api/act", Some(json!({ "service": "relay", "action": "explode" }))).unwrap_err().0, 400);

    web.stop();
    bench.finish();
}

#[test]
fn an_agent_is_not_talked_into_things() {
    let bench = Bench::new("refusals", 1);
    let fleet = bench.good(Path::new(SERVER));
    let url = bench.agents[0].url();

    // No token, no answer - and no token, no agent.
    let unauthorized = ureq::get(&format!("{url}/v1/status")).set("Authorization", "Bearer wrong-token-here").call();
    assert!(matches!(unauthorized, Err(ureq::Error::Status(401, _))));
    assert!(agent::start("127.0.0.1:0", bench.dir.join("x"), "short").is_err());

    // Bytes that are not the release they claim to be.
    let lie = ureq::put(&format!("{url}/v1/releases/relay/0123456789abcdef/server"))
        .set("Authorization", &format!("Bearer {TOKEN}"))
        .send_bytes(b"something else");
    assert!(matches!(lie, Err(ureq::Error::Status(400, _))));

    // A name that would climb out of the data directory.
    let climb = ureq::put(&format!("{url}/v1/releases/relay/{}/..%2F..%2Fevil", version_of(b"x")))
        .set("Authorization", &format!("Bearer {TOKEN}"))
        .send_bytes(b"x");
    assert!(matches!(climb, Err(ureq::Error::Status(400 | 404, _))), "{climb:?}");

    // A program for another kind of machine never leaves this one.
    let foreign = bench.dir.join("server-for-elsewhere");
    std::fs::write(&foreign, if cfg!(windows) { &b"\x7fELF\x02\x01"[..] } else { &b"MZ\x90\x00"[..] }).unwrap();
    let why = deploy(&bench.fleet(&foreign, json!([]))).unwrap_err();
    assert!(why.contains("program and the machine is"), "{why}");

    assert_eq!(client::status(&fleet)[0].1.as_ref().unwrap()["services"], json!([]), "and nothing was deployed");
    bench.finish();
}

/// The agent's token runs programs on its machine, so it should not cross
/// the network where it can be read: an agent with a certificate speaks
/// HTTPS, and the fleet trusts that certificate and no other.
#[test]
fn an_agent_with_a_certificate_is_reached_over_https_and_nothing_else() {
    use jengine_server::{Tls, listen};
    let dir = std::env::temp_dir().join(format!("jengine-fleet-tls-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (cert, key) = listen::self_signed(&["127.0.0.1".to_string()]).unwrap();
    std::fs::write(dir.join("cert.pem"), cert).unwrap();
    std::fs::write(dir.join("key.pem"), key).unwrap();
    // Another certificate for the same address: what an impostor would have.
    std::fs::write(dir.join("other.pem"), listen::self_signed(&["127.0.0.1".to_string()]).unwrap().0).unwrap();

    let tls = Tls::new(dir.join("cert.pem"), dir.join("key.pem"));
    let agent = agent::start_with("127.0.0.1:0", dir.join("data"), TOKEN, Some(tls)).unwrap();
    assert!(agent.url().starts_with("https://"));
    let port = free_port();
    let fleet_with = |ca: Option<&str>, url: String| {
        let file = json!({
            "machines": { "m0": { "url": url, "token": TOKEN, "ca": ca } },
            "services": { "relay": {
                "machines": ["m0"], "artifacts": { platform(): SERVER },
                "args": ["127.0.0.1:{port}"], "port": port, "admin_token": "server-admin-token",
                "drain_secs": 0, "health_secs": 8,
            } },
        });
        std::fs::write(dir.join("fleet.json"), file.to_string()).unwrap();
        Fleet::load(&dir.join("fleet.json")).unwrap()
    };

    // Trusting its certificate: a deploy works, end to end, over HTTPS.
    let fleet = fleet_with(Some("cert.pem"), agent.url());
    client::deploy(&fleet, "relay", None, &mut |_| {}).unwrap();
    assert_eq!(client::status(&fleet)[0].1.as_ref().unwrap()["services"][0]["status"], "running");

    // Not trusting it - or trusting a different one for the same address -
    // the fleet will not speak to it at all, token unsent.
    for ca in [None, Some("other.pem")] {
        let untrusting = fleet_with(ca, agent.url());
        let why = client::status(&untrusting)[0].1.clone().unwrap_err();
        assert!(why.contains("could not reach the agent"), "{why}");
    }
    // And it does not answer plain HTTP.
    let plain = fleet_with(None, agent.url().replace("https://", "http://"));
    assert!(client::status(&plain)[0].1.is_err());

    client::act(&fleet_with(Some("cert.pem"), agent.url()), "relay", "remove", None, &mut |_| {}).unwrap();
    agent.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
