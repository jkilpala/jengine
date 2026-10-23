//! **Several machines, one world.**
//!
//! One server process is one machine's worth of players. To go past that the
//! nodes have to agree on three things, and a small **coordinator** is where
//! they agree:
//!
//! - **One store.** Every node reads and writes players through the
//!   coordinator ([`RemoteStore`]), so a player saved on one node is the
//!   player loaded on the next.
//! - **Which node has which room.** A named room - a zone - lives on exactly
//!   one node. The first node asked for it *claims* it; any other node asked
//!   for it answers the player with a `redirect` to the node that has it.
//! - **Where each player is.** Signing in tells the coordinator, which tells
//!   whichever node had that player before to save them and let go - so there
//!   is still one connection per player, and one copy of their state.
//!
//! ```text
//!   players ──ws──►  node A  ◄─┐                 ┌─► node B  ◄──ws── players
//!                    (meadow)  └──► coordinator ◄┘   (cave)
//!                                   store, rooms, who is where
//! ```
//!
//! # What a player sees
//!
//! Nothing, when the room is on the node they are connected to. Otherwise
//! one frame - `{"t":"redirect","url":"ws://..","kind","room"}` - and the
//! client connects there, says `hello` with its token, and joins. Walking
//! through a door into a zone on another machine is exactly that: the state
//! is saved before the frame is sent, so it is the same player on the far
//! side. `jengine-online` follows a redirect by itself.
//!
//! # What crosses between nodes
//!
//! Channels and unaimed notices: a node publishes to the coordinator, which
//! hands the frame to every other node. Rooms do not - a room is on one node,
//! and everyone in it is connected to that node.
//!
//! # What it costs
//!
//! Every store call is a request to the coordinator. On a local network that
//! is a fraction of a millisecond, made from a blocking section so it holds
//! up nothing else - but it is one machine everyone depends on: if the
//! coordinator stops, nodes keep the players they have and can save nobody.
//!
//! # The secret never crosses the wire
//!
//! Every call between a node and its coordinator, either way, is **signed**:
//! an HMAC-SHA256 over its method, path, time, a nonce, who it is for and a
//! hash of its body, sent as headers (`x-jengine-sign` and friends). No
//! bearer secret is sent, so a call read off the wire gives nothing to replay
//! or reuse: one older than five minutes, or whose nonce was seen, is
//! refused. **Each node has a key of its own**, derived from the cluster's
//! secret and its name (`jengine-server node-key <name>`): give a node that
//! key alone (`JENGINE_CLUSTER_NODE_KEY`) and it can speak only as itself,
//! and cannot pass for the coordinator to another node. Calls between nodes
//! (an operator's action on a room another node has) go by way of the
//! coordinator, which signs each for the node it is going to. The bodies -
//! players' state among them - are still readable on a plain network; give
//! the coordinator a certificate (`ClusterConfig::ca`), and each node's game
//! port one too (`Server::tls`), or keep the cluster on a private network.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::store::Store;

/// How a node joins a cluster.
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// The coordinator: `http://host:port`.
    pub coordinator: String,
    /// Shared by the coordinator and every node - or, on a node, empty when
    /// it is given only its own key (`node_key`). Never sent: every call is
    /// signed with a key derived from it.
    pub secret: String,
    /// This node's own key, hex (`jengine-server node-key <name>`), in place
    /// of the cluster's secret: a node holding only this can sign as itself
    /// and as nothing else. `None`: derived from `secret`.
    pub node_key: Option<String>,
    /// This node's name, unique in the cluster.
    pub node: String,
    /// What players connect to: `wss://eu-1.example.com`. Empty means the
    /// address the server is bound to, which is right on one machine.
    pub public_url: String,
    /// How the coordinator reaches this node: `http://10.0.0.5:9001`. Empty
    /// means the bound address.
    pub internal_url: String,
    /// Seconds between this node saying it is alive.
    pub heartbeat_secs: f64,
    /// A certificate to trust when the coordinator is reached over `https`
    /// with one of its own making (`jengine-fleet cert`). Not needed for a
    /// certificate from a real authority.
    pub ca: Option<std::path::PathBuf>,
}

impl ClusterConfig {
    pub fn new(coordinator: &str, secret: &str, node: &str) -> Self {
        ClusterConfig {
            coordinator: coordinator.trim_end_matches('/').to_string(),
            secret: secret.to_string(),
            node: node.to_string(),
            public_url: String::new(),
            internal_url: String::new(),
            heartbeat_secs: 2.0,
            ca: None,
            node_key: None,
        }
    }

    /// The key this node signs with and checks the coordinator's calls by.
    pub(crate) fn key(&self) -> Vec<u8> {
        match self.node_key.as_deref().and_then(unhex) {
            Some(key) => key,
            None => node_key(&self.secret, &self.node),
        }
    }

