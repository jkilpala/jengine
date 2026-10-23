//! Seeing what the server is doing, and doing something about it.
//!
//! Three surfaces, by who reads them:
//!
//! - **`GET /metrics`** - Prometheus text, for a scraper and Grafana:
//!   counts per kind of room, never a player. Behind the admin token unless
//!   the server was told `public_stats`.
//! - **`/admin/api/..`** - JSON, for scripts and `curl`. Rooms and the
//!   players in them with their state, who is online, recent events; and the
//!   verbs: kick, disconnect, close a room, send a room one of its own
//!   commands, say something to everyone, edit a player who is offline, and
//!   drain the server before a deploy.
//! - **`GET /admin`** - one page that polls that API: the dashboard.
//!
//! Everything under `/admin` exists only when the server has an
//! [`admin_token`](super::Config::admin_token), and every API call must carry
//! it - or the token of an operator added at runtime - as `Authorization:
//! Bearer <token>`, over TLS or from this machine ([`wire_guard`]). The page
//! itself is static and carries nothing; it asks for the token and keeps it
//! for the tab.
//!
//! A room is asked about itself through its own task (`RoomCmd::Inspect`), so
//! looking never races the room's code; the numbers that are read every poll
//! (`RoomStats`, `Metrics`) are atomics the tasks publish.

// A refusal *is* a response here: these helpers return the one the handler
// sends back, which is the size it is.
#![allow(clippy::result_large_err)]

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use serde::Deserialize;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::{ConnEvent, Hub, NS_PLAYER, Operator, Role, RoomCmd, RoomHandle};

pub(super) fn routes() -> Router<Arc<Hub>> {
    Router::new()
        .route("/admin", get(|| async { Html(include_str!("admin.html")) }))
        .route("/admin/api/overview", get(overview))
        .route("/admin/api/players", get(players))
        .route("/admin/api/events", get(events))
        .route("/admin/api/history", get(history))
        .route("/admin/api/room/{kind}/{id}", get(room))
        .route("/admin/api/room/{kind}/{id}/close", post(close_room))
        .route("/admin/api/room/{kind}/{id}/command", post(command))
        .route("/admin/api/player/{id}", get(player).put(edit_player))
        .route("/admin/api/kick", post(kick))
        .route("/admin/api/account/{name}/reset", post(reset_account))
        .route("/admin/api/account/{name}", axum::routing::delete(delete_account))
        .route("/admin/api/notice", post(notice))
        .route("/admin/api/drain", post(drain))
        .route("/admin/api/shutdown", post(shutdown))
        .route("/admin/api/operators", get(list_operators).post(add_operator))
        .route("/admin/api/operators/{name}", axum::routing::delete(remove_operator))
        .route("/admin/api/player/{id}/sign_out", post(sign_out_player))
        .route("/admin/api/channels", get(list_channels))
        .route("/admin/api/channel/{name}/mute", post(mute))
        .route("/admin/api/channel/{name}/clear", post(clear_channel))
}

/// The channels open on this node: who is in each here, who is muted, and
/// what was last said.
async fn list_channels(State(hub): State<Arc<Hub>>, headers: HeaderMap) -> Response {
    if let Err(no) = authorize(&hub, &headers) {
        return no;
    }
    let now = super::unix_now();
    let channels = hub.channels.lock().unwrap();
    let mut rows: Vec<Value> = channels
        .iter()
        .map(|(name, c)| {
            let members: Vec<Value> = c.members.values().map(|m| json!({ "player": m.player, "name": m.name })).collect();
            let muted: Vec<Value> = c
                .muted
                .iter()
                .filter(|(_, until)| **until > now)
                .map(|(player, until)| json!({ "player": player, "until": until }))
                .collect();
            let recent: Vec<Value> = c.recent.iter().filter_map(|s| serde_json::from_str(s.as_str()).ok()).collect();
            json!({ "name": name, "members": members, "muted": muted, "recent": recent })
        })
        .collect();
    rows.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Json(json!({ "channels": rows })).into_response()
}

/// `{"player","minutes","why"}`: the player may not speak in the channel
/// for that long (0 lets them speak again). Kept in the store, and on every
/// node of a cluster.
async fn mute(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(name): Path<String>, Json(body): Json<Value>) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    let Some(player) = body["player"].as_str().filter(|p| !p.is_empty()).map(str::to_string) else {
        return refuse(StatusCode::BAD_REQUEST, "the body needs a \"player\"");
    };
    let minutes = body["minutes"].as_f64().unwrap_or(60.0).max(0.0);
    let until = if minutes > 0.0 { super::unix_now() + minutes * 60.0 } else { 0.0 };
    let key = format!("{name}|{player}");
    if until > 0.0 {
        hub.store.save(super::NS_MUTE, &key, &until.to_string());
    } else {
        hub.store.delete(super::NS_MUTE, &key);
    }
    apply_mute(&hub, &name, &player, until);
    if let Some(cluster) = &hub.cluster {
        cluster.publish(&format!("mute:{name}"), &json!({ "player": player, "until": until }));
    }
    let why = body["why"].as_str().unwrap_or("");
    let what = if until > 0.0 { format!("muted {player} in {name} for {minutes} min") } else { format!("let {player} speak in {name} again") };
    hub.event("admin", &format!("{who}: {what}{}", if why.is_empty() { String::new() } else { format!(": {why}") }));
    Json(json!({ "ok": true, "until": (until > 0.0).then_some(until) })).into_response()
}

/// A mute, here: on the channel if it is open on this node.
pub(super) fn apply_mute(hub: &Hub, channel: &str, player: &str, until: f64) {
    if let Some(c) = hub.channels.lock().unwrap().get_mut(channel) {
        if until > 0.0 {
            c.muted.insert(player.to_string(), until);
        } else {
            c.muted.remove(player);
        }
    }
}

/// Forget what a channel remembers: its backlog, here, in the store and on
/// every node - for something said that nobody should arrive to.
async fn clear_channel(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(name): Path<String>) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    if let Some(c) = hub.channels.lock().unwrap().get_mut(&name) {
        c.recent.clear();
    }
    hub.store.save(super::NS_CHANNEL, &name, "[]");
    if let Some(cluster) = &hub.cluster {
        cluster.publish(&format!("clear:{name}"), &json!({}));
    }
    hub.event("admin", &format!("{who}: cleared what {name} remembers"));
    Json(json!({ "ok": true })).into_response()
}

