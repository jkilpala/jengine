//! The agent: one per machine. It keeps releases on disk, runs each service
//! as a child process, restarts one that dies, and carries out deploys.
//!
//! # One task per service
//!
//! Each service has a supervisor task that owns its child process and takes
//! commands one at a time - so a deploy cannot overlap a restart, and "what
//! is this service doing" has one answer. It publishes a snapshot for
//! `/v1/status` and writes what should be running to `state.json`, which is
//! what a restarted agent (or a rebooted machine) starts again from.
//!
//! # Stopping a server
//!
//! With the service's admin token the agent asks the server itself: drain,
//! wait for the players to go (up to `drain_secs`), then `shutdown`, which
//! saves everyone. Without one it signals - SIGTERM on Unix, which
//! `jengine-server` treats the same way; on Windows a **stop file** the
//! server is told of in `JENGINE_STOP_FILE`, which a server built on
//! `jengine-server` watches for; anything else is killed, so give
//! a service its admin token.
//!
//! # On disk
//!
//! ```text
//! <data>/state.json
//! <data>/releases/<service>/<version>/<file>
//! <data>/releases/<service>/<version>/<file>.sig  its signature, if it came with one
//! <data>/services/<service>/            the server's working directory
//! <data>/services/<service>/service.log its output
//! ```

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, watch};

use super::{ReleaseKey, ServiceSpec, local, platform, safe_name, version_of};

/// The header an upload's signature comes in.
pub const SIGNATURE_HEADER: &str = "x-jengine-release-signature";

/// Largest release an agent will take.
const MAX_RELEASE_BYTES: usize = 1 << 30;
/// A log this big is set aside (as `service.log.1`, the one before it
/// becoming `.2`, up to [`LOG_GENERATIONS`]) when its service starts.
const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
/// Old logs kept beside the current one.
const LOG_GENERATIONS: usize = 5;
/// How long a stopping server gets before it is killed.
const STOP_SECS: u64 = 15;

/// What a service runs: which file of which release, and how.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Release {
    file: String,
    version: String,
    spec: ServiceSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedService {
    current: Release,
    /// What `rollback` goes back to.
    previous: Option<Release>,
    /// Whether it should be running - so a stopped service stays stopped
    /// across an agent restart.
    running: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    services: BTreeMap<String, SavedService>,
}

#[derive(Debug, Clone, Default)]
struct Live {
    status: String,
    pid: Option<u32>,
    since: Option<Instant>,
    restarts: u32,
    last_error: Option<String>,
}

type Reply = oneshot::Sender<Result<String, String>>;

enum Cmd {
    Deploy(Release, Reply),
    Start(Reply),
    Stop(Reply),
    Restart(Reply),
    Rollback(Reply),
    Remove(Reply),
}

struct Inner {
    data: PathBuf,
    token: String,
    /// Runs only releases signed by this key's secret half, if set.
    release_key: Option<ReleaseKey>,
    saved: Mutex<Saved>,
    live: Mutex<HashMap<String, Live>>,
    services: Mutex<HashMap<String, mpsc::Sender<Cmd>>>,
    /// Each supervisor holds a clone; stopping drops this and waits.
    done: Mutex<Option<mpsc::Sender<()>>>,
    /// The machine as last measured ([`super::watch::watch_machine`]).
    machine: Arc<Mutex<Value>>,
    /// Each service's log, counted for errors and warnings as it grows.
    logs: Mutex<HashMap<String, super::watch::LogWatch>>,
}

impl Inner {
    /// What a service's log has said that was bad, read up to now.
    fn log_summary(&self, service: &str) -> Value {
        let path = self.service_dir(service).join("service.log");
        let mut logs = self.logs.lock().unwrap();
        let watch = logs.entry(service.to_string()).or_default();
        watch.look(&path);
        watch.json()
    }

    fn release_path(&self, service: &str, release: &Release) -> PathBuf {
        let dir = self.data.join("releases").join(service).join(&release.version);
        if super::is_archive(&release.file) {
            // Unpacked beside the archive; the spec says which file to run.
            return dir.join("unpacked").join(release.spec.exe.as_deref().unwrap_or("server"));
        }
        dir.join(&release.file)
    }

    fn service_dir(&self, service: &str) -> PathBuf {
        self.data.join("services").join(service)
    }

    /// The release's uploaded file itself - an archive, not what is in it.
    fn uploaded_path(&self, service: &str, release: &Release) -> PathBuf {
        self.data.join("releases").join(service).join(&release.version).join(&release.file)
    }

    /// Whether a release may run here: signed by the release key, when this
    /// agent has one. The bytes are read and hashed again - what was signed
    /// is what runs.
    fn signed(&self, path: &Path) -> Result<(), String> {
        let Some(key) = &self.release_key else { return Ok(()) };
        let bytes = std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        let sig_path = PathBuf::from(format!("{}.sig", path.display()));
        let sig = std::fs::read_to_string(&sig_path).unwrap_or_default();
        if key.signed(&bytes, &sig) {
            Ok(())
        } else {
            Err(format!("that release is not signed by this agent's release key ({})", key.to_text()))
        }
    }