    /// Read a node's place in a cluster from the environment, which is how a
    /// deployed server is told: `JENGINE_CLUSTER` (the coordinator's
    /// address), `JENGINE_CLUSTER_SECRET`, `JENGINE_NODE` (this node's name),
    /// and optionally `JENGINE_PUBLIC_URL` and `JENGINE_INTERNAL_URL`.
    /// `None` when `JENGINE_CLUSTER` is not set: a server on its own.
    pub fn from_env() -> Option<Self> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let mut config = ClusterConfig::new(
            &var("JENGINE_CLUSTER")?,
            &var("JENGINE_CLUSTER_SECRET").unwrap_or_default(),
            &var("JENGINE_NODE").unwrap_or_else(|| format!("node-{}", std::process::id())),
        );
        config.public_url = var("JENGINE_PUBLIC_URL").unwrap_or_default();
        config.internal_url = var("JENGINE_INTERNAL_URL").unwrap_or_default();
        config.ca = var("JENGINE_CLUSTER_CA").map(std::path::PathBuf::from);
        config.node_key = var("JENGINE_CLUSTER_NODE_KEY");
        Some(config)
    }

    pub fn public_url(mut self, url: &str) -> Self {
        self.public_url = url.to_string();
        self
    }

    pub fn internal_url(mut self, url: &str) -> Self {
        self.internal_url = url.to_string();
        self
    }
}

/// Run something that waits on the network without holding up the async
/// tasks beside it. Outside a runtime it just runs.
fn blocking<T>(work: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(work)
        }
        _ => work(),
    }
}

/// The client a node reaches its coordinator with: the usual authorities,
/// plus the cluster's own certificate if it has one.
fn http(ca: Option<&std::path::Path>) -> ureq::Agent {
    crate::listen::client(ca).unwrap_or_else(|why| {
        log::error!("[cluster] {why}; trusting only the usual authorities");
        ureq::agent()
    })
}

// ---- signed calls ----------------------------------------------------------------

/// Which node a call is from (to the coordinator) or for (from it).
pub(crate) const H_NODE: &str = "x-jengine-node";
const H_TIME: &str = "x-jengine-time";
const H_NONCE: &str = "x-jengine-nonce";
const H_SIGN: &str = "x-jengine-sign";
/// How far a call's time may be from ours, either way.
const SKEW_SECS: u64 = 300;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() || !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

fn hmac(key: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    use hmac::Mac;
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key).expect("HMAC takes a key of any length");
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            mac.update(b"\n");
        }
        mac.update(part);
    }
    mac.finalize().into_bytes().to_vec()
}

/// A node's own key: derived from the cluster's secret and its name, so the
/// coordinator can work out any node's and a node told only its own cannot
/// work out another's.
pub fn node_key(secret: &str, node: &str) -> Vec<u8> {
    hmac(secret.as_bytes(), &[b"jengine-node-key-v1", node.as_bytes()])
}

/// [`node_key`] as the hex `JENGINE_CLUSTER_NODE_KEY` takes.
pub fn node_key_hex(secret: &str, node: &str) -> String {
    hex(&node_key(secret, node))
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn message(method: &str, path: &str, time: &str, nonce: &str, node: &str, body: &[u8]) -> Vec<u8> {
    use sha2::Digest;
    let hash = hex(&sha2::Sha256::digest(body));
    [method, path, time, nonce, node, &hash].join("\n").into_bytes()
}

/// The headers that sign one call: `key` is the node's, `node` its name.
pub(crate) fn sign(key: &[u8], node: &str, method: &str, path: &str, body: &[u8]) -> Vec<(&'static str, String)> {
    let time = unix_secs().to_string();
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).expect("the OS has randomness");
    let nonce = hex(&nonce);
    let signature = hex(&hmac(key, &[&message(method, path, &time, &nonce, node, body)]));
    vec![(H_NODE, node.to_string()), (H_TIME, time), (H_NONCE, nonce), (H_SIGN, signature)]
}

/// Nonces seen lately, so a call read off the wire cannot be sent again.
#[derive(Default)]
pub(crate) struct Replays(Mutex<HashMap<String, u64>>);

impl Replays {
    /// Whether this nonce is new; remembered either way.
    fn fresh(&self, nonce: &str, now: u64) -> bool {
        let mut seen = self.0.lock().unwrap();
        if seen.len() > 50_000 {
            seen.retain(|_, at| *at + 2 * SKEW_SECS > now);
        }
        seen.insert(nonce.to_string(), now).is_none()
    }
}

/// Whether a call was signed by `key` for `node`, recently, and not before.
/// `Err` says why not, for the log - never to the caller.
pub(crate) fn verify(
    key: &[u8],
    node: &str,
    method: &str,
    path: &str,
    headers: &HeaderMap,
    body: &[u8],
    replays: &Replays,
) -> Result<(), String> {
    let h = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("");
    let (time, nonce, given) = (h(H_TIME), h(H_NONCE), h(H_SIGN));
    if h(H_NODE) != node {
        return Err(format!("signed for '{}', not '{node}'", h(H_NODE)));
    }
    let now = unix_secs();
    let at: u64 = time.parse().map_err(|_| "no time on the call".to_string())?;
    if at.abs_diff(now) > SKEW_SECS {
        return Err(format!("the call's time is {}s from ours - are the clocks set?", at.abs_diff(now)));
    }
    let want = hmac(key, &[&message(method, path, time, nonce, node, body)]);
    let given = unhex(given).unwrap_or_default();
    let same = want.len() == given.len() && want.iter().zip(&given).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
    if !same {
        return Err("the signature is not the cluster's".into());
    }
    if nonce.len() < 16 || !replays.fresh(nonce, now) {
        return Err("a call seen before".into());
    }
    Ok(())
}

