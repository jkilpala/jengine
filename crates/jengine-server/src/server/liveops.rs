//! Live ops over HTTP ([`crate::liveops`]).
//!
//! What a game asks, with no token - a shipped game has nothing secret to
//! send, and what it gets is signed:
//!
//! | | |
//! |---|---|
//! | `GET /liveops/v1/{channel}/snapshot?install=&player=&platform=&version=` | the signed snapshot for that install |
//! | `GET /liveops/v1/packs/{file}` | a pack archive, cacheable forever (its name carries its hash) |
//! | `GET /liveops/v1/key` | the public key, for setting a game up - a game trusts the key it was built with, never this answer |
//! | `POST /liveops/v1/{channel}/exposure?install=&player=` `{"experiment","event","value"}` | an install saw its variant, or did something worth counting in it (L6) |
//!
//! A snapshot also reads `lang`, `region` and `a.<name>` - the game's
//! language, region and attributes of its own, which rules can ask (L5).
//!
//! All three answer any origin (`Access-Control-Allow-Origin: *`): a web
//! build is served from somewhere else, and the editor from its own origin.
//!
//! What an operator does, with the admin token (a viewer may only look):
//! `GET /admin/liveops` is the page; under `/admin/api/liveops`: `GET` (the
//! channels, the key, the archives), `GET`/`PUT channel/{name}`, `POST
//! channel/{name}/release`, `POST channel/{name}/rollback`, `GET
//! channel/{name}/preview?install=..`, `GET channel/{name}/results`, `POST
//! promote` and `PUT blob/{file}?sha256=..` (the archive's bytes as the
//! body). An operator added at runtime with `"channels"` may change only
//! those channels (L9).

#![allow(clippy::result_large_err)]

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post, put};
use serde::Deserialize;
use serde_json::{Value, json};

use super::Hub;
use super::admin::{authorize, authorize_write, refuse};
use crate::liveops::{ChannelEdit, Part, Service, UpdateError};

/// The largest archive an operator can upload.
pub const MAX_ARCHIVE_BYTES: usize = 1 << 30;

pub(super) fn public_routes() -> Router<Arc<Hub>> {
    Router::new()
        .route("/liveops/v1/{channel}/snapshot", get(snapshot))
        .route("/liveops/v1/{channel}/exposure", post(exposure).options(preflight))
        .route("/liveops/v1/packs/{file}", get(pack))
        .route("/liveops/v1/key", get(key))
}

pub(super) fn admin_routes() -> Router<Arc<Hub>> {
    Router::new()
        .route("/admin/liveops", get(|| async { Html(include_str!("liveops.html")) }))
        .route("/admin/api/liveops", get(overview))
        .route("/admin/api/liveops/channel/{name}", get(channel).put(save_channel))
        .route("/admin/api/liveops/channel/{name}/release", post(release))
        .route("/admin/api/liveops/channel/{name}/rollback", post(rollback))
        .route("/admin/api/liveops/channel/{name}/restore", post(restore))
        .route("/admin/api/liveops/channel/{name}/preview", get(preview))
        .route("/admin/api/liveops/channel/{name}/results", get(results))
        .route("/admin/api/liveops/promote", post(promote))
        .route("/admin/api/liveops/blob/{file}", put(upload).layer(DefaultBodyLimit::max(MAX_ARCHIVE_BYTES)))
}

fn service(hub: &Hub) -> Result<&Arc<Service>, Response> {
    hub.liveops.as_ref().ok_or_else(|| refuse(StatusCode::NOT_FOUND, "this server has no live ops"))
}

fn known_channel(s: &Service, name: &str) -> Result<(), Response> {
    if s.has_channel(name) {
        Ok(())
    } else {
        Err(refuse(StatusCode::NOT_FOUND, &format!("no channel '{name}' (there are {})", s.channels().join(", "))))
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Where a game reached this server, as it sees it - what a release's pack
/// URL is built on. A proxy in front says what it was asked for, and a
/// gateway that serves this server under a path (`/liveops` on a host) says
/// which: without it the packs would be sent for at a path the gateway
/// routes somewhere else.
fn base_url(hub: &Hub, headers: &HeaderMap) -> String {
    reached_at(hub.secure, headers)
}

fn reached_at(secure: bool, headers: &HeaderMap) -> String {
    let h = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let host = h("x-forwarded-host").or_else(|| h("host")).unwrap_or_else(|| "localhost".into());
    let proto = h("x-forwarded-proto").unwrap_or_else(|| if secure { "https".into() } else { "http".into() });
    // A plain path only: it ends up in a URL every game is sent.
    let prefix = h("x-forwarded-prefix")
        .filter(|p| p.starts_with('/') && !p.contains("//") && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b)))
        .map(|p| p.trim_end_matches('/').to_string())
        .unwrap_or_default();
    format!("{proto}://{host}{prefix}")
}

