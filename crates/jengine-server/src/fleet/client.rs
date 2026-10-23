//! The other end: what `jengine-fleet deploy` does, as functions.
//!
//! A **fleet file** names the machines and says what runs where:
//!
//! ```json
//! {
//!   "machines": {
//!     "eu-1": { "url": "http://10.0.0.5:9900", "token_env": "FLEET_TOKEN" },
//!     "dev":  { "url": "http://127.0.0.1:9900", "token": "local-secret" }
//!   },
//!   "services": {
//!     "zones": {
//!       "machines": ["eu-1", "dev"],
//!       "artifacts": {
//!         "linux-x86_64":   "target/x86_64-unknown-linux-gnu/release/examples/zones",
//!         "windows-x86_64": "target/release/examples/zones.exe"
//!       },
//!       "args": ["0.0.0.0:{port}", "zones.db"],
//!       "port": 9001,
//!       "admin_token_env": "ZONES_ADMIN_TOKEN",
//!       "drain_secs": 30,
//!       "overrides": { "dev": { "port": 9101, "drain_secs": 0 } }
//!     }
//!   }
//! }
//! ```
//!
//! No secret has to be in the file: a machine's token and a service's admin
//! token can each be named by the environment variable that holds it
//! (`token_env`, `admin_token_env`) instead of written out (`token`,
//! `admin_token`), which is what lets a fleet file be committed.
//!
//! An agent reached over `https` with a certificate of its own making
//! (`jengine-fleet cert`) is told which to trust: `"ca": "eu-1.pem"`.
//!
//! Paths are relative to the fleet file. `overrides` changes any of a
//! service's settings on one machine - a different port on the box that runs
//! two things, no drain on the one nobody plays on. A token is better named by the
//! environment variable holding it (`token_env`) than written in a file that
//! might be committed.
//!
//! **A deploy goes machine by machine and stops at the first that fails** -
//! which the agent has already put back as it was - so a bad build takes down
//! one machine for a moment, not all of them.
//!
//! **A release is signed** for an agent that runs only signed releases
//! (`--release-key`): with the key in `"release_key_file"` (relative to the
//! fleet file) or `JENGINE_RELEASE_KEY_FILE`, else by the `<artifact>.sig`
//! beside the artifact that `jengine-fleet sign` wrote where the build was
//! made - so the key need not be wherever deploys are pressed from.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::{ServiceSpec, safe_name, version_of};

#[derive(Debug, Clone, Deserialize)]
pub struct Machine {
    pub url: String,
    #[serde(default)]
    pub token: Option<String>,
    /// The environment variable the token is in.
    #[serde(default)]
    pub token_env: Option<String>,
    /// For an `https` agent with a certificate of its own making: the file
    /// to trust, relative to the fleet file. Nothing else will be trusted in
    /// its place.
    #[serde(default)]
    pub ca: Option<String>,
    /// Where this machine's console is, if it is a host serving one
    /// (`http://raspberrypi.local:9000/manage`) - what the editor's Open
    /// console button opens.
    #[serde(default)]
    pub console: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceDef {
    /// The machines this service runs on.
    pub machines: Vec<String>,
    /// Platform (`linux-x86_64`) -> the executable for it.
    pub artifacts: BTreeMap<String, String>,
    #[serde(flatten)]
    pub spec: ServiceSpec,
    /// Machine -> the settings that differ there.
    #[serde(default)]
    pub overrides: BTreeMap<String, Value>,
    /// The environment variable the server's admin token is in - instead of
    /// `admin_token`, so the fleet file holds no secret and can be committed.
    #[serde(default)]
    pub admin_token_env: Option<String>,
}

impl ServiceDef {
    /// The service's settings as they are on one machine.
    pub fn spec_for(&self, machine: &str) -> Result<ServiceSpec, String> {
        let mut spec = match self.overrides.get(machine) {
            Some(Value::Object(changes)) => {
                let mut spec = serde_json::to_value(&self.spec).map_err(|e| e.to_string())?;
                for (key, value) in changes {
                    spec[key] = value.clone();
                }
                serde_json::from_value(spec)
                    .map_err(|e| format!("the overrides for '{machine}' do not fit: {e}"))?
            }
            _ => self.spec.clone(),
        };
        // Read here, on the machine that deploys: the agent is sent the
        // token itself, since it is the one that has to present it.
        if let Some(var) = &self.admin_token_env {
            let token = std::env::var(var).map_err(|_| format!("the environment variable {var} is not set"))?;
            spec.admin_token = Some(token);
        }
        Ok(spec)
    }