/// In front of every admin route: a request that crossed the network in
/// plain text is refused before its token is looked at, unless the server
/// was told a plain network is fine (`Config::admin_over_plain`). The two
/// pages themselves carry nothing and are let through, so a person who
/// opens one is told why its numbers do not come.
pub(super) async fn wire_guard(
    State(hub): State<Arc<Hub>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let page =
        request.method() == axum::http::Method::GET && matches!(request.uri().path(), "/admin" | "/admin/liveops");
    if !page && let Err(no) = plain_refused(&hub, peer.ip(), request.headers()) {
        return no;
    }
    next.run(request).await
}

/// The refusal for a token sent in plain text, or `Ok` when it was not.
pub(super) fn plain_refused(hub: &Hub, peer: std::net::IpAddr, headers: &HeaderMap) -> Result<(), Response> {
    if hub.config.admin_over_plain || !hub.in_clear(peer, headers) {
        return Ok(());
    }
    Err(refuse(
        StatusCode::FORBIDDEN,
        &format!(
            "{}: give the server a certificate (--tls-cert/--tls-key), reach it through a TLS proxy it trusts, or call it from its own machine (Config::admin_over_plain allows a plain network)",
            crate::listen::NEEDS_TLS
        ),
    ))
}

pub(super) fn refuse(status: StatusCode, why: &str) -> Response {
    (status, Json(json!({ "error": why }))).into_response()
}

