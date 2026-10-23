//! **Putting servers on machines** - an agent on each one, and a command
//! line that talks to them.
//!
//! ```text
//!   your machine                      each server machine
//!   ┌──────────────┐   HTTP + token   ┌───────────────────────────────┐
//!   │ jengine-fleet│ ───────────────► │ jengine-fleet agent           │
//!   │ fleet.json   │   upload, deploy │  ├─ releases/zones/<hash>/..  │
//!   └──────────────┘   status, logs   │  └─ runs and watches `zones`  │
//!                                     └───────────────────────────────┘
//! ```
//!
//! # The three words
//!
//! - A **release** is one executable, named by the hash of its bytes. The
//!   same build is the same release everywhere, uploading it twice is a
//!   no-op, and an agent refuses bytes that do not hash to their name.
//! - A **service** is a release running on a machine with a [`ServiceSpec`]:
//!   its arguments, its port, how long to drain. Its working directory
//!   belongs to the service, not the release, so the database a server keeps
//!   there is still there after a deploy.
//! - A **deploy** replaces what a service runs, and either finishes or puts
//!   back what was there: drain the old server, stop it (everyone saved),
//!   start the new one, wait for its health check - and if that never comes,
//!   start the old one again and say so.
//!
//! The command line has a face, too: `jengine-fleet ui` serves a page on your
//! own machine that does all of the above with buttons - see [`ui`].
//!
//! # What the agent is trusted with
//!
//! **Its token alone does not run new code.** An agent given a **release
//! key** (`--release-key`, a public key `jengine-fleet keygen` makes) runs
//! only releases signed by its secret half: `jengine-fleet deploy` signs
//! each upload (`"release_key_file"` in the fleet file, or
//! `JENGINE_RELEASE_KEY_FILE`), or carries a `<artifact>.sig` made beforehand
//! (`jengine-fleet sign`) - so the signing key can stay on the machine that
//! builds, and a stolen agent token can start, stop and roll back what is
//! already there but cannot put a program of its own on the machine. An
//! agent listening beyond loopback refuses to start without a release key
//! unless told `--allow-unsigned`. A deploy's own settings cannot reach past
//! the release either: an archive's `exe` may not leave it, and loader
//! variables (`LD_PRELOAD` and kin) are refused in a signed agent's `env`.
//!
//! The token is still compared in constant time, and given a certificate
//! (`--tls-cert`, `--tls-key`; `jengine-fleet cert` makes a pair) the agent
//! speaks HTTPS only, the fleet file naming the one certificate to trust for
//! that machine. Without one it speaks plain HTTP - then reach it over a
//! private network (WireGuard, Tailscale, a cloud VPC), never across the
//! open internet.
//!
//! # What it does not do
//!
//! It does not build. A Linux machine needs a Linux executable, and making
//! one is the build's job (on Linux, in CI, or with a cross toolchain); the
//! command line picks the right artifact per machine from `fleet.json` and
//! refuses to send a Windows program to a Linux agent. One port means one
//! server at a time, so a deploy is a short gap per machine rather than a
//! hand-over: players reconnect with their token and are where they were.

pub mod agent;
pub mod alerts;
pub mod client;
pub mod gateway;
pub mod history;
pub mod tied;
pub mod ui;
pub mod watch;

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How a service is run. Everything has a default; a relay needs only a port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceSpec {
    /// Arguments to the executable. `{port}` becomes the port.
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// The port the server listens on. With one, a deploy waits for the
    /// health check to answer there; without, for the process to stay up.
    #[serde(default)]
    pub port: Option<u16>,
    /// The server's admin token. It is handed to the server as
    /// `JENGINE_ADMIN_TOKEN`, and lets the agent drain it and stop it
    /// cleanly. Without one a stop is a signal (or, on Windows, a kill).
    #[serde(default)]
    pub admin_token: Option<String>,
    /// Longest a deploy waits for players to leave a draining server before
    /// stopping it anyway. They are saved either way.
    #[serde(default = "default_drain")]
    pub drain_secs: f64,
    /// Longest a deploy waits for a new server to answer its health check.
    #[serde(default = "default_health")]
    pub health_secs: f64,
    #[serde(default = "default_health_path")]
    pub health_path: String,
    /// For an archive release - an artifact ending in `.zip`, unpacked by the
    /// agent - the file inside it to run (`bin/my-server`). The archive's
    /// other files are beside it, where a server finds them from its own
    /// path (`std::env::current_exe()`); the working directory stays the
    /// service's, so a database file named in `args` outlives a deploy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exe: Option<String>,
}

