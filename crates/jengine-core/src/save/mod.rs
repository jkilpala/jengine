//! Save game system — 2.3.
//!
//! Schema-versioned key/value storage scoped to named **slots**.
//! Scripts call `save_set` / `save_get` / `save_commit` / `save_clear`;
//! the engine routes those through this module.
//!
//! ## Platforms
//! - **Native**: each slot is a JSON file under
//!   `${JENGINE_SAVE_DIR:-./saves}/<encoded slot>.json`, written atomically
//!   (temp file + rename). The directory is created lazily on first commit.
//!   The slot name is percent-encoded into the filename so any name - spaces,
//!   slashes, `con` on Windows - maps to exactly one file and back.
//! - **WASM**: each slot is a `localStorage` entry keyed by
//!   `jengine_save:<slot>`. Requires a `Window` with storage available
//!   (no-op + warn if the browser denies it, e.g. private mode quota).
//!
//! ## On-disk format
//! ```json
//! { "version": 1, "data": { "<key>": <json>, ... } }
//! ```
//! The version field is reserved for future schema migrations; today the
//! loader accepts `version == 1` and treats unknown versions as a soft
//! error (logs + returns `Default::default()`).
//!
//! A slot that exists but cannot be read - corrupt, or written by a newer
//! build - is not silently overwritten: the first commit to it keeps the
//! original as a backup (`<file>.json.bak` natively, `jengine_save_backup:<slot>`
//! in `localStorage`) and logs where it went.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Current on-disk schema version.
pub const SAVE_FORMAT_VERSION: u32 = 1;

/// One slot's payload as written to disk / localStorage.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlotFile {
    version: u32,
    data: HashMap<String, Value>,
}

impl Default for SlotFile {
    fn default() -> Self {
        Self { version: SAVE_FORMAT_VERSION, data: HashMap::new() }
    }
}

/// In-memory store of save slots, lazy-loaded from disk / localStorage on
/// first access and persisted explicitly via `commit`.
#[derive(Debug, Default)]
pub struct SaveSystem {
    /// slot -> key -> value
    slots: HashMap<String, HashMap<String, Value>>,
    /// slots that have been touched by `set` / `clear` since the last commit.
    dirty: std::collections::HashSet<String>,
    /// slots known to be already loaded (so we don't reread from storage).
    loaded: std::collections::HashSet<String>,
    /// Optional override for the native save directory; takes priority over
    /// `JENGINE_SAVE_DIR`. Tests use this to point at a `tempdir`.
    base_dir_override: Option<PathBuf>,
    /// Whether [`Self::ensure_discovered`] has already scanned storage.
    discovered: bool,
    /// Slots whose stored payload exists but could not be loaded. They read
    /// as empty, so a commit would replace a save the game simply failed to
    /// understand; [`Self::commit`] backs the original up first.
    unreadable: std::collections::HashSet<String>,
}

impl SaveSystem {
    /// Construct an empty store with no overrides.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the base directory for native saves (tests + embedders).
    /// Overrides the `JENGINE_SAVE_DIR` env var.
    pub fn set_base_dir(&mut self, dir: PathBuf) {
        self.base_dir_override = Some(dir);
    }

    /// Get a value from a slot. Lazily loads the slot from storage on the
    /// first access. Returns `None` if the slot or key is absent.
    pub fn get(&mut self, slot: &str, key: &str) -> Option<Value> {
        self.ensure_loaded(slot);
        self.slots.get(slot).and_then(|m| m.get(key)).cloned()
    }

    /// Set a value in a slot. Marks the slot dirty (still needs `commit` to
    /// reach storage). Lazily loads the slot first so partial writes don't
    /// clobber existing keys.
    pub fn set(&mut self, slot: &str, key: &str, value: Value) {
        self.ensure_loaded(slot);
        self.slots
            .entry(slot.to_string())
            .or_default()
            .insert(key.to_string(), value);
        self.dirty.insert(slot.to_string());
    }

    /// Drop all keys for a slot (in memory and on storage). Marks the slot
    /// dirty so the next `commit` removes the backing file / localStorage
    /// entry.
    pub fn clear(&mut self, slot: &str) {
        self.slots.remove(slot);
        self.loaded.insert(slot.to_string());
        self.dirty.insert(slot.to_string());
    }