/// Which operator a request is from. Every token is compared in full
/// whatever matches, so the time it takes says nothing about a guess.
fn operator(hub: &Hub, headers: &HeaderMap) -> Option<Operator> {
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let mut found = None;
    for op in &hub.config.operators {
        let same = !op.token.is_empty()
            && op.token.len() == given.len()
            && op.token.bytes().zip(given.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
        if same && found.is_none() {
            found = Some(op.clone());
        }
    }
    if found.is_some() || given.is_empty() {
        return found;
    }
    // Then those added at runtime, whose tokens the store keeps as hashes.
    let hash = token_hash(given);
    let now = unix_secs();
    for op in stored_operators(hub) {
        let same = op.hash.len() == hash.len()
            && op.hash.bytes().zip(hash.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
        if same && found.is_none() && (op.expires == 0 || op.expires > now) {
            found = Some(Operator { name: op.name.clone(), token: String::new(), role: op.role });
        }
    }
    found
}

/// Where operators added at runtime are kept. Key: the name; value:
/// `{"role","hash","expires","by","at"}` - the SHA-256 of the token, never
/// the token itself, so a copy of the store opens no dashboard.
pub(super) const NS_OPERATOR: &str = "operator";

/// An operator added at runtime, as the store keeps them.
#[derive(Debug, Clone)]
pub(crate) struct StoredOperator {
    name: String,
    role: Role,
    hash: String,
    /// Unix seconds it stops working; 0 for never.
    expires: u64,
    by: String,
    at: u64,
    /// The live ops channels this operator may change; `None` is every one.
    channels: Option<Vec<String>>,
}

/// The live ops channels an operator added at runtime may change (L9);
/// `None` - every one - for anyone else.
pub(super) fn operator_channels(hub: &Hub, name: &str) -> Option<Vec<String>> {
    stored_operators(hub).into_iter().find(|op| op.name == name).and_then(|op| op.channels)
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn token_hash(token: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The runtime operators, re-read from the store at most every few seconds
/// - so one taken away on another node of a cluster stops working here too.
fn stored_operators(hub: &Hub) -> Vec<StoredOperator> {
    let mut cache = hub.stored_operators.lock().unwrap();
    if cache.0.is_some_and(|at| at.elapsed() < Duration::from_secs(5)) {
        return cache.1.clone();
    }
    let ops: Vec<StoredOperator> = hub
        .store
        .keys(NS_OPERATOR)
        .into_iter()
        .filter_map(|name| {
            let v: Value = serde_json::from_str(&hub.store.load(NS_OPERATOR, &name)?).ok()?;
            Some(StoredOperator {
                role: if v["role"] == "operator" { Role::Operator } else { Role::Viewer },
                hash: v["hash"].as_str()?.to_string(),
                expires: v["expires"].as_u64().unwrap_or(0),
                by: v["by"].as_str().unwrap_or("").to_string(),
                at: v["at"].as_u64().unwrap_or(0),
                channels: v["channels"]
                    .as_array()
                    .map(|list| list.iter().filter_map(|c| c.as_str().map(str::to_string)).collect()),
                name,
            })
        })
        .collect();
    *cache = (Some(std::time::Instant::now()), ops.clone());
    ops
}

fn forget_stored_operators(hub: &Hub) {
    hub.stored_operators.lock().unwrap().0 = None;
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Operator => "operator",
        Role::Viewer => "viewer",
    }
}

/// Everybody who may open the dashboard: those given in code or flags
/// (`"configured": true`, changed only there) and those added here.
async fn list_operators(State(hub): State<Arc<Hub>>, headers: HeaderMap) -> Response {
    if let Err(no) = authorize(&hub, &headers) {
        return no;
    }
    let mut rows: Vec<Value> = hub
        .config
        .operators
        .iter()
        .map(|op| json!({ "name": op.name, "role": role_name(op.role), "configured": true }))
        .collect();
    let now = unix_secs();
    for op in stored_operators(&hub) {
        rows.push(json!({
            "name": op.name, "role": role_name(op.role), "configured": false,
            "expires": (op.expires > 0).then_some(op.expires), "expired": op.expires > 0 && op.expires <= now,
            "by": op.by, "at": op.at, "channels": op.channels,
        }));
    }
    Json(json!({ "operators": rows })).into_response()
}

/// `{"name","role":"viewer"|"operator","days","channels"}`: a new operator, and their
/// token - said once, in this answer, and kept only as a hash. Giving an
/// existing name a new token replaces the old one, which is how a token is
/// rotated. `days` (optional) makes it stop working after that long.
async fn add_operator(State(hub): State<Arc<Hub>>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    let name = body["name"].as_str().unwrap_or("").trim().to_string();
    let fits =
        (1..=32).contains(&name.chars().count()) && name.chars().all(|c| c.is_alphanumeric() || "_-.@".contains(c));
    if !fits {
        return refuse(StatusCode::BAD_REQUEST, "an operator's name is 1 to 32 letters, digits, _ - . or @");
    }
    if hub.config.operators.iter().any(|op| op.name == name) {
        return refuse(StatusCode::CONFLICT, &format!("{name} is given in the server's code or flags; change them there"));
    }
    let role = match body["role"].as_str().unwrap_or("viewer") {
        "operator" => Role::Operator,
        "viewer" => Role::Viewer,
        other => return refuse(StatusCode::BAD_REQUEST, &format!("no role '{other}': viewer or operator")),
    };
    let expires =
        body["days"].as_f64().filter(|d| *d > 0.0).map(|d| unix_secs() + (d * 86400.0) as u64).unwrap_or(0);
    let token = super::random_hex(24);
    let mut record =
        json!({ "role": role_name(role), "hash": token_hash(&token), "expires": expires, "by": who, "at": unix_secs() });
    // Live ops channels they may change, if not every one (L9).
    if let Some(list) = body["channels"].as_array() {
        record["channels"] = json!(list.iter().filter_map(Value::as_str).collect::<Vec<_>>());
    }
    hub.store.save(NS_OPERATOR, &name, &record.to_string());
    forget_stored_operators(&hub);
    hub.event("admin", &format!("{who}: gave {name} a {} token", role_name(role)));
    Json(json!({
        "ok": true, "name": name, "role": role_name(role), "token": token,
        "expires": (expires > 0).then_some(expires),
    }))
    .into_response()
}

async fn remove_operator(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(name): Path<String>) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    if hub.config.operators.iter().any(|op| op.name == name) {
        return refuse(StatusCode::CONFLICT, &format!("{name} is given in the server's code or flags; change them there"));
    }
    if hub.store.load(NS_OPERATOR, &name).is_none() {
        return refuse(StatusCode::NOT_FOUND, "no such operator");
    }
    hub.store.delete(NS_OPERATOR, &name);
    forget_stored_operators(&hub);
    hub.event("admin", &format!("{who}: took {name}'s token away"));
    Json(json!({ "ok": true })).into_response()
}

/// Every session a player has ends: their sign-in tokens (or, for signed
/// tickets, every ticket issued until now) stop working, and a connection
/// they have open is closed.
async fn sign_out_player(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(id): Path<String>) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    let (auth_hub, player) = (hub.clone(), id.clone());
    let done = tokio::task::spawn_blocking(move || auth_hub.auth.sign_out(&player, &*auth_hub.store)).await;
    match done {
        Ok(Ok(())) => {}
        Ok(Err(why)) => return refuse(StatusCode::CONFLICT, &why),
        Err(_) => return refuse(StatusCode::INTERNAL_SERVER_ERROR, "the sign-out failed"),
    }
    // Remembered callers are forgotten now, not when their memory runs out.
    hub.api_callers.lock().unwrap().retain(|_, (player, _)| *player != id);
    let ctrl = hub.state.lock().unwrap().online.get(&id).map(|o| o.ctrl.clone());
    if let Some(ctrl) = &ctrl {
        let _ = ctrl.send(ConnEvent::Disconnect("signed out by an operator".into()));
    }
    hub.event("admin", &format!("{who}: signed {id} out everywhere"));
    Json(json!({ "ok": true, "was_online": ctrl.is_some() })).into_response()
}

/// May this request look?
pub(super) fn authorize(hub: &Hub, headers: &HeaderMap) -> Result<(), Response> {
    match operator(hub, headers) {
        Some(_) => Ok(()),
        None => Err(refuse(StatusCode::UNAUTHORIZED, "the admin token is missing or wrong")),
    }
}

/// May this request change something? Returns who is asking, for the record.
pub(super) fn authorize_write(hub: &Hub, headers: &HeaderMap) -> Result<String, Response> {
    match operator(hub, headers) {
        Some(op) if op.role == Role::Operator => Ok(op.name),
        Some(op) => Err(refuse(StatusCode::FORBIDDEN, &format!("{} may look, not change things", op.name))),
        None => Err(refuse(StatusCode::UNAUTHORIZED, "the admin token is missing or wrong")),
    }
}

fn find_room(hub: &Hub, kind: &str, id: &str) -> Result<RoomHandle, Response> {
    hub.state
        .lock()
        .unwrap()
        .rooms
        .get(&(kind.to_string(), id.to_string()))
        .cloned()
        .ok_or_else(|| refuse(StatusCode::NOT_FOUND, "no such room"))
}

fn room_row(hub: &Hub, kind: &str, id: &str, room: &RoomHandle) -> Value {
    let options = hub.kinds.get(kind).map(|k| &k.options);
    json!({
        "kind": kind,
        "id": id,
        "players": room.count.load(Ordering::Relaxed),
        "max_players": options.map(|o| o.max_players),
        "persistent": options.map(|o| o.persistent),
        "tick_hz": options.map(|o| o.tick_hz),
        "uptime": room.stats.opened.elapsed().as_secs_f64(),
        "messages": room.stats.messages.load(Ordering::Relaxed),
        "frames_out": room.stats.frames_out.load(Ordering::Relaxed),
        "tick_ms_avg": room.stats.tick_avg_us.load(Ordering::Relaxed) as f64 / 1000.0,
        "tick_ms_max": room.stats.tick_max_us.load(Ordering::Relaxed) as f64 / 1000.0,
        // What the room costs: its average tick, times how often it ticks,
        // as a share of one core. A room's code runs on one thread, so this
        // is its CPU - and 100 is a room that cannot keep up.
        "cpu": room.stats.tick_avg_us.load(Ordering::Relaxed) as f64 / 10_000.0
            * options.map(|o| o.tick_hz).unwrap_or(0.0),
    })
}

fn totals(hub: &Hub) -> Value {
    let m = &hub.metrics;
    let n = |a: &std::sync::atomic::AtomicU64| a.load(Ordering::Relaxed);
    json!({
        "connections": n(&m.connections_total),
        "messages_in": n(&m.messages_in),
        "frames_out": n(&m.frames_out),
        "bytes_out": n(&m.bytes_out),
        "joins": n(&m.joins),
        "joins_refused": n(&m.joins_refused),
        "rate_limited": n(&m.rate_limited),
        "slow_clients_dropped": n(&m.slow_clients_dropped),
        "room_panics": n(&m.room_panics),
        "player_saves": n(&m.player_saves),
        "api_requests": n(&m.api_requests),
        "api_failures": n(&m.api_failures),
        "api_limited": n(&m.api_limited),
    })
}

/// The game's own routes, each with its counts and times.
fn api_routes(hub: &Hub) -> Vec<Value> {
    let routes = hub.api_routes.lock().unwrap();
    let mut rows: Vec<(String, Value)> = routes.iter().map(|(name, s)| (name.clone(), s.json(name))).collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.into_iter().map(|(_, row)| row).collect()
}

/// This node's rooms and players, as rows.
fn summary(hub: &Hub) -> (Vec<Value>, Vec<Value>) {
    let state = hub.state.lock().unwrap();
    let mut rooms: Vec<Value> =
        state.rooms.iter().map(|((kind, id), room)| room_row(hub, kind, id, room)).collect();
    rooms.sort_by(|a, b| {
        (a["kind"].as_str(), a["id"].as_str()).cmp(&(b["kind"].as_str(), b["id"].as_str()))
    });
    let mut players: Vec<Value> = state
        .online
        .iter()
        .map(|(id, o)| {
            json!({
                "id": id, "name": o.name, "since": o.since, "address": o.address.to_string(), "rtt_ms": o.rtt_ms.map(|r| r.round()),
                "room": o.room.as_ref().map(|(kind, id)| json!({ "kind": kind, "id": id })),
            })
        })
        .collect();
    players.sort_by(|a, b| a["since"].as_f64().partial_cmp(&b["since"].as_f64()).unwrap());
    (rooms, players)
}

/// The rooms and players of the whole cluster: this node's, and each other
/// node's as it reports them, every row saying which node it is on. A node
/// that does not answer is named rather than left out in silence.
pub(super) fn everywhere(hub: &Hub) -> (Vec<Value>, Vec<Value>, Vec<String>) {
    let (mut rooms, mut players) = summary(hub);
    let mut silent = Vec::new();
    let Some(cluster) = &hub.cluster else { return (rooms, players, silent) };
    let me = &cluster.config.node;
    for row in rooms.iter_mut().chain(players.iter_mut()) {
        row["node"] = json!(me);
    }
    for node in cluster.nodes().as_array().into_iter().flatten() {
        let (Some(name), Some(internal)) = (node["node"].as_str(), node["internal"].as_str()) else { continue };
        if name == me {
            continue;
        }
        match cluster.forward(internal, "summary", &json!({})) {
            Ok(theirs) => {
                for (key, into) in [("rooms", &mut rooms), ("players", &mut players)] {
                    for row in theirs[key].as_array().into_iter().flatten() {
                        let mut row = row.clone();
                        row["node"] = json!(name);
                        into.push(row);
                    }
                }
            }
            Err(_) => silent.push(name.to_string()),
        }
    }
    (rooms, players, silent)
}

async fn overview(State(hub): State<Arc<Hub>>, headers: HeaderMap) -> Response {
    if let Err(no) = authorize(&hub, &headers) {
        return no;
    }
    let (connections, online, held) = {
        let state = hub.state.lock().unwrap();
        (state.connections, state.online.len(), state.parked.len())
    };
    let mut kinds: Vec<&String> = hub.kinds.keys().collect();
    kinds.sort();
    // Asked with no lock held: these are calls to other machines.
    let (rooms, _, silent) = everywhere(&hub);
    let cluster = hub
        .cluster
        .as_ref()
        .map(|c| json!({ "node": c.config.node, "nodes": c.nodes(), "silent": silent }));
    let warnings: Vec<String> = hub.warnings().into_iter().map(|(_, text)| text).collect();
    Json(json!({
        "resources": hub.resources.lock().unwrap().json(),
        "warnings": warnings,
        "cluster": cluster,
        "version": env!("CARGO_PKG_VERSION"),
        "uptime": hub.started.elapsed().as_secs_f64(),
        "draining": hub.draining.load(Ordering::Relaxed),
        "connections": connections,
        "players": online,
        "held_seats": held,
        "channels": hub.channels.lock().unwrap().len(),
        "kinds": kinds,
        "rooms": rooms,
        "totals": totals(&hub),
        "api_routes": api_routes(&hub),
    }))
    .into_response()
}

async fn players(State(hub): State<Arc<Hub>>, headers: HeaderMap) -> Response {
    if let Err(no) = authorize(&hub, &headers) {
        return no;
    }
    let (_, players, _) = everywhere(&hub);
    Json(json!({ "players": players })).into_response()
}

async fn events(State(hub): State<Arc<Hub>>, headers: HeaderMap) -> Response {
    if let Err(no) = authorize(&hub, &headers) {
        return no;
    }
    let mut events: Vec<Value> = hub.events.lock().unwrap().iter().cloned().collect();
    events.extend(hub.player_events.lock().unwrap().iter().cloned());
    events.sort_by(|a, b| a["at"].as_f64().partial_cmp(&b["at"].as_f64()).unwrap());
    Json(json!({ "events": events })).into_response()
}

/// The numbers over time, oldest first: what the dashboard's charts draw.
#[derive(Deserialize)]
pub(super) struct HistoryQuery {
    /// `hours` (the default: six hours of samples) or `days` (thirty days, hourly).
    range: Option<String>,
}

async fn history(State(hub): State<Arc<Hub>>, headers: HeaderMap, Query(q): Query<HistoryQuery>) -> Response {
    if let Err(no) = authorize(&hub, &headers) {
        return no;
    }
    if q.range.as_deref() == Some("days") {
        let samples: Vec<Value> = hub.hourly.lock().unwrap().iter().cloned().collect();
        return Json(json!({ "period": 3600.0, "samples": samples })).into_response();
    }
    if q.range.as_deref() == Some("week") {
        let samples: Vec<Value> = hub.minutes.lock().unwrap().iter().cloned().collect();
        return Json(json!({ "period": 60.0, "samples": samples })).into_response();
    }
    let samples: Vec<Value> = hub.history.lock().unwrap().iter().cloned().collect();
    Json(json!({ "period": hub.config.history_secs, "samples": samples })).into_response()
}

/// Ask a room's task a question and wait, briefly, for the answer.
async fn ask(room: &RoomHandle, cmd: RoomCmd, answer: oneshot::Receiver<Value>) -> Result<Value, Response> {
    if room.tx.send(cmd).await.is_err() {
        return Err(refuse(StatusCode::NOT_FOUND, "the room has closed"));
    }
    match tokio::time::timeout(Duration::from_secs(3), answer).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => Err(refuse(StatusCode::NOT_FOUND, "the room closed before answering")),
        Err(_) => Err(refuse(StatusCode::GATEWAY_TIMEOUT, "the room did not answer in time")),
    }
}

