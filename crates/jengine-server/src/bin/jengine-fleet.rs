//! `jengine-fleet` - put servers on machines, and see how they are doing.
//!
//! ```text
//! Once, where builds are made:
//!   jengine-fleet keygen [--out release.key]        a release key: agents run only what it signs
//!   jengine-fleet sign <artifact> [--key release.key]   <artifact>.sig, for a deploy made elsewhere
//!
//! On each machine, once:
//!   jengine-fleet cert <host-or-address>..          a certificate and key for the agent
//!   JENGINE_FLEET_TOKEN=<secret> jengine-fleet agent [--listen 0.0.0.0:9900] [--data ./fleet-data]
//!                                [--tls-cert cert.pem --tls-key key.pem] [--gateway 0.0.0.0:9000]
//!   JENGINE_FLEET_TOKEN=<secret> jengine-fleet host  the agent with its gateway on :9000 and a
//!                                                    console at /manage - a machine (a Raspberry
//!                                                    Pi) hosting game servers
//!   jengine-fleet gateway --routes a=9040,b=9041    only the gateway, before servers run by hand
//!   (a host or a gateway behind a TLS proxy: --trust-proxy <its address>, so
//!   players are told apart by their own addresses, not the proxy's)
//!   jengine-fleet user add|remove|list [<name>] [--role viewer|operator] --users <file>
//!                                                    people with tokens of their own for a console
//!
//! From yours, beside a fleet.json:
//!   jengine-fleet service [the agent's options]    what to give systemd or Task Scheduler
//!   jengine-fleet ui [--listen 127.0.0.1:9800]     the same, in a browser
//!   jengine-fleet status
//!   jengine-fleet deploy <service> [--machine <name>]
//!   jengine-fleet rollback|restart|stop|start|remove <service> [--machine <name>]
//!   jengine-fleet logs <service> --machine <name> [--lines 100] [--grep text]
//!
//!   --fleet <path>   the fleet file, if it is not ./fleet.json
//! ```
//!
//! See `jengine_server::fleet` for what a fleet file holds.

use std::path::PathBuf;

use jengine_server::fleet::client::{self, Fleet};
use jengine_server::fleet::gateway::{self, StaticRoutes};
use jengine_server::fleet::{agent, platform, ui};

