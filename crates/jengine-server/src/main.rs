//! `jengine-server [host:port] [--db <file>] [--admin-token <secret>]
//! [--public-stats] [--allow-plain-passwords] [--admin-over-plain]
//! [--trust-proxy <addr>,..] [--liveops <dir> [--liveops-cdn <url>]]` - the stock
//! server: relay rooms, lockstep rooms, and with `--liveops` a live ops service (remote
//! config, experiments, events and content releases; the dashboard at
//! `/admin/liveops`) whose signing key and pack archives are kept in `<dir>`
//! - and, with no `--db`, its channels too (`<dir>/liveops.db`).
//!
//! `jengine-server coordinator [host:port] --secret <s> [--db <file>]
//! [--tls-cert <pem> --tls-key <pem>] [--node-ca <pem>]` runs a
//! cluster's coordinator instead (default port 9100), and a server started
//! with `JENGINE_CLUSTER` set is one of that cluster's nodes - see
//! `jengine_server::cluster`.
//!
//! `jengine-server liveops-key new <file>` makes a live ops signing key (a
//! *root* to build games with, kept off the serving machine), and
//! `jengine-server liveops-key endorse --root <file> (--dir <liveops dir> |
//! --public <key> --out <file>) [--days 90]` has it endorse the service's own
//! key - see `jengine_server::liveops`, "Replacing the key".
//!
//! `jengine-server node-key <node> [--secret <s>]` prints the key a cluster's
//! node can be given in place of the cluster's secret
//! (`JENGINE_CLUSTER_NODE_KEY`): it can sign as that node and as nothing else.
//!
//! `jengine-server new <dir> [--template relay|match|world] [--name <crate>]
//! [--engine <path> | --git <url> [--rev <sha> | --tag <t> | --branch <b>] |
//! --release]` writes a game's own server crate to start
//! from - see `jengine_server::scaffold`.
//!
//! This is the server a turn-based game needs and nothing more; a game with
//! rules of its own builds its own binary on the library (see
//! `examples/zones`). Defaults to `127.0.0.1:9001`, where `jengine-relay`
//! listens, so the arcade's online games connect to either.

use jengine_server::Server;

