//! Asset packs: parts of a game that are downloaded when they are needed
//! instead of shipping in the build (Oct 2026; Unity's Addressables remote
//! groups, Godot's PCK packs loaded at runtime, Android's on-demand asset
//! packs).
//!
//! A project names its packs in `settings.asset_packs` - each a name and the
//! asset paths it takes, by prefix:
//!
//! ```json
//! "asset_packs": {
//!   "url": "https://cdn.example.com/my-game/",
//!   "packs": [
//!     { "name": "forest", "include": ["scenes/forest/", "sprites/forest/", "music/forest.ogg"] }
//!   ]
//! }
//! ```
//!
//! **A build moves those files out of the game** (`jengine_builder::packs`):
//! each pack becomes one archive (`forest-<hash>.jpak`, [`archive`]) in the
//! build's `packs/` folder, and `assets/packs.json` ([`PackManifest`]) tells
//! the game what each holds, how big it is and its SHA-256. The game is
//! smaller by exactly what the packs took.
//!
//! **A script fetches one when it wants it**: `Packs.download("forest")`,
//! then `Packs.status` / `Packs.progress` until it says `installed`. The
//! requests are the process's, as `scene::streaming`'s are: a script asks
//! here, each host drains them once a frame ([`take_requests`]) and fetches
//! in its own way - natively a worker thread ([`native`]) that verifies the
//! archive and unpacks it into a cache that outlives the session; in the
//! browser a `fetch` ([`web`]) kept in the Cache API, its files sliced out
//! of the archive as they are read.
//!
//! **An installed pack is mounted, and nothing else changes**: every native
//! loader resolves a file through [`crate::asset::find_in_roots`], which
//! looks in the game's own folders and then in the mounted packs, and the
//! web runtime's two fetch helpers ask [`web::read`] first. So a scene, a
//! texture, a sound or a video in a pack loads exactly as it would from the
//! build - a script just has to wait for the pack before loading the scene.
//! Mounting bumps [`generation`], which is how a host knows to ask again for
//! a texture it gave up on while its pack was still remote.
//!
//! **A live ops release can change the packs after the build** (Oct 2026;
//! `crate::liveops`): its manifest - signed, so a pack it names is trusted
//! as the build's own are - is laid over the build's with
//! [`Packs::apply_live`]. A pack it adds is `remote`; one it changes and
//! that is installed becomes **`outdated`**: the old copy stays mounted and
//! loads as before until `Packs.download` fetches the new one. Each pack is
//! fetched from where its manifest says, so a release's archives come from
//! the service and the build's from `packs/`.
//!
//! **Without a manifest every pack is local**: the editor and a run from the
//! project folder have every file on disk, so each declared pack reports
//! `installed` at once ([`configure`] with no manifest) - a game's download
//! flow is exercised by a build.
//!
//! **What a runtime keeps in memory arrives with the mount**: prefabs, data
//! tables, skeletal clips and rigs are read once at startup into maps (and
//! in the browser so are the scripts `require` can reach, since it cannot
//! wait for a fetch), so a host asks each frame what mounted since
//! ([`content::take_arrived`]) and adds a pack's to them. A script on an
//! entity is read when the entity first appears, like a texture, and needs
//! nothing more. The entry scene stays in the base game, and so do input
//! maps ([`stays_in_base`]), which are read before anything else.

pub mod archive;
pub mod content;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};

/// The manifest's file name, under `assets/`.
pub const MANIFEST_FILE: &str = "packs.json";

/// Where packs are fetched from when the project names no `url`: a `packs/`
/// folder beside the game (natively) or beside the page (web) - which is
/// where a build writes them, so a fresh build downloads from itself.
pub const DEFAULT_URL: &str = "packs/";

/// `settings.asset_packs`: what the project declares.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PackSettings {
    /// Where the archives are served from: `https://...`, or a folder
    /// (relative to the game). Empty is [`DEFAULT_URL`].
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packs: Vec<PackDef>,
}

impl PackSettings {
    pub fn is_empty(&self) -> bool {
        self.url.is_empty() && self.packs.is_empty()
    }

    /// `asset_packs` out of a project's `settings` object; empty if absent
    /// or malformed (logged).
    pub fn from_settings(settings: &serde_json::Value) -> Self {
        let Some(v) = settings.get("asset_packs") else { return Self::default() };
        serde_json::from_value(v.clone()).unwrap_or_else(|e| {
            log::warn!("[packs] settings.asset_packs is not valid: {e}");
            Self::default()
        })
    }