fn default_drain() -> f64 {
    30.0
}
fn default_health() -> f64 {
    20.0
}
fn default_health_path() -> String {
    "/healthz".into()
}

impl Default for ServiceSpec {
    fn default() -> Self {
        ServiceSpec {
            args: Vec::new(),
            env: BTreeMap::new(),
            port: None,
            admin_token: None,
            drain_secs: default_drain(),
            health_secs: default_health(),
            health_path: default_health_path(),
            exe: None,
        }
    }
}

/// A request to a server on this machine. `None` when it did not answer.
pub(crate) async fn local(method: &'static str, port: u16, path: &str, token: Option<&str>) -> Option<(u16, String)> {
    let url = format!("http://127.0.0.1:{port}{path}");
    let token = token.map(str::to_string);
    tokio::task::spawn_blocking(move || {
        let mut request = ureq::request(method, &url).timeout(Duration::from_secs(2));
        if let Some(token) = &token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        let result = if method == "POST" {
            request.set("Content-Type", "application/json").send_string("{}")
        } else {
            request.call()
        };
        match result {
            Ok(r) => Some((r.status(), r.into_string().unwrap_or_default())),
            Err(ureq::Error::Status(code, r)) => Some((code, r.into_string().unwrap_or_default())),
            Err(_) => None,
        }
    })
    .await
    .ok()
    .flatten()
}

/// Whether an artifact is an archive to unpack rather than a program to run.
pub fn is_archive(file: &str) -> bool {
    file.ends_with(".zip")
}

/// `windows-x86_64`, `linux-aarch64`: what an agent runs on, and the key an
/// artifact is listed under in `fleet.json`.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// What players on the LAN can call this machine: its name with `.local`,
/// which mDNS (Avahi on a Raspberry Pi, built into Windows and macOS)
/// answers without any DNS server.
pub fn host_name() -> String {
    match sysinfo::System::host_name() {
        Some(name) if !name.is_empty() && !name.contains('.') => format!("{}.local", name.to_lowercase()),
        Some(name) if !name.is_empty() => name,
        _ => "localhost".into(),
    }
}

/// A release's name: the first sixteen hex digits of its SHA-256.
pub fn version_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// A release key, the half that signs: `jfr-secret-<64 hex>`.
pub struct ReleaseSigner(jengine_liveops::SecretKey);

/// A release key, the half an agent checks by: `jfr-public-<64 hex>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseKey(jengine_liveops::PublicKey);

const RELEASE_SECRET: &str = "jfr-secret-";
const RELEASE_PUBLIC: &str = "jfr-public-";

/// What a release's signature is over: its whole SHA-256.
fn release_message(bytes: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    format!("jengine-release-v1\n{hash}").into_bytes()
}

fn seed32(text: &str, prefix: &str) -> Result<[u8; 32], String> {
    let hex = text.trim().strip_prefix(prefix).ok_or_else(|| format!("a release key here starts with {prefix}"))?;
    jengine_liveops::from_hex(hex).and_then(|v| v.try_into().ok()).ok_or_else(|| "expected 64 hex digits".to_string())
}

impl ReleaseSigner {
    /// A new key from the OS's randomness.
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).expect("the OS has randomness");
        ReleaseSigner(jengine_liveops::SecretKey::from_seed(seed))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        Ok(ReleaseSigner(jengine_liveops::SecretKey::from_seed(seed32(text, RELEASE_SECRET)?)))
    }

    /// From a file holding [`ReleaseSigner::to_text`].
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn to_text(&self) -> String {
        format!("{RELEASE_SECRET}{}", jengine_liveops::to_hex(&self.0.seed()))
    }

    pub fn public(&self) -> ReleaseKey {
        ReleaseKey(self.0.public())
    }

    /// The signature of a release's bytes, hex - what `<artifact>.sig` holds.
    pub fn sign(&self, bytes: &[u8]) -> String {
        self.0.sign_bytes(&release_message(bytes))
    }
}

