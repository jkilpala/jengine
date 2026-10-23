//! A project's test servers, from the editor (or any tool): build and run the
//! game's server locally, ask it how it is, and deploy it to machines with
//! `jengine-server`'s fleet.
//!
//! A project names its server in `project.jengine`:
//!
//! ```json
//! "server": {
//!   "package": "my-game-server",
//!   "address": "127.0.0.1:9001",
//!   "args": ["{address}", "my-game.db", "1"],
//!   "fleet": "server/fleet.json",
//!   "service": "my-game"
//! }
//! ```
//!
//! - `package` is a crate in the engine's workspace; `bin` (default: the
//!   package's name) is the executable it builds.
//! - `args` are handed to it, with `{address}` and `{port}` filled in. It runs
//!   in `data_dir` (default `.jengine/servers/local/` in the project), which
//!   is where a database it keeps ends up.
//! - `fleet` is the fleet file deploys go through (see
//!   [`jengine_server::fleet::client`]) and `service` the service in it.
//!
//! **Secrets are the project's, not the file's.** The server's admin token
//! (what lets the editor see its rooms and stop it cleanly) and the local
//! test agent's token are made on first use and kept in the OS keychain
//! ([`crate::secret_store`]), keyed by the project's path. Only where no
//! keychain answers (a headless Linux box, CI, `JENGINE_SECRETS=file`) are
//! they kept in `.jengine/servers/secrets.json` (gitignored); a file from
//! before the keychain is moved into it on first read. A fleet file names
//! its tokens by environment variable (`token_env`, `admin_token_env`) so it
//! can be committed; the tools here fill the tokens they hold into the
//! loaded fleet instead of setting the environment.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, LazyLock, Mutex, Once};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use jengine_server::fleet::client::{self, Fleet};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::secret_store::{Keychain, SecretStore};

/// A machine name the editor's own test agent goes by in a fleet file.
pub const LOCAL_MACHINE: &str = "local";
/// The environment variable a fleet file names the local agent's token by,
/// for the command line; the editor fills the token in itself.
pub const LOCAL_AGENT_TOKEN_ENV: &str = "JENGINE_LOCAL_AGENT_TOKEN";

#[derive(Debug, Clone, Deserialize)]
struct RawConfig {
    package: String,
    #[serde(default)]
    bin: Option<String>,
    #[serde(default = "default_address")]
    address: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    data_dir: Option<String>,
    #[serde(default)]
    fleet: Option<String>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default = "default_agent_port")]
    local_agent_port: u16,
}

fn default_address() -> String {
    "127.0.0.1:9001".into()
}

fn default_agent_port() -> u16 {
    9900
}

/// A project's server, as `project.jengine` describes it, paths resolved.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ServerConfig {
    pub project: PathBuf,
    pub package: String,
    pub bin: String,
    pub address: String,
    pub args: Vec<String>,
    pub data_dir: PathBuf,
    pub fleet: Option<PathBuf>,
    pub service: String,
    pub local_agent_port: u16,
}

impl ServerConfig {
    /// The `server` block of a project, or `None` if it has none.
    pub fn load(project: &Path) -> Result<Option<ServerConfig>> {
        let file = project.join("project.jengine");
        let text = std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
        let doc: Value = serde_json::from_str(&text).with_context(|| format!("parsing {}", file.display()))?;
        let Some(block) = doc.get("server").filter(|v| !v.is_null()) else { return Ok(None) };
        let raw: RawConfig =
            serde_json::from_value(block.clone()).context("the project's \"server\" block does not fit")?;
        let bin = raw.bin.unwrap_or_else(|| raw.package.clone());
        Ok(Some(ServerConfig {
            project: project.to_path_buf(),
            data_dir: project.join(raw.data_dir.as_deref().unwrap_or(".jengine/servers/local")),
            fleet: raw.fleet.map(|f| project.join(f)),
            service: raw.service.unwrap_or_else(|| bin.clone()),
            package: raw.package,
            bin,
            address: raw.address,
            args: raw.args,
            local_agent_port: raw.local_agent_port,
        }))
    }