/// POST a JSON body to `url`, signed with `key` as `node`.
pub(crate) fn post_signed(
    http: &ureq::Agent,
    url: &str,
    key: &[u8],
    node: &str,
    body: &Value,
    timeout: Duration,
) -> Result<Value, String> {
    let text = body.to_string();
    let path = url_path(url);
    let mut request = http.post(url).timeout(timeout).set("Content-Type", "application/json");
    for (name, value) in sign(key, node, "POST", &path, text.as_bytes()) {
        request = request.set(name, &value);
    }
    match request.send_string(&text) {
        Ok(r) => r.into_json().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(code, r)) => Err(format!("{code}: {}", r.into_string().unwrap_or_default())),
        Err(e) => Err(e.to_string()),
    }
}

/// One call to a coordinator as node `node` would make it - signed with the
/// key `secret` derives for that node. For a tool, a test, or a node that is
/// not a `jengine-server`.
pub fn call_as(coordinator: &str, secret: &str, node: &str, what: &str, mut body: Value) -> Result<Value, String> {
    body["node"] = json!(node);
    let url = format!("{}/c/{what}", coordinator.trim_end_matches('/'));
    post_signed(&ureq::agent(), &url, &node_key(secret, node), node, &body, Duration::from_secs(8))
}

/// `https://host:9100/c/store?x` -> `/c/store`: what is signed is the path.
fn url_path(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = rest.find('/').map_or("/", |i| &rest[i..]);
    path.split('?').next().unwrap_or("/").to_string()
}

/// A node's line to the coordinator.
pub struct Cluster {
    pub config: ClusterConfig,
    http: ureq::Agent,
    /// This node's key: what its calls are signed with, and the
    /// coordinator's calls to it are checked by.
    key: Vec<u8>,
    /// Nonces of the coordinator's calls seen lately.
    replays: Replays,
}

impl Cluster {
    pub fn new(config: ClusterConfig) -> Self {
        let http = http(config.ca.as_deref());
        let key = config.key();
        Cluster { config, http, key, replays: Replays::default() }
    }

    fn call(&self, path: &str, mut body: Value) -> Result<Value, String> {
        body["node"] = json!(self.config.node);
        let url = format!("{}{path}", self.config.coordinator);
        blocking(|| post_signed(&self.http, &url, &self.key, &self.config.node, &body, Duration::from_secs(8)))
            .map_err(|e| format!("the coordinator did not answer {path}: {e}"))
    }

    /// Whether a call to this node is the coordinator's: signed with this
    /// node's key, for this node, recently, once.
    pub(crate) fn from_coordinator(&self, method: &str, path: &str, headers: &HeaderMap, body: &[u8]) -> bool {
        match verify(&self.key, &self.config.node, method, path, headers, body, &self.replays) {
            Ok(()) => true,
            Err(why) => {
                log::warn!("[cluster] refused a call to {path}: {why}");
                false
            }
        }
    }

    /// Say this node is alive, and what it holds. `Ok(false)` when the
    /// coordinator did not know us - it has been restarted, and has to be
    /// told again what we have ([`Cluster::resync`]).
    pub fn register(&self, kinds: &[String], stats: Value) -> Result<bool, String> {
        let answer = self.call(
            "/c/register",
            json!({
                "public_url": self.config.public_url, "internal_url": self.config.internal_url,
                "kinds": kinds, "stats": stats,
            }),
        )?;
        Ok(answer["known"].as_bool().unwrap_or(true))
    }

    /// Tell a coordinator that has forgotten everything what this node has:
    /// its rooms, and the players signed in or held on it. The directory is
    /// only ever a summary of the nodes, so the nodes can rebuild it.
    pub fn resync(&self, rooms: &[(String, String)], players: &[String]) {
        let _ = self.call("/c/resync", json!({ "rooms": rooms, "players": players }));
    }

    /// Who has this room? Claims it for this node if nobody does. Returns
    /// `None` when it is ours, or the address of the node it is on.
    pub fn claim(&self, kind: &str, room: &str) -> Result<Option<String>, String> {
        let answer = self.call("/c/claim", json!({ "kind": kind, "room": room }))?;
        Ok(if answer["node"] == self.config.node { None } else { answer["url"].as_str().map(str::to_string) })
    }

    /// A node that runs a kind of room this one does not.
    pub fn find(&self, kind: &str) -> Result<Option<String>, String> {
        Ok(self.call("/c/find", json!({ "kind": kind }))?["url"].as_str().map(str::to_string))
    }

    pub fn release(&self, kind: &str, room: &str) {
        let _ = self.call("/c/release", json!({ "kind": kind, "room": room }));
    }

    /// This player is signing in here. Whichever node had them is told to
    /// let go, and has, by the time this returns. `Some(url)` if another
    /// node is holding their seat: they belong back there.
    pub fn online(&self, player: &str) -> Result<Option<String>, String> {
        Ok(self.call("/c/online", json!({ "player": player }))?["held_at"].as_str().map(str::to_string))
    }

    pub fn offline(&self, player: &str) {
        let _ = self.call("/c/offline", json!({ "player": player }));
    }

    /// Hand a frame to every other node, for a channel or for everyone.
    pub fn publish(&self, topic: &str, frame: &Value) {
        let _ = self.call("/c/publish", json!({ "topic": topic, "frame": frame }));
    }