    /// The pack that claims asset path `rel`, first declared first.
    pub fn claimant(&self, rel: &str) -> Option<&PackDef> {
        let rel = normalize(rel);
        self.packs.iter().find(|p| p.claims(&rel))
    }
}

/// One declared pack.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PackDef {
    /// Lowercase letters, digits, `-` and `_` - it names a file.
    pub name: String,
    /// Asset paths (relative to `assets/`) the pack takes: a folder with a
    /// trailing `/` takes everything under it, any other entry that file or
    /// that folder.
    #[serde(default)]
    pub include: Vec<String>,
}

impl PackDef {
    /// Whether this pack takes asset path `rel` (already [`normalize`]d).
    pub fn claims(&self, rel: &str) -> bool {
        self.include.iter().any(|inc| {
            let inc = normalize(inc);
            if inc.is_empty() {
                return false;
            }
            if inc.ends_with('/') {
                rel.starts_with(&inc)
            } else {
                rel == inc || (rel.starts_with(&inc) && rel.as_bytes().get(inc.len()) == Some(&b'/'))
            }
        })
    }
}

/// Whether `name` can name a pack (and its archive file).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// `assets/packs.json` and a live ops release's manifest share one format,
/// which lives in `jengine-liveops` - the crate a live ops service links too.
pub use jengine_liveops::{PackEntry, PackManifest};

/// Why `rel` cannot go in a pack, or `None` if it can. Each of these is read
/// by every runtime once, at startup, before a pack could have arrived.
pub fn stays_in_base(rel: &str) -> Option<&'static str> {
    let ext = Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "input" => Some("input maps are read before the game starts"),
        _ => None,
    }
}

/// An asset path as packs key it: forward slashes, relative to `assets/`
/// (a leading `assets/` or `./` is dropped).
pub fn normalize(rel: &str) -> String {
    let mut s = rel.replace('\\', "/");
    loop {
        if let Some(rest) = s.strip_prefix("./") {
            s = rest.to_string();
        } else if let Some(rest) = s.strip_prefix("assets/") {
            s = rest.to_string();
        } else {
            break;
        }
    }
    s
}

/// The SHA-256 of `bytes`, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex(&sha2::Sha256::digest(bytes))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Where a pack is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    /// No such pack in this game.
    Unknown,
    /// Built out of the game and not here yet.
    Remote,
    /// On its way, this far (0..1).
    Downloading(f32),
    /// Here: its files load like the game's own.
    Installed,
    /// Here, but a live ops release has a newer copy: the old one still
    /// loads, and `download` fetches the new one.
    Outdated,
    /// The last download failed ([`error`] says why); asking again retries.
    Failed,
}

impl Status {
    /// `Packs.status`.
    pub fn name(&self) -> &'static str {
        match self {
            Status::Unknown => "unknown",
            Status::Remote => "remote",
            Status::Downloading(_) => "downloading",
            Status::Installed => "installed",
            Status::Outdated => "outdated",
            Status::Failed => "failed",
        }
    }

    /// `Packs.progress`: 1 once installed.
    pub fn progress(&self) -> f32 {
        match self {
            Status::Downloading(p) => p.clamp(0.0, 0.99),
            Status::Installed | Status::Outdated => 1.0,
            _ => 0.0,
        }
    }
}

/// What a script asked a host for.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Fetch, verify and mount the pack.
    Download { name: String },
    /// Unmount the pack and delete its local copy.
    Remove { name: String },
}

#[derive(Debug, Clone)]
struct Pack {
    name: String,
    /// The current release of it; `None` for a local pack (no manifest - its
    /// files are on disk).
    entry: Option<PackEntry>,
    /// Where `entry`'s archive is fetched from, ending in `/`.
    base: String,
    /// What is mounted now: `entry`, or the copy an `outdated` pack still
    /// loads from.
    mounted: Option<PackEntry>,
    /// The copy `entry` replaced and where it came from - what a game
    /// started again with a newer release in its cache still has on disk.
    previous: Option<(PackEntry, String)>,
    status: Status,
    error: String,
}