    pub fn port(&self) -> u16 {
        self.address.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(9001)
    }

    /// The arguments with `{address}` and `{port}` filled in.
    pub fn resolved_args(&self) -> Vec<String> {
        self.args
            .iter()
            .map(|a| a.replace("{address}", &self.address).replace("{port}", &self.port().to_string()))
            .collect()
    }

    /// Where `cargo build --release -p <package>` leaves the server.
    pub fn executable(&self, engine: &Path) -> PathBuf {
        crate::cargo_target_dir(engine).join("release").join(format!("{}{}", self.bin, std::env::consts::EXE_SUFFIX))
    }
}

/// The tokens a project's test servers are run with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Secrets {
    pub admin_token: String,
    pub agent_token: String,
}

/// 32 hex characters from the process's random hasher keys - plenty for a
/// test server's token, with no dependency for it.
fn random_token() -> String {
    let mut out = String::new();
    for i in 0..2u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u128(Instant::now().elapsed().as_nanos());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// The names the tokens go by in a keychain.
const ADMIN_TOKEN: &str = "admin_token";
const AGENT_TOKEN: &str = "agent_token";

/// The environment variable that keeps the tokens in the file whatever the
/// machine has (`JENGINE_SECRETS=file`) - for CI, where a keychain that is
/// there but locked could stall a run.
pub const SECRETS_ENV: &str = "JENGINE_SECRETS";

/// Where the tokens were found, or put.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretsKept {
    /// In the store (the keychain).
    Store,
    /// Moved from `secrets.json` into the store just now; the file is gone.
    Migrated,
    /// In `secrets.json`: the store could not be used.
    File,
}

/// Tokens already read this run, by project - the panel asks for them on
/// every command, and a keychain may ask the person each time.
static CACHE: LazyLock<Mutex<HashMap<PathBuf, Secrets>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

impl Secrets {
    /// The file the tokens were kept in before the keychain, and still are
    /// where no keychain answers.
    pub fn file(project: &Path) -> PathBuf {
        project.join(".jengine").join("servers").join("secrets.json")
    }

    /// The project's tokens, from the OS keychain (made the first time),
    /// or from [`Secrets::file`] where there is none - see
    /// [`for_project_with`](Self::for_project_with).
    pub fn for_project(project: &Path) -> Result<Secrets> {
        let key = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
        if let Some(s) = CACHE.lock().unwrap_or_else(|p| p.into_inner()).get(&key) {
            return Ok(s.clone());
        }
        let forced_file = std::env::var(SECRETS_ENV).is_ok_and(|v| v == "file");
        let no_keychain = crate::secret_store::MemoryStore::unavailable();
        let store: &dyn SecretStore = if forced_file { &no_keychain } else { &Keychain };
        let (secrets, _) = Self::for_project_with(project, store)?;
        CACHE.lock().unwrap_or_else(|p| p.into_inner()).insert(key, secrets.clone());
        Ok(secrets)
    }

