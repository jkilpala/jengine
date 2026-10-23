# jengine-server

A game server framework for jEngine games: from a relay for a board game to
the zones of a small MMO, on one binary.

```rust
use jengine_server::{KindOptions, Server, SqliteStore};

Server::new()
    .store(Arc::new(SqliteStore::open("world.db")?))
    .relay()                                                    // turn-based games
    .room("zone", KindOptions::ticking(10.0).persistent(), Zone::new)
    .run("0.0.0.0:9001")?;
```

You describe a server by registering **kinds of room**. A room is whatever
the game needs: a relay, a lobby, a match, a zone. The framework does the
rest - sockets, sign-in, who is in which room, saving, and shutting down
without losing anyone.

## What it is built around

**A player's state outlives a room.** A `Player` is an id, a name and a state
document. The server loads it at sign-in, *hands it to* the room the player is
in, and takes it back when they leave - so `ctx.transfer(player, ..)` from one
zone to the next moves the same player along, on the same connection, and the
client just sees a new `joined`. A transfer from `on_join` - routing a new or
returning player to where they belong - passes through the asked-for room
unseen: the client gets only the `joined` of the room it lands in. Room-per-match frameworks (Colyseus is the
well-known one) leave that to the game; here it is the centre. The same state
is there after a dropped connection and after the server restarts.

**A place can live with nobody in it.** A room exists once somebody joins
it - unless the server opens it at start (`Server::open_at_start(kind, room)`),
and then it is up and ticking from the first second and stays: a port whose
traders trade while no captain is in it.

**A room is plain synchronous Rust.** Five callbacks that *record* what they
want done; no sockets, no `async`:

```rust
impl Room for Zone {
    fn on_join(&mut self, ctx: &mut RoomCtx, player: &mut Player, options: &Value)
        -> Result<Value, String> { .. }                 // Err turns them away
    fn on_message(&mut self, ctx: &mut RoomCtx, player: &mut Player, body: Value) { .. }
    fn on_leave(&mut self, ctx: &mut RoomCtx, player: &mut Player) { .. }
    fn on_tick(&mut self, ctx: &mut RoomCtx, players: &mut Players, dt: f64) { .. }
    fn on_close(&mut self, ctx: &mut RoomCtx) { .. }
}
```

