//! Listening and calling, with or without TLS.
//!
//! Three things in this crate carry a secret that must not be read off the
//! wire: the fleet's **agent** (its token runs programs on the machine), the
//! fleet's **web interface** (its token deploys), and a cluster's
//! **coordinator** (its secret reads and writes every player). Each can be
//! given a certificate, and then speaks HTTPS and nothing else.
//!
//! A certificate from a real authority just works. For machines that only
//! talk to each other, [`self_signed`] (`jengine-fleet cert`) makes one, and
//! the calling side is told to trust exactly that file ([`client`]) - which
//! is pinning: nobody else's certificate will do, whatever it claims.
//!
//! The game socket itself is not here. Players need a certificate their
//! browsers and devices already trust, and the usual way to have one is a
//! proxy in front of the server (see the README).

use std::future::Future;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;

/// A certificate and its key, as PEM files.
#[derive(Debug, Clone)]
pub struct Tls {
    pub cert: PathBuf,
    pub key: PathBuf,
}

impl Tls {
    pub fn new(cert: impl Into<PathBuf>, key: impl Into<PathBuf>) -> Self {
        Tls { cert: cert.into(), key: key.into() }
    }

    /// From a `--tls-cert` / `--tls-key` pair: both, or neither.
    pub fn from_options(cert: Option<&str>, key: Option<&str>) -> Result<Option<Self>, String> {
        match (cert, key) {
            (Some(cert), Some(key)) => Ok(Some(Tls::new(cert, key))),
            (None, None) => Ok(None),
            _ => Err("TLS needs both --tls-cert and --tls-key".into()),
        }
    }
}

/// Who a request is really from: its peer - unless `trusts(peer)` (the peer
/// is a proxy we believe) and it says it carried the request for somebody
/// else. Then the **last** `X-Forwarded-For` entry (the one the proxy added
/// itself; anything before it the client could have written), else
/// `X-Real-IP` (nginx's `$remote_addr`), else the peer after all.
///
/// The one rule for everything that reads an address: the gateway, before it
/// writes the `X-Forwarded-For` a server behind it believes (trusting the
/// proxies it was told of); the game's `/api/` rate limit and the game
/// socket's connections (trusting this machine - the gateway - and
/// `Config::trusted_proxies`). Anybody not trusted could name any address,
/// so their headers are not read at all.
pub fn client_address(
    peer: IpAddr,
    forwarded_for: Option<&str>,
    real_ip: Option<&str>,
    trusts: impl Fn(IpAddr) -> bool,
) -> IpAddr {
    if !trusts(peer) {
        return peer;
    }
    let last = forwarded_for.and_then(|v| v.rsplit(',').next()).and_then(|v| v.trim().parse().ok());
    last.or_else(|| real_ip.and_then(|v| v.trim().parse().ok())).unwrap_or(peer)
}

/// Whether a request crossed a network **unencrypted** on its way here - so
/// that a password or a token in it could have been read off the wire.
///
/// - Over this listener's own TLS (`here_secure`): no.
/// - From this machine, with nobody saying it carried it for anyone else
///   (the editor, the agent asking its own servers, a test): no - it never
///   left the machine.
/// - Through a proxy we believe (`trusts(peer)`: the gateway on this
///   machine, a TLS proxy named with `--trust-proxy`), for a client on
///   another machine: whatever the proxy says in `X-Forwarded-Proto` -
///   `https` is encrypted, anything else (or nothing) is not.
/// - From anywhere else, plain: yes.
///
/// The one rule for every surface that takes a secret: a password `hello`,
/// the admin API, private `/stats` and `/metrics`, the console.
pub fn arrived_in_clear(
    peer: IpAddr,
    forwarded_for: Option<&str>,
    real_ip: Option<&str>,
    forwarded_proto: Option<&str>,
    here_secure: bool,
    trusts: impl Fn(IpAddr) -> bool,
) -> bool {
    if here_secure {
        return false;
    }
    if !trusts(peer) {
        return !peer.is_loopback();
    }
    let client = client_address(peer, forwarded_for, real_ip, &trusts);
    if client.is_loopback() {
        return false;
    }
    !forwarded_proto.is_some_and(|p| p.trim().eq_ignore_ascii_case("https"))
}

/// [`arrived_in_clear`] read from an axum request's headers.
pub fn headers_in_clear(
    peer: IpAddr,
    headers: &axum::http::HeaderMap,
    here_secure: bool,
    trusts: impl Fn(IpAddr) -> bool,
) -> bool {
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    arrived_in_clear(peer, text("x-forwarded-for"), text("x-real-ip"), text("x-forwarded-proto"), here_secure, trusts)
}

/// What a surface that takes a secret answers a request that came in the clear.
pub const NEEDS_TLS: &str = "this takes a secret only over an encrypted connection (https / wss), and this request crossed the network in plain text";

/// Addresses from a `--trust-proxy` list: `198.51.100.90,10.0.0.2`.
pub fn parse_addresses(list: &str) -> Result<Vec<IpAddr>, String> {
    list.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().map_err(|_| format!("'{s}' is not an address")))
        .collect()
}