    /// The other node a player is signed in on, as the address to reach it
    /// at. `None` if they are here, or nowhere.
    pub fn locate(&self, player: &str) -> Option<String> {
        let answer = self.call("/c/where", json!({ "player": player })).ok()?;
        answer["internal_url"].as_str().map(str::to_string)
    }

    /// The other node that has a room, as the address to reach it at. Unlike
    /// [`Cluster::claim`] this only asks: a room nobody has stays nobody's.
    pub fn owner(&self, kind: &str, room: &str) -> Option<String> {
        let answer = self.call("/c/owner", json!({ "kind": kind, "room": room })).ok()?;
        answer["internal_url"].as_str().map(str::to_string)
    }

    /// Have another node carry out an operator's action on something it has.
    /// By way of the coordinator, which signs it for that node: a node holds
    /// no other node's key.
    pub fn forward(&self, internal_url: &str, action: &str, body: &Value) -> Result<Value, String> {
        let asked = json!({ "url": internal_url, "action": action, "body": body });
        let answer = self.call("/c/forward", asked)?;
        if let Some(why) = answer["error"].as_str() {
            return Err(why.to_string());
        }
        Ok(answer["answer"].clone())
    }

    pub fn nodes(&self) -> Value {
        self.call("/c/nodes", json!({})).map(|v| v["nodes"].clone()).unwrap_or(Value::Null)
    }
}

/// The coordinator's store, from a node.
///
/// **A write that cannot be sent is kept, not lost.** It waits here - the
/// latest value for each key - and goes out with the next thing that gets
/// through (`flush`, which the node's heartbeat calls). Until then this node
/// reads its own waiting writes back, so it stays consistent with itself. A
/// read that cannot be answered is an error (`try_load`), never "nothing
/// there".
pub struct RemoteStore {
    coordinator: String,
    node: String,
    key: Vec<u8>,
    http: ureq::Agent,
    /// (namespace, key) -> the value to write, or `None` to delete.
    waiting: Mutex<std::collections::BTreeMap<(String, String), Option<String>>>,
}

impl RemoteStore {
    pub fn new(config: &ClusterConfig) -> Self {
        RemoteStore {
            coordinator: config.coordinator.clone(),
            node: config.node.clone(),
            key: config.key(),
            http: http(config.ca.as_deref()),
            waiting: Mutex::new(Default::default()),
        }
    }

    fn op(&self, mut body: Value) -> Result<Value, String> {
        body["node"] = json!(self.node);
        let url = format!("{}/c/store", self.coordinator);
        // A short patience: a store that is down should fail a sign-in in
        // moments, not hold a room's task for seconds.
        blocking(|| post_signed(&self.http, &url, &self.key, &self.node, &body, Duration::from_secs(3)))
    }

    /// Write `items`, and anything still waiting, in one request.
    fn write(&self, items: Vec<((String, String), Option<String>)>) {
        let mut waiting = self.waiting.lock().unwrap();
        for (key, value) in items {
            waiting.insert(key, value);
        }
        if waiting.is_empty() {
            return;
        }
        let batch: Vec<Value> =
            waiting.iter().map(|((ns, key), value)| json!({ "ns": ns, "key": key, "value": value })).collect();
        match self.op(json!({ "op": "batch", "items": batch })) {
            Ok(_) => waiting.clear(),
            Err(e) => log::error!("[cluster] {} write(s) are waiting for the store: {e}", waiting.len()),
        }
    }

}

