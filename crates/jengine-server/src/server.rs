//! The socket shell: connections, the room tasks, and the hub between them.
//!
//! # Who owns a player
//!
//! Exactly one task at a time. A connection task holds its [`Player`] while
//! the player is in no room; joining *moves* it into the room's task, and
//! leaving - however it happens - moves it back as a `Released` event. Nothing
//! is shared and nothing is locked around a player's state, so a room can
//! write to it on every tick, and "which copy is the true one" has one
//! answer.
//!
//! # Tasks
//!
//! One per connection, one per room, and a writer per socket. A room's task
//! is the only thing that touches its [`RoomRunner`], so a room's code is
//! single-threaded however many cores the server has; rooms run in parallel
//! with each other, which is where the cores go.
//!
//! # A room that panics loses nobody
//!
//! Each call into a room's code is caught. A panic closes that room and
//! releases its players with the state they had; the server and every other
//! room carry on.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::State;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot, watch};

use crate::cluster::{Cluster, ClusterConfig, RemoteStore};
use crate::relay::RelayRoom;
use crate::room::{Effects, Frame, JoinRequest, Player, PlayerId, Release, Room, RoomRunner};
use crate::store::{MemoryStore, NS_PLAYER, NS_TOKEN, STORE_DOWN, Store};

/// Limits and timings. The defaults suit a small game on one machine.
#[derive(Debug, Clone)]
pub struct Config {
    pub max_connections: usize,
    pub max_rooms: usize,
    /// Largest frame a client may send.
    pub max_frame_bytes: usize,
    /// Largest `send` a client may make in `part` frames - a message too big
    /// for one frame, cut up by the client (`jengine-online` does it by
    /// itself) and put back together here before the room sees it.
    pub max_message_bytes: usize,
    /// `send` frames a connection may make a second, with a burst of the same.
    pub messages_per_second: f64,
    /// Frames that may wait for a slow client before it is dropped.
    pub outbox: usize,
    /// Seconds between pings to a client that answers them (it says so in
    /// its `hello`, as `jengine-online` does): what a connection's round
    /// trip - and so `RoomCtx::delay` - is measured by.
    pub ping_secs: f64,
    /// How often a room saves the players in it whose state has changed.
    pub autosave_secs: f64,
    /// The kind a `join` with no `kind` means.
    pub default_kind: Option<String>,
    /// The secret an operator presents to the admin API and dashboard
    /// (`/admin`). `None` - the default - and there is no admin surface at
    /// all: the routes do not exist.
    pub admin_token: Option<String>,
    /// Whether `/stats` and `/metrics` answer anyone. They hold only counts,
    /// but counts say how many are playing and how busy the machine is, so
    /// **the default is `false`**: both want the admin token (a scraper sends
    /// it as a bearer). `true` (`Server::public_stats`, `--public-stats`)
    /// opens them. `/healthz` always answers - a load balancer has no token.
    pub public_stats: bool,
    /// Whether a `hello` carrying a password (signing in, registering, a
    /// recovery code) is taken over a connection that crossed the network
    /// unencrypted ([`crate::listen::arrived_in_clear`]). **`false` by
    /// default**: such a password could be read by anyone on the way, so it
    /// is refused with the reason and the client is told to use `wss://`.
    /// From this machine, or through a TLS proxy that says so, it is taken.
    pub allow_plain_passwords: bool,
    /// Whether the admin API (and private `/stats`, `/metrics`) takes a token
    /// that crossed the network unencrypted. **`false` by default**: from
    /// another machine the token must come over TLS (the server's own,
    /// `Server::tls`, or a TLS proxy it trusts), or it could be read on the
    /// way and replayed. From this machine - the editor, the fleet's agent -
    /// it is always taken.
    pub admin_over_plain: bool,
    /// Seconds between the samples the dashboard's charts are drawn from.
    /// Six hours of them are kept, and across a restart.
    pub history_secs: f64,
    /// The machine's memory use, in percent, at which a warning starts.
    pub warn_memory_percent: u64,
    /// The machine's CPU use, in percent, at which a warning starts.
    pub warn_cpu_percent: f32,
    /// Free space on the disk the server runs from, in percent, below which
    /// a warning starts - a store that cannot write is a server that cannot
    /// save.
    pub warn_disk_free_percent: u64,
    /// A URL that is sent a POST when a warning starts or stops, and again
    /// while one goes on (`warn_repeat_secs`):
    /// `{"server":"..","started":[..],"ended":[..],"still":[..]}`. Point it at
    /// a chat webhook or a pager, and nobody has to be watching the
    /// dashboard. A POST that fails is tried again, three times over a
    /// minute.
    pub warn_webhook: Option<String>,
    /// Seconds a warning may go on before it is said again (`still` in the
    /// webhook, and in the event log). `0` says it once.
    pub warn_repeat_secs: f64,
    /// The pauses before each new try of a webhook POST that failed.
    pub warn_retry_secs: Vec<f64>,
    /// Named operators, each with a token and a role. `admin_token` is
    /// shorthand for one called `admin` who may do everything.
    pub operators: Vec<Operator>,
    /// Requests one address may make to the game's `/api/` routes a second,
    /// with a burst of twice that. Past it the answer is 429. `0` is no limit.
    /// Behind a proxy the address is the one the proxy says it carried the
    /// request for - see `trusted_proxies`.
    pub api_requests_per_second: f64,
    /// Requests one **bearer token** - one signed-in caller - may make to the
    /// `/api/` routes a second, with a burst of twice that, whatever address
    /// it comes from: a player on a shared address (a school, a café) is not
    /// held to everyone else's budget there, and one token cannot spend
    /// several addresses' worth. `0` is no limit. Requests without a token
    /// are held to the address limit alone.
    pub api_token_requests_per_second: f64,
    /// The largest body an `/api/` request may carry, in bytes. Past it the
    /// answer is 413 and the route never runs.
    pub api_max_body_bytes: usize,
    /// Proxies on other machines whose word on who a request is from is
    /// believed (`X-Forwarded-For`'s last entry, else `X-Real-IP`) - a TLS
    /// proxy in front of the game port - and on how it was reached
    /// (`X-Forwarded-Proto`). This machine (a host's gateway) is always
    /// believed. The address a connection or an `/api/` caller is counted
    /// and shown as. The default is `JENGINE_TRUST_PROXY` from the
    /// environment (`198.51.100.90,10.0.0.2`).
    pub trusted_proxies: Vec<std::net::IpAddr>,
    /// Which web origins may call the game's `/api/` routes from a browser:
    /// `None` is every origin (`*`); a list is exactly those, each as the
    /// browser sends it (`https://game.example`). Nothing rides on cookies,
    /// so `*` is safe; this is for a game that wants its routes to be its
    /// own site's.
    pub api_allow_origins: Option<Vec<String>>,
    /// How long `Api::caller` remembers who a bearer token is, in seconds,
    /// so a page making several requests a second does not sign in for each
    /// (a store read, or a round trip to a cluster's coordinator). `0` asks
    /// every time. A token signed out everywhere is believed for this long.
    pub api_caller_cache_secs: f64,
    /// A path that, once it exists, stops the server as Ctrl+C would - saving
    /// everyone. For whatever supervises the process on a platform with no
    /// signal to send (Windows): `jengine-fleet`'s agent writes it. The
    /// default is `JENGINE_STOP_FILE` from the environment, so a server
    /// built on this crate is stoppable that way with no code of its own.
    pub stop_file: Option<std::path::PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_connections: 10_000,
            max_rooms: 10_000,
            max_frame_bytes: 64 * 1024,
            max_message_bytes: 1024 * 1024,
            messages_per_second: 60.0,
            outbox: 256,
            ping_secs: 2.0,
            autosave_secs: 30.0,
            default_kind: None,
            admin_token: None,
            public_stats: false,
            allow_plain_passwords: false,
            admin_over_plain: false,
            history_secs: 10.0,
            warn_memory_percent: 90,
            warn_cpu_percent: 90.0,
            warn_disk_free_percent: 5,
            warn_webhook: None,
            warn_repeat_secs: 3600.0,
            warn_retry_secs: vec![5.0, 15.0, 40.0],
            operators: Vec::new(),
            api_requests_per_second: 20.0,
            api_token_requests_per_second: 20.0,
            api_max_body_bytes: 2 * 1024 * 1024,
            // So a server built on this crate behind a TLS proxy on another
            // machine is told apart by its players' addresses with no code.
            trusted_proxies: std::env::var("JENGINE_TRUST_PROXY")
                .ok()
                .and_then(|list| crate::listen::parse_addresses(&list).ok())
                .unwrap_or_default(),
            api_allow_origins: None,
            api_caller_cache_secs: 5.0,
            stop_file: std::env::var_os("JENGINE_STOP_FILE").map(std::path::PathBuf::from),
        }
    }
}

/// How rooms of one kind are run.
#[derive(Debug, Clone)]
pub struct KindOptions {
    /// `on_tick` calls a second. Zero for a room that only answers messages.
    pub tick_hz: f64,
    pub max_players: usize,
    /// Stays open with nobody in it - a zone of a world. Otherwise a room
    /// closes when its last player leaves.
    pub persistent: bool,
    /// Seconds a player's seat is held after their connection drops. Within
    /// it, signing in again puts them back as if nothing happened: the same
    /// place in the room, and the others never saw them leave. Zero - the
    /// default - makes a drop a leave. Only for players who signed in; a
    /// guest has no token to come back with.
    pub grace_secs: f64,
}

impl Default for KindOptions {
    fn default() -> Self {
        KindOptions { tick_hz: 0.0, max_players: 64, persistent: false, grace_secs: 0.0 }
    }
}

impl KindOptions {
    pub fn ticking(hz: f64) -> Self {
        KindOptions { tick_hz: hz, ..Default::default() }
    }

    pub fn max_players(mut self, n: usize) -> Self {
        self.max_players = n;
        self
    }

    pub fn persistent(mut self) -> Self {
        self.persistent = true;
        self
    }

    /// Hold a dropped player's seat for this long.
    pub fn grace(mut self, secs: f64) -> Self {
        self.grace_secs = secs.max(0.0);
        self
    }
}

/// Makes a room of some kind, given its id.
type Factory = Box<dyn Fn(&str) -> Box<dyn Room> + Send + Sync>;

struct Kind {
    options: KindOptions,
    factory: Factory,
}

/// What somebody holding an admin token may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Look: the dashboard, rooms, players, events.
    Viewer,
    /// Look and act: kick, close, notice, edit, drain, shut down.
    Operator,
}

/// Somebody who runs the server, by name - so the event log says who did it.
#[derive(Debug, Clone)]
pub struct Operator {
    pub name: String,
    pub token: String,
    pub role: Role,
}

/// A question to a channel policy: may this player listen to (or speak in)
/// this channel?
pub struct ChannelAsk<'a> {
    pub player: &'a str,
    pub name: &'a str,
    pub channel: &'a str,
    /// `false` for subscribing, `true` for each thing said.
    pub saying: bool,
}

type ChannelPolicy = Box<dyn Fn(&ChannelAsk) -> bool + Send + Sync>;

/// A channel: who is in it on this node, and the last things said.
#[derive(Default)]
struct Channel {
    /// By connection serial.
    members: HashMap<u64, Member>,
    /// Handed to whoever joins, so they are not met by silence.
    recent: VecDeque<Utf8Bytes>,
    /// Players an operator has muted here, and until when (Unix seconds).
    muted: HashMap<PlayerId, f64>,
}

/// Where channel mutes are kept. Key: `<channel>|<player>`; value: Unix
/// seconds the mute ends.
const NS_MUTE: &str = "mute";

struct Member {
    out: mpsc::Sender<Utf8Bytes>,
    player: PlayerId,
    name: String,
}

impl Channel {
    /// A channel as the store last saw it: its backlog, and nobody in it yet.
    fn load(store: &dyn Store, name: &str) -> Self {
        let recent: VecDeque<Utf8Bytes> = store
            .load(NS_CHANNEL, name)
            .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|said| Utf8Bytes::from(said.to_string()))
            .collect();
        Channel { members: HashMap::new(), recent, muted: Self::mutes(store, name) }
    }

    /// The mutes the store keeps for a channel that are still on.
    fn mutes(store: &dyn Store, name: &str) -> HashMap<PlayerId, f64> {
        let prefix = format!("{name}|");
        let now = unix_now();
        store
            .keys(NS_MUTE)
            .into_iter()
            .filter_map(|key| {
                let player = key.strip_prefix(&prefix)?.to_string();
                let until: f64 = store.load(NS_MUTE, &key)?.parse().ok()?;
                (until > now).then_some((player, until))
            })
            .collect()
    }

    fn is_muted(&self, player: &str) -> bool {
        self.muted.get(player).is_some_and(|until| *until > unix_now())
    }

    fn say(&mut self, said: &Utf8Bytes) {
        if self.recent.len() >= CHANNEL_BACKLOG {
            self.recent.pop_front();
        }
        self.recent.push_back(said.clone());
        for member in self.members.values() {
            let _ = member.out.try_send(said.clone());
        }
    }
}

/// Things said that a channel remembers for whoever joins next - and keeps
/// in the store, so a restart does not forget them.
const CHANNEL_BACKLOG: usize = 20;
const NS_CHANNEL: &str = "channel";
/// Most channels one connection may be in.
const MAX_SUBSCRIPTIONS: usize = 16;
/// Largest body of one `say`.
const MAX_SAY_BYTES: usize = 2048;
/// Events of these kinds are written to the store, so "what happened last
/// night" survives a restart. Rooms opening and players arriving are not:
/// there are too many, and they are not what anyone asks about afterwards.
const KEPT_EVENTS: [&str; 7] = ["server", "panic", "admin", "slow", "warn", "cluster", "liveops"];
const NS_EVENT: &str = "event";
const NS_HISTORY: &str = "history";
/// Samples kept: six hours at the default ten seconds.
const MAX_SAMPLES: usize = 2160;
/// Hourly samples kept: thirty days.
const MAX_HOURLY: usize = 720;
/// Minute samples kept: a week.
const MAX_MINUTES: usize = 7 * 24 * 60;
const NS_TOTALS: &str = "totals";
/// Wrong `hello`s a connection may make before it is closed - what keeps one
/// socket from being a password-guessing machine.
const MAX_FAILED_HELLOS: u32 = 5;

/// Who a connection turned out to be.
#[derive(Debug, Clone)]
pub struct Identity {
    pub player: PlayerId,
    /// Used when the `hello` gave no name and the player's record has none.
    pub name: String,
    /// Handed back in `welcome` for the client to keep and present next time.
    pub token: Option<String>,
}

/// Decides who a `hello` frame is from. Replace it to check a login your own
/// backend issued; the default takes anyone and remembers them by a token.
pub trait Authenticator: Send + Sync + 'static {
    fn authenticate(&self, hello: &Value, store: &dyn Store) -> Result<Identity, String>;

    /// Something a signed-in player asks about their own account - an
    /// `account` frame. What it returns is sent back to them.
    fn manage(&self, _player: &str, _request: &Value, _store: &dyn Store) -> Result<Value, String> {
        Err("this server has no account settings".into())
    }

    /// An operator gives an account a new password.
    fn reset_password(&self, _account: &str, _password: &str, _store: &dyn Store) -> Result<(), String> {
        Err("this server keeps no passwords".into())
    }

    /// An operator deletes an account. Returns the player it was.
    fn delete_account(&self, _account: &str, _store: &dyn Store) -> Result<String, String> {
        Err("this server keeps no accounts".into())
    }

    /// An operator ends every session a player has, so no token or ticket
    /// they hold now signs them in again (`POST /admin/api/player/{id}/sign_out`).
    fn sign_out(&self, _player: &str, _store: &dyn Store) -> Result<(), String> {
        Err("this server's sign-in has no sessions to end".into())
    }
}

