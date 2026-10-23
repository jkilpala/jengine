//! **Live ops** (Oct 2026): what a shipped game is told without a new build -
//! remote config, A/B experiments, scheduled live events and content
//! releases - kept per **channel** (`dev`, `staging`, `live`) and served to
//! each game as one signed [`Snapshot`] worked out for that player.
//!
//! ```no_run
//! # fn main() -> Result<(), String> {
//! use jengine_server::{LiveOps, Server};
//! Server::new()
//!     .admin_token("a-long-secret")                  // the dashboard: /admin/liveops
//!     .liveops(LiveOps::open("liveops")?)            // key and pack archives under liveops/
//!     .run("0.0.0.0:9001")
//!     .map_err(|e| e.to_string())
//! # }
//! ```
//!
//! # What a channel holds
//!
//! - **Parameters** ([`Param`]): a remote config value, and [`Rule`]s that
//!   give some players another - by platform, app version, player or install
//!   id, a percentage rollout, or a time window. The first rule that matches
//!   wins.
//! - **Experiments** ([`Experiment`]): variants with weights, each giving its
//!   own values for some parameters. An install is put in one variant by a
//!   hash of its id and the experiment's, so it stays there for as long as
//!   the experiment runs, and the game is told which ([`Snapshot::experiments`]).
//! - **Live events** ([`LiveEvent`]): a window of time with values of its own
//!   and, optionally, packs that exist only while it runs - a weekend
//!   double-coins event, a seasonal map.
//! - **A content release** ([`Release`]): which asset packs are current - the
//!   archives the service stores and serves (`/liveops/v1/packs/<file>`),
//!   named by their hash so a CDN can cache them forever.
//!
//! A value is decided in that order and the later one wins: the parameter,
//! its first matching rule, the experiment variant, then the live events on
//! now (the one that started last on top).
//!
//! # Why it is signed
//!
//! A release can carry scripts, so a game believes a snapshot only if it is
//! signed by the key the game was built with ([`jengine_liveops`]). The
//! service makes that key the first time it opens its folder and keeps it
//! there (`signing.key`); `/liveops/v1/key` and the dashboard show the
//! public half, which goes into the game's `settings.liveops.public_key`.
//! Lose the folder and every shipped game stops believing the service - so
//! back it up.
//!
//! # Replacing the key (L3)
//!
//! A game can instead be built with a **root** key that never sits on the
//! serving machine: the root *endorses* the service's own key for a while
//! (`endorsement.json` in the folder, `jengine-server liveops-key endorse`),
//! every snapshot carries that endorsement, and the game believes what the
//! endorsed key signs until it runs out ([`jengine_liveops::Endorsement`]).
//! A leaked serving key is then good only until its endorsement ends, and a
//! lost one is replaced by endorsing a new one - neither needs a new build.
//! An existing service moves to it without one too: its old `signing.key`
//! becomes the root (move it off the machine), and endorses a new one.
//! The dashboard warns a fortnight before an endorsement runs out.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use jengine_liveops::{Content, Endorsement, EventWindow, PackManifest, PublicKey, Query, SNAPSHOT_VERSION, SecretKey, Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::store::Store;

/// The store namespace channel documents live in. Key: the channel name.
pub const NS_LIVEOPS: &str = "liveops";
/// Release history: key `<channel>` -> a JSON list.
pub const NS_RELEASES: &str = "liveops_releases";
/// Pack archives the service holds: key the archive's file name.
pub const NS_BLOBS: &str = "liveops_blobs";

/// Releases kept per channel, for a rollback.
pub const MAX_RELEASES: usize = 50;
/// Where each channel's earlier configurations are kept.
pub const NS_CONFIG_HISTORY: &str = "liveops_config_history";
/// Experiment results: key `<channel>|<experiment>|<variant>|<what>`,
/// value `{"count","sum"}`.
pub const NS_RESULTS: &str = "liveops_results";
/// Configurations kept per channel.
pub const MAX_CONFIGS: usize = 50;
/// An archive no kept release names is deleted only once it is this old, so
/// one uploaded for a release about to be published is not taken first.
pub const BLOB_GRACE_SECS: u64 = 24 * 3600;

/// Turn live ops on with [`crate::Server::liveops`].
#[derive(Clone)]
pub struct LiveOps {
    dir: PathBuf,
    key: SecretKey,
    channels: Vec<String>,
    refresh_secs: u64,
    cdn_url: String,
    /// From `endorsement.json`: the key games are built with, and the
    /// endorsements from it down to `key`. `None`: games trust `key` itself.
    endorsed: Option<Endorsed>,
}

/// `endorsement.json`: `{"root": "jlo-public-..", "chain": [..]}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endorsed {
    /// The key games are built with.
    pub root: String,
    /// From the root down to the service's own key.
    pub chain: Vec<Endorsement>,
}

impl Endorsed {
    /// Whether the chain leads from `root` to `key`, every link signed.
    pub fn check(&self, key: &PublicKey) -> Result<(), String> {
        let mut signer = PublicKey::parse(&self.root).map_err(|e| format!("its root: {e}"))?;
        if self.chain.is_empty() || self.chain.len() > jengine_liveops::MAX_CHAIN {
            return Err(format!("a chain of 1 to {} endorsements is believed", jengine_liveops::MAX_CHAIN));
        }
        for e in &self.chain {
            if !signer.verify_bytes(&Endorsement::message(&e.key, e.not_after), &e.signature) {
                return Err(format!("the endorsement of {} is not signed by {}", e.key, signer.to_text()));
            }
            signer = PublicKey::parse(&e.key).map_err(|e| e.to_string())?;
        }
        if signer != *key {
            return Err(format!("it ends at {}, and the service signs with {}", signer.to_text(), key.to_text()));
        }
        Ok(())
    }

    /// When the first link runs out (Unix seconds), if any does.
    pub fn ends(&self) -> Option<u64> {
        self.chain.iter().map(|e| e.not_after).filter(|t| *t > 0).min()
    }
}

impl LiveOps {
    /// Live ops kept in `dir`: its signing key (`signing.key`, made the first
    /// time and kept), and the pack archives it serves (`blobs/`). Channels
    /// `dev`, `staging` and `live`; games are told to ask again every five
    /// minutes.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, String> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(dir.join("blobs")).map_err(|e| format!("{}: {e}", dir.display()))?;
        let key_path = dir.join("signing.key");
        let key = match std::fs::read_to_string(&key_path) {
            Ok(text) => SecretKey::parse(&text).map_err(|e| format!("{}: {e}", key_path.display()))?,
            Err(_) => {
                let key = new_key();
                std::fs::write(&key_path, key.to_text()).map_err(|e| format!("{}: {e}", key_path.display()))?;
                log::warn!(
                    "[liveops] made a new signing key in {} - back it up: a game built with its public key ({}) believes nothing else",
                    key_path.display(),
                    key.public().to_text()
                );
                key
            }
        };
        // An endorsement, if the key games trust is not this one.
        let endorsed = match std::fs::read_to_string(dir.join(ENDORSEMENT_FILE)) {
            Ok(text) => {
                let at = dir.join(ENDORSEMENT_FILE);
                let endorsed: Endorsed = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", at.display()))?;
                endorsed.check(&key.public()).map_err(|why| format!("{}: {why}", at.display()))?;
                if let Some(ends) = endorsed.ends() {
                    log::info!("[liveops] signing as {}, endorsed by {} until {ends}", key.public().to_text(), endorsed.root);
                }
                Some(endorsed)
            }
            Err(_) => None,
        };
        Ok(LiveOps {
            dir,
            key,
            channels: vec!["dev".into(), "staging".into(), "live".into()],
            refresh_secs: 300,
            cdn_url: String::new(),
            endorsed,
        })
    }

    /// The channels, in order - typically the way a change is promoted.
    pub fn channels(mut self, names: &[&str]) -> Self {
        self.channels = names.iter().filter(|n| valid_id(n)).map(|n| n.to_string()).collect();
        self
    }

    /// How often a game asks again when nothing is scheduled sooner.
    pub fn refresh_secs(mut self, secs: u64) -> Self {
        self.refresh_secs = secs.max(10);
        self
    }

    /// Where games fetch pack archives from, if not this service: a CDN in
    /// front of `/liveops/v1/packs/`. Archive names carry their hash, so it
    /// may cache them forever.
    pub fn cdn_url(mut self, url: &str) -> Self {
        self.cdn_url = url.to_string();
        self
    }

    /// The channels, joined - for a log line.
    pub fn channel_list(&self) -> String {
        self.channels.join(", ")
    }

    /// The key games are built with: the root, when the service's own key
    /// is endorsed by one.
    pub fn public_key(&self) -> String {
        match &self.endorsed {
            Some(e) => e.root.clone(),
            None => self.key.public().to_text(),
        }
    }

    /// The key this service signs with - the root's, or one it endorsed.
    pub fn serving_key(&self) -> String {
        self.key.public().to_text()
    }

    /// When the endorsement of the serving key runs out, if it is endorsed
    /// and runs out at all.
    pub fn endorsement_ends(&self) -> Option<u64> {
        self.endorsed.as_ref().and_then(Endorsed::ends)
    }
}

