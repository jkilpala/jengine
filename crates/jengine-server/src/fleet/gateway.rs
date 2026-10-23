//! **The gateway**: one port in front of every server a machine runs, so a
//! player needs one address and the machine one open port.
//!
//! ```text
//!   ws://raspberrypi.local:9000/chess        ─┐            ┌─► 127.0.0.1:9040  chess
//!   ws://raspberrypi.local:9000/arena        ─┼─ gateway ──┼─► 127.0.0.1:9041  arena
//!   http://raspberrypi.local:9000/           ─┘ (lists)    └─► 127.0.0.1:9042  zones
//! ```
//!
//! The first segment of the path names the service; the rest - `/`, `/ws`,
//! `/healthz`, `/admin`, a game's own `/api/..` - is the server's own path,
//! handed over with the prefix taken off. So a server needs no change to sit
//! behind it, and is still reachable on its own port by the machine itself.
//!
//! # How a connection is carried
//!
//! The gateway reads a request's head, picks the server, rewrites the request
//! line, and from then on copies bytes both ways. A WebSocket upgrade is
//! that and nothing more: after the head it is the server's socket. A plain
//! HTTP request is sent on with `Connection: close`, so a browser does not
//! reuse a connection the gateway has already tied to one server for a
//! request meant for another.
//!
//! `/` is the gateway's own: a page listing what runs here and the address to
//! type for each (from the `Host` the browser used, so it is right for
//! whoever reads it), or the same as JSON for anything that does not ask for
//! HTML - which is how a game can find its server on the LAN with one GET.
//!
//! # TLS
//!
//! Given a certificate ([`start_with`], `jengine-fleet host
//! --gateway-tls-cert .. --gateway-tls-key ..`) the gateway speaks only
//! `https://` and `wss://`: a web build served over `https` can reach it, and
//! nothing - a password, the console's token - crosses the network readable.
//! Without one it is plain, which is what a LAN of native clients wants, and
//! a server or console behind it is told so: every request carries an
//! `X-Forwarded-Proto` the gateway writes itself (`https` only over its own
//! TLS, or when a proxy it trusts says the player reached *it* over TLS), and
//! a secret that arrived in the clear is refused there
//! ([`crate::listen::arrived_in_clear`]).
//!
//! # Who a player is
//!
//! A server behind the gateway sees every connection come from this machine,
//! and believes the `X-Forwarded-For` the gateway writes - always one address,
//! never one a client sent (those are dropped). The address is the peer's,
//! unless the peer is a proxy the gateway was told to trust (`--trust-proxy`,
//! [`start_trusting`]; not even this machine is, unless named) - then the one
//! that proxy says it carried the connection for, by
//! [`crate::listen::client_address`]. Without that, every
//! player through jWorld's nginx was one address: nginx's.

use std::collections::BTreeMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

/// The path a host's console is served at (`jengine-fleet host`): a name no
/// service may take there.
pub const CONSOLE: &str = "manage";

/// Longest request head the gateway reads before giving up on it.
const MAX_HEAD: usize = 16 * 1024;
/// How long a client gets to send its request head.
const HEAD_SECS: u64 = 10;

/// What the gateway routes to: the services on this machine.
pub trait Routes: Send + Sync + 'static {
    /// The local port of the service a path's first segment names.
    fn port_of(&self, service: &str) -> Option<u16>;
    /// One row per service for the directory: at least `name`, and `status`
    /// and `players` where known.
    fn listing(&self) -> impl Future<Output = Vec<Value>> + Send;
    /// Where the directory links a service's dashboard: the server's own,
    /// unless a console reaches it with a token somebody knows.
    fn dashboard(&self, service: &str) -> String {
        format!("/{service}/admin")
    }
}

/// A fixed table, for a gateway in front of servers it does not run
/// (`jengine-fleet gateway --routes name=port,..`).
pub struct StaticRoutes(pub BTreeMap<String, u16>);

