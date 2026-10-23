#!/usr/bin/env bash
# On the Pi, as `jengine` - run by setup-pi.ps1, which has just copied the
# files it names into ~/jengine/incoming/. No sudo: the host is a systemd
# *user* service, kept running at boot by linger (prepare-pi.sh turned it on).
#
#   bash install-host.sh <address the Pi is reached at> [<trusted proxies>|none]
#
# Installs or upgrades the host tool (`jengine-fleet host`: the agent on 9900
# over HTTPS, the gateway on 9000, the console at /manage), makes its TLS
# certificate the first time, and (re)starts it. The game servers it runs are
# stopped cleanly by the restart and started again by it.
set -euo pipefail
address="${1:?usage: install-host.sh <address> [<trusted proxies>|none]}"
root="$HOME/jengine"
incoming="$root/incoming"
unit="$HOME/.config/systemd/user/jengine-host.service"
# The TLS proxy (or proxies, comma-separated) in front of the gateway - an
# nginx serving wss:// for web builds, say - believed about which player it
# carries, so the servers tell players apart by their own addresses rather
# than all being the proxy's. The second argument, else JENGINE_TRUST_PROXY,
# else what the installed host was started with (an upgrade keeps it);
# `none` trusts no proxy. There is no default: with none, the gateway
# believes nobody's X-Forwarded-For.
trust="${2:-${JENGINE_TRUST_PROXY:-}}"
if [ -z "$trust" ] && [ -f "$unit" ]; then
    trust="$(grep -o -- '--trust-proxy [^ ]*' "$unit" | head -1 | cut -d' ' -f2 || true)"
fi
if [ "$trust" = "none" ]; then
    trust=""
fi
if [ -n "$trust" ] && ! [[ "$trust" =~ ^[0-9A-Fa-f:.,/]+$ ]]; then
    echo "not a list of addresses to trust: $trust" >&2
    exit 1
fi
trust_flag=""
if [ -n "$trust" ]; then
    trust_flag="--trust-proxy $trust"
    echo "trusting the proxy at $trust"
fi
mkdir -p "$root/data" "$root/tls"

# The program, swapped in whole: a running binary is replaced by rename.
if [ -f "$incoming/jengine-fleet" ]; then
    install -m 755 "$incoming/jengine-fleet" "$root/jengine-fleet.new"
    mv -f "$root/jengine-fleet.new" "$root/jengine-fleet"
fi
# The tokens, readable by this account only.
if [ -f "$incoming/host.env" ]; then
    install -m 600 "$incoming/host.env" "$root/host.env"
fi
rm -rf "$incoming"
[ -x "$root/jengine-fleet" ] || { echo "no host tool in $root" >&2; exit 1; }
[ -f "$root/host.env" ] || { echo "no $root/host.env" >&2; exit 1; }

# A certificate naming every address the agent is reached at: this one, the
# machine's name, and loopback - which is how its own console reaches it.
# Made once; the Windows side pins exactly this certificate.
if [ ! -f "$root/tls/cert.pem" ] || ! grep -q "^$address\$" "$root/tls/names" 2>/dev/null; then
    (cd "$root/tls" && "$root/jengine-fleet" cert "$address" "$(hostname)" "$(hostname).local" 127.0.0.1 localhost --out .)
    chmod 600 "$root/tls/key.pem"
    printf '%s\n' "$address" "$(hostname)" 127.0.0.1 > "$root/tls/names"
    echo "made a certificate for $address"
fi

# An agent reachable from the LAN runs only releases signed by its release
# key (`jengine-fleet keygen`; JENGINE_RELEASE_KEY=jfr-public-.. in host.env,
# and "release_key_file" in the fleet file that deploys here). Without one in
# host.env it is told plainly that its token alone may run programs.
release_flag="--allow-unsigned"
if grep -q '^JENGINE_RELEASE_KEY=jfr-public-' "$root/host.env"; then
    release_flag=""
fi
# The gateway is plain on this LAN (native clients, and any TLS proxy in
# front of it for web builds), so the console's token would cross the LAN readable: it is
# taken that way only because this says the LAN is trusted. Give the host
# --gateway-tls-cert/--gateway-tls-key instead to drop this.
console_flag="--allow-plain-console"

mkdir -p "$(dirname "$unit")"
cat > "$unit" <<EOF
[Unit]
Description=jEngine game server host (agent, gateway, console)
After=network-online.target
Wants=network-online.target

[Service]
EnvironmentFile=$root/host.env
WorkingDirectory=$root
ExecStart=$root/jengine-fleet host --listen 0.0.0.0:9900 --gateway 0.0.0.0:9000 --data $root/data --tls-cert $root/tls/cert.pem --tls-key $root/tls/key.pem $trust_flag $release_flag $console_flag
Restart=always
RestartSec=2
# Long enough for every server to drain and save before it is killed.
KillSignal=SIGTERM
TimeoutStopSec=120

[Install]
WantedBy=default.target
EOF

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
systemctl --user daemon-reload
systemctl --user enable jengine-host.service >/dev/null 2>&1
systemctl --user restart jengine-host.service

# Up when its agent answers; it is HTTPS with our own certificate.
for _ in $(seq 1 50); do
    if curl -s --cacert "$root/tls/cert.pem" -o /dev/null "https://127.0.0.1:9900/v1/info" 2>/dev/null \
        || curl -sk -o /dev/null "https://127.0.0.1:9900/v1/info"; then
        break
    fi
    sleep 0.2
done
systemctl --user --no-pager --lines=0 status jengine-host.service | head -3
linger=$(loginctl show-user "$USER" -p Linger --value 2>/dev/null || echo "?")
if [ "$linger" != "yes" ]; then
    echo "warning: linger is off, so the host stops when you log out and does not start at boot." >&2
    echo "         As your own account: sudo loginctl enable-linger $USER" >&2
fi