/// rustls needs to be told which cryptography to use, once per process.
pub(crate) fn crypto() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Serve `router` on a listener that is already bound, until `stop` resolves.
pub(crate) async fn serve(
    listener: std::net::TcpListener,
    router: Router,
    tls: Option<Tls>,
    stop: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    // With the peer's address, which the game's `/api/` rate limit reads.
    let service = router.into_make_service_with_connect_info::<std::net::SocketAddr>();
    let Some(tls) = tls else {
        use axum::serve::ListenerExt;
        // No Nagle: a game's frames are small and wanted now. With it, a
        // reply could wait on the client's delayed ACK - the LAN Pi showed
        // +50 ms on some of a village game's actions and none locally (Oct
        // 2026). The TLS path (`axum-server`) already sets it.
        let listener = tokio::net::TcpListener::from_std(listener)?.tap_io(|tcp| {
            if let Err(e) = tcp.set_nodelay(true) {
                log::debug!("TCP_NODELAY not set on a connection: {e}");
            }
        });
        return axum::serve(listener, service).with_graceful_shutdown(stop).await;
    };
    crypto();
    let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&tls.cert, &tls.key).await.map_err(|e| {
        std::io::Error::other(format!("could not use {} and {}: {e}", tls.cert.display(), tls.key.display()))
    })?;
    let handle = axum_server::Handle::new();
    let stopper = handle.clone();
    tokio::spawn(async move {
        stop.await;
        stopper.graceful_shutdown(Some(Duration::from_secs(5)));
    });
    axum_server::from_tcp_rustls(listener, config).handle(handle).serve(service).await
}

/// An HTTP client that trusts the usual authorities and, if given, the
/// certificates in one more PEM file - a self-signed one, pinned.
pub fn client(ca: Option<&Path>) -> Result<ureq::Agent, String> {
    crypto();
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = ca {
        let pem = std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        let mut found = 0;
        for cert in rustls_pemfile::certs(&mut pem.as_slice()) {
            let cert = cert.map_err(|e| format!("{} is not a certificate: {e}", path.display()))?;
            roots.add(cert).map_err(|e| format!("{} is not a usable certificate: {e}", path.display()))?;
            found += 1;
        }
        if found == 0 {
            return Err(format!("{} holds no certificate", path.display()));
        }
    }
    let config = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
    Ok(ureq::AgentBuilder::new().tls_config(Arc::new(config)).build())
}

/// A self-signed certificate for these host names and addresses, as
/// (certificate, key) in PEM. Good for ten years, and for nothing but being
/// pinned: no browser will trust it, and no stranger should.
pub fn self_signed(hosts: &[String]) -> Result<(String, String), String> {
    if hosts.is_empty() {
        return Err("a certificate needs at least one host name or address".into());
    }
    let made = rcgen::generate_simple_self_signed(hosts.to_vec()).map_err(|e| e.to_string())?;
    Ok((made.cert.pem(), made.key_pair.serialize_pem()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_made_certificate_is_one_a_client_can_be_told_to_trust() {
        let (cert, key) = self_signed(&["127.0.0.1".into(), "fleet.internal".into()]).unwrap();
        assert!(cert.starts_with("-----BEGIN CERTIFICATE-----") && key.contains("PRIVATE KEY"));
        let dir = std::env::temp_dir().join(format!("jengine-listen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cert.pem"), &cert).unwrap();
        assert!(client(Some(&dir.join("cert.pem"))).is_ok());
        std::fs::write(dir.join("empty.pem"), "not a certificate").unwrap();
        assert!(client(Some(&dir.join("empty.pem"))).unwrap_err().contains("holds no certificate"));
        assert!(client(Some(&dir.join("missing.pem"))).is_err());
        assert!(self_signed(&[]).is_err());
        assert!(Tls::from_options(Some("c"), None).is_err(), "half a pair is a mistake, not plain HTTP");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_crossed_a_network_in_plain_text_is_told_apart() {
        let local: IpAddr = "127.0.0.1".parse().unwrap();
        let far: IpAddr = "203.0.113.9".parse().unwrap();
        let proxy: IpAddr = "198.51.100.90".parse().unwrap();
        let loopback_or = |extra: IpAddr| move |ip: IpAddr| ip.is_loopback() || ip == extra;
        // Over our own TLS, never.
        assert!(!arrived_in_clear(far, None, None, None, true, loopback_or(proxy)));
        // From this machine, carried for nobody: it never left it.
        assert!(!arrived_in_clear(local, None, None, None, false, loopback_or(proxy)));
        // Straight from another machine, plain.
        assert!(arrived_in_clear(far, None, None, None, false, loopback_or(proxy)));
        // Through the gateway on this machine, for somebody on the LAN: as
        // the gateway says - http is in the clear, https is not.
        assert!(arrived_in_clear(local, Some("203.0.113.9"), None, None, false, loopback_or(proxy)));
        assert!(arrived_in_clear(local, Some("203.0.113.9"), None, Some("http"), false, loopback_or(proxy)));
        assert!(!arrived_in_clear(local, Some("203.0.113.9"), None, Some("https"), false, loopback_or(proxy)));
        // The gateway carrying a browser on this very machine.
        assert!(!arrived_in_clear(local, Some("127.0.0.1"), None, Some("http"), false, loopback_or(proxy)));
        // A named TLS proxy is believed; a stranger saying "https" is not.
        assert!(!arrived_in_clear(proxy, None, Some("203.0.113.9"), Some("https"), false, loopback_or(proxy)));
        assert!(arrived_in_clear(far, None, None, Some("https"), false, loopback_or(proxy)));
    }
}
