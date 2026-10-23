//! `jengine-fleet ui`: the fleet in a browser.
//!
//! A small web server **on your own machine** that reads the same
//! `fleet.json` the command line does and shows it as a page: every machine
//! and what runs on it, a Deploy button per service, rollback / restart /
//! stop / start per machine, the logs, and each deploy's progress as it
//! happens.
//!
//! # It is the command line with a face
//!
//! The page calls this server, and this server calls the agents through the
//! very functions the command line uses ([`client::deploy`], [`client::act`],
//! [`client::logs`]) - so there is one implementation of a deploy, and what
//! the page does a script can do. **The agents' tokens never reach the
//! browser**: they stay in this process, which is also why the page needs no
//! cross-origin permission from an agent.
//!
//! # Who may press the buttons
//!
//! Anything that can call this server can deploy, so every `/api` call needs
//! a token. Listening on loopback (the default) one is made up at start and
//! written into the page as it is served - another web site open in the same
//! browser can send a request here but cannot read that page, so it cannot
//! learn the token. Listening on anything else needs `--token`, and the page
//! asks for it instead of carrying it.
//!
//! **More than one person** signs in with tokens of their own: a users file
//! (`--users users.json`, or `<data>/console/users.json` for a host's
//! console) names each with a role - a `viewer` sees everything and changes
//! nothing, an `operator` deploys - and keeps only a hash of their token.
//! `jengine-fleet user add <name> --role viewer` makes one; removing the
//! line ends it at once. Every job says who started it.
//!
//! **A token that crossed the network in plain text is refused** - reached
//! through a host's gateway without TLS from another machine, say - unless
//! the console was started with `--allow-plain-console` (a LAN you trust).
//! Give the gateway a certificate (`--gateway-tls-cert`), or host the page
//! with `--tls-cert`.
//!
//! # Jobs
//!
//! A deploy takes as long as a drain does, so it runs on a thread of its own
//! as a **job** the page polls: the same lines the command line prints, then
//! how it ended. One job per service at a time; the last thirty are kept -
//! in memory, or for a host's console in `jobs.json`, so they outlive a
//! restart.
//!
//! # Each server's dashboard, through this page
//!
//! `dash/<machine>/<service>/admin` is the server's own dashboard, fetched by
//! the agent on its machine with the server's admin token. The page opens it
//! with this page's token in its fragment, the dashboard sends that back as
//! its bearer, and this server checks it - so one sign-in reaches every
//! server, wherever it listens, and neither the agents' nor the servers'
//! tokens reach the browser.
//!
//! # A host's console
//!
//! `jengine-fleet host` serves this page behind its gateway at `/manage`
//! ([`start_hosted`]): its own fleet file in `<data>/console/` naming the
//! host as the machine `host`, its jobs kept there, and builds **uploaded
//! from the page** into `builds/` beside the file - a host has no build tree.
//! A service made there gets an admin token written in, made up once and
//! kept: nobody needs to know it, since its dashboard is reached through the
//! console. Reached through a gateway, the page never carries its token,
//! even though the request arrives from loopback.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::watch;

use super::client::{self, Fleet};
use super::version_of;

const MAX_JOBS: usize = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Job {
    id: u64,
    title: String,
    service: String,
    lines: Vec<String>,
    /// `running`, `ok` or `failed`.
    state: String,
    error: Option<String>,
    started: f64,
    /// Who started it: a user's name, or `console` for the page's own token.
    #[serde(default)]
    by: String,
}

/// Somebody signed in to the page, and what they may do.
#[derive(Debug, Clone, PartialEq)]
struct User {
    name: String,
    /// May deploy, act and edit; otherwise only look.
    operator: bool,
}

/// A users file: `{"users": {"ada": {"role": "operator", "sha256": ".."}}}`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Users {
    #[serde(default)]
    pub users: std::collections::BTreeMap<String, UserEntry>,
}

/// One person in a users file: a role and the SHA-256 of their token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserEntry {
    /// `viewer` or `operator`.
    pub role: String,
    /// Lowercase hex of the SHA-256 of the token; the token itself is never kept.
    pub sha256: String,
}

fn sha256_hex(text: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

impl Users {
    pub fn load(path: &Path) -> Result<Users, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{} is not a users file: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Users::default()),
            Err(e) => Err(format!("could not read {}: {e}", path.display())),
        }
    }

    /// Add (or give a new token to) `name`, write the file, and return the
    /// token - said once, here, and kept only as a hash.
    pub fn add(path: &Path, name: &str, role: &str) -> Result<String, String> {
        if !super::safe_name(name) {
            return Err("a user's name is letters, digits, '.', '_' and '-'".into());
        }
        if role != "viewer" && role != "operator" {
            return Err(format!("no role '{role}': viewer or operator"));
        }
        let mut users = Users::load(path)?;
        let mut bytes = [0u8; 24];
        getrandom::fill(&mut bytes).expect("the OS has randomness");
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        users.users.insert(name.to_string(), UserEntry { role: role.to_string(), sha256: sha256_hex(&token) });
        users.save(path)?;
        Ok(token)
    }

    pub fn remove(path: &Path, name: &str) -> Result<(), String> {
        let mut users = Users::load(path)?;
        users.users.remove(name).ok_or_else(|| format!("no user called '{name}'"))?;
        users.save(path)
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)
            .and_then(|_| std::fs::rename(&tmp, path))
            .map_err(|e| format!("could not write {}: {e}", path.display()))
    }
}