impl StaticRoutes {
    /// `chess=9040,arena=9041`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut routes = BTreeMap::new();
        for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (name, port) = item.split_once('=').ok_or_else(|| format!("'{item}' is not name=port"))?;
            if !super::safe_name(name) {
                return Err(format!("'{name}' is not a service name (letters, digits, '.', '_', '-')"));
            }
            let port = port.parse().map_err(|_| format!("'{port}' is not a port"))?;
            routes.insert(name.to_string(), port);
        }
        if routes.is_empty() {
            return Err("no routes given".into());
        }
        Ok(StaticRoutes(routes))
    }
}

impl Routes for StaticRoutes {
    fn port_of(&self, service: &str) -> Option<u16> {
        self.0.get(service).copied()
    }

    async fn listing(&self) -> Vec<Value> {
        let mut rows = Vec::new();
        for (name, port) in &self.0 {
            let status = match super::local("GET", *port, "/healthz", None).await {
                Some((200, _)) => "running",
                Some(_) => "draining",
                None => "down",
            };
            rows.push(json!({ "name": name, "status": status }));
        }
        rows
    }
}

/// Run a gateway on `listener` until `stop` changes, believing the proxies
/// in `trusted` about who they carry connections for.
pub async fn serve<R: Routes>(
    listener: TcpListener,
    routes: Arc<R>,
    trusted: Arc<[IpAddr]>,
    stop: watch::Receiver<bool>,
) {
    serve_tls(listener, routes, trusted, None, stop).await
}

/// [`serve`], over TLS when given a certificate: `https://` and `wss://`
/// only, the handshake made before a byte of the request is read.
pub async fn serve_tls<R: Routes>(
    listener: TcpListener,
    routes: Arc<R>,
    trusted: Arc<[IpAddr]>,
    tls: Option<crate::listen::Tls>,
    mut stop: watch::Receiver<bool>,
) {
    let acceptor = match &tls {
        Some(tls) => {
            crate::listen::crypto();
            match axum_server::tls_rustls::RustlsConfig::from_pem_file(&tls.cert, &tls.key).await {
                Ok(config) => Some(axum_server::tls_rustls::RustlsAcceptor::new(config)),
                Err(e) => {
                    // Never fall back to plain: a gateway told to be secure
                    // that is not would be the worse surprise.
                    log::error!("[gateway] could not use {} and {}: {e}; not listening", tls.cert.display(), tls.key.display());
                    return;
                }
            }
        }
        None => None,
    };
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((socket, peer)) = accepted else { continue };
                let _ = socket.set_nodelay(true);
                let (routes, trusted, acceptor) = (routes.clone(), trusted.clone(), acceptor.clone());
                tokio::spawn(async move {
                    let carried = match acceptor {
                        Some(acceptor) => {
                            use axum_server::accept::Accept;
                            match acceptor.accept(socket, ()).await {
                                Ok((socket, ())) => carry(socket, peer, routes, &trusted, true).await,
                                Err(e) => Err(format!("no TLS handshake: {e}")),
                            }
                        }
                        None => carry(socket, peer, routes, &trusted, false).await,
                    };
                    if let Err(why) = carried {
                        log::debug!("[gateway] {peer}: {why}");
                    }
                });
            }
            _ = stop.changed() => break,
        }
    }
}

/// Run a gateway on a thread of its own - for one in front of servers it does
/// not run, and for tests.
pub fn start<R: Routes>(addr: &str, routes: R) -> std::io::Result<RunningGateway> {
    start_trusting(addr, routes, Vec::new())
}

/// [`start`], believing these proxies about who they carry connections for.
pub fn start_trusting<R: Routes>(addr: &str, routes: R, trusted: Vec<IpAddr>) -> std::io::Result<RunningGateway> {
    start_with(addr, routes, trusted, None)
}