#[cfg(feature = "sqlite")]
fn with_db(server: Server, path: &str) -> Server {
    match jengine_server::SqliteStore::open(path) {
        Ok(store) => server.store(std::sync::Arc::new(store)),
        Err(e) => {
            eprintln!("jengine-server: could not open {path}: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(feature = "sqlite"))]
fn with_db(_: Server, _: &str) -> Server {
    eprintln!("jengine-server: built without the sqlite feature, so there is no --db");
    std::process::exit(1);
}

/// `jengine-server coordinator [host:port] --secret <s> [--db <file>]`
fn coordinator() {
    let (mut addr, mut db, mut secret) = ("127.0.0.1:9100".to_string(), None, std::env::var("JENGINE_CLUSTER_SECRET").unwrap_or_default());
    let (mut cert, mut key) = (None, None);
    let mut node_ca = std::env::var("JENGINE_CLUSTER_NODE_CA").ok();
    let mut args = std::env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = args.next(),
            "--secret" => secret = args.next().unwrap_or_default(),
            "--tls-cert" => cert = args.next(),
            "--tls-key" => key = args.next(),
            "--node-ca" => node_ca = args.next(),
            _ => addr = arg,
        }
    }
    let store: std::sync::Arc<dyn jengine_server::Store> = match db {
        #[cfg(feature = "sqlite")]
        Some(path) => match jengine_server::SqliteStore::open(&path) {
            Ok(store) => std::sync::Arc::new(store),
            Err(e) => {
                eprintln!("jengine-server: could not open {path}: {e}");
                std::process::exit(1);
            }
        },
        _ => {
            eprintln!("jengine-server: no --db, so the cluster's players are kept in memory only");
            std::sync::Arc::new(jengine_server::MemoryStore::new())
        }
    };
    let tls = jengine_server::Tls::from_options(cert.as_deref(), key.as_deref()).unwrap_or_else(|why| {
        eprintln!("jengine-server: {why}");
        std::process::exit(1);
    });
    let node_ca = node_ca.map(std::path::PathBuf::from);
    if let Err(e) = jengine_server::cluster::run_coordinator_trusting(&addr, store, &secret, tls, node_ca.as_deref()) {
        eprintln!("jengine-server: coordinator: {e}");
        std::process::exit(1);
    }
}

/// `jengine-server new <dir> [--template ..] [--name ..] [--engine <path> | --git <url>]`
fn new_server() {
    use jengine_server::scaffold::{Engine, Options, Pin, Template, generate};
    let (mut dir, mut name, mut template, mut engine) = (None, None, Template::Match, None);
    let mut pin: Option<Pin> = None;
    let mut args = std::env::args().skip(2);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--name" => name = args.next(),
            "--engine" => engine = args.next().map(Engine::Path),
            "--git" => engine = args.next().map(Engine::Git),
            "--rev" => pin = args.next().map(Pin::Rev),
            "--tag" => pin = args.next().map(Pin::Tag),
            "--branch" => pin = args.next().map(Pin::Branch),
            // The published engine, at this generator's own version.
            "--release" => engine = Some(Engine::released()),
            "--template" => {
                let asked = args.next().unwrap_or_default();
                template = Template::parse(&asked).unwrap_or_else(|| {
                    eprintln!("jengine-server: no template '{asked}': relay, match or world");
                    std::process::exit(1);
                });
            }
            _ => dir = Some(std::path::PathBuf::from(arg)),
        }
    }
    // A pin makes a plain `--git` one revision for everybody.
    if let (Some(pin), Some(Engine::Git(url))) = (pin.clone(), engine.clone()) {
        engine = Some(Engine::GitAt { url, pin });
    } else if pin.is_some() && !matches!(engine, Some(Engine::GitAt { .. })) {
        eprintln!("jengine-server: --rev, --tag and --branch go with --git <url>");
        std::process::exit(1);
    }
    let Some(dir) = dir else {
        eprintln!("usage: jengine-server new <dir> [--template relay|match|world] [--name <crate>] [--engine <path> | --git <url> [--rev <sha> | --tag <t>] | --release]");
        std::process::exit(1);
    };
    match generate(&Options { dir: dir.clone(), name, template, engine }) {
        Ok(files) => {
            for file in &files {
                println!("  wrote {}", file.display());
            }
            println!("
A {} server is in {}. Next:
  cd {}
  cargo test
  cargo run", template.name(), dir.display(), dir.display());
        }
        Err(why) => {
            eprintln!("jengine-server: {why}");
            std::process::exit(1);
        }
    }
}

/// `jengine-server liveops-key new <file>` / `liveops-key endorse ..`
fn liveops_key() {
    let fail = |why: &str| -> ! {
        eprintln!("jengine-server liveops-key: {why}");
        std::process::exit(2);
    };
    let args: Vec<String> = std::env::args().skip(2).collect();
    let option = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    match args.first().map(String::as_str) {
        Some("new") => {
            let Some(path) = args.get(1) else { fail("usage: liveops-key new <file>") };
            if std::path::Path::new(path).exists() {
                fail(&format!("{path} exists; a key is never written over"));
            }
            let key = jengine_server::liveops::new_key();
            if let Err(e) = std::fs::write(path, key.to_text()) {
                fail(&format!("{path}: {e}"));
            }
            println!("wrote {path} - keep it secret, and off the serving machine if it is a root");
            println!("build games with settings.liveops.public_key = {}", key.public().to_text());
        }
        Some("endorse") => {
            let root_path = option("--root").unwrap_or_else(|| fail("endorse needs --root <the key games trust>"));
            let root = std::fs::read_to_string(&root_path)
                .map_err(|e| e.to_string())
                .and_then(|t| jengine_liveops::SecretKey::parse(&t).map_err(|e| e.to_string()))
                .unwrap_or_else(|why| fail(&format!("{root_path}: {why}")));
            let days = option("--days").map(|d| d.parse::<f64>().unwrap_or_else(|_| fail("--days is a number"))).unwrap_or(90.0);
            // The key to endorse: the service's own (from its folder), or named.
            let (key, out) = match (option("--dir"), option("--public")) {
                (Some(dir), None) => {
                    let ops = jengine_server::LiveOps::open(&dir).unwrap_or_else(|why| fail(&why));
                    let key = jengine_liveops::PublicKey::parse(&ops.serving_key()).unwrap_or_else(|e| fail(&e.to_string()));
                    (key, std::path::Path::new(&dir).join(jengine_server::liveops::ENDORSEMENT_FILE))
                }
                (None, Some(text)) => {
                    let key = jengine_liveops::PublicKey::parse(&text).unwrap_or_else(|e| fail(&e.to_string()));
                    let out = option("--out").unwrap_or_else(|| fail("with --public, say where to write it: --out <file>"));
                    (key, std::path::PathBuf::from(out))
                }
                _ => fail("endorse needs --dir <live ops folder> or --public <key> --out <file>"),
            };
            let endorsed = jengine_server::liveops::endorse(&root, &key, days);
            let text = serde_json::to_string_pretty(&endorsed).unwrap_or_default();
            if let Err(e) = std::fs::write(&out, text) {
                fail(&format!("{}: {e}", out.display()));
            }
            println!("wrote {}: {} speaks for {} for {days} days", out.display(), key.to_text(), endorsed.root);
            println!("restart the service to sign with it; games built with {} believe it", endorsed.root);
        }
        _ => fail("usage: liveops-key new <file> | liveops-key endorse --root <file> (--dir <dir> | --public <key> --out <file>) [--days 90]"),
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("new") {
        return new_server();
    }
    if std::env::args().nth(1).as_deref() == Some("liveops-key") {
        return liveops_key();
    }
    // `jengine-server node-key <node> [--secret s]`: the key a node of a
    // cluster can be given in place of the cluster's secret
    // (`JENGINE_CLUSTER_NODE_KEY`) - it signs as that node and nothing else.
    if std::env::args().nth(1).as_deref() == Some("node-key") {
        let args: Vec<String> = std::env::args().skip(2).collect();
        let secret = args
            .iter()
            .position(|a| a == "--secret")
            .and_then(|i| args.get(i + 1).cloned())
            .or_else(|| std::env::var("JENGINE_CLUSTER_SECRET").ok())
            .unwrap_or_default();
        let Some(node) = args.iter().find(|a| !a.starts_with("--") && **a != secret) else {
            eprintln!("usage: jengine-server node-key <node> [--secret <the cluster secret>]");
            std::process::exit(2);
        };
        if secret.len() < 8 {
            eprintln!("jengine-server node-key: give the cluster's secret (--secret or JENGINE_CLUSTER_SECRET)");
            std::process::exit(2);
        }
        println!("{}", jengine_server::cluster::node_key_hex(&secret, node));
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("coordinator") {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
        return coordinator();
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut addr = "127.0.0.1:9001".to_string();
    let mut db: Option<String> = None;
    // The dashboard at /admin exists only when there is a token to guard it.
    let mut admin = std::env::var("JENGINE_ADMIN_TOKEN").unwrap_or_default();
    // `/stats` and `/metrics` want the admin token unless asked otherwise;
    // `--private-stats` (the old way to ask for that) is still taken.
    let (mut public_stats, mut plain_passwords, mut plain_admin) = (false, false, false);
    let mut trust: Vec<std::net::IpAddr> = Vec::new();
    let (mut liveops, mut liveops_cdn): (Option<String>, Option<String>) = (None, None);
    let (mut tls_cert, mut tls_key) = (std::env::var("JENGINE_TLS_CERT").ok(), std::env::var("JENGINE_TLS_KEY").ok());
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = args.next(),
            "--admin-token" => admin = args.next().unwrap_or_default(),
            "--private-stats" => public_stats = false,
            "--public-stats" => public_stats = true,
            "--allow-plain-passwords" => plain_passwords = true,
            "--admin-over-plain" => plain_admin = true,
            "--trust-proxy" => match jengine_server::listen::parse_addresses(&args.next().unwrap_or_default()) {
                Ok(list) => trust.extend(list),
                Err(why) => {
                    eprintln!("jengine-server: --trust-proxy: {why}");
                    std::process::exit(2);
                }
            },
            "--liveops" => liveops = args.next(),
            "--liveops-cdn" => liveops_cdn = args.next(),
            "--tls-cert" => tls_cert = args.next(),
            "--tls-key" => tls_key = args.next(),
            _ => addr = arg,
        }
    }
    let mut server = Server::new().relay().lockstep().admin_token(&admin).trust_proxies(trust);
    if public_stats {
        server = server.public_stats();
    }
    if plain_passwords {
        server = server.allow_plain_passwords();
    }
    if plain_admin {
        server = server.admin_over_plain();
    }
    match jengine_server::Tls::from_options(tls_cert.as_deref(), tls_key.as_deref()) {
        Ok(Some(tls)) => server = server.tls(tls),
        Ok(None) => {}
        Err(why) => {
            eprintln!("jengine-server: {why}");
            std::process::exit(2);
        }
    }
    // Live ops' channels are what every shipped game is told, so they are
    // kept even when the operator forgot `--db`.
    #[cfg(feature = "sqlite")]
    if db.is_none()
        && let Some(dir) = &liveops
    {
        let _ = std::fs::create_dir_all(dir);
        db = Some(std::path::Path::new(dir).join("liveops.db").to_string_lossy().into_owned());
    }
    if let Some(path) = db {
        server = with_db(server, &path);
    }
    if let Some(dir) = liveops {
        match jengine_server::LiveOps::open(&dir) {
            Ok(ops) => {
                let ops = match &liveops_cdn {
                    Some(url) => ops.cdn_url(url),
                    None => ops,
                };
                eprintln!("jengine-server: live ops in {dir}; build games with settings.liveops.public_key = {}", ops.public_key());
                server = server.liveops(ops);
            }
            Err(why) => {
                eprintln!("jengine-server: live ops: {why}");
                std::process::exit(2);
            }
        }
    }
    // After the store: a node's store is the coordinator's.
    if let Some(cluster) = jengine_server::ClusterConfig::from_env() {
        eprintln!("jengine-server: node '{}' of the cluster at {}", cluster.node, cluster.coordinator);
        server = server.cluster(cluster);
    }
    if let Err(e) = server.run(&addr) {
        eprintln!("jengine-server: could not listen on {addr}: {e}");
        std::process::exit(1);
    }
}