    /// Whether this service's servers can be drained and stopped cleanly.
    pub fn has_admin_token(&self) -> bool {
        self.spec.admin_token.is_some() || self.admin_token_env.is_some()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Fleet {
    pub machines: BTreeMap<String, Machine>,
    /// The release key that signs uploads ([`super::ReleaseSigner`]),
    /// relative to the fleet file. Else `JENGINE_RELEASE_KEY_FILE`, else the
    /// `<artifact>.sig` beside each artifact, else unsigned.
    #[serde(default)]
    pub release_key_file: Option<String>,
    /// Where the console POSTs its alerts (`fleet::alerts`); else
    /// `JENGINE_ALERT_WEBHOOK`.
    #[serde(default)]
    pub alert_webhook: Option<String>,
    #[serde(default)]
    pub services: BTreeMap<String, ServiceDef>,
    /// Where the fleet file is, for resolving artifact paths.
    #[serde(skip)]
    pub base: PathBuf,
}

impl Fleet {
    pub fn load(path: &Path) -> Result<Fleet, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        let mut fleet: Fleet =
            serde_json::from_str(&text).map_err(|e| format!("{} is not a fleet file: {e}", path.display()))?;
        fleet.base = path.parent().map(Path::to_path_buf).unwrap_or_default();
        // A certificate is named beside the fleet file, like an artifact.
        for machine in fleet.machines.values_mut() {
            if let Some(ca) = &machine.ca {
                machine.ca = Some(fleet.base.join(ca).to_string_lossy().to_string());
            }
        }
        for (name, service) in &fleet.services {
            if !safe_name(name) {
                return Err(format!("service name '{name}': letters, digits, '.', '_' and '-' only"));
            }
            for machine in &service.machines {
                if !fleet.machines.contains_key(machine) {
                    return Err(format!("service '{name}' names machine '{machine}', which is not in \"machines\""));
                }
            }
        }
        Ok(fleet)
    }

    fn machine(&self, name: &str) -> Result<&Machine, String> {
        self.machines.get(name).ok_or_else(|| format!("no machine called '{name}' in the fleet file"))
    }

    fn service(&self, name: &str) -> Result<&ServiceDef, String> {
        self.services.get(name).ok_or_else(|| format!("no service called '{name}' in the fleet file"))
    }

    /// The machines a command is for: all of a service's, or the one named.
    fn targets<'a>(&'a self, service: &'a ServiceDef, only: Option<&'a str>) -> Result<Vec<&'a str>, String> {
        match only {
            None => Ok(service.machines.iter().map(String::as_str).collect()),
            Some(one) if service.machines.iter().any(|m| m == one) => Ok(vec![one]),
            Some(one) => Err(format!("that service does not run on '{one}'")),
        }
    }
}

impl Machine {
    fn token(&self) -> Result<String, String> {
        if let Some(var) = &self.token_env {
            return std::env::var(var).map_err(|_| format!("the environment variable {var} is not set"));
        }
        self.token.clone().ok_or_else(|| "the machine has neither \"token\" nor \"token_env\"".to_string())
    }

    fn http(&self) -> Result<ureq::Agent, String> {
        crate::listen::client(self.ca.as_deref().map(Path::new))
    }