impl ReleaseKey {
    pub fn parse(text: &str) -> Result<Self, String> {
        jengine_liveops::PublicKey::from_bytes(&seed32(text, RELEASE_PUBLIC)?).map(ReleaseKey).map_err(|e| e.to_string())
    }

    pub fn to_text(&self) -> String {
        format!("{RELEASE_PUBLIC}{}", jengine_liveops::to_hex(&self.0.to_bytes()))
    }

    /// Whether `signature` is this key's over exactly these bytes.
    pub fn signed(&self, bytes: &[u8], signature: &str) -> bool {
        self.0.verify_bytes(&release_message(bytes), signature.trim())
    }
}

/// Environment variables that load code into a program before it runs - a
/// signed release could be made to run something else through them.
pub fn loader_variable(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n.starts_with("LD_") || n.starts_with("DYLD_") || n == "PATH"
}

/// Whether an archive's `exe` stays inside the archive: relative, and with
/// no `..` or root in it.
pub fn exe_inside(exe: &str) -> bool {
    let path = std::path::Path::new(exe);
    !exe.is_empty()
        && path.components().all(|c| matches!(c, std::path::Component::Normal(_)))
        && !exe.contains(':')
}

/// Names end up in paths on the agent's disk, so they are held to letters,
/// digits, `.`, `_` and `-`, and may not start with a dot.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_is_named_by_its_bytes() {
        assert_eq!(version_of(b"one"), version_of(b"one"));
        assert_ne!(version_of(b"one"), version_of(b"two"));
        assert_eq!(version_of(b"one").len(), 16);
    }

    #[test]
    fn names_that_would_leave_the_data_directory_are_refused() {
        for bad in ["", "..", ".hidden", "a/b", "a\\b", "c:", "a b", &"x".repeat(65)] {
            assert!(!safe_name(bad), "{bad:?}");
        }
        for good in ["zones", "zones.exe", "relay-eu_1", "3f9a0c11d2e4b6a7"] {
            assert!(safe_name(good), "{good:?}");
        }
    }

    #[test]
    fn a_release_is_signed_by_its_key_and_by_nothing_else() {
        let key = ReleaseSigner::generate();
        let back = ReleaseSigner::parse(&key.to_text()).unwrap();
        assert_eq!(back.public(), key.public());
        assert_eq!(ReleaseKey::parse(&key.public().to_text()).unwrap(), key.public());
        assert!(ReleaseKey::parse(&key.to_text()).is_err(), "the secret half is not the public one");
        let sig = key.sign(b"a server");
        assert!(key.public().signed(b"a server", &sig));
        assert!(!key.public().signed(b"a server, changed", &sig));
        assert!(!ReleaseSigner::generate().public().signed(b"a server", &sig));
        assert!(!key.public().signed(b"a server", ""));
    }

    #[test]
    fn a_deploy_cannot_reach_past_its_release() {
        for bad in ["", "../../bin/sh", "/bin/sh", "bin/../../x", "C:\\Windows\\cmd.exe", "c:x"] {
            assert!(!exe_inside(bad), "{bad:?}");
        }
        for good in ["server", "bin/my-server", "bin/my-server.exe"] {
            assert!(exe_inside(good), "{good:?}");
        }
        for loader in ["LD_PRELOAD", "ld_library_path", "DYLD_INSERT_LIBRARIES", "PATH"] {
            assert!(loader_variable(loader), "{loader}");
        }
        assert!(!loader_variable("RUST_LOG"));
    }

    #[test]
    fn a_spec_needs_nothing_but_what_differs() {
        let spec: ServiceSpec = serde_json::from_str(r#"{ "port": 9001 }"#).unwrap();
        assert_eq!(spec.port, Some(9001));
        assert_eq!((spec.drain_secs, spec.health_path.as_str()), (30.0, "/healthz"));
    }
}