/// Anyone may play. A first `hello` mints a player and a secret token; a
/// later `hello` carrying that token is the same player.
pub struct GuestAuth;

pub(crate) fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("the OS has randomness");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

impl Authenticator for GuestAuth {
    fn authenticate(&self, hello: &Value, store: &dyn Store) -> Result<Identity, String> {
        let name = hello.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        match hello.get("token").and_then(Value::as_str).filter(|t| !t.is_empty()) {
            // "Could not ask" is not "never heard of you": a client told
            // its token is unknown throws it away.
            Some(token) => match store.try_load(NS_TOKEN, token) {
                Ok(Some(player)) => Ok(Identity { player, name, token: Some(token.to_string()) }),
                Ok(None) => Err("unknown token".into()),
                Err(_) => Err(STORE_DOWN.into()),
            },
            None => {
                let player = format!("p_{}", random_hex(8));
                let token = random_hex(16);
                store.save(NS_TOKEN, &token, &player);
                Ok(Identity { player, name, token: Some(token) })
            }
        }
    }
}

/// A server being described. Register kinds of room, then [`Server::run`].
pub struct Server {
    kinds: HashMap<String, Kind>,
    store: Arc<dyn Store>,
    auth: Box<dyn Authenticator>,
    config: Config,
    channels: Option<ChannelPolicy>,
    cluster: Option<ClusterConfig>,
    routes: Vec<api::Route>,
    tls: Option<crate::listen::Tls>,
    /// A game's own axum routes, merged last.
    extra: Option<Box<dyn FnOnce(Router) -> Router + Send>>,
    liveops: Option<crate::liveops::LiveOps>,
    /// Rooms opened as the server starts, and kept open with nobody in them.
    at_start: Vec<(String, String)>,
    /// A game's own warnings ([`Server::warn_when`]).
    checks: Vec<(String, WarnCheck)>,
}

/// A game's question for the warnings: what is wrong, if anything.
type WarnCheck = Box<dyn Fn() -> Option<String> + Send + Sync>;

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Self {
        Server {
            kinds: HashMap::new(),
            store: Arc::new(MemoryStore::new()),
            auth: Box::new(GuestAuth),
            config: Config::default(),
            channels: None,
            cluster: None,
            routes: Vec::new(),
            tls: None,
            extra: None,
            liveops: None,
            at_start: Vec::new(),
            checks: Vec::new(),
        }
    }

    /// Serve **live ops** - remote config, experiments, live events and
    /// content releases, per channel, signed - see [`crate::liveops`]. Games
    /// ask `/liveops/v1/..` with no token; operators change it on
    /// `/admin/liveops` (which needs [`Server::admin_token`]). It keeps its
    /// channels in the server's [`Store`], so give the server one that lasts.
    pub fn liveops(mut self, ops: crate::liveops::LiveOps) -> Self {
        self.liveops = Some(ops);
        self
    }

    /// Serve over TLS - `wss://` for the game, `https://` for the dashboard
    /// and the routes - from a certificate and key in PEM. For a certificate
    /// a browser and the engine's client trust it has to come from an
    /// authority they know (a self-signed one is for tests); a proxy in
    /// front of a plain server is the other way to the same thing.
    pub fn tls(mut self, tls: crate::listen::Tls) -> Self {
        self.tls = Some(tls);
        self
    }

    /// Merge the game's own axum [`Router`] into the server's, for what
    /// [`Server::get`] and friends cannot express: a file, a stream, a
    /// custom header, middleware. It gets no [`Api`]; the server's own paths
    /// win on a clash.
    pub fn routes(mut self, extend: impl FnOnce(Router) -> Router + Send + 'static) -> Self {
        self.extra = Some(Box::new(extend));
        self
    }

    /// Answer `GET path` over HTTP - see [`api`]. The path starts with
    /// `/api/`; `{name}` parts are read with [`ApiRequest::param`].
    pub fn get(self, path: &str, handler: impl ApiHandler) -> Self {
        self.route("GET", path, handler)
    }

    /// Answer `POST path`; the JSON body is [`ApiRequest::body`].
    pub fn post(self, path: &str, handler: impl ApiHandler) -> Self {
        self.route("POST", path, handler)
    }

    pub fn put(self, path: &str, handler: impl ApiHandler) -> Self {
        self.route("PUT", path, handler)
    }

    pub fn delete(self, path: &str, handler: impl ApiHandler) -> Self {
        self.route("DELETE", path, handler)
    }

    fn route(mut self, method: &'static str, path: &str, handler: impl ApiHandler) -> Self {
        // Said at start-up, where the mistake is: under `/api/` a route
        // cannot shadow the socket, the dashboard or a health check.
        assert!(path.starts_with("/api/"), "an API route's path starts with /api/ - got {path}");
        assert!(
            !self.routes.iter().any(|r| r.method == method && r.path == path),
            "{method} {path} is registered twice"
        );
        self.routes.push(api::Route { method, path: path.to_string(), handler: Arc::new(handler), per_second: None });
        self
    }

    pub fn store(mut self, store: Arc<dyn Store>) -> Self {
        self.store = store;
        self
    }

    pub fn auth(mut self, auth: impl Authenticator) -> Self {
        self.auth = Box::new(auth);
        self
    }

    /// Turn the admin API and dashboard on, behind this token.
    pub fn admin_token(mut self, token: &str) -> Self {
        self.config.admin_token = Some(token.to_string()).filter(|t| !t.is_empty());
        self
    }

    /// Add a named operator. Viewers can see the dashboard and nothing more.
    pub fn operator(mut self, name: &str, token: &str, role: Role) -> Self {
        if !token.is_empty() {
            self.config.operators.push(Operator { name: name.to_string(), token: token.to_string(), role });
        }
        self
    }

    /// Turn on **channels**: something a signed-in player can be in beside
    /// their room - a chat, a guild, a party - and keeps while they move from
    /// room to room. `{"t":"sub","channel":"guild:7"}` joins one,
    /// `{"t":"say","channel":"guild:7","body":..}` speaks in it, and everyone
    /// in it (the speaker included) hears `{"t":"said","channel","from",
    /// "name","body"}`. `policy` decides who may listen and who may speak.
    pub fn channels(mut self, policy: impl Fn(&ChannelAsk) -> bool + Send + Sync + 'static) -> Self {
        self.channels = Some(Box::new(policy));
        self
    }

    /// Channels anyone signed in may join and speak in.
    pub fn open_channels(self) -> Self {
        self.channels(|_| true)
    }

    /// Make this server one node of a cluster - see [`crate::cluster`]. Its
    /// store becomes the coordinator's, so call this *instead of* `store`.
    pub fn cluster(mut self, config: ClusterConfig) -> Self {
        self.store = Arc::new(RemoteStore::new(&config));
        self.cluster = Some(config);
        self
    }

    /// Put the store behind a [`crate::WriteBehind`]: every write is taken
    /// at once and sent on a thread of its own, so a room's autosave and a
    /// player's leave never wait on a store a network away. Call it after
    /// [`Server::store`] or [`Server::cluster`]. The server flushes it
    /// before it hands a player to another node.
    pub fn write_behind(mut self) -> Self {
        self.store = Arc::new(crate::WriteBehind::new(self.store));
        self
    }

    /// Put `/stats` and `/metrics` behind the admin token - which is the
    /// default now; kept so a server that asked for it still builds.
    pub fn private_stats(mut self) -> Self {
        self.config.public_stats = false;
        self
    }

    /// Let anyone read `/stats` and `/metrics` (`Config::public_stats`):
    /// counts only, but counts say how many are playing.
    pub fn public_stats(mut self) -> Self {
        self.config.public_stats = true;
        self
    }

    /// Take password `hello`s over a connection that crossed the network
    /// unencrypted (`Config::allow_plain_passwords`) - for a LAN you trust.
    pub fn allow_plain_passwords(mut self) -> Self {
        self.config.allow_plain_passwords = true;
        self
    }

    /// Take the admin token over plain HTTP from another machine
    /// (`Config::admin_over_plain`) - for a LAN you trust.
    pub fn admin_over_plain(mut self) -> Self {
        self.config.admin_over_plain = true;
        self
    }

    /// Hold one of the game's routes to `per_second` requests an address
    /// (a burst of twice that), on top of the budget every `/api/` request
    /// shares - for a route that is expensive (a search, a report) beside
    /// ones that are cheap. The route must already be registered.
    pub fn route_limit(mut self, method: &str, path: &str, per_second: f64) -> Self {
        let route = self
            .routes
            .iter_mut()
            .find(|r| r.method.eq_ignore_ascii_case(method) && r.path == path)
            .unwrap_or_else(|| panic!("route_limit: no route {method} {path} is registered (register it first)"));
        route.per_second = Some(per_second.max(0.0));
        self
    }

    /// Believe these proxies about who a connection is from
    /// (`Config::trusted_proxies`).
    pub fn trust_proxies(mut self, proxies: impl IntoIterator<Item = std::net::IpAddr>) -> Self {
        self.config.trusted_proxies.extend(proxies);
        self
    }

    pub fn config(mut self, config: Config) -> Self {
        // A default kind named by `relay()` survives a later `config()`.
        let default_kind = self.config.default_kind.take();
        let admin_token = self.config.admin_token.take();
        let operators = std::mem::take(&mut self.config.operators);
        self.config = config;
        self.config.operators.extend(operators);
        if self.config.default_kind.is_none() {
            self.config.default_kind = default_kind;
        }
        if self.config.admin_token.is_none() {
            self.config.admin_token = admin_token;
        }
        self
    }

    /// Register a kind of room. `make` is called with the room's id each time
    /// one is opened.
    pub fn room<R: Room>(
        mut self,
        kind: &str,
        options: KindOptions,
        make: impl Fn(&str) -> R + Send + Sync + 'static,
    ) -> Self {
        self.kinds.insert(
            kind.to_string(),
            Kind { options, factory: Box::new(move |id| Box::new(make(id))) },
        );
        self
    }

    /// Open this room as the server starts, and keep it open - and ticking -
    /// with nobody in it: a market whose traders trade while no captain is
    /// in port, crops and monsters that live on their own. Without it a room
    /// exists only once somebody joins, so a world's places had to work out
    /// on opening what happened while they were shut. On a cluster the room
    /// is claimed as a join would claim it, and opened only by the node that
    /// gets it.
    pub fn open_at_start(mut self, kind: &str, room: &str) -> Self {
        self.at_start.push((kind.to_string(), room.to_string()));
        self
    }

    /// A warning of the game's own, beside the server's: `check` is asked
    /// whenever the server looks for what is wrong (every `history_secs`),
    /// and a sentence back is a warning - on the dashboard, in the event log
    /// and to the webhook, started, still and ended like the server's. It
    /// runs on the sampling task, so it should only read (an `Arc` the rooms
    /// write: a market with no sellers, a queue nobody is taken from).
    pub fn warn_when(mut self, key: &str, check: impl Fn() -> Option<String> + Send + Sync + 'static) -> Self {
        self.checks.push((key.to_string(), Box::new(check)));
        self
    }

    /// Register the `relay` kind and make it what a bare `join` means - which
    /// is `jengine-relay`'s protocol, so its clients connect unchanged.
    pub fn relay(mut self) -> Self {
        self.config.default_kind = Some("relay".into());
        self.room("relay", KindOptions::default().max_players(crate::relay::MAX_SEATS), |_| {
            RelayRoom::new()
        })
    }

    /// Register the `lockstep` kind (`crate::lockstep`): rooms that collect
    /// each seat's input for a fixed step and send every step out once all
    /// seats have sent it - the server half of a `lockstep_session`. A client joins it by name (`"kind": "lockstep"`).
    pub fn lockstep(self) -> Self {
        // Ticked, so a silent seat is noticed; room for spectators beside the seats.
        let options = KindOptions::ticking(crate::lockstep::TICK_HZ)
            .max_players(crate::lockstep::MAX_SEATS + crate::lockstep::MAX_WATCHERS);
        self.room("lockstep", options, |_| {
            crate::lockstep::LockstepRoom::new()
        })
    }

    /// Serve on a thread of its own and return at once - for a test, or a
    /// program with something else to do. `127.0.0.1:0` picks a free port.
    pub fn start(self, addr: &str) -> std::io::Result<Running> {
        let listener = std::net::TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let mut this = self;
        if let Some(cluster) = &mut this.cluster {
            // On one machine, the address we are bound to is the address.
            if cluster.public_url.is_empty() {
                cluster.public_url = format!("ws://{addr}");
            }
            if cluster.internal_url.is_empty() {
                cluster.internal_url = format!("http://{addr}");
            }
        }
        let secure = this.tls.is_some();
        if let Some(cluster) = &mut this.cluster
            && secure
            && cluster.public_url.starts_with("ws://")
        {
            cluster.public_url = cluster.public_url.replacen("ws://", "wss://", 1);
        }
        let (shutdown, shutdown_rx) = watch::channel(false);
        let shutdown = Arc::new(shutdown);
        let stop = shutdown.clone();
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
        let thread = std::thread::spawn(move || {
            runtime.block_on(async move { this.serve(listener, shutdown_rx, stop).await });
        });
        Ok(Running { addr, secure, shutdown, thread: Some(thread) })
    }

    /// Serve until Ctrl+C or SIGTERM, then stop cleanly: every player is
    /// taken out of their room and saved before the process ends, which is
    /// what makes a deploy safe to do with people online.
    pub fn run(self, addr: &str) -> std::io::Result<()> {
        let stop_file = self.config.stop_file.clone();
        let running = self.start(addr)?;
        log::info!("[server] listening on ws://{}", running.addr);
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let mut asked = running.shutdown.subscribe();
        runtime.block_on(async {
            // Ctrl+C, SIGTERM, or the admin API's `shutdown` - which is how
            // something supervising this process stops it on a platform with
            // no signal to send.
            let asked = async {
                let _ = asked.changed().await;
            };
            // Or a file appearing: how a supervisor with no signal to send
            // asks (`Config::stop_file`).
            let stop_file = async {
                let Some(path) = stop_file else { return std::future::pending::<()>().await };
                while !path.exists() {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                log::info!("[server] {} appeared", path.display());
                let _ = std::fs::remove_file(&path);
            };
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let mut term = signal(SignalKind::terminate()).expect("a SIGTERM handler");
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                    _ = asked => {}
                    _ = stop_file => {}
                }
            }
            #[cfg(not(unix))]
            {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = asked => {}
                    _ = stop_file => {}
                }
            }
        });
        log::info!("[server] stopping: saving players");
        running.stop();
        Ok(())
    }

    async fn serve(
        self,
        listener: std::net::TcpListener,
        mut shutdown: watch::Receiver<bool>,
        stop: Arc<watch::Sender<bool>>,
    ) {
        let mut config = self.config;
        if let Some(token) = config.admin_token.clone() {
            config.operators.push(Operator { name: "admin".into(), token, role: Role::Operator });
        }
        // What was worth keeping from before this start.
        let mut kept = self.store.keys(NS_EVENT);
        let old = kept.len().saturating_sub(MAX_EVENTS);
        for key in kept.drain(..old) {
            self.store.delete(NS_EVENT, &key);
        }
        let events: VecDeque<Value> = kept
            .iter()
            .filter_map(|key| serde_json::from_str(&self.store.load(NS_EVENT, key)?).ok())
            .collect();
        let history_key = match &self.cluster {
            Some(cluster) => format!("samples:{}", cluster.node),
            None => "samples".to_string(),
        };
        let history: VecDeque<Value> = self
            .store
            .load(NS_HISTORY, &history_key)
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let hourly: VecDeque<Value> = self
            .store
            .load(NS_HISTORY, &format!("hourly:{history_key}"))
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        // A week of minutes, kept a day to a key so a save writes one day.
        let today = (unix_now() / 86400.0).floor() as i64;
        let minutes: VecDeque<Value> = (today - 7..=today)
            .filter_map(|day| self.store.load(NS_HISTORY, &format!("minutes:{history_key}:{day}")))
            .filter_map(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
            .flatten()
            .collect();
        // Counters carry on from where the last run left them, so a chart
        // over days and a scrape of `/metrics` see one line, not a sawtooth.
        let metrics = Metrics::default();
        if let Some(totals) = self.store.load(NS_TOTALS, &history_key).and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
            metrics.restore(&totals);
        }
        let liveops = self.liveops.map(|ops| {
            log::info!("[liveops] channels {}; games are built with {}", ops.channel_list(), ops.public_key());
            Arc::new(crate::liveops::Service::new(ops, self.store.clone()))
        });
        let secure = self.tls.is_some();
        let (conns_done, mut conns_wait) = mpsc::channel::<()>(1);
        let (rooms_done, mut rooms_wait) = mpsc::channel::<()>(1);
        let hub = Arc::new(Hub {
            kinds: self.kinds,
            store: self.store,
            auth: self.auth,
            config,
            channel_policy: self.channels,
            cluster: self.cluster.map(|config| Arc::new(Cluster::new(config))),
            channels: Mutex::new(HashMap::new()),
            next_event: AtomicU64::new(0),
            state: Mutex::new(HubState::default()),
            shutdown: shutdown.clone(),
            stop,
            conns_done: Mutex::new(Some(conns_done)),
            rooms_done: Mutex::new(Some(rooms_done)),
            metrics,
            hourly: Mutex::new(hourly),
            minutes: Mutex::new(minutes),
            history: Mutex::new(history),
            resources: Mutex::new(Resources::default()),
            coordinator_down: AtomicBool::new(false),
            api_buckets: Mutex::new(HashMap::new()),
            api_token_buckets: Mutex::new(HashMap::new()),
            api_route_buckets: Mutex::new(HashMap::new()),
            stored_operators: Mutex::new((None, Vec::new())),
            api_routes: Mutex::new(HashMap::new()),
            api_callers: Mutex::new(HashMap::new()),
            warned: Mutex::new(HashMap::new()),
            events: Mutex::new(events),
            player_events: Mutex::new(VecDeque::new()),
            draining: AtomicBool::new(false),
            started: Instant::now(),
            liveops,
            secure,
            at_start: self.at_start.iter().cloned().collect(),
            checks: self.checks,
        });
        for (kind, room) in &self.at_start {
            hub.open_at_start(kind, room);
        }
        let mut router = Router::new()
            .route("/", get(upgrade))
            .route("/ws", get(upgrade))
            .route("/healthz", get(healthz))
            .route("/stats", get(stats))
            .route("/metrics", get(admin::metrics));
        if hub.cluster.is_some() {
            router = router
                .route("/cluster/replace", axum::routing::post(admin::cluster_replace))
                .route("/cluster/deliver", axum::routing::post(admin::cluster_deliver))
                .route("/cluster/admin/{action}", axum::routing::post(admin::cluster_admin));
        }
        // A token that crossed the network in plain text is refused before
        // any admin route sees it (`Config::admin_over_plain`).
        let guarded = |routes: Router<Arc<Hub>>| {
            routes.route_layer(axum::middleware::from_fn_with_state(hub.clone(), admin::wire_guard))
        };
        if !hub.config.operators.is_empty() {
            router = router.merge(guarded(admin::routes()));
        }
        if hub.liveops.is_some() {
            router = router.merge(liveops::public_routes());
            if !hub.config.operators.is_empty() {
                router = router.merge(guarded(liveops::admin_routes()));
            } else {
                log::warn!("[liveops] no admin token, so nobody can change live ops - games are served what the store holds");
            }
        }
        if !self.routes.is_empty() {
            // Every route is listed from the start, with nothing counted yet:
            // an operator sees what the game offers, not only what was called.
            let mut stats = hub.api_routes.lock().unwrap();
            for route in &self.routes {
                stats.entry(format!("{} {}", route.method, route.path)).or_default();
            }
            drop(stats);
            router = router.merge(api::routes(self.routes, hub.config.api_max_body_bytes));
        }
        let mut router = router.with_state(hub.clone());
        if let Some(extend) = self.extra {
            router = extend(router);
        }
        hub.event("server", "started");
        watch_resources(hub.clone(), shutdown.clone());
        // In a cluster: say we are alive, and what we hold, every so often.
        // The first is said before anyone can connect, so a redirect to this
        // node never names an address the coordinator has not heard of.
        let mut kinds: Vec<String> = hub.kinds.keys().cloned().collect();
        kinds.sort();
        if let Some(cluster) = hub.cluster.clone() {
            if let Err(e) = cluster.register(&kinds, json!({})) {
                log::error!("[cluster] {e}");
            }
            let mut reachable = true;
            let (hub, mut stop) = (hub.clone(), shutdown.clone());
            tokio::spawn(async move {
                let mut every =
                    tokio::time::interval(Duration::from_secs_f64(cluster.config.heartbeat_secs.max(0.05)));
                loop {
                    tokio::select! {
                        _ = every.tick() => {
                            let used = *hub.resources.lock().unwrap();
                            let (stats, rooms, players) = {
                                let state = hub.state.lock().unwrap();
                                let stats = json!({
                                    "connections": state.connections, "players": state.online.len(),
                                    "rooms": state.rooms.len(),
                                    "cpu": used.cpu_percent, "memory_mb": used.memory_bytes / (1024 * 1024),
                                    // The machine, for the coordinator's placement.
                                    "machine_cpu": used.machine_cpu_percent,
                                    "memory_percent": (used.machine_memory_used * 100).checked_div(used.machine_memory_total).unwrap_or(0),
                                });
                                let rooms: Vec<(String, String)> = state.rooms.keys().cloned().collect();
                                let players: Vec<String> =
                                    state.online.keys().chain(state.parked.keys()).cloned().collect();
                                (stats, rooms, players)
                            };
                            match cluster.register(&kinds, stats) {
                                // It did not know us: a new coordinator, with an
                                // empty directory. Tell it what we hold.
                                Ok(false) => cluster.resync(&rooms, &players),
                                Ok(true) => {}
                                Err(e) => {
                                    hub.coordinator_down.store(true, Ordering::Relaxed);
                                    if std::mem::replace(&mut reachable, false) {
                                        hub.event("cluster", &format!("the coordinator is not answering: {e}"));
                                    }
                                    continue;
                                }
                            }
                            hub.coordinator_down.store(false, Ordering::Relaxed);
                            if !std::mem::replace(&mut reachable, true) {
                                hub.event("cluster", "the coordinator is answering again");
                            }
                            // Whatever could not be written while it was away.
                            hub.store.flush();
                        }
                        _ = stop.changed() => break,
                    }
                }
            });
        }
        // The numbers, sampled, for the dashboard's charts.
        let sampler = {
            let (hub, mut stop) = (hub.clone(), shutdown.clone());
            tokio::spawn(async move {
                let mut every = tokio::time::interval(Duration::from_secs_f64(hub.config.history_secs.max(0.05)));
                let mut taken = 0u32;
                loop {
                    tokio::select! {
                        _ = every.tick() => {
                            hub.sample();
                            hub.roll_minute();
                            hub.roll_hour();
                            hub.note_warnings();
                            taken += 1;
                            // Written as it goes - every sixth sample, a minute
                            // at the default - so a crash costs a minute of
                            // history, not six hours.
                            if taken.is_multiple_of(6) {
                                hub.save_history();
                            }
                        }
                        _ = stop.changed() => break,
                    }
                }
            })
        };
        let stop = async move {
            let _ = shutdown.changed().await;
        };
        if let Err(e) = crate::listen::serve(listener, router, self.tls, stop).await {
            log::error!("[server] {e}");
        }
        // Connections first - each leaves its room and saves its player -
        // then the rooms, so a world can save itself with everyone out.
        hub.conns_done.lock().unwrap().take();
        let _ = tokio::time::timeout(Duration::from_secs(10), conns_wait.recv()).await;
        let rooms: Vec<RoomHandle> = hub.state.lock().unwrap().rooms.values().cloned().collect();
        for room in rooms {
            let _ = room.tx.send(RoomCmd::Close).await;
        }
        hub.rooms_done.lock().unwrap().take();
        let _ = tokio::time::timeout(Duration::from_secs(10), rooms_wait.recv()).await;
        let _ = sampler.await;
        hub.save_history();
    }
}