/// A JSON body, whatever `Content-Type` the caller sent - `curl -d` sends a
/// form type, and a refusal over a header helps nobody.
fn json_body(body: &Bytes) -> Result<Value, Response> {
    serde_json::from_slice(body).map_err(|e| refuse(StatusCode::BAD_REQUEST, &format!("the body is not JSON: {e}")))
}

fn open(mut r: Response) -> Response {
    r.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    r
}

#[derive(Deserialize, Default)]
struct Asker {
    #[serde(default)]
    install: String,
    #[serde(default)]
    player: String,
    #[serde(default)]
    platform: String,
    #[serde(default)]
    version: String,
}

impl Asker {
    fn query(self) -> jengine_liveops::Query {
        jengine_liveops::Query { install: self.install, player: self.player, platform: self.platform, app_version: self.version }
    }

    /// The query and what else the game said of itself, from the URL's
    /// parameters: `lang`, `region`, and `a.<name>` for its own attributes.
    fn from_params(params: &std::collections::HashMap<String, String>) -> (jengine_liveops::Query, crate::liveops::Asked) {
        let p = |k: &str| params.get(k).cloned().unwrap_or_default();
        let q = Asker { install: p("install"), player: p("player"), platform: p("platform"), version: p("version") }.query();
        let asked = crate::liveops::Asked {
            language: p("lang"),
            region: p("region"),
            attributes: params
                .iter()
                .filter_map(|(k, v)| Some((k.strip_prefix("a.")?.to_string(), v.clone())))
                .filter(|(k, v)| k.len() <= 64 && v.len() <= 256)
                .take(32)
                .collect(),
        };
        (q, asked)
    }
}

/// A browser asks before it posts JSON from another origin.
async fn preflight() -> Response {
    let mut r = open(StatusCode::NO_CONTENT.into_response());
    r.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("content-type"));
    r.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, OPTIONS"));
    r
}

/// L6: `{"experiment","event","value"}` from a game - counted for the
/// variant this install is in, by the service's own reckoning.
async fn exposure(
    State(hub): State<Arc<Hub>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    body: Bytes,
) -> Response {
    let from = super::api::caller_ip(peer.ip(), &headers, &hub.config.trusted_proxies);
    if !super::api::allowed(&hub, from) {
        return open(refuse(StatusCode::TOO_MANY_REQUESTS, "too many from this address; try again shortly"));
    }
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return open(r),
    };
    if let Err(r) = known_channel(s, &name) {
        return open(r);
    }
    let (q, _) = Asker::from_params(&params);
    if q.install.is_empty() {
        return open(refuse(StatusCode::BAD_REQUEST, "say which install is telling (?install=)"));
    }
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let (experiment, event) = (body["experiment"].as_str().unwrap_or(""), body["event"].as_str().unwrap_or(""));
    let value = body["value"].as_f64().unwrap_or(0.0);
    match s.expose(&name, &q, experiment, event, value) {
        Ok(variant) => open(Json(json!({ "ok": true, "variant": variant })).into_response()),
        Err(why) => open(refuse(StatusCode::BAD_REQUEST, &why)),
    }
}

async fn results(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(name): Path<String>) -> Response {
    if let Err(no) = authorize(&hub, &headers) {
        return no;
    }
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(r) = known_channel(s, &name) {
        return r;
    }
    Json(json!({ "channel": name, "results": s.results(&name) })).into_response()
}

async fn snapshot(
    State(hub): State<Arc<Hub>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let (q, asked) = Asker::from_params(&params);
    // Each answer is worked out and signed afresh - for this install, so
    // there is nothing to cache - and an address asking faster than the
    // game's `/api/` budget allows is told to wait.
    let from = super::api::caller_ip(peer.ip(), &headers, &hub.config.trusted_proxies);
    if !super::api::allowed(&hub, from) {
        let mut r = open(refuse(StatusCode::TOO_MANY_REQUESTS, "too many asks from this address; try again shortly"));
        r.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        return r;
    }
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return open(r),
    };
    if let Err(r) = known_channel(s, &name) {
        return open(r);
    }
    if q.install.is_empty() {
        return open(refuse(StatusCode::BAD_REQUEST, "say which install is asking (?install=)"));
    }
    let env = s.snapshot_asked(&name, &q, &asked, now(), &base_url(&hub, &headers));
    let mut r = Json(env).into_response();
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    open(r)
}