/// Every pack this game knows and where each is. One per process
/// ([`with`]); tests make their own.
#[derive(Debug, Default)]
pub struct Packs {
    /// The manifest's (or the project's) url.
    url: String,
    /// `Packs.set_url`: every download from now on comes from here.
    url_override: String,
    packs: Vec<Pack>,
    /// Asset path -> index into `packs`, from the current entries.
    owner: HashMap<String, usize>,
    /// Asset path -> the file on disk, for every mounted native pack.
    files: HashMap<String, PathBuf>,
    requests: Vec<Request>,
    /// The game's own asset roots, tried after the packs by
    /// [`crate::asset::find_in_roots`]: a scene loaded out of a pack resolves
    /// its candidate roots inside the pack, and still has to find the
    /// game's textures.
    base_roots: Vec<PathBuf>,
    generation: u64,
    /// Packs mounted since a host last asked ([`Packs::take_arrivals`]).
    arrived: Vec<usize>,
    /// The live ops release laid over the build, if any.
    release: Option<u64>,
}

fn with_slash(u: &str) -> String {
    let u = if u.is_empty() { DEFAULT_URL.to_string() } else { u.to_string() };
    if u.ends_with('/') { u } else { format!("{u}/") }
}

impl Packs {
    /// What this game has: a built game's `manifest` (every pack remote until
    /// mounted), or - with none - the project's declared packs, every one of
    /// them local and installed.
    pub fn configure(&mut self, manifest: Option<PackManifest>, settings: &PackSettings) {
        self.packs.clear();
        self.owner.clear();
        self.files.clear();
        self.requests.clear();
        self.arrived.clear();
        self.url_override.clear();
        self.release = None;
        match manifest {
            Some(m) => {
                self.url = if settings.url.is_empty() { m.url.clone() } else { settings.url.clone() };
                let base = with_slash(&self.url);
                for entry in m.packs {
                    self.packs.push(Pack {
                        name: entry.name.clone(),
                        entry: Some(entry),
                        base: base.clone(),
                        mounted: None,
                        previous: None,
                        status: Status::Remote,
                        error: String::new(),
                    });
                }
            }
            None => {
                self.url = settings.url.clone();
                for def in &settings.packs {
                    self.packs.push(Pack {
                        name: def.name.clone(),
                        entry: None,
                        base: with_slash(&self.url),
                        mounted: None,
                        previous: None,
                        status: Status::Installed,
                        error: String::new(),
                    });
                }
            }
        }
        self.reindex();
        self.generation += 1;
    }

    /// Lay a live ops release's manifest over the build's: a pack it adds is
    /// `remote`; one it changes keeps what is mounted - `outdated` if that
    /// was installed - until the new copy is downloaded. A pack the release
    /// does not name is left as it was, and a local pack (no build manifest:
    /// the editor, a run from the project folder) is never replaced -
    /// its files on disk are the newest there are. Returns whether anything
    /// changed.
    pub fn apply_live(&mut self, release: u64, manifest: &PackManifest) -> bool {
        let base = with_slash(&manifest.url);
        let mut changed = self.release != Some(release);
        self.release = Some(release);
        for le in &manifest.packs {
            if !valid_name(&le.name) {
                log::warn!("[packs] the live release names a pack '{}' - not a pack name", le.name);
                continue;
            }
            match self.find(&le.name) {
                Some(i) => {
                    let p = &mut self.packs[i];
                    let Some(old) = &p.entry else { continue };
                    if old.sha256 == le.sha256 {
                        continue;
                    }
                    if p.previous.is_none() {
                        p.previous = Some((old.clone(), p.base.clone()));
                    }
                    p.entry = Some(le.clone());
                    p.base = base.clone();
                    p.status = match p.status {
                        Status::Installed | Status::Outdated => Status::Outdated,
                        Status::Downloading(f) => Status::Downloading(f),
                        _ => Status::Remote,
                    };
                    changed = true;
                }
                None => {
                    self.packs.push(Pack {
                        name: le.name.clone(),
                        entry: Some(le.clone()),
                        base: base.clone(),
                        mounted: None,
                        previous: None,
                        status: Status::Remote,
                        error: String::new(),
                    });
                    changed = true;
                }
            }
        }
        if changed {
            self.reindex();
            self.generation += 1;
        }
        changed
    }