/// A server started with [`Server::start`].
pub struct Running {
    pub addr: SocketAddr,
    /// Serving over TLS: `wss://` and `https://`.
    pub secure: bool,
    shutdown: Arc<watch::Sender<bool>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Running {
    pub fn url(&self) -> String {
        format!("{}://{}", if self.secure { "wss" } else { "ws" }, self.addr)
    }

    /// The HTTP side: `http://` or `https://`.
    pub fn http_url(&self) -> String {
        format!("{}://{}", if self.secure { "https" } else { "http" }, self.addr)
    }

    /// Stop, having saved everyone. Returns when the server has finished.
    pub fn stop(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

mod admin;
pub mod api;
mod liveops;
pub use api::{Api, ApiError, ApiHandler, ApiRequest};

// ---- the hub ------------------------------------------------------------------

struct Hub {
    kinds: HashMap<String, Kind>,
    store: Arc<dyn Store>,
    auth: Box<dyn Authenticator>,
    config: Config,
    /// `None`: this server has no channels.
    channel_policy: Option<ChannelPolicy>,
    /// `Some`: this server is one node of several.
    cluster: Option<Arc<Cluster>>,
    channels: Mutex<HashMap<String, Channel>>,
    next_event: AtomicU64,
    state: Mutex<HubState>,
    shutdown: watch::Receiver<bool>,
    /// The other end of `shutdown`, for the admin API to stop the server.
    stop: Arc<watch::Sender<bool>>,
    /// Each task holds a clone; shutdown drops these and waits for the rest.
    conns_done: Mutex<Option<mpsc::Sender<()>>>,
    rooms_done: Mutex<Option<mpsc::Sender<()>>>,
    metrics: Metrics,
    /// The last [`MAX_EVENTS`] things worth an operator's attention, and -
    /// kept apart, so a busy evening of arrivals cannot push a panic off the
    /// end - the last [`MAX_EVENTS`] comings and goings of players.
    events: Mutex<VecDeque<Value>>,
    player_events: Mutex<VecDeque<Value>>,
    /// The numbers over time: one sample every `history_secs`.
    history: Mutex<VecDeque<Value>>,
    /// And one an hour, for thirty days: each the hour's samples boiled down.
    hourly: Mutex<VecDeque<Value>>,
    /// And one a minute, for a week: each the minute's samples boiled down.
    minutes: Mutex<VecDeque<Value>>,
    resources: Mutex<Resources>,
    /// What each address has left to spend on `/api/`: (tokens, as of).
    api_buckets: Mutex<HashMap<std::net::IpAddr, (f64, Instant)>>,
    /// The same per bearer token (by a hash of it, so no token is kept).
    api_token_buckets: Mutex<HashMap<u64, (f64, Instant)>>,
    /// And per (route, address), for routes with a limit of their own.
    api_route_buckets: Mutex<HashMap<(Arc<str>, std::net::IpAddr), (f64, Instant)>>,
    /// Operators added at runtime (`/admin/api/operators`), as the store last
    /// said - re-read every few seconds, so a removal on one node of a
    /// cluster reaches the others.
    stored_operators: Mutex<(Option<Instant>, Vec<admin::StoredOperator>)>,
    /// Each `/api/` route's counts and times, by `"GET /api/x"`.
    api_routes: Mutex<HashMap<String, api::RouteStats>>,
    /// Who a bearer token was, as of when: `Api::caller`'s memory.
    api_callers: Mutex<HashMap<String, (PlayerId, Instant)>>,
    /// Whether the cluster's coordinator answered the last heartbeat.
    coordinator_down: AtomicBool,
    /// What was wrong at the last look, by what it is about - so the event
    /// log says when something started and stopped, not every ten seconds.
    warned: Mutex<HashMap<String, Instant>>,
    /// Taking no new connections - set before a deploy so a load balancer
    /// sends new players elsewhere while the ones here finish.
    draining: AtomicBool,
    started: Instant,
    /// Live ops, if the server serves them.
    liveops: Option<Arc<crate::liveops::Service>>,
    /// Served over TLS - what a URL this server hands out starts with.
    secure: bool,
    /// Rooms opened at start ([`Server::open_at_start`]): open with nobody
    /// in them, whatever their kind says.
    at_start: HashSet<(String, String)>,
    /// The game's own warnings.
    checks: Vec<(String, WarnCheck)>,
}

const MAX_EVENTS: usize = 500;

/// Counters since the server started. All of them only go up, which is what
/// a scraper wants: it takes the rate itself.
#[derive(Default)]
struct Metrics {
    connections_total: AtomicU64,
    messages_in: AtomicU64,
    /// Bytes of every text frame players sent, delivered or not.
    bytes_in: AtomicU64,
    frames_out: AtomicU64,
    bytes_out: AtomicU64,
    joins: AtomicU64,
    joins_refused: AtomicU64,
    rate_limited: AtomicU64,
    slow_clients_dropped: AtomicU64,
    room_panics: AtomicU64,
    player_saves: AtomicU64,
    /// Requests to the game's own `/api/` routes, and those that failed in
    /// the route's code (a panic, or a 5xx it returned).
    api_requests: AtomicU64,
    api_failures: AtomicU64,
    api_limited: AtomicU64,
}

impl Metrics {
    fn pairs(&self) -> [(&'static str, &AtomicU64); 14] {
        [
            ("connections_total", &self.connections_total),
            ("messages_in", &self.messages_in),
            ("bytes_in", &self.bytes_in),
            ("frames_out", &self.frames_out),
            ("bytes_out", &self.bytes_out),
            ("joins", &self.joins),
            ("joins_refused", &self.joins_refused),
            ("rate_limited", &self.rate_limited),
            ("slow_clients_dropped", &self.slow_clients_dropped),
            ("room_panics", &self.room_panics),
            ("player_saves", &self.player_saves),
            ("api_requests", &self.api_requests),
            ("api_failures", &self.api_failures),
            ("api_limited", &self.api_limited),
        ]
    }

    /// The counters, to be written down.
    fn snapshot(&self) -> Value {
        Value::Object(self.pairs().iter().map(|(k, a)| (k.to_string(), json!(a.load(Ordering::Relaxed)))).collect())
    }

    /// Carry on from counters written down by an earlier run.
    fn restore(&self, totals: &Value) {
        for (key, atomic) in self.pairs() {
            if let Some(n) = totals[key].as_u64() {
                atomic.store(n, Ordering::Relaxed);
            }
        }
    }
}

/// What this process and its machine are using, as last measured.
#[derive(Debug, Clone, Copy, Default)]
struct Resources {
    /// Of one core: 250 is two and a half cores busy.
    cpu_percent: f32,
    cores: usize,
    memory_bytes: u64,
    /// The whole machine, every process: 0 to 100.
    machine_cpu_percent: f32,
    machine_memory_used: u64,
    machine_memory_total: u64,
    /// The disk the working directory is on - where a store's file lives.
    disk_free: u64,
    disk_total: u64,
    /// Bytes a second over every interface, the whole machine, as measured
    /// between two looks.
    net_rx_per_sec: f64,
    net_tx_per_sec: f64,
}

impl Resources {
    fn json(&self) -> Value {
        json!({
            "cpu": (self.cpu_percent * 10.0).round() / 10.0,
            "cores": self.cores,
            "memory_mb": self.memory_bytes / (1024 * 1024),
            "machine_cpu": (self.machine_cpu_percent * 10.0).round() / 10.0,
            "machine_memory_used_mb": self.machine_memory_used / (1024 * 1024),
            "machine_memory_total_mb": self.machine_memory_total / (1024 * 1024),
            "disk_free_mb": self.disk_free / (1024 * 1024),
            "disk_total_mb": self.disk_total / (1024 * 1024),
            "net_rx_kb_per_sec": (self.net_rx_per_sec / 1024.0 * 10.0).round() / 10.0,
            "net_tx_kb_per_sec": (self.net_tx_per_sec / 1024.0 * 10.0).round() / 10.0,
        })
    }
}

/// The disk the working directory is on: the mount with the longest path
/// that is a prefix of it.
fn disk_of_cwd(disks: &sysinfo::Disks) -> Option<(u64, u64)> {
    let cwd = std::env::current_dir().ok()?;
    disks
        .iter()
        .filter(|d| cwd.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| (d.available_space(), d.total_space()))
}

/// Measure, every two seconds, on a thread of its own: asking the operating
/// system about a process is a system call or several, and CPU use is the
/// difference between two askings.
fn watch_resources(hub: Arc<Hub>, stop: watch::Receiver<bool>) {
    use sysinfo::{Disks, Networks, ProcessesToUpdate, System};
    std::thread::spawn(move || {
        let Ok(pid) = sysinfo::get_current_pid() else { return };
        let mut system = System::new();
        system.refresh_cpu_usage();
        let mut disks = Disks::new_with_refreshed_list();
        let mut networks = Networks::new_with_refreshed_list();
        let total_bytes = |networks: &Networks| -> (u64, u64) {
            networks.iter().fold((0, 0), |(rx, tx), (_, d)| (rx + d.total_received(), tx + d.total_transmitted()))
        };
        let (mut last_net, mut last_at) = (total_bytes(&networks), Instant::now());
        let mut looks: u32 = 0;
        while !*stop.borrow() {
            system.refresh_cpu_usage();
            system.refresh_memory();
            system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
            // The disk every look; the list of them (a mount can come and
            // go) once a minute.
            disks.refresh(looks.is_multiple_of(30));
            networks.refresh(looks.is_multiple_of(30));
            looks = looks.wrapping_add(1);
            let now_net = total_bytes(&networks);
            let secs = last_at.elapsed().as_secs_f64().max(1e-3);
            let (rx, tx) = (
                now_net.0.saturating_sub(last_net.0) as f64 / secs,
                now_net.1.saturating_sub(last_net.1) as f64 / secs,
            );
            (last_net, last_at) = (now_net, Instant::now());
            let (disk_free, disk_total) = disk_of_cwd(&disks).unwrap_or((0, 0));
            let mut now = Resources {
                cores: system.cpus().len(),
                machine_cpu_percent: system.global_cpu_usage(),
                machine_memory_used: system.used_memory(),
                machine_memory_total: system.total_memory(),
                disk_free,
                disk_total,
                net_rx_per_sec: rx,
                net_tx_per_sec: tx,
                ..Default::default()
            };
            if let Some(process) = system.process(pid) {
                now.cpu_percent = process.cpu_usage();
                now.memory_bytes = process.memory();
            }
            *hub.resources.lock().unwrap() = now;
            // In short sleeps, so stopping the server does not wait on this.
            for _ in 0..20 {
                if *stop.borrow() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
}

/// What one room publishes about itself, readable without asking its task.
struct RoomStats {
    opened: Instant,
    messages: AtomicU64,
    frames_out: AtomicU64,
    /// Microseconds a tick takes - rules, sync and handing frames to the
    /// sockets - as a moving average and as the worst of the last hundred.
    tick_avg_us: AtomicU64,
    tick_max_us: AtomicU64,
}

fn unix_now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

#[derive(Default)]
struct HubState {
    /// (kind, id) -> room
    rooms: HashMap<(String, String), RoomHandle>,
    online: HashMap<PlayerId, Online>,
    connections: usize,
    next_serial: u64,
    /// Every connection's way out, by its serial - for what is said to
    /// everyone whether or not they are in a room.
    outboxes: HashMap<u64, mpsc::Sender<Utf8Bytes>>,
    /// Players whose connection dropped and whose seat a room is holding:
    /// player -> (kind, id) of that room.
    parked: HashMap<PlayerId, (String, String)>,
}

struct Online {
    serial: u64,
    ctrl: mpsc::UnboundedSender<ConnEvent>,
    name: String,
    /// Where they are connected from, as `Config::trusted_proxies` decides.
    address: std::net::IpAddr,
    /// Their connection's measured round trip, if it answers pings.
    rtt_ms: Option<f64>,
    /// Unix seconds they signed in at.
    since: f64,
    /// (kind, id) of the room they are in.
    room: Option<(String, String)>,
}

#[derive(Clone)]
struct RoomHandle {
    serial: u64,
    tx: mpsc::Sender<RoomCmd>,
    /// Players in the room, for matchmaking and `/stats`. The room itself is
    /// what enforces its size.
    count: Arc<AtomicUsize>,
    stats: Arc<RoomStats>,
}

enum RoomCmd {
    Join {
        player: Player,
        options: Value,
        conn: ConnHandle,
        reply: oneshot::Sender<Result<(), (Player, Refusal)>>,
    },
    /// `delay`: how long ago it was sent, by the connection's measure.
    Message { player: PlayerId, body: Value, delay: f64 },
    Leave { player: PlayerId },
    Close,
    // From an operator:
    Inspect { reply: oneshot::Sender<Value> },
    Admin { command: Value, reply: oneshot::Sender<Value> },
    Kick { player: PlayerId, why: String },
    Notice { frame: Value },
    /// The player's connection dropped; hold their seat if the kind does.
    /// Answered once the room has decided, so a new connection signing in
    /// as them afterwards finds the seat already held.
    Dropped { player: PlayerId, done: oneshot::Sender<()> },
    /// The player is back on a new connection. `true` if their seat was
    /// still held and is theirs again.
    Rebind { player: PlayerId, conn: ConnHandle, reply: oneshot::Sender<bool> },
    /// An operator replaces a player's state. Answers whether they were here.
    EditState { player: PlayerId, state: Value, reply: oneshot::Sender<Value> },
    /// One of the game's HTTP routes changes a player's state in place.
    /// Answers the state after it, or `None` if they are not here.
    EditWith { player: PlayerId, edit: StateEdit, reply: oneshot::Sender<Option<Value>> },
}

enum Refusal {
    /// The room closed while the join was on its way; try again.
    Gone,
    Refused(String),
}

/// Why a player is not in the room they asked for.
enum NotJoined {
    Refused(String),
    /// It is on another node of the cluster: this is where.
    Elsewhere(String),
}

/// How a connection's attempt to join ended.
#[derive(PartialEq)]
enum Joining {
    In,
    Refused,
    /// The client has been sent to another node.
    Redirected,
}

/// What a room needs to reach a connection.
#[derive(Clone)]
struct ConnHandle {
    out: mpsc::Sender<Utf8Bytes>,
    ctrl: mpsc::UnboundedSender<ConnEvent>,
    /// Whether this player is saved at all - a guest who never said `hello`
    /// is not.
    persist: bool,
}

enum ConnEvent {
    Released(Player, Release),
    /// The same player signed in on another connection. Fire the sender once
    /// this one has saved and gone.
    Replaced(oneshot::Sender<()>),
    /// The client is not reading fast enough to keep.
    TooSlow,
    /// An operator closed the connection.
    Disconnect(String),
    /// An operator replaced the state of a player who is in no room.
    Edit(Value),
    /// One of the game's HTTP routes changes the state of a player who is in
    /// no room. Answers the state after it.
    EditWith(StateEdit, oneshot::Sender<Option<Value>>),
}

/// A change to a player's state, made wherever the true copy is.
type StateEdit = Arc<dyn Fn(&mut Value) + Send + Sync>;

/// A player's saved record: who they are, their state, and the room they
/// were last in so a client can go back to it.
fn record(player: &Player, room: Option<(&str, &str)>) -> String {
    json!({
        "name": player.name,
        "state": player.state,
        "room": room.map(|(kind, id)| json!({ "kind": kind, "id": id })),
    })
    .to_string()
}

/// A room's refusal that sends a **matchmaking** join on to another room:
/// with no room named, the server tries the next one with a free seat (or
/// opens one) rather than turning the player away. A room returns it from
/// `on_join` when there is no seat *for this player* - a private match
/// refusing a stranger, a ranked room refusing the wrong level - as well as
/// when it is simply full. A join that named its room is refused with it.
pub const ROOM_FULL: &str = "the room is full";
const FULL: &str = ROOM_FULL;

impl Hub {
    /// Whether a request or connection from `peer` crossed a network
    /// unencrypted - believing this machine (a host's gateway) and the named
    /// proxies about how it was reached.
    fn in_clear(&self, peer: std::net::IpAddr, headers: &axum::http::HeaderMap) -> bool {
        let trusted = &self.config.trusted_proxies;
        crate::listen::headers_in_clear(peer, headers, self.secure, |ip| ip.is_loopback() || trusted.contains(&ip))
    }

    /// Note something an operator would want to know happened.
    fn event(&self, kind: &str, text: &str) {
        log::info!("[{kind}] {text}");
        let event = json!({ "at": unix_now(), "kind": kind, "text": text });
        if KEPT_EVENTS.contains(&kind) {
            // Keyed so that sorting the keys is sorting by time.
            let n = self.next_event.fetch_add(1, Ordering::Relaxed);
            let key = format!("{:020}-{n:06}", (unix_now() * 1e6) as u64);
            self.store.save(NS_EVENT, &key, &event.to_string());
        }
        let ring = if kind == "player" { &self.player_events } else { &self.events };
        let mut events = ring.lock().unwrap();
        if events.len() >= MAX_EVENTS {
            events.pop_front();
        }
        events.push_back(event);
    }

    /// What is wrong right now, as (what it is about, a sentence). Nothing
    /// here acts: it is what an operator should be told before they find
    /// out some other way.
    fn warnings(&self) -> Vec<(String, String)> {
        let mut found = Vec::new();
        let used = *self.resources.lock().unwrap();
        // No total means nothing has been measured yet.
        if let Some(percent) = (used.machine_memory_used * 100).checked_div(used.machine_memory_total)
            && percent >= self.config.warn_memory_percent
        {
            found.push(("memory".to_string(), format!("the machine is at {percent}% of its memory")));
        }
        if used.machine_cpu_percent >= self.config.warn_cpu_percent {
            found.push(("cpu".to_string(), format!("the machine's CPU is {:.0}% busy", used.machine_cpu_percent)));
        }
        if let Some(percent) = (used.disk_free * 100).checked_div(used.disk_total)
            && percent < self.config.warn_disk_free_percent
        {
            found.push(("disk".to_string(), format!("the disk has {percent}% left")));
        }
        {
            let state = self.state.lock().unwrap();
            for ((kind, id), room) in &state.rooms {
                let hz = self.kinds.get(kind).map(|k| k.options.tick_hz).unwrap_or(0.0);
                let worst = room.stats.tick_max_us.load(Ordering::Relaxed) as f64 / 1000.0;
                if hz > 0.0 && worst > 1000.0 / hz {
                    found.push((
                        format!("tick:{kind}/{id}"),
                        format!("{kind}/{id} took {worst:.0} ms for a tick; it has {:.0} ms", 1000.0 / hz),
                    ));
                }
            }
        }
        if self.coordinator_down.load(Ordering::Relaxed) {
            found.push(("coordinator".to_string(), "the cluster's coordinator is not answering: nobody new can sign in".to_string()));
        }
        let waiting = self.store.waiting();
        if waiting > 0 {
            found.push(("writes".to_string(), format!("{waiting} save(s) are waiting for the store to come back")));
        }
        if self.draining.load(Ordering::Relaxed) {
            found.push(("draining".to_string(), "draining: no new connections are being taken".to_string()));
        }
        // A live ops key whose endorsement is about to run out: past it no
        // game believes the service, so it is said a fortnight ahead.
        if let Some(ends) = self.liveops.as_ref().and_then(|ops| ops.endorsement_ends()) {
            let left = ends as f64 - unix_now();
            if left < 14.0 * 86400.0 {
                let days = (left / 86400.0).max(0.0);
                found.push((
                    "liveops-key".to_string(),
                    format!("the live ops key's endorsement runs out in {days:.1} days: endorse a new one (jengine-server liveops-key endorse)"),
                ));
            }
        }
        for (key, check) in &self.checks {
            if let Some(text) = check() {
                found.push((format!("game:{key}"), text));
            }
        }
        found
    }

    /// Put what has started or stopped being wrong in the event log - and
    /// what is still wrong, every `warn_repeat_secs`, so a warning whose one
    /// message was missed is not forgotten.
    fn note_warnings(&self) {
        let now = self.warnings();
        let repeat = Duration::from_secs_f64(self.config.warn_repeat_secs.max(0.0));
        let mut warned = self.warned.lock().unwrap();
        let (mut started, mut ended, mut still) = (Vec::new(), Vec::new(), Vec::new());
        for (key, text) in &now {
            if key == "draining" {
                continue;
            }
            match warned.get_mut(key) {
                None => {
                    started.push(text.clone());
                    warned.insert(key.clone(), Instant::now());
                }
                Some(told) if !repeat.is_zero() && told.elapsed() >= repeat => {
                    still.push(text.clone());
                    *told = Instant::now();
                }
                Some(_) => {}
            }
        }
        warned.retain(|key, _| {
            let on = now.iter().any(|(k, _)| k == key);
            if !on {
                ended.push(key.clone());
            }
            on
        });
        drop(warned);
        if started.is_empty() && ended.is_empty() && still.is_empty() {
            return;
        }
        for text in &started {
            self.event("warn", text);
        }
        for text in &still {
            self.event("warn", &format!("still: {text}"));
        }
        for key in &ended {
            self.event("warn", &format!("no longer a problem: {key}"));
        }
        // Told to whoever asked to be - on a thread of its own, because a
        // slow webhook must not be something the server waits for.
        if let Some(url) = self.config.warn_webhook.clone() {
            let server = match &self.cluster {
                Some(cluster) => cluster.config.node.clone(),
                None => "jengine-server".to_string(),
            };
            let body = json!({ "server": server, "started": started, "ended": ended, "still": still }).to_string();
            let retry = self.config.warn_retry_secs.clone();
            std::thread::spawn(move || {
                // Tried again after each pause, then given up on.
                for wait in std::iter::once(0.0).chain(retry) {
                    std::thread::sleep(Duration::from_secs_f64(wait));
                    let sent = ureq::post(&url)
                        .timeout(Duration::from_secs(5))
                        .set("Content-Type", "application/json")
                        .send_string(&body);
                    match sent {
                        Ok(_) => return,
                        Err(e) => log::warn!("[server] the warning webhook did not take it: {e}"),
                    }
                }
                log::error!("[server] the warning webhook was given up on: {body}");
            });
        }
    }

    fn save_history(&self) {
        let samples: Vec<Value> = self.history.lock().unwrap().iter().cloned().collect();
        self.store.save(NS_HISTORY, &self.history_key(), &Value::Array(samples).to_string());
        let hourly: Vec<Value> = self.hourly.lock().unwrap().iter().cloned().collect();
        self.store.save(NS_HISTORY, &format!("hourly:{}", self.history_key()), &Value::Array(hourly).to_string());
        // Today's minutes (and yesterday's, the first save after midnight);
        // the days before are already written and are left alone.
        let today = (unix_now() / 86400.0).floor() as i64;
        let minutes = self.minutes.lock().unwrap();
        for day in [today - 1, today] {
            let of_day: Vec<Value> = minutes
                .iter()
                .filter(|m| m["at"].as_f64().is_some_and(|at| (at / 86400.0).floor() as i64 == day))
                .cloned()
                .collect();
            if !of_day.is_empty() {
                self.store.save(NS_HISTORY, &format!("minutes:{}:{day}", self.history_key()), &Value::Array(of_day).to_string());
            }
        }
        drop(minutes);
        // A week and a day ago is gone.
        self.store.delete(NS_HISTORY, &format!("minutes:{}:{}", self.history_key(), today - 8));
        self.store.save(NS_TOTALS, &self.history_key(), &self.metrics.snapshot().to_string());
    }

    /// The minute that has just ended, as one sample - a week of them is
    /// kept, for a look at last Tuesday evening in more than hours.
    fn roll_minute(&self) {
        let ended = {
            let history = self.history.lock().unwrap();
            let done = self.minutes.lock().unwrap().back().and_then(|m| m["at"].as_f64()).unwrap_or(0.0);
            rolled(&history, done, 60.0)
        };
        if let Some(ended) = ended {
            let mut minutes = self.minutes.lock().unwrap();
            if minutes.len() >= MAX_MINUTES {
                minutes.pop_front();
            }
            minutes.push_back(ended);
        }
    }

    /// The hour that has just ended, as one sample: gauges averaged, the
    /// slowest tick kept, counters as they stood at its end.
    fn roll_hour(&self) {
        let ended = {
            let history = self.history.lock().unwrap();
            let done = self.hourly.lock().unwrap().back().and_then(|h| h["at"].as_f64()).unwrap_or(0.0);
            rolled(&history, done, 3600.0)
        };
        if let Some(ended) = ended {
            let mut hourly = self.hourly.lock().unwrap();
            if hourly.len() >= MAX_HOURLY {
                hourly.pop_front();
            }
            hourly.push_back(ended);
        }
    }

    /// Where this server keeps its samples: its own, in a shared store.
    fn history_key(&self) -> String {
        match &self.cluster {
            Some(cluster) => format!("samples:{}", cluster.config.node),
            None => "samples".to_string(),
        }
    }

    /// Note the numbers as they are now. Counters are cumulative since this
    /// start; whoever draws them takes the differences.
    fn sample(&self) {
        let used = *self.resources.lock().unwrap();
        let (connections, players, rooms, tick_us) = {
            let state = self.state.lock().unwrap();
            let worst =
                state.rooms.values().map(|r| r.stats.tick_max_us.load(Ordering::Relaxed)).max().unwrap_or(0);
            (state.connections, state.online.len(), state.rooms.len(), worst)
        };
        let sample = json!({
            "at": unix_now(),
            "connections": connections,
            "players": players,
            "rooms": rooms,
            "messages_in": self.metrics.messages_in.load(Ordering::Relaxed),
            "bytes_out": self.metrics.bytes_out.load(Ordering::Relaxed),
            "tick_ms": tick_us as f64 / 1000.0,
            "cpu": used.cpu_percent,
            "memory_mb": used.memory_bytes / (1024 * 1024),
            "disk_free_mb": used.disk_free / (1024 * 1024),
            "net_rx_kb": used.net_rx_per_sec / 1024.0,
            "net_tx_kb": used.net_tx_per_sec / 1024.0,
        });
        push_sample(&mut self.history.lock().unwrap(), sample);
    }

    /// Something said in a channel: to everyone in it here, into its backlog
    /// (kept in the store), and to the other nodes of a cluster.
    fn channel_said(&self, name: &str, said: Utf8Bytes, frame: &Value) {
        {
            let mut channels = self.channels.lock().unwrap();
            let channel = channels.entry(name.to_string()).or_insert_with(|| Channel::load(&*self.store, name));
            channel.say(&said);
            let recent: Vec<Value> = channel.recent.iter().filter_map(|s| serde_json::from_str(s.as_str()).ok()).collect();
            self.store.save(NS_CHANNEL, name, &Value::Array(recent).to_string());
        }
        if let Some(cluster) = &self.cluster {
            cluster.publish(&format!("channel:{name}"), frame);
        }
    }

    fn save_player(&self, id: &str, record: &str) {
        self.metrics.player_saves.fetch_add(1, Ordering::Relaxed);
        self.store.save(NS_PLAYER, id, record);
    }

    fn next_serial(&self) -> u64 {
        let mut state = self.state.lock().unwrap();
        state.next_serial += 1;
        state.next_serial
    }

    /// Open a room nobody has asked for yet ([`Server::open_at_start`]).
    fn open_at_start(self: &Arc<Self>, kind: &str, room: &str) {
        if let Some(cluster) = &self.cluster {
            match cluster.claim(kind, room) {
                Ok(None) => {}
                Ok(Some(url)) => return log::info!("[server] {kind}/{room} is opened by the node at {url}"),
                Err(why) => return log::warn!("[server] {kind}/{room} not opened at start: {why}"),
            }
        }
        if let Err(why) = self.pick(&JoinRequest::named(kind, room), &HashSet::new()) {
            log::warn!("[server] {kind}/{room} not opened at start: {why}");
        }
    }

    /// Find the room a request means, opening it if need be.
    fn pick(
        self: &Arc<Self>,
        req: &JoinRequest,
        tried: &HashSet<String>,
    ) -> Result<(String, RoomHandle), String> {
        let kind = self.kinds.get(&req.kind).ok_or_else(|| format!("no room kind '{}'", req.kind))?;
        let mut state = self.state.lock().unwrap();
        let id = match &req.room {
            Some(id) => {
                let id = id.trim();
                if id.is_empty() || id.chars().count() > 64 {
                    return Err("a room needs a name of 1 to 64 characters".into());
                }
                id.to_string()
            }
            None => {
                let open = state.rooms.iter().find(|((k, id), room)| {
                    *k == req.kind
                        && !tried.contains(id)
                        && room.count.load(Ordering::Relaxed) < kind.options.max_players
                });
                match open {
                    Some(((_, id), _)) => id.clone(),
                    None => random_hex(4),
                }
            }
        };
        let key = (req.kind.clone(), id.clone());
        if let Some(room) = state.rooms.get(&key) {
            return Ok((id, room.clone()));
        }
        if state.rooms.len() >= self.config.max_rooms {
            return Err("the server is full".into());
        }
        let Some(done) = self.rooms_done.lock().unwrap().clone() else {
            return Err("the server is stopping".into());
        };
        state.next_serial += 1;
        let (tx, rx) = mpsc::channel(1024);
        let handle = RoomHandle {
            serial: state.next_serial,
            tx,
            count: Arc::new(AtomicUsize::new(0)),
            stats: Arc::new(RoomStats {
                opened: Instant::now(),
                messages: AtomicU64::new(0),
                frames_out: AtomicU64::new(0),
                tick_avg_us: AtomicU64::new(0),
                tick_max_us: AtomicU64::new(0),
            }),
        };
        state.rooms.insert(key, handle.clone());
        tokio::spawn(run_room(self.clone(), req.kind.clone(), id.clone(), handle.clone(), rx, done));
        Ok((id, handle))
    }

    /// Put a player in a room, or hand them back with the reason.
    async fn join(
        self: &Arc<Self>,
        mut player: Player,
        req: &JoinRequest,
        conn: &ConnHandle,
    ) -> Result<(String, RoomHandle), (Player, NotJoined)> {
        // In a cluster a named room lives on one node. If we do not already
        // have it, ask whose it is - which makes it ours if it is nobody's.
        if let Some(cluster) = &self.cluster {
            let elsewhere = if !self.kinds.contains_key(&req.kind) {
                cluster.find(&req.kind)
            } else {
                match &req.room {
                    Some(room) => {
                        let key = (req.kind.clone(), room.trim().to_string());
                        if self.state.lock().unwrap().rooms.contains_key(&key) {
                            Ok(None)
                        } else {
                            cluster.claim(&key.0, &key.1)
                        }
                    }
                    None => Ok(None),
                }
            };
            match elsewhere {
                Ok(Some(url)) => return Err((player, NotJoined::Elsewhere(url))),
                Ok(None) => {}
                Err(why) => return Err((player, NotJoined::Refused(why))),
            }
        }
        let mut tried = HashSet::new();
        for _ in 0..8 {
            let (id, room) = match self.pick(req, &tried) {
                Ok(found) => found,
                Err(why) => return Err((player, NotJoined::Refused(why))),
            };
            // Kept in case the room's task dies with the player inside the
            // command - a join is rare enough to afford the copy.
            let backup = player.clone();
            let (reply, answer) = oneshot::channel();
            let cmd =
                RoomCmd::Join { player, options: req.options.clone(), conn: conn.clone(), reply };
            if room.tx.send(cmd).await.is_err() {
                player = backup;
                tokio::task::yield_now().await;
                continue;
            }
            match answer.await {
                Ok(Ok(())) => {
                    self.metrics.joins.fetch_add(1, Ordering::Relaxed);
                    return Ok((id, room));
                }
                Ok(Err((p, Refusal::Gone))) => player = p,
                // Asked for any room and this one filled first: try another.
                Ok(Err((p, Refusal::Refused(why)))) if req.room.is_none() && why == FULL => {
                    tried.insert(id);
                    player = p;
                }
                Ok(Err((p, Refusal::Refused(why)))) => {
                    self.metrics.joins_refused.fetch_add(1, Ordering::Relaxed);
                    return Err((p, NotJoined::Refused(why)));
                }
                Err(_) => return Err((backup, NotJoined::Refused("the room failed".into()))),
            }
        }
        Err((player, NotJoined::Refused("could not find a room".into())))
    }
}

// ---- a room's task --------------------------------------------------------------

async fn run_room(
    hub: Arc<Hub>,
    kind_name: String,
    id: String,
    me: RoomHandle,
    mut rx: mpsc::Receiver<RoomCmd>,
    _done: mpsc::Sender<()>,
) {
    let kind = &hub.kinds[&kind_name];
    let options = kind.options.clone();
    let mut conns: HashMap<PlayerId, ConnHandle> = HashMap::new();
    // What was last written for each player, so autosave writes only changes.
    let mut saved: HashMap<PlayerId, String> = HashMap::new();
    // Players whose connection dropped, and when their seat stops being held.
    let mut parked: HashMap<PlayerId, Instant> = HashMap::new();

    let made = catch_unwind(AssertUnwindSafe(|| {
        RoomRunner::new(&kind_name, &id, (kind.factory)(&id), hub.store.clone(), options.max_players)
    }));
    let Ok((mut runner, effects)) = made else {
        hub.metrics.room_panics.fetch_add(1, Ordering::Relaxed);
        hub.event("panic", &format!("room {kind_name}/{id} panicked while being created"));
        forget(&hub, &kind_name, &id, &me);
        refuse_queued(&mut rx, "the room failed").await;
        return;
    };
    let mut room = RoomTask {
        hub: &hub,
        kind: &kind_name,
        id: &id,
        conns: &mut conns,
        saved: &mut saved,
        parked: &mut parked,
        stats: &me.stats,
    };
    room.apply(effects);
    let mut grace_check = tokio::time::interval(Duration::from_millis(200));
    grace_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let ticking = options.tick_hz > 0.0;
    let period = Duration::from_secs_f64(if ticking { 1.0 / options.tick_hz } else { 3600.0 });
    let mut tick = tokio::time::interval(period);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_tick = Instant::now();
    let mut autosave =
        tokio::time::interval(Duration::from_secs_f64(hub.config.autosave_secs.max(0.05)));
    autosave.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    hub.event("room", &format!("{kind_name}/{id} opened"));
    let mut failed = false;
    // Tick cost: a moving average, and the worst of each hundred ticks.
    let (mut tick_avg, mut tick_worst, mut tick_count) = (0.0f64, 0u64, 0u32);
    // A persistent room nobody was ever let into is not a place yet: without
    // this, every refused join of a made-up name would leave a room behind.
    // One opened at start is a place from the first: nobody need come.
    let at_start = hub.at_start.contains(&(kind_name.clone(), id.clone()));
    let mut ever_joined = at_start;
    loop {
        // Whether this step could have emptied the room. A tick that moves
        // nobody out must not close a room whose first joiner is on the way.
        let mut may_be_done = false;
        let mut tick_started = None;
        let step: Result<Effects, ()> = tokio::select! {
            cmd = rx.recv() => match cmd {
                None => break,
                Some(RoomCmd::Join { player, options, conn, reply }) => {
                    may_be_done = true;
                    let backup = player.clone();
                    let pid = player.id.clone();
                    match catch_unwind(AssertUnwindSafe(|| runner.join(player, &options))) {
                        Ok(Ok(effects)) => {
                            ever_joined = true;
                            room.conns.insert(pid, conn);
                            let _ = reply.send(Ok(()));
                            Ok(effects)
                        }
                        Ok(Err((player, why))) => {
                            let _ = reply.send(Err((player, Refusal::Refused(why))));
                            Ok(Effects::default())
                        }
                        Err(_) => {
                            let _ = reply.send(Err((backup, Refusal::Refused("the room failed".into()))));
                            Err(())
                        }
                    }
                }
                Some(RoomCmd::Message { player, body, delay }) => {
                    me.stats.messages.fetch_add(1, Ordering::Relaxed);
                    catch_unwind(AssertUnwindSafe(|| runner.message_after(&player, body, delay))).map_err(|_| ())
                }
                Some(RoomCmd::Leave { player }) => {
                    may_be_done = true;
                    catch_unwind(AssertUnwindSafe(|| runner.leave(&player))).map_err(|_| ())
                }
                Some(RoomCmd::Close) => {
                    catch_unwind(AssertUnwindSafe(|| runner.close())).map_err(|_| ())
                }
                Some(RoomCmd::Inspect { reply }) => {
                    match catch_unwind(AssertUnwindSafe(|| runner.inspect())) {
                        Ok(seen) => {
                            let _ = reply.send(seen);
                            continue;
                        }
                        Err(_) => Err(()),
                    }
                }
                Some(RoomCmd::Admin { command, reply }) => {
                    may_be_done = true;
                    match catch_unwind(AssertUnwindSafe(|| runner.admin(command))) {
                        Ok((answer, effects)) => {
                            let _ = reply.send(answer);
                            Ok(effects)
                        }
                        Err(_) => Err(()),
                    }
                }
                Some(RoomCmd::Kick { player, why }) => {
                    may_be_done = true;
                    catch_unwind(AssertUnwindSafe(|| runner.kick(&player, &why))).map_err(|_| ())
                }
                Some(RoomCmd::Notice { frame }) => {
                    catch_unwind(AssertUnwindSafe(|| runner.notice(frame))).map_err(|_| ())
                }
                Some(RoomCmd::EditState { player, state, reply }) => {
                    match catch_unwind(AssertUnwindSafe(|| runner.edit_state(&player, state))) {
                        Ok(Some(effects)) => {
                            let _ = reply.send(json!(true));
                            Ok(effects)
                        }
                        Ok(None) => {
                            let _ = reply.send(json!(false));
                            Ok(Effects::default())
                        }
                        Err(_) => Err(()),
                    }
                }
                Some(RoomCmd::EditWith { player, edit, reply }) => {
                    // `edit` catches its own panics: a route's bug is not the room's.
                    let mut after = None;
                    let done = catch_unwind(AssertUnwindSafe(|| {
                        runner.edit_state_with(&player, |state| {
                            edit(state);
                            after = Some(state.clone());
                        })
                    }));
                    match done {
                        Ok(effects) => {
                            let _ = reply.send(after);
                            Ok(effects.unwrap_or_default())
                        }
                        Err(_) => Err(()),
                    }
                }
                Some(RoomCmd::Dropped { player, done }) => {
                    let held = options.grace_secs > 0.0
                        && room.conns.get(&player).is_some_and(|conn| conn.persist);
                    if held {
                        room.conns.remove(&player);
                        room.parked.insert(player.clone(), Instant::now() + Duration::from_secs_f64(options.grace_secs));
                        hub.state.lock().unwrap().parked.insert(player.clone(), (kind_name.clone(), id.clone()));
                        let _ = done.send(());
                        catch_unwind(AssertUnwindSafe(|| runner.disconnected(&player))).map_err(|_| ())
                    } else {
                        // No grace here, or nobody who could come back: a leave.
                        may_be_done = true;
                        catch_unwind(AssertUnwindSafe(|| runner.leave(&player))).map_err(|_| ())
                    }
                }
                Some(RoomCmd::Rebind { player, conn, reply }) => {
                    if room.parked.remove(&player).is_some() {
                        hub.state.lock().unwrap().parked.remove(&player);
                        room.conns.insert(player.clone(), conn);
                        let _ = reply.send(true);
                        catch_unwind(AssertUnwindSafe(|| runner.rejoin(&player))).map_err(|_| ())
                    } else {
                        let _ = reply.send(false);
                        Ok(Effects::default())
                    }
                }
            },
            _ = grace_check.tick(), if !room.parked.is_empty() => {
                // A seat held too long is given up: the player leaves, as
                // they would have at once without a grace period.
                let now = Instant::now();
                let late: Vec<PlayerId> =
                    room.parked.iter().filter(|(_, until)| **until <= now).map(|(p, _)| p.clone()).collect();
                if late.is_empty() {
                    continue;
                }
                may_be_done = true;
                catch_unwind(AssertUnwindSafe(|| {
                    let mut all = Effects::default();
                    for player in &late {
                        let effects = runner.leave(player);
                        all.frames.extend(effects.frames);
                        all.released.extend(effects.released);
                        all.closed |= effects.closed;
                        all.said.extend(effects.said);
                    }
                    all
                }))
                .map_err(|_| ())
            }
            _ = tick.tick(), if ticking => {
                let now = Instant::now();
                // A stalled server must not hand a room one enormous step.
                let dt = now.duration_since(last_tick).as_secs_f64().min(0.25);
                last_tick = now;
                tick_started = Some(now);
                catch_unwind(AssertUnwindSafe(|| runner.tick(dt))).map_err(|_| ())
            }
            _ = autosave.tick() => {
                room.autosave(&runner);
                continue;
            }
        };
        let Ok(effects) = step else {
            failed = true;
            break;
        };
        let closed = effects.closed;
        may_be_done |= !effects.released.is_empty();
        room.apply(effects);
        me.count.store(runner.player_count(), Ordering::Relaxed);
        if let Some(started) = tick_started {
            let us = started.elapsed().as_micros() as u64;
            tick_avg = if tick_count == 0 { us as f64 } else { tick_avg * 0.95 + us as f64 * 0.05 };
            tick_worst = tick_worst.max(us);
            tick_count += 1;
            me.stats.tick_avg_us.store(tick_avg as u64, Ordering::Relaxed);
            if tick_count % 100 == 0 || tick_worst > me.stats.tick_max_us.load(Ordering::Relaxed) {
                me.stats.tick_max_us.store(tick_worst, Ordering::Relaxed);
            }
            if tick_count % 100 == 0 {
                tick_worst = 0;
            }
        }
        let stays = (options.persistent || at_start) && ever_joined;
        if closed || (may_be_done && runner.player_count() == 0 && !stays) {
            break;
        }
    }

    // No new joiner can find the room from here on; the ones already queued
    // are told to look again.
    forget(&hub, &kind_name, &id, &me);
    if failed {
        hub.metrics.room_panics.fetch_add(1, Ordering::Relaxed);
        hub.event("panic", &format!("room {kind_name}/{id} panicked; its players were released with what they had"));
        for player in runner.abandon() {
            room.release(player, Release::Closed);
        }
    } else if let Ok(effects) = catch_unwind(AssertUnwindSafe(|| runner.close())) {
        room.apply(effects);
    }
    refuse_queued(&mut rx, "").await;
    hub.event("room", &format!("{kind_name}/{id} closed"));
}

fn forget(hub: &Hub, kind: &str, id: &str, me: &RoomHandle) {
    let ours = {
        let mut state = hub.state.lock().unwrap();
        let key = (kind.to_string(), id.to_string());
        let ours = state.rooms.get(&key).is_some_and(|r| r.serial == me.serial);
        if ours {
            state.rooms.remove(&key);
        }
        ours
    };
    // In a cluster the name is free for any node to open again.
    if let (true, Some(cluster)) = (ours, &hub.cluster) {
        cluster.release(kind, id);
    }
}

/// Answer the joins still in a closing room's queue. An empty `why` means
/// "the room went away, try again"; anything else is a refusal.
async fn refuse_queued(rx: &mut mpsc::Receiver<RoomCmd>, why: &str) {
    rx.close();
    while let Some(cmd) = rx.recv().await {
        if let RoomCmd::Join { player, reply, .. } = cmd {
            let refusal =
                if why.is_empty() { Refusal::Gone } else { Refusal::Refused(why.to_string()) };
            let _ = reply.send(Err((player, refusal)));
        }
    }
}

struct RoomTask<'a> {
    hub: &'a Hub,
    kind: &'a str,
    id: &'a str,
    conns: &'a mut HashMap<PlayerId, ConnHandle>,
    saved: &'a mut HashMap<PlayerId, String>,
    parked: &'a mut HashMap<PlayerId, Instant>,
    stats: &'a RoomStats,
}

impl RoomTask<'_> {
    fn apply(&mut self, effects: Effects) {
        // A broadcast is one shared value; serialise it once.
        let mut last: Option<(*const Value, Utf8Bytes)> = None;
        let mut sent = 0u64;
        for (to, frame) in &effects.frames {
            let Some(conn) = self.conns.get(to) else { continue };
            let text = match frame {
                Frame::Text(text) => Utf8Bytes::from(&**text),
                Frame::Json(value) => {
                    let ptr = Arc::as_ptr(value);
                    match &last {
                        Some((p, text)) if *p == ptr => text.clone(),
                        _ => {
                            let text = Utf8Bytes::from(value.to_string());
                            last = Some((ptr, text.clone()));
                            text
                        }
                    }
                }
            };
            sent += 1;
            if let Err(mpsc::error::TrySendError::Full(_)) = conn.out.try_send(text) {
                let _ = conn.ctrl.send(ConnEvent::TooSlow);
            }
        }
        if sent > 0 {
            self.stats.frames_out.fetch_add(sent, Ordering::Relaxed);
            self.hub.metrics.frames_out.fetch_add(sent, Ordering::Relaxed);
        }
        for (channel, frame) in effects.said {
            if self.hub.channel_policy.is_none() {
                log::warn!("[room {}/{}] said in '{channel}', but this server has no channels", self.kind, self.id);
                continue;
            }
            self.hub.channel_said(&channel, Utf8Bytes::from(frame.to_string()), &frame);
        }
        for (player, why) in effects.released {
            self.release(player, why);
        }
    }

    fn release(&mut self, player: Player, why: Release) {
        self.saved.remove(&player.id);
        if self.parked.remove(&player.id).is_some() {
            // Nobody is connected to hand them to, so the room saves them -
            // with this room as where they were, to come back to.
            self.hub.state.lock().unwrap().parked.remove(&player.id);
            self.hub.save_player(&player.id, &record(&player, Some((self.kind, self.id))));
            // The seat was the last thing keeping them "here" for the cluster.
            if let Some(cluster) = &self.hub.cluster {
                cluster.offline(&player.id);
            }
            return;
        }
        let Some(conn) = self.conns.remove(&player.id) else { return };
        let persist = conn.persist;
        if let Err(mpsc::error::SendError(ConnEvent::Released(player, _))) =
            conn.ctrl.send(ConnEvent::Released(player, why))
        {
            // The connection is already gone, so nobody else will save this.
            if persist {
                self.hub.save_player(&player.id, &record(&player, Some((self.kind, self.id))));
            }
        }
    }

    fn autosave(&mut self, runner: &RoomRunner) {
        let mut changed: Vec<(PlayerId, String)> = Vec::new();
        for player in runner.players() {
            let kept = self.conns.get(&player.id).is_some_and(|c| c.persist)
                || self.parked.contains_key(&player.id);
            if !kept {
                continue;
            }
            let now = record(player, Some((self.kind, self.id)));
            if self.saved.get(&player.id) != Some(&now) {
                changed.push((player.id.clone(), now));
            }
        }
        if changed.is_empty() {
            return;
        }
        // Together: for a store a network away this is one request, not one
        // for every player in the room.
        let items: Vec<(&str, &str, &str)> =
            changed.iter().map(|(id, record)| (NS_PLAYER, id.as_str(), record.as_str())).collect();
        self.hub.metrics.player_saves.fetch_add(items.len() as u64, Ordering::Relaxed);
        self.hub.store.save_many(&items);
        self.saved.extend(changed);
    }
}

// ---- a connection's task ----------------------------------------------------------

async fn upgrade(
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
    State(hub): State<Arc<Hub>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
) -> Response {
    let Ok(ws) = ws else {
        return "jengine-server: connect with a WebSocket\n".into_response();
    };
    let Some(done) = hub.conns_done.lock().unwrap().clone() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "stopping").into_response();
    };
    if hub.draining.load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "draining").into_response();
    }
    hub.metrics.connections_total.fetch_add(1, Ordering::Relaxed);
    {
        let mut state = hub.state.lock().unwrap();
        if state.connections >= hub.config.max_connections {
            return (StatusCode::SERVICE_UNAVAILABLE, "full").into_response();
        }
        state.connections += 1;
    }
    // Behind the gateway every connection comes from this machine; the
    // address it carried the connection for is the player's.
    let address = api::caller_ip(peer.ip(), &headers, &hub.config.trusted_proxies);
    // Whether a password said on this socket could be read on the way.
    let in_clear = hub.in_clear(peer.ip(), &headers);
    let max = hub.config.max_frame_bytes;
    ws.max_message_size(max).max_frame_size(max).on_upgrade(move |socket| async move {
        run_conn(hub.clone(), socket, address, in_clear).await;
        hub.state.lock().unwrap().connections -= 1;
        drop(done);
    })
}

/// `ok` while the server wants players; 503 once it is draining, which is
/// how a load balancer learns to stop sending them here.
async fn healthz(State(hub): State<Arc<Hub>>) -> Response {
    if hub.draining.load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "draining").into_response();
    }
    "ok".into_response()
}