/// The file in a live ops folder that endorses its key.
pub const ENDORSEMENT_FILE: &str = "endorsement.json";

/// `jengine-server liveops-key endorse`: `root` says `key` speaks for it
/// for `days` (0 for ever), written as the [`Endorsed`] a live ops folder
/// keeps beside its signing key.
pub fn endorse(root: &SecretKey, key: &PublicKey, days: f64) -> Endorsed {
    let not_after = if days > 0.0 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
            + (days * 86400.0) as u64
    } else {
        0
    };
    Endorsed { root: root.public().to_text(), chain: vec![root.endorse(key, not_after)] }
}

/// A fresh signing key from the OS's randomness.
pub fn new_key() -> SecretKey {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("the OS has randomness");
    SecretKey::from_seed(seed)
}

/// One remote config value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Param {
    /// What every game gets unless a rule, experiment or event says else.
    pub value: Value,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Checked in order; the first that matches gives its value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default)]
    pub when: Condition,
    pub value: Value,
}

/// Who and when. Every part given must hold; an empty condition always does.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// `windows`, `linux`, `macos`, `android`, `ios`, `web`, `editor`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<String>,
    /// The game's version at least this (`1.4`, `1.4.2`), compared number by number.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub min_version: String,
    /// And at most this.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub max_version: String,
    /// Player or install ids - testers, a support case.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub players: Vec<String>,
    /// This percentage of installs (0-100), the same ones every time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout: Option<f64>,
    /// From this Unix time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<u64>,
    /// Until this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<u64>,
    /// The game's language, as the engine names its locale: `ja` takes
    /// `ja` and `ja-JP`. Asked as `&lang=` (L5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
    /// Regions, as the game says where it is (`eu`, `jp`, a country code):
    /// `&region=`, any case.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<String>,
    /// Attributes the game sends of its own (`&a.level=21&a.tier=gold`):
    /// each named here must be one of the values listed - `"level_band":
    /// ["20-29"]`. An attribute the game did not send matches nothing.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, Vec<String>>,
    /// Put a rollout's share by the **signed-in player** when the game names
    /// one, rather than by install - so a player is in or out on every
    /// device, and cannot change it by reinstalling (L11).
    #[serde(default, skip_serializing_if = "is_false")]
    pub by_player: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// What a game says of itself beside a [`Query`] - read from the snapshot
/// URL's `lang`, `region` and `a.<name>` parameters, which an engine that
/// predates them simply does not send.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Asked {
    pub language: String,
    pub region: String,
    pub attributes: BTreeMap<String, String>,
}

/// The id a share is bucketed by: the signed-in player when asked for and
/// there is one, else the install.
fn bucket_id<'a>(q: &'a Query, by_player: bool) -> &'a str {
    if by_player && !q.player.is_empty() { &q.player } else { &q.install }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentStatus {
    /// Written, assigning nobody.
    #[default]
    Draft,
    /// Assigning and changing values.
    Running,
    /// Over: values back to normal, nobody assigned.
    Stopped,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Experiment {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default)]
    pub status: ExperimentStatus,
    /// Who is in it at all; the rest see no variant.
    #[serde(default)]
    pub audience: Condition,
    pub variants: Vec<Variant>,
    /// Put players in variants by the signed-in player when the game names
    /// one, not by install - for a test a player should not be able to
    /// leave by reinstalling (L11). Changing it mid-test moves people.
    #[serde(default, skip_serializing_if = "is_false")]
    pub by_player: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    pub name: String,
    /// Its share, against the other variants' (any positive numbers).
    pub weight: f64,
    /// Parameter values for installs in it.
    #[serde(default)]
    pub values: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LiveEvent {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Unix seconds, the service's clock; on from `start` until `end`.
    pub start: u64,
    pub end: u64,
    /// Seconds from one start to the next, for an event that comes round
    /// again - `604800` is every week, at the same hour of the service's
    /// clock (UTC: a summer-time change moves it an hour for those who
    /// keep one). `0` is once.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub every: u64,
    /// No occurrence starts at or after this, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<u64>,
    /// Parameter values while it is on.
    #[serde(default)]
    pub values: Map<String, Value>,
    /// Packs of the release that exist only while it is on - a seasonal
    /// map. A pack no event names is always there.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packs: Vec<String>,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

impl LiveEvent {
    /// The occurrence `k` (0 is the first): its start and end.
    fn occurrence(&self, k: u64) -> Option<(u64, u64)> {
        let shift = k.checked_mul(self.every)?;
        let (start, end) = (self.start.checked_add(shift)?, self.end.checked_add(shift)?);
        (k == 0 || self.every > 0).then_some(())?;
        self.until.is_none_or(|u| start < u).then_some((start, end))
    }

    /// The occurrence on at `now`, if one is.
    pub fn on_at(&self, now: u64) -> Option<(u64, u64)> {
        if now < self.start {
            return None;
        }
        let k = if self.every > 0 { (now - self.start) / self.every } else { 0 };
        self.occurrence(k).filter(|(s, e)| *s <= now && now < *e)
    }

    /// The first occurrence to start after `now`.
    pub fn next_after(&self, now: u64) -> Option<(u64, u64)> {
        if now < self.start {
            return self.occurrence(0);
        }
        if self.every == 0 {
            return None;
        }
        self.occurrence((now - self.start) / self.every + 1)
    }
}

/// A channel as it was before a save: what `restore` puts back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedConfig {
    pub revision: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub updated_by: String,
    #[serde(flatten)]
    pub edit: ChannelEdit,
}

/// A published content release.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub number: u64,
    pub manifest: PackManifest,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub published_at: u64,
    #[serde(default)]
    pub published_by: String,
}

/// Everything one channel holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    /// Goes up with every change; a game refuses a snapshot older than one
    /// it already has.
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub params: BTreeMap<String, Param>,
    #[serde(default)]
    pub experiments: Vec<Experiment>,
    #[serde(default)]
    pub events: Vec<LiveEvent>,
    #[serde(default)]
    pub release: Option<Release>,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub updated_by: String,
}

/// The parts of a channel [`Service::promote`] copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Config,
    Experiments,
    Events,
    Content,
}

/// A pack archive the service holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Blob {
    pub file: String,
    pub sha256: String,
    pub size: u64,
    pub uploaded_at: u64,
}