struct Ui {
    fleet_path: PathBuf,
    /// For a console a host serves: the folder holding its fleet file, its
    /// jobs (`jobs.json`, so they outlive a restart) and uploaded builds.
    hosted: Option<PathBuf>,
    token: String,
    /// Whether the page is served with the token in it (loopback only).
    embed: bool,
    /// People with tokens of their own, and their roles.
    users: Option<PathBuf>,
    /// Where the jobs are kept across restarts - a hosted console's
    /// `jobs.json`, or `--jobs` for a page on one's own machine (U1).
    jobs_file: Option<PathBuf>,
    /// Over its own TLS.
    secure: bool,
    /// Take a token that crossed the network in plain text.
    allow_plain: bool,
    jobs: Mutex<VecDeque<Job>>,
    next_job: AtomicU64,
    /// path -> (modified, length, version): hashing a build is not free, and
    /// the page asks every few seconds.
    hashes: Mutex<HashMap<PathBuf, (SystemTime, u64, String)>>,
    /// Flips when the page's server is stopping: what ends a pushed stream,
    /// which would otherwise hold its connection open through the shutdown.
    stopping: watch::Receiver<bool>,
    /// Every machine and service sampled every few seconds, for the
    /// dashboard's charts.
    history: Mutex<super::history::History>,
    /// What the samples say is wrong, for the webhook and `/api/alerts`.
    alerts: Mutex<super::alerts::Alerts>,
}

fn unix_now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

impl Ui {
    /// The release a local build would be, if the file is there.
    fn local_version(&self, path: &Path) -> Option<(u64, String)> {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        let mut hashes = self.hashes.lock().unwrap();
        if let Some((at, len, version)) = hashes.get(path)
            && (*at, *len) == (modified, meta.len())
        {
            return Some((*len, version.clone()));
        }
        let version = version_of(&std::fs::read(path).ok()?);
        hashes.insert(path.to_path_buf(), (modified, meta.len(), version.clone()));
        Some((meta.len(), version))
    }

    /// What the fleet file says, without anything secret in it.
    fn describe(&self) -> Value {
        let fleet = match Fleet::load(&self.fleet_path) {
            Ok(fleet) => fleet,
            Err(why) => return json!({ "file": self.fleet_path, "error": why, "machines": [], "services": [] }),
        };
        let machines: Vec<Value> =
            fleet.machines.iter().map(|(name, m)| json!({ "name": name, "url": m.url })).collect();
        let services: Vec<Value> = fleet
            .services
            .iter()
            .map(|(name, def)| {
                let artifacts: Vec<Value> = def
                    .artifacts
                    .iter()
                    .map(|(platform, path)| {
                        let local = self.local_version(&fleet.base.join(path));
                        json!({
                            "platform": platform, "path": path,
                            "bytes": local.as_ref().map(|l| l.0),
                            "version": local.as_ref().map(|l| &l.1),
                        })
                    })
                    .collect();
                let on: Vec<Value> = def
                    .machines
                    .iter()
                    .map(|m| json!({ "name": m, "port": def.spec_for(m).ok().and_then(|s| s.port) }))
                    .collect();
                json!({
                    "name": name, "machines": on, "artifacts": artifacts,
                    "drain_secs": def.spec.drain_secs,
                    "has_admin": def.has_admin_token(),
                })
            })
            .collect();
        json!({ "file": self.fleet_path, "error": null, "machines": machines, "services": services })
    }

    /// Start a job, unless that service already has one running.
    fn begin(self: &Arc<Self>, title: String, service: &str, by: &str) -> Result<u64, String> {
        let id = {
            let mut jobs = self.jobs.lock().unwrap();
            if jobs.iter().any(|j| j.service == service && j.state == "running") {
                return Err(format!("'{service}' already has a job running; wait for it to finish"));
            }
            let id = self.next_job.fetch_add(1, Ordering::Relaxed) + 1;
            if jobs.len() >= MAX_JOBS {
                jobs.pop_front();
            }
            jobs.push_back(Job {
                id,
                title,
                service: service.to_string(),
                lines: Vec::new(),
                state: "running".into(),
                error: None,
                started: unix_now(),
                by: by.to_string(),
            });
            id
        };
        self.save_jobs();
        Ok(id)
    }