    /// The project's tokens from `store`, keyed by the project's canonical
    /// path:
    ///
    /// - in the store: those;
    /// - not there, but in an old `secrets.json`: written into the store,
    ///   read back, and only then is the file deleted (**migrated**);
    /// - in neither: made, and written into the store;
    /// - the store erring (no keychain): the file, read or made - logged
    ///   once a run.
    pub fn for_project_with(project: &Path, store: &dyn SecretStore) -> Result<(Secrets, SecretsKept)> {
        let file = Self::file(project);
        let id = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
        let id = id.to_string_lossy();
        // `\\?\C:\...` on Windows: one project whichever way it is named.
        let id = id.strip_prefix(r"\\?\").unwrap_or(&id).to_string();
        let from_file = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<Secrets>(&t).ok());

        let stored = store.get(&id, ADMIN_TOKEN).and_then(|a| Ok((a, store.get(&id, AGENT_TOKEN)?)));
        let failure = match stored {
            Ok((Some(admin_token), Some(agent_token))) => {
                let secrets = Secrets { admin_token, agent_token };
                // A file left by a migration that stopped short of deleting it.
                if from_file.as_ref() == Some(&secrets) {
                    let _ = std::fs::remove_file(&file);
                }
                return Ok((secrets, SecretsKept::Store));
            }
            Ok(_) => {
                let migrating = from_file.is_some();
                let secrets = from_file.clone().unwrap_or_else(|| Secrets { admin_token: random_token(), agent_token: random_token() });
                match Self::store_and_confirm(store, &id, &secrets) {
                    Ok(()) if migrating => {
                        std::fs::remove_file(&file)
                            .with_context(|| format!("removing {} after moving it into {}", file.display(), store.describe()))?;
                        log::info!("[servers] moved {} into {}", file.display(), store.describe());
                        return Ok((secrets, SecretsKept::Migrated));
                    }
                    Ok(()) => return Ok((secrets, SecretsKept::Store)),
                    Err(e) => e,
                }
            }
            Err(e) => e,
        };
        static WARNED: Once = Once::new();
        WARNED.call_once(|| {
            log::warn!("[servers] {} is not usable ({failure:#}) - the test servers' tokens stay in {}", store.describe(), file.display());
        });
        if let Some(s) = from_file {
            return Ok((s, SecretsKept::File));
        }
        let secrets = Secrets { admin_token: random_token(), agent_token: random_token() };
        std::fs::create_dir_all(file.parent().unwrap())?;
        std::fs::write(&file, serde_json::to_string_pretty(&secrets)?)?;
        Ok((secrets, SecretsKept::File))
    }

    /// Write both tokens and read them back: a keychain that says yes and
    /// keeps nothing must not cost the file.
    fn store_and_confirm(store: &dyn SecretStore, id: &str, s: &Secrets) -> Result<()> {
        store.set(id, ADMIN_TOKEN, &s.admin_token)?;
        store.set(id, AGENT_TOKEN, &s.agent_token)?;
        let back = (store.get(id, ADMIN_TOKEN)?, store.get(id, AGENT_TOKEN)?);
        if back != (Some(s.admin_token.clone()), Some(s.agent_token.clone())) {
            bail!("it took the tokens and gave back something else");
        }
        Ok(())
    }
}

/// `cargo build --release -p <package>` in the engine's workspace, its output
/// streamed through [`crate::progress`].
pub fn build(engine: &Path, cfg: &ServerConfig) -> Result<PathBuf> {
    crate::progress::step(&format!("Building {} (release)", cfg.package));
    let mut cmd = Command::new("cargo");
    cmd.current_dir(engine).args(["build", "--release", "-p", &cfg.package, "--bin", &cfg.bin]);
    let status = crate::progress::run_streaming(&mut cmd, "cargo build")?;
    if !status.success() {
        bail!("cargo build -p {} failed", cfg.package);
    }
    let exe = cfg.executable(engine);
    if !exe.exists() {
        bail!("the build finished but {} is not there", exe.display());
    }
    Ok(exe)
}

/// The project's server, running on this machine.
pub struct LocalServer {
    child: Child,
    pub address: String,
    token: String,
    pub started: Instant,
}

/// Where a running server's output goes, a line at a time.
pub type LineSink = Arc<dyn Fn(&str) + Send + Sync>;

