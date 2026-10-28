# A Raspberry Pi as a game server host

Scripts that turn a Raspberry Pi on your LAN into a host for jEngine game
servers: every server behind one address, `ws://<pi-address>:9000/<service>`,
a list of them at `http://<pi-address>:9000/`, and a management console at
`http://<pi-address>:9000/manage`. They run from Windows (PowerShell, from the
repository root): build for the Pi, install the host, deploy, and the
day-to-day commands. The Pi needs nothing installed but a 64-bit OS with
systemd (Raspberry Pi OS or Ubuntu); a Pi 4 or 5 with 4 GB runs several
servers comfortably.

| File | What it is |
|---|---|
| `fleet.example.json` | A fleet file to copy to `fleet.json`: the Pi, a relay and a live ops service |
| `prepare-pi.sh` | Once, on the Pi with sudo: the account the servers run as |
| `setup-pi.ps1` | Install or upgrade the host on the Pi (runs `install-host.sh` there) |
| `deploy-pi.ps1` | Build the servers for the Pi and deploy them |
| `pi.ps1` | Day to day: status, logs, restart, console, live ops, backup |
| `common.ps1` | What the three share |
| `../tools/build-pi.ps1`, `build-pi.sh` | The cross-build (cargo-zigbuild; no WSL) |

## Before you start

- A Raspberry Pi with a **64-bit** OS that uses systemd, on your network,
  with SSH on, and your SSH key accepted by your own account there
  (`ssh you@<pi-address>` works without a password).
- A **fixed address** for the Pi in your router. Ubuntu Server does not
  announce `<name>.local` (Raspberry Pi OS does, as `gamebox.local` for a Pi
  named `gamebox`), so the scripts use whatever address the fleet file names.
- **Your fleet file.** Copy the example and put the Pi's address in it - the
  `pi` machine's `url` and `console` - or let `setup-pi.ps1 -Address` do it:

  ```powershell
  Copy-Item crates/jengine-server/pi/fleet.example.json crates/jengine-server/pi/fleet.json
  ```

  `fleet.json` is gitignored: it is yours. To keep it somewhere else (outside
  the repository, or one whose URLs go through an SSH tunnel), set
  `JENGINE_FLEET_FILE` to its path; every script here, and `build-pi.*`,
  read that file instead. Its paths are relative to the file itself.

## Once: an account for the servers (needs sudo, on the Pi)

```powershell
scp crates/jengine-server/pi/prepare-pi.sh you@<pi-address>:
ssh -t you@<pi-address> "sudo bash ~/prepare-pi.sh"
```

This makes the `jengine` account the host runs as. It has no sudo rights,
and your SSH keys can sign in as it. The host tool runs whatever builds are
uploaded to it, so this account is all a leaked token could reach. It also
turns on *linger*, so the host starts at boot with nobody logged in.

## Install, and upgrade

```powershell
./crates/jengine-server/pi/setup-pi.ps1 -Address 203.0.113.10   # the first time: your Pi's address
./crates/jengine-server/pi/setup-pi.ps1                         # after that
```

This builds `jengine-fleet` for the Pi and installs it as the systemd user
service `jengine-host`: the agent on 9900 (HTTPS), the gateway on 9000 and
the console at `/manage`. It also makes the tokens (`secrets.json` beside
these scripts, gitignored - it exists nowhere else, so back it up) and
fetches the Pi's certificate into `pi-cert.pem`, which the fleet file pins,
so nothing else is trusted as the Pi.

Run it again to upgrade the host tool; the servers stop cleanly and come
back. If the Pi gets a new address: `setup-pi.ps1 -Address <new address>`.

**Behind a TLS proxy.** A web build served over `https` cannot open a plain
`ws://` socket, so a public game puts a TLS proxy (nginx, say) in front of the
gateway. Tell the host to believe that proxy's `X-Forwarded-For`, or every
player looks like the proxy:

```powershell
./crates/jengine-server/pi/setup-pi.ps1 -TrustProxy 198.51.100.7   # comma-separate several
```

Without `-TrustProxy` the script takes `JENGINE_TRUST_PROXY`, else a
`"trust_proxy"` field on the `pi` machine in your fleet file (read by these
scripts only), else keeps what the host was installed with; `-TrustProxy
none` clears it. There is no default: with none named, the gateway believes
nobody's forwarded address.

## Deploy the game servers

```powershell
./crates/jengine-server/pi/deploy-pi.ps1                      # every service in the fleet file
./crates/jengine-server/pi/deploy-pi.ps1 -Services relay      # just these
./crates/jengine-server/pi/deploy-pi.ps1 -NoBuild             # what is already built
```

This builds for the Pi, deploys each service in the fleet file, and tells
the Pi's console about them. A deploy uploads only a build the Pi doesn't
already have. It drains the running server, stops it (its players are
saved), starts the new build, and puts the old one back if the new one never
passes its health check.

Players connect to `ws://<pi-address>:9000/<service>`; type that into a
game's server field. `http://<pi-address>:9000/` lists what's running.

The example runs two services, both the stock `jengine-server`:

| Service | Path | Local port on the Pi | What |
|---|---|---|---|
| `relay` | `/relay` | 9040 | rooms and one ordered log, for turn-based games; and input-lockstep rooms (`"kind": "lockstep"`, a `lockstep_session`) |
| `liveops` | `/liveops` | 9044 | remote config, experiments, events and releases |

**Your own game server** is one more entry: a free port, its build's path,
and its arguments (`127.0.0.1:{port}` first, so only the gateway reaches
it). Its name is its path. The build is a package in this workspace - one
made with `cargo run -p jengine-server -- new ...`, say - and the artifact's
file name is the package `build-pi` builds:

```json
"my-game": {
  "machines": ["pi"],
  "artifacts": { "linux-aarch64": "../../../target/aarch64-unknown-linux-gnu/release/my-game-server" },
  "args": ["127.0.0.1:{port}", "my-game.db"],
  "port": 9041,
  "admin_token_env": "PI_ADMIN_TOKEN",
  "drain_secs": 10
}
```

The deploy refuses a build for the wrong processor, so an x86-64 program
never reaches the Pi.

**From off the LAN** the agent's address (`192.168.50.x:9900`) is not
reachable, but the Pi's SSH is, if it is forwarded. Tunnel the agent and the
console, then deploy with a copy of the fleet file that points at the tunnel
(`JENGINE_FLEET_FILE`; give the copy absolute `artifacts` and `ca` paths,
since they are relative to the fleet file). The Pi's certificate also names
`127.0.0.1`, so the pinned TLS still verifies:

```powershell
ssh -N -L 9900:127.0.0.1:9900 -L 19000:127.0.0.1:9000 -p <ssh port> jengine@<public address>
# the copy: "url": "https://127.0.0.1:9900", "console": "http://127.0.0.1:19000/manage"
$env:JENGINE_FLEET_FILE = "<the copy>"; ./crates/jengine-server/pi/deploy-pi.ps1 -Services my-game -NoBuild
```

## Day to day

```powershell
./crates/jengine-server/pi/pi.ps1 status              # every service: version, players, uptime
./crates/jengine-server/pi/pi.ps1 logs relay
./crates/jengine-server/pi/pi.ps1 restart relay       # also stop, start, rollback
./crates/jengine-server/pi/pi.ps1 console             # the console in your browser, signed in
./crates/jengine-server/pi/pi.ps1 open                # the list players see
./crates/jengine-server/pi/pi.ps1 host-log            # the host tool's own log
./crates/jengine-server/pi/pi.ps1 ssh                 # a shell as jengine
./crates/jengine-server/pi/pi.ps1 tokens              # the console token, to sign in from a phone
./crates/jengine-server/pi/pi.ps1 backup              # the live ops key and tokens, to ~/jengine-pi-backup
```

The console opens on an overview: players, servers, errors and game traffic
over the last hour or day, then the Pi itself (CPU, memory, disk, temperature,
network) with charts of each, and the latest errors and warnings from the
servers' logs. Below it, any server's own dashboard opens inside the console
(rooms, players, notices, drain, accounts, live ops, the game's API routes),
signed in by the console.

To use the console from another device (a phone), open
`http://<pi-address>:9000/manage` and sign in with the console token
(`pi.ps1 tokens` prints it).

## Live ops

The example's `liveops` service is the stock `jengine-server --liveops`, at
`http://<pi-address>:9000/liveops`. It handles remote config, A/B
experiments, live events and content releases.

```powershell
./crates/jengine-server/pi/pi.ps1 liveops                             # what a game is built with
./crates/jengine-server/pi/pi.ps1 publish <game build> -Channel dev   # its asset packs, as a release
```

To edit it, open the console, then Server dashboards → `host / liveops` →
**Live ops**. A game believes only what this service signs, with the key
`pi.ps1 liveops` prints. That key lives on the Pi in
`~jengine/jengine/data/services/liveops/liveops/signing.key`. **Back it
up:** a new key means rebuilding every game.

```powershell
./crates/jengine-server/pi/pi.ps1 backup              # or: pi.ps1 backup D:\somewhere-private
```

That copies the key, `secrets.json` and the Pi's certificate into a dated
set of files. Keep the folder private: the key signs what every game
believes, and the tokens deploy programs to the Pi. To restore the key on a
new Pi, deploy `liveops` once, copy the key over the new one with `scp`,
then run `pi.ps1 restart liveops`.

## What is where on the Pi

```text
~jengine/jengine/jengine-fleet       the host tool
~jengine/jengine/host.env            its tokens (600)
~jengine/jengine/tls/                its certificate and key
~jengine/jengine/data/               releases, each service's folder (its database, its log)
~jengine/jengine/data/console/       the console's fleet file, jobs, uploaded builds
~jengine/.config/systemd/user/jengine-host.service
```

## Limits

- **The gateway and console are plain HTTP** (H2 in `../LIMITS.md`). That's
  fine on a home LAN, but anyone on the network who can see the traffic can
  read the console token. The agent (9900) is HTTPS.
- **No Mac/Linux twins** of these scripts yet; `../tools/build-pi.sh` covers
  the build.
- **Checked by hand on one Pi.** The scripts have been run end to end on a Pi
  with 4 GB running Ubuntu 26.04; on another OS or board, watch the first `setup-pi.ps1` run.