async fn stats(
    State(hub): State<Arc<Hub>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
) -> Response {
    if !hub.config.public_stats {
        if let Err(no) = admin::plain_refused(&hub, peer.ip(), &headers) {
            return no;
        }
        if let Err(no) = admin::authorize(&hub, &headers) {
            return no;
        }
    }
    let used = *hub.resources.lock().unwrap();
    let state = hub.state.lock().unwrap();
    let mut kinds: HashMap<&str, (usize, usize)> = HashMap::new();
    for ((kind, _), room) in &state.rooms {
        let entry = kinds.entry(kind).or_default();
        entry.0 += 1;
        entry.1 += room.count.load(Ordering::Relaxed);
    }
    let kinds: serde_json::Map<String, Value> = kinds
        .into_iter()
        .map(|(k, (rooms, players))| (k.to_string(), json!({ "rooms": rooms, "players": players })))
        .collect();
    axum::Json(json!({
        "connections": state.connections,
        "players": state.online.len(),
        "rooms": state.rooms.len(),
        "kinds": kinds,
        "cpu": used.cpu_percent,
        "memory_mb": used.memory_bytes / (1024 * 1024),
        "uptime_secs": hub.started.elapsed().as_secs(),
        // Counters that only go up (kept across restarts, as the dashboard
        // keeps them): whoever samples them takes the rate.
        "totals": hub.metrics.snapshot(),
    }))
    .into_response()
}