    /// Persist a slot to storage. No-op if the slot has not been touched
    /// since the last commit.
    pub fn commit(&mut self, slot: &str) -> Result<()> {
        if !self.dirty.contains(slot) {
            return Ok(());
        }
        let payload = SlotFile {
            version: SAVE_FORMAT_VERSION,
            data: self.slots.get(slot).cloned().unwrap_or_default(),
        };
        if self.unreadable.contains(slot) {
            // Losing a save because this build could not parse it - a
            // corrupt write, or a file from a newer version of the game - is
            // worse than keeping a copy nobody looks at.
            self.backup_slot(slot)?;
            self.unreadable.remove(slot);
        }
        self.write_slot(slot, &payload)?;
        self.dirty.remove(slot);
        Ok(())
    }

    /// Returns true if `commit` would do work.
    pub fn is_dirty(&self, slot: &str) -> bool {
        self.dirty.contains(slot)
    }

    /// Returns a clone of the in-memory data for `slot` (loads it lazily).
    /// Used by the script bridge to populate per-frame read snapshots.
    pub fn slot_data(&mut self, slot: &str) -> HashMap<String, Value> {
        self.ensure_loaded(slot);
        self.slots.get(slot).cloned().unwrap_or_default()
    }

    /// Returns a clone of every loaded slot. Used by the bridge runner once
    /// per frame so synchronous `save_get` calls in scripts see the latest
    /// in-memory state without holding a borrow on the system.
    pub fn snapshot_all(&self) -> HashMap<String, HashMap<String, Value>> {
        self.slots.clone()
    }

    /// Load every slot that exists in storage. Idempotent.
    ///
    /// Without this a game cannot read its own save on startup. Scripts get
    /// `save_get` answered from [`Self::snapshot_all`], which only clones
    /// slots already in memory — and a fresh process has none, so a value
    /// written by the previous run reads back as nil until something happens
    /// to touch that slot. `ensure_loaded` fixes it per slot, but nothing
    /// knows the slot names up front: they are whatever the game chose.
    ///
    /// So: discover them from storage instead. Runs once, costs one directory
    /// listing (or one `localStorage` scan) plus a read per existing slot,
    /// and saves are small.
    pub fn ensure_discovered(&mut self) {
        if self.discovered {
            return;
        }
        self.discovered = true;
        for slot in self.discover_slots() {
            self.ensure_loaded(&slot);
        }
    }

    /// Eagerly load a slot from storage. Idempotent.
    pub fn ensure_loaded(&mut self, slot: &str) {
        if self.loaded.contains(slot) {
            return;
        }
        self.loaded.insert(slot.to_string());
        match self.read_slot(slot) {
            Ok(Some(payload)) => {
                if payload.version != SAVE_FORMAT_VERSION {
                    log::warn!(
                        "[save] slot '{slot}' has version {} (expected {SAVE_FORMAT_VERSION}); ignoring",
                        payload.version
                    );
                    self.unreadable.insert(slot.to_string());
                    return;
                }
                self.slots.insert(slot.to_string(), payload.data);
            }
            Ok(None) => {}
            Err(e) => {
                log::warn!("[save] failed to load slot '{slot}': {e:#}");
                self.unreadable.insert(slot.to_string());
            }
        }
    }
}

// ─── Native I/O ──────────────────────────────────────────────────────────────
#[cfg(not(target_arch = "wasm32"))]
impl SaveSystem {
    /// Slot names present on disk: every `<slot>.json` in the save directory,
    /// decoded back to the name the game used. A missing directory simply
    /// means no saves yet.
    fn discover_slots(&self) -> Vec<String> {
        let dir = self.base_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Some(slot) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(decode_slot)
            {
                out.push(slot);
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn base_dir(&self) -> PathBuf {
        if let Some(d) = &self.base_dir_override {
            return d.clone();
        }
        if let Ok(env) = std::env::var("JENGINE_SAVE_DIR")
            && !env.is_empty()
        {
            return PathBuf::from(env);
        }
        PathBuf::from("saves")
    }

    fn slot_path(&self, slot: &str) -> PathBuf {
        self.base_dir().join(format!("{}.json", encode_slot(slot)))
    }

    /// Where saves written before slot names were encoded live: every
    /// character outside `[A-Za-z0-9._-]` replaced by `_`. Read as a
    /// fallback so an existing save survives the change of scheme.
    fn legacy_slot_path(&self, slot: &str) -> PathBuf {
        let safe: String = slot
            .chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' => c,
                _ => '_',
            })
            .collect();
        self.base_dir().join(format!("{safe}.json"))
    }

