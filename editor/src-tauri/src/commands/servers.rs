//! The Servers panel: the project's test server run from the editor, asked
//! how it is, and deployed through the fleet - `jengine_builder::servers`
//! with the editor's state around it (which server is running, the local
//! agent) and its output sent to the panel.
//!
//! Every command that builds, deploys or waits is `async` for the reason
//! `build::build_windows` gives: a synchronous command runs on the thread
//! pumping WebView2 messages and freezes the editor for its length.

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use jengine_builder::servers::{self, LocalServer, Secrets, ServerConfig};
use jengine_server::fleet::agent::RunningAgent;
use serde_json::{Value, json};
use tauri::Emitter;

/// Event the panel listens on: one line, and which log it belongs in.
const SERVER_OUTPUT_EVENT: &str = "server-output";

#[derive(Clone, serde::Serialize)]
struct OutputLine<'a> {
    /// `local` (the server running here) or `fleet` (builds and deploys).
    source: &'a str,
    text: &'a str,
}

fn emit(app: &tauri::AppHandle, source: &str, text: &str) {
    let _ = app.emit(SERVER_OUTPUT_EVENT, OutputLine { source, text });
}

/// The server the editor started, and what it was started from.
struct Running {
    server: LocalServer,
    project: PathBuf,
}

static LOCAL: LazyLock<Mutex<Option<Running>>> = LazyLock::new(|| Mutex::new(None));
static AGENT: LazyLock<Mutex<Option<RunningAgent>>> = LazyLock::new(|| Mutex::new(None));
/// Why the last server stopped by itself, until the panel has seen it.
static EXITED: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));

fn config(project_path: &str) -> Result<(ServerConfig, Secrets), String> {
    let project = Path::new(project_path);
    let cfg = ServerConfig::load(project)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "This project names no server: add a \"server\" block to project.jengine.".to_string())?;
    let secrets = Secrets::for_project(project).map_err(|e| e.to_string())?;
    Ok((cfg, secrets))
}

/// Send `cargo` output to the panel's fleet log while a build runs. The
/// builder's sink is process-wide: cleared on drop, as `build.rs` does.
struct Stream;

impl Drop for Stream {
    fn drop(&mut self) {
        jengine_builder::progress::set_sink(None);
    }
}

fn stream_to(app: &tauri::AppHandle, source: &'static str) -> Stream {
    let handle = app.clone();
    jengine_builder::progress::set_sink(Some(Arc::new(move |line: &str, replaces: bool| {
        // A progress-bar redraw is noise in a server log.
        if !replaces {
            emit(&handle, source, line);
        }
    })));
    Stream
}

/// What the panel shows before anything runs: the server block, and
/// whether there is a fleet file yet.
#[tauri::command]
pub fn server_config(project_path: String) -> Result<Value, String> {
    let project = Path::new(&project_path);
    let Some(cfg) = ServerConfig::load(project).map_err(|e| e.to_string())? else {
        return Ok(Value::Null);
    };
    let fleet_exists = cfg.fleet.as_ref().is_some_and(|f| f.exists());
    Ok(json!({
        "package": cfg.package, "bin": cfg.bin, "address": cfg.address, "args": cfg.resolved_args(),
        "service": cfg.service, "fleet": cfg.fleet, "fleet_exists": fleet_exists,
        "local_agent_port": cfg.local_agent_port,
        "admin_url": format!("http://{}/admin", cfg.address),
    }))
}

