//! **Live ops in the game** (Oct 2026): remote config, the experiments this
//! install is in, the live events on now, and the content release - asked of
//! a live ops service (`jengine-server`'s `liveops`) and believed only if
//! signed by the key the game was built with.
//!
//! A project turns it on in `settings.liveops` (Project Settings -> Live Ops):
//!
//! ```json
//! "liveops": {
//!   "url": "https://ops.example.com",
//!   "channel": "live",
//!   "editor_channel": "dev",
//!   "public_key": "jlo-public-...",
//!   "defaults": { "coins_per_win": 10, "welcome": "Hello" }
//! }
//! ```
//!
//! **The defaults ship with the game**, so a value is there from the first
//! frame - before the service answers, offline, or with live ops off - and a
//! script reads it the same way whatever answered: `LiveOps.get("welcome")`.
//!
//! **Every answer is kept.** The last snapshot the game believed is cached
//! (natively beside the saves, in the browser in `localStorage`) and believed
//! again at the next start - signature checked again - so a game opened on a
//! plane still has last week's prices and this weekend's event. A snapshot
//! older than the one the game has is refused, so an old answer replayed at
//! it cannot undo a change; so is one made for another install or channel.
//!
//! The state is the process's, as `asset::packs`' is: a script asks here
//! (`LiveOps.fetch`, `set_player`), and each host runs [`native::sync`] /
//! [`web::sync`] once a frame, which fetches when a fetch is due - at start,
//! every `refresh_secs`, when something scheduled starts or ends, and when a
//! script asks. A content release is laid over the build's packs
//! ([`crate::asset::packs::Packs::apply_live`]) - except in the editor, whose
//! packs are its project's own files.
//!
//! **What the game says of itself**: every ask carries the player's
//! language ([`crate::locale::current`], asked again when it changes), the
//! region (the system locale's, `FI` of `fi-FI`, unless
//! `settings.liveops.region` names one), and the game's own attributes -
//! `settings.liveops.attributes` and whatever a script sets with
//! `LiveOps.set_attribute` - which the service's rules can target
//! (`languages`, `regions`, `attributes`). And `LiveOps.expose(experiment,
//! event, value)` tells it an install saw its variant (no event) or did
//! something worth counting in it: a POST queued here and sent by the same
//! per-frame sync, so no host wires anything.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

pub use jengine_liveops::{Content, Envelope, EventWindow, PublicKey, Query, Snapshot};

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web;

/// How long after a failed fetch the next one is tried.
pub const RETRY_SECS: f64 = 60.0;

/// The most exposures waiting to be sent; past it the oldest go (a host
/// that never syncs must not grow without end).
pub const MAX_OUTBOX: usize = 256;

/// `settings.liveops`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LiveOpsSettings {
    /// The service's address (`https://ops.example.com`). Empty: off, and
    /// every value is its default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// What a built game asks for. Empty is `live`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub channel: String,
    /// What the editor's Play asks for. Empty is `dev`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub editor_channel: String,
    /// The service's public key (`jlo-public-...`): nothing it did not sign
    /// is believed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub public_key: String,
    /// Every value a script may ask for, as the game ships it.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub defaults: Map<String, Value>,
    /// The region rules see (`FI`). Empty: the system locale's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub region: String,
    /// The game's own attributes rules can target (`"store": "steam"`),
    /// sent as text; a script adds more with `LiveOps.set_attribute`.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub attributes: Map<String, Value>,
}

impl LiveOpsSettings {
    /// `liveops` out of a project's `settings` object; empty if absent or
    /// malformed (logged).
    pub fn from_settings(settings: &Value) -> Self {
        // `null` is absent too: the web runtime reads `settings` into a
        // struct and hands `liveops` back as `null` when the project has none.
        let Some(v) = settings.get("liveops").filter(|v| !v.is_null()) else { return Self::default() };
        serde_json::from_value(v.clone()).unwrap_or_else(|e| {
            log::warn!("[liveops] settings.liveops is not valid: {e}");
            Self::default()
        })
    }

    /// The channel a built game asks for.
    pub fn game_channel(&self) -> String {
        if self.channel.is_empty() { "live".into() } else { self.channel.clone() }
    }

    /// The channel the editor's Play asks for.
    pub fn editor_channel(&self) -> String {
        if self.editor_channel.is_empty() { "dev".into() } else { self.editor_channel.clone() }
    }
}