async fn pack(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(file): Path<String>) -> Response {
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return open(r),
    };
    let Some((blob, path)) = s.blob(&file) else {
        return open(refuse(StatusCode::NOT_FOUND, "no such archive"));
    };
    let etag = format!("\"{}\"", blob.sha256);
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return open(StatusCode::NOT_MODIFIED.into_response());
    }
    // Streamed from the disk a piece at a time: a few hundred players
    // fetching a 300 MB pack at once used to be that many copies in memory.
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => return open(refuse(StatusCode::INTERNAL_SERVER_ERROR, &format!("reading the archive: {e}"))),
    };
    let chunks = futures_util::stream::unfold(file, |mut file| async move {
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0u8; 64 * 1024];
        match file.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<Bytes, std::io::Error>(Bytes::from(buf)), file))
            }
            Err(e) => Some((Err(e), file)),
        }
    });
    let mut r = Response::new(Body::from_stream(chunks));
    let h = r.headers_mut();
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(blob.size));
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable"));
    if let Ok(v) = HeaderValue::from_str(&etag) {
        h.insert(header::ETAG, v);
    }
    open(r)
}

async fn key(State(hub): State<Arc<Hub>>) -> Response {
    match service(&hub) {
        Ok(s) => open(s.public_key().into_response()),
        Err(r) => open(r),
    }
}

async fn overview(State(hub): State<Arc<Hub>>, headers: HeaderMap) -> Response {
    if let Err(no) = authorize(&hub, &headers) {
        return no;
    }
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let channels: Vec<Value> = s
        .channels()
        .iter()
        .map(|name| {
            let c = s.channel(name);
            json!({
                "name": name, "revision": c.revision, "params": c.params.len(),
                "experiments": c.experiments.len(), "events": c.events.len(),
                "release": c.release.as_ref().map(|r| r.number),
                "updated_at": c.updated_at, "updated_by": c.updated_by,
            })
        })
        .collect();
    Json(json!({
        "channels": channels,
        "public_key": s.public_key(),
        // When games trust a root that endorsed this service's own key: the
        // key it signs with, and when that endorsement runs out.
        "serving_key": s.serving_key(),
        "endorsed_until": s.endorsement_ends(),
        "refresh_secs": s.refresh_secs(),
        "packs_url": s.packs_url(&base_url(&hub, &headers)),
        // Where a game - and `jengine liveops publish` - reaches this
        // service, as the request that asked was told.
        "base_url": base_url(&hub, &headers),
        "blobs": s.blobs(),
        "now": now(),
    }))
    .into_response()
}

async fn channel(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(name): Path<String>) -> Response {
    if let Err(no) = authorize(&hub, &headers) {
        return no;
    }
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(r) = known_channel(s, &name) {
        return r;
    }
    Json(json!({
        "channel": s.channel(&name),
        "releases": s.releases(&name),
        "history": s.config_history(&name),
        "now": now(),
    }))
    .into_response()
}

#[derive(Deserialize)]
struct Save {
    revision: u64,
    #[serde(flatten)]
    edit: ChannelEdit,
}

/// May `who` change channel `name`? An operator added at runtime with a list
/// of channels may change only those (L9).
fn on_channel(hub: &Hub, who: &str, name: &str) -> Result<(), Response> {
    match super::admin::operator_channels(hub, who) {
        Some(channels) if !channels.iter().any(|c| c == name) => {
            Err(refuse(StatusCode::FORBIDDEN, &format!("{who} may change {} only, not {name}", channels.join(", "))))
        }
        _ => Ok(()),
    }
}