    /// A hosted console's jobs, written where the next start reads them.
    fn save_jobs(&self) {
        let Some(file) = &self.jobs_file else { return };
        let jobs: Vec<Job> = self.jobs.lock().unwrap().iter().cloned().collect();
        let text = serde_json::to_string_pretty(&jobs).unwrap_or_default();
        let tmp = file.with_extension("json.tmp");
        if std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, file)).is_err() {
            log::warn!("[console] could not write {}", file.display());
        }
    }

    fn update(&self, id: u64, change: impl FnOnce(&mut Job)) {
        if let Some(job) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
            change(job);
        }
    }

    /// Run `work` as a job on a thread of its own.
    fn run(
        self: &Arc<Self>,
        title: String,
        service: String,
        by: &str,
        work: impl FnOnce(&Fleet, &mut dyn FnMut(&str)) -> Result<(), String> + Send + 'static,
    ) -> Result<u64, String> {
        let id = self.begin(title, &service, by)?;
        let ui = self.clone();
        std::thread::spawn(move || {
            let mut say = |line: &str| ui.update(id, |job| job.lines.push(line.to_string()));
            let result = Fleet::load(&ui.fleet_path).and_then(|fleet| work(&fleet, &mut say));
            ui.update(id, |job| match result {
                Ok(()) => job.state = "ok".into(),
                Err(why) => {
                    job.state = "failed".into();
                    job.error = Some(why);
                }
            });
            ui.save_jobs();
        });
        Ok(id)
    }
}

fn refuse(status: StatusCode, why: &str) -> Response {
    (status, Json(json!({ "error": why }))).into_response()
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Who a token is: the page's own (`console`, an operator), or somebody in
/// the users file. Compared in constant time, every entry.
fn user_of(ui: &Ui, given: &str) -> Option<User> {
    if given.is_empty() {
        return None;
    }
    if same(&ui.token, given) {
        return Some(User { name: "console".into(), operator: true });
    }
    let path = ui.users.as_ref()?;
    let users = Users::load(path).map_err(|why| log::warn!("[console] {why}")).ok()?;
    let hash = sha256_hex(given);
    let mut found = None;
    for (name, entry) in &users.users {
        if same(&entry.sha256, &hash) && found.is_none() {
            found = Some(User { name: name.clone(), operator: entry.role == "operator" });
        }
    }
    found
}

fn who(ui: &Ui, headers: &HeaderMap) -> Option<User> {
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    user_of(ui, given)
}

/// May look: anybody signed in.
macro_rules! guard {
    ($ui:expr, $headers:expr) => {
        if who(&$ui, &$headers).is_none() {
            return refuse(StatusCode::UNAUTHORIZED, "the token is missing or wrong");
        }
    };
}

/// May change something: an operator. Evaluates to who it is.
macro_rules! guard_write {
    ($ui:expr, $headers:expr) => {
        match who(&$ui, &$headers) {
            Some(user) if user.operator => user.name,
            Some(user) => return refuse(StatusCode::FORBIDDEN, &format!("{} may look, not change things", user.name)),
            None => return refuse(StatusCode::UNAUTHORIZED, "the token is missing or wrong"),
        }
    };
}

/// In front of everything but the page itself: a request that crossed the
/// network in plain text carries a token anyone on the way could read, so
/// it is refused unless the console was told the network is trusted.
async fn wire_guard(
    State(ui): State<Arc<Ui>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let path = request.uri().path();
    let page = request.method() == axum::http::Method::GET
        && (path == "/" || path.ends_with("/admin") || path.ends_with("/admin/liveops"));
    let in_clear = crate::listen::headers_in_clear(peer.ip(), request.headers(), ui.secure, |ip| ip.is_loopback());
    if !page && in_clear && !ui.allow_plain {
        return refuse(
            StatusCode::FORBIDDEN,
            &format!(
                "{}: give the gateway a certificate (--gateway-tls-cert / --gateway-tls-key), or start the console with --allow-plain-console on a network you trust",
                crate::listen::NEEDS_TLS
            ),
        );
    }
    next.run(request).await
}

async fn page(State(ui): State<Arc<Ui>>, headers: HeaderMap) -> Html<String> {
    // Listening on loopback is not the same as being asked from this
    // machine: a request a gateway carried in comes from loopback too, and
    // says so. That one is asked for the token like any other.
    let forwarded = headers.contains_key("x-forwarded-for");
    let token = if ui.embed && !forwarded { ui.token.as_str() } else { "" };
    let hosted = if ui.hosted.is_some() { "true" } else { "false" };
    Html(include_str!("ui.html").replace("__TOKEN__", token).replace("__HOSTED__", hosted))
}

/// Whether a request through `dash/` may go without the token: only the
/// dashboards' own pages, which are static and ask for a token themselves.
fn dashboard_page(method: &axum::http::Method, path: &str) -> bool {
    method == axum::http::Method::GET && matches!(path, "admin" | "admin/liveops")
}

/// Whether a request through `dash/` only looks: what a viewer may send.
fn only_looks(method: &axum::http::Method) -> bool {
    matches!(*method, axum::http::Method::GET | axum::http::Method::HEAD)
}

/// `dash/<machine>/<service>/<path>`: a server's own pages and API, reached
/// through this page's token and the agent on its machine. The dashboard is
/// opened with this page's token in its fragment, sends it back as its
/// bearer, and never sees the server's admin token - the agent puts that on.
async fn dashboard(
    State(ui): State<Arc<Ui>>,
    headers: HeaderMap,
    method: axum::http::Method,
    axum::extract::Path((machine, service, path)): axum::extract::Path<(String, String, String)>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
    body: axum::body::Bytes,
) -> Response {
    if !dashboard_page(&method, &path) {
        if only_looks(&method) {
            guard!(ui, headers);
        } else {
            let _who = guard_write!(ui, headers);
        }
    }
    let kind = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_string);
    // A host's console is reached through the gateway that serves this very
    // machine's servers at /<service>: so the server is told it was reached
    // there, as a game reaches it - not at the loopback address the agent
    // uses. Another machine's gateway, if any, is not this one's to name.
    let mut forward: Vec<(&str, String)> = Vec::new();
    if ui.hosted.is_some()
        && machine == super::agent::CONSOLE_MACHINE
        && let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok())
    {
        forward.push(("x-forwarded-host", host.to_string()));
        forward.push(("x-forwarded-prefix", format!("/{service}")));
    }
    let path = match query {
        Some(q) => format!("{path}?{q}"),
        None => path,
    };
    let answer = tokio::task::spawn_blocking(move || {
        let fleet = Fleet::load(&ui.fleet_path)?;
        let body = (!body.is_empty()).then(|| (kind.as_deref().unwrap_or("application/json"), &body[..]));
        client::service_http(&fleet, &machine, &service, method.as_str(), &path, body, &forward)
    })
    .await;
    match answer {
        Ok(Ok(p)) => (
            StatusCode::from_u16(p.status).unwrap_or(StatusCode::BAD_GATEWAY),
            [(header::CONTENT_TYPE, p.content_type)],
            p.body,
        )
            .into_response(),
        Ok(Err(why)) => refuse(StatusCode::BAD_GATEWAY, &why),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "the request failed"),
    }
}