/// Ids name things in URLs and code: letters, digits, `_ - .`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// What is wrong with a channel an operator wants to save, or `Ok`.
pub fn validate(channel: &Channel) -> Result<(), String> {
    for (key, p) in &channel.params {
        if !valid_id(key) {
            return Err(format!("'{key}' is not a parameter name (letters, digits, _ - .)"));
        }
        for r in &p.rules {
            check_condition(&r.when).map_err(|e| format!("{key}: rule '{}': {e}", r.name))?;
        }
    }
    let mut seen = Vec::new();
    for x in &channel.experiments {
        if !valid_id(&x.id) || seen.contains(&&x.id) {
            return Err(format!("experiment '{}': ids are unique names of letters, digits, _ - .", x.id));
        }
        seen.push(&x.id);
        check_condition(&x.audience).map_err(|e| format!("experiment '{}': {e}", x.id))?;
        if x.variants.is_empty() {
            return Err(format!("experiment '{}' has no variants", x.id));
        }
        let mut names = Vec::new();
        for v in &x.variants {
            if v.name.is_empty() || names.contains(&&v.name) {
                return Err(format!("experiment '{}': each variant needs a name of its own", x.id));
            }
            names.push(&v.name);
            if !(v.weight.is_finite() && v.weight > 0.0) {
                return Err(format!("experiment '{}': variant '{}' needs a weight above 0", x.id, v.name));
            }
        }
    }
    let mut seen = Vec::new();
    for e in &channel.events {
        if !valid_id(&e.id) || seen.contains(&&e.id) {
            return Err(format!("event '{}': ids are unique names of letters, digits, _ - .", e.id));
        }
        seen.push(&e.id);
        if e.end <= e.start {
            return Err(format!("event '{}' ends before it starts", e.id));
        }
        if e.every > 0 && e.end - e.start > e.every {
            return Err(format!("event '{}' lasts longer than it takes to come round again", e.id));
        }
    }
    Ok(())
}

fn check_condition(c: &Condition) -> Result<(), String> {
    if let Some(r) = c.rollout
        && !(0.0..=100.0).contains(&r)
    {
        return Err(format!("a rollout is a percentage, not {r}"));
    }
    if let (Some(a), Some(b)) = (c.after, c.before)
        && b <= a
    {
        return Err("'before' is not after 'after'".into());
    }
    Ok(())
}

/// Where `seed` falls in 0..1, the same every time: rollouts and
/// experiments bucket installs by it.
pub fn bucket(seed: &str) -> f64 {
    use sha2::Digest;
    let h = sha2::Sha256::digest(seed.as_bytes());
    let n = u64::from_be_bytes(h[..8].try_into().unwrap());
    (n >> 11) as f64 / (1u64 << 53) as f64
}

/// `a` against `b`, number by number (`1.10` is after `1.9`); a missing
/// part is 0 and anything after a part's digits is ignored.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        match a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0)) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// Whether `c` holds for `q` at `now`. `salt` keeps one rollout's buckets
/// apart from another's.
pub fn matches(c: &Condition, q: &Query, now: u64, salt: &str) -> bool {
    matches_asked(c, q, &Asked::default(), now, salt)
}

/// [`matches`], with what the game said of itself beside its query.
pub fn matches_asked(c: &Condition, q: &Query, asked: &Asked, now: u64, salt: &str) -> bool {
    use std::cmp::Ordering::*;
    if !c.languages.is_empty() {
        let lang = asked.language.to_ascii_lowercase();
        let base = lang.split(['-', '_']).next().unwrap_or("");
        let fits = c.languages.iter().any(|l| {
            let l = l.to_ascii_lowercase();
            l == lang || (!base.is_empty() && l == base)
        });
        if !fits {
            return false;
        }
    }
    if !c.regions.is_empty() && !c.regions.iter().any(|r| r.eq_ignore_ascii_case(&asked.region)) {
        return false;
    }
    for (name, values) in &c.attributes {
        let Some(given) = asked.attributes.get(name) else { return false };
        if !values.iter().any(|v| v == given) {
            return false;
        }
    }
    if !c.platforms.is_empty() && !c.platforms.iter().any(|p| p.eq_ignore_ascii_case(&q.platform)) {
        return false;
    }
    if !c.min_version.is_empty() && compare_versions(&q.app_version, &c.min_version) == Less {
        return false;
    }
    if !c.max_version.is_empty() && compare_versions(&q.app_version, &c.max_version) == Greater {
        return false;
    }
    if !c.players.is_empty() && !c.players.iter().any(|p| (!q.player.is_empty() && *p == q.player) || *p == q.install) {
        return false;
    }
    if let Some(pct) = c.rollout
        && bucket(&format!("{salt}:{}", bucket_id(q, c.by_player))) * 100.0 >= pct
    {
        return false;
    }
    if c.after.is_some_and(|t| now < t) || c.before.is_some_and(|t| now >= t) {
        return false;
    }
    true
}

/// The variant `install` is in, by weight.
pub fn assign<'a>(x: &'a Experiment, install: &str) -> Option<&'a Variant> {
    let total: f64 = x.variants.iter().map(|v| v.weight.max(0.0)).sum();
    if total <= 0.0 {
        return None;
    }
    let mut at = bucket(&format!("experiment:{}:{install}", x.id)) * total;
    for v in &x.variants {
        at -= v.weight.max(0.0);
        if at < 0.0 {
            return Some(v);
        }
    }
    x.variants.last()
}

/// The snapshot `channel` gives `q` at `now` - everything decided, nothing
/// signed. `packs_url` is where a release's archives are fetched from when
/// its manifest names nowhere.
pub fn resolve(name: &str, channel: &Channel, q: &Query, now: u64, refresh_secs: u64, packs_url: &str) -> Snapshot {
    resolve_asked(name, channel, q, &Asked::default(), now, refresh_secs, packs_url)
}

/// [`resolve`], with what the game said of itself beside its query.
pub fn resolve_asked(
    name: &str,
    channel: &Channel,
    q: &Query,
    asked: &Asked,
    now: u64,
    refresh_secs: u64,
    packs_url: &str,
) -> Snapshot {
    let mut config = Map::new();
    for (key, p) in &channel.params {
        let value = p.rules.iter().find(|r| matches_asked(&r.when, q, asked, now, key)).map_or(&p.value, |r| &r.value);
        config.insert(key.clone(), value.clone());
    }
    let mut experiments = BTreeMap::new();
    for x in channel.experiments.iter().filter(|x| x.status == ExperimentStatus::Running) {
        if !matches_asked(&x.audience, q, asked, now, &format!("audience:{}", x.id)) {
            continue;
        }
        if let Some(v) = assign(x, bucket_id(q, x.by_player)) {
            experiments.insert(x.id.clone(), v.name.clone());
            for (k, val) in &v.values {
                config.insert(k.clone(), val.clone());
            }
        }
    }
    // Each event as the occurrence that is on now, or the next to come.
    let mut on: Vec<(&LiveEvent, (u64, u64))> =
        channel.events.iter().filter_map(|e| Some((e, e.on_at(now)?))).collect();
    on.sort_by_key(|(_, (start, _))| *start);
    for (e, _) in &on {
        for (k, v) in &e.values {
            config.insert(k.clone(), v.clone());
        }
    }
    let window = |e: &LiveEvent, (start, end): (u64, u64)| EventWindow { id: e.id.clone(), name: e.name.clone(), start, end };
    let mut upcoming: Vec<(&LiveEvent, (u64, u64))> =
        channel.events.iter().filter_map(|e| Some((e, e.next_after(now)?))).collect();
    upcoming.sort_by_key(|(_, (start, _))| *start);
    // Ask again when something scheduled starts or ends.
    let ends = on.iter().map(|(_, (_, end))| *end);
    let starts = upcoming.iter().map(|(_, (start, _))| *start);
    let mut next = ends.chain(starts).filter(|&t| t > now).min();
    for p in channel.params.values() {
        for r in &p.rules {
            for t in [r.when.after, r.when.before].into_iter().flatten() {
                if t > now {
                    next = Some(next.map_or(t, |n: u64| n.min(t)));
                }
            }
        }
    }
    let content = channel.release.as_ref().map(|r| {
        let mut manifest = r.manifest.clone();
        // A pack an event names is there only while one of its events is.
        manifest.packs.retain(|p| {
            let gated: Vec<&LiveEvent> = channel.events.iter().filter(|e| e.packs.contains(&p.name)).collect();
            gated.is_empty() || gated.iter().any(|e| on.iter().any(|(o, _)| o == e))
        });
        if manifest.url.is_empty() {
            manifest.url = packs_url.to_string();
        }
        Content { release: r.number, manifest }
    });
    Snapshot {
        version: SNAPSHOT_VERSION,
        channel: name.to_string(),
        install: q.install.clone(),
        issued_at: now,
        revision: channel.revision,
        config,
        experiments,
        events: on.iter().map(|(e, w)| window(e, *w)).collect(),
        upcoming: upcoming.iter().take(20).map(|(e, w)| window(e, *w)).collect(),
        content,
        refresh_secs,
        next_change: next,
    }
}