    /// One call to this machine's agent. `Err` carries the agent's own words
    /// when it refused, or why it could not be reached.
    fn call(&self, method: &str, path: &str, body: Option<&[u8]>, timeout: Duration) -> Result<String, String> {
        self.call_with(method, path, body, timeout, &[])
    }

    fn call_with(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        timeout: Duration,
        extra: &[(&str, &str)],
    ) -> Result<String, String> {
        let url = format!("{}{path}", self.url.trim_end_matches('/'));
        let mut request = self
            .http()?
            .request(method, &url)
            .timeout(timeout)
            .set("Authorization", &format!("Bearer {}", self.token()?));
        for (name, value) in extra {
            request = request.set(name, value);
        }
        let result = match body {
            Some(bytes) => request.set("Content-Type", "application/octet-stream").send_bytes(bytes),
            None if method == "POST" => request.send_bytes(&[]),
            None => request.call(),
        };
        match result {
            Ok(r) => r.into_string().map_err(|e| e.to_string()),
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                let why = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["error"].as_str().map(str::to_string))
                    .unwrap_or(text);
                Err(format!("{why} ({code})"))
            }
            Err(e) => Err(format!("could not reach the agent: {e}")),
        }
    }

    fn json(&self, method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<Value, String> {
        let url = format!("{}{path}", self.url.trim_end_matches('/'));
        let Some(body) = body else {
            let text = self.call(method, path, None, timeout)?;
            return serde_json::from_str(&text).map_err(|e| format!("the agent answered something odd: {e}"));
        };
        let request = self
            .http()?
            .request(method, &url)
            .timeout(timeout)
            .set("Authorization", &format!("Bearer {}", self.token()?))
            .set("Content-Type", "application/json");
        match request.send_string(&body.to_string()) {
            Ok(r) => r.into_json().map_err(|e| e.to_string()),
            Err(ureq::Error::Status(code, r)) => {
                let why = r.into_json::<Value>().ok().and_then(|v| v["error"].as_str().map(str::to_string));
                Err(format!("{} ({code})", why.unwrap_or_else(|| "the agent refused".into())))
            }
            Err(e) => Err(format!("could not reach the agent: {e}")),
        }
    }
}

const QUICK: Duration = Duration::from_secs(10);

/// A server's answer, carried back through its agent.
pub struct Proxied {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// A request to `service` on `machine`, made by its agent on that machine
/// with the server's admin token - so a dashboard is reached through the one
/// token the fleet already holds, whatever address the server listens on.
/// `path` is the server's own, with any query (`admin/api/overview?x=1`).
/// `Err` when the agent itself refused or could not be reached; a server's
/// own refusal is an `Ok` with its status.
pub fn service_http(
    fleet: &Fleet,
    machine: &str,
    service: &str,
    method: &str,
    path: &str,
    body: Option<(&str, &[u8])>,
    headers: &[(&str, String)],
) -> Result<Proxied, String> {
    let m = fleet.machine(machine)?;
    if !safe_name(service) {
        return Err("not a service name".into());
    }
    let url = format!("{}/v1/services/{service}/http/{}", m.url.trim_end_matches('/'), path.trim_start_matches('/'));
    let mut request = m
        .http()?
        .request(method, &url)
        .timeout(Duration::from_secs(35))
        .set("Authorization", &format!("Bearer {}", m.token()?));
    for (name, value) in headers {
        request = request.set(name, value);
    }
    let result = match body {
        Some((kind, bytes)) => request.set("Content-Type", kind).send_bytes(bytes),
        None if method == "GET" || method == "HEAD" => request.call(),
        None => request.send_bytes(&[]),
    };
    let response = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) if r.header("x-jengine-proxied").is_some() => r,
        Err(ureq::Error::Status(code, r)) => {
            let why = r.into_json::<Value>().ok().and_then(|v| v["error"].as_str().map(str::to_string));
            return Err(format!("{} ({code})", why.unwrap_or_else(|| "the agent refused".into())));
        }
        Err(e) => return Err(format!("could not reach the agent: {e}")),
    };
    let status = response.status();
    let content_type = response.header("content-type").unwrap_or("application/octet-stream").to_string();
    let mut body = Vec::new();
    std::io::Read::read_to_end(&mut response.into_reader(), &mut body).map_err(|e| e.to_string())?;
    Ok(Proxied { status, content_type, body })
}