/// The largest build a hosted console takes.
const MAX_BUILD_BYTES: usize = 1 << 30;

#[derive(Deserialize)]
struct BuildQuery {
    file: String,
}

/// `PUT api/build/<service>/<platform>?file=<name>`: a build uploaded from
/// the page, for a console a host serves - which has no build tree of its
/// own. It is kept under `builds/` beside the console's fleet file, and a
/// service already in the file is pointed at it; answers the path to name
/// as the artifact.
async fn upload_build(
    State(ui): State<Arc<Ui>>,
    headers: HeaderMap,
    axum::extract::Path((service, platform)): axum::extract::Path<(String, String)>,
    Query(q): Query<BuildQuery>,
    body: axum::body::Bytes,
) -> Response {
    let who = guard_write!(ui, headers);
    let Some(dir) = ui.hosted.clone() else {
        return refuse(StatusCode::NOT_FOUND, "builds are uploaded to a console a host serves; this page deploys the builds on its own machine");
    };
    if ![&service, &platform, &q.file].into_iter().all(|n| super::safe_name(n)) {
        return refuse(StatusCode::BAD_REQUEST, "names may hold letters, digits, '.', '_' and '-'");
    }
    if body.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "the build is empty");
    }
    let signature = headers
        .get(super::agent::SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit()));
    let done = tokio::task::spawn_blocking(move || {
        let relative = format!("builds/{service}/{platform}/{}", q.file);
        let path = dir.join(&relative);
        let folder = path.parent().unwrap_or(&dir);
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("upload");
        std::fs::write(&tmp, &body).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| e.to_string())?;
        // Its signature, made where it was built (`jengine-fleet sign`): kept
        // beside it, where a deploy looks for one. A new build with none
        // takes the old one away, so it is never sent with the wrong one.
        let sig_path = std::path::PathBuf::from(format!("{}.sig", path.display()));
        match &signature {
            Some(sig) => std::fs::write(&sig_path, sig).map_err(|e| e.to_string())?,
            None => {
                let _ = std::fs::remove_file(&sig_path);
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
        let known = Fleet::load(&ui.fleet_path).map(|f| f.services.contains_key(&service)).unwrap_or(false);
        if known {
            client::set_artifact(&ui.fleet_path, &service, &platform, &relative)?;
        }
        log::info!("[console] {who} uploaded {relative}");
        Ok::<_, String>(json!({ "path": relative, "version": version_of(&body), "bytes": body.len(), "signed": signature.is_some() }))
    })
    .await;
    match done {
        Ok(Ok(answer)) => Json(answer).into_response(),
        Ok(Err(why)) => refuse(StatusCode::INTERNAL_SERVER_ERROR, &why),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "the upload failed"),
    }
}