struct Conn {
    hub: Arc<Hub>,
    serial: u64,
    /// Where the player is connected from: the peer, or who the gateway or a
    /// trusted proxy carried the connection for.
    address: std::net::IpAddr,
    /// The socket crossed a network unencrypted: no password is taken on it
    /// (`Config::allow_plain_passwords`).
    in_clear: bool,
    handle: ConnHandle,
    /// The player, while in no room. `None` before `hello` and while a room
    /// has them.
    player: Option<Player>,
    id: Option<PlayerId>,
    room: Option<(String, String, RoomHandle)>,
    /// Saved with the player so a client can go back after a dropped
    /// connection. Cleared when they leave on purpose.
    last_room: Option<(String, String)>,
    /// Set when another connection has signed in as this player.
    replaced: Option<oneshot::Sender<()>>,
    dropping: bool,
    /// The socket went away without anyone deciding it should.
    lost: bool,
    failed_hellos: u32,
    /// The player the cluster has recorded as being on this node, through us.
    clustered: Option<PlayerId>,
    /// The name others see in a channel.
    name: String,
    /// The channels this connection is in.
    subs: HashSet<String>,
    tokens: f64,
    refilled: Instant,
    /// A message arriving in parts: its id, the next part due, how many
    /// there are, and the text so far.
    parts: Option<(u64, u64, u64, String)>,
    /// The client answers pings (it said so in its `hello`).
    pings: bool,
    /// The ping waiting for its pong: its number, and when it went.
    ping: Option<(u64, Instant)>,
    /// The connection's round trip in seconds, smoothed, once measured.
    rtt: Option<f64>,
}