An `Err` from `on_join` turns the player away - except `Err(ROOM_FULL.into())`
on a **matchmaking** join (no room named), which sends them on to the next
room with a free seat, or a new one. That is how a private match keeps
strangers out without refusing them a game (a two-player duel with a room
code: the first joiner's code, and `ROOM_FULL` for anyone else).

`RoomRunner` drives the same room in a unit test that the server drives in
production, so server rules are tested like any other Rust - and can share a
crate with the game's package, which is how a client and a server come to
agree on the rules. `runner.room::<T>()` / `room_mut::<T>()` hand a test the
room as its own type, to read its state or set a position up before the
messages it means to test.

**Several machines can be one world.** Nodes that share a coordinator share
their players and split their rooms between them; a door can lead to another
machine. See "Past one machine" below.

**A big room tells each player only what is near them.** `sync::Space` holds
a room's entities and sends each watcher what entered their view, what changed
in it, and what left - on the layers they see (`spawn_on`, `watch_layers`:
floors, a ghost only a medium sees, fog by team). A thing that faces a way
is turned with `set_rotation`, which rides on its moves as a fourth number
and turns the client's slot with it. `only_for(id, players)` shows a thing to
chosen watchers alone (a hider's own body, a hand of cards), `opaque_ids(seed)`
makes ids that say nothing of their order, and `with_cell(radius, cell)` sizes
the grid for the common view when views differ widely. Underground, range is not sight: `watch_sight`
gives a watcher the cells they can see (the room's own line of sight), and
an entity round a corner is not sent to them at all - a dungeon's levels,
where the room computes line of sight over its own tiles, are the case it
was made for.

**A server links the engine.** It is plain Rust in the engine's workspace,
so it uses the crates the engine and its packages are made of rather than
copies: `jengine-core` headless (`default-features = false` - no scripting,
audio or GPU; `features = ["physics"]` adds rapier) and any gameplay package
the same way. `examples/zones` is the worked case: the zone owns an engine
`Tilemap`, its monsters hunt with the engine's A* over it
(`jengine_core::tilemap::pathfinding`), and the same layer is handed to the
netcode crate as `Walls` and sent to the client - so a player is predicted
against the very walls the monsters path through. The one difference from a
runtime: a package's *systems* (`register_systems`, the `PackageHost` phases)
are not run - a room owns its data and calls the package's functions, the way
the package's own tests do. The generated `world` server links it the same way.

**Steering can be predicted.** [`jengine-netcode`](../jengine-netcode/src/lib.rs)
(re-exported as `jengine_server::netcode`) is the arithmetic both ends share:
a fixed step, one numbered input per step, and a `Motion` that turns an input
into a move - `TopDown` (a direction, a speed, a box) or `TileMotion` (the
same, stopped by and sliding along `Walls`, which the room sends with its
numbers). A room queues inputs (`InputQueue`), applies one a tick and
acknowledges it in the `sync` frame (`Space::ack`); the client applies the
same input the frame it is pressed and, when the server's answer arrives,
replays what has not been answered on top of it (`Reconciler`). The server
still decides everything - a client that lies about where it is has only
moved its own picture. `examples/zones` does it (`{"input":{"seq","x","y"}}`),
and `jengine-online`'s `Online.input` is the client half. `netcode::History`
and `Space::position_at` are for lag compensation: where something *was*
when a client fired. The example zone's `tag` is the worked case - judged
against the tick the tagger says they were looking at.

**A board is ordered by the store.** `store.save_scored(ns, key, value,
score)` keeps a score beside the value; `top(ns, n, Order::Lowest)` and
`rank(ns, key, order)` read only what is shown - one query in SQLite, one
request to a cluster's coordinator. A time-trial leaderboard, with each
run's ghost saved as the value, is the case it was made for.

**A game has HTTP routes beside its socket.** What is a question rather than
play - a player's stats for a profile page, who is on for a launcher, a
leaderboard for a web site:

```rust
Server::new()
    .get("/api/players/{id}", |req, api| {
        let player = api.player(req.param("id")).ok_or_else(ApiError::not_found)?;
        Ok(json!({ "name": player["name"], "coins": player["state"]["coins"] }))
    })
    .get("/api/me", |_req, api| {
        let me = api.caller()?;            // who the bearer token is; 401 if nobody
        api.player(&me).ok_or_else(ApiError::not_found)
    })
```

A handler is synchronous Rust given the request (path `{parts}`, query, JSON
body) and an `Api`: `player(id)` - their state **as the room has it now** if
they are in one, as last saved if not - `caller()`, `online()`, `rooms()`,
`room(kind, id)`, `store()`, `edit_player(id, |state| ..)` - which changes a
player's state in one step wherever it lives: their room, their connection or
the saved record (a shop page granting a coin) - and `command(kind, id, json)`,
which hands a room's own `on_admin` a command. `get`, `post`, `put`, `delete`; paths start
with `/api/`; answers are JSON (`Server::routes(|router| ..)` merges a game's
own axum router for anything else); any origin may call (a bearer token, not a
cookie, says who is asking - `Config::api_allow_origins` narrows it to a
game's own site); a route that panics fails one request. Each route's
requests, failures and times are on the dashboard and in `/metrics`. In a
cluster a route sees the whole of it: a player on another node is asked of
that node. And the port can be TLS (`Server::tls`, `--tls-cert` /
`--tls-key`): `wss://` for the game, `https://` for everything else. One
address gets `Config::api_requests_per_second` (20, with a burst of twice)
and a 429 past it; a caller with a bearer token gets
`Config::api_token_requests_per_second` of their own instead, so a player on
a shared address is not held to everyone else's budget there; an expensive
route can have a budget of its own (`Server::route_limit("POST",
"/api/search", 1.0)`); and a body past `Config::api_max_body_bytes` (2 MiB)
is a 413. **Behind a proxy** every request comes from the proxy, so
name it: `Config::trusted_proxies` (`--trust-proxy <addr>`, or
`JENGINE_TRUST_PROXY` for any server built on this crate) believes it about
who it carried - the last `X-Forwarded-For` entry, else `X-Real-IP` - for the
rate limit and for each connection's address (the dashboard's player list,
the "signed in from" event) - and about how the player reached it
(`X-Forwarded-Proto`), which decides whether a secret came in the clear. A
host's gateway on the same machine is believed without being named; the
gateway itself believes only the proxies given to `jengine-fleet host
--trust-proxy`.

**A connection is not a player either.** Give a kind of room a grace period
(`KindOptions::grace(15.0)`) and a player whose connection drops keeps their
seat: sign in again within it and they are put back - the same avatar, the
same place, a `joined` frame marked `resumed` - and nobody in the room saw
them go. `jengine-online` reconnects by itself, so on a phone that changes
networks the game just carries on.

**Channels sit beside rooms.** `Server::open_channels()` (or `.channels(policy)`)
gives signed-in players something to be in *besides* their room and keep from
room to room: `sub`, `say`, and everyone in it hears `said`. Joining says who
is there and the last few things said (kept in the store, so a restart does
not forget them); members hear who comes and goes; a room can speak in one
(`ctx.say`). Chat, a party, a guild. An operator lists the channels
(`/admin/api/channels`), mutes a player in one for a while
(`/admin/api/channel/{name}/mute`) and clears what one remembers - on every
node of a cluster.

**Who a player is, is pluggable.** `GuestAuth` (the default) knows a player
by a token it handed out. `PasswordAuth` keeps accounts - a name and a
password, stored as a salted, stretched hash - so a player is the same from
any device. Sessions end thirty days after their last use; a player can
change their password or sign out everywhere (`{"t":"account",..}`); wrong
passwords lock an account for a growing while; an operator can reset or
delete one. Give `PasswordAuth::recovery` a way to reach a player (an email
the game keeps) and one who forgot their password gets back in by a
short-lived code (`{"t":"hello","account":"ada","recover":true}`, then
`"code"` with a new `"password"`). `SignedAuth` believes tickets signed by
*your* backend with a shared secret - its own format or a standard HS256
JWT (`sub`, `exp`; `SignedAuth::issuer`/`audience` to check `iss`/`aud`) -
so the game server never sees a password; `SignedAuth::revoke` (or the
dashboard's sign-out of a player, for any of these) ends every ticket
issued until then. Or implement `Authenticator`.

**A password never crosses a network in plain text.** A `hello` with a
password (or a recovery code, or a change of password) on a socket that
came over the network unencrypted is refused with the reason
(`PASSWORD_NEEDS_TLS`): give the server a certificate (`Server::tls`), put
a TLS proxy in front, or run a host's gateway with TLS. From the same
machine, or through a proxy that says the player reached it over `https`,
it is taken. `Server::allow_plain_passwords()` takes it anyway, for a LAN
you trust.

**A queue in front of a match.** `matchmaking::Matchmaker` is a kind of
room that holds players until a rule groups them - by rating, with a window
that widens the longer they wait, within a region until a patience runs
out, or a game's own function - then sends each group into a fresh room of
the match kind with what it knew of them in the join's options:

```rust
.room("duel", KindOptions::default().max_players(2), |_| Duel)
.room("queue", KindOptions::ticking(4.0).max_players(10_000).persistent(), |_| {
    Matchmaker::by_rating("duel", 2).window(100.0, 25.0).region_patience(20.0)
})
```

## Starting your own

```powershell
cargo run -p jengine-server -- new ../my-game-server --template world
cd ../my-game-server
cargo test        # the room's rules, no sockets
cargo run         # ws://127.0.0.1:9001
```

`jengine-server new <dir> [--template relay|match|world]` writes a game's own
server crate: a library that builds the `Server` (so a test can start exactly
what ships), a room to start from with its tests, a `main.rs` that takes its
address, database, dashboard token and cluster from the environment, a
`fleet.json` to deploy it with, and a README.

| Template | What you get | Start here for |
|---|---|---|
| `relay` | relay rooms only | a turn-based game whose clients run the rules |
| `match` (the default) | a two-player match the server referees, opened on demand, seats held through a dropped connection | anything with matches |
| `world` | persistent zones with doors, chat channels, held seats | a small MMO |

The `world` template's room is `examples/zones/world.rs` itself, read at
build time, so a generated world is the example the engine's own tests play
through. `cargo test -p jengine-server --test a_generated_server -- --ignored`
generates all three and builds and tests each as a standalone crate.

## Three servers, smallest first

| | What you write |
|---|---|
| **A relay** | Nothing: `cargo run -p jengine-server`. It speaks `jengine-relay`'s protocol frame for frame - the relay's own socket tests pass against it unchanged (`tests/relay_clients_connect_unchanged.rs`). |
| **A match or a lobby** | A `Room` with `on_join` / `on_message`; rooms open on demand and close when empty. `{"t":"join","kind":"match"}` with no room name is matchmaking: any room with space, or a new one. |
| **A world** | Persistent, ticking rooms with a `Space` each, and `ctx.transfer` between them. [`examples/zones`](examples/zones/world.rs) is 170 lines: avatars the server walks, coins, a door between two zones. |

```powershell
cargo run -p jengine-server                          # relay rooms on 127.0.0.1:9001
cargo run -p jengine-server -- 0.0.0.0:9001 --db relay.db
cargo run -p jengine-server --example zones          # the small world
cargo run -p jengine-server --release --example zone_cost   # what a crowded zone costs
```

The protocol is in the crate doc (`src/lib.rs`): JSON text frames, four from
the client (`hello`, `join`, `send`, `leave`).

## The stack, and why

| Choice | Because |
|---|---|
| **Rust** | The engine and its packages are Rust, so a game's rules compile into both ends. One static binary to deploy. |
| **WebSocket**, JSON text | The only transport a browser has, and what `jengine-net` speaks on native, web and in the editor. Text because the script channel is text. A binary encoding and WebTransport can come later behind the same `Room` API; nothing in a room knows how a frame is carried. |
| **tokio + axum** | Thousands of mostly idle sockets and rooms on timers is what an async runtime is for. axum puts `/healthz` and `/stats` on the same port as the socket. |
| **One task per room** | A room's code is single-threaded and lock-free; rooms run in parallel with each other, which is where the cores go. A room that panics is closed and its players released with their state - the server carries on. |
| **SQLite** (WAL), behind a `Store` trait | No second service to run for a small game: the world is one file. The trait is a namespaced key to a string, so Postgres or Redis is an implementation, not a redesign. |
| **TLS at the server or a proxy** | `Server::tls`, a host's gateway with a certificate, or Caddy / a cloud load balancer in front for `wss://`. A web build served over `https` *needs* `wss://`, and the server refuses a password or an admin token that crossed a network without it. |
| **A store behind a trait** | `Server::write_behind()` puts a store a network away behind a writer thread: a room's autosave never waits on it, and the server flushes before it hands a player to another node. |

## Deploying

The server is one executable and, with SQLite, one file. It stops cleanly on
SIGTERM, Ctrl+C or the admin API's `shutdown`: every player is taken out of
their room and saved, then the rooms close - so a deploy with people online
loses nothing (they reconnect with their token and are where they were).

### `jengine-fleet`: servers on machines

An **agent** runs on each machine; a command line on yours pushes builds to
them. Nothing else to install - no Docker, no SSH keys, no orchestrator - and
it is the same on Linux and Windows.

```powershell
# On each machine, once (as a service, so it survives a reboot):
$env:JENGINE_FLEET_TOKEN = "a-long-secret"
jengine-fleet agent --listen 0.0.0.0:9900 --data ./fleet-data

# On yours, beside a fleet.json:
jengine-fleet deploy zones          # every machine the service lists, one at a time
jengine-fleet status
jengine-fleet rollback zones        # back to the build before
jengine-fleet logs zones --machine eu-1
jengine-fleet stop|start|restart|remove zones [--machine eu-1]
```

```text
MACHINE        PLATFORM         SERVICE        VERSION            STATUS    PLAYERS   UPTIME RESTARTS
eu-1           linux-x86_64     zones          e753c5dee0d8452d   running       212     3.1h        0
us-1           linux-x86_64     zones          e753c5dee0d8452d   running        87     3.1h        0
```

Or the same with buttons:

```powershell
jengine-fleet ui          # then open http://127.0.0.1:9800
```

One page, on your own machine: every machine and what runs on it (a `behind`
badge where the local build is newer), a **Deploy** button per service,
**Update / Restart / Stop / Start / Rollback / Logs** per machine, each
server's dashboard (reached through the page and the machine's agent, signed
in by the page's own token), and every deploy as a job whose progress you watch.
It calls the same functions the command line does, so there is one
implementation of a deploy; the agents' tokens stay in the `ui` process and
never reach the browser. On loopback it needs no sign-in (a token is made up
at start and served inside the page); to host it for a team, give it
`--listen 0.0.0.0:9800 --token <secret>` behind a TLS proxy.

`fleet.json` names the machines and says what runs where:

```json
{
  "machines": {
    "eu-1": { "url": "http://10.0.0.5:9900", "token_env": "FLEET_TOKEN" },
    "us-1": { "url": "http://10.0.1.5:9900", "token_env": "FLEET_TOKEN" }
  },
  "services": {
    "zones": {
      "machines": ["eu-1", "us-1"],
      "artifacts": { "linux-x86_64": "target/x86_64-unknown-linux-gnu/release/zones" },
      "args": ["0.0.0.0:{port}", "zones.db"],
      "port": 9001,
      "admin_token_env": "ZONES_ADMIN_TOKEN",
      "drain_secs": 30,
      "overrides": { "us-1": { "port": 9002 } }
    }
  }
}
```

What a deploy does on each machine, and why it is safe to run with players on:

1. **Upload**, unless the machine has it. A release is named by the hash of
   its bytes, so the same build is the same release everywhere and the agent
   refuses bytes that are not what they claim.
2. **Drain** the running server: no new connections, `/healthz` to 503, wait
   up to `drain_secs` for the players to go.
3. **Stop** it through its admin API, which saves everyone.
4. **Start** the new build in the *service's* directory - the database is the
   service's, not the release's - and wait for its health check.
5. If that never answers, **put the old build back** and report it. The
   rollout stops there: a bad build costs one machine a moment, not all of them.

Between deploys the agent restarts a server that dies (backing off if it keeps
dying), keeps its output in a log, and after a reboot starts whatever was
running. `jengine-fleet status` exits non-zero if anything is not well, so it
is also a check for a script.

**A token alone does not run new code on the machine.** Make a release key
where builds are made, give each agent its public half, and an agent runs
only releases its secret half signed - so a stolen agent token can stop,
start and roll back what is there, and nothing more:

```powershell
jengine-fleet keygen                              # release.key; prints the public half
jengine-fleet agent --release-key jfr-public-...  # on each machine
# fleet.json: "release_key_file": "release.key"   - deploy signs each upload
jengine-fleet sign target/release/zones           # or: zones.sig beside the build, made where it was built
```

The console takes a build with its `.sig` (pick both). **An agent listening
beyond loopback will not start without a release key** unless told
`--allow-unsigned` - the old behaviour, where the token was the keys to the
machine. The editor's own test agent is on loopback and needs none.

And give the agent a certificate, so its token is never readable on the
wire - it speaks HTTPS only:

```powershell
jengine-fleet cert 10.0.0.5 eu-1.internal        # writes cert.pem and key.pem
jengine-fleet agent --tls-cert cert.pem --tls-key key.pem
# fleet.json:  "eu-1": { "url": "https://10.0.0.5:9900", "token_env": "FLEET_TOKEN", "ca": "eu-1.pem" }
```

The fleet trusts exactly the certificate it is given (`ca`) and no other for
that machine. `jengine-fleet ui` takes the same two options when it is hosted
for a team, and so does a cluster's coordinator
(`jengine-server coordinator --tls-cert .. --tls-key ..`, with
`JENGINE_CLUSTER_CA` on the nodes). Without a certificate the agent is plain
HTTP and says so when it starts: keep it on a private network.

`jengine-fleet service` prints what to give systemd or Task Scheduler so the
agent comes back after a reboot.

What it does not do: **build** (a Linux machine needs a Linux executable -
build it on Linux, in CI, or with a cross toolchain; the CLI picks the
artifact per machine's platform and will not send a Windows program to a Linux
agent), hand a port over without a gap (one port, one server: a deploy is a
few seconds per machine), or start machines. And one honest gap: **it has only
been run on Windows**, agents and servers on one machine with different ports.
The Unix paths (SIGTERM, file modes) are written and unexercised.

### A host on your LAN: a Raspberry Pi running several servers

`jengine-fleet host` is the agent with a **gateway**: one port (9000) in front
of every server the machine runs, routing by the first segment of the path.
Each server listens on `127.0.0.1` only; the gateway is the way in.

```text
  ws://raspberrypi.local:9000/chess        ─┐            ┌─► 127.0.0.1:9040  chess
  ws://raspberrypi.local:9000/arena        ─┼─ gateway ──┼─► 127.0.0.1:9041  arena
  http://raspberrypi.local:9000/           ─┘ (lists)    └─► 127.0.0.1:9042  zones ...
```

The rest of the path is the server's own - `/`, `/ws`, `/healthz`, a game's
`/api/..`, and the dashboard at `/<service>/admin`. `http://<pi>:9000/` lists
what runs and the address to type (JSON to anything not asking for HTML, so a
game can find its server with one GET). `raspberrypi.local` is the Pi's own
name over mDNS, which Raspberry Pi OS answers and Windows, macOS and phones
ask - no DNS server needed. Use whatever name the Pi was given.

Scripts in [`pi/`](pi/README.md) do the whole thing from Windows: build for
the Pi (cross-compiled with cargo-zigbuild, no WSL), install the host as a
systemd user service under its own `jengine` account, and deploy.

```powershell
# Your fleet file: the Pi and its services (gitignored; JENGINE_FLEET_FILE for one elsewhere).
Copy-Item crates/jengine-server/pi/fleet.example.json crates/jengine-server/pi/fleet.json
# Once, on the Pi with sudo: the account the servers run as.
#   scp crates/jengine-server/pi/prepare-pi.sh <you>@<pi-address>:   then: sudo bash prepare-pi.sh
./crates/jengine-server/pi/setup-pi.ps1 -Address <pi-address>   # install / upgrade the host
./crates/jengine-server/pi/deploy-pi.ps1     # build and deploy every game server
./crates/jengine-server/pi/pi.ps1 status     # also logs, restart, console, ...
```

[`pi/fleet.example.json`](pi/fleet.example.json) lists the Pi and two
services, a relay and a live ops service, both the stock `jengine-server`.
A new server is one more entry there, with its own port. Its name is its
path. The deploy refuses a build for the wrong processor, so an x86-64
program never reaches the Pi. In each game's server field, type
`ws://<pi>:9000/<service>`.

`jengine-fleet gateway --routes chess=9040,arena=9041` is only the
gateway, for servers started by hand.

#### The host's console: managing it from any browser

A host also serves **its console** at `http://raspberrypi.local:9000/manage`.
It's the fleet's web interface (`jengine-fleet ui`) running on the host
itself, so a phone or a friend's laptop on the LAN can manage it without
installing anything. Sign in with the console's token: `--console-token`,
`JENGINE_CONSOLE_TOKEN`, or the agent's own token if neither is given - or a
person's own (`jengine-fleet user add`, below). **From another machine the
token is taken only over TLS** (`--gateway-tls-cert`/`--gateway-tls-key`),
or with `--allow-plain-console` on a LAN you trust; the page itself loads
either way and says so.

- **An overview at the top**: players online, servers running, errors
  and game traffic over the last hour or day, then each machine - CPU,
  memory, disk, temperature (a Raspberry Pi's SoC), network now and since
  boot - with charts of each over time, and the latest errors and warnings
  from the servers' logs (click one for the log). The console samples every
  machine every five seconds whether or not the page is open, keeping an
  hour at that grain and a day at one a minute (`<data>/console/history.json`),
  and checks each sample: a machine away for 30 s, a service crashed for 15,
  a disk 95% full or a machine at 80 °C, a restart, new error lines - POSTed
  to `"alert_webhook"` in the fleet file (or `JENGINE_ALERT_WEBHOOK`) and
  listed at `api/alerts`.
- **Upload a build from the page**, then Deploy. A host has no build tree,
  so the console keeps uploaded builds beside its own fleet file in
  `<data>/console/`, where the host calls itself `host`. A service made
  there gets an admin token the console makes up and keeps.
- **Every server's dashboard, inside the console, from the one sign-in.**
  Pick a server (or press Dashboard on its row) and its own dashboard opens
  below: rooms, players, kick, notices, drain, accounts (reset a password,
  delete an account), live ops (a **Live ops** button appears for a server
  that runs it), and every one of the game's HTTP routes
  with its counts. It goes through the console to the agent, which adds the
  server's admin token, so the browser never holds that token or the
  agent's.
- **Jobs outlive a restart**, so what was deployed is still listed after
  the Pi reboots.
- Deploys, restarts, rollbacks and logs work as from your own machine.

The editor's Servers panel stays the light side: build, run, deploy this
project. A machine in the fleet file with
`"console": "http://raspberrypi.local:9000/manage"` gets a **Console ↗**
button there that opens it.

**The console has people of its own.** `jengine-fleet user add ada --role
viewer --users <data>/console/users.json` prints a token for Ada, kept only
as a hash: a viewer looks, an operator deploys, and each job says who
started it. Remove the line and the token stops at once.

**The gateway can speak TLS**: `jengine-fleet host --gateway-tls-cert
cert.pem --gateway-tls-key key.pem` makes it `https://` and `wss://` only.
Without a certificate it is plain - right for a LAN of native clients - and
it tells every server and the console behind it so (`X-Forwarded-Proto`):
**a password, a server's admin token and the console's token from another
machine are then refused**. `--allow-plain-console` takes the console's
token over a plain LAN you trust; a server takes its admin token so only
with `Server::admin_over_plain`. A web build served over `https` can't open
`ws://` at all: serve it from the gateway's TLS, or put Caddy in front. The
rest is H1-H9 in [`LIMITS.md`](LIMITS.md).

### Without the fleet tool

- **A VPS by hand**: copy the binary, run it under systemd, put Caddy in front
  (`reverse_proxy localhost:9001` is the whole config, and it gets the
  certificate). On the same machine the proxy is loopback, already believed
  about who it carries.
- **A container**: [`Dockerfile`](Dockerfile), built from the repo root
  (`docker build -f crates/jengine-server/Dockerfile .`). Mount a volume at
  `/data` for the database. **Not built on the machine this was written on**
  (no Docker there) - treat it as a starting point.

Either way players need TLS: put Caddy or a load balancer in front of the
game port for `wss://`.

A game with rooms of its own is its own small binary crate depending on this
one - `examples/zones/main.rs` is the shape. Turn the `fleet` feature off
there; it is only the deploy tool.

## Watching it, and running it

Start a server with an admin token and it has a dashboard:

```powershell
$env:JENGINE_ADMIN_TOKEN = "a-long-secret"
cargo run -p jengine-server --release --example zones
# then open http://127.0.0.1:9001/admin
cargo run -p jengine-server --release --example bots -- ws://127.0.0.1:9001 300   # somebody to watch
```

(`Server::admin_token("..")` in a game's own server; `--admin-token` on the
stock binary. `Server::operator(name, token, Role::Viewer)` adds people by
name: a viewer's token opens the dashboard and can change nothing, and what
an operator does is logged under their name.) **With no token there is no admin surface at all** - the routes
do not exist - and every API call must carry `Authorization: Bearer <token>`.
More people are added while the server runs - `POST /admin/api/operators
{"name","role","days"}` answers a token once and keeps only its hash;
giving a name a new one rotates it, `DELETE` ends it. **The token is a
password, and is refused when it crossed a network in plain text**: from
this machine (the editor, the fleet's agent) or over TLS - the server's own,
or a trusted proxy's - it is taken (`Config::admin_over_plain` for a LAN you
trust).

| | For | What |
|---|---|---|
| `GET /admin` | a person | One page, polling the API every two seconds: connections, traffic, each room's players and **what a tick costs against its budget**, who is online and where, an event log. CPU and memory of the process and the machine. Six charts of the last six hours (players, messages, traffic, slowest tick, CPU, memory), with a table view. Click a room for its players' live state and what the room says about itself. Kick, disconnect, close a room, send a notice, drain. |
| `/admin/api/..` | scripts, `curl` | The same, as JSON: `overview`, `players`, `events`, `room/{kind}/{id}`, `history` (`?range=week` a minute a point, `days` an hour), `account/{name}/reset` (POST a new password) and `account/{name}` (DELETE), `player/{id}` (GET the saved record; PUT `{"state":{..}}` to edit a player wherever they are - they are told with a `state` frame), POST `player/{id}/sign_out`, `operators`, `channels`, and POST `kick`, `notice`, `drain`, `room/{kind}/{id}/close`, `room/{kind}/{id}/command`, `channel/{name}/mute`, `channel/{name}/clear`. |
| `GET /metrics` | Prometheus, Grafana | Counters and gauges, labelled per *kind* of room and never per room or player. **Behind the admin token by default** (a scraper sends it as a bearer); `Server::public_stats()` / `--public-stats` opens it and `/stats`. |
| `GET /healthz` | a load balancer | `ok`, or 503 while draining. Always open. |
| `GET /stats` | an autoscaler, the fleet's agent | Counts as JSON; private like `/metrics`. |

Two hooks make it a game's own tool rather than a generic one:

```rust
impl Room for Zone {
    // Shown on the room's page.
    fn describe(&self) -> Value { json!({ "wolves": self.wolves.len() }) }
    // POST /admin/api/room/zone/meadow/command  {"spawn":"boss"}
    fn on_admin(&mut self, ctx: &mut RoomCtx, players: &mut Players, command: Value) -> Value { .. }
}
```

**Draining** is how a deploy goes: `POST /admin/api/drain {"on":true}` stops
new connections and turns `/healthz` to 503, so a load balancer sends new
players elsewhere while the ones here play on; then stop the process, which
saves everyone.

`tools/click_pages.mjs` clicks through this page and the fleet's in a headless
browser - every button, against a live server - when either is changed.

**What is wrong is said first.** A banner at the top of the page, and a
`warn` line in the event log when it starts and when it stops: the machine
short of memory or CPU, a room whose tick ran past its budget, a cluster's
coordinator not answering, saves waiting for the store. Set
`Config::warn_webhook` and the same is POSTed to a URL - a chat webhook, a
pager - so nobody has to be watching; a POST that fails is tried again
(`warn_retry_secs`), and a warning still on after `warn_repeat_secs` (an hour)
is said again as `still`. A game adds warnings of its own with
`Server::warn_when(key, || -> Option<String>)` - a market nobody sells in, a
queue nobody is taken from. Each room's row shows its share of one core.

What an operator does is written to the event log (`admin` events), beside
rooms opening and closing, panics, and clients dropped for not reading.
Those - with starts and panics - are written to the store and are still there
after a restart; rooms opening and players coming and going are kept in memory
only. It is the last 500 of each, for "what happened", not a full audit trail;
the same lines go to the process log.

## How far one machine goes

`cargo run --release --example zone_cost` measures a tick of a crowded zone -
every player moving, every player watching (Ryzen laptop, one core):

| Players in one zone | Others in view | ms per tick | Per player at 10 Hz |
|---|---|---|---|
| 100 | 12 | 0.2 | 2.4 KB/s |
| 500, spread out | 16 | 1.0 | 3.5 KB/s |
| 500, bunched up | 95 | 3.4 | 19 KB/s |
| 2000, spread out | 16 | 4.6 | 3.8 KB/s |
| 2000, bunched up | 104 | 18 | 22 KB/s |

At ten ticks a second a zone has 100 ms, so a zone of 500 costs a few percent
of a core for sync and a zone of 2000 fits unless everyone stands in one
place. Zones are separate tasks, so a world is as many of them as there are
cores. That is the sync alone - a game's own rules are on top.

Those are one core. Since 2026-10-22 a flush with a lot to do is split
across cores (up to eight, at least 32 watchers each; `Space::flush_threads`
caps it), its frames joined in the same order - `zone_cost` prints both. In
a debug build here a bunched-up crowd of 365 (134 in view each) went from
30 ms to 9.7 ms a tick, and 2000 bunched up from 146 to 46; release numbers
are smaller still. Re-run it in release on your own machine.

Over real sockets, on loopback, `examples/bots` against `examples/zones`
(measured before movement was sent as triples, which roughly halved both
columns of the table above; not re-run since):

| Connected | In one zone | Tick, average | Tick, worst | Out |
|---|---|---|---|---|
| 300 | 192 | 3.4 ms | 6.8 ms | 6 MB/s |
| 1,000 | about 365, in each of two | 13-15 ms | **50-116 ms** | 29 MB/s |
| 2,500 | (most refused: zones cap at 500) | - | - | - |

The server held 2,500 connections with nobody dropped and 420 MB of memory.
The zone is what gives: this example world is so small that nearly everyone is
in everyone's view, so 365 players is 365 x 365 updates a tick, the worst tick
ran past its 100 ms budget, and each player was sent about 40 KB/s. A zone
laid out so most players are *out* of each other's view is the first table,
not this one. One laptop with the bots on it, not a real network.

## Past one machine: a cluster

A server on its own is one machine's worth of players. Several of them become
one world by sharing a **coordinator**:

```powershell
# One coordinator: the cluster's store, and its directory.
jengine-server coordinator 0.0.0.0:9100 --secret <long-secret> --db world.db

# Any number of nodes - the same server binary, told where the coordinator is.
$env:JENGINE_CLUSTER = "http://10.0.0.1:9100"
$env:JENGINE_CLUSTER_SECRET = "<long-secret>"
$env:JENGINE_NODE = "eu-1"
$env:JENGINE_PUBLIC_URL = "wss://eu-1.example.com"     # what players connect to
$env:JENGINE_INTERNAL_URL = "http://10.0.0.5:9001"     # how the coordinator reaches it
cargo run -p jengine-server --release --example zones -- 0.0.0.0:9001
```

(`Server::cluster(ClusterConfig::new(..))` in a game's own server;
`ClusterConfig::from_env()` reads those variables.)

- **One store.** Nodes read and write players through the coordinator, so a
  player saved on one node is the player loaded on the next.
- **A room is on one node.** The first node asked for a named room opens it;
  any other node answers with `{"t":"redirect","url":..,"kind","room"}`, and
  the client connects there with its token. `jengine-online` does that by
  itself. A door between two zones can be a door between two machines - the
  player is saved before they are sent.
- **One connection per player, across nodes.** Signing in on one node has
  whichever node had the player save them and let go first. A seat *held*
  after a dropped connection sends the player back to the node holding it.
- **Channels and notices cross.** Rooms do not: everyone in a room is
  connected to its node.

Rooms joined by "any with space" - matches - never leave the node they are
asked on, so matchmade games need no coordinator at all: put the nodes behind
a load balancer.

**The coordinator can stop and come back.** While it is away the players who
are on play on, nobody new is signed in (and is told why - a store that does
not answer is never taken for "no such player"), and writes wait on the nodes.
A coordinator that returns knowing nothing is told by each node what it holds.
A new room asked of a node far busier than another goes to the quieter one.

The dashboard of any node shows the whole cluster: its nodes with their CPU
and memory, and every room and player marked with the node it is on. Looking
into a room, sending it a command, closing it, kicking or editing a player -
each is carried out by the node that has them, from whichever dashboard you
have open.

**The secret never crosses the wire.** Every call between a node and its
coordinator is signed (an HMAC over its method, path, time, a nonce and its
body) with a key derived from the secret for that node; a replay or a call
five minutes old is refused - so the machines' clocks must agree. A node
can be given its own key alone (`jengine-server node-key eu-1 --secret ..`
into `JENGINE_CLUSTER_NODE_KEY`), and then cannot speak as any other; the
coordinator trusts the nodes' own certificates with `--node-ca`, so with TLS
on both sides nothing between them is readable. Upgrade a cluster's nodes
and coordinator together. What a cluster does **not** do - and it is a real
list: the coordinator is a single point, rooms are not balanced or moved,
one zone is still one machine - is K1 to K8 in [`LIMITS.md`](LIMITS.md).

## Live ops: config, experiments, events and content without a new build

```powershell
cargo run -p jengine-server --release -- 0.0.0.0:9001 --admin-token a-long-secret --liveops liveops/
# then open http://127.0.0.1:9001/admin/liveops
```

(`Server::new().liveops(LiveOps::open("liveops")?)` in a game's own server.)
A shipped game asks `GET /liveops/v1/{channel}/snapshot` and is told, worked
out for that player:

- **Remote config** - parameters with rules: a platform, an app version
  range, named players or installs, a percentage rollout, a time window. The
  first rule that matches wins.
- **A/B experiments** - variants with weights; an install is bucketed by a
  hash of its id and stays in its variant, which overrides the parameters it
  names. The game is told which variant it is in.
- **Live events** - windows of time with values of their own, and packs that
  exist only while they run (a seasonal map), once or coming round again
  (`every` seconds - a weekend every week - until `until`; on the service's
  UTC clock). The dashboard draws them on a timeline; a game switches one on
  and off on time between asks, and asks again when one starts or ends.
- **The content release** - which asset packs are current. `jengine liveops
  publish <build> --server <url> --channel <c>` uploads a build's archives and
  publishes them; the service serves them at `/liveops/v1/packs/<file>`,
  named by their hash and cacheable forever, so a CDN can sit in front
  (`--liveops-cdn <url>`). A rollback publishes an older release again, as a
  new one.

The later wins: parameter, rule, variant, event. Each channel (`dev`,
`staging`, `live`) holds all four, and **Promote** copies any of them from one
channel to the next. Every change is a `liveops` line in the event log, by
operator, kept across restarts; a save made from a stale page is refused
(`409`) rather than overwriting somebody else's. The last fifty saves of a
channel's config, experiments and events are kept, and **Put back** saves an
earlier one again (`POST .../channel/{name}/restore`). After each release,
archives no kept release names are deleted once a day old. A snapshot shares
the `/api/` budget per address (`api_requests_per_second`; 429 past it).

**Every snapshot is signed** (Ed25519). The service makes its key the first
time it opens its folder (`signing.key` - back it up, and keep it secret) and
shows the public half on the dashboard and at `/liveops/v1/key`; a game is
built with it in `settings.liveops.public_key` and believes nothing it did not
sign. A content release can carry scripts, so this is what keeps the address,
a proxy or a CDN from being able to change what a game runs. A pack archive is
then checked against the SHA-256 the signed manifest gives.

**The key can be replaced without a new build.** Build games with a *root*
key kept off the serving machine (`jengine-server liveops-key new root.key`)
and have it endorse the service's own key for a while (`jengine-server
liveops-key endorse --root root.key --dir liveops/ --days 90`, which writes
`liveops/endorsement.json`): every snapshot carries the endorsement, a game
believes what the endorsed key signs until it runs out, and the dashboard
warns a fortnight before. A leaked serving key is good only until then; a
lost one is a new endorsement. A service already in use moves to it with no
new build: its `signing.key` becomes the root (move it off the machine) and
endorses a fresh one.

Rules and audiences can also ask the game's language (`ja` takes `ja-JP`),
region and attributes of its own (`&lang=`, `&region=`, `&a.<name>=` on the
snapshot URL, `snapshot_url_with`), and bucket a rollout or an experiment by
the signed-in player rather than the install (`by_player`). A game reports
what an install saw and did - `POST /liveops/v1/{channel}/exposure`
`{"experiment","event","value"}`, counted for the variant the service gave
it - and the results are at `/admin/api/liveops/channel/{name}/results`. An
operator added at runtime with `"channels": ["dev"]` may change only those.

The engine side is `jengine_core::liveops` and `LiveOps.*` in scripts (see the
scripting API). The game's own `defaults` are there from the first frame, the
last answer is kept and believed again at the next start, and the editor's
Play asks the `dev` channel. **[`LIMITS.md`](LIMITS.md) L2-L13** are what this
does not do yet - archives on one node's disk, no approval step, and an
engine that does not yet send a language or report an exposure.

## What is not here

**[`LIMITS.md`](LIMITS.md) is the full, numbered list** - everything the
server, the admin tools and the fleet do not do or have not been proved to,
with what each costs and what closing it takes. Add to it when you find one.
The ones that shape a decision today:

- **A game on it.** [`jengine-online`](../../packages/jengine-online/README.md)
  is the client half, and Test Lab scene 221 runs it against `examples/zones`
  in the native runtime. No game uses it yet, and it has not run in the
  editor or a web build.
- **Prediction is movement only.** Steering is predicted and reconciled
  (`jengine-netcode`), against walls too, and a hit can be judged in the
  past (`position_at`); a shot, a pickup or a spawn is not predicted. Right for a town, a co-op
  dungeon, an action game's movement; a twitch shooter needs the rest.
- **History of the numbers past a month.** The dashboard keeps six hours at
  ten seconds, a week of minutes and thirty days of hours, per node; a year
  or a cluster's sum is for Prometheus scraping `/metrics`.
- **Recovering an account by itself.** `PasswordAuth::recovery` makes and
  checks the code; delivering it - an email, a text - is the game's.
- **A Linux machine of its own.** The suites, the generated templates and a
  release build have run on Linux - in WSL, which shares the host's clock,
  disk and loopback - and a deploy has crossed from Windows to a Linux
  agent there. A real VPS, a systemd service and a slow link have not been
  seen. The Dockerfile builds and runs under Podman.