async fn fleet(State(ui): State<Arc<Ui>>, headers: HeaderMap) -> Response {
    guard!(ui, headers);
    let described = tokio::task::spawn_blocking(move || ui.describe()).await.unwrap_or(Value::Null);
    Json(described).into_response()
}

async fn status(State(ui): State<Arc<Ui>>, headers: HeaderMap) -> Response {
    guard!(ui, headers);
    let rows = tokio::task::spawn_blocking(move || {
        let fleet = Fleet::load(&ui.fleet_path)?;
        Ok::<_, String>(
            client::status(&fleet)
                .into_iter()
                .map(|(machine, answer)| match answer {
                    Ok(status) => json!({ "machine": machine, "status": status }),
                    Err(why) => json!({ "machine": machine, "error": why }),
                })
                .collect::<Vec<Value>>(),
        )
    })
    .await;
    match rows {
        Ok(Ok(rows)) => Json(json!({ "machines": rows })).into_response(),
        Ok(Err(why)) => refuse(StatusCode::CONFLICT, &why),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "the status check failed"),
    }
}

#[derive(Deserialize)]
struct Order {
    service: String,
    machine: Option<String>,
    /// For `/api/act`: start, stop, restart, rollback or remove.
    action: Option<String>,
}

async fn deploy(State(ui): State<Arc<Ui>>, headers: HeaderMap, Json(order): Json<Order>) -> Response {
    let who = guard_write!(ui, headers);
    let Order { service, machine, .. } = order;
    let title = match &machine {
        Some(machine) => format!("deploy {service} to {machine}"),
        None => format!("deploy {service}"),
    };
    let name = service.clone();
    match ui.run(title, service, &who, move |fleet, say| client::deploy(fleet, &name, machine.as_deref(), say)) {
        Ok(id) => Json(json!({ "job": id })).into_response(),
        Err(why) => refuse(StatusCode::CONFLICT, &why),
    }
}

async fn act(State(ui): State<Arc<Ui>>, headers: HeaderMap, Json(order): Json<Order>) -> Response {
    let who = guard_write!(ui, headers);
    let Order { service, machine, action } = order;
    let action = action.unwrap_or_default();
    if !["start", "stop", "restart", "rollback", "remove"].contains(&action.as_str()) {
        return refuse(StatusCode::BAD_REQUEST, "the actions are start, stop, restart, rollback and remove");
    }
    let title = match &machine {
        Some(machine) => format!("{action} {service} on {machine}"),
        None => format!("{action} {service}"),
    };
    let name = service.clone();
    match ui.run(title, service, &who, move |fleet, say| client::act(fleet, &name, &action, machine.as_deref(), say)) {
        Ok(id) => Json(json!({ "job": id })).into_response(),
        Err(why) => refuse(StatusCode::CONFLICT, &why),
    }
}

fn job_rows(ui: &Ui) -> Vec<Value> {
    ui.jobs
        .lock()
        .unwrap()
        .iter()
        .rev()
        .map(|j| {
            json!({
                "id": j.id, "title": j.title, "service": j.service, "lines": j.lines,
                "state": j.state, "error": j.error, "started": j.started, "by": j.by,
            })
        })
        .collect()
}

async fn jobs(State(ui): State<Arc<Ui>>, headers: HeaderMap) -> Response {
    guard!(ui, headers);
    Json(json!({ "jobs": job_rows(&ui) })).into_response()
}

/// The fleet file, edited from the page: a machine or a service put or
/// removed. The file is checked before it is written, so the page cannot
/// leave a file the command line would refuse.
#[derive(Deserialize)]
struct Edit {
    name: String,
    #[serde(flatten)]
    rest: serde_json::Map<String, Value>,
}

async fn put_machine(State(ui): State<Arc<Ui>>, headers: HeaderMap, Json(edit): Json<Edit>) -> Response {
    let _who = guard_write!(ui, headers);
    let done = tokio::task::spawn_blocking(move || client::put_machine(&ui.fleet_path, &edit.name, Value::Object(edit.rest))).await;
    edited(done)
}

async fn remove_machine(State(ui): State<Arc<Ui>>, headers: HeaderMap, Json(edit): Json<Edit>) -> Response {
    let _who = guard_write!(ui, headers);
    let done = tokio::task::spawn_blocking(move || client::remove_machine(&ui.fleet_path, &edit.name)).await;
    edited(done)
}

async fn put_service(State(ui): State<Arc<Ui>>, headers: HeaderMap, Json(edit): Json<Edit>) -> Response {
    let _who = guard_write!(ui, headers);
    let done = tokio::task::spawn_blocking(move || {
        let mut service = edit.rest;
        if ui.hosted.is_some() {
            hosted_service_defaults(&ui.fleet_path, &edit.name, &mut service);
        }
        client::put_service(&ui.fleet_path, &edit.name, Value::Object(service))
    })
    .await;
    edited(done)
}