const USAGE: &str = "usage:
  jengine-fleet keygen [--out release.key]      a release key; prints the public half for --release-key
  jengine-fleet sign <artifact> [--key release.key]   writes <artifact>.sig
  jengine-fleet cert <host-or-address>.. [--out <dir>]
  jengine-fleet agent [--listen 0.0.0.0:9900] [--data ./fleet-data] [--token <secret>]
                      [--tls-cert cert.pem --tls-key key.pem] [--gateway 0.0.0.0:9000]
                      [--release-key <jfr-public-..> | --allow-unsigned]
  jengine-fleet host [the agent's options] [--console-token <secret>] [--trust-proxy <addr>,..]
                     [--gateway-tls-cert cert.pem --gateway-tls-key key.pem] [--allow-plain-console]
                                                the agent, with its gateway on 0.0.0.0:9000
                                                and a console behind it at /manage
  jengine-fleet gateway --routes name=port,.. [--listen 0.0.0.0:9000] [--trust-proxy <addr>,..]
                        [--tls-cert .. --tls-key ..]
  jengine-fleet service [the agent's options]   what to give systemd or Task Scheduler
  jengine-fleet ui [--listen 127.0.0.1:9800] [--token <secret>] [--tls-cert .. --tls-key ..]
                   [--users users.json] [--allow-plain-console] [--jobs jobs.json]
  jengine-fleet user add <name> [--role viewer|operator] --users <file>   prints their token
  jengine-fleet user remove <name> --users <file>
  jengine-fleet user list --users <file>
  jengine-fleet status
  jengine-fleet deploy <service> [--machine <name>]
  jengine-fleet rollback|restart|stop|start|remove <service> [--machine <name>]
  jengine-fleet logs <service> --machine <name> [--lines 100] [--grep text]
options:
  --fleet <path>   the fleet file (default ./fleet.json)";

fn fail(why: &str) -> ! {
    eprintln!("jengine-fleet: {why}");
    std::process::exit(1);
}

fn ago(seconds: f64) -> String {
    match seconds {
        s if s < 90.0 => format!("{s:.0}s"),
        s if s < 5400.0 => format!("{:.0}m", s / 60.0),
        s if s < 129_600.0 => format!("{:.1}h", s / 3600.0),
        s => format!("{:.1}d", s / 86400.0),
    }
}

fn print_status(fleet: &Fleet) -> bool {
    let mut all_well = true;
    println!(
        "{:<14} {:<16} {:<14} {:<18} {:<9} {:>7} {:>8} {:>8}  NOTE",
        "MACHINE", "PLATFORM", "SERVICE", "VERSION", "STATUS", "PLAYERS", "UPTIME", "RESTARTS"
    );
    for (name, answer) in client::status(fleet) {
        let status = match answer {
            Ok(status) => status,
            Err(why) => {
                all_well = false;
                println!("{name:<14} {:<16} {:<14} {:<18} {:<9} {:>7} {:>8} {:>8}  {why}", "?", "-", "-", "UNREACHED", "-", "-", "-");
                continue;
            }
        };
        let platform = status["platform"].as_str().unwrap_or("?");
        let services = status["services"].as_array().cloned().unwrap_or_default();
        if services.is_empty() {
            println!("{name:<14} {platform:<16} {:<14} {:<18} {:<9} {:>7} {:>8} {:>8}", "-", "-", "idle", "-", "-", "-");
        }
        for s in services {
            let state = s["status"].as_str().unwrap_or("?");
            all_well &= state == "running" || state == "stopped";
            println!(
                "{name:<14} {platform:<16} {:<14} {:<18} {:<9} {:>7} {:>8} {:>8}  {}",
                s["name"].as_str().unwrap_or("?"),
                s["version"].as_str().unwrap_or("?"),
                state,
                s["stats"]["players"].as_u64().map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                s["uptime"].as_f64().map(ago).unwrap_or_else(|| "-".into()),
                s["restarts"].as_u64().unwrap_or(0),
                s["last_error"].as_str().unwrap_or(""),
            );
        }
    }
    all_well
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut words: Vec<String> = Vec::new();
    let mut options = std::collections::HashMap::new();
    let mut args = std::env::args().skip(1);
    // Options that are a yes by being there, and take no value.
    const SWITCHES: [&str; 2] = ["allow-unsigned", "allow-plain-console"];
    while let Some(arg) = args.next() {
        match arg.strip_prefix("--") {
            Some("help") => {
                println!("{USAGE}");
                return;
            }
            Some(name) if SWITCHES.contains(&name) => {
                options.insert(name.to_string(), "yes".to_string());
            }
            Some(name) => {
                let Some(value) = args.next() else { fail(&format!("--{name} needs a value")) };
                options.insert(name.to_string(), value);
            }
            None => words.push(arg),
        }
    }
    let option = |name: &str| options.get(name).map(String::as_str);
    let command = words.first().map(String::as_str).unwrap_or("");

    let tls = jengine_server::Tls::from_options(option("tls-cert"), option("tls-key")).unwrap_or_else(|why| fail(&why));
    // A TLS proxy in front of the gateway, believed about who it carries.
    let trusted = || {
        let list = option("trust-proxy").map(str::to_string).or_else(|| std::env::var("JENGINE_TRUST_PROXY").ok());
        list.map(|list| jengine_server::listen::parse_addresses(&list).unwrap_or_else(|why| fail(&format!("--trust-proxy: {why}"))))
            .unwrap_or_default()
    };

    if command == "keygen" {
        // The key releases are signed with. The public half goes to every
        // agent (--release-key); the secret half stays where builds are made.
        let out = PathBuf::from(option("out").unwrap_or("release.key"));
        if out.exists() {
            fail(&format!("{} exists; a key is never written over", out.display()));
        }
        let key = jengine_server::fleet::ReleaseSigner::generate();
        if let Err(e) = std::fs::write(&out, key.to_text()) {
            fail(&format!("could not write {}: {e}", out.display()));
        }
        println!("wrote {} - keep it secret, on the machine that builds", out.display());
        println!("  each agent:  jengine-fleet agent --release-key {}", key.public().to_text());
        println!("  fleet.json:  \"release_key_file\": \"{}\"   (or jengine-fleet sign <artifact>)", out.display());
        return;
    }

    if command == "sign" {
        let Some(artifact) = words.get(1) else { fail("sign which artifact?") };
        let key_file = option("key").map(PathBuf::from).or_else(|| std::env::var_os("JENGINE_RELEASE_KEY_FILE").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("release.key"));
        let key = jengine_server::fleet::ReleaseSigner::load(&key_file).unwrap_or_else(|why| fail(&why));
        match client::sign_artifact(&key, std::path::Path::new(artifact)) {
            Ok(out) => println!("wrote {} - a deploy sends it with the artifact", out.display()),
            Err(why) => fail(&why),
        }
        return;
    }

    if command == "user" {
        let users = PathBuf::from(option("users").unwrap_or_else(|| fail("user needs --users <file> (a host's console: <data>/console/users.json)")));
        match words.get(1).map(String::as_str) {
            Some("add") => {
                let Some(name) = words.get(2) else { fail("user add <name>") };
                let role = option("role").unwrap_or("viewer");
                match ui::Users::add(&users, name, role) {
                    Ok(token) => {
                        println!("{name} ({role}) signs in with this token - it is shown once, and kept only as a hash:");
                        println!("{token}");
                    }
                    Err(why) => fail(&why),
                }
            }
            Some("remove") => {
                let Some(name) = words.get(2) else { fail("user remove <name>") };
                if let Err(why) = ui::Users::remove(&users, name) {
                    fail(&why);
                }
                println!("{name} can no longer sign in");
            }
            Some("list") => match ui::Users::load(&users) {
                Ok(list) => {
                    for (name, entry) in &list.users {
                        println!("{name:<24} {}", entry.role);
                    }
                }
                Err(why) => fail(&why),
            },
            _ => fail("user add|remove|list"),
        }
        return;
    }

    if command == "cert" {
        // A certificate for an agent (or a coordinator) that only its own
        // fleet has to trust: name every address it will be reached at.
        let hosts: Vec<String> = words[1..].to_vec();
        let (cert, key) = jengine_server::listen::self_signed(&hosts).unwrap_or_else(|why| fail(&why));
        let dir = PathBuf::from(option("out").unwrap_or("."));
        let written = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(dir.join("cert.pem"), cert))
            .and_then(|_| std::fs::write(dir.join("key.pem"), key));
        if let Err(e) = written {
            fail(&format!("could not write to {}: {e}", dir.display()));
        }
        println!("wrote {} and {}", dir.join("cert.pem").display(), dir.join("key.pem").display());
        println!("  the agent:   jengine-fleet agent --tls-cert cert.pem --tls-key key.pem");
        println!("  fleet.json:  \"url\": \"https://{}:9900\", \"ca\": \"cert.pem\"", hosts[0]);
        println!("Keep key.pem on the machine it is for; cert.pem is the one to copy.");
        return;
    }

    if command == "service" {
        // What to hand the operating system so the agent - and with it every
        // server it runs - is there again after a reboot. Printed, not
        // installed: where it goes and who it runs as is the machine's
        // owner's to decide.
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "jengine-fleet".into());
        let data = std::path::absolute(option("data").unwrap_or("fleet-data"))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "fleet-data".into());
        // Given a gateway, the machine is a host: the console comes with it.
        let what = if option("gateway").is_some() { "host" } else { "agent" };
        let mut run = format!("{what} --listen {} --data \"{data}\"", option("listen").unwrap_or("0.0.0.0:9900"));
        if let Some(at) = option("gateway") {
            run.push_str(&format!(" --gateway {at}"));
        }
        if let Some(proxies) = option("trust-proxy") {
            run.push_str(&format!(" --trust-proxy {proxies}"));
        }
        let full = |p: &str| std::path::absolute(p).map(|p| p.display().to_string()).unwrap_or_else(|_| p.to_string());
        if let (Some(cert), Some(key)) = (option("tls-cert"), option("tls-key")) {
            run.push_str(&format!(" --tls-cert \"{}\" --tls-key \"{}\"", full(cert), full(key)));
        }
        if let (Some(cert), Some(key)) = (option("gateway-tls-cert"), option("gateway-tls-key")) {
            run.push_str(&format!(" --gateway-tls-cert \"{}\" --gateway-tls-key \"{}\"", full(cert), full(key)));
        }
        if let Some(key) = option("release-key") {
            run.push_str(&format!(" --release-key {key}"));
        }
        for switch in SWITCHES {
            if option(switch).is_some() {
                run.push_str(&format!(" --{switch}"));
            }
        }
        if cfg!(windows) {
            println!("rem As an administrator. The token is set for the whole machine, then a task starts the agent at boot.");
            println!("setx /M JENGINE_FLEET_TOKEN \"<the agent's token>\"");
            println!("schtasks /Create /TN jengine-fleet-agent /SC ONSTART /RU SYSTEM /RL HIGHEST /F /TR \"\\\"{exe}\\\" {}\"", run.replace('"', "\\\""));
            println!("schtasks /Run /TN jengine-fleet-agent");
        } else {
            println!("# /etc/systemd/system/jengine-fleet-agent.service");
            println!("# then: systemctl daemon-reload && systemctl enable --now jengine-fleet-agent");
            println!("[Unit]\nDescription=jengine-fleet agent\nAfter=network-online.target\nWants=network-online.target\n");
            println!("[Service]\nExecStart={exe} {run}\nEnvironment=JENGINE_FLEET_TOKEN=<the agent's token>");
            // Long enough for every server to drain and save before it is killed.
            println!("Restart=always\nRestartSec=2\nKillSignal=SIGTERM\nTimeoutStopSec=120\n");
            println!("[Install]\nWantedBy=multi-user.target");
        }
        return;
    }

    if command == "agent" || command == "host" {
        let token = option("token")
            .map(str::to_string)
            .or_else(|| std::env::var("JENGINE_FLEET_TOKEN").ok())
            .unwrap_or_default();
        let listen = option("listen").unwrap_or("0.0.0.0:9900");
        let data = PathBuf::from(option("data").unwrap_or("fleet-data"));
        // A host is an agent whose machine players connect to: it has a
        // gateway unless told where else to put it.
        let gateway = option("gateway").or((command == "host").then_some("0.0.0.0:9000"));
        // ..and a console behind it, signed in to with its own token or,
        // with none given, the agent's: either one deploys programs here.
        let console_token = (command == "host" && gateway.is_some()).then(|| {
            option("console-token")
                .map(str::to_string)
                .or_else(|| std::env::var("JENGINE_CONSOLE_TOKEN").ok())
                .unwrap_or_else(|| token.clone())
        });
        eprintln!("jengine-fleet {command}: {} on {listen}, data in {}", platform(), data.display());
        if let Some(at) = gateway {
            let port = at.rsplit(':').next().unwrap_or("9000");
            let name = jengine_server::fleet::host_name();
            eprintln!("  players connect to ws://{name}:{port}/<service>; http://{name}:{port}/ lists them");
            if console_token.is_some() {
                eprintln!("  manage it at http://{name}:{port}/manage");
            }
        }
        let release_key = option("release-key")
            .map(str::to_string)
            .or_else(|| std::env::var("JENGINE_RELEASE_KEY").ok().filter(|k| !k.is_empty()))
            .map(|text| jengine_server::fleet::ReleaseKey::parse(&text).unwrap_or_else(|why| fail(&format!("--release-key: {why}"))));
        match &release_key {
            Some(key) => eprintln!("  runs only releases signed by {}", key.to_text()),
            None => eprintln!("  no --release-key: whoever holds the token can run programs here"),
        }
        let gateway_tls = jengine_server::Tls::from_options(option("gateway-tls-cert"), option("gateway-tls-key"))
            .unwrap_or_else(|why| fail(&why.replace("--tls", "--gateway-tls")));
        let host = agent::HostOptions {
            gateway: gateway.map(str::to_string),
            console_token,
            trust_proxies: trusted(),
            gateway_tls,
            allow_plain_console: option("allow-plain-console").is_some(),
            release_key,
            allow_unsigned: option("allow-unsigned").is_some(),
        };
        if let Err(e) = agent::run(listen, data, &token, tls, host) {
            fail(&e.to_string());
        }
        return;
    }

    if command == "gateway" {
        let routes = option("routes").unwrap_or_else(|| fail("gateway needs --routes name=port,.."));
        let routes = StaticRoutes::parse(routes).unwrap_or_else(|why| fail(&why));
        let listen = option("listen").unwrap_or("0.0.0.0:9000");
        let names: Vec<String> = routes.0.iter().map(|(n, p)| format!("/{n} -> 127.0.0.1:{p}")).collect();
        let running =
            gateway::start_with(listen, routes, trusted(), tls).unwrap_or_else(|e| fail(&format!("{listen}: {e}")));
        eprintln!("jengine-fleet gateway on {}: {}", running.url(), names.join(", "));
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let _ = tokio::signal::ctrl_c().await;
        });
        running.stop();
        return;
    }

    let path = PathBuf::from(option("fleet").unwrap_or("fleet.json"));
    if command == "ui" {
        // Checked now, so a typo in the path is an error here rather than a
        // message on a page.
        if let Err(why) = Fleet::load(&path) {
            fail(&why);
        }
        let options = ui::Options {
            users: option("users").map(PathBuf::from),
            allow_plain: option("allow-plain-console").is_some(),
            jobs: option("jobs").map(PathBuf::from),
        };
        if let Err(e) = ui::run_options(option("listen").unwrap_or("127.0.0.1:9800"), path, option("token"), tls, options) {
            fail(&e.to_string());
        }
        return;
    }
    let load = || Fleet::load(&path).unwrap_or_else(|why| fail(&why));
    let service = || words.get(1).map(String::as_str).unwrap_or_else(|| fail("which service?"));
    let mut say = |line: &str| println!("{line}");
    let result = match command {
        "status" => {
            if !print_status(&load()) {
                std::process::exit(2);
            }
            Ok(())
        }
        "deploy" => client::deploy(&load(), service(), option("machine"), &mut say),
        "rollback" | "restart" | "stop" | "start" | "remove" => {
            client::act(&load(), service(), command, option("machine"), &mut say)
        }
        "logs" => {
            let machine = option("machine").unwrap_or_else(|| fail("logs needs --machine <name>"));
            let lines = option("lines").and_then(|n| n.parse().ok()).unwrap_or(100);
            let grep = option("grep").unwrap_or_default();
            client::logs_matching(&load(), service(), machine, lines, grep).map(|text| println!("{text}"))
        }
        _ => fail(USAGE),
    };
    if let Err(why) = result {
        fail(&why);
    }
}
