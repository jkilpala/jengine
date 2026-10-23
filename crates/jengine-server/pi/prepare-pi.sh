#!/usr/bin/env bash
# Once, on the Pi, as you (it needs sudo):  sudo bash prepare-pi.sh
#
# Makes the account the game servers run as - `jengine`, with no sudo of its
# own - so the host tool, which runs whatever builds are uploaded to it, can
# reach that account and nothing else. Everything after this (installing the
# host tool, upgrading it, deploying servers) is done as `jengine` over SSH by
# setup-pi.ps1 / deploy-pi.ps1, with no sudo.
#
#   - creates the user `jengine` (a home, a shell for SSH, no password)
#   - lets your SSH keys in as it (copies the invoking user's authorized_keys)
#   - turns on "linger", so its services start at boot with nobody logged in
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    echo "run it with sudo: sudo bash $0" >&2
    exit 1
fi
owner="${SUDO_USER:-}"
if [ -z "$owner" ] || [ "$owner" = "root" ]; then
    echo "run it with sudo from your own account, so your SSH keys can be copied" >&2
    exit 1
fi

if id jengine >/dev/null 2>&1; then
    echo "the user jengine is there already"
else
    useradd --create-home --shell /bin/bash --comment "jEngine game servers" jengine
    passwd --lock jengine >/dev/null
    echo "made the user jengine (no password, no sudo)"
fi

home=$(getent passwd jengine | cut -d: -f6)
install -d -m 700 -o jengine -g jengine "$home/.ssh"
src="$(getent passwd "$owner" | cut -d: -f6)/.ssh/authorized_keys"
if [ -f "$src" ]; then
    touch "$home/.ssh/authorized_keys"
    # Add the keys it does not have yet; keep any it has.
    sort -u "$home/.ssh/authorized_keys" "$src" -o "$home/.ssh/authorized_keys"
    chown jengine:jengine "$home/.ssh/authorized_keys"
    chmod 600 "$home/.ssh/authorized_keys"
    echo "your SSH keys ($src) can sign in as jengine"
else
    echo "warning: $owner has no $src - add a key to $home/.ssh/authorized_keys yourself" >&2
fi

loginctl enable-linger jengine
echo "linger: $(loginctl show-user jengine -p Linger --value 2>/dev/null || echo '?')"
echo
echo "Done. Back on Windows: ./crates/jengine-server/pi/setup-pi.ps1"
