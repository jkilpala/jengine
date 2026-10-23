//! A game's own HTTP routes, beside its socket.
//!
//! Not everything about a game is a frame in a room. A profile page wants a
//! player's stats; a launcher wants to know how many are on; a web site
//! wants a leaderboard. Those are requests with answers, and they belong on
//! HTTP, on the port the game already has:
//!
//! ```no_run
//! # use jengine_server::{ApiError, Server};
//! # use serde_json::json;
//! Server::new()
//!     .get("/api/players/{id}", |req, api| {
//!         let player = api.player(req.param("id")).ok_or_else(ApiError::not_found)?;
//!         Ok(json!({ "name": player["name"], "coins": player["state"]["coins"] }))
//!     })
//!     .get("/api/me", |_req, api| {
//!         let me = api.caller()?;                 // who the bearer token is
//!         api.player(&me).ok_or_else(ApiError::not_found)
//!     });
//! ```
//!
//! A handler is plain synchronous Rust, like a room: it is given the request
//! and an [`Api`] to ask things of, and returns JSON or an [`ApiError`]. It
//! runs off the tasks that serve sockets, so it may take its time.
//!
//! # What a handler can ask
//!
//! - [`Api::player`]: a player's name, state and whereabouts. **Their state
//!   as it is now** if they are in a room - the room is asked - and as last
//!   saved if they are not.
//! - [`Api::caller`]: who is asking, from the `Authorization: Bearer` token -
//!   the same token the game signed in with. Nothing is taken on trust: a
//!   route that returns "my" anything calls this.
//! - [`Api::rooms`], [`Api::room`], [`Api::online`]: what the server is doing
//!   - the whole cluster, when it is one node of one.
//! - [`Api::command`]: tell a room something. It arrives at the room's own
//!   `Room::on_admin`, on the room's task, so it can change a player who is
//!   playing - a shop page granting a coin - which writing the store cannot:
//!   the room holds the true copy and would save over it.
//! - [`Api::edit_player`]: change a player's state wherever the true copy
//!   is - in their room's task, on their connection, or in the store - as one
//!   step, so a coin granted is not lost to a coin picked up the same moment.
//! - [`Api::store`]: the store itself, for what the game keeps there - a
//!   leaderboard a room writes, say.
//!
//! # What is decided for you
//!
//! Paths start with `/api/` (so they cannot shadow the socket, the dashboard
//! or the health check). Answers are JSON. Every origin may call them
//! (`Access-Control-Allow-Origin: *`) unless `Config::api_allow_origins`
//! names the ones that may: a token, not a cookie, is what identifies a
//! caller, so there is nothing for another site to ride on. Each route's
//! requests, failures and times are on the dashboard and in `/metrics`.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, Query, RawPathParams, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodFilter, on};
use axum::{Json, Router};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::{ConnEvent, Hub, NS_PLAYER, RoomCmd, StateEdit};
use crate::store::Store;

/// What a route was asked.
#[derive(Debug, Clone, Default)]
pub struct ApiRequest {
    /// `{name}` parts of the path.
    pub params: HashMap<String, String>,
    /// `?key=value` parts.
    pub query: HashMap<String, String>,
    /// The body, if it was JSON; `Null` otherwise.
    pub body: Value,
    /// The bearer token, if one was sent.
    pub token: Option<String>,
}

impl ApiRequest {
    /// A `{name}` from the path. Empty if the route has no such part.
    pub fn param(&self, name: &str) -> &str {
        self.params.get(name).map(String::as_str).unwrap_or("")
    }

    pub fn query(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(String::as_str)
    }
}

/// A refusal: a status and a reason, sent as `{"error": why}`.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub why: String,
}

impl ApiError {
    pub fn new(status: u16, why: &str) -> Self {
        ApiError { status, why: why.to_string() }
    }
    pub fn not_found() -> Self {
        Self::new(404, "not found")
    }
    pub fn bad(why: &str) -> Self {
        Self::new(400, why)
    }
    pub fn unauthorized() -> Self {
        Self::new(401, "sign in: this needs a bearer token")
    }
    pub fn forbidden() -> Self {
        Self::new(403, "that is not yours")
    }
}

/// What a route can ask of the server.
pub struct Api<'a> {
    hub: &'a Arc<Hub>,
    token: Option<&'a str>,
}