fn error(why: &str) -> Value {
    json!({ "t": "error", "why": why })
}

/// What a password `hello` (or `account` frame) is told over a connection
/// that crossed the network unencrypted.
pub const PASSWORD_NEEDS_TLS: &str =
    "a password is only taken over an encrypted connection (wss://): this one crosses the network in plain text";

/// Whether a frame carries a password, or something as good as one: a
/// sign-in or registration, a recovery code, a change of password.
fn carries_password(frame: &Value) -> bool {
    let said = |key: &str| frame.get(key).and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    said("password") || said("code") || frame.get("change_password").is_some_and(Value::is_object)
}

impl Conn {
    fn tell(&self, frame: Value) {
        if self.handle.out.try_send(Utf8Bytes::from(frame.to_string())).is_err() {
            let _ = self.handle.ctrl.send(ConnEvent::TooSlow);
        }
    }

    fn save(&self) {
        if let (true, Some(player)) = (self.handle.persist, &self.player) {
            let room = self.last_room.as_ref().map(|(k, i)| (k.as_str(), i.as_str()));
            self.hub.save_player(&player.id, &record(player, room));
        }
    }

    /// Keep the hub's list of who is online up to date with where we are.
    fn note_room(&self) {
        let Some(id) = &self.id else { return };
        let mut state = self.hub.state.lock().unwrap();
        if let Some(online) = state.online.get_mut(id).filter(|o| o.serial == self.serial) {
            online.room = self.room.as_ref().map(|(kind, id, _)| (kind.clone(), id.clone()));
        }
    }