impl LocalServer {
    /// Start `exe` with the project's arguments in its data directory, the
    /// admin token in `JENGINE_ADMIN_TOKEN`, and every line it prints handed
    /// to `on_line`.
    pub fn spawn(exe: &Path, cfg: &ServerConfig, token: &str, on_line: LineSink) -> Result<LocalServer> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        let mut child = Command::new(exe)
            .args(cfg.resolved_args())
            .current_dir(&cfg.data_dir)
            .env("JENGINE_ADMIN_TOKEN", token)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {}", exe.display()))?;
        // It ends with the editor, however the editor ends: a crash used to
        // leave it running, holding its port against the next Start.
        jengine_server::fleet::tied::tie(&child);
        let out = child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>);
        let err = child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>);
        for stream in [out, err].into_iter().flatten() {
            let sink = on_line.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    sink(&line);
                }
            });
        }
        Ok(LocalServer { child, address: cfg.address.clone(), token: token.to_string(), started: Instant::now() })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Whether the process is still alive; its exit, if it has gone.
    pub fn exited(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.to_string()),
            Ok(None) => None,
            Err(e) => Some(e.to_string()),
        }
    }

    /// Stop it the way a deploy does: ask it to shut down (everyone saved),
    /// wait up to `grace`, then kill it. Says which happened.
    pub fn stop(mut self, grace: Duration) -> String {
        if self.exited().is_some() {
            return "it had already stopped".into();
        }
        let asked = admin_post(&self.address, &self.token, "/admin/api/shutdown", &json!({})).is_ok();
        let deadline = Instant::now() + grace;
        while asked && Instant::now() < deadline {
            if self.exited().is_some() {
                return "shut down cleanly".into();
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if asked { "did not shut down in time - killed".into() } else { "killed (its admin API did not answer)".into() }
    }
}

impl Drop for LocalServer {
    /// A server the editor started does not outlive the editor.
    fn drop(&mut self) {
        if self.exited().is_none() {
            let _ = self.child.kill();
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).build()
}

/// How a server answers right now.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Probe {
    /// `/healthz` said ok.
    pub up: bool,
    /// How long `/healthz` took, in milliseconds.
    pub ms: Option<f64>,
    /// `/admin/api/overview`, when the token is right: rooms, players, uptime.
    pub overview: Option<Value>,
    pub error: Option<String>,
}

/// Ask a server at `address` (`host:port`) its health, and with `token` its
/// overview.
pub fn probe(address: &str, token: Option<&str>) -> Probe {
    let base = format!("http://{address}");
    let started = Instant::now();
    let health = agent().get(&format!("{base}/healthz")).call();
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut probe = match health {
        Ok(_) => Probe { up: true, ms: Some(ms), ..Default::default() },
        // Draining answers 503, and is still a server that is there.
        Err(ureq::Error::Status(code, r)) => Probe {
            up: false,
            ms: Some(ms),
            error: Some(format!("{code}: {}", r.into_string().unwrap_or_default())),
            ..Default::default()
        },
        Err(e) => return Probe { error: Some(e.to_string()), ..Default::default() },
    };
    if let Some(token) = token {
        match agent().get(&format!("{base}/admin/api/overview")).set("Authorization", &format!("Bearer {token}")).call()
        {
            Ok(r) => probe.overview = r.into_json().ok(),
            Err(e) => probe.error = Some(format!("overview: {e}")),
        }
    }
    probe
}

/// A POST to a server's admin API (`/admin/api/notice`, `/admin/api/kick` ...).
pub fn admin_post(address: &str, token: &str, path: &str, body: &Value) -> Result<Value> {
    let answer = agent()
        .post(&format!("http://{address}{path}"))
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(body.clone())
        .map_err(|e| anyhow!("{path}: {e}"))?;
    Ok(answer.into_json().unwrap_or(Value::Null))
}

// ── Deploying ───────────────────────────────────────────────────────────────

/// A fleet file for the project's service: the local test machine, the
/// executable [`build`] makes for this platform, and the same arguments as
/// the local server - on port 9101, so it does not fight the local one.
pub fn default_fleet(engine: &Path, cfg: &ServerConfig, fleet_path: &Path) -> Value {
    let exe = cfg.executable(engine);
    let base = fleet_path.parent().unwrap_or(Path::new("."));
    let artifact = relative(base, &exe).unwrap_or(exe).to_string_lossy().replace('\\', "/");
    let args: Vec<String> = cfg.args.iter().map(|a| a.replace("{address}", "0.0.0.0:{port}")).collect();
    json!({
        "machines": {
            LOCAL_MACHINE: { "url": format!("http://127.0.0.1:{}", cfg.local_agent_port), "token_env": LOCAL_AGENT_TOKEN_ENV }
        },
        "services": {
            cfg.service.clone(): {
                "machines": [LOCAL_MACHINE],
                "artifacts": { jengine_server::fleet::platform(): artifact },
                "args": args,
                "port": 9101,
                "admin_token_env": format!("{}_ADMIN_TOKEN", cfg.service.to_uppercase().replace('-', "_")),
                "drain_secs": 5,
            }
        }
    })
}

/// `to` relative to the directory `from`, for paths on one drive.
fn relative(from: &Path, to: &Path) -> Option<PathBuf> {
    let from_abs = std::path::absolute(from).ok()?;
    let from: Vec<_> = from_abs.components().collect();
    let to_abs = std::path::absolute(to).ok()?;
    let to: Vec<_> = to_abs.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for c in &to[common..] {
        out.push(c.as_os_str());
    }
    Some(out)
}

/// The project's fleet, with the tokens the project holds filled in: the
/// local agent's, and the server's admin token wherever a service names it
/// by an environment variable that is not set.
pub fn load_fleet(cfg: &ServerConfig, secrets: &Secrets) -> Result<Fleet> {
    let path = cfg.fleet.as_ref().ok_or_else(|| anyhow!("the project's server block names no \"fleet\" file"))?;
    let mut fleet = Fleet::load(path).map_err(|e| anyhow!(e))?;
    for (name, machine) in fleet.machines.iter_mut() {
        let named_local = machine.token_env.as_deref() == Some(LOCAL_AGENT_TOKEN_ENV);
        if machine.token.is_none() && (name == LOCAL_MACHINE || named_local) && std::env::var(LOCAL_AGENT_TOKEN_ENV).is_err() {
            machine.token = Some(secrets.agent_token.clone());
            machine.token_env = None;
        }
    }
    for def in fleet.services.values_mut() {
        if let Some(var) = &def.admin_token_env
            && std::env::var(var).is_err()
        {
            def.spec.admin_token = Some(secrets.admin_token.clone());
            def.admin_token_env = None;
        }
    }
    Ok(fleet)
}

/// Every machine's answer, and for each service that has a port, how the
/// server itself answers there - the connection a player would make.
pub fn fleet_status(cfg: &ServerConfig, secrets: &Secrets) -> Result<Value> {
    let fleet = load_fleet(cfg, secrets)?;
    let answers = client::status_within(&fleet, Duration::from_secs(2));
    let mut machines = Vec::new();
    for (name, answer) in answers {
        let url = fleet.machines.get(&name).map(|m| m.url.clone()).unwrap_or_default();
        let console = fleet.machines.get(&name).and_then(|m| m.console.clone());
        let host = host_of(&url);
        let mut row = json!({ "name": name, "url": url, "console": console });
        match answer {
            Ok(mut info) => {
                if let Some(services) = info["services"].as_array_mut() {
                    for svc in services {
                        if let Some(port) = svc["port"].as_u64() {
                            let token = fleet
                                .services
                                .get(svc["name"].as_str().unwrap_or(""))
                                .and_then(|d| d.spec.admin_token.clone());
                            let p = probe(&format!("{host}:{port}"), token.as_deref());
                            svc["probe"] = serde_json::to_value(p).unwrap_or(Value::Null);
                        }
                    }
                }
                row["info"] = info;
            }
            Err(e) => row["error"] = Value::String(e),
        }
        machines.push(row);
    }
    Ok(json!({ "service": cfg.service, "machines": machines }))
}

fn host_of(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let authority = rest.split('/').next().unwrap_or(rest);
    authority.rsplit_once(':').map(|(h, _)| h.to_string()).unwrap_or_else(|| authority.to_string())
}

/// Build the release executable and deploy it to the service's machines (or
/// one of them), saying each step.
pub fn deploy(engine: &Path, cfg: &ServerConfig, secrets: &Secrets, only: Option<&str>, say: &mut dyn FnMut(&str)) -> Result<()> {
    build(engine, cfg)?;
    let fleet = load_fleet(cfg, secrets)?;
    say(&format!("deploying {} ...", cfg.service));
    client::deploy(&fleet, &cfg.service, only, say).map_err(|e| anyhow!(e))?;
    say("deployed");
    Ok(())
}

/// `start`, `stop`, `restart` or `rollback` the service.
pub fn act(cfg: &ServerConfig, secrets: &Secrets, action: &str, only: Option<&str>, say: &mut dyn FnMut(&str)) -> Result<()> {
    if !matches!(action, "start" | "stop" | "restart" | "rollback") {
        bail!("not something a service can be told: {action}");
    }
    let fleet = load_fleet(cfg, secrets)?;
    client::act(&fleet, &cfg.service, action, only, say).map_err(|e| anyhow!(e))
}

pub fn logs(cfg: &ServerConfig, secrets: &Secrets, machine: &str, lines: usize) -> Result<String> {
    let fleet = load_fleet(cfg, secrets)?;
    client::logs(&fleet, &cfg.service, machine, lines).map_err(|e| anyhow!(e))
}

/// A fleet agent on this machine - somewhere to deploy to without a server
/// machine - with its releases and services under the project's
/// `.jengine/servers/agent/`.
pub fn start_local_agent(cfg: &ServerConfig, secrets: &Secrets) -> Result<jengine_server::fleet::agent::RunningAgent> {
    let dir = cfg.project.join(".jengine").join("servers").join("agent");
    jengine_server::fleet::agent::start(&format!("127.0.0.1:{}", cfg.local_agent_port), dir, &secrets.agent_token)
        .with_context(|| format!("starting a fleet agent on port {}", cfg.local_agent_port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_store::MemoryStore;

    fn project(server: Value) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jengine-servers-{}-{}", std::process::id(), random_token()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("project.jengine"), json!({ "name": "t", "server": server }).to_string()).unwrap();
        dir
    }

    #[test]
    fn a_server_block_is_read_with_its_defaults() {
        let dir = project(json!({ "package": "my-game-server", "args": ["{address}", "x.db", ":{port}"], "fleet": "server/fleet.json" }));
        let cfg = ServerConfig::load(&dir).unwrap().unwrap();
        assert_eq!((cfg.bin.as_str(), cfg.service.as_str()), ("my-game-server", "my-game-server"));
        assert_eq!(cfg.resolved_args(), ["127.0.0.1:9001", "x.db", ":9001"]);
        assert_eq!(cfg.data_dir, dir.join(".jengine/servers/local"));
        assert_eq!(cfg.fleet, Some(dir.join("server/fleet.json")));
        // A project with no block has no server.
        std::fs::write(dir.join("project.jengine"), r#"{ "name": "t" }"#).unwrap();
        assert_eq!(ServerConfig::load(&dir).unwrap(), None);
    }

    #[test]
    fn secrets_are_made_once_and_kept_in_the_store() {
        let dir = project(json!({ "package": "p" }));
        let store = MemoryStore::new();
        let (first, kept) = Secrets::for_project_with(&dir, &store).unwrap();
        assert_eq!(kept, SecretsKept::Store);
        assert_eq!(first.admin_token.len(), 32);
        assert_ne!(first.admin_token, first.agent_token);
        assert_eq!(store.len(), 2, "both tokens, one entry each");
        assert!(!Secrets::file(&dir).exists(), "nothing written to the project");
        // A token round-trips: asked again, the store gives back the same.
        assert_eq!(Secrets::for_project_with(&dir, &store).unwrap(), (first, SecretsKept::Store));
    }

    #[test]
    fn an_old_secrets_file_moves_into_the_store_and_is_deleted() {
        let dir = project(json!({ "package": "p" }));
        let old = Secrets { admin_token: "a".repeat(32), agent_token: "b".repeat(32) };
        std::fs::create_dir_all(Secrets::file(&dir).parent().unwrap()).unwrap();
        std::fs::write(Secrets::file(&dir), serde_json::to_string(&old).unwrap()).unwrap();
        let store = MemoryStore::new();
        assert_eq!(Secrets::for_project_with(&dir, &store).unwrap(), (old.clone(), SecretsKept::Migrated));
        assert!(!Secrets::file(&dir).exists(), "the file goes once the store has the tokens");
        assert_eq!(Secrets::for_project_with(&dir, &store).unwrap(), (old.clone(), SecretsKept::Store), "the same tokens: running servers keep working");

        // A store that says yes and keeps nothing: the file stays.
        let dir = project(json!({ "package": "p" }));
        std::fs::create_dir_all(Secrets::file(&dir).parent().unwrap()).unwrap();
        std::fs::write(Secrets::file(&dir), serde_json::to_string(&old).unwrap()).unwrap();
        assert_eq!(Secrets::for_project_with(&dir, &MemoryStore::write_only()).unwrap(), (old, SecretsKept::File));
        assert!(Secrets::file(&dir).exists(), "the write was not confirmed, so the file is all there is");
    }

    #[test]
    fn without_a_keychain_the_file_is_used() {
        let dir = project(json!({ "package": "p" }));
        let none = MemoryStore::unavailable();
        let (first, kept) = Secrets::for_project_with(&dir, &none).unwrap();
        assert_eq!(kept, SecretsKept::File);
        assert!(Secrets::file(&dir).exists());
        assert_eq!(Secrets::for_project_with(&dir, &none).unwrap(), (first.clone(), SecretsKept::File), "and kept there");
        // The keychain arrives later (the same machine, unlocked): moved in.
        let store = MemoryStore::new();
        assert_eq!(Secrets::for_project_with(&dir, &store).unwrap(), (first, SecretsKept::Migrated));
        assert!(!Secrets::file(&dir).exists());
    }

    /// A secret is named by the project's path and the token's name, so
    /// two projects never share one.
    #[test]
    fn two_projects_have_their_own_tokens() {
        let (a, b) = (project(json!({ "package": "p" })), project(json!({ "package": "p" })));
        let store = MemoryStore::new();
        let (sa, _) = Secrets::for_project_with(&a, &store).unwrap();
        let (sb, _) = Secrets::for_project_with(&b, &store).unwrap();
        assert_ne!(sa, sb);
        assert_eq!(store.len(), 4);
    }

    /// The default fleet loads, and the tokens it names by environment
    /// variable are filled from the project's secrets.
    #[test]
    fn the_default_fleet_carries_the_projects_tokens() {
        let dir = project(json!({ "package": "my-game-server", "service": "my-game", "args": ["{address}", "u.db"], "fleet": "server/fleet.json" }));
        let cfg = ServerConfig::load(&dir).unwrap().unwrap();
        let engine = dir.join("engine");
        let fleet_path = cfg.fleet.clone().unwrap();
        std::fs::create_dir_all(fleet_path.parent().unwrap()).unwrap();
        std::fs::write(&fleet_path, default_fleet(&engine, &cfg, &fleet_path).to_string()).unwrap();
        let (secrets, _) = Secrets::for_project_with(&dir, &MemoryStore::new()).unwrap();
        let fleet = load_fleet(&cfg, &secrets).unwrap();
        assert_eq!(fleet.machines[LOCAL_MACHINE].token.as_deref(), Some(secrets.agent_token.as_str()));
        let def = &fleet.services["my-game"];
        assert_eq!(def.spec.admin_token.as_deref(), Some(secrets.admin_token.as_str()));
        assert_eq!(def.spec.args, ["0.0.0.0:{port}", "u.db"]);
        let artifact = def.artifacts.values().next().unwrap();
        if std::env::var_os("CARGO_TARGET_DIR").is_some_and(|v| !v.is_empty()) {
            // A target directory elsewhere (another drive) is named as it is.
            let release = crate::cargo_target_dir(&engine).join("release");
            let release = release.to_string_lossy().replace('\\', "/");
            assert!(artifact.replace('\\', "/").starts_with(&release), "{artifact} is not in {release}");
        } else {
            assert!(artifact.starts_with("../engine/target/release/my-game-server"), "{artifact}");
        }
    }

    #[test]
    fn hosts_come_out_of_agent_urls() {
        assert_eq!(host_of("http://127.0.0.1:9900"), "127.0.0.1");
        assert_eq!(host_of("https://eu-1.example.com:9900/"), "eu-1.example.com");
    }
}
