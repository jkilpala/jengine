#!/usr/bin/env bash
# Build the server side for a Raspberry Pi (64-bit): jengine-fleet, the stock
# jengine-server and the game servers, cross-compiled with cargo-zigbuild.
# The twin of build-pi.ps1.
#
#   ./crates/jengine-server/tools/build-pi.sh                     # the fleet file's servers
#   ./crates/jengine-server/tools/build-pi.sh my-game-server      # just these packages
#   ./crates/jengine-server/tools/build-pi.sh --none              # only jengine-fleet and jengine-server
#
# With no arguments, the game servers are the workspace packages named by the
# `linux-aarch64` artifacts in the Pi's fleet file (pi/fleet.json, or the file
# JENGINE_FLEET_FILE names) - an artifact's file name is its package. Reading
# it needs python3; without it, only the two tools are built.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
target=aarch64-unknown-linux-gnu
glibc="${GLIBC:-2.31}"
servers=()
if [ $# -eq 1 ] && [ "$1" = "--none" ]; then
    :
elif [ $# -gt 0 ]; then
    servers=("$@")
else
    fleet="${JENGINE_FLEET_FILE:-$here/../pi/fleet.json}"
    if [ -f "$fleet" ] && command -v python3 >/dev/null; then
        while IFS= read -r s; do
            [ -n "$s" ] && servers+=("$s")
        done < <(python3 - "$fleet" <<'EOF'
import json, os, sys
fleet = json.load(open(sys.argv[1], encoding="utf-8"))
names = {os.path.basename(s.get("artifacts", {}).get("linux-aarch64", "").replace("\\", "/"))
         for s in fleet.get("services", {}).values()}
print("\n".join(sorted(n for n in names if n)))
EOF
)
        echo "servers named by $fleet: ${servers[*]:-none}"
    else
        echo "no fleet file at $fleet (or no python3) - building jengine-fleet and jengine-server only"
    fi
fi

rustup target list --installed | grep -q "$target" || rustup target add "$target"
command -v cargo-zigbuild >/dev/null || cargo install --locked cargo-zigbuild
if ! command -v zig >/dev/null && ! python3 -c "import ziglang" 2>/dev/null; then
    python3 -m pip install --user ziglang
fi

cd "$root"
cargo zigbuild --release --target "$target.$glibc" -p jengine-server --bins
packages=()
for s in ${servers[@]+"${servers[@]}"}; do
    case "$s" in
        jengine-fleet|jengine-server) ;;  # built above
        *) packages+=(-p "$s") ;;
    esac
done
if [ ${#packages[@]} -gt 0 ]; then
    cargo zigbuild --release --target "$target.$glibc" "${packages[@]}"
fi

echo
echo "Built for the Pi, in target/$target/release:"
for exe in jengine-fleet jengine-server ${servers[@]+"${servers[@]}"}; do
    [ -f "target/$target/release/$exe" ] && ls -lh "target/$target/release/$exe" | awk '{print "  " $9 "  " $5}'
done
echo
echo "Install or upgrade the host: crates/jengine-server/pi/setup-pi.ps1"
echo "Deploy the servers:         crates/jengine-server/pi/deploy-pi.ps1 -NoBuild"