    fn online(&self, name: &str) -> Online {
        Online {
            serial: self.serial,
            ctrl: self.handle.ctrl.clone(),
            name: name.to_string(),
            address: self.address,
            rtt_ms: self.rtt.map(|r| r * 1000.0),
            since: unix_now(),
            room: None,
        }
    }

    /// Become somebody: by `hello`, or as an unsaved guest when a client
    /// goes straight to `join` the way a relay client does.
    async fn identify(&mut self, hello: Option<&Value>) -> Result<(), String> {
        let Some(hello) = hello else {
            let id = format!("g_{}", random_hex(8));
            self.player = Some(Player::new(&id, ""));
            self.id = Some(id);
            return Ok(());
        };
        // Stretching a password is meant to be slow; it must not hold up the
        // other connections on this thread.
        let identity =
            tokio::task::block_in_place(|| self.hub.auth.authenticate(hello, &*self.hub.store))?;

        // Ask for the record before anything is changed: if the store cannot
        // answer, nobody is displaced and nothing is recorded.
        if self.hub.store.try_load(NS_PLAYER, &identity.player).is_err() {
            return Err(STORE_DOWN.into());
        }

        // In a cluster the same holds across nodes: the coordinator has the
        // node that had this player save them and let go before it answers.
        // Unless that node is holding their seat - then they belong back
        // there, and are sent.
        if let Some(cluster) = self.hub.cluster.clone() {
            if let Some(url) = cluster.online(&identity.player)?.filter(|url| *url != cluster.config.public_url) {
                self.tell(json!({ "t": "redirect", "url": url }));
                return Ok(());
            }
            self.clustered = Some(identity.player.clone());
        }

        // One connection per player: two would be two writers of one state.
        // The older one is asked to save and go, and is waited for, so what
        // is loaded below is what it saved.
        for _ in 0..3 {
            let (wait, old) = {
                let mut state = self.hub.state.lock().unwrap();
                match state.online.get(&identity.player) {
                    Some(online) => {
                        let (done, wait) = oneshot::channel();
                        let _ = online.ctrl.send(ConnEvent::Replaced(done));
                        (Some(wait), true)
                    }
                    None => {
                        state.online.insert(
                            identity.player.clone(),
                            self.online(&identity.name),
                        );
                        (None, false)
                    }
                }
            };
            if let Some(wait) = wait {
                let _ = tokio::time::timeout(Duration::from_secs(5), wait).await;
            }
            if !old {
                break;
            }
        }
        {
            // After three tries the old connection is not answering; take over.
            let mut state = self.hub.state.lock().unwrap();
            state.online.insert(identity.player.clone(), self.online(&identity.name));
        }

        let saved: Value = match self.hub.store.try_load(NS_PLAYER, &identity.player) {
            Ok(text) => text.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null),
            // It answered a moment ago and not now. Starting this player
            // from nothing would end with nothing saved over what they had.
            Err(_) => {
                let mut state = self.hub.state.lock().unwrap();
                if state.online.get(&identity.player).is_some_and(|o| o.serial == self.serial) {
                    state.online.remove(&identity.player);
                }
                self.clustered = None;
                return Err(STORE_DOWN.into());
            }
        };
        let mut player = Player::new(&identity.player, "");
        player.name = [identity.name.as_str(), saved["name"].as_str().unwrap_or("")]
            .into_iter()
            .find(|n| !n.trim().is_empty())
            .unwrap_or("")
            .chars()
            .take(24)
            .collect();
        if saved["state"].is_object() {
            player.state = saved["state"].clone();
        }
        self.last_room = match (saved["room"]["kind"].as_str(), saved["room"]["id"].as_str()) {
            (Some(kind), Some(id)) => Some((kind.to_string(), id.to_string())),
            _ => None,
        };
        self.handle.persist = true;
        self.name = player.name.clone();
        self.tell(json!({
            "t": "welcome",
            "player": player.id,
            "token": identity.token,
            "name": player.name,
            "state": player.state,
            "room": saved["room"],
        }));

        // A room may be holding this player's seat from a connection that
        // dropped. Then the room has the player, and this connection takes
        // the old one's place: `joined` follows, and nobody saw them leave.
        let held = {
            let state = self.hub.state.lock().unwrap();
            state
                .parked
                .get(&identity.player)
                .and_then(|key| state.rooms.get(key).map(|room| (key.clone(), room.clone())))
        };
        if let Some(((kind, id), room)) = held {
            let (reply, answer) = oneshot::channel();
            let cmd = RoomCmd::Rebind { player: identity.player.clone(), conn: self.handle.clone(), reply };
            let back = room.tx.send(cmd).await.is_ok()
                && matches!(tokio::time::timeout(Duration::from_secs(3), answer).await, Ok(Ok(true)));
            if back {
                self.hub.event("player", &format!("{} ({}) is back in {kind}/{id}", player.name, player.id));
                self.id = Some(identity.player);
                self.last_room = Some((kind.clone(), id.clone()));
                self.room = Some((kind, id, room));
                self.note_room();
                return Ok(());
            }
            // Too late: the seat was given up and the room saved them on the
            // way out, after the record above was read. Read it again.
            if let Some(newer) = self
                .hub
                .store
                .load(NS_PLAYER, &identity.player)
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .filter(|v| v["state"].is_object())
            {
                player.state = newer["state"].clone();
            }
        }
        {
            let mut state = self.hub.state.lock().unwrap();
            if let Some(online) = state.online.get_mut(&identity.player) {
                online.name = player.name.clone();
            }
        }
        self.hub.event("player", &format!("{} ({}) signed in from {}", player.name, player.id, self.address));
        self.id = Some(identity.player);
        self.player = Some(player);
        self.save();
        Ok(())
    }

    /// Come out of the current room and wait to be handed the player back.
    async fn leave_room(&mut self, ctrl: &mut mpsc::UnboundedReceiver<ConnEvent>) {
        let Some((_, _, room)) = self.room.take() else { return };
        let Some(id) = self.id.clone() else { return };
        let _ = room.tx.send(RoomCmd::Leave { player: id.clone() }).await;
        let wait = async {
            while let Some(event) = ctrl.recv().await {
                match event {
                    ConnEvent::Released(player, _) => return Some(player),
                    ConnEvent::Replaced(done) => self.replaced = Some(done),
                    ConnEvent::TooSlow | ConnEvent::Disconnect(_) => self.dropping = true,
                    // They are in a room, or just were: the room's copy is the one that counts.
                    // (An `EditWith` dropped here drops its reply, and the route tries again.)
                    ConnEvent::Edit(_) | ConnEvent::EditWith(..) => {}
                }
            }
            None
        };
        self.player = match tokio::time::timeout(Duration::from_secs(5), wait).await {
            Ok(Some(player)) => Some(player),
            // The room never answered. What it last saved is the best there is.
            _ => {
                log::error!("[server] a room did not release player {id}; using their last save");
                let saved: Value = self
                    .hub
                    .store
                    .load(NS_PLAYER, &id)
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or(Value::Null);
                let mut player = Player::new(&id, saved["name"].as_str().unwrap_or(""));
                if saved["state"].is_object() {
                    player.state = saved["state"].clone();
                }
                Some(player)
            }
        };
        self.save();
    }

    async fn join(&mut self, req: JoinRequest, ctrl: &mut mpsc::UnboundedReceiver<ConnEvent>) -> Joining {
        if self.player.is_none() && self.id.is_none() {
            // Cannot fail: a guest needs no one's leave.
            let _ = self.identify(None).await;
        }
        // Already there - which is what a client that reconnected and was
        // put back in its held seat finds when it asks for its room again.
        if let Some((kind, id, _)) = &self.room
            && *kind == req.kind
            && req.room.as_deref() == Some(id.as_str())
        {
            return Joining::In;
        }
        self.leave_room(ctrl).await;
        let Some(player) = self.player.take() else { return Joining::Refused };
        match self.hub.join(player, &req, &self.handle).await {
            Ok((id, room)) => {
                self.last_room = Some((req.kind.clone(), id.clone()));
                self.room = Some((req.kind, id, room));
                self.note_room();
                Joining::In
            }
            Err((player, NotJoined::Refused(why))) => {
                self.player = Some(player);
                self.tell(error(&why));
                Joining::Refused
            }
            Err((player, NotJoined::Elsewhere(url))) => {
                // Saved before the client is told to go - and sent, if the
                // store writes behind: the node it arrives at loads exactly this.
                self.player = Some(player);
                self.last_room = req.room.clone().map(|room| (req.kind.clone(), room));
                self.save();
                let store = self.hub.store.clone();
                tokio::task::block_in_place(|| store.flush());
                let mut frame = json!({ "t": "redirect", "url": url, "kind": req.kind, "room": req.room });
                if !req.options.is_null() && req.options.get("t").is_none() {
                    frame["options"] = req.options;
                }
                self.tell(frame);
                Joining::Redirected
            }
        }
    }

    /// A room let go of the player without being asked to.
    async fn released(
        &mut self,
        player: Player,
        why: Release,
        ctrl: &mut mpsc::UnboundedReceiver<ConnEvent>,
    ) {
        self.room = None;
        self.note_room();
        self.player = Some(player);
        match why {
            Release::Transfer(to) => {
                // Saved before the move, so a failure on the way loses nothing.
                self.save();
                match self.join(to, ctrl).await {
                    Joining::Refused => {
                        self.last_room = None;
                        self.tell(json!({ "t": "left" }));
                    }
                    // Sent to another node with where to go saved: leave it so.
                    Joining::Redirected => return,
                    Joining::In => {}
                }
            }
            Release::Kicked(why) => {
                self.last_room = None;
                self.tell(json!({ "t": "kicked", "why": why }));
            }
            Release::Closed | Release::Left => {
                self.last_room = None;
                self.tell(json!({ "t": "closed" }));
            }
        }
        self.save();
    }

    /// Ask the room to hold this player's seat instead of leaving it. `false`
    /// if this kind of room does not, or there is nobody to hold it for.
    async fn hold_seat(&mut self) -> bool {
        let (Some((kind, _, room)), Some(id), true) = (&self.room, &self.id, self.handle.persist) else {
            return false;
        };
        if self.hub.kinds.get(kind).is_none_or(|k| k.options.grace_secs <= 0.0) {
            return false;
        }
        let (room, id) = (room.clone(), id.clone());
        let (done, parked) = oneshot::channel();
        if room.tx.send(RoomCmd::Dropped { player: id, done }).await.is_err() {
            return false;
        }
        // Wait for the room to hold the seat. The connection signing in as
        // this player next is let in only after this one is done, and it
        // looks for a held seat once: sent and not waited for, the seat was
        // not held yet when a busy machine got there first, and the room
        // refused the newcomer as "already in this room".
        let _ = tokio::time::timeout(Duration::from_secs(3), parked).await;
        self.room = None;
        true
    }

    fn leave_channels(&mut self) {
        for name in std::mem::take(&mut self.subs) {
            self.leave_channel(&name);
        }
    }

    /// Come out of one channel, and tell whoever is still in it.
    fn leave_channel(&self, name: &str) {
        let mut channels = self.hub.channels.lock().unwrap();
        let Some(channel) = channels.get_mut(name) else { return };
        let Some(me) = channel.members.remove(&self.serial) else { return };
        if channel.members.is_empty() {
            // Nobody left to remember it for.
            channels.remove(name);
            return;
        }
        let gone = Utf8Bytes::from(
            json!({ "t": "presence", "channel": name, "left": { "player": me.player, "name": me.name } }).to_string(),
        );
        for member in channel.members.values() {
            let _ = member.out.try_send(gone.clone());
        }
    }

    /// `sub`, `unsub` and `say`: channels, beside whatever room we are in.
    fn channel(&mut self, verb: &str, frame: &Value) {
        let hub = self.hub.clone();
        let Some(policy) = hub.channel_policy.as_ref() else {
            return self.tell(error("this server has no channels"));
        };
        let (Some(player), true) = (self.id.clone(), self.handle.persist) else {
            return self.tell(error("sign in to use channels"));
        };
        let name = frame["channel"].as_str().unwrap_or("").trim().to_string();
        if name.is_empty() || name.chars().count() > 64 {
            return self.tell(error("a channel needs a name of 1 to 64 characters"));
        }
        let my_name = self.name.clone();
        let ask = |saying| ChannelAsk { player: &player, name: &my_name, channel: &name, saying };
        match verb {
            "sub" => {
                if self.subs.contains(&name) {
                    return;
                }
                if self.subs.len() >= MAX_SUBSCRIPTIONS {
                    return self.tell(error("too many channels"));
                }
                if !policy(&ask(false)) {
                    return self.tell(error("you may not join that channel"));
                }
                // Who is here, and what was just said, for the one arriving;
                // that they arrived, for the ones here.
                let (members, recent) = {
                    let mut channels = self.hub.channels.lock().unwrap();
                    let channel = channels.entry(name.clone()).or_insert_with(|| Channel::load(&*self.hub.store, &name));
                    let arrived = Utf8Bytes::from(
                        json!({ "t": "presence", "channel": name, "joined": { "player": player, "name": my_name } })
                            .to_string(),
                    );
                    for member in channel.members.values() {
                        let _ = member.out.try_send(arrived.clone());
                    }
                    channel.members.insert(
                        self.serial,
                        Member { out: self.handle.out.clone(), player: player.clone(), name: my_name.clone() },
                    );
                    let members: Vec<Value> =
                        channel.members.values().map(|m| json!({ "player": m.player, "name": m.name })).collect();
                    let recent: Vec<Value> =
                        channel.recent.iter().filter_map(|said| serde_json::from_str(said.as_str()).ok()).collect();
                    (members, recent)
                };
                self.tell(json!({ "t": "subbed", "channel": name, "members": members, "recent": recent }));
                self.subs.insert(name);
            }
            "unsub" => {
                if self.subs.remove(&name) {
                    self.leave_channel(&name);
                    self.tell(json!({ "t": "unsubbed", "channel": name }));
                }
            }
            _ => {
                if !self.subs.contains(&name) {
                    return self.tell(error("join that channel first"));
                }
                let body = &frame["body"];
                if body.to_string().len() > MAX_SAY_BYTES {
                    return self.tell(error("that message is too large"));
                }
                if !policy(&ask(true)) {
                    return self.tell(error("you may not speak in that channel"));
                }
                let muted = self.hub.channels.lock().unwrap().get(&name).is_some_and(|c| c.is_muted(&player));
                if muted {
                    return self.tell(error("an operator has muted you in that channel"));
                }
                if !self.allowed() {
                    self.hub.metrics.rate_limited.fetch_add(1, Ordering::Relaxed);
                    return self.tell(error("slow down"));
                }
                let said = Utf8Bytes::from(
                    json!({ "t": "said", "channel": name, "from": player, "name": my_name, "body": body })
                        .to_string(),
                );
                // To everyone in it, the speaker included: what comes back is
                // the order, as in a relay.
                let frame: Value = serde_json::from_str(said.as_str()).unwrap_or(Value::Null);
                hub.channel_said(&name, said, &frame);
            }
        }
    }

    fn allowed(&mut self) -> bool {
        let rate = self.hub.config.messages_per_second;
        let now = Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.refilled).as_secs_f64() * rate).min(rate);
        self.refilled = now;
        if self.tokens < 1.0 {
            return false;
        }
        self.tokens -= 1.0;
        true
    }

    /// A message to the room the player is in, already counted.
    async fn deliver(&mut self, body: Value) {
        let (Some((_, _, room)), Some(id)) = (&self.room, &self.id) else {
            return self.tell(error("join a room first"));
        };
        let (room, id) = (room.clone(), id.clone());
        self.hub.metrics.messages_in.fetch_add(1, Ordering::Relaxed);
        // Sent half a round trip ago, as far as the server can tell.
        let delay = self.rtt.map_or(0.0, |rtt| rtt / 2.0);
        let _ = room.tx.send(RoomCmd::Message { player: id, body, delay }).await;
    }

    async fn frame(&mut self, text: &str, ctrl: &mut mpsc::UnboundedReceiver<ConnEvent>) {
        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return self.tell(error("not JSON"));
        };
        match frame.get("t").and_then(Value::as_str) {
            Some("hello") => {
                if self.id.is_some() {
                    return self.tell(error("already signed in"));
                }
                self.pings = frame["pings"] == true;
                // A password on a socket anyone on the way could read is
                // turned away before it is checked: refused, the client is
                // told why, and nothing is counted against the account.
                if self.in_clear && !self.hub.config.allow_plain_passwords && carries_password(&frame) {
                    return self.tell(error(PASSWORD_NEEDS_TLS));
                }
                if let Err(why) = self.identify(Some(&frame)).await {
                    self.tell(error(&why));
                    // The store being away is not the client guessing, and
                    // a recovery code on its way is not a wrong answer.
                    if why != STORE_DOWN && why != crate::auth::RECOVERY_SENT {
                        self.failed_hellos += 1;
                    }
                    if self.failed_hellos >= MAX_FAILED_HELLOS {
                        self.dropping = true;
                    }
                }
            }
            Some("join") => {
                let kind = frame
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| self.hub.config.default_kind.clone());
                let Some(kind) = kind else {
                    return self.tell(error("join needs a kind"));
                };
                let req = JoinRequest {
                    kind,
                    room: frame.get("room").and_then(Value::as_str).map(str::to_string),
                    // A relay client puts `seats` and `name` on the frame itself.
                    options: frame.get("options").cloned().unwrap_or_else(|| frame.clone()),
                };
                self.join(req, ctrl).await;
            }
            Some("send") => {
                let Some(body) = frame.get("body") else {
                    return self.tell(error("send needs a body"));
                };
                if !self.allowed() {
                    self.hub.metrics.rate_limited.fetch_add(1, Ordering::Relaxed);
                    return self.tell(error("slow down"));
                }
                self.deliver(body.clone()).await;
            }
            // A `send` too big for one frame, in pieces of its text: each
            // piece is a message to the rate limit, the whole one to the room.
            Some("part") => {
                let (Some(id), Some(i), Some(of), Some(text)) =
                    (frame["id"].as_u64(), frame["i"].as_u64(), frame["of"].as_u64(), frame["text"].as_str())
                else {
                    return self.tell(error("a part needs id, i, of and text"));
                };
                if !self.allowed() {
                    self.parts = None;
                    self.hub.metrics.rate_limited.fetch_add(1, Ordering::Relaxed);
                    return self.tell(error("slow down"));
                }
                // A new message starts at its first part; anything else must
                // be the next part of the one under way.
                if i == 0 {
                    self.parts = Some((id, 0, of, String::new()));
                }
                let Some((at, next, total, so_far)) = &mut self.parts else {
                    return self.tell(error("a part of no message"));
                };
                if *at != id || *next != i || *total != of {
                    self.parts = None;
                    return self.tell(error("a part out of order"));
                }
                so_far.push_str(text);
                *next += 1;
                if so_far.len() > self.hub.config.max_message_bytes {
                    self.parts = None;
                    return self.tell(error("message too large"));
                }
                if *next < *total {
                    return;
                }
                let Some((.., whole)) = self.parts.take() else { return };
                let whole: Value = match serde_json::from_str(&whole) {
                    Ok(whole) => whole,
                    Err(_) => return self.tell(error("not JSON")),
                };
                match (whole["t"].as_str(), whole.get("body")) {
                    (Some("send"), Some(body)) => self.deliver(body.clone()).await,
                    _ => self.tell(error("only a send comes in parts")),
                }
            }
            // The answer to a ping: a round-trip sample.
            Some("pong") => {
                if let (Some((n, sent)), Some(got)) = (self.ping, frame["n"].as_u64())
                    && n == got
                {
                    self.ping = None;
                    let sample = sent.elapsed().as_secs_f64();
                    let rtt = self.rtt.map_or(sample, |rtt| rtt * 0.7 + sample * 0.3);
                    self.rtt = Some(rtt);
                    if let Some(id) = &self.id {
                        let mut state = self.hub.state.lock().unwrap();
                        if let Some(online) = state.online.get_mut(id).filter(|o| o.serial == self.serial) {
                            online.rtt_ms = Some(rtt * 1000.0);
                        }
                    }
                }
            }
            Some("leave") => {
                if self.room.is_some() {
                    self.leave_room(ctrl).await;
                    self.note_room();
                    self.last_room = None;
                    self.save();
                    self.tell(json!({ "t": "left" }));
                }
            }
            Some(verb @ ("sub" | "unsub" | "say")) => self.channel(verb, &frame),
            Some("account") => {
                let (Some(player), true) = (self.id.clone(), self.handle.persist) else {
                    return self.tell(error("sign in first"));
                };
                if self.in_clear && !self.hub.config.allow_plain_passwords && carries_password(&frame) {
                    return self.tell(error(PASSWORD_NEEDS_TLS));
                }
                // Hashing a password is slow on purpose; not on this thread's time.
                let hub = self.hub.clone();
                match tokio::task::block_in_place(|| hub.auth.manage(&player, &frame, &*hub.store)) {
                    Ok(mut answer) => {
                        answer["t"] = json!("account");
                        self.tell(answer);
                    }
                    Err(why) => self.tell(error(&why)),
                }
            }
            _ => self.tell(error("unknown frame")),
        }
    }
}