/// [`start_trusting`], over TLS when given a certificate.
pub fn start_with<R: Routes>(
    addr: &str,
    routes: R,
    trusted: Vec<IpAddr>,
    tls: Option<crate::listen::Tls>,
) -> std::io::Result<RunningGateway> {
    if let Some(tls) = &tls {
        // Looked for now, so a missing file is an error here, not a log line later.
        for path in [&tls.cert, &tls.key] {
            std::fs::metadata(path).map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?;
        }
    }
    let secure = tls.is_some();
    let listener = std::net::TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let (shutdown, stop) = watch::channel(false);
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let thread = std::thread::spawn(move || {
        runtime.block_on(async move {
            match TcpListener::from_std(listener) {
                Ok(listener) => serve_tls(listener, Arc::new(routes), trusted.into(), tls, stop).await,
                Err(e) => log::error!("[gateway] {e}"),
            }
        });
    });
    Ok(RunningGateway { addr, secure, shutdown, thread: Some(thread) })
}

/// A gateway started with [`start`].
pub struct RunningGateway {
    pub addr: SocketAddr,
    /// Over TLS: `https://` and `wss://`.
    pub secure: bool,
    shutdown: watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RunningGateway {
    /// `ws://` or `wss://` and the address.
    pub fn url(&self) -> String {
        format!("{}://{}", if self.secure { "wss" } else { "ws" }, self.addr)
    }

    pub fn stop(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for RunningGateway {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

/// A request head, as far as the gateway needs to read it.
struct Head {
    method: String,
    target: String,
    version: String,
    headers: Vec<(String, String)>,
}

impl Head {
    fn parse(bytes: &[u8]) -> Option<Head> {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut lines = text.split("\r\n");
        let mut first = lines.next()?.split(' ');
        let (method, target, version) = (first.next()?, first.next()?, first.next()?);
        if first.next().is_some() || !version.starts_with("HTTP/1.") || !target.starts_with('/') {
            return None;
        }
        let headers = lines
            .filter(|l| !l.is_empty())
            .map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
            .collect::<Option<Vec<_>>>()?;
        Some(Head { method: method.into(), target: target.into(), version: version.into(), headers })
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    fn upgrade(&self) -> bool {
        self.header("upgrade").is_some()
    }
}

/// `/chess/ws?x=1` -> (`chess`, `/ws?x=1`); `/` -> (``, `/`).
pub fn split_target(target: &str) -> (&str, String) {
    let rest = &target[1..];
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (service, tail) = rest.split_at(end);
    let tail = if tail.starts_with('/') { tail.to_string() } else { format!("/{tail}") };
    (service, tail)
}

/// Any byte stream the gateway carries: a TCP socket, or TLS over one.
trait Socket: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Socket for S {}

async fn respond(socket: &mut impl Socket, status: &str, kind: &str, body: &str) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        body.len()
    );
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(body.as_bytes()).await?;
    socket.shutdown().await
}

async fn carry<R: Routes>(
    mut socket: impl Socket,
    peer: SocketAddr,
    routes: Arc<R>,
    trusted: &[IpAddr],
    secure: bool,
) -> Result<(), String> {
    // The head, and whatever of the body came with it.
    let mut buf = Vec::with_capacity(1024);
    let end = tokio::time::timeout(Duration::from_secs(HEAD_SECS), async {
        let mut chunk = [0u8; 2048];
        loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                return Ok(i);
            }
            if buf.len() > MAX_HEAD {
                return Err("request head too long".to_string());
            }
            let n = socket.read(&mut chunk).await.map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("closed before a request".into());
            }
            buf.extend_from_slice(&chunk[..n]);
        }
    })
    .await
    .map_err(|_| "no request head in time".to_string())??;
    let Some(head) = Head::parse(&buf[..end]) else {
        let _ = respond(&mut socket, "400 Bad Request", "text/plain", "not an HTTP/1 request\n").await;
        return Err("bad request head".into());
    };
    let (service, tail) = split_target(&head.target);