/// The signature to send a release with, if there is one to send: made with
/// the fleet's release key, or the `.sig` file written beside the artifact.
fn signature_for(fleet: &Fleet, artifact: &Path, bytes: &[u8]) -> Result<Option<String>, String> {
    let key_file = fleet
        .release_key_file
        .as_ref()
        .map(|f| fleet.base.join(f))
        .or_else(|| std::env::var_os("JENGINE_RELEASE_KEY_FILE").map(PathBuf::from));
    if let Some(file) = key_file {
        return Ok(Some(super::ReleaseSigner::load(&file)?.sign(bytes)));
    }
    let beside = PathBuf::from(format!("{}.sig", artifact.display()));
    match std::fs::read_to_string(&beside) {
        Ok(sig) => Ok(Some(sig.trim().to_string())),
        Err(_) => Ok(None),
    }
}

/// `jengine-fleet sign <artifact>`: write `<artifact>.sig` with `key`.
pub fn sign_artifact(key: &super::ReleaseSigner, artifact: &Path) -> Result<PathBuf, String> {
    let bytes = std::fs::read(artifact).map_err(|e| format!("could not read {}: {e}", artifact.display()))?;
    let out = PathBuf::from(format!("{}.sig", artifact.display()));
    std::fs::write(&out, key.sign(&bytes)).map_err(|e| format!("could not write {}: {e}", out.display()))?;
    Ok(out)
}

/// What an executable was built for, read from its first bytes - so a
/// Windows program is never sent to a Linux machine to fail there.
fn built_for(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [b'M', b'Z', ..] => Some("windows"),
        [0x7f, b'E', b'L', b'F', ..] => Some("linux"),
        _ => None,
    }
}

/// The processor a Linux program is for, as `std::env::consts::ARCH` names
/// it - so an x86-64 build is refused for a Raspberry Pi here, not by a
/// server that "exited at once" there.
fn elf_arch(bytes: &[u8]) -> Option<&'static str> {
    // e_machine, little-endian (EI_DATA 1) on every target this deploys to.
    if built_for(bytes) != Some("linux") || bytes.get(5) != Some(&1) {
        return None;
    }
    match bytes.get(18..20)? {
        [0x3e, 0] => Some("x86_64"),
        [0xb7, 0] => Some("aarch64"),
        [0x28, 0] => Some("arm"),
        _ => None,
    }
}

/// Every machine's status, in the fleet file's order. A machine that cannot
/// be reached is an `Err`, not a missing row.
///
/// Asked all at once: one machine that is down takes its whole timeout to
/// say so, and should not hold up the answer about the others.
pub fn status(fleet: &Fleet) -> Vec<(String, Result<Value, String>)> {
    status_within(fleet, QUICK)
}

/// The same, giving each machine `patience` to answer - a page that asks
/// every couple of seconds wants an unreachable one said so soon.
pub fn status_within(fleet: &Fleet, patience: Duration) -> Vec<(String, Result<Value, String>)> {
    std::thread::scope(|scope| {
        let asked: Vec<_> = fleet
            .machines
            .iter()
            .map(|(name, machine)| (name, scope.spawn(move || machine.json("GET", "/v1/status", None, patience))))
            .collect();
        asked
            .into_iter()
            .map(|(name, answer)| {
                let answer = answer.join().unwrap_or_else(|_| Err("the status check failed".into()));
                (name.clone(), answer)
            })
            .collect()
    })
}