    /// The live ops release laid over the build, if one is.
    pub fn release(&self) -> Option<u64> {
        self.release
    }

    fn reindex(&mut self) {
        self.owner.clear();
        for (i, p) in self.packs.iter().enumerate() {
            for f in p.entry.iter().chain(p.mounted.iter()).flat_map(|e| &e.files) {
                self.owner.insert(normalize(f), i);
            }
        }
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.packs.iter().position(|p| p.name == name)
    }

    /// `Packs.download(name)`: a pack already here or on its way is left
    /// alone; an `outdated` one fetches its new copy.
    pub fn download(&mut self, name: &str) {
        let Some(i) = self.find(name) else {
            log::warn!("[packs] download: this game has no pack '{name}'");
            return;
        };
        if matches!(self.packs[i].status, Status::Remote | Status::Failed | Status::Outdated) {
            self.packs[i].status = Status::Downloading(0.0);
            self.packs[i].error.clear();
            self.requests.push(Request::Download { name: name.to_string() });
        }
    }

    /// `Packs.remove(name)`: unmounted at once, deleted by the host. A local
    /// pack (no manifest) is the project's own files and is never removed.
    pub fn remove(&mut self, name: &str) {
        let Some(i) = self.find(name) else { return };
        if self.packs[i].entry.is_none() {
            log::info!("[packs] '{name}' is local here (no build manifest) - nothing to remove");
            return;
        }
        if matches!(self.packs[i].status, Status::Installed | Status::Outdated | Status::Failed | Status::Downloading(_)) {
            self.unmount(i);
            self.packs[i].previous = None;
            self.packs[i].status = Status::Remote;
            self.requests.push(Request::Remove { name: name.to_string() });
        }
    }

    fn unmount(&mut self, i: usize) {
        if let Some(m) = self.packs[i].mounted.take() {
            for f in &m.files {
                self.files.remove(&normalize(f));
            }
        }
        self.generation += 1;
    }

    pub fn status(&self, name: &str) -> Status {
        self.find(name).map_or(Status::Unknown, |i| self.packs[i].status)
    }

    /// What the last failed download said, or "".
    pub fn error(&self, name: &str) -> String {
        self.find(name).map(|i| self.packs[i].error.clone()).unwrap_or_default()
    }

    /// The archive's size in bytes (0 for a local or unknown pack).
    pub fn size(&self, name: &str) -> u64 {
        self.find(name).and_then(|i| self.packs[i].entry.as_ref()).map_or(0, |e| e.size)
    }

    /// Every pack's name, in the order declared, then the live release's.
    pub fn names(&self) -> Vec<String> {
        self.packs.iter().map(|p| p.name.clone()).collect()
    }

    /// The pack holding asset path `rel`, if a built pack does.
    pub fn owner_of(&self, rel: &str) -> Option<String> {
        self.owner.get(&normalize(rel)).map(|&i| self.packs[i].name.clone())
    }

    /// The current entry for a built pack.
    pub fn entry(&self, name: &str) -> Option<PackEntry> {
        self.find(name).and_then(|i| self.packs[i].entry.clone())
    }

    /// The copy a changed pack replaced, and the URL it came from - what a
    /// host looks for in its cache when the current copy is not there.
    pub fn previous(&self, name: &str) -> Option<(PackEntry, String)> {
        self.find(name).and_then(|i| {
            let p = &self.packs[i];
            p.previous.as_ref().map(|(e, base)| (e.clone(), format!("{base}{}", e.file)))
        })
    }

    /// `Packs.set_url`: where downloads from now on are fetched.
    pub fn set_url(&mut self, url: &str) {
        self.url_override = url.to_string();
    }

    /// Where archives come from, ending in `/`.
    pub fn url(&self) -> String {
        with_slash(if self.url_override.is_empty() { &self.url } else { &self.url_override })
    }

    /// The URL (or path) of `name`'s current archive.
    pub fn source(&self, name: &str) -> Option<String> {
        let i = self.find(name)?;
        let p = &self.packs[i];
        let e = p.entry.as_ref()?;
        let base = if self.url_override.is_empty() { p.base.clone() } else { with_slash(&self.url_override) };
        Some(format!("{base}{}", e.file))
    }

    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    /// A host's progress report. Ignored for a pack no longer downloading,
    /// so a late report cannot bring back a pack that was removed.
    pub fn set_progress(&mut self, name: &str, p: f32) {
        if let Some(i) = self.find(name)
            && matches!(self.packs[i].status, Status::Downloading(_))
        {
            self.packs[i].status = Status::Downloading(p);
        }
    }