/// Build the server (release) and start it here, its output streamed.
#[tauri::command(async)]
pub fn server_start(app: tauri::AppHandle, project_path: String) -> Result<String, String> {
    if LOCAL.lock().unwrap().is_some() {
        return Err("A server is already running - stop it first.".into());
    }
    let (cfg, secrets) = config(&project_path)?;
    let engine = super::build::engine_root()?;
    let exe = {
        let _stream = stream_to(&app, "local");
        servers::build(&engine, &cfg).map_err(|e| {
            let msg = format!("The server did not build: {e}");
            emit(&app, "local", &msg);
            msg
        })?
    };
    let handle = app.clone();
    let server = LocalServer::spawn(&exe, &cfg, &secrets.admin_token, Arc::new(move |line: &str| emit(&handle, "local", line)))
        .map_err(|e| e.to_string())?;
    let pid = server.pid();
    emit(&app, "local", &format!("== started {} (pid {pid}) on {}", cfg.bin, cfg.address));
    *EXITED.lock().unwrap() = None;
    *LOCAL.lock().unwrap() = Some(Running { server, project: PathBuf::from(&project_path) });
    Ok(format!("running on {}", cfg.address))
}

/// Stop the local server cleanly (it saves everyone), or kill it.
#[tauri::command(async)]
pub fn server_stop(app: tauri::AppHandle) -> Result<String, String> {
    let Some(running) = LOCAL.lock().unwrap().take() else { return Ok("nothing is running".into()) };
    let how = running.server.stop(Duration::from_secs(10));
    emit(&app, "local", &format!("== stopped: {how}"));
    Ok(how)
}

/// The local server as it is now: whether it runs, and how it answers.
#[tauri::command(async)]
pub fn server_status(project_path: String) -> Result<Value, String> {
    let (cfg, secrets) = config(&project_path)?;
    let mut guard = LOCAL.lock().unwrap();
    let mut row = json!({ "running": false, "address": cfg.address });
    if let Some(running) = guard.as_mut() {
        if let Some(exit) = running.server.exited() {
            // It stopped by itself: say so once, and forget it.
            *EXITED.lock().unwrap() = Some(exit);
            *guard = None;
        } else {
            row["running"] = json!(true);
            row["pid"] = json!(running.server.pid());
            row["uptime"] = json!(running.server.started.elapsed().as_secs_f64());
            row["mine"] = json!(running.project == Path::new(&project_path));
        }
    }
    drop(guard);
    if let Some(exit) = EXITED.lock().unwrap().take() {
        row["exited"] = json!(exit);
    }
    // Asked even when the editor did not start it: something else may be
    // running on the address (a server started from a terminal).
    row["probe"] = serde_json::to_value(servers::probe(&cfg.address, Some(&secrets.admin_token))).unwrap_or(Value::Null);
    Ok(row)
}

/// Open a server's admin dashboard in the default browser, signed in: the
/// page takes its token from `#token=` (and keeps it for the tab), so the
/// project's admin token goes in the fragment - which a browser never sends
/// to the server. `address` is a deployed server's `host:port`; without it,
/// the local one.
#[tauri::command]
pub fn server_open_dashboard(project_path: String, address: Option<String>) -> Result<(), String> {
    let (cfg, secrets) = config(&project_path)?;
    let address = address.unwrap_or(cfg.address);
    let url = format!("http://{address}/admin#token={}", secrets.admin_token);
    open_in_browser(&url)
}

/// Open a machine's console - the full management page a host serves
/// (`jengine-fleet host`, `/manage`) - named by `console` on the machine in
/// the project's fleet file. It asks for its own token: the editor holds the
/// project's test tokens, not a host's.
#[tauri::command]
pub fn fleet_open_console(project_path: String, machine: String) -> Result<(), String> {
    let (cfg, secrets) = config(&project_path)?;
    let fleet = servers::load_fleet(&cfg, &secrets).map_err(|e| e.to_string())?;
    let url = fleet
        .machines
        .get(&machine)
        .and_then(|m| m.console.clone())
        .ok_or_else(|| format!("'{machine}' has no \"console\" in the fleet file"))?;
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(format!("'{url}' is not a web address"));
    }
    open_in_browser(&url)
}

fn open_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        // `start` hands the URL to the shell's default browser; the empty
        // title argument keeps a quoted URL from being taken as the title.
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| format!("Could not open a browser: {e}"))
}