    if service.is_empty() || service == "favicon.ico" {
        return directory(&mut socket, &head, &*routes, secure).await.map_err(|e| e.to_string());
    }
    let Some(port) = routes.port_of(service) else {
        let body = format!("no service named '{service}' here - see / for what is\n");
        let _ = respond(&mut socket, "404 Not Found", "text/plain", &body).await;
        return Err(format!("no service {service}"));
    };
    let connect = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(("127.0.0.1", port))).await;
    let Ok(Ok(mut server)) = connect else {
        let body = format!("'{service}' is not answering (port {port}) - is it running?\n");
        let _ = respond(&mut socket, "502 Bad Gateway", "text/plain", &body).await;
        return Err(format!("{service} on {port} did not answer"));
    };
    let _ = server.set_nodelay(true);

    // Who it is for: the peer, or whoever a trusted proxy says.
    let client = crate::listen::client_address(
        peer.ip(),
        head.header("x-forwarded-for"),
        head.header("x-real-ip"),
        |ip| trusted.contains(&ip),
    );
    // How it reached us: over our own TLS, or over a trusted proxy's that
    // says so. Never as anybody else says - a header is easy to write.
    let proto = if secure
        || (trusted.contains(&peer.ip())
            && head.header("x-forwarded-proto").is_some_and(|p| p.trim().eq_ignore_ascii_case("https")))
    {
        "https"
    } else {
        "http"
    };
    // The same request, for the server's own path.
    let upgrade = head.upgrade();
    let mut out = format!("{} {} {}\r\n", head.method, tail, head.version);
    for (k, v) in &head.headers {
        let drop = k.eq_ignore_ascii_case("x-forwarded-for")
            || k.eq_ignore_ascii_case("x-real-ip")
            || k.eq_ignore_ascii_case("x-forwarded-prefix")
            || k.eq_ignore_ascii_case("x-forwarded-proto")
            || (!upgrade && (k.eq_ignore_ascii_case("connection") || k.eq_ignore_ascii_case("keep-alive")));
        if !drop {
            out.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    if !upgrade {
        out.push_str("Connection: close\r\n");
    }
    out.push_str(&format!(
        "X-Forwarded-For: {client}\r\nX-Forwarded-Proto: {proto}\r\nX-Forwarded-Prefix: /{service}\r\n\r\n"
    ));
    server.write_all(out.as_bytes()).await.map_err(|e| e.to_string())?;
    server.write_all(&buf[end + 4..]).await.map_err(|e| e.to_string())?;
    tokio::io::copy_bidirectional(&mut socket, &mut server).await.map_err(|e| e.to_string())?;
    Ok(())
}

/// `/`: what runs here, as a page or as JSON.
async fn directory<R: Routes>(socket: &mut impl Socket, head: &Head, routes: &R, secure: bool) -> std::io::Result<()> {
    let host = head.header("host").unwrap_or("localhost").to_string();
    let (ws, http) = if secure { ("wss", "https") } else { ("ws", "http") };
    let services: Vec<Value> = routes
        .listing()
        .await
        .into_iter()
        .map(|row| {
            let name = row["name"].as_str().unwrap_or("").to_string();
            json!({
                "name": name,
                "status": row["status"],
                "players": row["players"],
                "path": format!("/{name}"),
                "url": format!("{ws}://{host}/{name}"),
                "dashboard": routes.dashboard(&name),
            })
        })
        .collect();
    let html = head.header("accept").is_some_and(|a| a.contains("text/html")) && !head.target.contains("json");
    let console = routes.port_of(CONSOLE).is_some();
    if !html {
        let console = console.then(|| format!("{http}://{host}/{CONSOLE}"));
        let body = json!({ "host": host, "services": services, "console": console }).to_string();
        return respond(socket, "200 OK", "application/json", &body).await;
    }
    respond(socket, "200 OK", "text/html; charset=utf-8", &page(&host, &services, console)).await
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn page(host: &str, services: &[Value], console: bool) -> String {
    let manage = if console {
        format!("<p class=\"manage\"><a href=\"/{CONSOLE}\">Manage these servers →</a></p>")
    } else {
        String::new()
    };
    let mut rows = String::new();
    for s in services {
        let name = escape(s["name"].as_str().unwrap_or(""));
        let status = escape(s["status"].as_str().unwrap_or("?"));
        let players = s["players"].as_u64().map(|p| p.to_string()).unwrap_or_else(|| "-".into());
        let url = escape(s["url"].as_str().unwrap_or(""));
        let dashboard = escape(s["dashboard"].as_str().unwrap_or(""));
        rows.push_str(&format!(
            "<tr><td>{name}</td><td class=\"{status}\">{status}</td><td>{players}</td>\
             <td><code>{url}</code></td><td><a href=\"{dashboard}\">dashboard</a></td></tr>"
        ));
    }
    if rows.is_empty() {
        rows.push_str("<tr><td colspan=\"5\">Nothing is running here yet.</td></tr>");
    }
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Game servers</title><meta http-equiv="refresh" content="5">
<style>
:root{{--bg:#f6f6f4;--fg:#1d1d1b;--dim:#6b6b66;--line:#ddd;--ok:#2f7d32;--bad:#b3261e}}
@media (prefers-color-scheme:dark){{:root{{--bg:#161615;--fg:#ececea;--dim:#9a9a94;--line:#333;--ok:#7cc47f;--bad:#f28b82}}}}
body{{background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif;margin:0;padding:24px 16px}}
main{{max-width:880px;margin:auto}} h1{{font-size:20px;margin:0 0 4px}} p{{color:var(--dim);margin:0 0 20px}}
table{{width:100%;border-collapse:collapse}} th,td{{text-align:left;padding:8px 6px;border-bottom:1px solid var(--line)}}
th{{color:var(--dim);font-weight:500;font-size:13px}} code{{font-size:13px;word-break:break-all}}
.running{{color:var(--ok)}} .down,.crashed,.stopped{{color:var(--bad)}} a{{color:inherit}} .manage{{margin-top:20px}}
</style></head><body><main>
<h1>Game servers on {host}</h1>
<p>Type the address into a game's server field. The page refreshes itself; <a href="/?json">as JSON</a>.</p>
<table><tr><th>Service</th><th>Status</th><th>Players</th><th>Address</th><th></th></tr>{rows}</table>
{manage}
</main></body></html>"#,
        host = escape(host)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_segment_names_the_service_and_the_rest_is_the_servers() {
        assert_eq!(split_target("/chess"), ("chess", "/".to_string()));
        assert_eq!(split_target("/chess/"), ("chess", "/".to_string()));
        assert_eq!(split_target("/chess/ws"), ("chess", "/ws".to_string()));
        assert_eq!(split_target("/chess?x=1"), ("chess", "/?x=1".to_string()));
        assert_eq!(split_target("/chess/admin/api/x?y"), ("chess", "/admin/api/x?y".to_string()));
        assert_eq!(split_target("/"), ("", "/".to_string()));
        assert_eq!(split_target("/?json"), ("", "/?json".to_string()));
    }

    #[test]
    fn routes_are_parsed_and_bad_names_refused() {
        let r = StaticRoutes::parse("chess-club=9040, arena=9041").unwrap();
        assert_eq!(r.port_of("arena"), Some(9041));
        assert!(StaticRoutes::parse("../x=1").is_err());
        assert!(StaticRoutes::parse("a=port").is_err());
        assert!(StaticRoutes::parse("").is_err());
    }

    #[test]
    fn a_head_is_read_or_refused() {
        let h = Head::parse(b"GET /a/ws HTTP/1.1\r\nHost: pi:9000\r\nUpgrade: websocket").unwrap();
        assert_eq!((h.method.as_str(), h.target.as_str()), ("GET", "/a/ws"));
        assert_eq!(h.header("HOST"), Some("pi:9000"));
        assert!(h.upgrade());
        assert!(Head::parse(b"GET http://x/ HTTP/1.1").is_none());
        assert!(Head::parse(b"garbage").is_none());
    }
}
