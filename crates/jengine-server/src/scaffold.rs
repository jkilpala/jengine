//! `jengine-server new`: a game's own server, written out and ready to run.
//!
//! A game with rules of its own is a small crate on this library. This makes
//! one: a `Cargo.toml` that depends on the engine's `jengine-server`, a
//! library that builds the [`Server`](crate::Server) (so tests can start the
//! very server `main` runs), a room to start from with its tests, a
//! `fleet.json` to deploy it with, and a README that says how.
//!
//! Three starting points:
//!
//! | Template | What it is | Start here for |
//! |---|---|---|
//! | `relay` | relay rooms and nothing else | a turn-based game whose clients run the rules |
//! | `match` | a refereed two-player match, opened on demand | anything with matches: the server holds the board |
//! | `world` | persistent zones with doors between them, chat, held seats | a small MMO |
//!
//! The `world` template's room **is** `examples/zones/world.rs`, read at
//! build time - so a generated world is always the example that the engine's
//! own tests play through, not a copy that drifted.

use std::path::{Path, PathBuf};

/// Which server to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Relay,
    Match,
    World,
}

impl Template {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "relay" => Some(Template::Relay),
            "match" => Some(Template::Match),
            "world" => Some(Template::World),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Template::Relay => "relay",
            Template::Match => "match",
            Template::World => "world",
        }
    }
}

/// Where the generated crate finds `jengine-server`.
#[derive(Debug, Clone)]
pub enum Engine {
    /// A path to the `jengine-server` crate - relative to the new project,
    /// or absolute.
    Path(String),
    /// A git repository holding the engine, at whatever its default branch
    /// is when the crate is first built (and `Cargo.lock` then keeps).
    Git(String),
    /// A git repository at one revision, tag or branch - the same engine for
    /// everybody who builds the server, on any machine (`--git <url> --rev
    /// <sha>` / `--tag <tag>`; `--release` is the published engine's tag for
    /// this version).
    GitAt { url: String, pin: Pin },
}

/// What a git dependency is pinned to.
#[derive(Debug, Clone, PartialEq)]
pub enum Pin {
    Rev(String),
    Tag(String),
    Branch(String),
}

/// Where the engine is published.
pub const PUBLIC_REPOSITORY: &str = "https://github.com/jkilpala/jengine";

impl Engine {
    /// The published engine at this crate's own version's tag (`v0.1.0`) -
    /// a server generated anywhere builds against exactly the engine its
    /// generator was.
    pub fn released() -> Engine {
        Engine::GitAt { url: PUBLIC_REPOSITORY.into(), pin: Pin::Tag(format!("v{}", env!("CARGO_PKG_VERSION"))) }
    }