async fn save_channel(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(name): Path<String>, body: Bytes) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    if let Err(no) = on_channel(&hub, &who, &name) {
        return no;
    }
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(r) = known_channel(s, &name) {
        return r;
    }
    let save: Save = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, &format!("not a channel: {e}")),
    };
    match s.update(&name, save.revision, save.edit, &who, now()) {
        Ok(c) => {
            hub.event("liveops", &format!("{who}: saved {name} (revision {})", c.revision));
            Json(json!({ "ok": true, "channel": c })).into_response()
        }
        Err(UpdateError::Stale(rev)) => refuse(
            StatusCode::CONFLICT,
            &format!("{name} was changed by somebody else since you loaded it (now revision {rev}) - load it again"),
        ),
        Err(UpdateError::Invalid(why)) => refuse(StatusCode::BAD_REQUEST, &why),
    }
}

async fn release(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(name): Path<String>, body: Bytes) -> Response {
    let body = match json_body(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    if let Err(no) = on_channel(&hub, &who, &name) {
        return no;
    }
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(r) = known_channel(s, &name) {
        return r;
    }
    let manifest: jengine_liveops::PackManifest = match serde_json::from_value(body["manifest"].clone()) {
        Ok(m) => m,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, &format!("the body needs a \"manifest\" (a packs.json): {e}")),
    };
    let notes = body["notes"].as_str().unwrap_or("");
    match s.publish(&name, manifest, notes, &who, now()) {
        Ok(r) => {
            hub.event("liveops", &format!("{who}: published release {} on {name} ({} pack(s))", r.number, r.manifest.packs.len()));
            // What no kept release names any more, and is old enough to be
            // nobody's next release, goes.
            let gone = s.collect_archives(now());
            if !gone.is_empty() {
                hub.event("liveops", &format!("deleted {} archive(s) no release names: {}", gone.len(), gone.join(", ")));
            }
            Json(json!({ "ok": true, "release": r, "deleted": gone })).into_response()
        }
        Err(why) => refuse(StatusCode::BAD_REQUEST, &why),
    }
}