async fn room(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
) -> Response {
    if let Err(no) = authorize(&hub, &headers) {
        return no;
    }
    room_action(&hub, "", "room", &kind, &id, Value::Null, true).await
}

async fn close_room(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
) -> Response {
    match authorize_write(&hub, &headers) {
        Ok(who) => room_action(&hub, &who, "close", &kind, &id, Value::Null, true).await,
        Err(no) => no,
    }
}

async fn command(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    match authorize_write(&hub, &headers) {
        Ok(who) => room_action(&hub, &who, "command", &kind, &id, body, true).await,
        Err(no) => no,
    }
}

/// Look into a room, close it, or send it one of its own commands - here,
/// or, in a cluster, on the node that has it.
async fn room_action(
    hub: &Arc<Hub>,
    who: &str,
    action: &str,
    kind: &str,
    id: &str,
    body: Value,
    may_pass_on: bool,
) -> Response {
    let handle = match find_room(hub, kind, id) {
        Ok(handle) => handle,
        Err(no) => {
            let elsewhere = hub.cluster.as_ref().filter(|_| may_pass_on).and_then(|c| Some((c, c.owner(kind, id)?)));
            let Some((cluster, node)) = elsewhere else { return no };
            let asked = json!({
                "who": format!("{who}@{}", cluster.config.node), "kind": kind, "room": id, "body": body,
            });
            return match cluster.forward(&node, action, &asked) {
                Ok(answer) => {
                    if action != "room" {
                        hub.event("admin", &format!("{who}: {action} for room {kind}/{id}, on the node that has it"));
                    }
                    Json(answer).into_response()
                }
                Err(why) => refuse(StatusCode::BAD_GATEWAY, &format!("the node that has that room did not do it: {why}")),
            };
        }
    };
    match action {
        "close" => {
            hub.event("admin", &format!("{who}: closed room {kind}/{id}"));
            let _ = handle.tx.send(RoomCmd::Close).await;
            Json(json!({ "ok": true })).into_response()
        }
        "command" => {
            hub.event("admin", &format!("{who}: command to {kind}/{id}: {body}"));
            let (reply, answer) = oneshot::channel();
            match ask(&handle, RoomCmd::Admin { command: body, reply }, answer).await {
                Ok(answer) => Json(answer).into_response(),
                Err(no) => no,
            }
        }
        _ => {
            let (reply, answer) = oneshot::channel();
            match ask(&handle, RoomCmd::Inspect { reply }, answer).await {
                Ok(mut seen) => {
                    seen["stats"] = room_row(hub, kind, id, &handle);
                    Json(seen).into_response()
                }
                Err(no) => no,
            }
        }
    }
}