/// A hosted console's services carry their admin token written out - the
/// host has no environment to name it in, and nobody needs to know it: the
/// dashboards are reached through the console. One the service already has
/// is kept, as are its uploaded builds the edit did not mention.
fn hosted_service_defaults(fleet_path: &Path, name: &str, service: &mut serde_json::Map<String, Value>) {
    let before: Value = std::fs::read_to_string(fleet_path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|f| f["services"][name].clone())
        .unwrap_or(Value::Null);
    if !service.contains_key("admin_token") && !service.contains_key("admin_token_env") {
        let token = before["admin_token"].as_str().map(str::to_string).unwrap_or_else(|| {
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).expect("the OS has randomness");
            bytes.iter().map(|b| format!("{b:02x}")).collect()
        });
        service.insert("admin_token".into(), json!(token));
    }
    if let Some(old) = before["artifacts"].as_object() {
        let artifacts = service.entry("artifacts").or_insert_with(|| json!({}));
        if let Some(new) = artifacts.as_object_mut() {
            for (platform, path) in old {
                new.entry(platform.clone()).or_insert_with(|| path.clone());
            }
        }
    }
}

async fn remove_service(State(ui): State<Arc<Ui>>, headers: HeaderMap, Json(edit): Json<Edit>) -> Response {
    let _who = guard_write!(ui, headers);
    let done = tokio::task::spawn_blocking(move || client::remove_service(&ui.fleet_path, &edit.name)).await;
    edited(done)
}

fn edited(done: Result<Result<(), String>, tokio::task::JoinError>) -> Response {
    match done {
        Ok(Ok(())) => Json(json!({ "ok": true })).into_response(),
        Ok(Err(why)) => refuse(StatusCode::BAD_REQUEST, &why),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "the edit failed"),
    }
}

/// Status and jobs, pushed every couple of seconds as server-sent events,
/// so the page does not poll - and with three seconds' patience per
/// machine, so an unreachable one is said so within that. `EventSource`
/// cannot send a header, so the token comes as `?token=`.
#[derive(Deserialize)]
struct EventsQuery {
    token: Option<String>,
}

async fn events(State(ui): State<Arc<Ui>>, headers: HeaderMap, Query(q): Query<EventsQuery>) -> Response {
    let by_query = user_of(&ui, q.token.as_deref().unwrap_or("")).is_some();
    if !by_query && who(&ui, &headers).is_none() {
        return refuse(StatusCode::UNAUTHORIZED, "the token is missing or wrong");
    }
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
    let mut stopping = ui.stopping.clone();
    tokio::spawn(async move {
        loop {
            if *stopping.borrow() {
                break;
            }
            let fleet_path = ui.fleet_path.clone();
            let status = tokio::task::spawn_blocking(move || {
                let fleet = Fleet::load(&fleet_path)?;
                Ok::<_, String>(
                    client::status_within(&fleet, Duration::from_secs(3))
                        .into_iter()
                        .map(|(machine, answer)| match answer {
                            Ok(status) => json!({ "machine": machine, "status": status }),
                            Err(why) => json!({ "machine": machine, "error": why }),
                        })
                        .collect::<Vec<Value>>(),
                )
            })
            .await
            .unwrap_or_else(|_| Err("the status check failed".into()));
            let payload = match status {
                Ok(machines) => json!({ "machines": machines, "jobs": job_rows(&ui) }),
                Err(why) => json!({ "error": why, "jobs": job_rows(&ui) }),
            };
            if tx.send(format!("data: {payload}\n\n")).await.is_err() {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                _ = stopping.changed() => break,
            }
        }
    });
    let body = axum::body::Body::from_stream(Pushed(rx));
    (
        [(header::CONTENT_TYPE, "text/event-stream"), (header::CACHE_CONTROL, "no-cache")],
        body,
    )
        .into_response()
}

/// A receiver as a stream of body chunks.
struct Pushed(tokio::sync::mpsc::Receiver<String>);

impl futures_util::Stream for Pushed {
    type Item = Result<String, std::convert::Infallible>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0.poll_recv(cx).map(|chunk| chunk.map(Ok))
    }
}

#[derive(Deserialize)]
struct LogQuery {
    service: String,
    machine: String,
    lines: Option<usize>,
    grep: Option<String>,
}

async fn logs(State(ui): State<Arc<Ui>>, headers: HeaderMap, Query(q): Query<LogQuery>) -> Response {
    guard!(ui, headers);
    let text = tokio::task::spawn_blocking(move || {
        let fleet = Fleet::load(&ui.fleet_path)?;
        client::logs_matching(&fleet, &q.service, &q.machine, q.lines.unwrap_or(200), q.grep.as_deref().unwrap_or(""))
    })
    .await;
    match text {
        Ok(Ok(text)) => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response(),
        Ok(Err(why)) => refuse(StatusCode::BAD_GATEWAY, &why),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "could not fetch the log"),
    }
}