/// Where the values a script reads came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Source {
    /// Live ops are off (no `url`, or no usable key): the defaults.
    #[default]
    Off,
    /// On, and nothing believed yet: the defaults.
    Defaults,
    /// The snapshot believed last time, from the cache.
    Cached,
    /// A snapshot fetched this session.
    Live,
}

impl Source {
    /// `LiveOps.status`.
    pub fn name(&self) -> &'static str {
        match self {
            Source::Off => "off",
            Source::Defaults => "defaults",
            Source::Cached => "cached",
            Source::Live => "live",
        }
    }
}

/// An exposure on its way to the service: POST `body` (JSON) to `url`.
#[derive(Debug, Clone, PartialEq)]
pub struct Exposure {
    pub url: String,
    pub body: String,
}

/// A value as a query parameter: text as it is, a whole number without a
/// `.0` - the way a script's `3` and the browsers' `String(3)` both read.
pub fn attribute_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), number_text),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// `3.0` as `3`, `2.5` as `2.5`.
pub fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") }
}

/// The region in a language code: `FI` of `fi-FI`, `TW` of `zh-Hant-TW`,
/// `419` of `es-419`; "" when it names none.
pub fn region_of(code: &str) -> String {
    code.split(['-', '_'])
        .skip(1)
        .find(|t| (t.len() == 2 && t.chars().all(|c| c.is_ascii_alphabetic())) || (t.len() == 3 && t.chars().all(|c| c.is_ascii_digit())))
        .map(|t| t.to_ascii_uppercase())
        .unwrap_or_default()
}

/// The clock endorsements are held to: this machine's - in a browser,
/// `Date.now()`, since `PublicKey::verify` has no clock on wasm.
fn clock() -> Option<u64> {
    #[cfg(not(target_arch = "wasm32"))]
    return std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs());
    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    return Some((js_sys::Date::now() / 1000.0) as u64);
    #[cfg(all(target_arch = "wasm32", not(feature = "web")))]
    return None;
}

/// The game's live ops: what it was told, and when to ask again.
#[derive(Debug, Default)]
pub struct LiveOps {
    settings: LiveOpsSettings,
    key: Option<PublicKey>,
    channel: String,
    query: Query,
    /// Whether a release changes the packs - not in the editor.
    apply_content: bool,
    snapshot: Option<Snapshot>,
    source: Source,
    fetching: bool,
    /// A script asked for a fetch (or the player changed).
    wanted: bool,
    /// When the next fetch is due, by the local clock (0: now).
    next_at: f64,
    error: String,
    /// The service's clock minus this one, from the last fetch.
    offset: f64,
    generation: u64,
    /// The language the next ask says ([`follow_locale`](Self::follow_locale)).
    language: String,
    /// The `locale::stamp` the language was read at.
    locale_stamp: u64,
    region: String,
    /// What scripts set (`LiveOps.set_attribute`), over the settings'.
    script_attributes: BTreeMap<String, String>,
    /// Exposures not sent yet.
    outbox: Vec<Exposure>,
}

impl LiveOps {
    /// Set up from the project's settings. `channel` is the one to ask for
    /// (the game's, or the editor's); `install` this install's id; `platform`
    /// and `app_version` what targeting rules match. Anything believed
    /// before is forgotten.
    pub fn configure(&mut self, settings: LiveOpsSettings, channel: &str, install: &str, platform: &str, app_version: &str, apply_content: bool) {
        self.key = None;
        self.error.clear();
        self.source = Source::Off;
        if !settings.url.is_empty() {
            match PublicKey::parse(&settings.public_key) {
                Ok(k) => {
                    self.key = Some(k);
                    self.source = Source::Defaults;
                }
                Err(e) => {
                    self.error = format!("settings.liveops.public_key: {e} - live ops are off");
                    log::warn!("[liveops] {}", self.error);
                }
            }
        }
        self.channel = channel.to_string();
        self.query = Query { install: install.to_string(), player: self.query.player.clone(), platform: platform.to_string(), app_version: app_version.to_string() };
        self.apply_content = apply_content;
        self.settings = settings;
        self.snapshot = None;
        self.fetching = false;
        self.wanted = false;
        self.next_at = 0.0;
        self.offset = 0.0;
        self.generation += 1;
        self.region = if self.settings.region.is_empty() {
            crate::locale::system_language().map(|l| region_of(&l)).unwrap_or_default()
        } else {
            self.settings.region.to_ascii_uppercase()
        };
        // Read the language again at the next sync.
        self.locale_stamp = 0;
        self.outbox.clear();
    }