/// `{"revision": n}`: the channel's config, experiments and events as they
/// were at revision `n`, saved again.
async fn restore(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(name): Path<String>, body: Bytes) -> Response {
    let body = match json_body(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    if let Err(no) = on_channel(&hub, &who, &name) {
        return no;
    }
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(r) = known_channel(s, &name) {
        return r;
    }
    let Some(revision) = body["revision"].as_u64() else {
        return refuse(StatusCode::BAD_REQUEST, "the body needs a \"revision\" number");
    };
    match s.restore(&name, revision, &who, now()) {
        Ok(c) => {
            hub.event("liveops", &format!("{who}: put {name} back as it was at revision {revision} (now revision {})", c.revision));
            Json(json!({ "ok": true, "channel": c })).into_response()
        }
        Err(UpdateError::Stale(rev)) => refuse(StatusCode::CONFLICT, &format!("{name} changed meanwhile (revision {rev})")),
        Err(UpdateError::Invalid(why)) => refuse(StatusCode::BAD_REQUEST, &why),
    }
}

async fn rollback(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(name): Path<String>, body: Bytes) -> Response {
    let body = match json_body(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    if let Err(no) = on_channel(&hub, &who, &name) {
        return no;
    }
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(r) = known_channel(s, &name) {
        return r;
    }
    let Some(number) = body["release"].as_u64() else {
        return refuse(StatusCode::BAD_REQUEST, "the body needs a \"release\" number");
    };
    match s.rollback(&name, number, &who, now()) {
        Ok(r) => {
            hub.event("liveops", &format!("{who}: rolled {name} back to release {number} (now release {})", r.number));
            Json(json!({ "ok": true, "release": r })).into_response()
        }
        Err(why) => refuse(StatusCode::BAD_REQUEST, &why),
    }
}

async fn preview(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(name): Path<String>, Query(a): Query<Asker>) -> Response {
    if let Err(no) = authorize(&hub, &headers) {
        return no;
    }
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(r) = known_channel(s, &name) {
        return r;
    }
    let at = headers.get("x-liveops-at").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or_else(now);
    let snap = crate::liveops::resolve(&name, &s.channel(&name), &a.query(), at, s.refresh_secs(), &s.packs_url(&base_url(&hub, &headers)));
    Json(snap).into_response()
}

async fn promote(State(hub): State<Arc<Hub>>, headers: HeaderMap, body: Bytes) -> Response {
    let body = match json_body(&body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let (Some(from), Some(to)) = (body["from"].as_str(), body["to"].as_str()) else {
        return refuse(StatusCode::BAD_REQUEST, "the body needs \"from\" and \"to\"");
    };
    // Promoting changes the channel promoted to.
    if let Err(no) = on_channel(&hub, &who, to) {
        return no;
    }
    for c in [from, to] {
        if let Err(r) = known_channel(s, c) {
            return r;
        }
    }
    let parts: Vec<Part> = match serde_json::from_value(body["parts"].clone()) {
        Ok(p) => p,
        Err(_) => return refuse(StatusCode::BAD_REQUEST, "\"parts\" is a list of config, experiments, events, content"),
    };
    match s.promote(from, to, &parts, &who, now()) {
        Ok(c) => {
            hub.event("liveops", &format!("{who}: promoted {parts:?} from {from} to {to} (revision {})", c.revision));
            Json(json!({ "ok": true, "channel": c })).into_response()
        }
        Err(why) => refuse(StatusCode::BAD_REQUEST, &why),
    }
}

#[derive(Deserialize)]
struct Upload {
    sha256: String,
}

async fn upload(State(hub): State<Arc<Hub>>, headers: HeaderMap, Path(file): Path<String>, Query(u): Query<Upload>, body: Body) -> Response {
    let who = match authorize_write(&hub, &headers) {
        Ok(who) => who,
        Err(no) => return no,
    };
    let s = match service(&hub) {
        Ok(s) => s,
        Err(r) => return r,
    };
    // Written to disk and hashed as it arrives, never held whole.
    let (tmp, got, size) = match receive(s, body).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let s = s.clone();
    let result = tokio::task::spawn_blocking(move || s.put_blob_file(&file, &u.sha256, &tmp, &got, size, now())).await;
    match result {
        Ok(Ok(blob)) => {
            hub.event("liveops", &format!("{who}: uploaded {} ({} bytes)", blob.file, blob.size));
            Json(json!({ "ok": true, "blob": blob })).into_response()
        }
        Ok(Err(why)) => refuse(StatusCode::BAD_REQUEST, &why),
        Err(e) => refuse(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// An upload's body into a file beside the archives: (where, its SHA-256,
/// its size). Past `MAX_ARCHIVE_BYTES` it is refused and the file removed.
async fn receive(s: &Service, body: Body) -> Result<(std::path::PathBuf, String, u64), Response> {
    use futures_util::StreamExt;
    use sha2::Digest;
    use tokio::io::AsyncWriteExt;
    let tmp = s.upload_path();
    let fail = |status, why: String, tmp: &std::path::Path| {
        let _ = std::fs::remove_file(tmp);
        refuse(status, &why)
    };
    let mut out = match tokio::fs::File::create(&tmp).await {
        Ok(f) => f,
        Err(e) => return Err(fail(StatusCode::INTERNAL_SERVER_ERROR, format!("{}: {e}", tmp.display()), &tmp)),
    };
    let (mut hasher, mut size) = (sha2::Sha256::new(), 0u64);
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => return Err(fail(StatusCode::BAD_REQUEST, format!("the upload broke off: {e}"), &tmp)),
        };
        size += chunk.len() as u64;
        if size > MAX_ARCHIVE_BYTES as u64 {
            return Err(fail(StatusCode::PAYLOAD_TOO_LARGE, format!("an archive is at most {MAX_ARCHIVE_BYTES} bytes"), &tmp));
        }
        hasher.update(&chunk);
        if let Err(e) = out.write_all(&chunk).await {
            return Err(fail(StatusCode::INTERNAL_SERVER_ERROR, format!("writing the upload: {e}"), &tmp));
        }
    }
    if let Err(e) = out.flush().await {
        return Err(fail(StatusCode::INTERNAL_SERVER_ERROR, format!("writing the upload: {e}"), &tmp));
    }
    drop(out);
    Ok((tmp, jengine_liveops::to_hex(&hasher.finalize()), size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behind_a_gateway_the_packs_are_sent_for_under_its_path() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "203.0.113.126:9000".parse().unwrap());
        assert_eq!(reached_at(false, &headers), "http://203.0.113.126:9000");
        headers.insert("x-forwarded-prefix", "/liveops".parse().unwrap());
        assert_eq!(reached_at(false, &headers), "http://203.0.113.126:9000/liveops");
        assert_eq!(jengine_liveops::packs_url(&reached_at(false, &headers)), "http://203.0.113.126:9000/liveops/liveops/v1/packs/");
        headers.insert("x-forwarded-prefix", "//evil.example".parse().unwrap());
        assert_eq!(reached_at(false, &headers), "http://203.0.113.126:9000", "only a plain path is believed");
    }
}