    fn write_state(&self) {
        let text = serde_json::to_string_pretty(&*self.saved.lock().unwrap()).unwrap_or_default();
        // Written beside and renamed over, so a crash mid-write leaves the
        // old file rather than half of a new one.
        let tmp = self.data.join("state.json.tmp");
        if std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, self.data.join("state.json"))).is_err() {
            log::error!("[agent] could not write state.json");
        }
    }

    /// The supervisor for a service, started if there is none.
    fn supervisor(self: &Arc<Self>, name: &str) -> Result<mpsc::Sender<Cmd>, String> {
        let mut services = self.services.lock().unwrap();
        if let Some(tx) = services.get(name).filter(|tx| !tx.is_closed()) {
            return Ok(tx.clone());
        }
        let Some(done) = self.done.lock().unwrap().clone() else {
            return Err("the agent is stopping".into());
        };
        let (tx, rx) = mpsc::channel(16);
        services.insert(name.to_string(), tx.clone());
        tokio::spawn(supervise(self.clone(), name.to_string(), rx, done));
        Ok(tx)
    }
}

// ---- a service's supervisor -------------------------------------------------------

struct Supervisor {
    inner: Arc<Inner>,
    name: String,
    current: Option<Release>,
    previous: Option<Release>,
    child: Option<Child>,
    /// Should be running: a child that exits is started again.
    desired: bool,
    started: Option<Instant>,
    restarts: u32,
    backoff: Duration,
    restart_at: Option<tokio::time::Instant>,
    last_error: Option<String>,
}

async fn exited(child: &mut Option<Child>) -> std::io::Result<std::process::ExitStatus> {
    match child {
        Some(child) => child.wait().await,
        None => std::future::pending().await,
    }
}

impl Supervisor {
    fn publish(&self, status: &str) {
        self.inner.live.lock().unwrap().insert(
            self.name.clone(),
            Live {
                status: status.to_string(),
                pid: self.child.as_ref().and_then(Child::id),
                since: self.started.filter(|_| self.child.is_some()),
                restarts: self.restarts,
                last_error: self.last_error.clone(),
            },
        );
    }

    fn persist(&self) {
        {
            let mut saved = self.inner.saved.lock().unwrap();
            match &self.current {
                Some(current) => {
                    saved.services.insert(
                        self.name.clone(),
                        SavedService {
                            current: current.clone(),
                            previous: self.previous.clone(),
                            running: self.desired,
                        },
                    );
                }
                None => {
                    saved.services.remove(&self.name);
                }
            }
        }
        self.inner.write_state();
    }

    /// Start a release's executable. Does not wait for it to be healthy.
    fn spawn(&mut self, release: &Release) -> Result<(), String> {
        let exe = self.inner.release_path(&self.name, release);
        let dir = self.inner.service_dir(&self.name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
        let log = dir.join("service.log");
        if std::fs::metadata(&log).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
            for n in (1..LOG_GENERATIONS).rev() {
                let _ = std::fs::rename(dir.join(format!("service.log.{n}")), dir.join(format!("service.log.{}", n + 1)));
            }
            let _ = std::fs::rename(&log, dir.join("service.log.1"));
        }
        let open = || std::fs::OpenOptions::new().create(true).append(true).open(&log);
        let (out, err) = open().and_then(|o| Ok((o, open()?))).map_err(|e| format!("could not open the log: {e}"))?;

        let port = release.spec.port.map(|p| p.to_string()).unwrap_or_default();
        let mut command = Command::new(&exe);
        command
            .args(release.spec.args.iter().map(|a| a.replace("{port}", &port)))
            .envs(&release.spec.env)
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            // If the agent dies without stopping its services, they go with
            // it rather than holding their ports against the next agent.
            .kill_on_drop(true);
        if let Some(token) = &release.spec.admin_token {
            command.env("JENGINE_ADMIN_TOKEN", token);
        }
        // How it is stopped without a token on a platform with no signal:
        // a server built on `jengine-server` watches for this file.
        let stop = dir.join("stop");
        let _ = std::fs::remove_file(&stop);
        command.env("JENGINE_STOP_FILE", &stop);
        let child = command.spawn().map_err(|e| format!("could not start {}: {e}", exe.display()))?;
        // And if it dies without even dropping them - a crash, a kill.
        super::tied::tie_tokio(&child);
        self.child = Some(child);
        self.started = Some(Instant::now());
        Ok(())
    }

    /// Wait for a just-started server to be ready, or say why it is not.
    async fn healthy(&mut self, release: &Release) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs_f64(release.spec.health_secs.max(1.0));
        let began = Instant::now();
        loop {
            if let Some(child) = &mut self.child
                && let Ok(Some(status)) = child.try_wait()
            {
                self.child = None;
                return Err(format!("it exited at once ({status}); see its log"));
            }
            match release.spec.port {
                Some(port) => {
                    if let Some((200, _)) = local("GET", port, &release.spec.health_path, None).await {
                        return Ok(());
                    }
                }
                // Nothing to ask: staying up for two seconds has to do.
                None if began.elapsed() >= Duration::from_secs(2) => return Ok(()),
                None => {}
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "it did not answer {} within {:.0}s",
                    release.spec.health_path, release.spec.health_secs
                ));
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    async fn launch(&mut self, release: &Release) -> Result<(), String> {
        self.spawn(release)?;
        self.healthy(release).await
    }