    /// Say `language` and `region` from now on (the per-frame sync reads the
    /// language from [`crate::locale`]); asks again if the language changed
    /// after something was asked.
    pub fn set_locale(&mut self, language: &str, region: &str) {
        if self.language != language && (self.snapshot.is_some() || self.next_at > 0.0) {
            self.wanted = true;
        }
        self.language = language.to_string();
        self.region = region.to_string();
    }

    /// Bring the language up to date with [`crate::locale`] - one stamp
    /// compared when nothing changed. Each host's sync calls it.
    pub fn follow_locale(&mut self) {
        if !self.is_on() {
            return;
        }
        let stamp = crate::locale::stamp();
        if stamp == self.locale_stamp {
            return;
        }
        self.locale_stamp = stamp;
        let language = crate::locale::current();
        let region = self.region.clone();
        self.set_locale(&language, &region);
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn region(&self) -> &str {
        &self.region
    }

    /// Every attribute the next ask carries: the settings', then the
    /// scripts' over them; an empty one is left out.
    pub fn attributes(&self) -> BTreeMap<String, String> {
        let mut out: BTreeMap<String, String> = self.settings.attributes.iter().map(|(k, v)| (k.clone(), attribute_text(v))).collect();
        out.extend(self.script_attributes.iter().map(|(k, v)| (k.clone(), v.clone())));
        out.retain(|_, v| !v.is_empty());
        out
    }

    /// `LiveOps.set_attribute(name, value)`: an attribute rules can target;
    /// "" takes it away. Asks again when that changes what is said.
    pub fn set_attribute(&mut self, name: &str, value: &str) {
        if name.is_empty() {
            return;
        }
        let before = self.attributes();
        self.script_attributes.insert(name.to_string(), value.to_string());
        if self.attributes() != before {
            self.wanted = true;
        }
    }

    /// The snapshot route as asked now: who, and what the game says of itself.
    pub fn snapshot_request(&self) -> String {
        let attributes = self.attributes();
        let pairs: Vec<(&str, &str)> = attributes.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        jengine_liveops::snapshot_url_with(&self.settings.url, &self.channel, &self.query, &self.language, &self.region, &pairs)
    }

    /// `LiveOps.expose(experiment, event, value)`: queue telling the service
    /// this install saw its variant of `experiment` (`event` empty) or did
    /// `event` in it, `value` summed. False when live ops are off or no
    /// experiment is named - nothing is sent then.
    pub fn expose(&mut self, experiment: &str, event: &str, value: f64) -> bool {
        if !self.is_on() || experiment.is_empty() {
            return false;
        }
        if self.outbox.len() >= MAX_OUTBOX {
            self.outbox.remove(0);
            log::warn!("[liveops] more than {MAX_OUTBOX} exposures waiting - the oldest is dropped");
        }
        let value = if value.is_finite() { value } else { 0.0 };
        let body = serde_json::json!({ "experiment": experiment, "event": event, "value": value });
        self.outbox.push(Exposure { url: jengine_liveops::exposure_url(&self.settings.url, &self.channel, &self.query), body: body.to_string() });
        true
    }

    /// The exposures to send now; the host's sync posts them.
    pub fn take_exposures(&mut self) -> Vec<Exposure> {
        std::mem::take(&mut self.outbox)
    }

    pub fn is_on(&self) -> bool {
        self.key.is_some()
    }

    /// The URL to fetch now, if a fetch is due and none is running.
    pub fn poll(&mut self, now: f64) -> Option<String> {
        if !self.is_on() || self.fetching || !(self.wanted || now >= self.next_at) {
            return None;
        }
        self.fetching = true;
        self.wanted = false;
        Some(self.snapshot_request())
    }

    /// What a fetch [`poll`](Self::poll) started brought back: the
    /// response's text, or why there is none. Returns the text to cache when
    /// it was believed.
    pub fn deliver(&mut self, result: Result<String, String>, now: f64) -> Option<String> {
        self.fetching = false;
        let text = match result {
            Ok(t) => t,
            Err(e) => {
                self.fail(format!("fetching the snapshot: {e}"), now);
                return None;
            }
        };
        let envelope: Envelope = match serde_json::from_str(&text) {
            Ok(e) => e,
            Err(e) => {
                // An error page, or a proxy's answer.
                let short: String = text.chars().take(120).collect();
                self.fail(format!("not a snapshot ({e}): {short}"), now);
                return None;
            }
        };
        match self.accept(&envelope, Source::Live, now) {
            Ok(()) => {
                self.error.clear();
                let s = self.snapshot.as_ref().expect("just believed");
                let mut wait = s.refresh_secs.max(10) as f64;
                if let Some(t) = s.next_change {
                    // The service's clock, turned into this one's.
                    wait = wait.min((t as f64 - self.offset - now).max(1.0));
                }
                self.next_at = now + wait;
                Some(text)
            }
            Err(e) => {
                self.fail(e, now);
                None
            }
        }
    }

    fn fail(&mut self, why: String, now: f64) {
        log::warn!("[liveops] {why}");
        self.error = why;
        let refresh = self.snapshot.as_ref().map_or(300, |s| s.refresh_secs.max(10)) as f64;
        self.next_at = now + RETRY_SECS.min(refresh);
    }

    /// Believe `envelope` if this game's key signed it, it is for this
    /// install and channel, and it is not older than what the game has.
    fn accept(&mut self, envelope: &Envelope, source: Source, now: f64) -> Result<(), String> {
        let key = self.key.ok_or("live ops are off")?;
        let s = key.verify_at(envelope, clock()).map_err(|e| e.to_string())?;
        if s.channel != self.channel {
            return Err(format!("the snapshot is for channel '{}', this game reads '{}'", s.channel, self.channel));
        }
        if s.install != self.query.install {
            return Err("the snapshot was made for another install".into());
        }
        if let Some(cur) = &self.snapshot
            && s.issued_at < cur.issued_at
        {
            return Err(format!("the snapshot is older ({}) than the one this game has ({})", s.issued_at, cur.issued_at));
        }
        if source == Source::Live {
            self.offset = s.issued_at as f64 - now;
        }
        if self.apply_content
            && let Some(c) = &s.content
        {
            crate::asset::packs::apply_live(c.release, &c.manifest);
        }
        self.snapshot = Some(s);
        self.source = source;
        self.generation += 1;
        Ok(())
    }

    /// Believe the snapshot cached last time (`text`, an [`Envelope`]), if it
    /// still checks out. Returns its content release, for the packs.
    pub fn load_cached(&mut self, text: &str, now: f64) -> Option<Content> {
        let envelope: Envelope = serde_json::from_str(text).ok()?;
        match self.accept(&envelope, Source::Cached, now) {
            Ok(()) => {
                log::info!("[liveops] believed the cached snapshot (revision {})", self.revision());
                self.snapshot.as_ref().and_then(|s| s.content.clone())
            }
            Err(e) => {
                log::info!("[liveops] the cached snapshot is not believed: {e}");
                None
            }
        }
    }

    /// `LiveOps.fetch()`: ask now.
    pub fn fetch(&mut self) {
        self.wanted = true;
    }

    /// `LiveOps.set_player(id)`: the signed-in player, whom rules can name -
    /// and ask again, since their values may differ.
    pub fn set_player(&mut self, id: &str) {
        if self.query.player != id {
            self.query.player = id.to_string();
            self.wanted = true;
        }
    }

    /// A value: the service's, or the game's default; `None` if neither.
    pub fn get(&self, key: &str) -> Option<Value> {
        self.snapshot.as_ref().and_then(|s| s.config.get(key)).or_else(|| self.settings.defaults.get(key)).cloned()
    }

    /// Every key there is a value for, sorted.
    pub fn keys(&self) -> Vec<String> {
        let mut k: Vec<String> = self.settings.defaults.keys().cloned().collect();
        if let Some(s) = &self.snapshot {
            k.extend(s.config.keys().cloned());
        }
        k.sort();
        k.dedup();
        k
    }

    pub fn source(&self) -> Source {
        self.source
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    pub fn fetching(&self) -> bool {
        self.fetching
    }

    /// The variant of `experiment` this install is in, or "".
    pub fn variant(&self, experiment: &str) -> String {
        self.snapshot.as_ref().and_then(|s| s.experiments.get(experiment).cloned()).unwrap_or_default()
    }

    /// The service's clock, as near as this one can tell.
    pub fn server_time(&self, now: f64) -> f64 {
        now + self.offset
    }

    fn windows(&self) -> impl Iterator<Item = &EventWindow> {
        self.snapshot.iter().flat_map(|s| s.events.iter().chain(s.upcoming.iter()))
    }

    /// The live events on at `now` - worked out here from their windows, so
    /// one starts and ends on time even before the game asks again.
    pub fn events(&self, now: f64) -> Vec<String> {
        let t = self.server_time(now);
        let mut ids: Vec<String> = self.windows().filter(|e| e.start as f64 <= t && t < e.end as f64).map(|e| e.id.clone()).collect();
        ids.dedup();
        ids
    }

    pub fn event_active(&self, id: &str, now: f64) -> bool {
        let t = self.server_time(now);
        self.windows().any(|e| e.id == id && e.start as f64 <= t && t < e.end as f64)
    }

    /// Seconds until event `id` ends, if it is on; 0 otherwise.
    pub fn event_ends_in(&self, id: &str, now: f64) -> f64 {
        let t = self.server_time(now);
        self.windows().find(|e| e.id == id && e.start as f64 <= t && t < e.end as f64).map_or(0.0, |e| e.end as f64 - t)
    }

    /// Seconds until event `id` starts, if it has not; 0 otherwise.
    pub fn event_starts_in(&self, id: &str, now: f64) -> f64 {
        let t = self.server_time(now);
        self.windows().find(|e| e.id == id && t < e.start as f64).map_or(0.0, |e| e.start as f64 - t)
    }

    /// An event's name, as the service calls it.
    pub fn event_name(&self, id: &str) -> String {
        self.windows().find(|e| e.id == id).map(|e| e.name.clone()).unwrap_or_default()
    }

    /// The snapshot's revision (0 with none).
    pub fn revision(&self) -> u64 {
        self.snapshot.as_ref().map_or(0, |s| s.revision)
    }

    /// The content release named (0 with none).
    pub fn release(&self) -> u64 {
        self.snapshot.as_ref().and_then(|s| s.content.as_ref()).map_or(0, |c| c.release)
    }

    /// Goes up whenever the values may have changed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn channel(&self) -> &str {
        &self.channel
    }

    pub fn install(&self) -> &str {
        &self.query.install
    }
}

static STATE: LazyLock<Mutex<LiveOps>> = LazyLock::new(|| Mutex::new(LiveOps::default()));

/// The process's live ops.
pub fn with<R>(f: impl FnOnce(&mut LiveOps) -> R) -> R {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut s)
}