async fn run_conn(hub: Arc<Hub>, socket: WebSocket, address: std::net::IpAddr, in_clear: bool) {
    let (mut sink, mut stream) = socket.split();
    let (out, mut out_rx) = mpsc::channel::<Utf8Bytes>(hub.config.outbox);
    let (ctrl_tx, mut ctrl) = mpsc::unbounded_channel();
    // Every byte written to a player is counted here, where it is written:
    // the server's own replies and channel lines as well as rooms' frames,
    // and nothing a slow client's full outbox dropped.
    let counted = hub.clone();
    let writer = tokio::spawn(async move {
        while let Some(text) = out_rx.recv().await {
            let len = text.len() as u64;
            if sink.send(Message::Text(text)).await.is_err() {
                break;
            }
            counted.metrics.bytes_out.fetch_add(len, Ordering::Relaxed);
        }
        let _ = sink.close().await;
    });

    let mut shutdown = hub.shutdown.clone();
    let rate = hub.config.messages_per_second;
    let mut conn = Conn {
        serial: hub.next_serial(),
        address,
        in_clear,
        hub,
        handle: ConnHandle { out, ctrl: ctrl_tx, persist: false },
        player: None,
        id: None,
        room: None,
        last_room: None,
        replaced: None,
        dropping: false,
        lost: false,
        failed_hellos: 0,
        clustered: None,
        name: String::new(),
        subs: HashSet::new(),
        tokens: rate,
        refilled: Instant::now(),
        parts: None,
        pings: false,
        ping: None,
        rtt: None,
    };

    conn.hub.state.lock().unwrap().outboxes.insert(conn.serial, conn.handle.out.clone());

    // Pings for a client that answers them: what its round trip is measured
    // by. One at a time; a ping unanswered by the next is given up on.
    let mut pinging = tokio::time::interval(Duration::from_secs_f64(conn.hub.config.ping_secs.max(0.05)));
    pinging.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut next_ping = 0u64;
    while conn.replaced.is_none() && !conn.dropping && !*shutdown.borrow() {
        tokio::select! {
            _ = pinging.tick(), if conn.pings => {
                next_ping += 1;
                conn.ping = Some((next_ping, Instant::now()));
                conn.tell(json!({ "t": "ping", "n": next_ping }));
            }
            msg = stream.next() => match msg {
                Some(Ok(Message::Text(text))) => {
                    conn.hub.metrics.bytes_in.fetch_add(text.len() as u64, Ordering::Relaxed);
                    conn.frame(text.as_str(), &mut ctrl).await
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                    conn.lost = true;
                    break;
                }
                Some(Ok(_)) => {}
            },
            event = ctrl.recv() => match event {
                Some(ConnEvent::Released(player, why)) => conn.released(player, why, &mut ctrl).await,
                Some(ConnEvent::Replaced(done)) => conn.replaced = Some(done),
                Some(ConnEvent::TooSlow) => {
                    conn.hub.metrics.slow_clients_dropped.fetch_add(1, Ordering::Relaxed);
                    let who = conn.id.clone().unwrap_or_default();
                    conn.hub.event("slow", &format!("dropped {who}: not reading fast enough"));
                    conn.dropping = true;
                }
                Some(ConnEvent::Disconnect(why)) => {
                    conn.tell(error(&why));
                    conn.dropping = true;
                }
                Some(ConnEvent::Edit(state)) => {
                    if let Some(player) = &mut conn.player {
                        player.state = state.clone();
                        conn.save();
                        conn.tell(json!({ "t": "state", "sys": true, "state": state }));
                    }
                }
                Some(ConnEvent::EditWith(edit, reply)) => {
                    if let Some(player) = &mut conn.player {
                        edit(&mut player.state);
                        let state = player.state.clone();
                        conn.save();
                        conn.tell(json!({ "t": "state", "sys": true, "state": state }));
                        let _ = reply.send(Some(state));
                    }
                }
                None => conn.dropping = true,
            },
            _ = shutdown.changed() => {}
        }
    }

    // However it ended: out of the room, saved, and off the list - in that
    // order, so whoever signs in next as this player loads what was saved.
    conn.hub.state.lock().unwrap().outboxes.remove(&conn.serial);
    conn.leave_channels();
    let held = conn.lost && conn.hold_seat().await;
    if let (false, Some(cluster), Some(player)) = (held, conn.hub.cluster.clone(), conn.clustered.take()) {
        // Leave first, so "offline" is said after the save it promises -
        // and that save is in the coordinator's store, not on its way.
        conn.leave_room(&mut ctrl).await;
        conn.save();
        let store = conn.hub.store.clone();
        tokio::task::block_in_place(|| store.flush());
        cluster.offline(&player);
    }
    if held {
        // The room keeps the player. If it had just let go of them (a
        // transfer on its way), the hand-back is waiting here: take it.
        // Closed first, so the hand-off has no gap: a room letting go of
        // them from now on finds no connection to hand them to, and saves
        // them itself - where an open channel would have taken the player
        // into a queue nobody reads (S11).
        ctrl.close();
        while let Ok(event) = ctrl.try_recv() {
            if let ConnEvent::Released(player, _) = event {
                conn.player = Some(player);
            }
        }
    } else {
        conn.leave_room(&mut ctrl).await;
    }
    conn.save();
    if conn.replaced.is_some() {
        // Whoever is waiting loads the player next - through the store, so
        // what was just saved must be in it, not on its way.
        let store = conn.hub.store.clone();
        tokio::task::block_in_place(|| store.flush());
        conn.tell(error("signed in somewhere else"));
    }
    if let Some(id) = &conn.id {
        let mut state = conn.hub.state.lock().unwrap();
        if state.online.get(id).is_some_and(|o| o.serial == conn.serial) {
            state.online.remove(id);
            drop(state);
            if conn.handle.persist {
                conn.hub.event("player", &format!("{id} signed out"));
            }
        }
    }
    let Conn { handle, replaced, .. } = conn;
    drop(handle);
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
    if let Some(done) = replaced {
        let _ = done.send(());
    }
}

/// Keep a sample, the oldest going once there are [`MAX_SAMPLES`] - unless it
/// is from before the newest one. A Raspberry Pi has no clock of its own: after
/// a boot it reads months back until the network sets it, and a sample from
/// then would be drawn far off the start of the charts and muddle the minutes.
fn push_sample(history: &mut VecDeque<Value>, sample: Value) {
    let at = sample["at"].as_f64().unwrap_or(0.0);
    if history.back().is_some_and(|last| at < last["at"].as_f64().unwrap_or(0.0)) {
        return;
    }
    if history.len() >= MAX_SAMPLES {
        history.pop_front();
    }
    history.push_back(sample);
}

/// The period of `period` seconds that ended before the newest sample, as
/// one sample - if it has ended, has samples, and is later than `done` (the
/// last one rolled): gauges averaged, the slowest tick kept, counters as
/// they stood at its end.
fn rolled(history: &VecDeque<Value>, done: f64, period: f64) -> Option<Value> {
    let last = history.back()?;
    let end = (last["at"].as_f64().unwrap_or(0.0) / period).floor() * period;
    let start = end - period;
    // Already rolled, or still running.
    if start <= done {
        return None;
    }
    let within: Vec<&Value> = history.iter().filter(|s| s["at"].as_f64().is_some_and(|at| at >= start && at < end)).collect();
    if within.is_empty() {
        return None;
    }
    let avg = |key: &str| within.iter().filter_map(|s| s[key].as_f64()).sum::<f64>() / within.len() as f64;
    let last = within[within.len() - 1];
    Some(json!({
        "at": start,
        "connections": avg("connections").round(),
        "players": avg("players").round(),
        "rooms": avg("rooms").round(),
        "messages_in": last["messages_in"],
        "bytes_out": last["bytes_out"],
        "tick_ms": within.iter().filter_map(|s| s["tick_ms"].as_f64()).fold(0.0, f64::max),
        "cpu": avg("cpu"),
        "memory_mb": avg("memory_mb").round(),
        "disk_free_mb": last["disk_free_mb"],
        "net_rx_kb": avg("net_rx_kb"),
        "net_tx_kb": avg("net_tx_kb"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M1: samples every ten seconds boil down to one a minute (and one an
    /// hour) once the period is over, and only once.
    #[test]
    fn samples_roll_into_minutes_once_each_minute_is_over() {
        let sample = |at: f64, players: f64, tick: f64| json!({ "at": at, "players": players, "tick_ms": tick, "messages_in": at });
        let history: VecDeque<Value> = (0..13).map(|i| sample(6000.0 + 10.0 * i as f64, i as f64, i as f64 % 4.0)).collect();
        // 6000..6120: the minute 6000-6060 is over, 6060-6120 is not until 6120.
        let first = rolled(&history, 0.0, 60.0).unwrap();
        assert_eq!(first["at"], 6060.0, "the newest whole minute");
        assert_eq!(first["players"], 9.0, "the six samples of 6060-6119 averaged (8.5), rounded");
        let again = rolled(&history, 6060.0, 60.0);
        assert!(again.is_none(), "a minute is rolled once");
        assert!(rolled(&history, 0.0, 3600.0).is_none(), "no hour has ended with samples in it");
        let mut longer = history.clone();
        longer.push_back(sample(7200.0, 0.0, 0.0));
        let hour = rolled(&longer, 0.0, 3600.0).unwrap();
        assert_eq!(hour["at"], 3600.0);
        assert_eq!(hour["tick_ms"], 3.0, "the slowest tick of the hour is kept");
    }

    /// A sample taken before the clock was set (a Pi just booted) is not kept.
    #[test]
    fn a_sample_from_before_the_newest_is_dropped() {
        let mut history = VecDeque::new();
        for at in [6000.0, 6010.0, 1500.0, 6030.0] {
            push_sample(&mut history, json!({ "at": at }));
        }
        let ats: Vec<f64> = history.iter().map(|s| s["at"].as_f64().unwrap()).collect();
        assert_eq!(ats, [6000.0, 6010.0, 6030.0]);
    }
}