/// The saved record, and whether they are on right now. For somebody in a
/// room the record is their last save - at most an autosave old; the room's
/// own page shows their state as it is this moment.
async fn player(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(no) = authorize(&hub, &headers) {
        return no;
    }
    let online = hub.state.lock().unwrap().online.get(&id).map(|o| {
        json!({ "since": o.since, "room": o.room.as_ref().map(|(k, i)| json!({ "kind": k, "id": i })) })
    });
    let saved: Option<Value> =
        hub.store.load(NS_PLAYER, &id).and_then(|text| serde_json::from_str(&text).ok());
    if online.is_none() && saved.is_none() {
        return refuse(StatusCode::NOT_FOUND, "no such player");
    }
    Json(json!({ "id": id, "online": online, "saved": saved })).into_response()
}

/// Replace a player's state: `{"state":{..}}`, and optionally `"name"`.
///
/// Wherever the true copy is: offline, the saved record; in a room (or with
/// a seat held in one), through that room's task, which tells the room
/// (`Room::on_state_edited`); signed in but in no room, through their
/// connection. A player who is on is told with a `state` frame.
async fn edit_player(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    edit_as(&hub, &who, &id, body, true).await
}

async fn edit_as(hub: &Arc<Hub>, who: &str, id: &str, body: Value, may_pass_on: bool) -> Response {
    let id = id.to_string();
    if !body["state"].is_object() {
        return refuse(StatusCode::BAD_REQUEST, "the body needs a \"state\" object");
    }
    let new_state = body["state"].clone();
    let (room, conn) = {
        let state = hub.state.lock().unwrap();
        let online = state.online.get(&id);
        let key = online.and_then(|o| o.room.clone()).or_else(|| state.parked.get(&id).cloned());
        (key.and_then(|key| state.rooms.get(&key).cloned()), online.map(|o| o.ctrl.clone()))
    };
    if let Some(room) = room {
        let (reply, answer) = oneshot::channel();
        let cmd = RoomCmd::EditState { player: id.clone(), state: new_state.clone(), reply };
        return match ask(&room, cmd, answer).await {
            Ok(Value::Bool(true)) => {
                hub.event("admin", &format!("{who}: edited the state of {id}, in a room"));
                Json(json!({ "ok": true, "where": "room" })).into_response()
            }
            Ok(_) => refuse(StatusCode::CONFLICT, "they left that room just now; try again"),
            Err(no) => no,
        };
    }
    if let Some(conn) = conn {
        let _ = conn.send(ConnEvent::Edit(new_state));
        hub.event("admin", &format!("{who}: edited the state of {id}, online"));
        return Json(json!({ "ok": true, "where": "connection" })).into_response();
    }
    // Not here. On another node of the cluster, perhaps: it has the true copy.
    if may_pass_on && let Some(answer) = pass_on(hub, who, "edit", &id, &body) {
        return answer;
    }
    // Held across the write, so they cannot sign in between the check and it.
    let state = hub.state.lock().unwrap();
    if state.online.contains_key(&id) || state.parked.contains_key(&id) {
        return refuse(StatusCode::CONFLICT, "they signed in just now; try again");
    }
    let Some(mut saved) =
        hub.store.load(NS_PLAYER, &id).and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return refuse(StatusCode::NOT_FOUND, "no such player");
    };
    saved["state"] = new_state;
    if let Some(name) = body["name"].as_str() {
        saved["name"] = json!(name);
    }
    hub.save_player(&id, &saved.to_string());
    drop(state);
    hub.event("admin", &format!("{who}: edited the saved state of {id}"));
    Json(json!({ "ok": true, "where": "store", "saved": saved })).into_response()
}