/// The game's version, out of a project's `settings` (`app.version`).
pub fn app_version(settings: &Value) -> String {
    settings.get("app").and_then(|a| a.get("version")).and_then(Value::as_str).unwrap_or("").to_string()
}

/// The platform targeting rules match, for a native build.
pub fn native_platform() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "windows"
    }
}

/// What a script call gives back.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptAnswer {
    Nothing,
    Text(String),
    Number(f64),
    Bool(bool),
    List(Vec<String>),
    Json(Value),
}

/// [`script_call`] with one argument.
pub fn script_op(op: &str, arg: &str) -> ScriptAnswer {
    script_call(op, &[arg])
}

/// The one script-facing operation, shared by the Luau globals and the
/// browsers' `state.liveops(op, a, b, c)` hook so the two cannot answer
/// differently. Every argument arrives as text (a number as
/// [`number_text`] writes it, `nil` as ""). `get` answers
/// [`ScriptAnswer::Nothing`] for a key with no value, and the caller
/// returns the script's own default then.
pub fn script_call(op: &str, args: &[&str]) -> ScriptAnswer {
    let now = crate::profiling::unix_time();
    let arg = args.first().copied().unwrap_or("");
    let nth = |i: usize| args.get(i).copied().unwrap_or("");
    with(|l| match op {
        "get" => l.get(arg).map_or(ScriptAnswer::Nothing, ScriptAnswer::Json),
        "keys" => ScriptAnswer::List(l.keys()),
        "status" => ScriptAnswer::Text(l.source().name().into()),
        "error" => ScriptAnswer::Text(l.error().into()),
        "fetch" => {
            l.fetch();
            ScriptAnswer::Nothing
        }
        "fetching" => ScriptAnswer::Bool(l.fetching()),
        "variant" => ScriptAnswer::Text(l.variant(arg)),
        "events" => ScriptAnswer::List(l.events(now)),
        "event_active" => ScriptAnswer::Bool(l.event_active(arg, now)),
        "event_ends" => ScriptAnswer::Number(l.event_ends_in(arg, now)),
        "event_starts" => ScriptAnswer::Number(l.event_starts_in(arg, now)),
        "event_name" => ScriptAnswer::Text(l.event_name(arg)),
        "set_player" => {
            l.set_player(arg);
            ScriptAnswer::Nothing
        }
        "server_time" => ScriptAnswer::Number(l.server_time(now).floor()),
        "revision" => ScriptAnswer::Number(l.revision() as f64),
        "release" => ScriptAnswer::Number(l.release() as f64),
        "updated" => ScriptAnswer::Number(l.generation() as f64),
        "expose" => ScriptAnswer::Bool(l.expose(arg, nth(1), nth(2).trim().parse().unwrap_or(0.0))),
        "set_attribute" => {
            l.set_attribute(arg, nth(1));
            ScriptAnswer::Nothing
        }
        other => {
            log::warn!("[liveops] unknown op '{other}'");
            ScriptAnswer::Nothing
        }
    })
}