/// Live ops at work: the channels in the store, the archives on disk, the key.
pub struct Service {
    config: LiveOps,
    store: Arc<dyn Store>,
    /// One change at a time: a save is read, checked against its revision
    /// and written back under this.
    write: Mutex<()>,
    /// Archive file name -> its blob, as the store has it.
    blobs: Mutex<HashMap<String, Blob>>,
}

impl Service {
    pub fn new(config: LiveOps, store: Arc<dyn Store>) -> Self {
        let blobs = store
            .keys(NS_BLOBS)
            .into_iter()
            .filter_map(|k| Some((k.clone(), serde_json::from_str::<Blob>(&store.load(NS_BLOBS, &k)?).ok()?)))
            .collect();
        Service { config, store, write: Mutex::new(()), blobs: Mutex::new(blobs) }
    }

    pub fn channels(&self) -> &[String] {
        &self.config.channels
    }

    pub fn has_channel(&self, name: &str) -> bool {
        self.config.channels.iter().any(|c| c == name)
    }

    pub fn public_key(&self) -> String {
        self.config.public_key()
    }

    /// The key snapshots are signed with (see [`LiveOps::serving_key`]).
    pub fn serving_key(&self) -> String {
        self.config.serving_key()
    }

    /// When the serving key's endorsement runs out.
    pub fn endorsement_ends(&self) -> Option<u64> {
        self.config.endorsement_ends()
    }

    pub fn refresh_secs(&self) -> u64 {
        self.config.refresh_secs
    }

    /// Where a release's archives come from when its manifest names nowhere:
    /// the CDN if there is one, else this service at `base`.
    pub fn packs_url(&self, base: &str) -> String {
        if self.config.cdn_url.is_empty() {
            jengine_liveops::packs_url(base)
        } else {
            let u = self.config.cdn_url.clone();
            if u.ends_with('/') { u } else { format!("{u}/") }
        }
    }