/// Sample every machine every few seconds, whether or not anybody is looking:
/// the charts are of what happened, not of what happened while the page was
/// open.
async fn sample_forever(ui: Arc<Ui>) {
    let mut stopping = ui.stopping.clone();
    loop {
        let fleet_path = ui.fleet_path.clone();
        let answers = tokio::task::spawn_blocking(move || {
            Fleet::load(&fleet_path)
                .map(|fleet| (client::status_within(&fleet, Duration::from_secs(3)), fleet.alert_webhook.clone()))
        })
        .await;
        if let Ok(Ok((answers, webhook))) = answers {
            let sample = super::history::sample(unix_now(), &answers);
            let change = ui.alerts.lock().unwrap().check(&sample);
            ui.history.lock().unwrap().push(sample);
            if !change.is_empty() {
                for line in change.started.iter().chain(&change.happened).chain(&change.ended) {
                    log::warn!("[console] {line}");
                }
                if let Some(url) = webhook.or_else(|| std::env::var("JENGINE_ALERT_WEBHOOK").ok()).filter(|u| !u.is_empty()) {
                    super::alerts::send(&url, &change);
                }
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(super::history::INTERVAL)) => {}
            _ = stopping.changed() => break,
        }
    }
}

#[derive(Deserialize)]
struct HistoryQuery {
    range: Option<String>,
}

/// `api/history?range=hour|day`: the samples the charts are drawn from.
async fn history(State(ui): State<Arc<Ui>>, headers: HeaderMap, Query(q): Query<HistoryQuery>) -> Response {
    guard!(ui, headers);
    Json(ui.history.lock().unwrap().range(q.range.as_deref().unwrap_or("hour"))).into_response()
}

/// `api/alerts`: what the console's samples say is wrong now.
async fn alerts(State(ui): State<Arc<Ui>>, headers: HeaderMap) -> Response {
    guard!(ui, headers);
    Json(json!({ "on": ui.alerts.lock().unwrap().on() })).into_response()
}