    async fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
    }

    /// Stop the running server as gently as its spec allows.
    async fn stop_child(&mut self, spec: &ServiceSpec) {
        let Some(mut child) = self.child.take() else { return };
        // Whether the server has been asked to stop, and so is worth waiting for.
        let asked = match (&spec.admin_token, spec.port) {
            (Some(token), Some(port)) => {
                let token = Some(token.as_str());
                if local("POST", port, "/admin/api/drain", token).await.is_some() {
                    let deadline = Instant::now() + Duration::from_secs_f64(spec.drain_secs.max(0.0));
                    while Instant::now() < deadline {
                        let connections = local("GET", port, "/stats", token)
                            .await
                            .and_then(|(_, body)| serde_json::from_str::<Value>(&body).ok())
                            .and_then(|stats| stats["connections"].as_u64());
                        if connections.unwrap_or(0) == 0 {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                }
                local("POST", port, "/admin/api/shutdown", token).await.is_some()
            }
            _ => {
                #[cfg(unix)]
                {
                    let pid = child.id().map(|pid| pid.to_string()).unwrap_or_default();
                    Command::new("kill").args(["-TERM", &pid]).status().await.is_ok_and(|s| s.success())
                }
                #[cfg(not(unix))]
                {
                    // No signal to send: the stop file, which a server built
                    // on `jengine-server` watches for. Anything else is killed.
                    std::fs::write(self.inner.service_dir(&self.name).join("stop"), "stop").is_ok()
                }
            }
        };
        if !asked || tokio::time::timeout(Duration::from_secs(STOP_SECS), child.wait()).await.is_err() {
            let _ = child.kill().await;
        }
        let _ = std::fs::remove_file(self.inner.service_dir(&self.name).join("stop"));
    }

    /// Replace what the service runs - or put back what was there.
    async fn deploy(&mut self, new: Release) -> Result<String, String> {
        let old = self.current.clone();
        let was_running = self.desired && old.is_some();
        self.restart_at = None;
        if let Some(old) = &old {
            self.publish("stopping");
            self.stop_child(&old.spec).await;
        }
        self.publish("starting");
        match self.launch(&new).await {
            Ok(()) => {
                if old.as_ref().is_some_and(|old| *old != new) {
                    self.previous = old;
                }
                let version = new.version.clone();
                self.current = Some(new);
                self.desired = true;
                self.restarts = 0;
                self.backoff = Duration::from_secs(1);
                self.last_error = None;
                self.persist();
                self.publish("running");
                self.prune();
                Ok(format!("running {version}"))
            }
            Err(why) => {
                self.kill().await;
                self.last_error = Some(format!("deploy of {} failed: {why}", new.version));
                let Some(old) = old else {
                    self.publish("failed");
                    return Err(why);
                };
                if !was_running {
                    self.publish("stopped");
                    return Err(format!("{why}; {} is still what this service runs (stopped)", old.version));
                }
                let back = self.launch(&old).await;
                self.publish(if back.is_ok() { "running" } else { "crashed" });
                if back.is_err() {
                    self.kill().await;
                    self.restart_at = Some(tokio::time::Instant::now() + self.backoff);
                }
                Err(format!(
                    "{why}; put back {} ({})",
                    old.version,
                    if back.is_ok() { "running" } else { "which is not starting either" }
                ))
            }
        }
    }

    /// Delete this service's releases other than the two it can still use.
    fn prune(&self) {
        let keep: Vec<&str> =
            self.current.iter().chain(&self.previous).map(|r| r.version.as_str()).collect();
        let dir = self.inner.data.join("releases").join(&self.name);
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if !keep.contains(&entry.file_name().to_string_lossy().as_ref()) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    async fn handle(&mut self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::Deploy(release, reply) => {
                let _ = reply.send(self.deploy(release).await);
            }
            Cmd::Rollback(reply) => {
                let result = match self.previous.clone() {
                    Some(previous) => self.deploy(previous).await,
                    None => Err("there is nothing to roll back to".into()),
                };
                let _ = reply.send(result);
            }
            Cmd::Restart(reply) => {
                let result = match self.current.clone() {
                    Some(current) => self.deploy(current).await,
                    None => Err("nothing has been deployed".into()),
                };
                let _ = reply.send(result);
            }
            Cmd::Start(reply) => {
                let result = match (self.current.clone(), self.child.is_some()) {
                    (None, _) => Err("nothing has been deployed".into()),
                    (Some(_), true) => Ok("already running".to_string()),
                    (Some(current), false) => self.deploy(current).await,
                };
                let _ = reply.send(result);
            }
            Cmd::Stop(reply) => {
                self.halt().await;
                self.persist();
                let _ = reply.send(Ok("stopped".into()));
            }
            Cmd::Remove(reply) => {
                self.halt().await;
                self.current = None;
                self.previous = None;
                self.persist();
                self.prune();
                self.inner.live.lock().unwrap().remove(&self.name);
                let _ = reply.send(Ok("removed".into()));
                return false;
            }
        }
        true
    }

    async fn halt(&mut self) {
        self.desired = false;
        self.restart_at = None;
        if let Some(current) = self.current.clone() {
            self.publish("stopping");
            self.stop_child(&current.spec).await;
        }
        self.publish("stopped");
    }
}

async fn supervise(inner: Arc<Inner>, name: String, mut rx: mpsc::Receiver<Cmd>, _done: mpsc::Sender<()>) {
    let saved = inner.saved.lock().unwrap().services.get(&name).cloned();
    let mut sup = Supervisor {
        inner,
        name,
        current: saved.as_ref().map(|s| s.current.clone()),
        previous: saved.as_ref().and_then(|s| s.previous.clone()),
        child: None,
        desired: saved.as_ref().is_some_and(|s| s.running),
        started: None,
        restarts: 0,
        backoff: Duration::from_secs(1),
        restart_at: None,
        last_error: None,
    };
    // An agent coming back up starts what was running when it went down.
    if sup.desired {
        sup.restart_at = Some(tokio::time::Instant::now());
        sup.publish("starting");
    } else {
        sup.publish("stopped");
    }

    loop {
        let restart_at = sup.restart_at;
        let wake = async move {
            match restart_at {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(cmd) => {
                    if !sup.handle(cmd).await {
                        break;
                    }
                }
                // The agent is stopping: its servers stop with it, saved.
                None => {
                    if let Some(current) = sup.current.clone() {
                        sup.stop_child(&current.spec).await;
                    }
                    break;
                }
            },
            status = exited(&mut sup.child) => {
                sup.child = None;
                if sup.desired {
                    // A server that ran for a while and fell over is tried
                    // again at once; one that keeps dying is tried less often.
                    if sup.started.is_some_and(|s| s.elapsed() > Duration::from_secs(60)) {
                        sup.backoff = Duration::from_secs(1);
                    }
                    sup.restarts += 1;
                    let how = status.map(|s| s.to_string()).unwrap_or_else(|e| e.to_string());
                    sup.last_error = Some(format!("exited ({how})"));
                    log::warn!("[agent] {} exited ({how}); restarting in {:?}", sup.name, sup.backoff);
                    sup.restart_at = Some(tokio::time::Instant::now() + sup.backoff);
                    sup.backoff = (sup.backoff * 2).min(Duration::from_secs(30));
                    sup.publish("crashed");
                } else {
                    sup.publish("stopped");
                }
            }
            _ = wake => {
                sup.restart_at = None;
                if let Some(current) = sup.current.clone() {
                    match sup.spawn(&current) {
                        Ok(()) => sup.publish("running"),
                        Err(why) => {
                            sup.last_error = Some(why);
                            sup.restart_at = Some(tokio::time::Instant::now() + sup.backoff);
                            sup.backoff = (sup.backoff * 2).min(Duration::from_secs(30));
                            sup.publish("crashed");
                        }
                    }
                }
            }
        }
    }
}

// ---- the agent's HTTP API ---------------------------------------------------------

fn refuse(status: StatusCode, why: &str) -> Response {
    (status, Json(json!({ "error": why }))).into_response()
}

fn authorized(inner: &Inner, headers: &HeaderMap) -> bool {
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    inner.token.len() == given.len()
        && inner.token.bytes().zip(given.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

macro_rules! guard {
    ($inner:expr, $headers:expr) => {
        if !authorized(&$inner, &$headers) {
            return refuse(StatusCode::UNAUTHORIZED, "the agent token is missing or wrong");
        }
    };
}

async fn info(State(inner): State<Arc<Inner>>, headers: HeaderMap) -> Response {
    guard!(inner, headers);
    Json(json!({ "platform": platform(), "version": env!("CARGO_PKG_VERSION") })).into_response()
}

async fn status(State(inner): State<Arc<Inner>>, headers: HeaderMap) -> Response {
    guard!(inner, headers);
    let rows = service_rows(&inner).await;
    let machine = inner.machine.lock().unwrap().clone();
    Json(json!({ "platform": platform(), "version": env!("CARGO_PKG_VERSION"), "services": rows, "machine": machine }))
        .into_response()
}

/// Every service, what it runs and how it is doing - asked of the server
/// itself where it is up.
async fn service_rows(inner: &Inner) -> Vec<Value> {
    let services: Vec<(String, SavedService)> =
        inner.saved.lock().unwrap().services.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let mut rows = Vec::new();
    for (name, saved) in services {
        let live = inner.live.lock().unwrap().get(&name).cloned().unwrap_or_default();
        // What the server says about itself, if it is there to ask.
        let stats = match (saved.current.spec.port, live.pid) {
            (Some(port), Some(_)) => local("GET", port, "/stats", saved.current.spec.admin_token.as_deref())
                .await
                .and_then(|(_, body)| serde_json::from_str::<Value>(&body).ok()),
            _ => None,
        };
        rows.push(json!({
            "name": name,
            "version": saved.current.version,
            "previous": saved.previous.as_ref().map(|p| &p.version),
            "port": saved.current.spec.port,
            "status": live.status,
            "pid": live.pid,
            "uptime": live.since.map(|s| s.elapsed().as_secs_f64()),
            "restarts": live.restarts,
            "last_error": live.last_error,
            "stats": stats,
            "log": inner.log_summary(&name),
        }));
    }
    rows
}

/// The agent's services are what its gateway routes to. Only the name, the
/// state and the player count reach the directory: it is open to anyone on
/// the network, and the rest is the operator's.
impl super::gateway::Routes for Inner {
    fn port_of(&self, service: &str) -> Option<u16> {
        self.saved.lock().unwrap().services.get(service).and_then(|s| s.current.spec.port)
    }

    async fn listing(&self) -> Vec<Value> {
        service_rows(self)
            .await
            .into_iter()
            .filter(|row| row["port"].is_u64())
            .map(|row| json!({ "name": row["name"], "status": row["status"], "players": row["stats"]["players"] }))
            .collect()
    }
}

async fn has_release(
    State(inner): State<Arc<Inner>>,
    headers: HeaderMap,
    UrlPath((service, version, file)): UrlPath<(String, String, String)>,
) -> Response {
    guard!(inner, headers);
    if ![&service, &version, &file].into_iter().all(|n| safe_name(n)) {
        return refuse(StatusCode::BAD_REQUEST, "names may hold letters, digits, '.', '_' and '-'");
    }
    let path = inner.data.join("releases").join(&service).join(&version).join(&file);
    // An unsigned copy is as good as none to an agent that wants a
    // signature: the fleet uploads it again, signed.
    let usable = {
        let inner = inner.clone();
        let path = path.clone();
        tokio::task::spawn_blocking(move || path.is_file() && inner.signed(&path).is_ok()).await.unwrap_or(false)
    };
    if usable {
        Json(json!({ "ok": true })).into_response()
    } else {
        refuse(StatusCode::NOT_FOUND, "no such release")
    }
}

async fn upload(
    State(inner): State<Arc<Inner>>,
    headers: HeaderMap,
    UrlPath((service, version, file)): UrlPath<(String, String, String)>,
    body: Bytes,
) -> Response {
    guard!(inner, headers);
    if ![&service, &version, &file].into_iter().all(|n| safe_name(n)) {
        return refuse(StatusCode::BAD_REQUEST, "names may hold letters, digits, '.', '_' and '-'");
    }
    if version_of(&body) != version {
        return refuse(StatusCode::BAD_REQUEST, "those bytes are not that release: the hash does not match");
    }
    // A signed agent takes only what its release key signed: a token alone
    // can no longer put a program on this machine.
    let signature = headers.get(SIGNATURE_HEADER).and_then(|v| v.to_str().ok()).map(str::to_string);
    if let Some(key) = &inner.release_key
        && !signature.as_deref().is_some_and(|sig| key.signed(&body, sig))
    {
        let why = match signature {
            None => "this agent runs only signed releases, and this one came with no signature (jengine-fleet sign, or \"release_key_file\" in the fleet file)",
            Some(_) => "this release's signature is not by this agent's release key",
        };
        log::warn!("[agent] refused an upload of {service} {version}: {why}");
        return refuse(StatusCode::FORBIDDEN, why);
    }
    let dir = inner.data.join("releases").join(&service).join(&version);
    let path = dir.join(&file);
    let sig_path = dir.join(format!("{file}.sig"));
    let written = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, &body)).and_then(|_| match &signature {
        Some(sig) => std::fs::write(&sig_path, sig),
        None => Ok(()),
    });
    if let Err(e) = written {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, &format!("could not store the release: {e}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }
    if super::is_archive(&file)
        && let Err(why) = unpack(&body, &dir.join("unpacked"))
    {
        let _ = std::fs::remove_dir_all(&dir);
        return refuse(StatusCode::BAD_REQUEST, &format!("could not unpack the archive: {why}"));
    }
    log::info!("[agent] stored {service} {version} ({} bytes)", body.len());
    Json(json!({ "ok": true, "bytes": body.len() })).into_response()
}

#[derive(Deserialize)]
struct DeployBody {
    version: String,
    file: String,
    #[serde(default)]
    spec: ServiceSpec,
}

async fn send(inner: &Arc<Inner>, name: &str, make: impl FnOnce(Reply) -> Cmd) -> Response {
    let tx = match inner.supervisor(name) {
        Ok(tx) => tx,
        Err(why) => return refuse(StatusCode::SERVICE_UNAVAILABLE, &why),
    };
    let (reply, answer) = oneshot::channel();
    if tx.send(make(reply)).await.is_err() {
        return refuse(StatusCode::SERVICE_UNAVAILABLE, "the service is being removed");
    }
    match answer.await {
        Ok(Ok(message)) => Json(json!({ "ok": true, "message": message })).into_response(),
        Ok(Err(why)) => refuse(StatusCode::CONFLICT, &why),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "the service's supervisor stopped"),
    }
}

async fn deploy(
    State(inner): State<Arc<Inner>>,
    headers: HeaderMap,
    UrlPath(name): UrlPath<String>,
    Json(body): Json<DeployBody>,
) -> Response {
    guard!(inner, headers);
    if ![&name, &body.version, &body.file].into_iter().all(|n| safe_name(n)) {
        return refuse(StatusCode::BAD_REQUEST, "names may hold letters, digits, '.', '_' and '-'");
    }
    if name == super::gateway::CONSOLE {
        return refuse(StatusCode::BAD_REQUEST, "'manage' is the console's path on a host's gateway; name the service something else");
    }
    // A deploy's own settings may not reach past its release: the file it
    // runs stays inside the archive, and nothing is loaded in front of it.
    if let Some(exe) = &body.spec.exe
        && !super::exe_inside(exe)
    {
        return refuse(StatusCode::BAD_REQUEST, "an archive's \"exe\" is a path inside it, with no '..' and no root");
    }
    if inner.release_key.is_some()
        && let Some(var) = body.spec.env.keys().find(|k| super::loader_variable(k))
    {
        return refuse(StatusCode::FORBIDDEN, &format!("{var} would load something besides the signed release; this agent does not set it"));
    }
    let release = Release { file: body.file, version: body.version, spec: body.spec };
    if !inner.release_path(&name, &release).is_file() {
        return refuse(StatusCode::NOT_FOUND, "that release has not been uploaded to this machine");
    }
    let checked = {
        let (inner, path) = (inner.clone(), inner.uploaded_path(&name, &release));
        tokio::task::spawn_blocking(move || inner.signed(&path)).await.unwrap_or_else(|_| Err("the check failed".into()))
    };
    if let Err(why) = checked {
        return refuse(StatusCode::FORBIDDEN, &why);
    }
    log::info!("[agent] deploying {name} {}", release.version);
    send(&inner, &name, |reply| Cmd::Deploy(release, reply)).await
}

/// The largest answer [`proxy`] carries back from a server.
const MAX_PROXIED_BYTES: u64 = 32 << 20;

/// A request to one of this machine's servers, made here: the server need
/// not listen beyond loopback, and its admin token is put on by the agent -
/// so whoever holds the agent's token reaches every server's dashboard
/// without holding theirs. Its answer is passed back as it is, marked
/// `x-jengine-proxied` so it is not taken for the agent's own refusal.
async fn proxy(
    State(inner): State<Arc<Inner>>,
    headers: HeaderMap,
    method: axum::http::Method,
    UrlPath((name, path)): UrlPath<(String, String)>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
    body: Bytes,
) -> Response {
    guard!(inner, headers);
    let spec = inner.saved.lock().unwrap().services.get(&name).map(|s| s.current.spec.clone());
    let Some(spec) = spec else {
        return refuse(StatusCode::NOT_FOUND, "no such service on this machine");
    };
    let Some(port) = spec.port else {
        return refuse(StatusCode::NOT_FOUND, "that service has no port");
    };
    let url = match query {
        Some(q) => format!("http://127.0.0.1:{port}/{path}?{q}"),
        None => format!("http://127.0.0.1:{port}/{path}"),
    };
    let kind = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_string);
    let forwarded: Vec<(&'static str, String)> = ["x-forwarded-host", "x-forwarded-proto", "x-forwarded-prefix"]
        .into_iter()
        .filter_map(|name| Some((name, headers.get(name)?.to_str().ok()?.to_string())))
        .collect();
    let answer = tokio::task::spawn_blocking(move || {
        let mut request = ureq::request(method.as_str(), &url).timeout(Duration::from_secs(30));
        if let Some(token) = &spec.admin_token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        if let Some(kind) = &kind {
            request = request.set("Content-Type", kind);
        }
        // Where the caller was reached, as the console says: what a server
        // describes itself by (live ops' pack and publish addresses).
        for (name, value) in &forwarded {
            request = request.set(name, value);
        }
        let result = if body.is_empty() && matches!(method, axum::http::Method::GET | axum::http::Method::HEAD) {
            request.call()
        } else {
            request.send_bytes(&body)
        };
        let response = match result {
            Ok(r) | Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(e.to_string()),
        };
        let status = response.status();
        let kind = response.header("content-type").unwrap_or("application/octet-stream").to_string();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(response.into_reader(), MAX_PROXIED_BYTES), &mut bytes)
            .map_err(|e| e.to_string())?;
        Ok((status, kind, bytes))
    })
    .await;
    match answer {
        Ok(Ok((status, kind, bytes))) => (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            [(header::CONTENT_TYPE, kind), (header::HeaderName::from_static("x-jengine-proxied"), "1".to_string())],
            bytes,
        )
            .into_response(),
        Ok(Err(why)) => refuse(StatusCode::BAD_GATEWAY, &format!("'{name}' did not answer: {why}")),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "the request failed"),
    }
}