impl Store for RemoteStore {
    fn load(&self, ns: &str, key: &str) -> Option<String> {
        self.try_load(ns, key).unwrap_or_else(|e| {
            log::error!("[cluster] the store did not answer: {e}");
            None
        })
    }
    fn try_load(&self, ns: &str, key: &str) -> Result<Option<String>, String> {
        if let Some(ours) = self.waiting.lock().unwrap().get(&(ns.to_string(), key.to_string())) {
            return Ok(ours.clone());
        }
        let answer = self.op(json!({ "op": "load", "ns": ns, "key": key }))?;
        Ok(answer["value"].as_str().map(str::to_string))
    }
    fn save(&self, ns: &str, key: &str, value: &str) {
        self.write(vec![((ns.to_string(), key.to_string()), Some(value.to_string()))]);
    }
    fn save_many(&self, items: &[(&str, &str, &str)]) {
        self.write(
            items.iter().map(|(ns, key, value)| ((ns.to_string(), key.to_string()), Some(value.to_string()))).collect(),
        );
    }
    fn delete(&self, ns: &str, key: &str) {
        self.write(vec![((ns.to_string(), key.to_string()), None)]);
    }
    fn keys(&self, ns: &str) -> Vec<String> {
        self.op(json!({ "op": "keys", "ns": ns }))
            .ok()
            .and_then(|v| v["keys"].as_array().cloned())
            .map(|keys| keys.iter().filter_map(|k| k.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }
    /// One request: the coordinator's store answers from its own (an
    /// indexed query, when it is SQLite).
    fn top(&self, ns: &str, n: usize, order: crate::store::Order) -> Vec<crate::store::Scored> {
        let lowest = order == crate::store::Order::Lowest;
        self.op(json!({ "op": "top", "ns": ns, "n": n, "lowest": lowest }))
            .ok()
            .and_then(|v| v["top"].as_array().cloned())
            .into_iter()
            .flatten()
            .filter_map(|row| {
                Some(crate::store::Scored {
                    key: row["key"].as_str()?.to_string(),
                    score: row["score"].as_f64()?,
                    value: row["value"].as_str()?.to_string(),
                })
            })
            .collect()
    }
    fn rank(&self, ns: &str, key: &str, order: crate::store::Order) -> Option<(usize, usize)> {
        let lowest = order == crate::store::Order::Lowest;
        let answer = self.op(json!({ "op": "rank", "ns": ns, "key": key, "lowest": lowest })).ok()?;
        Some((answer["rank"].as_u64()? as usize, answer["of"].as_u64()? as usize))
    }
    fn flush(&self) {
        self.write(Vec::new());
    }
    fn waiting(&self) -> usize {
        self.waiting.lock().unwrap().len()
    }
}

/// How many more players a node may have than the quietest before a new room
/// asked of it is opened on the quietest instead.
const SPREAD: u64 = 50;

// ---- the coordinator ------------------------------------------------------------

/// Whether a node's machine is too busy to be given more: over
/// [`LOADED_CPU`] percent of its CPU or [`LOADED_MEMORY`] of its memory,
/// as it last reported. A node that reports neither is never loaded.
fn loaded(n: &Node) -> bool {
    n.stats["machine_cpu"].as_f64().unwrap_or(0.0) >= LOADED_CPU
        || n.stats["memory_percent"].as_f64().unwrap_or(0.0) >= LOADED_MEMORY
}

const LOADED_CPU: f64 = 85.0;
const LOADED_MEMORY: f64 = 90.0;

struct Node {
    public_url: String,
    internal_url: String,
    kinds: Vec<String>,
    stats: Value,
    seen: Instant,
}

#[derive(Default)]
struct Directory {
    nodes: HashMap<String, Node>,
    /// (kind, room) -> the node that has it.
    rooms: HashMap<(String, String), String>,
    /// player -> the node they are signed in on (or held on).
    online: HashMap<String, String>,
}

struct Coord {
    store: Arc<dyn Store>,
    secret: String,
    /// How the coordinator reaches nodes: trusting the usual authorities and,
    /// if given, the nodes' own certificate - so a node's game port can be
    /// `https` with a certificate of its own making.
    http: ureq::Agent,
    /// Nonces of the nodes' calls seen lately.
    replays: Replays,
    /// A node unheard from for this long is gone, and its rooms are free.
    node_timeout: Duration,
    dir: Mutex<Directory>,
}

impl Coord {
    /// Forget nodes that have stopped saying they are alive, with what they held.
    fn sweep(&self, dir: &mut Directory) {
        let dead: Vec<String> =
            dir.nodes.iter().filter(|(_, n)| n.seen.elapsed() > self.node_timeout).map(|(k, _)| k.clone()).collect();
        for node in dead {
            log::warn!("[coordinator] node {node} has gone quiet; its rooms are free");
            dir.nodes.remove(&node);
            dir.rooms.retain(|_, owner| *owner != node);
            dir.online.retain(|_, at| *at != node);
        }
    }
}

impl Coord {
    /// A call to a node, signed with that node's key.
    fn to_node(&self, node: &str, url: &str, body: &Value, timeout: Duration) -> Result<Value, String> {
        post_signed(&self.http, url, &node_key(&self.secret, node), node, body, timeout)
    }
}

/// The node a call is signed by, if it is signed by that node's key.
fn signed_by(coord: &Coord, path: &str, headers: &HeaderMap, body: &[u8]) -> Option<String> {
    let node = headers.get(H_NODE).and_then(|v| v.to_str().ok())?.to_string();
    match verify(&node_key(&coord.secret, &node), &node, "POST", path, headers, body, &coord.replays) {
        Ok(()) => Some(node),
        Err(why) => {
            log::warn!("[coordinator] refused a call to {path} as '{node}': {why}");
            None
        }
    }
}

fn text<'a>(body: &'a Value, key: &str) -> &'a str {
    body[key].as_str().unwrap_or("")
}