/// A UI started with [`start`].
pub struct RunningUi {
    pub addr: SocketAddr,
    secure: bool,
    token: String,
    shutdown: watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RunningUi {
    pub fn url(&self) -> String {
        format!("{}://{}", if self.secure { "https" } else { "http" }, self.addr)
    }

    /// What every `/api` call must present as `Authorization: Bearer ..`.
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn stop(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for RunningUi {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

/// Serve the fleet's web interface on a thread of its own.
///
/// With no `token`, one is made up and written into the page - which is only
/// allowed on loopback, where nobody else can ask for the page.
pub fn start(addr: &str, fleet_path: impl Into<PathBuf>, token: Option<&str>) -> std::io::Result<RunningUi> {
    start_with(addr, fleet_path, token, None)
}

/// [`start`], over HTTPS when given a certificate - for a page that is
/// hosted for a team rather than opened on one's own machine.
pub fn start_with(
    addr: &str,
    fleet_path: impl Into<PathBuf>,
    token: Option<&str>,
    tls: Option<crate::listen::Tls>,
) -> std::io::Result<RunningUi> {
    start_inner(addr, fleet_path.into(), token, tls, None, Options::default())
}

/// The console a host serves behind its gateway: the same page over
/// `<dir>/fleet.json`, with its jobs kept in `<dir>/jobs.json` across
/// restarts and builds uploaded from the page into `<dir>/builds/`. Always
/// asks for `token`: it is reached from the network through the gateway.
/// People with tokens of their own are in `<dir>/users.json`. A token that
/// crossed the network in plain text is refused unless `allow_plain`.
pub fn start_hosted(addr: &str, dir: &Path, token: &str) -> std::io::Result<RunningUi> {
    start_hosted_with(addr, dir, token, false)
}

/// [`start_hosted`], saying whether a plain network is trusted.
pub fn start_hosted_with(addr: &str, dir: &Path, token: &str, allow_plain: bool) -> std::io::Result<RunningUi> {
    if token.len() < 8 {
        return Err(std::io::Error::other("a console needs a token of at least 8 characters: it deploys programs"));
    }
    let options = Options { users: Some(dir.join("users.json")), allow_plain, jobs: None };
    start_inner(addr, dir.join("fleet.json"), Some(token), None, Some(dir.to_path_buf()), options)
}

/// What else a page is started with.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// A users file: people with tokens of their own, and their roles.
    pub users: Option<PathBuf>,
    /// Take a token that crossed the network in plain text.
    pub allow_plain: bool,
    /// Keep the jobs in this file across restarts - so two people's pages
    /// over one fleet file see each other's deploys (`--jobs`).
    pub jobs: Option<PathBuf>,
}

/// [`start_with`], with people of its own and a word on the network.
pub fn start_options(
    addr: &str,
    fleet_path: impl Into<PathBuf>,
    token: Option<&str>,
    tls: Option<crate::listen::Tls>,
    options: Options,
) -> std::io::Result<RunningUi> {
    start_inner(addr, fleet_path.into(), token, tls, None, options)
}

/// The jobs a page kept, with any it was in the middle of marked as cut
/// short - a restart ends the thread that ran them.
fn load_jobs(file: &Path) -> VecDeque<Job> {
    let mut jobs: VecDeque<Job> = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    for job in jobs.iter_mut().filter(|j| j.state == "running") {
        job.state = "failed".into();
        job.error = Some("the console stopped while this was running; check the machine's status".into());
    }
    jobs
}

fn start_inner(
    addr: &str,
    fleet_path: PathBuf,
    token: Option<&str>,
    tls: Option<crate::listen::Tls>,
    hosted: Option<PathBuf>,
    options: Options,
) -> std::io::Result<RunningUi> {
    let secure = tls.is_some();
    let listener = std::net::TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let (token, embed) = match token.filter(|t| !t.is_empty()) {
        Some(token) => (token.to_string(), false),
        None if addr.ip().is_loopback() => {
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).expect("the OS has randomness");
            (bytes.iter().map(|b| format!("{b:02x}")).collect(), true)
        }
        None => {
            return Err(std::io::Error::other(
                "listening beyond this machine needs --token: whoever reaches this page can deploy",
            ));
        }
    };
    let (shutdown, mut shutdown_rx) = watch::channel(false);
    let jobs_file = options.jobs.clone().or_else(|| hosted.as_ref().map(|dir| dir.join("jobs.json")));
    let kept = jobs_file.as_deref().map(load_jobs).unwrap_or_default();
    let hosted_dir = hosted.clone();
    let last_job = kept.iter().map(|j| j.id).max().unwrap_or(0);
    let ui = Arc::new(Ui {
        fleet_path,
        hosted,
        token: token.clone(),
        embed,
        users: options.users,
        jobs_file,
        secure,
        allow_plain: options.allow_plain,
        jobs: Mutex::new(kept),
        next_job: AtomicU64::new(last_job),
        hashes: Mutex::new(HashMap::new()),
        stopping: shutdown_rx.clone(),
        history: Mutex::new(super::history::History::new(hosted_dir.map(|d| d.join("history.json")))),
        alerts: Mutex::new(super::alerts::Alerts::default()),
    });
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let thread = std::thread::spawn(move || {
        runtime.block_on(async move {
            tokio::spawn(sample_forever(ui.clone()));
            let router = Router::new()
                .route("/", get(page))
                .route("/api/history", get(history))
                .route("/api/alerts", get(alerts))
                .route("/api/fleet", get(fleet))
                .route("/api/status", get(status))
                .route("/api/jobs", get(jobs))
                .route("/api/logs", get(logs))
                .route("/api/deploy", post(deploy))
                .route("/api/act", post(act))
                .route("/api/events", get(events))
                .route("/api/fleet/machine", axum::routing::put(put_machine).delete(remove_machine))
                .route("/api/fleet/service", axum::routing::put(put_service).delete(remove_service))
                .route(
                    "/api/build/{service}/{platform}",
                    axum::routing::put(upload_build).layer(axum::extract::DefaultBodyLimit::max(MAX_BUILD_BYTES)),
                )
                .route("/dash/{machine}/{service}/{*path}", axum::routing::any(dashboard))
                .layer(axum::middleware::from_fn_with_state(ui.clone(), wire_guard))
                .with_state(ui);
            let stop = async move {
                let _ = shutdown_rx.changed().await;
            };
            if let Err(e) = crate::listen::serve(listener, router, tls, stop).await {
                log::error!("[fleet ui] {e}");
            }
        });
    });
    Ok(RunningUi { addr, secure, token, shutdown, thread: Some(thread) })
}

/// Serve the web interface until Ctrl+C.
pub fn run(
    addr: &str,
    fleet_path: impl Into<PathBuf>,
    token: Option<&str>,
    tls: Option<crate::listen::Tls>,
) -> std::io::Result<()> {
    run_options(addr, fleet_path, token, tls, Options::default())
}

/// [`run`], with people of its own and a word on the network.
pub fn run_options(
    addr: &str,
    fleet_path: impl Into<PathBuf>,
    token: Option<&str>,
    tls: Option<crate::listen::Tls>,
    options: Options,
) -> std::io::Result<()> {
    let ui = start_options(addr, fleet_path, token, tls, options)?;
    eprintln!("jengine-fleet ui: open {}", ui.url());
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(async {
        let _ = tokio::signal::ctrl_c().await;
    });
    ui.stop();
    Ok(())
}