/// Change the fleet file in place: read, `edit` the JSON, check that what
/// came out is still a fleet file, and write it back - beside and renamed
/// over, so a crash leaves the old file. Comments and key order are the
/// JSON's own: a fleet file is data, and this keeps it so.
pub fn edit_fleet_file(path: &Path, edit: impl FnOnce(&mut Value) -> Result<(), String>) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let mut file: Value = serde_json::from_str(&text).map_err(|e| format!("{} is not JSON: {e}", path.display()))?;
    if !file.is_object() {
        return Err(format!("{} is not a fleet file", path.display()));
    }
    edit(&mut file)?;
    // What a load would say of it, before it is written.
    let mut fleet: Fleet = serde_json::from_value(file.clone()).map_err(|e| format!("that would not be a fleet file: {e}"))?;
    fleet.base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    for (name, service) in &fleet.services {
        if !safe_name(name) {
            return Err(format!("service name '{name}': letters, digits, '.', '_' and '-' only"));
        }
        for machine in &service.machines {
            if !fleet.machines.contains_key(machine) {
                return Err(format!("service '{name}' runs on '{machine}', which is not a machine in the file"));
            }
        }
    }
    let pretty = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, pretty.as_bytes())
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Add or replace a machine: `{ "url", "token" | "token_env", "ca" }`.
pub fn put_machine(path: &Path, name: &str, machine: Value) -> Result<(), String> {
    if name.trim().is_empty() || name.len() > 64 {
        return Err("a machine needs a name of 1 to 64 characters".into());
    }
    if machine["url"].as_str().is_none_or(|u| !u.starts_with("http://") && !u.starts_with("https://")) {
        return Err("a machine needs a url starting with http:// or https://".into());
    }
    edit_fleet_file(path, |file| {
        let machines = file["machines"].as_object_mut().ok_or("the file has no \"machines\" object")?;
        machines.insert(name.to_string(), machine);
        Ok(())
    })
}

/// Remove a machine. A service still naming it is refused, so a service
/// is never left running on a machine the file has forgotten.
pub fn remove_machine(path: &Path, name: &str) -> Result<(), String> {
    edit_fleet_file(path, |file| {
        let used_by: Vec<String> = file["services"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, s)| s["machines"].as_array().is_some_and(|m| m.iter().any(|m| m == name)))
            .map(|(n, _)| n.clone())
            .collect();
        if !used_by.is_empty() {
            return Err(format!("'{name}' still runs {}: take it off those services first", used_by.join(", ")));
        }
        let machines = file["machines"].as_object_mut().ok_or("the file has no \"machines\" object")?;
        machines.remove(name).map(|_| ()).ok_or_else(|| format!("no machine called '{name}'"))
    })
}

/// Add or replace a service: `{ "machines": [..], "artifacts": {..}, ..spec }`.
pub fn put_service(path: &Path, name: &str, service: Value) -> Result<(), String> {
    if !safe_name(name) {
        return Err("a service name is letters, digits, '.', '_' and '-'".into());
    }
    if !service["machines"].is_array() || !service["artifacts"].is_object() {
        return Err("a service needs a list of machines and a map of artifacts".into());
    }
    edit_fleet_file(path, |file| {
        if !file["services"].is_object() {
            file["services"] = json!({});
        }
        file["services"][name] = service;
        Ok(())
    })
}

/// Point a service's build for one platform at `artifact`.
pub fn set_artifact(path: &Path, service: &str, platform: &str, artifact: &str) -> Result<(), String> {
    edit_fleet_file(path, |file| {
        let def = file["services"].get_mut(service).ok_or_else(|| format!("no service called '{service}'"))?;
        if !def["artifacts"].is_object() {
            def["artifacts"] = json!({});
        }
        def["artifacts"][platform] = json!(artifact);
        Ok(())
    })
}