    /// The copy of the pack with hash `sha256` is here. `dir` is where its
    /// files were unpacked (native); `None` when the host serves them itself
    /// (web). Ignored unless the pack was downloading - or is being restored
    /// from a cache at startup (`restoring`). The current release's copy
    /// makes the pack `installed`; an older one (`previous`, or a download
    /// that finished after a release changed the pack) mounts it `outdated`.
    pub fn set_installed(&mut self, name: &str, sha256: &str, dir: Option<&Path>, restoring: bool) {
        let Some(i) = self.find(name) else { return };
        let p = &self.packs[i];
        let ok = matches!(p.status, Status::Downloading(_)) || (restoring && matches!(p.status, Status::Remote | Status::Outdated));
        if !ok {
            return;
        }
        let current = p.entry.as_ref().is_some_and(|e| e.sha256 == sha256);
        let copy = if current {
            p.entry.clone()
        } else {
            p.previous.as_ref().map(|(e, _)| e.clone()).filter(|e| e.sha256 == sha256).or_else(|| p.mounted.clone().filter(|e| e.sha256 == sha256))
        };
        let Some(copy) = copy else {
            log::warn!("[packs] '{name}': a copy ({}) no release names arrived - ignored", &sha256[..sha256.len().min(12)]);
            if matches!(p.status, Status::Downloading(_)) {
                self.packs[i].status = if self.packs[i].mounted.is_some() { Status::Outdated } else { Status::Remote };
            }
            return;
        };
        if restoring && !current && self.packs[i].mounted.is_some() {
            // The current copy is already mounted; an older one is not news.
            return;
        }
        self.unmount(i);
        if let Some(dir) = dir {
            for f in &copy.files {
                let rel = normalize(f);
                self.files.insert(rel.clone(), dir.join(&rel));
            }
        }
        let p = &mut self.packs[i];
        p.mounted = Some(copy);
        if current {
            p.previous = None;
            p.status = Status::Installed;
        } else {
            p.status = Status::Outdated;
        }
        self.arrived.push(i);
        self.reindex();
        self.generation += 1;
    }

    /// The download failed. An `outdated` pack keeps loading its old copy.
    pub fn set_failed(&mut self, name: &str, why: &str) {
        if let Some(i) = self.find(name)
            && matches!(self.packs[i].status, Status::Downloading(_))
        {
            log::warn!("[packs] '{name}' failed: {why}");
            self.packs[i].status = if self.packs[i].mounted.is_some() { Status::Outdated } else { Status::Failed };
            self.packs[i].error = why.to_string();
        }
    }

    /// Where a mounted pack's copy of asset path `rel` is on disk.
    pub fn locate(&self, rel: &str) -> Option<PathBuf> {
        if self.files.is_empty() {
            return None;
        }
        self.files.get(&normalize(rel)).cloned()
    }

    /// The built packs that mounted since the last call - restored from the
    /// cache at startup or downloaded - and are still mounted, in the order
    /// they arrived, each as the copy that is mounted. A host adds what each
    /// brought to what it keeps in memory ([`content::take_arrived`]).
    pub fn take_arrivals(&mut self) -> Vec<PackEntry> {
        let arrived = std::mem::take(&mut self.arrived);
        let mut out: Vec<PackEntry> = Vec::new();
        for i in arrived {
            if let Some(m) = &self.packs[i].mounted
                && !out.iter().any(|o| o.name == m.name)
            {
                out.push(m.clone());
            }
        }
        out
    }

    pub fn set_base_roots(&mut self, roots: Vec<PathBuf>) {
        self.base_roots = roots;
    }

    /// Bumped by every mount, unmount, release and reconfiguration.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// "in pack 'forest', which is remote" for an asset that is missing
    /// because its pack is not here; `None` otherwise. For a host's warning.
    pub fn missing_hint(&self, rel: &str) -> Option<String> {
        let &i = self.owner.get(&normalize(rel))?;
        let p = &self.packs[i];
        (!matches!(p.status, Status::Installed | Status::Outdated)).then(|| format!("in pack '{}', which is {}", p.name, p.status.name()))
    }
}