impl Api<'_> {
    /// The server's store.
    pub fn store(&self) -> &dyn Store {
        &*self.hub.store
    }

    /// The player the request's bearer token belongs to. An error - not a
    /// guess - when there is no token or it is not one of ours.
    pub fn caller(&self) -> Result<String, ApiError> {
        // No token is not "a new guest": that is for `hello`, not for this.
        let token = self.token.filter(|t| !t.is_empty()).ok_or_else(ApiError::unauthorized)?;
        let keep = self.hub.config.api_caller_cache_secs;
        if keep > 0.0 {
            let callers = self.hub.api_callers.lock().unwrap();
            if let Some((player, since)) = callers.get(token)
                && since.elapsed().as_secs_f64() < keep
            {
                return Ok(player.clone());
            }
        }
        let player = self
            .hub
            .auth
            .authenticate(&json!({ "token": token }), &*self.hub.store)
            .map(|identity| identity.player)
            .map_err(|why| ApiError::new(401, &why))?;
        if keep > 0.0 {
            let mut callers = self.hub.api_callers.lock().unwrap();
            if callers.len() > 4096 {
                callers.retain(|_, (_, since)| since.elapsed().as_secs_f64() < keep);
            }
            callers.insert(token.to_string(), (player.clone(), std::time::Instant::now()));
        }
        Ok(player)
    }

    /// A player: `{"id","name","state","online","room"}`, or `None` if there
    /// is no such player. The state is the room's own copy when they are in
    /// one - what is true this moment - and the last save otherwise. In a
    /// cluster a player on another node is asked of that node (`"node"`
    /// names it); one on no node is read from the saved record.
    pub fn player(&self, id: &str) -> Option<Value> {
        if let Some(found) = player_here(self.hub, id) {
            return Some(found);
        }
        if let Some(cluster) = &self.hub.cluster
            && let Some(node) = cluster.locate(id)
            && let Ok(found) = cluster.forward(&node, "player", &json!({ "player": id }))
            && found.is_object()
        {
            return Some(found);
        }
        let saved: Value = serde_json::from_str(&self.hub.store.load(NS_PLAYER, id)?).ok()?;
        Some(json!({ "id": id, "name": saved["name"], "state": saved["state"], "online": false, "room": Value::Null }))
    }

    /// Players signed in right now - across the cluster, in one.
    pub fn online(&self) -> usize {
        if self.hub.cluster.is_some() {
            return super::admin::everywhere(self.hub).1.len();
        }
        self.hub.state.lock().unwrap().online.len()
    }

    /// The rooms that are open: `[{"kind","id","players"}]` - with a
    /// `"node"` each in a cluster, where every node's rooms are listed.
    pub fn rooms(&self) -> Vec<Value> {
        if self.hub.cluster.is_some() {
            let (rooms, _, _) = super::admin::everywhere(self.hub);
            return rooms
                .into_iter()
                .map(|r| json!({ "kind": r["kind"], "id": r["id"], "players": r["players"], "node": r["node"] }))
                .collect();
        }
        let state = self.hub.state.lock().unwrap();
        let mut rooms: Vec<Value> = state
            .rooms
            .iter()
            .map(|((kind, id), room)| {
                json!({ "kind": kind, "id": id, "players": room.count.load(std::sync::atomic::Ordering::Relaxed) })
            })
            .collect();
        rooms.sort_by(|a, b| (a["kind"].as_str(), a["id"].as_str()).cmp(&(b["kind"].as_str(), b["id"].as_str())));
        rooms
    }

    /// One room, as it is now: its players with their state, and whatever
    /// the room says of itself (`Room::describe`). `None` if it is not open
    /// - on this server, or in a cluster on any node.
    pub fn room(&self, kind: &str, id: &str) -> Option<Value> {
        if let Some(seen) = room_here(self.hub, kind, id) {
            return Some(seen);
        }
        let cluster = self.hub.cluster.as_ref()?;
        let node = cluster.owner(kind, id)?;
        let asked = json!({ "who": "api", "kind": kind, "room": id, "body": Value::Null });
        cluster.forward(&node, "room", &asked).ok().filter(|v| v.is_object())
    }

    /// Give a room a command and return its answer - what an operator's
    /// command from the dashboard does, so it is `Room::on_admin` that
    /// receives it. **Who may ask is the route's to decide**: check
    /// [`Api::caller`], or a secret of the game's own, before calling this.
    /// `None` if the room is not open on this server.
    pub fn command(&self, kind: &str, id: &str, command: Value) -> Option<Value> {
        let handle = self.hub.state.lock().unwrap().rooms.get(&(kind.to_string(), id.to_string())).cloned()?;
        let (reply, answer) = oneshot::channel();
        handle.tx.blocking_send(RoomCmd::Admin { command, reply }).ok()?;
        answer.blocking_recv().ok()
    }

    /// Change a player's state and return it as it then is. `edit` is run
    /// where the true copy lives: on the room's task if they are in a room
    /// (the room hears `Room::on_state_edited`), on their connection if they
    /// are signed in but in no room, on the saved record if they are away.
    /// One step in each case - nothing else touches the state between the
    /// read and the write - and a player who is on is sent a `state` frame.
    ///
    /// The state stays a JSON object: an edit that leaves it anything else,
    /// or panics, changes nothing and is an error. `edit` may run more than
    /// once if the player moves while it is being delivered, but only one
    /// run is kept.
    ///
    /// **Who may ask is the route's to decide.**
    pub fn edit_player(
        &self,
        id: &str,
        edit: impl Fn(&mut Value) + Send + Sync + 'static,
    ) -> Result<Value, ApiError> {
        let failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = failed.clone();
        // On a copy, kept only if it is still an object: wherever this runs,
        // a route's bug must not take a room down or leave a state no room
        // can read.
        let edit: StateEdit = Arc::new(move |state: &mut Value| {
            let mut copy = state.clone();
            let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| edit(&mut copy)));
            if ran.is_ok() && copy.is_object() {
                *state = copy;
            } else {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        let check = |state: Value| {
            if failed.load(std::sync::atomic::Ordering::Relaxed) {
                Err(ApiError::new(500, "the edit failed: a player's state stays a JSON object"))
            } else {
                Ok(state)
            }
        };
        // They may be between a room and their connection: look again.
        for _ in 0..5 {
            let (room, conn) = {
                let state = self.hub.state.lock().unwrap();
                let online = state.online.get(id);
                let key = online.and_then(|o| o.room.clone()).or_else(|| state.parked.get(id).cloned());
                (key.and_then(|key| state.rooms.get(&key).cloned()), online.map(|o| o.ctrl.clone()))
            };
            if let Some(room) = room {
                let (reply, answer) = oneshot::channel();
                let cmd = RoomCmd::EditWith { player: id.to_string(), edit: edit.clone(), reply };
                if room.tx.blocking_send(cmd).is_ok()
                    && let Ok(Some(state)) = answer.blocking_recv()
                {
                    return check(state);
                }
            } else if let Some(conn) = conn {
                let (reply, answer) = oneshot::channel();
                if conn.send(ConnEvent::EditWith(edit.clone(), reply)).is_ok()
                    && let Ok(Some(state)) = answer.blocking_recv()
                {
                    return check(state);
                }
            } else {
                // Held across the write, so they cannot sign in between the check and it.
                let state = self.hub.state.lock().unwrap();
                if state.online.contains_key(id) || state.parked.contains_key(id) {
                    continue;
                }
                // On another node of the cluster, perhaps: it has the true
                // copy. Read it, change it, write it back - two steps, not
                // one, so a change the room makes in between is lost.
                if let Some(cluster) = &self.hub.cluster
                    && let Some(node) = cluster.locate(id)
                {
                    drop(state);
                    let found = cluster
                        .forward(&node, "player", &json!({ "player": id }))
                        .map_err(|why| ApiError::new(502, &format!("the node that has them did not answer: {why}")))?;
                    let mut after = if found["state"].is_object() { found["state"].clone() } else { json!({}) };
                    edit(&mut after);
                    let after = check(after)?;
                    let body = json!({ "player": id, "state": after, "who": "api" });
                    cluster
                        .forward(&node, "edit", &body)
                        .map_err(|why| ApiError::new(502, &format!("the node that has them did not do it: {why}")))?;
                    return Ok(after);
                }
                let mut saved: Value = self
                    .hub
                    .store
                    .load(NS_PLAYER, id)
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .ok_or_else(ApiError::not_found)?;
                if !saved["state"].is_object() {
                    saved["state"] = json!({});
                }
                edit(&mut saved["state"]);
                let after = check(saved["state"].clone())?;
                self.hub.save_player(id, &saved.to_string());
                return Ok(after);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Err(ApiError::new(409, "they are moving between rooms; try again"))
    }

    /// The room a player is in (or has a held seat in), as `(kind, id)`.
    pub fn room_of(&self, player: &str) -> Option<(String, String)> {
        let state = self.hub.state.lock().unwrap();
        state.online.get(player).and_then(|o| o.room.clone()).or_else(|| state.parked.get(player).cloned())
    }
}

/// A player this node has - signed in here, or with a seat held here - with
/// their state as the room has it. `None` if they are not here.
pub(super) fn player_here(hub: &Arc<Hub>, id: &str) -> Option<Value> {
    let (online, key) = {
        let state = hub.state.lock().unwrap();
        let online = state.online.get(id);
        let key = online.and_then(|o| o.room.clone()).or_else(|| state.parked.get(id).cloned());
        (online.is_some(), key)
    };
    if !online && key.is_none() {
        return None;
    }
    let room = key.as_ref().map(|(kind, id)| json!({ "kind": kind, "id": id }));
    let node = hub.cluster.as_ref().map(|c| json!(c.config.node)).unwrap_or(Value::Null);
    // In a room: the room has the true copy. Ask it.
    if let Some(seen) = key.as_ref().and_then(|(kind, room)| room_here(hub, kind, room))
        && let Some(found) = seen["players"].as_array().into_iter().flatten().find(|p| p["id"] == id)
    {
        return Some(json!({
            "id": id, "name": found["name"], "state": found["state"], "online": online, "room": room, "node": node,
        }));
    }
    let saved: Value = serde_json::from_str(&hub.store.load(NS_PLAYER, id)?).ok()?;
    Some(json!({ "id": id, "name": saved["name"], "state": saved["state"], "online": online, "room": room, "node": node }))
}

/// A room open on this node, inspected.
fn room_here(hub: &Arc<Hub>, kind: &str, id: &str) -> Option<Value> {
    let handle = hub.state.lock().unwrap().rooms.get(&(kind.to_string(), id.to_string())).cloned()?;
    let (reply, answer) = oneshot::channel();
    handle.tx.blocking_send(RoomCmd::Inspect { reply }).ok()?;
    answer.blocking_recv().ok()
}

/// A route's code: any `Fn(&ApiRequest, &Api) -> Result<Value, ApiError>`.
pub trait ApiHandler: Fn(&ApiRequest, &Api) -> Result<Value, ApiError> + Send + Sync + 'static {}
impl<F: Fn(&ApiRequest, &Api) -> Result<Value, ApiError> + Send + Sync + 'static> ApiHandler for F {}

pub(super) type Handler = Arc<dyn Fn(&ApiRequest, &Api) -> Result<Value, ApiError> + Send + Sync>;

/// A route as it was registered.
pub(super) struct Route {
    pub method: &'static str,
    pub path: String,
    pub handler: Handler,
    /// A budget of its own, per address ([`crate::Server::route_limit`]).
    pub per_second: Option<f64>,
}

/// A route's counts and times, for the dashboard and `/metrics`.
#[derive(Debug, Clone, Default)]
pub(crate) struct RouteStats {
    pub requests: u64,
    /// Answered 5xx, or panicked.
    pub failures: u64,
    pub total_ms: f64,
    pub max_ms: f64,
}

impl RouteStats {
    pub fn json(&self, route: &str) -> Value {
        let avg = if self.requests > 0 { self.total_ms / self.requests as f64 } else { 0.0 };
        json!({ "route": route, "requests": self.requests, "failures": self.failures, "avg_ms": avg, "max_ms": self.max_ms })
    }
}

/// The CORS headers: every origin, or exactly the ones the game allows -
/// and for a browser whose origin is not among them, no allow header at
/// all, which is how a browser is told no.
fn cors(hub: &Hub, origin: Option<&str>, mut response: Response) -> Response {
    let allow = match (&hub.config.api_allow_origins, origin) {
        (None, _) => Some("*".to_string()),
        (Some(list), Some(origin)) if list.iter().any(|o| o == origin) => Some(origin.to_string()),
        _ => None,
    };
    let headers = response.headers_mut();
    if let Some(allow) = allow.and_then(|a| HeaderValue::from_str(&a).ok()) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, allow);
        headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    }
    headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("authorization, content-type"));
    headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"));
    response
}