    /// The file a slot's data is in right now: the encoded path, or the
    /// legacy one if only that exists.
    fn existing_slot_path(&self, slot: &str) -> Option<PathBuf> {
        let path = self.slot_path(slot);
        if path.exists() {
            return Some(path);
        }
        let legacy = self.legacy_slot_path(slot);
        (legacy != path && legacy.exists()).then_some(legacy)
    }

    fn read_slot(&self, slot: &str) -> Result<Option<SlotFile>> {
        let Some(path) = self.existing_slot_path(slot) else {
            return Ok(None);
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        let payload: SlotFile = serde_json::from_str(&text)
            .with_context(|| format!("parse {}", path.display()))?;
        Ok(Some(payload))
    }

    fn write_slot(&self, slot: &str, payload: &SlotFile) -> Result<()> {
        let path = self.slot_path(slot);
        let legacy = self.legacy_slot_path(slot);
        // If clear was called and there are no keys, delete the file - and
        // the legacy one, or the next launch would read the cleared data back.
        if payload.data.is_empty() {
            for p in [&path, &legacy] {
                if p.exists() {
                    std::fs::remove_file(p).with_context(|| format!("remove {}", p.display()))?;
                }
            }
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(payload)?;
        // Write beside, then rename over. `fs::write` truncates the save
        // first, so a crash or power cut mid-write left a half-written file
        // that the next launch failed to parse - and treated as no save.
        // The rename replaces the old file in one step on every platform
        // std supports.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("replace {} with {}", path.display(), tmp.display()))?;
        Ok(())
    }

    /// Copy an unreadable slot's file to `<file>.json.bak` before it is
    /// overwritten.
    fn backup_slot(&self, slot: &str) -> Result<()> {
        let Some(path) = self.existing_slot_path(slot) else {
            return Ok(());
        };
        let backup = path.with_extension("json.bak");
        std::fs::copy(&path, &backup)
            .with_context(|| format!("back up {} to {}", path.display(), backup.display()))?;
        log::warn!(
            "[save] slot '{slot}' could not be read; kept the original as {} before overwriting it",
            backup.display()
        );
        Ok(())
    }
}

/// Filename stem for a slot name: bytes outside `[A-Za-z0-9_-]` become `%XX`.
///
/// Reversible, so [`decode_slot`] recovers the name. The old scheme replaced
/// every such character with `_`, which lost the name: discovery handed back
/// `my_save` for a slot the game calls `my save`, so its `save_get` after a
/// restart answered nil, and `a/b` and `a_b` shared a file. `.` is encoded too
/// (Windows strips a trailing one), and so is the first letter of a device
/// name like `con` or `nul`, which Windows will not create even with an
/// extension. Slot names differing only in case still share a file on a
/// case-insensitive filesystem.
#[cfg(not(target_arch = "wasm32"))]
fn encode_slot(slot: &str) -> String {
    use std::fmt::Write;
    if slot.is_empty() {
        return "%".to_string();
    }
    let mut out = String::with_capacity(slot.len());
    for b in slot.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => out.push(b as char),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    if is_windows_device_name(&out) {
        let first = out.as_bytes()[0];
        out.replace_range(0..1, &format!("%{first:02X}"));
    }
    out
}

/// Inverse of [`encode_slot`]. `None` for a stem that is not valid encoding.
///
/// A legacy stem contains no `%` and decodes to itself, which is what makes
/// saves from before the change discoverable.
#[cfg(not(target_arch = "wasm32"))]
fn decode_slot(stem: &str) -> Option<String> {
    if stem == "%" {
        return Some(String::new());
    }
    let bytes = stem.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = stem.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Whether `stem` is a name Windows reserves for a device, in any case.
#[cfg(not(target_arch = "wasm32"))]
fn is_windows_device_name(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit())
}

// ─── WASM I/O ────────────────────────────────────────────────────────────────
#[cfg(target_arch = "wasm32")]
impl SaveSystem {
    fn storage_key(slot: &str) -> String {
        format!("jengine_save:{slot}")
    }

    /// Slot names present in `localStorage`: every key under the
    /// `jengine_save:` prefix. Other keys on the origin are left alone.
    fn discover_slots(&self) -> Vec<String> {
        #[cfg(feature = "web")]
        {
            const PREFIX: &str = "jengine_save:";
            let Some(storage) = Self::local_storage() else {
                return Vec::new();
            };
            let Ok(len) = storage.length() else {
                return Vec::new();
            };
            let mut out = Vec::new();
            for i in 0..len {
                if let Ok(Some(key)) = storage.key(i)
                    && let Some(slot) = key.strip_prefix(PREFIX)
                {
                    out.push(slot.to_string());
                }
            }
            out.sort();
            out
        }
        #[cfg(not(feature = "web"))]
        {
            Vec::new()
        }
    }

    /// Copy an unreadable slot's `localStorage` entry to
    /// `jengine_save_backup:<slot>` before it is overwritten. A different
    /// prefix, so discovery does not mistake the backup for a slot.
    fn backup_slot(&self, slot: &str) -> Result<()> {
        #[cfg(feature = "web")]
        {
            let Some(storage) = Self::local_storage() else {
                return Ok(());
            };
            if let Ok(Some(text)) = storage.get_item(&Self::storage_key(slot)) {
                storage
                    .set_item(&format!("jengine_save_backup:{slot}"), &text)
                    .map_err(|e| anyhow::anyhow!("backing up save slot '{slot}': {e:?}"))?;
                log::warn!(
                    "[save] slot '{slot}' could not be read; kept the original as \
                     localStorage['jengine_save_backup:{slot}'] before overwriting it"
                );
            }
            Ok(())
        }
        #[cfg(not(feature = "web"))]
        {
            let _ = slot;
            Ok(())
        }
    }

    #[cfg(feature = "web")]
    fn local_storage() -> Option<web_sys::Storage> {
        web_sys::window().and_then(|w| w.local_storage().ok().flatten())
    }

    #[cfg(not(feature = "web"))]
    fn local_storage() -> Option<()> {
        None
    }

    fn read_slot(&self, slot: &str) -> Result<Option<SlotFile>> {
        #[cfg(feature = "web")]
        {
            let Some(storage) = Self::local_storage() else {
                return Ok(None);
            };
            let key = Self::storage_key(slot);
            match storage.get_item(&key) {
                Ok(Some(text)) => {
                    let payload: SlotFile = serde_json::from_str(&text)
                        .with_context(|| format!("parse localStorage[{key}]"))?;
                    Ok(Some(payload))
                }
                Ok(None) => Ok(None),
                Err(_) => Ok(None),
            }
        }
        #[cfg(not(feature = "web"))]
        {
            let _ = slot;
            Ok(None)
        }
    }

    fn write_slot(&self, slot: &str, payload: &SlotFile) -> Result<()> {
        #[cfg(feature = "web")]
        {
            let Some(storage) = Self::local_storage() else {
                anyhow::bail!("localStorage unavailable");
            };
            let key = Self::storage_key(slot);
            if payload.data.is_empty() {
                let _ = storage.remove_item(&key);
                return Ok(());
            }
            let text = serde_json::to_string(payload)?;
            storage
                .set_item(&key, &text)
                .map_err(|e| anyhow::anyhow!("localStorage quota? {e:?}"))?;
            Ok(())
        }
        #[cfg(not(feature = "web"))]
        {
            let _ = (slot, payload);
            anyhow::bail!("save not available: web feature disabled")
        }
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_in(dir: &std::path::Path) -> SaveSystem {
        let mut s = SaveSystem::new();
        s.set_base_dir(dir.to_path_buf());
        s
    }

    #[test]
    fn set_get_roundtrips_in_memory() {
        let tmp = tempdir();
        let mut s = make_in(tmp.path());
        s.set("profile1", "level", json!(7));
        s.set("profile1", "name", json!("hero"));
        assert_eq!(s.get("profile1", "level"), Some(json!(7)));
        assert_eq!(s.get("profile1", "name"), Some(json!("hero")));
        assert_eq!(s.get("profile1", "missing"), None);
    }

    #[test]
    fn commit_then_reload_from_disk() {
        let tmp = tempdir();
        {
            let mut s = make_in(tmp.path());
            s.set("p1", "coins", json!(42));
            s.set("p1", "items", json!(["sword", "potion"]));
            s.commit("p1").unwrap();
        }
        let mut s2 = make_in(tmp.path());
        assert_eq!(s2.get("p1", "coins"), Some(json!(42)));
        assert_eq!(s2.get("p1", "items"), Some(json!(["sword", "potion"])));
    }

    #[test]
    fn clear_then_commit_removes_file() {
        let tmp = tempdir();
        let mut s = make_in(tmp.path());
        s.set("p1", "k", json!(1));
        s.commit("p1").unwrap();
        let path = s.slot_path("p1");
        assert!(path.exists());
        s.clear("p1");
        s.commit("p1").unwrap();
        assert!(!path.exists());
        assert_eq!(s.get("p1", "k"), None);
    }

    #[test]
    fn commit_is_noop_when_not_dirty() {
        let tmp = tempdir();
        let mut s = make_in(tmp.path());
        s.set("p1", "k", json!(1));
        s.commit("p1").unwrap();
        assert!(!s.is_dirty("p1"));
        // Direct re-commit should not re-touch file mtime semantics; just
        // verify the flag stays clean.
        s.commit("p1").unwrap();
        assert!(!s.is_dirty("p1"));
    }

    #[test]
    fn a_slot_name_with_a_space_is_readable_after_a_restart() {
        // The script bridge answers `save_get` from the discovered slots, by
        // name. Discovery used to return the sanitized filename (`my_save`),
        // so a game's `save_get("my save", ...)` on the next launch was nil.
        let tmp = tempdir();
        {
            let mut w = make_in(tmp.path());
            w.set("my save", "coins", json!(9));
            w.commit("my save").unwrap();
        }
        let mut fresh = make_in(tmp.path());
        fresh.ensure_discovered();
        let snap = fresh.snapshot_all();
        assert_eq!(
            snap.get("my save").and_then(|m| m.get("coins")),
            Some(&json!(9)),
            "discovered slots: {:?}",
            snap.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn distinct_slot_names_never_share_a_file() {
        let tmp = tempdir();
        let s = make_in(tmp.path());
        let names = ["a/b", "a_b", "a b", "a.b", "a%b", "", "."];
        let paths: std::collections::HashSet<_> = names.iter().map(|n| s.slot_path(n)).collect();
        assert_eq!(paths.len(), names.len());
        for n in names {
            let stem = s.slot_path(n).file_stem().unwrap().to_string_lossy().to_string();
            assert_eq!(decode_slot(&stem).as_deref(), Some(n), "round trip of {n:?}");
        }
    }

    #[test]
    fn windows_device_names_are_writable() {
        let tmp = tempdir();
        let mut s = make_in(tmp.path());
        for name in ["con", "NUL", "com1", "Lpt9"] {
            s.set(name, "k", json!(1));
            s.commit(name).unwrap_or_else(|e| panic!("commit {name}: {e:#}"));
        }
        let mut fresh = make_in(tmp.path());
        fresh.ensure_discovered();
        assert_eq!(fresh.snapshot_all().len(), 4);
    }

    #[test]
    fn a_save_written_under_the_old_names_is_still_read() {
        let tmp = tempdir();
        std::fs::write(
            tmp.path().join("my_save.json"),
            r#"{"version": 1, "data": {"coins": 3}}"#,
        )
        .unwrap();
        let mut s = make_in(tmp.path());
        assert_eq!(s.get("my save", "coins"), Some(json!(3)));

        // Clearing it must not let the legacy file bring it back.
        s.clear("my save");
        s.commit("my save").unwrap();
        let mut fresh = make_in(tmp.path());
        assert_eq!(fresh.get("my save", "coins"), None);
    }

    #[test]
    fn commit_replaces_the_file_without_leaving_a_temp_behind() {
        let tmp = tempdir();
        let mut s = make_in(tmp.path());
        s.set("p1", "k", json!(1));
        s.commit("p1").unwrap();
        s.set("p1", "k", json!(2));
        s.commit("p1").unwrap();
        let names: Vec<String> = std::fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["p1.json".to_string()]);
        assert_eq!(make_in(tmp.path()).get("p1", "k"), Some(json!(2)));
    }

    #[test]
    fn an_unreadable_save_is_backed_up_before_it_is_overwritten() {
        for (label, contents) in [
            ("corrupt", "{\"version\": 1, \"data\": {\"coins\": 5"),
            ("future version", r#"{"version": 999, "data": {"coins": 5}}"#),
        ] {
            let tmp = tempdir();
            std::fs::write(tmp.path().join("p1.json"), contents).unwrap();
            let mut s = make_in(tmp.path());
            assert_eq!(s.get("p1", "coins"), None, "{label}: reads as empty");

            s.set("p1", "coins", json!(1));
            s.commit("p1").unwrap();
            let backup = std::fs::read_to_string(tmp.path().join("p1.json.bak"))
                .unwrap_or_else(|_| panic!("{label}: no backup was kept"));
            assert_eq!(backup, contents, "{label}: backup is not the original");
            assert_eq!(make_in(tmp.path()).get("p1", "coins"), Some(json!(1)));

            // Only the first overwrite backs up; the slot is readable now.
            s.set("p1", "coins", json!(2));
            s.commit("p1").unwrap();
            assert_eq!(
                std::fs::read_to_string(tmp.path().join("p1.json.bak")).unwrap(),
                contents,
                "{label}: a later commit replaced the backup"
            );
        }
    }

    #[test]
    fn unsafe_slot_names_are_sanitized() {
        let tmp = tempdir();
        let s = make_in(tmp.path());
        let p = s.slot_path("../evil/path");
        // Must stay inside base_dir; no path traversal allowed.
        assert!(p.starts_with(tmp.path()));
        // The filename component itself must not contain separators or `..`.
        let fname = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(!fname.contains('/'));
        assert!(!fname.contains('\\'));
        // Path traversal requires `..` as its own path component; encoding
        // turns `../evil/path` into `%2E%2E%2Fevil%2Fpath.json`, which has no
        // separator and so cannot escape base_dir.
        assert!(p.starts_with(tmp.path()));
    }

    #[test]
    fn unknown_version_logs_and_returns_default() {
        let tmp = tempdir();
        let s = make_in(tmp.path());
        // Hand-craft a file with a future version
        std::fs::create_dir_all(tmp.path()).unwrap();
        let p = s.slot_path("p1");
        std::fs::write(
            &p,
            r#"{"version": 999, "data": {"k": 1}}"#,
        )
        .unwrap();
        let mut s2 = make_in(tmp.path());
        assert_eq!(s2.get("p1", "k"), None);
    }

    #[test]
    fn snapshot_all_clones_state() {
        let tmp = tempdir();
        let mut s = make_in(tmp.path());
        s.set("a", "x", json!(1));
        s.set("b", "y", json!(2));
        let snap = s.snapshot_all();
        assert_eq!(snap.get("a").and_then(|m| m.get("x")), Some(&json!(1)));
        assert_eq!(snap.get("b").and_then(|m| m.get("y")), Some(&json!(2)));
    }

    #[test]
    fn snapshot_all_misses_a_slot_written_by_an_earlier_run() {
        // The script bridge answers `save_get` from `snapshot_all()`, which
        // only clones slots already in memory. A fresh process has none, so a
        // value persisted by a previous run is invisible until something
        // touches the slot. This is what a game reading its save on startup
        // hits.
        let tmp = tempdir();
        {
            let mut writer = make_in(tmp.path());
            writer.set("profile", "coins", json!(42));
            writer.commit("profile").expect("commit");
        }
        let mut fresh = make_in(tmp.path());
        assert!(
            fresh.snapshot_all().is_empty(),
            "snapshot_all sees an unloaded slot - has the load path changed?"
        );
        // The data is on disk; `get` finds it because it calls ensure_loaded.
        assert_eq!(fresh.get("profile", "coins"), Some(json!(42)));
    }

    #[test]
    fn ensure_discovered_makes_an_earlier_runs_save_readable() {
        // The fix for the case above: the bridge calls this before publishing
        // `snapshot_all`, so a script reading its save in `init()` on a fresh
        // launch sees it without having to write to the slot first.
        let tmp = tempdir();
        {
            let mut writer = make_in(tmp.path());
            writer.set("profile", "coins", json!(42));
            writer.set("options", "volume", json!(0.5));
            writer.commit("profile").expect("commit profile");
            writer.commit("options").expect("commit options");
        }
        let mut fresh = make_in(tmp.path());
        fresh.ensure_discovered();
        let snap = fresh.snapshot_all();
        assert_eq!(snap.get("profile").and_then(|m| m.get("coins")), Some(&json!(42)));
        assert_eq!(snap.get("options").and_then(|m| m.get("volume")), Some(&json!(0.5)));
    }

    #[test]
    fn ensure_discovered_is_quiet_without_a_save_directory() {
        // First ever launch: no directory, no slots, no error.
        let tmp = tempdir();
        let mut s = make_in(&tmp.path().join("does-not-exist"));
        s.ensure_discovered();
        assert!(s.snapshot_all().is_empty());
    }

    // Tiny tempdir helper that creates a unique dir under std::env::temp_dir()
    // and removes it on drop. Avoids adding a `tempfile` dependency.
    struct TempDir(PathBuf);
    impl TempDir {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!(
            "jengine-save-{}-{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}