static STATE: LazyLock<Mutex<Packs>> = LazyLock::new(|| Mutex::new(Packs::default()));

/// The process's packs.
pub fn with<R>(f: impl FnOnce(&mut Packs) -> R) -> R {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut s)
}

/// See [`Packs::configure`].
pub fn configure(manifest: Option<PackManifest>, settings: &PackSettings) {
    with(|p| p.configure(manifest, settings));
}

pub fn download(name: &str) {
    with(|p| p.download(name));
}

pub fn remove(name: &str) {
    with(|p| p.remove(name));
}

pub fn status(name: &str) -> Status {
    with(|p| p.status(name))
}

pub fn error(name: &str) -> String {
    with(|p| p.error(name))
}

pub fn size(name: &str) -> u64 {
    with(|p| p.size(name))
}

pub fn names() -> Vec<String> {
    with(|p| p.names())
}

pub fn owner_of(rel: &str) -> Option<String> {
    with(|p| p.owner_of(rel))
}

pub fn set_url(url: &str) {
    with(|p| p.set_url(url));
}

pub fn take_requests() -> Vec<Request> {
    with(|p| p.take_requests())
}

pub fn set_progress(name: &str, progress: f32) {
    with(|p| p.set_progress(name, progress));
}

pub fn set_installed(name: &str, sha256: &str, dir: Option<&Path>, restoring: bool) {
    with(|p| p.set_installed(name, sha256, dir, restoring));
}

/// See [`Packs::apply_live`].
pub fn apply_live(release: u64, manifest: &PackManifest) -> bool {
    with(|p| p.apply_live(release, manifest))
}

pub fn set_failed(name: &str, why: &str) {
    with(|p| p.set_failed(name, why));
}

pub fn locate(rel: &str) -> Option<PathBuf> {
    with(|p| p.locate(rel))
}

pub fn set_base_roots(roots: Vec<PathBuf>) {
    with(|p| p.set_base_roots(roots));
}

pub fn take_arrivals() -> Vec<PackEntry> {
    with(|p| p.take_arrivals())
}

pub(crate) fn base_roots() -> Vec<PathBuf> {
    with(|p| p.base_roots.clone())
}

pub fn generation() -> u64 {
    with(|p| p.generation())
}

pub fn missing_hint(rel: &str) -> Option<String> {
    with(|p| p.missing_hint(rel))
}

/// The one script-facing operation, shared by the Luau globals and the
/// browsers' `state.packs(op, a)` hook so the two cannot answer differently.
/// Returns what the script function returns.
pub fn script_op(op: &str, arg: &str) -> ScriptAnswer {
    match op {
        "download" => download(arg),
        "remove" => remove(arg),
        "set_url" => set_url(arg),
        "status" => return ScriptAnswer::Text(status(arg).name().to_string()),
        "progress" => return ScriptAnswer::Number(status(arg).progress() as f64),
        "size" => return ScriptAnswer::Number(size(arg) as f64),
        "error" => return ScriptAnswer::Text(error(arg)),
        "of" => return ScriptAnswer::Text(owner_of(arg).unwrap_or_default()),
        "list" => return ScriptAnswer::List(names()),
        other => log::warn!("[packs] unknown op '{other}'"),
    }
    ScriptAnswer::Nothing
}