/// `{"player":"p_..","why":"..","disconnect":false}` - out of their room, or
/// with `disconnect` off the server. In a cluster a player on another node
/// is kicked there: the request is passed on.
async fn kick(State(hub): State<Arc<Hub>>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    kick_as(&hub, &who, body, true).await
}

/// Pass an operator's action to the node that has the player, if another has.
fn pass_on(hub: &Hub, who: &str, action: &str, player: &str, body: &Value) -> Option<Response> {
    let cluster = hub.cluster.as_ref()?;
    let node = cluster.locate(player)?;
    let mut body = body.clone();
    body["who"] = json!(format!("{who}@{}", cluster.config.node));
    body["player"] = json!(player);
    Some(match cluster.forward(&node, action, &body) {
        Ok(answer) => {
            // On record here too: the operator did it from this node, and
            // should not have to open another to see that they did.
            let did = if action == "kick" { "kicked" } else { "edited the state of" };
            hub.event("admin", &format!("{who}: {did} {player}, on the node that has them"));
            Json(answer).into_response()
        }
        Err(why) => refuse(StatusCode::BAD_GATEWAY, &format!("the node that has them did not do it: {why}")),
    })
}

async fn kick_as(hub: &Arc<Hub>, who: &str, body: Value, may_pass_on: bool) -> Response {
    let Some(id) = body["player"].as_str() else {
        return refuse(StatusCode::BAD_REQUEST, "the body needs a \"player\"");
    };
    let why = body["why"].as_str().unwrap_or("removed by an operator").to_string();
    let disconnect = body["disconnect"].as_bool().unwrap_or(false);
    let found = {
        let state = hub.state.lock().unwrap();
        state.online.get(id).map(|online| {
            let room = online.room.as_ref().and_then(|key| state.rooms.get(key)).cloned();
            (online.ctrl.clone(), room)
        })
    };
    let Some((ctrl, room)) = found else {
        if may_pass_on && let Some(answer) = pass_on(hub, who, "kick", id, &body) {
            return answer;
        }
        return refuse(StatusCode::NOT_FOUND, "that player is not online");
    };
    if disconnect {
        let _ = ctrl.send(ConnEvent::Disconnect(why.clone()));
        hub.event("admin", &format!("{who}: disconnected {id}: {why}"));
    } else {
        let Some(room) = room else {
            return refuse(StatusCode::CONFLICT, "that player is in no room; use \"disconnect\": true");
        };
        let _ = room.tx.send(RoomCmd::Kick { player: id.to_string(), why: why.clone() }).await;
        hub.event("admin", &format!("{who}: kicked {id}: {why}"));
    }
    Json(json!({ "ok": true })).into_response()
}

/// `{"password":".."}`: an operator gives an account a new password. Every
/// session it had ends.
async fn reset_account(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    let password = body["password"].as_str().unwrap_or("").to_string();
    let (auth_hub, account) = (hub.clone(), name.clone());
    let done = tokio::task::spawn_blocking(move || auth_hub.auth.reset_password(&account, &password, &*auth_hub.store)).await;
    match done {
        Ok(Ok(())) => {
            hub.event("admin", &format!("{who}: reset the password of account {name}"));
            Json(json!({ "ok": true })).into_response()
        }
        Ok(Err(why)) => refuse(StatusCode::CONFLICT, &why),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "the reset failed"),
    }
}

/// Delete an account, its sessions, and the player's saved record. Somebody
/// signed in as it is disconnected first.
async fn delete_account(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(name): Path<String>) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    let player = match hub.auth.delete_account(&name, &*hub.store) {
        Ok(player) => player,
        Err(why) => return refuse(StatusCode::CONFLICT, &why),
    };
    let ctrl = hub.state.lock().unwrap().online.get(&player).map(|o| o.ctrl.clone());
    if let Some(ctrl) = ctrl {
        let (done, wait) = oneshot::channel();
        if ctrl.send(ConnEvent::Replaced(done)).is_ok() {
            let _ = tokio::time::timeout(Duration::from_secs(5), wait).await;
        }
    }
    // After they have gone, so their leaving does not save them back.
    hub.store.delete(NS_PLAYER, &player);
    hub.event("admin", &format!("{who}: deleted account {name} ({player})"));
    Json(json!({ "ok": true, "player": player })).into_response()
}

/// `{"text":".."}` to everyone connected - in a room or not - or, with
/// `"kind"` and/or `"room"`, to the players of those rooms. It arrives as
/// `{"t":"notice","text":..}`.
async fn notice(State(hub): State<Arc<Hub>>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    let Some(text) = body["text"].as_str() else {
        return refuse(StatusCode::BAD_REQUEST, "the body needs a \"text\"");
    };
    let frame = json!({ "t": "notice", "text": text });
    if body["kind"].is_null() && body["room"].is_null() {
        // Straight to every socket: somebody between rooms is still somebody
        // who should hear that the server is about to restart.
        let outboxes: Vec<_> = hub.state.lock().unwrap().outboxes.values().cloned().collect();
        let text_frame = axum::extract::ws::Utf8Bytes::from(frame.to_string());
        let reached = outboxes.iter().filter(|out| out.try_send(text_frame.clone()).is_ok()).count();
        if let Some(cluster) = &hub.cluster {
            cluster.publish("notice", &frame);
        }
        hub.event("admin", &format!("{who}: notice to {reached} connection(s): {text}"));
        return Json(json!({ "ok": true, "connections": reached })).into_response();
    }
    let rooms: Vec<RoomHandle> = hub
        .state
        .lock()
        .unwrap()
        .rooms
        .iter()
        .filter(|((kind, id), _)| {
            body["kind"].as_str().is_none_or(|k| k == kind) && body["room"].as_str().is_none_or(|r| r == id)
        })
        .map(|(_, room)| room.clone())
        .collect();
    for room in &rooms {
        let _ = room.tx.send(RoomCmd::Notice { frame: frame.clone() }).await;
    }
    hub.event("admin", &format!("{who}: notice to {} room(s): {text}", rooms.len()));
    Json(json!({ "ok": true, "rooms": rooms.len() })).into_response()
}

