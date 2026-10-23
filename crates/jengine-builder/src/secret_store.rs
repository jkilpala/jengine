//! Where a project's secrets are kept: the OS keychain (Windows Credential
//! Manager, the macOS Keychain, the Secret Service on Linux), behind a trait
//! so tests never touch the real one.
//!
//! A secret is named by its project (the canonical path) and its name
//! (`admin_token`). [`crate::servers::Secrets`] is the one user: it reads its
//! tokens here, moves an old `secrets.json` in, and keeps the file only when
//! no keychain answers ([`SecretStore::get`] / [`SecretStore::set`] erring
//! means "there is no keychain to use", never "not found").

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{Result, anyhow};

/// The keychain service every project's secrets go under, followed by the
/// project's path: `jengine-servers:C:\Games\my-game`.
pub const SERVICE_PREFIX: &str = "jengine-servers:";

/// A place secrets are kept.
pub trait SecretStore: Send + Sync {
    /// `project`'s secret `name`: `Ok(None)` when it has none, `Err` when
    /// the store cannot be used at all (no keychain on this machine, a
    /// locked one, a refusal) - the caller falls back then.
    fn get(&self, project: &str, name: &str) -> Result<Option<String>>;

    /// Keep `value` as `project`'s secret `name`, replacing any. `Err` as
    /// for [`get`](Self::get).
    fn set(&self, project: &str, name: &str, value: &str) -> Result<()>;

    /// What to call it in a log line.
    fn describe(&self) -> &'static str;
}

/// The OS keychain, through the `keyring` crate.
pub struct Keychain;

fn entry(project: &str, name: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(&format!("{SERVICE_PREFIX}{project}"), name).map_err(|e| anyhow!("the keychain: {e}"))
}

impl SecretStore for Keychain {
    fn get(&self, project: &str, name: &str) -> Result<Option<String>> {
        match entry(project, name)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow!("the keychain: {e}")),
        }
    }

    fn set(&self, project: &str, name: &str, value: &str) -> Result<()> {
        entry(project, name)?.set_password(value).map_err(|e| anyhow!("the keychain: {e}"))
    }

    fn describe(&self) -> &'static str {
        if cfg!(windows) {
            "Windows Credential Manager"
        } else if cfg!(target_os = "macos") {
            "the macOS Keychain"
        } else {
            "the Secret Service"
        }
    }
}

/// Secrets in memory - for tests, and for a tool that must not keep any.
/// [`MemoryStore::unavailable`] is a machine with no keychain;
/// [`MemoryStore::write_only`] one that takes a write and gives back
/// something else (a keychain that failed without saying so).
#[derive(Default)]
pub struct MemoryStore {
    values: Mutex<HashMap<(String, String), String>>,
    unavailable: bool,
    write_only: bool,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// A store that errs on every call, as a headless Linux box with no
    /// Secret Service does.
    pub fn unavailable() -> Self {
        Self { unavailable: true, ..Self::default() }
    }

    /// A store whose writes seem to succeed and keep nothing.
    pub fn write_only() -> Self {
        Self { write_only: true, ..Self::default() }
    }

    /// Everything kept, for a test to look at.
    pub fn len(&self) -> usize {
        self.values.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, project: &str, name: &str) -> Result<Option<String>> {
        if self.unavailable {
            return Err(anyhow!("no keychain here"));
        }
        Ok(self.values.lock().unwrap().get(&(project.to_string(), name.to_string())).cloned())
    }

    fn set(&self, project: &str, name: &str, value: &str) -> Result<()> {
        if self.unavailable {
            return Err(anyhow!("no keychain here"));
        }
        if !self.write_only {
            self.values.lock().unwrap().insert((project.to_string(), name.to_string()), value.to_string());
        }
        Ok(())
    }

    fn describe(&self) -> &'static str {
        "memory"
    }
}