async fn handle(
    State(coord): State<Arc<Coord>>,
    headers: HeaderMap,
    axum::extract::Path(what): axum::extract::Path<String>,
    raw: axum::body::Bytes,
) -> Response {
    // Who signed it is who it is from: the body's own "node" is not believed.
    let Some(node) = signed_by(&coord, &format!("/c/{what}"), &headers, &raw) else {
        return (StatusCode::UNAUTHORIZED, "not signed with a key of this cluster").into_response();
    };
    let Ok(body) = serde_json::from_slice::<Value>(&raw) else {
        return (StatusCode::BAD_REQUEST, "not JSON").into_response();
    };
    let answer = match what.as_str() {
        "store" => {
            let (ns, key) = (text(&body, "ns"), text(&body, "key"));
            match text(&body, "op") {
                "load" => json!({ "value": coord.store.load(ns, key) }),
                "save" => {
                    coord.store.save(ns, key, text(&body, "value"));
                    json!({})
                }
                "delete" => {
                    coord.store.delete(ns, key);
                    json!({})
                }
                "keys" => json!({ "keys": coord.store.keys(ns) }),
                "top" | "rank" => {
                    let order = if body["lowest"] == true { crate::store::Order::Lowest } else { crate::store::Order::Highest };
                    if text(&body, "op") == "top" {
                        let n = body["n"].as_u64().unwrap_or(10) as usize;
                        let top: Vec<Value> = coord
                            .store
                            .top(ns, n, order)
                            .into_iter()
                            .map(|s| json!({ "key": s.key, "score": s.score, "value": s.value }))
                            .collect();
                        json!({ "top": top })
                    } else {
                        match coord.store.rank(ns, key, order) {
                            Some((rank, of)) => json!({ "rank": rank, "of": of }),
                            None => json!({}),
                        }
                    }
                }
                "batch" => {
                    for item in body["items"].as_array().into_iter().flatten() {
                        let (ns, key) = (text(item, "ns"), text(item, "key"));
                        match item["value"].as_str() {
                            Some(value) => coord.store.save(ns, key, value),
                            None => coord.store.delete(ns, key),
                        }
                    }
                    json!({})
                }
                _ => return (StatusCode::BAD_REQUEST, "no such store operation").into_response(),
            }
        }
        "register" => {
            let mut dir = coord.dir.lock().unwrap();
            coord.sweep(&mut dir);
            let kinds = body["kinds"].as_array().into_iter().flatten().filter_map(|k| k.as_str().map(str::to_string)).collect();
            // A node we have never heard of is told so: it may be a new node,
            // or we may be a new coordinator, and then it has things to tell.
            let known = dir.nodes.contains_key(&node);
            dir.nodes.insert(
                node,
                Node {
                    public_url: text(&body, "public_url").to_string(),
                    internal_url: text(&body, "internal_url").to_string(),
                    kinds,
                    stats: body["stats"].clone(),
                    seen: Instant::now(),
                },
            );
            json!({ "known": known })
        }
        "resync" => {
            let mut dir = coord.dir.lock().unwrap();
            for room in body["rooms"].as_array().into_iter().flatten() {
                if let (Some(kind), Some(id)) = (room[0].as_str(), room[1].as_str()) {
                    // First to say so keeps it, as with a claim.
                    dir.rooms.entry((kind.to_string(), id.to_string())).or_insert_with(|| node.clone());
                }
            }
            for player in body["players"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                dir.online.insert(player.to_string(), node.clone());
            }
            log::info!("[coordinator] node {node} told us what it holds");
            json!({})
        }
        "claim" => {
            let mut dir = coord.dir.lock().unwrap();
            coord.sweep(&mut dir);
            let key = (text(&body, "kind").to_string(), text(&body, "room").to_string());
            // A room nobody has goes to whoever asks - unless they are much
            // busier than another node that could run it, or their machine
            // is loaded and another's is not, in which case it goes there,
            // and the asker's player is sent after it.
            let players = |n: &Node| n.stats["players"].as_u64().unwrap_or(0);
            let asking = dir.nodes.get(&node);
            let asker = asking.map(players).unwrap_or(0);
            let asker_loaded = asking.is_some_and(loaded);
            let quieter = dir
                .nodes
                .iter()
                .filter(|(_, n)| n.kinds.contains(&key.0))
                // Unloaded machines first, then the fewest players.
                .min_by_key(|(name, n)| (loaded(n), players(n), (*name).clone()))
                .filter(|(name, n)| **name != node && (players(n) + SPREAD < asker || (asker_loaded && !loaded(n))))
                .map(|(name, _)| name.clone());
            let owner = dir.rooms.entry(key).or_insert_with(|| quieter.unwrap_or_else(|| node.clone())).clone();
            let url = dir.nodes.get(&owner).map(|n| n.public_url.clone());
            json!({ "node": owner, "url": url })
        }
        "find" => {
            let mut dir = coord.dir.lock().unwrap();
            coord.sweep(&mut dir);
            let kind = text(&body, "kind");
            // The least busy node that runs that kind - an unloaded machine first.
            let found = dir
                .nodes
                .iter()
                .filter(|(name, n)| **name != node && n.kinds.iter().any(|k| k == kind))
                .min_by_key(|(_, n)| (loaded(n), n.stats["players"].as_u64().unwrap_or(0)))
                .map(|(_, n)| n.public_url.clone());
            json!({ "url": found })
        }
        "release" => {
            let mut dir = coord.dir.lock().unwrap();
            let key = (text(&body, "kind").to_string(), text(&body, "room").to_string());
            if dir.rooms.get(&key) == Some(&node) {
                dir.rooms.remove(&key);
            }
            json!({})
        }
        "online" => {
            let player = text(&body, "player").to_string();
            // Whoever had them, and how to reach it.
            let before = {
                let mut dir = coord.dir.lock().unwrap();
                coord.sweep(&mut dir);
                let before = dir.online.get(&player).filter(|at| **at != node).cloned();
                before.and_then(|at| dir.nodes.get(&at).map(|n| (at.clone(), n.internal_url.clone(), n.public_url.clone())))
            };
            let mut held_at = None;
            if let Some((at, internal, public)) = before {
                // That node saves the player and lets go before it answers -
                // unless it is holding their seat, in which case they belong
                // back there and nothing moves.
                let (coord, who) = (coord.clone(), player.clone());
                let told = tokio::task::spawn_blocking(move || {
                    coord.to_node(&at, &format!("{internal}/cluster/replace"), &json!({ "player": who }), Duration::from_secs(8))
                })
                .await;
                if matches!(&told, Ok(Ok(answer)) if answer["held"] == true) {
                    held_at = Some(public);
                }
            }
            if held_at.is_none() {
                coord.dir.lock().unwrap().online.insert(player, node);
            }
            json!({ "held_at": held_at })
        }
        "owner" => {
            let dir = coord.dir.lock().unwrap();
            let key = (text(&body, "kind").to_string(), text(&body, "room").to_string());
            let at = dir.rooms.get(&key).filter(|at| **at != node);
            let found = at.and_then(|at| dir.nodes.get(at).map(|n| n.internal_url.clone()));
            json!({ "internal_url": found })
        }
        "where" => {
            let dir = coord.dir.lock().unwrap();
            let at = dir.online.get(text(&body, "player")).filter(|at| **at != node);
            let found = at.and_then(|at| dir.nodes.get(at).map(|n| (at.clone(), n.internal_url.clone())));
            json!({ "node": found.as_ref().map(|f| &f.0), "internal_url": found.as_ref().map(|f| &f.1) })
        }
        "offline" => {
            let mut dir = coord.dir.lock().unwrap();
            let player = text(&body, "player");
            if dir.online.get(player) == Some(&node) {
                dir.online.remove(player);
            }
            json!({})
        }
        "publish" => {
            let others: Vec<(String, String)> = {
                let mut dir = coord.dir.lock().unwrap();
                coord.sweep(&mut dir);
                dir.nodes
                    .iter()
                    .filter(|(name, _)| **name != node)
                    .map(|(name, n)| (name.clone(), n.internal_url.clone()))
                    .collect()
            };
            let message = json!({ "topic": body["topic"], "frame": body["frame"] });
            for (name, internal) in others {
                let (coord, message) = (coord.clone(), message.clone());
                tokio::task::spawn_blocking(move || {
                    let _ = coord.to_node(&name, &format!("{internal}/cluster/deliver"), &message, Duration::from_secs(4));
                });
            }
            json!({})
        }
        // A node asks another to do something for an operator: carried and
        // signed for that node here, since no node holds another's key.
        "forward" => {
            let url = text(&body, "url").to_string();
            let action = text(&body, "action").to_string();
            let target = {
                let dir = coord.dir.lock().unwrap();
                dir.nodes.iter().find(|(_, n)| n.internal_url == url).map(|(name, _)| name.clone())
            };
            let Some(target) = target else {
                return Json(json!({ "error": "no node is reached at that address" })).into_response();
            };
            if !action.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return (StatusCode::BAD_REQUEST, "not an action").into_response();
            }
            let inner = body["body"].clone();
            let coord = coord.clone();
            let done = tokio::task::spawn_blocking(move || {
                coord.to_node(&target, &format!("{url}/cluster/admin/{action}"), &inner, Duration::from_secs(4))
            })
            .await;
            match done {
                Ok(Ok(answer)) => json!({ "answer": answer }),
                Ok(Err(why)) => json!({ "error": why }),
                Err(_) => json!({ "error": "the call failed" }),
            }
        }
        "nodes" => {
            let mut dir = coord.dir.lock().unwrap();
            coord.sweep(&mut dir);
            let mut nodes: Vec<Value> = dir
                .nodes
                .iter()
                .map(|(name, n)| {
                    let rooms: Vec<String> = dir
                        .rooms
                        .iter()
                        .filter(|(_, owner)| *owner == name)
                        .map(|((kind, id), _)| format!("{kind}/{id}"))
                        .collect();
                    json!({
                        "node": name, "url": n.public_url, "internal": n.internal_url,
                        "kinds": n.kinds, "stats": n.stats,
                        "rooms": rooms, "seen": n.seen.elapsed().as_secs_f64(),
                    })
                })
                .collect();
            nodes.sort_by(|a, b| a["node"].as_str().cmp(&b["node"].as_str()));
            json!({ "nodes": nodes, "players": dir.online.len() })
        }
        _ => return (StatusCode::NOT_FOUND, "no such call").into_response(),
    };
    Json(answer).into_response()
}