/// `{"on":true}` stops new connections and turns `/healthz` to 503; the
/// players already here carry on. `{"on":false}` undoes it.
async fn drain(State(hub): State<Arc<Hub>>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    let on = body["on"].as_bool().unwrap_or(true);
    hub.draining.store(on, Ordering::Relaxed);
    let what = if on { "draining: no new connections" } else { "taking connections again" };
    hub.event("admin", &format!("{who}: {what}"));
    Json(json!({ "ok": true, "draining": on })).into_response()
}

/// Stop the server as Ctrl+C would: everyone saved, then the process ends.
/// What `jengine-fleet`'s agent calls to replace a running server - and the
/// only clean stop there is on Windows, which has no SIGTERM to send.
async fn shutdown(State(hub): State<Arc<Hub>>, headers: HeaderMap) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    hub.event("admin", &format!("{who}: shutdown asked for"));
    let _ = hub.stop.send(true);
    Json(json!({ "ok": true })).into_response()
}

/// Prometheus text exposition. Per kind of room, never per room or player:
/// label values that come and go are what makes a metrics store fall over.
pub(super) async fn metrics(
    State(hub): State<Arc<Hub>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !hub.config.public_stats {
        if let Err(no) = plain_refused(&hub, peer.ip(), &headers) {
            return no;
        }
        if let Err(no) = authorize(&hub, &headers) {
            return no;
        }
    }
    let mut out = String::new();
    let mut one = |name: &str, kind: &str, help: &str, value: f64| {
        let _ = writeln!(out, "# HELP jengine_{name} {help}\n# TYPE jengine_{name} {kind}\njengine_{name} {value}");
    };
    let m = &hub.metrics;
    let n = |a: &std::sync::atomic::AtomicU64| a.load(Ordering::Relaxed) as f64;
    let state = hub.state.lock().unwrap();
    one("uptime_seconds", "gauge", "Seconds since the server started.", hub.started.elapsed().as_secs_f64());
    one("connections", "gauge", "Open sockets.", state.connections as f64);
    one("players_online", "gauge", "Signed-in players.", state.online.len() as f64);
    one("draining", "gauge", "1 while the server takes no new connections.", f64::from(u8::from(hub.draining.load(Ordering::Relaxed))));
    one("connections_total", "counter", "Sockets accepted.", n(&m.connections_total));
    one("messages_in_total", "counter", "Frames from players delivered to rooms.", n(&m.messages_in));
    one("bytes_in_total", "counter", "Bytes of every frame players sent.", n(&m.bytes_in));
    one("frames_out_total", "counter", "Frames rooms sent to players.", n(&m.frames_out));
    one("bytes_out_total", "counter", "Bytes written to players: rooms' frames, replies and channel lines.", n(&m.bytes_out));
    one("joins_total", "counter", "Players let into rooms.", n(&m.joins));
    one("joins_refused_total", "counter", "Joins a room turned away.", n(&m.joins_refused));
    one("rate_limited_total", "counter", "Frames dropped for arriving too fast.", n(&m.rate_limited));
    one("slow_clients_dropped_total", "counter", "Connections dropped for not reading.", n(&m.slow_clients_dropped));
    one("room_panics_total", "counter", "Rooms closed because their code panicked.", n(&m.room_panics));
    one("player_saves_total", "counter", "Player records written to the store.", n(&m.player_saves));
    one("api_requests_total", "counter", "Requests to the game's /api/ routes.", n(&m.api_requests));
    one("api_limited_total", "counter", "Requests to /api/ refused for coming too fast.", n(&m.api_limited));
    one("api_failures_total", "counter", "Of those, the ones that failed in the route (5xx or a panic).", n(&m.api_failures));
    let used = *hub.resources.lock().unwrap();
    one("process_cpu_cores", "gauge", "Cores this process is using: 2.5 is two and a half.", f64::from(used.cpu_percent) / 100.0);
    one("process_memory_bytes", "gauge", "Memory this process holds.", used.memory_bytes as f64);
    one("machine_cpu_ratio", "gauge", "How busy the whole machine is, 0 to 1.", f64::from(used.machine_cpu_percent) / 100.0);
    one("machine_memory_used_bytes", "gauge", "Memory in use on the machine.", used.machine_memory_used as f64);
    one("machine_memory_total_bytes", "gauge", "Memory the machine has.", used.machine_memory_total as f64);
    one("disk_free_bytes", "gauge", "Free space on the disk the server runs from.", used.disk_free as f64);
    one("disk_total_bytes", "gauge", "That disk's size.", used.disk_total as f64);
    one("machine_network_receive_bytes_per_second", "gauge", "Bytes a second coming into the machine, every interface.", used.net_rx_per_sec);
    one("machine_network_transmit_bytes_per_second", "gauge", "Bytes a second leaving the machine, every interface.", used.net_tx_per_sec);

    // (rooms, players, worst average tick, worst tick) per kind.
    let mut kinds: std::collections::BTreeMap<&str, (u64, u64, u64, u64)> =
        hub.kinds.keys().map(|k| (k.as_str(), (0, 0, 0, 0))).collect();
    for ((kind, _), room) in &state.rooms {
        let k = kinds.entry(kind.as_str()).or_default();
        k.0 += 1;
        k.1 += room.count.load(Ordering::Relaxed) as u64;
        k.2 = k.2.max(room.stats.tick_avg_us.load(Ordering::Relaxed));
        k.3 = k.3.max(room.stats.tick_max_us.load(Ordering::Relaxed));
    }
    type Pick = fn(&(u64, u64, u64, u64)) -> f64;
    let per_kind: [(&str, &str, Pick); 4] = [
        ("rooms", "Open rooms.", |k| k.0 as f64),
        ("room_players", "Players in rooms.", |k| k.1 as f64),
        ("tick_seconds_avg", "The slowest room's average tick.", |k| k.2 as f64 / 1e6),
        ("tick_seconds_max", "The slowest recent tick of any room.", |k| k.3 as f64 / 1e6),
    ];
    for (name, help, pick) in per_kind {
        let _ = writeln!(out, "# HELP jengine_{name} {help}\n# TYPE jengine_{name} gauge");
        for (kind, k) in &kinds {
            let _ = writeln!(out, "jengine_{name}{{kind=\"{kind}\"}} {}", pick(k));
        }
    }
    // Per route: the set of routes is fixed when the server starts, so these
    // labels are bounded.
    let routes = hub.api_routes.lock().unwrap();
    if !routes.is_empty() {
        let mut names: Vec<&String> = routes.keys().collect();
        names.sort();
        let _ = writeln!(out, "# HELP jengine_api_route_requests_total Requests to one /api/ route.\n# TYPE jengine_api_route_requests_total counter");
        for name in &names {
            let _ = writeln!(out, "jengine_api_route_requests_total{{route=\"{name}\"}} {}", routes[*name].requests);
        }
        let _ = writeln!(out, "# HELP jengine_api_route_failures_total Of those, failures (5xx or a panic).\n# TYPE jengine_api_route_failures_total counter");
        for name in &names {
            let _ = writeln!(out, "jengine_api_route_failures_total{{route=\"{name}\"}} {}", routes[*name].failures);
        }
        let _ = writeln!(out, "# HELP jengine_api_route_seconds_total Time spent in one /api/ route.\n# TYPE jengine_api_route_seconds_total counter");
        for name in &names {
            let _ = writeln!(out, "jengine_api_route_seconds_total{{route=\"{name}\"}} {}", routes[*name].total_ms / 1000.0);
        }
        let _ = writeln!(out, "# HELP jengine_api_route_max_seconds The slowest request to one /api/ route.\n# TYPE jengine_api_route_max_seconds gauge");
        for name in &names {
            let _ = writeln!(out, "jengine_api_route_max_seconds{{route=\"{name}\"}} {}", routes[*name].max_ms / 1000.0);
        }
    }
    drop(routes);
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], out).into_response()
}