fn origin_of(headers: &HeaderMap) -> Option<String> {
    headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()).map(str::to_string)
}

/// Whether this address may make another request now - of the game's
/// `/api/` routes, or of live ops' snapshots, which share the budget.
pub(super) fn allowed(hub: &Hub, from: std::net::IpAddr) -> bool {
    take(&mut hub.api_buckets.lock().unwrap(), from, hub.config.api_requests_per_second)
}

/// The bucket name callers with a token share per address.
const TOKENED: &str = "* with a token";

/// One request from a bucket keyed by `key`, refilled at `rate` a second up
/// to twice that. `false` when it is empty. A rate of 0 is no limit.
fn take<K: std::hash::Hash + Eq>(buckets: &mut HashMap<K, (f64, std::time::Instant)>, key: K, rate: f64) -> bool {
    if rate <= 0.0 {
        return true;
    }
    let now = std::time::Instant::now();
    // Keys that have been quiet long enough to be full again say nothing.
    if buckets.len() > 4096 {
        buckets.retain(|_, (_, as_of)| now.duration_since(*as_of).as_secs_f64() < 2.0);
    }
    let (tokens, as_of) = buckets.entry(key).or_insert((rate * 2.0, now));
    *tokens = (*tokens + now.duration_since(*as_of).as_secs_f64() * rate).min(rate * 2.0);
    *as_of = now;
    if *tokens < 1.0 {
        return false;
    }
    *tokens -= 1.0;
    true
}