/// A coordinator started with [`coordinator`].
pub struct RunningCoordinator {
    pub addr: SocketAddr,
    secure: bool,
    shutdown: watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RunningCoordinator {
    pub fn url(&self) -> String {
        format!("{}://{}", if self.secure { "https" } else { "http" }, self.addr)
    }

    pub fn stop(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for RunningCoordinator {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

/// Run a coordinator on a thread of its own: the cluster's store, its
/// directory of rooms, and its record of who is signed in where.
pub fn coordinator(
    addr: &str,
    store: Arc<dyn Store>,
    secret: &str,
    node_timeout_secs: f64,
) -> std::io::Result<RunningCoordinator> {
    coordinator_with(addr, store, secret, node_timeout_secs, None)
}

/// [`coordinator`], over HTTPS when given a certificate - which is how the
/// cluster's secret, and every player's state, stay off the wire in the clear.
pub fn coordinator_with(
    addr: &str,
    store: Arc<dyn Store>,
    secret: &str,
    node_timeout_secs: f64,
    tls: Option<crate::listen::Tls>,
) -> std::io::Result<RunningCoordinator> {
    coordinator_trusting(addr, store, secret, node_timeout_secs, tls, None)
}

/// [`coordinator_with`], trusting the certificate in `node_ca` (PEM) when it
/// calls a node - for nodes whose game port is `https` with a certificate of
/// their own making (`jengine-fleet cert`). With it and the coordinator's
/// own certificate, nothing between the machines of a cluster is readable.
pub fn coordinator_trusting(
    addr: &str,
    store: Arc<dyn Store>,
    secret: &str,
    node_timeout_secs: f64,
    tls: Option<crate::listen::Tls>,
    node_ca: Option<&std::path::Path>,
) -> std::io::Result<RunningCoordinator> {
    if secret.len() < 8 {
        return Err(std::io::Error::other("a cluster needs a secret of at least 8 characters"));
    }
    let listener = std::net::TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let http = crate::listen::client(node_ca).map_err(std::io::Error::other)?;
    let coord = Arc::new(Coord {
        store,
        secret: secret.to_string(),
        http,
        replays: Replays::default(),
        node_timeout: Duration::from_secs_f64(node_timeout_secs.max(0.2)),
        dir: Mutex::new(Directory::default()),
    });
    let secure = tls.is_some();
    let (shutdown, mut shutdown_rx) = watch::channel(false);
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let thread = std::thread::spawn(move || {
        runtime.block_on(async move {
            let router = Router::new().route("/c/{what}", post(handle)).with_state(coord);
            let stop = async move {
                let _ = shutdown_rx.changed().await;
            };
            if let Err(e) = crate::listen::serve(listener, router, tls, stop).await {
                log::error!("[coordinator] {e}");
            }
        });
    });
    Ok(RunningCoordinator { addr, secure, shutdown, thread: Some(thread) })
}

/// Run a coordinator until Ctrl+C.
pub fn run_coordinator(
    addr: &str,
    store: Arc<dyn Store>,
    secret: &str,
    tls: Option<crate::listen::Tls>,
) -> std::io::Result<()> {
    run_coordinator_trusting(addr, store, secret, tls, None)
}

/// [`run_coordinator`], trusting `node_ca` when it calls a node.
pub fn run_coordinator_trusting(
    addr: &str,
    store: Arc<dyn Store>,
    secret: &str,
    tls: Option<crate::listen::Tls>,
    node_ca: Option<&std::path::Path>,
) -> std::io::Result<()> {
    let running = coordinator_trusting(addr, store, secret, 10.0, tls, node_ca)?;
    log::info!("[coordinator] listening on {}", running.url());
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(async {
        let _ = tokio::signal::ctrl_c().await;
    });
    running.stop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(signed: &[(&'static str, String)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in signed {
            h.insert(*k, v.parse().unwrap());
        }
        h
    }

    /// K6: a call is signed, never carries the secret, and is believed once,
    /// for the node it names, with the body it was signed with.
    #[test]
    fn a_call_is_signed_and_believed_once_for_its_node() {
        let secret = "the-cluster-secret";
        let key = node_key(secret, "eu-1");
        let body = br#"{"op":"load","ns":"player","key":"p1"}"#;
        let signed = sign(&key, "eu-1", "POST", "/c/store", body);
        assert!(signed.iter().all(|(_, v)| !v.contains(secret)), "the secret itself is never sent");
        let replays = Replays::default();
        let h = headers(&signed);
        assert!(verify(&key, "eu-1", "POST", "/c/store", &h, body, &replays).is_ok());
        assert!(verify(&key, "eu-1", "POST", "/c/store", &h, body, &replays).unwrap_err().contains("seen before"), "a replay");

        let fresh = Replays::default();
        let tampered = br#"{"op":"load","ns":"player","key":"p2"}"#;
        assert!(verify(&key, "eu-1", "POST", "/c/store", &h, tampered, &fresh).is_err(), "another body");
        assert!(verify(&key, "eu-1", "POST", "/c/nodes", &h, body, &fresh).is_err(), "another path");
        // A node holding only its own key cannot pass for another.
        let theirs = node_key(secret, "us-1");
        let posing = headers(&sign(&key, "us-1", "POST", "/c/store", body));
        assert!(verify(&theirs, "us-1", "POST", "/c/store", &posing, body, &fresh).is_err());
        assert_ne!(node_key(secret, "eu-1"), node_key(secret, "us-1"));
        assert_ne!(node_key(secret, "eu-1"), node_key("another-secret", "eu-1"));
        // Too old.
        let mut old = signed.clone();
        old[1].1 = (unix_secs() - 3600).to_string();
        let stale = verify(&key, "eu-1", "POST", "/c/store", &headers(&old), body, &fresh).unwrap_err();
        assert!(stale.contains("clocks"), "{stale}");
    }

    #[test]
    fn a_node_given_its_own_key_signs_as_itself() {
        let mut config = ClusterConfig::new("http://coord", "the-cluster-secret", "eu-1");
        let derived = config.key();
        config.secret = String::new();
        config.node_key = Some(node_key_hex("the-cluster-secret", "eu-1"));
        assert_eq!(config.key(), derived, "the key it was given is the one the coordinator derives");
        assert_eq!(url_path("https://10.0.0.1:9100/c/store?x=1"), "/c/store");
        assert_eq!(url_path("http://h/cluster/admin/kick"), "/cluster/admin/kick");
    }
}