    pub fn channel(&self, name: &str) -> Channel {
        self.store.load(NS_LIVEOPS, name).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// The snapshot for `q`, signed.
    pub fn snapshot(&self, name: &str, q: &Query, now: u64, base: &str) -> jengine_liveops::Envelope {
        self.snapshot_asked(name, q, &Asked::default(), now, base)
    }

    /// [`Service::snapshot`], with what the game said of itself.
    pub fn snapshot_asked(&self, name: &str, q: &Query, asked: &Asked, now: u64, base: &str) -> jengine_liveops::Envelope {
        let snapshot = resolve_asked(name, &self.channel(name), q, asked, now, self.config.refresh_secs, &self.packs_url(base));
        match &self.config.endorsed {
            Some(e) => self.config.key.sign_endorsed(&snapshot, &e.chain),
            None => self.config.key.sign(&snapshot),
        }
    }

    fn save(&self, name: &str, channel: &Channel) {
        self.store.save(NS_LIVEOPS, name, &serde_json::to_string(channel).expect("a channel serialises"));
    }

    /// L6: a game says an install saw its variant (and, optionally, did
    /// something worth counting - `event`, with a `value` to sum: a purchase
    /// and its price). Counted only for a running experiment and the
    /// variant the service itself puts that install in, so a game's slip -
    /// or a stranger's - cannot fill another variant's column.
    pub fn expose(&self, channel: &str, q: &Query, experiment: &str, event: &str, value: f64) -> Result<String, String> {
        if !event.is_empty() && !valid_id(event) {
            return Err("an event is letters, digits, _ - .".into());
        }
        let c = self.channel(channel);
        let x = c
            .experiments
            .iter()
            .find(|x| x.id == experiment && x.status == ExperimentStatus::Running)
            .ok_or_else(|| format!("no running experiment '{experiment}' on {channel}"))?;
        let variant = assign(x, bucket_id(q, x.by_player)).map(|v| v.name.clone()).ok_or("the experiment has no variants")?;
        let what = if event.is_empty() { "exposed" } else { event };
        let key = format!("{channel}|{experiment}|{variant}|{what}");
        let _one = self.write.lock().unwrap();
        let mut counted: Value = self.store.load(NS_RESULTS, &key).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        counted["count"] = json!(counted["count"].as_u64().unwrap_or(0) + 1);
        counted["sum"] = json!(counted["sum"].as_f64().unwrap_or(0.0) + if value.is_finite() { value } else { 0.0 });
        self.store.save(NS_RESULTS, &key, &counted.to_string());
        Ok(variant)
    }

    /// What has been counted for a channel's experiments: experiment ->
    /// variant -> what (`exposed` or an event) -> `{count, sum}`.
    pub fn results(&self, channel: &str) -> Value {
        let mut out = Map::new();
        let prefix = format!("{channel}|");
        for key in self.store.keys(NS_RESULTS) {
            let Some(rest) = key.strip_prefix(&prefix) else { continue };
            let parts: Vec<&str> = rest.splitn(3, '|').collect();
            let [experiment, variant, what] = parts[..] else { continue };
            let counted: Value = self.store.load(NS_RESULTS, &key).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
            let x = out.entry(experiment.to_string()).or_insert_with(|| json!({}));
            x[variant][what] = counted;
        }
        Value::Object(out)
    }

    /// Replace a channel's parameters, experiments and events - not its
    /// release - if `expected` is still its revision (else someone else
    /// saved first, and their change would be lost). Returns the new one.
    pub fn update(&self, name: &str, expected: u64, edit: ChannelEdit, by: &str, now: u64) -> Result<Channel, UpdateError> {
        let _lock = self.write.lock().unwrap();
        let mut c = self.channel(name);
        if c.revision != expected {
            return Err(UpdateError::Stale(c.revision));
        }
        let next = Channel { params: edit.params, experiments: edit.experiments, events: edit.events, ..c.clone() };
        validate(&next).map_err(UpdateError::Invalid)?;
        self.remember(name, &c);
        c = Channel { revision: c.revision + 1, updated_at: now, updated_by: by.to_string(), ..next };
        self.save(name, &c);
        Ok(c)
    }

    /// Keep what a channel was, before it is changed.
    fn remember(&self, name: &str, c: &Channel) {
        if c.revision == 0 {
            return;
        }
        let mut kept = self.config_history(name);
        kept.push(SavedConfig {
            revision: c.revision,
            updated_at: c.updated_at,
            updated_by: c.updated_by.clone(),
            edit: ChannelEdit { params: c.params.clone(), experiments: c.experiments.clone(), events: c.events.clone() },
        });
        let drop = kept.len().saturating_sub(MAX_CONFIGS);
        kept.drain(..drop);
        self.store.save(NS_CONFIG_HISTORY, name, &serde_json::to_string(&kept).expect("configs serialise"));
    }

    /// What `name` was before each of its last saves, oldest first: its
    /// parameters, experiments and events as of each revision.
    pub fn config_history(&self, name: &str) -> Vec<SavedConfig> {
        self.store.load(NS_CONFIG_HISTORY, name).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// Put `name`'s configuration back as it was at `revision` - as a new
    /// save, so it is in the history too and can itself be undone.
    pub fn restore(&self, name: &str, revision: u64, by: &str, now: u64) -> Result<Channel, UpdateError> {
        let Some(old) = self.config_history(name).into_iter().find(|s| s.revision == revision) else {
            return Err(UpdateError::Invalid(format!("{name} keeps no revision {revision}")));
        };
        let current = self.channel(name).revision;
        self.update(name, current, old.edit, by, now)
    }

    /// Publish a release on `name`: every archive it names must be here,
    /// with the hash and size it says.
    pub fn publish(&self, name: &str, manifest: PackManifest, notes: &str, by: &str, now: u64) -> Result<Release, String> {
        {
            let blobs = self.blobs.lock().unwrap();
            for p in &manifest.packs {
                match blobs.get(&p.file) {
                    Some(b) if b.sha256 == p.sha256 && b.size == p.size => {}
                    Some(_) => return Err(format!("{} is here with another hash or size than the release says", p.file)),
                    None => return Err(format!("{} has not been uploaded", p.file)),
                }
            }
        }
        let mut names = Vec::new();
        for p in &manifest.packs {
            if names.contains(&&p.name) {
                return Err(format!("two packs are called '{}'", p.name));
            }
            names.push(&p.name);
        }
        let _lock = self.write.lock().unwrap();
        let mut history = self.releases(name);
        let mut c = self.channel(name);
        let number = history.iter().map(|r| r.number).max().unwrap_or(0).max(c.release.as_ref().map_or(0, |r| r.number)) + 1;
        let release = Release { number, manifest, notes: notes.to_string(), published_at: now, published_by: by.to_string() };
        history.push(release.clone());
        let drop = history.len().saturating_sub(MAX_RELEASES);
        history.drain(..drop);
        self.store.save(NS_RELEASES, name, &serde_json::to_string(&history).expect("releases serialise"));
        c.release = Some(release.clone());
        c.revision += 1;
        c.updated_at = now;
        c.updated_by = by.to_string();
        self.save(name, &c);
        Ok(release)
    }

    /// Publish release `number`'s packs again, as a new release.
    pub fn rollback(&self, name: &str, number: u64, by: &str, now: u64) -> Result<Release, String> {
        let old = self.releases(name).into_iter().find(|r| r.number == number).ok_or_else(|| format!("{name} has no release {number}"))?;
        self.publish(name, old.manifest, &format!("rollback to release {number}"), by, now)
    }

    /// Every release kept for `name`, oldest first.
    pub fn releases(&self, name: &str) -> Vec<Release> {
        self.store.load(NS_RELEASES, name).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// Copy `parts` of channel `from` over `to`.
    pub fn promote(&self, from: &str, to: &str, parts: &[Part], by: &str, now: u64) -> Result<Channel, String> {
        if from == to {
            return Err("a channel cannot be promoted onto itself".into());
        }
        let source = self.channel(from);
        if parts.contains(&Part::Content)
            && let Some(r) = source.release.clone()
        {
            self.publish(to, r.manifest, &format!("promoted from {from} release {}", r.number), by, now)?;
        }
        let _lock = self.write.lock().unwrap();
        let mut c = self.channel(to);
        if parts.contains(&Part::Config) {
            c.params = source.params.clone();
        }
        if parts.contains(&Part::Experiments) {
            c.experiments = source.experiments.clone();
        }
        if parts.contains(&Part::Events) {
            c.events = source.events.clone();
        }
        self.remember(to, &self.channel(to));
        c.revision += 1;
        c.updated_at = now;
        c.updated_by = by.to_string();
        self.save(to, &c);
        Ok(c)
    }

    /// Delete every archive no kept release names - on any channel, current
    /// or in its history (so a rollback still finds its packs) - once it is
    /// `BLOB_GRACE_SECS` old. Returns the files deleted.
    pub fn collect_archives(&self, now: u64) -> Vec<String> {
        let mut named: HashSet<String> = HashSet::new();
        for channel in &self.config.channels {
            let current = self.channel(channel).release.into_iter();
            for release in current.chain(self.releases(channel)) {
                named.extend(release.manifest.packs.into_iter().map(|p| p.file));
            }
        }
        let mut blobs = self.blobs.lock().unwrap();
        let doomed: Vec<Blob> = blobs
            .values()
            .filter(|b| !named.contains(&b.file) && now.saturating_sub(b.uploaded_at) >= BLOB_GRACE_SECS)
            .cloned()
            .collect();
        for b in &doomed {
            blobs.remove(&b.file);
            self.store.delete(NS_BLOBS, &b.file);
            // The bytes, unless another name shares them.
            if !blobs.values().any(|o| o.sha256 == b.sha256) {
                let _ = std::fs::remove_file(self.blob_path(&b.sha256));
            }
        }
        doomed.into_iter().map(|b| b.file).collect()
    }

    /// Keep a pack archive. `file` is its name in a manifest; the bytes must
    /// hash to `sha256`. The same archive again is a no-op.
    pub fn put_blob(&self, file: &str, sha256: &str, bytes: &[u8], now: u64) -> Result<Blob, String> {
        use sha2::Digest;
        let tmp = self.upload_path();
        std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let got = jengine_liveops::to_hex(&sha2::Sha256::digest(bytes));
        self.put_blob_file(file, sha256, &tmp, &got, bytes.len() as u64, now)
    }

    /// Somewhere to write an archive as it arrives, beside where it is kept.
    pub fn upload_path(&self) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = self.config.dir.join("blobs");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(format!("upload-{}-{}.part", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
    }

    /// Keep an archive already written to `tmp` (by a streamed upload, which
    /// hashed it as it came: `got`, `size`) - moved into place if it hashes
    /// to `sha256`, removed if not or if the same archive is already here.
    pub fn put_blob_file(&self, file: &str, sha256: &str, tmp: &Path, got: &str, size: u64, now: u64) -> Result<Blob, String> {
        let refuse = |why: String| {
            let _ = std::fs::remove_file(tmp);
            Err(why)
        };
        if !valid_id(file) || !file.ends_with(".jpak") {
            return refuse(format!("'{file}' is not an archive name"));
        }
        if got != sha256 {
            return refuse(format!("{file} hashes to {got}, not {sha256}"));
        }
        let path = self.blob_path(sha256);
        if path.is_file() {
            let _ = std::fs::remove_file(tmp);
        } else if let Err(e) = std::fs::rename(tmp, &path) {
            return refuse(format!("{}: {e}", path.display()));
        }
        let blob = Blob { file: file.to_string(), sha256: sha256.to_string(), size, uploaded_at: now };
        self.store.save(NS_BLOBS, file, &serde_json::to_string(&blob).expect("a blob serialises"));
        self.blobs.lock().unwrap().insert(file.to_string(), blob.clone());
        Ok(blob)
    }

    /// The archive called `file`, if it is here.
    pub fn blob(&self, file: &str) -> Option<(Blob, PathBuf)> {
        let b = self.blobs.lock().unwrap().get(file).cloned()?;
        let path = self.blob_path(&b.sha256);
        path.is_file().then_some((b, path))
    }

    pub fn blobs(&self) -> Vec<Blob> {
        let mut v: Vec<Blob> = self.blobs.lock().unwrap().values().cloned().collect();
        v.sort_by(|a, b| a.file.cmp(&b.file));
        v
    }

    fn blob_path(&self, sha256: &str) -> PathBuf {
        self.config.dir.join("blobs").join(sha256)
    }
}

/// What an operator saves of a channel: everything but its release, which
/// changes only by publishing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChannelEdit {
    #[serde(default)]
    pub params: BTreeMap<String, Param>,
    #[serde(default)]
    pub experiments: Vec<Experiment>,
    #[serde(default)]
    pub events: Vec<LiveEvent>,
}

/// Why [`Service::update`] did not save.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateError {
    /// Somebody saved first; this is the channel's revision now.
    Stale(u64),
    Invalid(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use jengine_liveops::PackEntry;
    use serde_json::json;

    fn q(install: &str) -> Query {
        Query { install: install.into(), player: String::new(), platform: "windows".into(), app_version: "1.2.0".into() }
    }

    fn param(value: Value, rules: Vec<Rule>) -> Param {
        Param { value, description: String::new(), rules }
    }

    #[test]
    fn the_first_matching_rule_wins() {
        let mut c = Channel::default();
        c.params.insert(
            "coins".into(),
            param(
                json!(10),
                vec![
                    Rule { name: "phones".into(), when: Condition { platforms: vec!["android".into(), "ios".into()], ..Default::default() }, value: json!(20) },
                    Rule { name: "new builds".into(), when: Condition { min_version: "1.2".into(), ..Default::default() }, value: json!(30) },
                    Rule { name: "everyone".into(), when: Condition::default(), value: json!(40) },
                ],
            ),
        );
        let s = resolve("live", &c, &q("a"), 0, 300, "");
        assert_eq!(s.config["coins"], json!(30), "windows 1.2.0: not a phone, but a new build");
        let mut old = q("a");
        old.app_version = "1.1.9".into();
        assert_eq!(resolve("live", &c, &old, 0, 300, "").config["coins"], json!(40));
        let mut phone = q("a");
        phone.platform = "Android".into();
        assert_eq!(resolve("live", &c, &phone, 0, 300, "").config["coins"], json!(20), "platform names ignore case");
    }

    /// L5 and L11: a rule can ask the game's language, region and its own
    /// attributes; a share can follow the signed-in player across installs.
    #[test]
    fn language_region_attributes_and_the_player_decide_too() {
        let asked = |lang: &str, region: &str, attrs: &[(&str, &str)]| Asked {
            language: lang.into(),
            region: region.into(),
            attributes: attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        };
        let japanese = Condition { languages: vec!["ja".into()], ..Default::default() };
        assert!(matches_asked(&japanese, &q("a"), &asked("ja-JP", "", &[]), 0, "k"));
        assert!(matches_asked(&japanese, &q("a"), &asked("ja", "", &[]), 0, "k"));
        assert!(!matches_asked(&japanese, &q("a"), &asked("en-US", "", &[]), 0, "k"));
        assert!(!matches_asked(&japanese, &q("a"), &Asked::default(), 0, "k"), "an engine that says nothing is not Japanese");
        let eu = Condition { regions: vec!["EU".into()], ..Default::default() };
        assert!(matches_asked(&eu, &q("a"), &asked("", "eu", &[]), 0, "k"));
        let veterans = Condition { attributes: BTreeMap::from([("band".to_string(), vec!["20-29".to_string(), "30+".to_string()])]), ..Default::default() };
        assert!(matches_asked(&veterans, &q("a"), &asked("", "", &[("band", "30+")]), 0, "k"));
        assert!(!matches_asked(&veterans, &q("a"), &asked("", "", &[("band", "1-9")]), 0, "k"));
        assert!(!matches_asked(&veterans, &q("a"), &Asked::default(), 0, "k"));

        // By player: the same answer on two installs of one player.
        let half = Condition { rollout: Some(50.0), by_player: true, ..Default::default() };
        let player = |install: &str, who: &str| Query { player: who.into(), ..q(install) };
        for who in ["p1", "p2", "p3", "p4", "p5", "p6"] {
            let on = |i: &str| matches(&half, &player(i, who), 0, "k");
            assert_eq!(on("phone"), on("tablet"), "{who} is in or out everywhere");
        }
        let varied = (0..40).filter(|n| matches(&half, &q(&format!("i{n}")), 0, "k")).count();
        assert!(varied > 5 && varied < 35, "with no player, by install as before: {varied}");
    }

    /// L6: exposures and events are counted for the variant the service put
    /// the install in, and nowhere else.
    #[test]
    fn what_installs_saw_and_did_is_counted_by_variant() {
        let (s, dir) = service();
        let edit = ChannelEdit {
            experiments: vec![Experiment {
                id: "price".into(),
                status: ExperimentStatus::Running,
                variants: vec![
                    Variant { name: "low".into(), weight: 1.0, values: Map::new() },
                    Variant { name: "high".into(), weight: 1.0, values: Map::new() },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        s.update("live", 0, edit, "ops", 1).unwrap();
        let mut seen = std::collections::HashMap::<String, u64>::new();
        for n in 0..20 {
            let v = s.expose("live", &q(&format!("i{n}")), "price", "", 0.0).unwrap();
            *seen.entry(v).or_default() += 1;
        }
        s.expose("live", &q("i3"), "price", "purchase", 4.99).unwrap();
        s.expose("live", &q("i3"), "price", "purchase", 0.99).unwrap();
        let results = s.results("live");
        let total: u64 = ["low", "high"].iter().map(|v| results["price"][v]["exposed"]["count"].as_u64().unwrap_or(0)).sum();
        assert_eq!(total, 20);
        let v3 = s.snapshot("live", &q("i3"), 2, "").payload;
        let v3: Snapshot = serde_json::from_str(&v3).unwrap();
        let bought = &results["price"][&v3.experiments["price"]]["purchase"];
        assert_eq!(bought["count"], 2);
        assert!((bought["sum"].as_f64().unwrap() - 5.98).abs() < 1e-9);
        assert!(s.expose("live", &q("i3"), "nope", "", 0.0).is_err(), "only a running experiment counts");
        assert!(s.expose("live", &q("i3"), "price", "bad|event", 0.0).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn versions_compare_number_by_number() {
        use std::cmp::Ordering::*;
        assert_eq!(compare_versions("1.10", "1.9"), Greater);
        assert_eq!(compare_versions("1.2", "1.2.0"), Equal);
        assert_eq!(compare_versions("2.0.1-beta", "2.0.1"), Equal);
        assert_eq!(compare_versions("0.9", "1"), Less);
    }

    #[test]
    fn a_rollout_takes_its_share_and_keeps_it() {
        let c = Condition { rollout: Some(25.0), ..Default::default() };
        let installs: Vec<String> = (0..4000).map(|i| format!("install-{i}")).collect();
        let inside = installs.iter().filter(|i| matches(&c, &q(i), 0, "coins")).count();
        assert!((850..1150).contains(&inside), "about a quarter: {inside} of 4000");
        assert!(installs.iter().all(|i| matches(&c, &q(i), 0, "coins") == matches(&c, &q(i), 0, "coins")), "the same every time");
        let other = installs.iter().filter(|i| matches(&c, &q(i), 0, "coins") && matches(&c, &q(i), 0, "gems")).count();
        assert!(other < inside / 2, "two rollouts pick different installs: {other} in both");
        assert!(!matches(&Condition { rollout: Some(0.0), ..Default::default() }, &q("x"), 0, "k"));
        assert!(matches(&Condition { rollout: Some(100.0), ..Default::default() }, &q("x"), 0, "k"));
    }

    #[test]
    fn players_and_time_windows() {
        let c = Condition { players: vec!["tester-1".into()], after: Some(100), before: Some(200), ..Default::default() };
        let mut tester = q("install-9");
        tester.player = "tester-1".into();
        assert!(matches(&c, &tester, 150, ""));
        assert!(!matches(&c, &tester, 99, ""), "before the window");
        assert!(!matches(&c, &tester, 200, ""), "the window has ended");
        assert!(!matches(&c, &q("install-9"), 150, ""), "somebody else");
        assert!(matches(&c, &q("tester-1"), 150, ""), "an install id names one too");
    }

    #[test]
    fn experiments_split_by_weight_and_override_values() {
        let mut c = Channel::default();
        c.params.insert("price".into(), param(json!(100), vec![]));
        c.experiments.push(Experiment {
            id: "price_test".into(),
            status: ExperimentStatus::Running,
            variants: vec![
                Variant { name: "control".into(), weight: 1.0, values: Map::new() },
                Variant { name: "cheap".into(), weight: 3.0, values: [("price".to_string(), json!(80))].into_iter().collect() },
            ],
            ..Default::default()
        });
        let mut cheap = 0;
        for i in 0..2000 {
            let s = resolve("live", &c, &q(&format!("i{i}")), 0, 300, "");
            let v = s.experiments["price_test"].clone();
            assert_eq!(s.config["price"], if v == "cheap" { json!(80) } else { json!(100) });
            cheap += (v == "cheap") as i32;
        }
        assert!((1350..1650).contains(&cheap), "three quarters cheap: {cheap}");
        let once = resolve("live", &c, &q("i7"), 0, 300, "").experiments["price_test"].clone();
        assert_eq!(resolve("live", &c, &q("i7"), 99, 300, "").experiments["price_test"], once, "an install keeps its variant");

        c.experiments[0].status = ExperimentStatus::Stopped;
        let s = resolve("live", &c, &q("i7"), 0, 300, "");
        assert!(s.experiments.is_empty() && s.config["price"] == json!(100), "a stopped experiment changes nothing");
        c.experiments[0].status = ExperimentStatus::Running;
        c.experiments[0].audience = Condition { platforms: vec!["ios".into()], ..Default::default() };
        assert!(resolve("live", &c, &q("i7"), 0, 300, "").experiments.is_empty(), "outside the audience");
    }

    #[test]
    fn live_events_switch_values_and_packs_on_and_off() {
        let mut c = Channel::default();
        c.params.insert("coins".into(), param(json!(10), vec![]));
        c.events.push(LiveEvent {
            id: "double".into(),
            name: "Double coins weekend".into(),
            start: 1000,
            end: 2000,
            values: [("coins".to_string(), json!(20))].into_iter().collect(),
            packs: vec!["halloween".into()],
            ..Default::default()
        });
        c.events.push(LiveEvent { id: "later".into(), start: 3000, end: 4000, ..Default::default() });
        let pack = |name: &str| PackEntry { name: name.into(), file: format!("{name}-0.jpak"), sha256: "0".into(), ..Default::default() };
        c.release = Some(Release {
            number: 3,
            manifest: PackManifest { version: 1, url: String::new(), packs: vec![pack("base"), pack("halloween")] },
            ..Default::default()
        });
        let packs = |s: &Snapshot| s.content.as_ref().unwrap().manifest.packs.iter().map(|p| p.name.clone()).collect::<Vec<_>>();

        let before = resolve("live", &c, &q("a"), 500, 300, "https://ops/liveops/v1/packs/");
        assert_eq!(before.config["coins"], json!(10));
        assert!(before.events.is_empty());
        assert_eq!(before.upcoming.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["double", "later"]);
        assert_eq!(before.next_change, Some(1000), "ask again when the event starts");
        assert_eq!(packs(&before), ["base"], "the event's pack is not there yet");
        assert_eq!(before.content.as_ref().unwrap().manifest.url, "https://ops/liveops/v1/packs/");

        let during = resolve("live", &c, &q("a"), 1500, 300, "");
        assert_eq!(during.config["coins"], json!(20));
        assert_eq!(during.events[0].id, "double");
        assert_eq!(during.next_change, Some(2000));
        assert_eq!(packs(&during), ["base", "halloween"]);

        let after = resolve("live", &c, &q("a"), 2000, 300, "");
        assert_eq!(after.config["coins"], json!(10), "an event ends at its end");
        assert_eq!(packs(&after), ["base"]);
    }

    #[test]
    fn an_event_beats_an_experiment_beats_a_rule() {
        let mut c = Channel::default();
        c.params.insert("k".into(), param(json!("default"), vec![Rule { when: Condition::default(), value: json!("rule"), ..Default::default() }]));
        assert_eq!(resolve("x", &c, &q("a"), 5, 0, "").config["k"], json!("rule"));
        c.experiments.push(Experiment {
            id: "x".into(),
            status: ExperimentStatus::Running,
            variants: vec![Variant { name: "only".into(), weight: 1.0, values: [("k".to_string(), json!("variant"))].into_iter().collect() }],
            ..Default::default()
        });
        assert_eq!(resolve("x", &c, &q("a"), 5, 0, "").config["k"], json!("variant"));
        c.events.push(LiveEvent { id: "e".into(), start: 0, end: 10, values: [("k".to_string(), json!("event"))].into_iter().collect(), ..Default::default() });
        assert_eq!(resolve("x", &c, &q("a"), 5, 0, "").config["k"], json!("event"));
    }

    #[test]
    fn a_bad_channel_is_refused() {
        let mut c = Channel::default();
        c.params.insert("has space".into(), Param::default());
        assert!(validate(&c).is_err());
        let mut c = Channel::default();
        c.events.push(LiveEvent { id: "e".into(), start: 10, end: 5, ..Default::default() });
        assert!(validate(&c).unwrap_err().contains("ends before"));
        let mut c = Channel::default();
        c.experiments.push(Experiment { id: "x".into(), variants: vec![Variant { name: "a".into(), weight: 0.0, ..Default::default() }], ..Default::default() });
        assert!(validate(&c).unwrap_err().contains("weight"));
        let mut c = Channel::default();
        c.params.insert("k".into(), param(json!(1), vec![Rule { when: Condition { rollout: Some(150.0), ..Default::default() }, value: json!(2), ..Default::default() }]));
        assert!(validate(&c).unwrap_err().contains("percentage"));
    }

    fn service() -> (Service, PathBuf) {
        let dir = std::env::temp_dir().join(format!("jengine-liveops-{}-{}", std::process::id(), next()));
        let _ = std::fs::remove_dir_all(&dir);
        let ops = LiveOps::open(&dir).unwrap();
        (Service::new(ops, Arc::new(crate::store::MemoryStore::new())), dir)
    }

    fn next() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }

    #[test]
    fn a_save_on_a_stale_revision_is_refused() {
        let (s, dir) = service();
        let mut params = BTreeMap::new();
        params.insert("k".to_string(), param(json!(1), vec![]));
        let c = s.update("dev", 0, ChannelEdit { params: params.clone(), ..Default::default() }, "ada", 1).unwrap();
        assert_eq!(c.revision, 1);
        assert_eq!(s.update("dev", 0, ChannelEdit { params, ..Default::default() }, "bob", 2), Err(UpdateError::Stale(1)), "bob saved over nothing he saw");
        assert_eq!(s.channel("dev").updated_by, "ada");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_release_needs_its_archives_and_rolls_back_as_a_new_release() {
        let (s, dir) = service();
        let bytes = b"JPAK fake archive".to_vec();
        let sha = {
            use sha2::Digest;
            jengine_liveops::to_hex(&sha2::Sha256::digest(&bytes))
        };
        let entry = PackEntry { name: "forest".into(), file: "forest-abc.jpak".into(), size: bytes.len() as u64, sha256: sha.clone(), files: vec![] };
        let manifest = PackManifest { version: 1, url: String::new(), packs: vec![entry.clone()] };
        assert!(s.publish("live", manifest.clone(), "", "ada", 1).unwrap_err().contains("not been uploaded"));
        assert!(s.put_blob("forest-abc.jpak", "00", &bytes, 1).unwrap_err().contains("hashes to"));
        assert!(s.put_blob("../evil.jpak", &sha, &bytes, 1).is_err());
        s.put_blob("forest-abc.jpak", &sha, &bytes, 1).unwrap();
        assert_eq!(std::fs::read(s.blob("forest-abc.jpak").unwrap().1).unwrap(), bytes);

        let r1 = s.publish("live", manifest.clone(), "first", "ada", 2).unwrap();
        let empty = PackManifest { version: 1, url: String::new(), packs: vec![] };
        let r2 = s.publish("live", empty, "oops", "ada", 3).unwrap();
        assert_eq!((r1.number, r2.number), (1, 2));
        let r3 = s.rollback("live", 1, "ada", 4).unwrap();
        assert_eq!(r3.number, 3, "a rollback is a new release, so games take it");
        assert_eq!(r3.manifest, manifest);
        assert_eq!(s.channel("live").revision, 3);

        // And promoting carries the release and the config.
        let mut params = BTreeMap::new();
        params.insert("k".to_string(), param(json!(1), vec![]));
        s.update("live", 3, ChannelEdit { params, ..Default::default() }, "ada", 5).unwrap();
        let staged = s.promote("live", "staging", &[Part::Config, Part::Content], "ada", 6).unwrap();
        assert_eq!(staged.params["k"].value, json!(1));
        assert_eq!(staged.release.unwrap().manifest, manifest);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_key_is_made_once_and_kept() {
        let (s, dir) = service();
        let again = LiveOps::open(&dir).unwrap();
        assert_eq!(s.public_key(), again.public_key());
        let env = s.snapshot("live", &q("a"), 10, "http://x");
        let pk = jengine_liveops::PublicKey::parse(&s.public_key()).unwrap();
        assert_eq!(pk.verify(&env).unwrap().install, "a");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// L3: the key games are built with stays off the serving machine; the
    /// service signs with a key it endorsed, and replacing that key is a new
    /// endorsement, not a new build. An endorsement that does not lead to
    /// the service's key is refused at start.
    #[test]
    fn a_service_signs_with_a_key_its_root_endorsed() {
        let (s, dir) = service();
        let first_key = s.public_key();
        // The old key becomes the root: what games are already built with.
        let root = SecretKey::parse(&std::fs::read_to_string(dir.join("signing.key")).unwrap()).unwrap();
        std::fs::remove_file(dir.join("signing.key")).unwrap();
        let fresh = LiveOps::open(&dir).unwrap();
        assert_ne!(fresh.public_key(), first_key, "a new serving key, not yet endorsed");
        let serving = PublicKey::parse(&fresh.serving_key()).unwrap();
        let endorsed = endorse(&root, &serving, 90.0);
        std::fs::write(dir.join(ENDORSEMENT_FILE), serde_json::to_string(&endorsed).unwrap()).unwrap();

        let ops = LiveOps::open(&dir).unwrap();
        assert_eq!(ops.public_key(), first_key, "games are told the root, as before");
        assert!(ops.endorsement_ends().unwrap() > 80 * 86400);
        let s = Service::new(ops, Arc::new(crate::store::MemoryStore::new()));
        let env = s.snapshot("live", &q("a"), 10, "http://x");
        assert_eq!(env.chain.len(), 1);
        let game = PublicKey::parse(&first_key).unwrap();
        assert_eq!(game.verify(&env).unwrap().install, "a", "a game built before the change believes it");

        // An endorsement of somebody else's key is not one to start with.
        let other = new_key();
        let wrong = endorse(&root, &other.public(), 90.0);
        std::fs::write(dir.join(ENDORSEMENT_FILE), serde_json::to_string(&wrong).unwrap()).unwrap();
        assert!(LiveOps::open(&dir).err().unwrap().contains("ends at"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// L8: an event that comes round again is on in each occurrence and off
    /// between them, says its next one, and stops at `until`; one lasting
    /// longer than its period is refused.
    #[test]
    fn a_weekly_event_comes_round_again() {
        let week = 7 * 86400;
        let mut c = Channel::default();
        c.params.insert("coins".into(), param(json!(10), vec![]));
        c.events.push(LiveEvent {
            id: "weekend".into(),
            start: 1000,
            end: 1000 + 2 * 86400,
            every: week,
            until: Some(1000 + 3 * week),
            values: [("coins".to_string(), json!(20))].into_iter().collect(),
            ..Default::default()
        });
        let at = |now: u64| resolve("live", &c, &q("i"), now, 300, "");
        assert_eq!(at(1000 + 3600).config["coins"], json!(20), "the first weekend");
        assert_eq!(at(1000 + 3 * 86400).config["coins"], json!(10), "midweek");
        let midweek = at(1000 + 3 * 86400);
        assert_eq!((midweek.upcoming[0].start, midweek.next_change), (1000 + week, Some(1000 + week)), "next weekend");
        let second = at(1000 + week + 60);
        assert_eq!(second.config["coins"], json!(20), "the second weekend");
        assert_eq!((second.events[0].start, second.events[0].end), (1000 + week, 1000 + week + 2 * 86400));
        assert_eq!(second.next_change, Some(1000 + week + 2 * 86400), "asked again when it ends");
        assert_eq!(at(1000 + 2 * week + 60).config["coins"], json!(20), "the third");
        assert_eq!(at(1000 + 3 * week + 60).config["coins"], json!(10), "and no fourth: until");
        assert!(at(1000 + 3 * week + 60).upcoming.is_empty());
        c.events[0].end = c.events[0].start + week + 1;
        assert!(validate(&c).is_err(), "an event that overlaps its own next");
    }

    /// L7: every save keeps what the channel was, and an earlier revision
    /// can be put back - as a save of its own, so the putting back can be
    /// undone too.
    #[test]
    fn a_config_is_kept_at_every_revision_and_can_be_put_back() {
        let (s, dir) = service();
        let edit = |price: i64| ChannelEdit {
            params: [("price".to_string(), param(json!(price), vec![]))].into_iter().collect(),
            ..Default::default()
        };
        let r1 = s.update("live", 0, edit(100), "ada", 10).unwrap().revision;
        let r2 = s.update("live", r1, edit(80), "bo", 20).unwrap().revision;
        s.update("live", r2, edit(5), "cy", 30).unwrap();
        let kept = s.config_history("live");
        assert_eq!(kept.iter().map(|k| k.revision).collect::<Vec<_>>(), vec![r1, r2]);
        assert_eq!((kept[1].updated_by.as_str(), kept[1].edit.params["price"].value.clone()), ("bo", json!(80)));
        let back = s.restore("live", r1, "ada", 40).unwrap();
        assert_eq!(back.params["price"].value, json!(100), "the price before the mistake");
        assert_eq!(back.updated_by, "ada");
        assert_eq!(s.config_history("live").last().unwrap().edit.params["price"].value, json!(5), "and the mistake is kept too");
        assert!(s.restore("live", 999, "ada", 50).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// L10: an archive no kept release names is deleted once it is old
    /// enough; one a release names, or one just uploaded, is not.
    #[test]
    fn archives_no_release_names_are_collected() {
        use sha2::Digest;
        let (s, dir) = service();
        let put = |file: &str, bytes: &[u8], at: u64| {
            let sha = jengine_liveops::to_hex(&sha2::Sha256::digest(bytes));
            s.put_blob(file, &sha, bytes, at).unwrap()
        };
        let kept = put("maps-1.jpak", b"maps", 0);
        put("old-1.jpak", b"old", 0);
        put("fresh-1.jpak", b"fresh", BLOB_GRACE_SECS);
        let entry = PackEntry { name: "maps".into(), file: kept.file.clone(), sha256: kept.sha256.clone(), size: kept.size, ..Default::default() };
        s.publish("live", PackManifest { packs: vec![entry], ..Default::default() }, "", "ada", 1).unwrap();
        let gone = s.collect_archives(BLOB_GRACE_SECS + 10);
        assert_eq!(gone, vec!["old-1.jpak".to_string()]);
        assert!(s.blob("maps-1.jpak").is_some() && s.blob("fresh-1.jpak").is_some());
        assert!(s.blob("old-1.jpak").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