/// A notice to everyone on the local server.
#[tauri::command(async)]
pub fn server_notice(project_path: String, text: String) -> Result<(), String> {
    let (cfg, secrets) = config(&project_path)?;
    servers::admin_post(&cfg.address, &secrets.admin_token, "/admin/api/notice", &json!({ "text": text }))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Write the default fleet file (the local test machine, this platform's
/// build) if the project has none.
#[tauri::command(async)]
pub fn fleet_create(project_path: String) -> Result<String, String> {
    let (cfg, _) = config(&project_path)?;
    let path = cfg.fleet.clone().ok_or("The server block names no \"fleet\" file.")?;
    if path.exists() {
        return Err(format!("{} already exists.", path.display()));
    }
    let engine = super::build::engine_root()?;
    let doc = servers::default_fleet(&engine, &cfg, &path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Every machine in the fleet, its services, and how each answers.
#[tauri::command(async)]
pub fn fleet_status(project_path: String) -> Result<Value, String> {
    let (cfg, secrets) = config(&project_path)?;
    let mut status = servers::fleet_status(&cfg, &secrets).map_err(|e| e.to_string())?;
    status["local_agent"] = json!(AGENT.lock().unwrap().as_ref().map(|a| a.url()));
    Ok(status)
}

/// Build and deploy to every machine, or one.
#[tauri::command(async)]
pub fn fleet_deploy(app: tauri::AppHandle, project_path: String, machine: Option<String>) -> Result<(), String> {
    let (cfg, secrets) = config(&project_path)?;
    let engine = super::build::engine_root()?;
    let _stream = stream_to(&app, "fleet");
    let mut say = |line: &str| emit(&app, "fleet", line);
    servers::deploy(&engine, &cfg, &secrets, machine.as_deref(), &mut say).map_err(|e| {
        let msg = format!("Deploy failed: {e}");
        emit(&app, "fleet", &msg);
        msg
    })
}

/// `start`, `stop`, `restart` or `rollback` on every machine, or one.
#[tauri::command(async)]
pub fn fleet_act(app: tauri::AppHandle, project_path: String, action: String, machine: Option<String>) -> Result<(), String> {
    let (cfg, secrets) = config(&project_path)?;
    let mut say = |line: &str| emit(&app, "fleet", line);
    servers::act(&cfg, &secrets, &action, machine.as_deref(), &mut say).map_err(|e| {
        let msg = format!("{action} failed: {e}");
        emit(&app, "fleet", &msg);
        msg
    })
}

#[tauri::command(async)]
pub fn fleet_logs(project_path: String, machine: String, lines: Option<usize>) -> Result<String, String> {
    let (cfg, secrets) = config(&project_path)?;
    servers::logs(&cfg, &secrets, &machine, lines.unwrap_or(200)).map_err(|e| e.to_string())
}

/// Start or stop the fleet agent on this machine - the "local" machine a
/// default fleet deploys to. Stopping it stops what it runs.
#[tauri::command(async)]
pub fn fleet_local_agent(app: tauri::AppHandle, project_path: String, on: bool) -> Result<Option<String>, String> {
    let mut agent = AGENT.lock().unwrap();
    if !on {
        if let Some(a) = agent.take() {
            a.stop();
            emit(&app, "fleet", "== local test machine stopped");
        }
        return Ok(None);
    }
    if let Some(a) = agent.as_ref() {
        return Ok(Some(a.url()));
    }
    let (cfg, secrets) = config(&project_path)?;
    let started = servers::start_local_agent(&cfg, &secrets).map_err(|e| format!("{e:#}"))?;
    let url = started.url();
    emit(&app, "fleet", &format!("== local test machine listening on {url}"));
    *agent = Some(started);
    Ok(Some(url))
}

/// Called as the editor closes: nothing it started outlives it.
pub fn shutdown() {
    if let Some(running) = LOCAL.lock().unwrap().take() {
        running.server.stop(Duration::from_secs(5));
    }
    if let Some(agent) = AGENT.lock().unwrap().take() {
        agent.stop();
    }
}
