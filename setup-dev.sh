#!/usr/bin/env bash
# One-time developer setup on macOS / Linux - the twin of setup-dev.ps1.
# Installs wasm-pack, the wasm32 target, the Tauri CLI and the editor's npm
# dependencies; on a Mac also the iOS targets, and checks Xcode.
#
#   ./setup-dev.sh            # install what is missing
#   ./setup-dev.sh --dry-run  # say what would be installed
. "$(dirname "$0")/tools/sh/common.sh"

DRY_RUN=0
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=1

run() {
    if (( DRY_RUN )); then warn "[dry-run] would run: $*"; else "$@"; fi
}

require rustup "Install Rust from https://rustup.rs/."
require cargo "Install Rust from https://rustup.rs/."
require node "Install Node.js 20+ (brew install node)."
require npm "Install Node.js 20+ (brew install node)."

node_major="$(node --version | sed -E 's/^v([0-9]+).*/\1/')"
(( node_major >= 20 )) || die "Node.js 20+ is required; found $(node --version)."
note "Node.js version: $(node --version)"
# CI runs Node 20 and the docs site's `npm ci` builds a native module
# (better-sqlite3, through TinaCMS) that does not compile against Node 26's
# V8. The editor is fine on newer Node; the website is not.
if (( node_major > 24 )); then
    warn "Node $(node --version) is newer than the website supports (its better-sqlite3 will not build)."
    warn "For website/, use an LTS Node: brew install node@22, then"
    warn "  PATH=/opt/homebrew/opt/node@22/bin:\$PATH npm ci   (in website/)"
fi

if command -v wasm-pack >/dev/null; then note "wasm-pack already installed."
else info "Installing wasm-pack..."; run cargo install wasm-pack --locked; fi

if command -v cargo-tauri >/dev/null; then note "cargo tauri already installed."
else info "Installing Tauri CLI..."; run cargo install tauri-cli --locked --version '^2.0.0'; fi

targets=(wasm32-unknown-unknown)
[[ "$(uname)" == "Darwin" ]] && targets+=(aarch64-apple-ios aarch64-apple-ios-sim)
installed="$(rustup target list --installed)"
for t in "${targets[@]}"; do
    if grep -qx "$t" <<<"$installed"; then note "Rust target $t already installed."
    else info "Installing Rust target $t..."; run rustup target add "$t"; fi
done

info "Installing editor npm dependencies..."
(cd "$REPO_ROOT/editor" && run npm ci)

if [[ "$(uname)" == "Darwin" ]]; then
    if [[ ! -d /Applications/Xcode.app ]]; then
        warn "Xcode.app not found. The editor and macOS builds need only the Command Line"
        warn "Tools, but iOS builds need Xcode (App Store)."
    elif ! xcrun simctl help >/dev/null 2>&1; then
        warn "Xcode is installed but not usable yet. Run once in a terminal:"
        warn "  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer"
        warn "  sudo xcodebuild -license accept"
        warn "  xcodebuild -runFirstLaunch"
    else
        note "Xcode: $(xcodebuild -version | head -1)"
        if ! xcrun simctl list runtimes | grep -q iOS; then
            warn "No iOS simulator runtime. Install one: xcodebuild -downloadPlatform iOS"
        fi
    fi
fi

echo
printf '\033[32m%s\033[0m\n' "Setup complete. You can now run ./run-editor.sh."