/// Whether one more request may be made now, by everything that holds it
/// back: the address's budget, the bearer token's (a signed-in caller is not
/// held to a shared address's, nor can one spend many addresses'), and the
/// route's own if it has one. Each spends only if all agree - a request
/// refused by one costs the others nothing.
fn admitted(hub: &Hub, from: std::net::IpAddr, token: Option<&str>, route: &Arc<str>, per_second: Option<f64>) -> bool {
    let token_key = token.filter(|t| !t.is_empty()).map(|t| {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        t.hash(&mut h);
        h.finish()
    });
    // The route's limit is checked first: it is the narrowest.
    if let Some(rate) = per_second
        && !take(&mut hub.api_route_buckets.lock().unwrap(), (route.clone(), from), rate)
    {
        return false;
    }
    if let Some(key) = token_key {
        // A caller with a token is held to its own budget, not the
        // address's: everybody at one school is not one player.
        // ..but an address cannot make up tokens to get past its own budget
        // either: callers with tokens from one address share four times
        // its rate between them.
        return take(&mut hub.api_token_buckets.lock().unwrap(), key, hub.config.api_token_requests_per_second)
            && take(
                &mut hub.api_route_buckets.lock().unwrap(),
                (Arc::from(TOKENED), from),
                hub.config.api_requests_per_second * 4.0,
            );
    }
    allowed(hub, from)
}