    /// The `{ git = .., rev = .. }` part of a dependency on it.
    fn git(&self) -> Option<String> {
        match self {
            Engine::Git(url) => Some(format!("git = \"{url}\"")),
            Engine::GitAt { url, pin } => Some(match pin {
                Pin::Rev(rev) => format!("git = \"{url}\", rev = \"{rev}\""),
                Pin::Tag(tag) => format!("git = \"{url}\", tag = \"{tag}\""),
                Pin::Branch(branch) => format!("git = \"{url}\", branch = \"{branch}\""),
            }),
            Engine::Path(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    /// The directory to write into. It must not exist, or be empty.
    pub dir: PathBuf,
    /// The crate's name; defaults to the directory's.
    pub name: Option<String>,
    pub template: Template,
    /// Defaults to a path to the engine this generator was built from.
    pub engine: Option<Engine>,
}

const MATCH_ROOMS: &str = include_str!("../templates/server/match_rooms.rs");
const WORLD_ROOMS: &str = include_str!("../examples/zones/world.rs");
const WORLD_TESTS: &str = include_str!("../templates/server/world_tests.rs");

/// A crate name: lower case, digits and `-`, starting with a letter.
fn crate_name(raw: &str) -> Result<String, String> {
    let name: String = raw
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let name = name.trim_matches('-').to_string();
    if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Err(format!("'{raw}' cannot be a crate name: start it with a letter"));
    }
    Ok(name)
}

/// The path from `from` to `to`, relative if they share a root - so a server
/// generated inside the engine's repository does not carry one machine's
/// absolute path.
fn relative(from: &Path, to: &Path) -> String {
    let (from, to) = (std::path::absolute(from), std::path::absolute(to));
    let (Ok(from), Ok(to)) = (from, to) else { return String::new() };
    // `absolute` resolves `..` on Windows and leaves it alone on Unix, so a
    // `../servers/mine` compared component by component shared nothing with
    // the engine there and came out as a climb to the root. Resolved here,
    // lexically, the same on both.
    let (from, to) = (lexical(&from), lexical(&to));
    let (a, b): (Vec<_>, Vec<_>) = (from.components().collect(), to.components().collect());
    let shared = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if shared == 0 {
        return to.to_string_lossy().replace('\\', "/");
    }
    // A long climb is only a relative path by accident - two directories on
    // one disk with nothing to do with each other. Say where the engine is.
    if a.len() - shared > 4 {
        return to.to_string_lossy().replace('\\', "/");
    }
    let mut parts: Vec<String> = a[shared..].iter().map(|_| "..".to_string()).collect();
    parts.extend(b[shared..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    parts.join("/")
}

/// A path with its `.` and `..` components folded away, without touching
/// the disk: `/a/b/../c` is `/a/c`.
fn lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn lib_rs(name: &str, template: Template) -> String {
    let (uses, kinds, about) = match template {
        Template::Relay => (
            "use jengine_server::{Server, Store};\nuse serde_json::json;",
            "        // Rooms that decide the order of messages and nothing else: the\n        // clients run the rules. `{\"t\":\"join\",\"room\":\"ABCD\",\"seats\":2}`.\n        .relay()\n        // HTTP beside the socket, for what is a question rather than play: a\n        // launcher asking who is on. Routes live under `/api/`.\n        .get(\"/api/status\", |_req, api| Ok(json!({ \"online\": api.online(), \"rooms\": api.rooms() })))",
            "relay rooms: the clients run the rules, the server keeps them in step.",
        ),
        Template::Match => (
            "use jengine_server::{ApiError, KindOptions, Server, Store};\nuse serde_json::json;",
            "        // A match is opened when somebody asks for one and closed when it is\n        // empty. `{\"t\":\"join\",\"kind\":\"match\"}` with no room name is\n        // matchmaking: any match with a free seat, or a new one. A seat is\n        // held for twenty seconds if a connection drops.\n        .room(\"match\", KindOptions::default().max_players(2).grace(20.0), |_| rooms::Match::new())\n        // Turn-based games whose clients run the rules can share the port.\n        .relay()\n        // HTTP beside the socket, for what is a question rather than play: a\n        // launcher asking who is on. Routes live under `/api/`.\n        .get(\"/api/status\", |_req, api| Ok(json!({ \"online\": api.online(), \"rooms\": api.rooms() })))\n        // A player's card, for a profile page. `api.player` answers from the\n        // room they are in, or from what was saved if they are in none. For\n        // \"my\" anything, ask `api.caller()?` who the bearer token is.\n        .get(\"/api/players/{id}\", |req, api| {\n            let player = api.player(req.param(\"id\")).ok_or_else(ApiError::not_found)?;\n            Ok(json!({ \"name\": player[\"name\"], \"wins\": player[\"state\"][\"wins\"], \"online\": player[\"online\"] }))\n        })",
            "matches the server referees. See `rooms.rs`.",
        ),
        Template::World => (
            "use jengine_server::{ApiError, KindOptions, Server, Store};\nuse serde_json::json;",
            "        // Chat beside the zones, for anyone signed in.\n        .open_channels()\n        // A zone is a place, not a match: it stays open with nobody in it,\n        // ticks ten times a second, and holds a dropped player's seat.\n        .room(\"zone\", KindOptions::ticking(10.0).max_players(500).persistent().grace(15.0), rooms::Zone::new)\n        // HTTP beside the socket, for what is a question rather than play: a\n        // launcher asking who is on. Routes live under `/api/`.\n        .get(\"/api/status\", |_req, api| Ok(json!({ \"online\": api.online(), \"rooms\": api.rooms() })))\n        // A player's card, for a profile page. `api.player` answers from the\n        // room they are in, or from what was saved if they are in none. For\n        // \"my\" anything, ask `api.caller()?` who the bearer token is.\n        .get(\"/api/players/{id}\", |req, api| {\n            let player = api.player(req.param(\"id\")).ok_or_else(ApiError::not_found)?;\n            Ok(json!({ \"name\": player[\"name\"], \"coins\": player[\"state\"][\"coins\"], \"online\": player[\"online\"] }))\n        })",
            "a small persistent world: zones with doors between them. See `rooms.rs`.",
        ),
    };
    let rooms = if template == Template::Relay { "" } else { "pub mod rooms;\n\n" };
    format!(
        r#"//! {name}: {about}
//!
//! The server is built here rather than in `main.rs` so that a test can
//! start exactly what ships: `{ident}::server(store).start("127.0.0.1:0")`.

use std::sync::Arc;

{uses}

{rooms}/// The server, with its kinds of room. `main.rs` adds what depends on where
/// it runs: the address, the admin token, the cluster.
pub fn server(store: Arc<dyn Store>) -> Server {{
    Server::new()
        .store(store)
{kinds}
}}
"#,
        ident = name.replace('-', "_"),
    )
}

fn main_rs(name: &str) -> String {
    format!(
        r#"//! `{name} [host:port]` - run the server.
//!
//! Everything about *where* it runs comes from the environment, so the same
//! build runs on a laptop and on a fleet:
//!
//! | Variable | |
//! |---|---|
//! | `JENGINE_DB` | the database file (default `{name}.db`) |
//! | `JENGINE_ADMIN_TOKEN` | turns on the dashboard at `/admin` |
//! | `JENGINE_CLUSTER`, `JENGINE_CLUSTER_SECRET`, `JENGINE_NODE` | makes this one node of a cluster |

use std::sync::Arc;

use jengine_server::{{ClusterConfig, SqliteStore}};

fn main() {{
    jengine_server::init_logging();
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:9001".to_string());
    let db = std::env::var("JENGINE_DB").unwrap_or_else(|_| "{name}.db".to_string());
    let store = SqliteStore::open(&db).unwrap_or_else(|e| {{
        eprintln!("{name}: could not open {{db}}: {{e}}");
        std::process::exit(1);
    }});
    let mut server = {ident}::server(Arc::new(store))
        .admin_token(&std::env::var("JENGINE_ADMIN_TOKEN").unwrap_or_default());
    // After the store: a node of a cluster keeps its players with the coordinator.
    if let Some(cluster) = ClusterConfig::from_env() {{
        server = server.cluster(cluster);
    }}
    if let Err(e) = server.run(&addr) {{
        eprintln!("{name}: could not listen on {{addr}}: {{e}}");
        std::process::exit(1);
    }}
}}
"#,
        ident = name.replace('-', "_"),
    )
}

fn readme(name: &str, template: Template) -> String {
    let (what, try_it, client) = match template {
        Template::Relay => (
            "Relay rooms: seats, and one ordered log of messages. The clients run the rules.",
            r#"> {"t":"join","room":"ABCD","name":"Ada","seats":2}
< {"t":"joined","room":"ABCD","seat":0,"seats":2,"peers":[..],"log":[]}
> {"t":"send","body":{"move":4}}
< {"t":"msg","seq":0,"from":0,"body":{"move":4}}      to everyone, the sender included"#,
            "A lockstep game - every client running the same deterministic rules on the same ordered moves - needs nothing more.",
        ),
        Template::Match => (
            "Two-player matches the server referees ([`src/rooms.rs`](src/rooms.rs)): it holds the board, and a move that is not legal never happens.",
            r#"> {"t":"hello","name":"Ada"}
< {"t":"welcome","player":"p_..","token":"..","state":{}}
> {"t":"join","kind":"match"}                         any match with a free seat, or a new one
< {"t":"joined","kind":"match","room":"3fa9c1d2","seat":0,"board":[0,0,0,0,0,0,0,0,0],..}
< {"t":"start","names":["Ada","Bo"]}                  when somebody takes the other seat
> {"t":"send","body":{"place":4}}
< {"t":"placed","seat":0,"cell":4,"board":[..],"turn":1}"#,
            "From a game: the `jengine-online` package signs in, joins, and hands a script the room's frames as events - `Online.join(e, \"match\", \"\")`, `Online.send(e, { place = 4 })`.",
        ),
        Template::World => (
            "A small persistent world ([`src/rooms.rs`](src/rooms.rs)): zones the server runs, players it walks where they ask to go, coins, and a door between zones. What a player has is theirs through every door, after a dropped connection, and after a restart.",
            r#"> {"t":"hello","name":"Ada"}
< {"t":"welcome","player":"p_..","token":"..","state":{}}
> {"t":"join","kind":"zone","room":"meadow"}
< {"t":"joined","kind":"zone","room":"meadow","you":4,"coins":0,..}
< {"t":"sync","enter":[{"id":4,"kind":"player","x":500,"y":300,..},{"id":1,"kind":"coin",..}]}
> {"t":"send","body":{"go":[1010,300]}}               walk east, through the door
< {"t":"joined","kind":"zone","room":"cave",..}        the same player, the next zone"#,
            "From a game: the `jengine-online` package shows the server's entities with a pool of scene entities. The engine's Test Lab scene 221 is a client for exactly this server.",
        ),
    };
    format!(
        r#"# {name}

A game server on jEngine's `jengine-server` - made with
`jengine-server new --template {template}`.

{what}

## Run it

```
cargo run                      # ws://127.0.0.1:9001
cargo test                     # the room's rules, with no sockets
JENGINE_ADMIN_TOKEN=secret cargo run   # and a dashboard at http://127.0.0.1:9001/admin
```

Try it with any WebSocket client:

```
{try_it}
```

{client}

## Where things are

| | |
|---|---|
| `src/lib.rs` | the server: which kinds of room it has |
| `src/rooms.rs` | the rules - plain synchronous Rust, tested with `RoomRunner` |
| `src/main.rs` | where it runs: address, database, dashboard, cluster |
| `fleet.json` | which machines it is deployed to |

A room is five callbacks (`on_join`, `on_message`, `on_leave`, `on_tick`,
`on_close`) that say what they want done to a `RoomCtx`. A client can only
*ask* (`send`); whether what it asks is allowed is whatever `on_message`
checks. What a player owns goes in `player.state`, which the server loads,
carries from room to room, and saves.

## Deploy it

```
cargo build --release
jengine-fleet deploy {name}          # reads fleet.json
jengine-fleet ui                      # or the same in a browser
```

`fleet.json` here names one machine, `local`, with an agent on this
computer: start one with `JENGINE_FLEET_TOKEN=<secret> jengine-fleet agent`,
set `FLEET_TOKEN` to the same secret and `{upper}_ADMIN_TOKEN` to another,
and the deploy above works as it stands. Add real machines beside it. A
Linux machine needs a Linux build: add its path under `"artifacts"`.

## Grow it

- **Accounts.** `.auth(jengine_server::PasswordAuth::new())` in `lib.rs`, or
  `SignedAuth` if a backend of yours signs players in.
- **Several machines.** Run `jengine-server coordinator`, and start each
  server with `JENGINE_CLUSTER` pointing at it.
- **More HTTP routes.** `.get("/api/...", |req, api| ...)` and `.post` in
  `lib.rs`: a leaderboard, a profile, anything a web page or launcher asks.
- **Operators.** `.operator("name", token, Role::Viewer)` for people who may
  look at the dashboard and not touch.

The engine's `crates/jengine-server/README.md` has all of it, and
`LIMITS.md` beside it says what the framework does not do.
"#,
        template = template.name(),
        upper = name.replace('-', "_").to_uppercase(),
    )
}

/// Write a server into `options.dir`. Returns the files written.
pub fn generate(options: &Options) -> Result<Vec<PathBuf>, String> {
    let dir = &options.dir;
    if dir.exists() && std::fs::read_dir(dir).map(|mut d| d.next().is_some()).unwrap_or(true) {
        return Err(format!("{} exists and is not empty", dir.display()));
    }
    let name = match &options.name {
        Some(name) => crate_name(name)?,
        None => crate_name(&dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())?,
    };
    let engine = match &options.engine {
        Some(Engine::Path(path)) => format!("{{ path = \"{}\" }}", path.replace('\\', "/")),
        Some(git) => format!("{{ {} }}", git.git().unwrap_or_default()),
        None => format!("{{ path = \"{}\" }}", relative(dir, Path::new(env!("CARGO_MANIFEST_DIR")))),
    };
    // The world template owns an engine `Tilemap` and paths monsters with
    // the engine's A*, so it links the engine's core crate, headless, from
    // wherever `jengine-server` came from.
    let core = match &options.engine {
        Some(git @ (Engine::Git(_) | Engine::GitAt { .. })) => {
            format!("{{ {}, default-features = false }}", git.git().unwrap_or_default())
        }
        Some(Engine::Path(path)) => {
            format!("{{ path = \"{}/../jengine-core\", default-features = false }}", path.replace('\\', "/"))
        }
        None => format!(
            "{{ path = \"{}/../jengine-core\", default-features = false }}",
            relative(dir, Path::new(env!("CARGO_MANIFEST_DIR")))
        ),
    };
    let core = if options.template == Template::World { format!("jengine-core = {core}\n") } else { String::new() };
    let upper = name.replace('-', "_").to_uppercase();
    let exe = &name;

    let mut files: Vec<(&str, String)> = vec![
        (
            "Cargo.toml",
            format!(
                r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"

[dependencies]
jengine-server = {engine}
{core}serde_json = "1"

# Its own workspace, so it builds the same inside the engine's repository
# as outside it.
[workspace]
"#
            ),
        ),
        ("src/lib.rs", lib_rs(&name, options.template)),
        ("src/main.rs", main_rs(&name)),
        ("README.md", readme(&name, options.template)),
        (".gitignore", "target\n*.db\n*.db-shm\n*.db-wal\nfleet-data\n".to_string()),
        (
            "fleet.json",
            format!(
                r#"{{
  "machines": {{
    "local": {{ "url": "http://127.0.0.1:9900", "token_env": "FLEET_TOKEN" }}
  }},
  "services": {{
    "{name}": {{
      "machines": ["local"],
      "artifacts": {{
        "windows-x86_64": "target/release/{exe}.exe",
        "linux-x86_64": "target/release/{exe}"
      }},
      "args": ["0.0.0.0:{{port}}"],
      "env": {{ "JENGINE_DB": "{name}.db" }},
      "port": 9001,
      "admin_token_env": "{upper}_ADMIN_TOKEN",
      "drain_secs": 30
    }}
  }}
}}
"#
            ),
        ),
    ];
    match options.template {
        Template::Relay => {}
        Template::Match => files.push(("src/rooms.rs", MATCH_ROOMS.to_string())),
        Template::World => files.push(("src/rooms.rs", format!("{WORLD_ROOMS}{WORLD_TESTS}"))),
    }

    let mut written = Vec::new();
    for (path, content) in files {
        let path = dir.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("could not make {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, content).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jengine-scaffold-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn each_template_writes_a_crate_that_names_what_it_is() {
        for template in [Template::Relay, Template::Match, Template::World] {
            let dir = scratch(template.name()).join("My Game Server");
            let written =
                generate(&Options { dir: dir.clone(), name: None, template, engine: None }).unwrap();
            let read = |file: &str| std::fs::read_to_string(dir.join(file)).unwrap();
            assert!(read("Cargo.toml").contains("name = \"my-game-server\""), "the directory's name, made a crate's");
            assert!(read("src/main.rs").contains("my_game_server::server("));
            assert!(read("fleet.json").contains("\"MY_GAME_SERVER_ADMIN_TOKEN\""));
            serde_json::from_str::<serde_json::Value>(&read("fleet.json")).expect("fleet.json is JSON");
            assert_eq!(dir.join("src/rooms.rs").exists(), template != Template::Relay);
            assert!(read("README.md").contains(&format!("--template {}", template.name())));
            assert!(written.len() >= 6);
            assert!(!read("src/lib.rs").contains("{{") && !read("README.md").contains("{upper}"), "no placeholder left behind");
            let _ = std::fs::remove_dir_all(dir.parent().unwrap());
        }
    }

    #[test]
    fn it_will_not_write_over_something() {
        let dir = scratch("occupied");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "mine").unwrap();
        let why = generate(&Options { dir: dir.clone(), name: None, template: Template::Relay, engine: None });
        assert!(why.unwrap_err().contains("not empty"));
        assert_eq!(std::fs::read_to_string(dir.join("notes.txt")).unwrap(), "mine");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G1: a generated server can name the engine at one revision, so every
    /// machine that builds it builds the same engine.
    #[test]
    fn a_git_engine_is_pinned_to_a_revision() {
        let dir = std::env::temp_dir().join(format!("jengine-scaffold-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pinned = Engine::GitAt { url: "https://example.com/engine".into(), pin: Pin::Rev("0123abcd".into()) };
        generate(&Options { dir: dir.clone(), name: Some("pinned".into()), template: Template::World, engine: Some(pinned) })
            .unwrap();
        let toml = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(toml.contains(r#"jengine-server = { git = "https://example.com/engine", rev = "0123abcd" }"#), "{toml}");
        assert!(toml.contains(r#"jengine-core = { git = "https://example.com/engine", rev = "0123abcd", default-features = false }"#), "{toml}");
        let _ = std::fs::remove_dir_all(&dir);
        match Engine::released() {
            Engine::GitAt { url, pin: Pin::Tag(tag) } => {
                assert_eq!(url, PUBLIC_REPOSITORY);
                assert_eq!(tag, format!("v{}", env!("CARGO_PKG_VERSION")));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_engine_is_found_by_a_relative_path_when_there_is_one() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(relative(&here.join("../../servers/mine"), here), "../../crates/jengine-server");
        assert!(crate_name("9lives").is_err());
        assert_eq!(crate_name("Moon Fort!").unwrap(), "moon-fort");
    }
}