/// What [`script_op`] gives back.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptAnswer {
    Nothing,
    Text(String),
    Number(f64),
    List(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> PackManifest {
        PackManifest {
            version: 1,
            url: "https://cdn.example.com/game".into(),
            packs: vec![PackEntry {
                name: "forest".into(),
                file: "forest-0123abcd.jpak".into(),
                size: 1234,
                sha256: "00".into(),
                files: vec!["scenes/forest/a.scene".into(), "sprites/forest/tree.png".into()],
            }],
        }
    }

    #[test]
    fn a_pack_claims_by_folder_or_file() {
        let def = PackDef { name: "forest".into(), include: vec!["sprites/forest/".into(), "music/forest.ogg".into(), "assets/scenes/forest".into()] };
        assert!(def.claims("sprites/forest/tree.png"));
        assert!(def.claims("music/forest.ogg"));
        assert!(def.claims("scenes/forest/a.scene"), "a folder named without its slash");
        assert!(def.claims("scenes/forest"));
        assert!(!def.claims("scenes/forestry/a.scene"), "a prefix of a name is not a folder");
        assert!(!def.claims("music/forest.ogg.bak"));
        assert!(!def.claims("sprites/hero.png"));
        let empty = PackDef { name: "all".into(), include: vec!["".into(), "./".into()] };
        assert!(!empty.claims("sprites/hero.png"), "an empty entry takes nothing, not everything");
    }

    #[test]
    fn only_input_maps_stay_in_the_base_game() {
        assert!(stays_in_base("controls.input").is_some());
        for rel in [
            "sprites/a.png", "scenes/a.scene", "music/a.ogg", "video/a.mp4", "fonts/a.font", "ui/a.gui",
            "scripts/a.lua", "scripts/b.luau", "scripts/c.js", "prefabs/x.prefab", "data/t.data",
            "data/t.dataschema", "locale/ui.data", "anims/walk.skelanim", "rigs/arm.rig",
        ] {
            assert!(stays_in_base(rel).is_none(), "{rel}");
        }
    }

    #[test]
    fn a_built_pack_downloads_mounts_and_goes_again() {
        let mut p = Packs::default();
        p.configure(Some(manifest()), &PackSettings::default());
        assert_eq!(p.status("forest"), Status::Remote);
        assert_eq!(p.status("desert"), Status::Unknown);
        assert_eq!(p.size("forest"), 1234);
        assert_eq!(p.owner_of("assets/sprites/forest/tree.png").as_deref(), Some("forest"));
        assert_eq!(p.source("forest").as_deref(), Some("https://cdn.example.com/game/forest-0123abcd.jpak"));
        assert!(p.missing_hint("sprites/forest/tree.png").unwrap().contains("remote"));

        p.download("forest");
        p.download("forest");
        assert_eq!(p.take_requests(), vec![Request::Download { name: "forest".into() }], "asked once");
        p.set_progress("forest", 0.5);
        assert_eq!(p.status("forest").progress(), 0.5);
        let g = p.generation();
        let dir = PathBuf::from("/cache/forest-0123");
        p.set_installed("forest", "00", Some(&dir), false);
        assert_eq!(p.status("forest").name(), "installed");
        assert!(p.generation() > g, "a mount is news to the hosts");
        assert_eq!(p.locate("sprites/forest/tree.png"), Some(dir.join("sprites/forest/tree.png")));
        assert_eq!(p.locate("assets\\sprites\\forest\\tree.png"), Some(dir.join("sprites/forest/tree.png")));
        assert_eq!(p.locate("sprites/hero.png"), None);
        assert!(p.missing_hint("sprites/forest/tree.png").is_none());
        let arrived = p.take_arrivals();
        assert_eq!(arrived.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["forest"], "the hosts hear of it once");
        assert!(p.take_arrivals().is_empty());

        p.remove("forest");
        assert_eq!(p.status("forest"), Status::Remote);
        assert_eq!(p.locate("sprites/forest/tree.png"), None, "unmounted at once");
        assert_eq!(p.take_requests(), vec![Request::Remove { name: "forest".into() }]);
        p.set_installed("forest", "00", Some(&dir), false);
        assert_eq!(p.status("forest"), Status::Remote, "a late install cannot bring a removed pack back");
        assert!(p.take_arrivals().is_empty());
    }

    #[test]
    fn a_live_release_adds_packs_and_outdates_changed_ones() {
        let mut p = Packs::default();
        p.configure(Some(manifest()), &PackSettings::default());
        p.download("forest");
        p.take_requests();
        let old_dir = PathBuf::from("/cache/forest-old");
        p.set_installed("forest", "00", Some(&old_dir), false);
        p.take_arrivals();

        let live = PackManifest {
            version: 1,
            url: "https://ops.example.com/liveops/v1/packs".into(),
            packs: vec![
                PackEntry { name: "forest".into(), file: "forest-11.jpak".into(), size: 99, sha256: "11".into(), files: vec!["scenes/forest/a.scene".into()] },
                PackEntry { name: "desert".into(), file: "desert-22.jpak".into(), size: 5, sha256: "22".into(), files: vec!["scenes/desert.scene".into()] },
            ],
        };
        assert!(p.apply_live(4, &live));
        assert!(!p.apply_live(4, &live), "the same release again changes nothing");
        assert_eq!(p.release(), Some(4));
        assert_eq!(p.status("forest").name(), "outdated");
        assert_eq!(p.status("forest").progress(), 1.0);
        assert_eq!(p.locate("sprites/forest/tree.png"), Some(old_dir.join("sprites/forest/tree.png")), "the old copy still loads");
        assert_eq!(p.status("desert").name(), "remote");
        assert_eq!(p.owner_of("scenes/desert.scene").as_deref(), Some("desert"));
        assert_eq!(p.source("forest").as_deref(), Some("https://ops.example.com/liveops/v1/packs/forest-11.jpak"), "from the service");
        assert_eq!(p.previous("forest").unwrap().1, "https://cdn.example.com/game/forest-0123abcd.jpak", "and the old one from the build's url");

        // Downloading the new copy swaps it in; a failure keeps the old.
        p.download("forest");
        p.set_failed("forest", "offline");
        assert_eq!(p.status("forest").name(), "outdated", "a failed update still has a pack");
        p.download("forest");
        let new_dir = PathBuf::from("/cache/forest-new");
        p.set_installed("forest", "11", Some(&new_dir), false);
        assert_eq!(p.status("forest").name(), "installed");
        assert_eq!(p.locate("scenes/forest/a.scene"), Some(new_dir.join("scenes/forest/a.scene")));
        assert_eq!(p.locate("sprites/forest/tree.png"), None, "the new copy has no tree");
        assert_eq!(p.take_arrivals()[0].sha256, "11");

        // Started again with the release in the cache: the old copy mounts
        // outdated when the new one is not on disk.
        let mut q = Packs::default();
        q.configure(Some(manifest()), &PackSettings::default());
        q.apply_live(4, &live);
        q.set_installed("forest", "00", Some(&old_dir), true);
        assert_eq!(q.status("forest").name(), "outdated");
        assert_eq!(q.take_arrivals()[0].sha256, "00", "the copy that is mounted is what arrived");
    }

    #[test]
    fn a_local_pack_is_never_replaced_by_a_release() {
        let mut p = Packs::default();
        let settings = PackSettings { url: String::new(), packs: vec![PackDef { name: "forest".into(), include: vec!["sprites/forest/".into()] }] };
        p.configure(None, &settings);
        let live = PackManifest { version: 1, url: String::new(), packs: vec![PackEntry { name: "forest".into(), sha256: "9".into(), ..Default::default() }] };
        p.apply_live(1, &live);
        assert_eq!(p.status("forest"), Status::Installed, "the files on disk are the newest");
    }

    #[test]
    fn a_failure_is_reported_and_retried() {
        let mut p = Packs::default();
        p.configure(Some(manifest()), &PackSettings::default());
        p.download("forest");
        p.set_failed("forest", "HTTP 404");
        assert_eq!(p.status("forest").name(), "failed");
        assert_eq!(p.error("forest"), "HTTP 404");
        p.take_requests();
        p.download("forest");
        assert_eq!(p.take_requests().len(), 1, "asking again retries");
        assert_eq!(p.error("forest"), "");
    }

    #[test]
    fn without_a_manifest_every_declared_pack_is_local() {
        let mut p = Packs::default();
        let settings = PackSettings { url: String::new(), packs: vec![PackDef { name: "forest".into(), include: vec!["sprites/forest/".into()] }] };
        p.configure(None, &settings);
        assert_eq!(p.status("forest"), Status::Installed);
        p.download("forest");
        p.remove("forest");
        assert!(p.take_requests().is_empty(), "nothing to fetch or delete");
        assert_eq!(p.status("forest"), Status::Installed);
        assert_eq!(p.url(), "packs/");
    }

    #[test]
    fn a_url_set_at_runtime_wins_and_gains_its_slash() {
        let mut p = Packs::default();
        p.configure(Some(manifest()), &PackSettings::default());
        p.set_url("https://mirror.example.com/g");
        assert_eq!(p.source("forest").as_deref(), Some("https://mirror.example.com/g/forest-0123abcd.jpak"));
    }

    #[test]
    fn names_are_file_safe() {
        assert!(valid_name("forest-2_b"));
        for bad in ["", "Forest", "a/b", "..", "a b", "ä"] {
            assert!(!valid_name(bad), "{bad:?}");
        }
    }
}