#[allow(clippy::too_many_arguments)]
async fn run(
    hub: Arc<Hub>,
    from: std::net::IpAddr,
    route: Arc<str>,
    per_second: Option<f64>,
    handler: Handler,
    params: RawPathParams,
    query: HashMap<String, String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request = ApiRequest {
        params: params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        query,
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
        token: headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_string),
    };
    // Off the socket-serving tasks: a handler may read the store or wait on
    // a room, and neither should hold up a tick. And caught, so a route's
    // bug is one failed request rather than a dead thread.
    use std::sync::atomic::Ordering;
    let origin = origin_of(&headers);
    hub.metrics.api_requests.fetch_add(1, Ordering::Relaxed);
    if !admitted(&hub, from, request.token.as_deref(), &route, per_second) {
        hub.metrics.api_limited.fetch_add(1, Ordering::Relaxed);
        let refused = (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "error": "slow down" })));
        return cors(&hub, origin.as_deref(), refused.into_response());
    }
    let started = std::time::Instant::now();
    let counted = hub.clone();
    let done = tokio::task::spawn_blocking(move || {
        let api = Api { hub: &hub, token: request.token.as_deref() };
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(&request, &api)))
    })
    .await;
    let hub = counted;
    let failed = match &done {
        Ok(Ok(Ok(_))) => false,
        Ok(Ok(Err(refused))) => refused.status >= 500,
        _ => true,
    };
    if failed {
        hub.metrics.api_failures.fetch_add(1, Ordering::Relaxed);
    }
    {
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut routes = hub.api_routes.lock().unwrap();
        let stats = routes.entry(route.to_string()).or_default();
        stats.requests += 1;
        stats.failures += u64::from(failed);
        stats.total_ms += ms;
        stats.max_ms = stats.max_ms.max(ms);
    }
    cors(&hub, origin.as_deref(), match done {
        Ok(Ok(Ok(answer))) => Json(answer).into_response(),
        Ok(Ok(Err(refused))) => {
            let status = StatusCode::from_u16(refused.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status, Json(json!({ "error": refused.why }))).into_response()
        }
        _ => {
            log::error!("[api] a route panicked");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "the route failed" }))).into_response()
        }
    })
}