pub fn remove_service(path: &Path, name: &str) -> Result<(), String> {
    edit_fleet_file(path, |file| {
        let services = file["services"].as_object_mut().ok_or("the file has no \"services\" object")?;
        services.remove(name).map(|_| ()).ok_or_else(|| format!("no service called '{name}'"))
    })
}

/// Deploy a service to its machines, one at a time, stopping at the first
/// failure. `say` is told what is happening, for a person to read.
pub fn deploy(fleet: &Fleet, service: &str, only: Option<&str>, say: &mut dyn FnMut(&str)) -> Result<(), String> {
    let def = fleet.service(service)?;
    let targets = fleet.targets(def, only)?;
    for (done, name) in targets.iter().enumerate() {
        let machine = fleet.machine(name)?;
        let spec = def.spec_for(name)?;
        let fail = |why: String| {
            let left = targets.len() - done - 1;
            let rest = if left > 0 { format!("; {left} machine(s) after it were not touched") } else { String::new() };
            format!("{name}: {why}{rest}")
        };
        let platform = machine.json("GET", "/v1/info", None, QUICK).map_err(&fail)?["platform"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let Some(artifact) = def.artifacts.get(&platform) else {
            return Err(fail(format!("it is {platform}, and '{service}' lists no artifact for that")));
        };
        let path = fleet.base.join(artifact);
        let bytes = std::fs::read(&path).map_err(|e| fail(format!("could not read {}: {e}", path.display())))?;
        if let Some(os) = built_for(&bytes)
            && !platform.starts_with(os)
        {
            return Err(fail(format!("{} is a {os} program and the machine is {platform}", path.display())));
        }
        if let Some(arch) = elf_arch(&bytes)
            && !platform.ends_with(arch)
        {
            return Err(fail(format!("{} is built for {arch} and the machine is {platform}", path.display())));
        }
        let version = version_of(&bytes);
        let file = path.file_name().and_then(|f| f.to_str()).unwrap_or("server").to_string();
        if super::is_archive(&file) && spec.exe.is_none() {
            return Err(fail(format!("{file} is an archive: say which file inside it to run (\"exe\")")));
        }
        if !safe_name(&file) {
            return Err(fail(format!("the artifact's file name '{file}' has characters an agent will not store")));
        }

        let release = format!("/v1/releases/{service}/{version}/{file}");
        if machine.call("GET", &release, None, QUICK).is_ok() {
            say(&format!("{name}: already has {version}"));
        } else {
            let signature = signature_for(fleet, &path, &bytes).map_err(&fail)?;
            let signed = if signature.is_some() { ", signed" } else { "" };
            say(&format!("{name}: uploading {version} ({:.1} MB{signed})", bytes.len() as f64 / 1e6));
            let headers: Vec<(&str, &str)> =
                signature.as_deref().map(|sig| vec![(super::agent::SIGNATURE_HEADER, sig)]).unwrap_or_default();
            machine.call_with("PUT", &release, Some(&bytes), Duration::from_secs(600), &headers).map_err(&fail)?;
        }
        say(&format!("{name}: deploying (draining up to {:.0}s)", spec.drain_secs));
        let body = json!({ "version": version, "file": file, "spec": spec });
        // Long enough for the drain, the stop, the health check and a put-back.
        let patience = Duration::from_secs_f64(spec.drain_secs + 2.0 * spec.health_secs + 60.0);
        let answer = machine
            .json("POST", &format!("/v1/services/{service}/deploy"), Some(&body), patience)
            .map_err(&fail)?;
        say(&format!("{name}: {}", answer["message"].as_str().unwrap_or("done")));
    }
    Ok(())
}

/// `start`, `stop`, `restart`, `rollback` or `remove` a service on its
/// machines (or one of them).
pub fn act(fleet: &Fleet, service: &str, action: &str, only: Option<&str>, say: &mut dyn FnMut(&str)) -> Result<(), String> {
    let def = fleet.service(service)?;
    let patience = Duration::from_secs_f64(def.spec.drain_secs + 2.0 * def.spec.health_secs + 60.0);
    for name in fleet.targets(def, only)? {
        let answer = fleet
            .machine(name)?
            .json("POST", &format!("/v1/services/{service}/{action}"), None, patience)
            .map_err(|why| format!("{name}: {why}"))?;
        say(&format!("{name}: {}", answer["message"].as_str().unwrap_or("done")));
    }
    Ok(())
}

/// The end of a service's output on one machine.
pub fn logs(fleet: &Fleet, service: &str, machine: &str, lines: usize) -> Result<String, String> {
    logs_matching(fleet, service, machine, lines, "")
}

/// The last `lines` log lines containing `grep` (every line, if empty) -
/// searched across the generations the agent keeps.
pub fn logs_matching(fleet: &Fleet, service: &str, machine: &str, lines: usize, grep: &str) -> Result<String, String> {
    let grep: String = grep.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.:/ ".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    let path = format!("/v1/services/{service}/logs?lines={lines}&grep={}", grep.replace(' ', "%20"));
    fleet.machine(machine)?.call("GET", &path, None, QUICK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_executable_says_what_it_was_built_for() {
        assert_eq!(built_for(b"MZ\x90\x00"), Some("windows"));
        assert_eq!(built_for(b"\x7fELF\x02"), Some("linux"));
        assert_eq!(built_for(b"#!/bin/sh"), None, "a script is let through");
        let elf = |machine: u8| [&[0x7f, b'E', b'L', b'F', 2, 1][..], &[0; 12], &[machine, 0]].concat();
        assert_eq!(elf_arch(&elf(0xb7)), Some("aarch64"));
        assert_eq!(elf_arch(&elf(0x3e)), Some("x86_64"));
        assert_eq!(elf_arch(b"MZ\x90\x00"), None);
    }

    #[test]
    fn a_secret_can_be_named_instead_of_written() {
        let def: ServiceDef = serde_json::from_str(
            r#"{ "machines": ["a"], "artifacts": {}, "admin_token_env": "JENGINE_TEST_ADMIN_TOKEN_F9" }"#,
        )
        .unwrap();
        assert!(def.has_admin_token());
        let why = def.spec_for("a").unwrap_err();
        assert!(why.contains("JENGINE_TEST_ADMIN_TOKEN_F9"), "an unset variable is said by name: {why}");
        // SAFETY: no other test in this binary reads or writes this variable.
        unsafe { std::env::set_var("JENGINE_TEST_ADMIN_TOKEN_F9", "from-the-environment") };
        assert_eq!(def.spec_for("a").unwrap().admin_token.as_deref(), Some("from-the-environment"));
    }

    #[test]
    fn a_machine_can_differ_from_the_rest() {
        let def: ServiceDef = serde_json::from_str(
            r#"{ "machines": ["a", "b"], "artifacts": {}, "port": 9001, "args": ["{port}"],
                 "overrides": { "b": { "port": 9101, "drain_secs": 0 } } }"#,
        )
        .unwrap();
        assert_eq!(def.spec_for("a").unwrap().port, Some(9001));
        let b = def.spec_for("b").unwrap();
        assert_eq!((b.port, b.drain_secs), (Some(9101), 0.0));
        assert_eq!(b.args, ["{port}"], "what is not overridden is kept");
    }

    #[test]
    fn a_fleet_file_naming_a_machine_it_does_not_have_is_refused() {
        let dir = std::env::temp_dir().join(format!("jengine-fleet-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fleet.json");
        std::fs::write(
            &path,
            r#"{ "machines": { "a": { "url": "http://x", "token": "t" } },
                 "services": { "relay": { "machines": ["a", "b"], "artifacts": {}, "port": 9001 } } }"#,
        )
        .unwrap();
        let why = Fleet::load(&path).unwrap_err();
        assert!(why.contains("machine 'b'"), "{why}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