async fn act(
    State(inner): State<Arc<Inner>>,
    headers: HeaderMap,
    UrlPath((name, action)): UrlPath<(String, String)>,
) -> Response {
    guard!(inner, headers);
    if !inner.saved.lock().unwrap().services.contains_key(&name) {
        return refuse(StatusCode::NOT_FOUND, "no such service on this machine");
    }
    log::info!("[agent] {action} {name}");
    match action.as_str() {
        "start" => send(&inner, &name, Cmd::Start).await,
        "stop" => send(&inner, &name, Cmd::Stop).await,
        "restart" => send(&inner, &name, Cmd::Restart).await,
        "rollback" => send(&inner, &name, Cmd::Rollback).await,
        "remove" => send(&inner, &name, Cmd::Remove).await,
        _ => refuse(StatusCode::NOT_FOUND, "the actions are start, stop, restart, rollback and remove"),
    }
}

#[derive(Deserialize)]
struct LogQuery {
    lines: Option<usize>,
    /// Only lines containing this, across the kept generations.
    grep: Option<String>,
}

async fn logs(
    State(inner): State<Arc<Inner>>,
    headers: HeaderMap,
    UrlPath(name): UrlPath<String>,
    Query(query): Query<LogQuery>,
) -> Response {
    guard!(inner, headers);
    if !safe_name(&name) {
        return refuse(StatusCode::BAD_REQUEST, "no such service");
    }
    let lines = query.lines.unwrap_or(100).min(5000);
    let dir = inner.service_dir(&name);
    let text = match query.grep.as_deref().filter(|g| !g.is_empty()) {
        None => tail(&dir.join("service.log"), lines),
        // Oldest generation first, so the last `lines` matches are the newest.
        Some(needle) => {
            let mut found: Vec<String> = Vec::new();
            for n in (1..=LOG_GENERATIONS).rev() {
                found.extend(matching(&dir.join(format!("service.log.{n}")), needle));
            }
            found.extend(matching(&dir.join("service.log"), needle));
            found[found.len().saturating_sub(lines)..].join("\n")
        }
    };
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
}