/// Every op [`script_op`] answers, by the script global that asks it.
pub const SCRIPT_GLOBALS: [(&str, &str); 19] = [
    ("liveops_get", "get"),
    ("liveops_keys", "keys"),
    ("liveops_status", "status"),
    ("liveops_error", "error"),
    ("liveops_fetch", "fetch"),
    ("liveops_fetching", "fetching"),
    ("liveops_variant", "variant"),
    ("liveops_events", "events"),
    ("liveops_event_active", "event_active"),
    ("liveops_event_ends", "event_ends"),
    ("liveops_event_starts", "event_starts"),
    ("liveops_event_name", "event_name"),
    ("liveops_set_player", "set_player"),
    ("liveops_server_time", "server_time"),
    ("liveops_revision", "revision"),
    ("liveops_release", "release"),
    ("liveops_updated", "updated"),
    ("liveops_expose", "expose"),
    ("liveops_set_attribute", "set_attribute"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use jengine_liveops::{PackEntry, PackManifest, SNAPSHOT_VERSION, SecretKey};
    use serde_json::json;

    fn key() -> SecretKey {
        SecretKey::from_seed([3; 32])
    }

    fn settings() -> LiveOpsSettings {
        LiveOpsSettings {
            url: "https://ops.example.com".into(),
            public_key: key().public().to_text(),
            defaults: [("coins".to_string(), json!(10)), ("welcome".to_string(), json!("Hi"))].into_iter().collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_null_liveops_setting_is_no_setting() {
        // What the web runtime hands over for a project without live ops.
        let s = LiveOpsSettings::from_settings(&json!({ "liveops": null }));
        assert!(s.url.is_empty());
    }

    fn snapshot(issued_at: u64, coins: i64) -> Snapshot {
        Snapshot {
            version: SNAPSHOT_VERSION,
            channel: "live".into(),
            install: "me".into(),
            issued_at,
            revision: issued_at,
            config: [("coins".to_string(), json!(coins))].into_iter().collect(),
            experiments: [("shop".to_string(), "list".to_string())].into_iter().collect(),
            events: vec![EventWindow { id: "weekend".into(), name: "Weekend".into(), start: 900, end: 1100 }],
            upcoming: vec![EventWindow { id: "later".into(), name: String::new(), start: 1500, end: 1600 }],
            refresh_secs: 300,
            next_change: Some(1100),
            ..Default::default()
        }
    }

    fn ops() -> LiveOps {
        let mut l = LiveOps::default();
        l.configure(settings(), "live", "me", "windows", "1.0", false);
        l
    }

    fn signed(s: &Snapshot) -> String {
        serde_json::to_string(&key().sign(s)).unwrap()
    }

    #[test]
    fn defaults_until_a_signed_snapshot_arrives() {
        let mut l = ops();
        assert_eq!(l.source(), Source::Defaults);
        assert_eq!(l.get("coins"), Some(json!(10)));
        assert_eq!(l.get("nothing"), None);
        let url = l.poll(1000.0).expect("a fetch at once");
        assert!(url.starts_with("https://ops.example.com/liveops/v1/live/snapshot?install=me&"), "{url}");
        assert!(l.poll(1000.0).is_none(), "one at a time");
        assert!(l.deliver(Ok(signed(&snapshot(1000, 25))), 1000.0).is_some());
        assert_eq!(l.source(), Source::Live);
        assert_eq!(l.get("coins"), Some(json!(25)));
        assert_eq!(l.get("welcome"), Some(json!("Hi")), "a key the service does not set keeps its default");
        assert_eq!(l.variant("shop"), "list");
        assert_eq!(l.keys(), vec!["coins".to_string(), "welcome".to_string()]);
        assert!(l.poll(1050.0).is_none());
        assert!(l.poll(1100.0).is_some(), "asked again when the event ends, before the refresh");
    }

    #[test]
    fn a_forged_stale_or_foreign_snapshot_is_refused() {
        let mut l = ops();
        l.poll(0.0);
        let mut env = key().sign(&snapshot(1000, 25));
        env.payload = env.payload.replace("25", "9999");
        assert!(l.deliver(Ok(serde_json::to_string(&env).unwrap()), 0.0).is_none());
        assert!(l.error().contains("signature"), "{}", l.error());
        assert_eq!(l.get("coins"), Some(json!(10)), "nothing believed");

        let other = SecretKey::from_seed([4; 32]).sign(&snapshot(1000, 25));
        l.poll(100.0);
        assert!(l.deliver(Ok(serde_json::to_string(&other).unwrap()), 100.0).is_none());

        l.poll(200.0);
        assert!(l.deliver(Ok(signed(&snapshot(1000, 25))), 200.0).is_some());
        l.poll(2000.0);
        assert!(l.deliver(Ok(signed(&snapshot(900, 1))), 2000.0).is_none(), "an older answer replayed");
        assert!(l.error().contains("older"));
        assert_eq!(l.get("coins"), Some(json!(25)));

        let mut theirs = snapshot(3000, 1);
        theirs.install = "someone-else".into();
        l.fetch();
        l.poll(2001.0);
        assert!(l.deliver(Ok(signed(&theirs)), 2001.0).is_none());
        l.fetch();
        l.poll(2002.0);
        assert!(l.deliver(Err("offline".into()), 2002.0).is_none());
        assert!(l.poll(2003.0).is_none(), "a failure waits before trying again");
        assert!(l.poll(2002.0 + RETRY_SECS).is_some());
    }

    #[test]
    fn events_switch_on_and_off_by_the_service_clock() {
        let mut l = ops();
        l.poll(0.0);
        // The service is 500 s ahead of this machine.
        l.deliver(Ok(signed(&snapshot(1000, 25))), 500.0);
        assert_eq!(l.server_time(500.0), 1000.0);
        assert!(l.event_active("weekend", 500.0));
        assert_eq!(l.event_ends_in("weekend", 500.0), 100.0);
        assert_eq!(l.events(500.0), vec!["weekend".to_string()]);
        assert_eq!(l.event_starts_in("later", 500.0), 500.0);
        assert!(!l.event_active("later", 500.0));
        assert!(l.event_active("later", 1000.0), "an upcoming event starts on time, without a fetch");
        assert!(!l.event_active("weekend", 700.0), "and one ends");
        assert_eq!(l.event_name("weekend"), "Weekend");
    }

    #[test]
    fn a_cached_snapshot_is_believed_at_start_and_brings_its_release() {
        let mut s = snapshot(1000, 25);
        s.content = Some(Content {
            release: 7,
            manifest: PackManifest { version: 1, url: "https://ops/p/".into(), packs: vec![PackEntry { name: "zz-cache-test".into(), ..Default::default() }] },
        });
        let mut l = ops();
        let content = l.load_cached(&signed(&s), 5.0).expect("its release");
        assert_eq!(content.release, 7);
        assert_eq!(l.source(), Source::Cached);
        assert_eq!(l.get("coins"), Some(json!(25)));
        assert_eq!(l.release(), 7);
        let mut tampered = ops();
        assert!(tampered.load_cached(&signed(&s).replace("25", "26"), 5.0).is_none());
        assert_eq!(tampered.source(), Source::Defaults, "an edited cache file is not believed");
    }

    #[test]
    fn without_a_url_or_a_key_it_is_off() {
        let mut l = LiveOps::default();
        l.configure(LiveOpsSettings { defaults: settings().defaults, ..Default::default() }, "live", "me", "windows", "1", false);
        assert_eq!(l.source(), Source::Off);
        assert!(l.poll(0.0).is_none());
        assert_eq!(l.get("coins"), Some(json!(10)));
        l.configure(LiveOpsSettings { url: "https://x".into(), public_key: "nonsense".into(), ..Default::default() }, "live", "me", "windows", "1", false);
        assert_eq!(l.source(), Source::Off);
        assert!(l.error().contains("public_key"));
    }

    #[test]
    fn an_ask_says_the_language_region_and_attributes() {
        let mut s = settings();
        s.region = "fi".into();
        s.attributes = [("store".to_string(), json!("steam")), ("level".to_string(), json!(3))].into_iter().collect();
        let mut l = LiveOps::default();
        l.configure(s, "live", "me", "windows", "1.0", false);
        assert_eq!(l.region(), "FI", "the settings' region, as the service writes regions");
        l.set_locale("ja-JP", "FI");
        let url = l.poll(0.0).unwrap();
        for part in ["&lang=ja-JP", "&region=FI", "&a.level=3", "&a.store=steam"] {
            assert!(url.contains(part), "{part} missing from {url}");
        }
        l.deliver(Ok(signed(&snapshot(1000, 25))), 0.0);
        assert!(l.poll(1.0).is_none());

        // An attribute from a script asks again, and "" takes one away.
        l.set_attribute("band", "20-29");
        let url = l.poll(1.0).expect("asked again");
        assert!(url.contains("&a.band=20-29"), "{url}");
        l.deliver(Ok(signed(&snapshot(1001, 25))), 1.0);
        l.set_attribute("store", "");
        assert!(!l.poll(2.0).unwrap().contains("a.store"));
        l.deliver(Ok(signed(&snapshot(1002, 25))), 2.0);
        l.set_attribute("band", "20-29");
        assert!(l.poll(3.0).is_none(), "the same value again changes nothing");

        // A new language asks again.
        l.set_locale("en", "FI");
        assert!(l.poll(3.0).expect("asked in English").contains("&lang=en&"));
    }

    #[test]
    fn regions_come_out_of_language_codes() {
        assert_eq!(region_of("fi-FI"), "FI");
        assert_eq!(region_of("zh-Hant-TW"), "TW");
        assert_eq!(region_of("es_419"), "419");
        assert_eq!(region_of("en"), "");
        assert_eq!((number_text(3.0), number_text(2.5), attribute_text(&json!(true))), ("3".into(), "2.5".into(), "true".into()));
    }

    #[test]
    fn an_exposure_is_queued_for_the_service() {
        let mut l = ops();
        l.set_player("ada");
        assert!(l.expose("shop", "", 0.0));
        assert!(l.expose("shop", "purchase", 2.5));
        assert!(!l.expose("", "purchase", 1.0), "no experiment, nothing to count");
        let sent = l.take_exposures();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].url, "https://ops.example.com/liveops/v1/live/exposure?install=me&player=ada");
        let body: Value = serde_json::from_str(&sent[1].body).unwrap();
        assert_eq!(body, json!({ "experiment": "shop", "event": "purchase", "value": 2.5 }));
        assert!(l.take_exposures().is_empty(), "each is sent once");
        for _ in 0..MAX_OUTBOX + 5 {
            l.expose("shop", "tick", 1.0);
        }
        assert_eq!(l.take_exposures().len(), MAX_OUTBOX, "a host that never syncs does not grow it forever");

        let mut off = LiveOps::default();
        off.configure(LiveOpsSettings::default(), "live", "me", "windows", "1", false);
        assert!(!off.expose("shop", "", 0.0), "live ops off: nothing to tell");
    }

    #[test]
    fn an_endorsement_is_held_to_the_clock() {
        // The game trusts a root; the service signs with a key the root
        // endorsed until t=1500. A snapshot issued at 1000 is believed while
        // the clock reads before 1500, refused after - the clock the web now
        // passes from `Date.now()` too.
        let serving = SecretKey::from_seed([5; 32]);
        let chain = [key().endorse(&serving.public(), 1500)];
        let env = serving.sign_endorsed(&snapshot(1000, 25), &chain);
        assert!(key().public().verify_at(&env, Some(1400)).is_ok());
        assert!(key().public().verify_at(&env, Some(1600)).is_err());
        let mut l = ops();
        l.poll(0.0);
        assert!(l.deliver(Ok(serde_json::to_string(&env).unwrap()), 0.0).is_none(), "this machine's clock is long past 1500");
        assert!(l.error().contains("run out"), "{}", l.error());
    }

    #[test]
    fn script_calls_take_several_arguments() {
        assert_eq!(script_call("expose", &[""]), ScriptAnswer::Bool(false));
        assert_eq!(script_call("set_attribute", &["x", "1"]), ScriptAnswer::Nothing);
    }

    #[test]
    fn a_new_player_asks_again() {
        let mut l = ops();
        l.poll(0.0);
        l.deliver(Ok(signed(&snapshot(1000, 25))), 0.0);
        assert!(l.poll(1.0).is_none());
        l.set_player("ada");
        let url = l.poll(1.0).expect("asked again");
        assert!(url.contains("player=ada"));
    }
}