/// The game's routes, as a router to merge into the server's.
/// Who is calling - for the rate limit, and for a connection's address: the
/// peer, unless it is this machine (a host's gateway) or a proxy in
/// `Config::trusted_proxies`, by [`crate::listen::client_address`]. Without
/// this every player behind a gateway or a proxy is one caller, sharing one
/// rate limit.
pub(super) fn caller_ip(peer: std::net::IpAddr, headers: &HeaderMap, trusted: &[std::net::IpAddr]) -> std::net::IpAddr {
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    crate::listen::client_address(peer, text("x-forwarded-for"), text("x-real-ip"), |ip| {
        ip.is_loopback() || trusted.contains(&ip)
    })
}

pub(super) fn routes(routes: Vec<Route>, max_body: usize) -> Router<Arc<Hub>> {
    let mut router = Router::new();
    let mut preflight: Vec<String> = Vec::new();
    for Route { method, path, handler, per_second } in routes {
        let filter = match method {
            "GET" => MethodFilter::GET,
            "POST" => MethodFilter::POST,
            "PUT" => MethodFilter::PUT,
            _ => MethodFilter::DELETE,
        };
        let name: Arc<str> = Arc::from(format!("{method} {path}"));
        let serve = move |State(hub): State<Arc<Hub>>,
                          ConnectInfo(from): ConnectInfo<std::net::SocketAddr>,
                          params: RawPathParams,
                          Query(query): Query<HashMap<String, String>>,
                          headers: HeaderMap,
                          body: Bytes| {
            let ip = caller_ip(from.ip(), &headers, &hub.config.trusted_proxies);
            run(hub, ip, name.clone(), per_second, handler.clone(), params, query, headers, body)
        };
        router = router.route(&path, on(filter, serve));
        if !preflight.contains(&path) {
            // A browser asks first whether it may.
            let ask = |State(hub): State<Arc<Hub>>, headers: HeaderMap| async move {
                cors(&hub, origin_of(&headers).as_deref(), StatusCode::NO_CONTENT.into_response())
            };
            router = router.route(&path, on(MethodFilter::OPTIONS, ask));
            preflight.push(path);
        }
    }
    // A body past the limit is refused (413) before any route reads it.
    router.layer(axum::extract::DefaultBodyLimit::max(max_body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gateway_on_this_machine_names_the_caller_and_nobody_else_may() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "203.0.113.20".parse().unwrap());
        let local: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        let remote: std::net::IpAddr = "198.51.100.99".parse().unwrap();
        assert_eq!(caller_ip(local, &headers, &[]).to_string(), "203.0.113.20");
        assert_eq!(caller_ip(remote, &headers, &[]), remote, "a stranger cannot name somebody else");
        assert_eq!(caller_ip(local, &HeaderMap::new(), &[]), local);
    }

    /// A proxy in front of the server (nginx, on another machine) is
    /// believed once it is named - its `X-Real-IP`, or the last
    /// `X-Forwarded-For` entry, which is the one it wrote: the earlier ones
    /// are whatever the client sent it.
    #[test]
    fn a_named_proxy_is_believed_about_the_entry_it_wrote() {
        let proxy: std::net::IpAddr = "198.51.100.90".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", "203.0.113.7".parse().unwrap());
        assert_eq!(caller_ip(proxy, &headers, &[]), proxy, "not named, not believed");
        assert_eq!(caller_ip(proxy, &headers, &[proxy]).to_string(), "203.0.113.7");
        headers.insert("x-forwarded-for", "10.9.9.9, 198.51.100.4".parse().unwrap());
        assert_eq!(caller_ip(proxy, &headers, &[proxy]).to_string(), "198.51.100.4", "the client wrote the first");
    }
}