// ---- what the cluster's coordinator asks of a node --------------------------

/// A call from the coordinator: signed with this node's key, for this node,
/// recently, and not seen before - its JSON body, or the refusal.
fn from_cluster(hub: &Hub, path: &str, headers: &HeaderMap, raw: &[u8]) -> Result<Value, Response> {
    let signed = hub.cluster.as_ref().is_some_and(|cluster| cluster.from_coordinator("POST", path, headers, raw));
    if !signed {
        return Err(refuse(StatusCode::UNAUTHORIZED, "not signed by this cluster's coordinator"));
    }
    serde_json::from_slice(raw).map_err(|_| refuse(StatusCode::BAD_REQUEST, "not JSON"))
}

/// A player is signing in on another node. If they are connected here, that
/// connection saves them and goes - and this answers only once it has. If
/// their seat is held here, say so: they belong back on this node.
pub(super) async fn cluster_replace(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    raw: axum::body::Bytes,
) -> Response {
    let body = match from_cluster(&hub, "/cluster/replace", &headers, &raw) {
        Ok(body) => body,
        Err(no) => return no,
    };
    let player = body["player"].as_str().unwrap_or("");
    let (ctrl, held) = {
        let state = hub.state.lock().unwrap();
        (state.online.get(player).map(|o| o.ctrl.clone()), state.parked.contains_key(player))
    };
    if held {
        return Json(json!({ "held": true })).into_response();
    }
    if let Some(ctrl) = ctrl {
        let (done, wait) = oneshot::channel();
        if ctrl.send(ConnEvent::Replaced(done)).is_ok() {
            let _ = tokio::time::timeout(Duration::from_secs(5), wait).await;
        }
    }
    Json(json!({ "ok": true })).into_response()
}

/// Something said on another node, for whoever here should hear it.
pub(super) async fn cluster_deliver(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    raw: axum::body::Bytes,
) -> Response {
    let body = match from_cluster(&hub, "/cluster/deliver", &headers, &raw) {
        Ok(body) => body,
        Err(no) => return no,
    };
    let frame = axum::extract::ws::Utf8Bytes::from(body["frame"].to_string());
    let topic = body["topic"].as_str().unwrap_or("");
    // An operator on another node muted somebody, or cleared a channel.
    if let Some(channel) = topic.strip_prefix("mute:") {
        let (player, until) = (body["frame"]["player"].as_str().unwrap_or(""), body["frame"]["until"].as_f64().unwrap_or(0.0));
        apply_mute(&hub, channel, player, until);
        return Json(json!({ "ok": true })).into_response();
    }
    if let Some(channel) = topic.strip_prefix("clear:") {
        if let Some(c) = hub.channels.lock().unwrap().get_mut(channel) {
            c.recent.clear();
        }
        return Json(json!({ "ok": true })).into_response();
    }
    let outs: Vec<_> = match topic.strip_prefix("channel:") {
        // Remembered here too, for whoever joins the channel on this node.
        Some(channel) => {
            if let Some(found) = hub.channels.lock().unwrap().get_mut(channel) {
                found.say(&frame);
            }
            Vec::new()
        }
        None if topic == "notice" => hub.state.lock().unwrap().outboxes.values().cloned().collect(),
        None => Vec::new(),
    };
    for out in &outs {
        let _ = out.try_send(frame.clone());
    }
    Json(json!({ "ok": true, "reached": outs.len() })).into_response()
}

/// An operator on another node asked for something to be done to a player
/// who is on this one.
pub(super) async fn cluster_admin(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    Path(action): Path<String>,
    raw: axum::body::Bytes,
) -> Response {
    let body = match from_cluster(&hub, &format!("/cluster/admin/{action}"), &headers, &raw) {
        Ok(body) => body,
        Err(no) => return no,
    };
    let who = body["who"].as_str().unwrap_or("another node").to_string();
    match action.as_str() {
        "kick" => kick_as(&hub, &who, body, false).await,
        "edit" => {
            let player = body["player"].as_str().unwrap_or("").to_string();
            edit_as(&hub, &who, &player, body, false).await
        }
        "summary" => {
            let (rooms, players) = summary(&hub);
            Json(json!({ "rooms": rooms, "players": players })).into_response()
        }
        // What one of the game's routes asks about a player this node has.
        "player" => {
            let id = body["player"].as_str().unwrap_or("").to_string();
            let found = tokio::task::spawn_blocking(move || super::api::player_here(&hub, &id)).await.ok().flatten();
            match found {
                Some(player) => Json(player).into_response(),
                None => refuse(StatusCode::NOT_FOUND, "not here"),
            }
        }
        "room" | "close" | "command" => {
            let (kind, room) = (body["kind"].as_str().unwrap_or(""), body["room"].as_str().unwrap_or(""));
            room_action(&hub, &who, &action, kind, room, body["body"].clone(), false).await
        }
        _ => refuse(StatusCode::NOT_FOUND, "no such action"),
    }
}