/// Unpack a zip into `into`, refusing an entry that would land outside it.
/// Every file is made executable on Unix: the archive does not say which
/// one is the program, and a data file that may be run does no harm.
fn unpack(bytes: &[u8], into: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(into).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = entry.enclosed_name() else {
            return Err(format!("'{}' would escape the archive", entry.name()));
        };
        let path = into.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }
    Ok(())
}

/// Every line of a file containing `needle`; nothing for a file that is not there.
fn matching(path: &Path, needle: &str) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    text.lines().filter(|l| l.contains(needle)).map(str::to_string).collect()
}

/// The last `lines` lines of a file, reading no more than its last megabyte.
fn tail(path: &Path, lines: usize) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(len.saturating_sub(1 << 20)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

// ---- running one ------------------------------------------------------------------

/// An agent started with [`start`].
pub struct RunningAgent {
    pub addr: SocketAddr,
    /// Where its gateway listens, if it was given one.
    pub gateway: Option<SocketAddr>,
    /// Whether that gateway speaks TLS.
    pub gateway_secure: bool,
    /// The console behind the gateway, if it has one.
    pub console: Option<super::ui::RunningUi>,
    secure: bool,
    shutdown: watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RunningAgent {
    pub fn url(&self) -> String {
        format!("{}://{}", if self.secure { "https" } else { "http" }, self.addr)
    }

    /// Stop the agent and, cleanly, every server it runs.
    pub fn stop(mut self) {
        if let Some(console) = self.console.take() {
            console.stop();
        }
        let _ = self.shutdown.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for RunningAgent {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

/// Run an agent on a thread of its own. `data` is where it keeps releases,
/// its services' working directories and `state.json`; whatever that says
/// was running is started again.
pub fn start(addr: &str, data: impl Into<PathBuf>, token: &str) -> std::io::Result<RunningAgent> {
    start_with(addr, data, token, None)
}

/// [`start`], over HTTPS when given a certificate. The agent's token runs
/// programs on this machine; with a certificate it never crosses the network
/// where it can be read.
pub fn start_with(
    addr: &str,
    data: impl Into<PathBuf>,
    token: &str,
    tls: Option<crate::listen::Tls>,
) -> std::io::Result<RunningAgent> {
    start_host(addr, data, token, tls, HostOptions::default())
}

/// What makes an agent a **host**: a machine players connect to, and that
/// its operator manages from a browser.
#[derive(Debug, Clone, Default)]
pub struct HostOptions {
    /// Where the [gateway](super::gateway) listens: one port for players, in
    /// front of every service this agent runs.
    pub gateway: Option<String>,
    /// With a gateway, the token of a **console** served behind it at
    /// `/manage`: the fleet's web interface run on the host itself, its fleet
    /// file, jobs and uploaded builds kept in `<data>/console/`.
    pub console_token: Option<String>,
    /// Proxies in front of the gateway (a TLS proxy on another machine)
    /// whose word on who a connection is for is believed - see
    /// [`super::gateway`].
    pub trust_proxies: Vec<std::net::IpAddr>,
    /// A certificate for the gateway: `https://` and `wss://` for players
    /// and the console. Without one the gateway is plain, and the console
    /// refuses a token from another machine unless `allow_plain_console`.
    pub gateway_tls: Option<crate::listen::Tls>,
    /// Let the console take its token over a plain network (a LAN you trust).
    pub allow_plain_console: bool,
    /// Run only releases this key signed ([`super::ReleaseSigner`]).
    pub release_key: Option<ReleaseKey>,
    /// Listen beyond loopback without a release key - so whoever steals the
    /// agent's token can run programs here. Off: such an agent will not start.
    pub allow_unsigned: bool,
}

/// The machine name a host's console calls itself by in its own fleet file.
pub const CONSOLE_MACHINE: &str = "host";

/// [`start_with`], as a host: a gateway, and a console behind it.
pub fn start_host(
    addr: &str,
    data: impl Into<PathBuf>,
    token: &str,
    tls: Option<crate::listen::Tls>,
    host: HostOptions,
) -> std::io::Result<RunningAgent> {
    let secure = tls.is_some();
    if token.len() < 8 {
        return Err(std::io::Error::other(
            "an agent needs a token of at least 8 characters: it is what stands between the network and running programs on this machine",
        ));
    }
    let data: PathBuf = data.into();
    std::fs::create_dir_all(&data)?;
    let saved: Saved = std::fs::read_to_string(data.join("state.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    // Reachable from the network with nothing but a token between a thief
    // and running programs here: not without being told that is meant.
    // Asked before the port is opened, and again once it is (a host name).
    let unsigned_beyond_loopback = |ip: std::net::IpAddr| {
        if !ip.is_loopback() && host.release_key.is_none() && !host.allow_unsigned {
            return Err(std::io::Error::other(
                "an agent listening beyond this machine runs only signed releases: give it --release-key <jfr-public-..> (jengine-fleet keygen makes one), or --allow-unsigned to let its token alone run programs here",
            ));
        }
        Ok(())
    };
    if let Ok(asked) = addr.parse::<SocketAddr>() {
        unsigned_beyond_loopback(asked.ip())?;
    }
    let listener = std::net::TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    unsigned_beyond_loopback(addr.ip())?;
    let gateway = match &host.gateway {
        Some(at) => {
            let listener = std::net::TcpListener::bind(at)?;
            listener.set_nonblocking(true)?;
            Some(listener)
        }
        None => None,
    };
    let gateway_addr = gateway.as_ref().map(|l| l.local_addr()).transpose()?;
    // The console reaches this agent on loopback, as any fleet machine, and
    // the gateway is its only way in from the network.
    let console = match (&gateway, host.console_token.as_deref()) {
        (Some(_), Some(console_token)) => {
            let dir = data.join("console");
            std::fs::create_dir_all(&dir)?;
            let fleet_path = dir.join("fleet.json");
            if !fleet_path.is_file() {
                std::fs::write(&fleet_path, "{ \"machines\": {}, \"services\": {} }")?;
            }
            // Rewritten at every start, so a changed token or port is followed.
            let mut machine = json!({
                "url": format!("{}://127.0.0.1:{}", if secure { "https" } else { "http" }, addr.port()),
                "token": token,
            });
            if let Some(tls) = &tls {
                machine["ca"] = json!(std::path::absolute(&tls.cert)?);
            }
            super::client::put_machine(&fleet_path, CONSOLE_MACHINE, machine).map_err(std::io::Error::other)?;
            Some(super::ui::start_hosted_with("127.0.0.1:0", &dir, console_token, host.allow_plain_console)?)
        }
        _ => None,
    };
    let routes = HostRoutes { inner: None, console: console.as_ref().map(|c| c.addr.port()) };
    let trusted = host.trust_proxies.clone();
    let gateway_tls = host.gateway_tls.clone();
    let gateway_secure = gateway_tls.is_some();
    let release_key = host.release_key;
    let (shutdown, mut shutdown_rx) = watch::channel(false);
    let gateway_stop = shutdown_rx.clone();
    let machine = super::watch::watch_machine(data.clone(), shutdown_rx.clone());
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let token = token.to_string();
    let thread = std::thread::spawn(move || {
        runtime.block_on(async move {
            let (done, mut wait) = mpsc::channel::<()>(1);
            let names: Vec<String> = saved.services.keys().cloned().collect();
            let inner = Arc::new(Inner {
                data,
                token,
                release_key,
                saved: Mutex::new(saved),
                live: Mutex::new(HashMap::new()),
                services: Mutex::new(HashMap::new()),
                done: Mutex::new(Some(done)),
                machine,
                logs: Mutex::new(HashMap::new()),
            });
            for name in names {
                let _ = inner.supervisor(&name);
            }
            if let Some(listener) = gateway {
                match tokio::net::TcpListener::from_std(listener) {
                    Ok(listener) => {
                        let routes = Arc::new(HostRoutes { inner: Some(inner.clone()), ..routes });
                        tokio::spawn(super::gateway::serve_tls(listener, routes, trusted.into(), gateway_tls, gateway_stop));
                    }
                    Err(e) => log::error!("[agent] the gateway could not listen: {e}"),
                }
            }
            let router = Router::new()
                .route("/v1/info", get(info))
                .route("/v1/status", get(status))
                .route("/v1/releases/{service}/{version}/{file}", put(upload).get(has_release))
                .route("/v1/services/{name}/deploy", post(deploy))
                .route("/v1/services/{name}/logs", get(logs))
                .route("/v1/services/{name}/{action}", post(act))
                .route("/v1/services/{name}/http/{*path}", axum::routing::any(proxy))
                .layer(DefaultBodyLimit::max(MAX_RELEASE_BYTES))
                .with_state(inner.clone());
            let stop = async move {
                let _ = shutdown_rx.changed().await;
            };
            if let Err(e) = crate::listen::serve(listener, router, tls, stop).await {
                log::error!("[agent] {e}");
            }
            // Dropping the senders is what tells each supervisor to stop its
            // server; then wait for them to have done it.
            inner.services.lock().unwrap().clear();
            inner.done.lock().unwrap().take();
            let _ = tokio::time::timeout(Duration::from_secs(90), wait.recv()).await;
        });
    });
    Ok(RunningAgent { addr, gateway: gateway_addr, gateway_secure, console, secure, shutdown, thread: Some(thread) })
}

/// What a host's gateway routes to: the console at `/manage`, then the
/// agent's services.
struct HostRoutes {
    inner: Option<Arc<Inner>>,
    console: Option<u16>,
}

impl super::gateway::Routes for HostRoutes {
    fn port_of(&self, service: &str) -> Option<u16> {
        if service == super::gateway::CONSOLE {
            return self.console;
        }
        super::gateway::Routes::port_of(&**self.inner.as_ref()?, service)
    }

    async fn listing(&self) -> Vec<Value> {
        match &self.inner {
            Some(inner) => super::gateway::Routes::listing(&**inner).await,
            None => Vec::new(),
        }
    }

    /// Through the console, which asks for its own token: the servers' admin
    /// tokens a console made are known to nobody.
    fn dashboard(&self, service: &str) -> String {
        match self.console {
            Some(_) => format!("/{}/dash/{CONSOLE_MACHINE}/{service}/admin", super::gateway::CONSOLE),
            None => format!("/{service}/admin"),
        }
    }
}

/// Run an agent until Ctrl+C.
pub fn run(
    addr: &str,
    data: impl Into<PathBuf>,
    token: &str,
    tls: Option<crate::listen::Tls>,
    host: HostOptions,
) -> std::io::Result<()> {
    let agent = start_host(addr, data, token, tls, host)?;
    log::info!("[agent] listening on {} ({})", agent.url(), platform());
    if let Some(at) = agent.gateway {
        let ws = if agent.gateway_secure { "wss" } else { "ws" };
        log::info!("[agent] gateway on {at}: players connect to {ws}://<this machine>:{}/<service>", at.port());
        if !agent.gateway_secure {
            log::warn!("[agent] the gateway is plain: passwords and the console's token from other machines are refused there. Give it --gateway-tls-cert and --gateway-tls-key to take them.");
        }
    }
    if agent.console.is_some() {
        log::info!("[agent] console behind the gateway at /manage");
    }
    if !agent.secure {
        log::warn!("[agent] plain HTTP: the token can be read by whoever can see this traffic. Give it --tls-cert and --tls-key, or keep it on a private network.");
    }
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(async {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let mut term = signal(SignalKind::terminate()).expect("a SIGTERM handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
    });
    log::info!("[agent] stopping its services");
    agent.stop();
    Ok(())
}
