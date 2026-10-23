# Shared helpers for the macOS / Linux shell scripts at the repo root.
# The PowerShell scripts beside them are the Windows twins; keep the two
# sets doing the same thing.
#
# Sourced, not run: `. "$(dirname "$0")/tools/sh/common.sh"`.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"

# A Mac whose `xcode-select` still points at the Command Line Tools cannot
# reach `simctl`, the iOS SDK or Metal's shader tools, while Xcode.app may be
# installed beside them. Use Xcode for this process when it is there, rather
# than making every script depend on a `sudo xcode-select -s` - but only once
# its license is accepted: an unaccepted Xcode fails every `cc` link, which
# the Command Line Tools would have done fine.
XCODE_DEV=/Applications/Xcode.app/Contents/Developer
if [[ "$(uname)" == "Darwin" && -z "${DEVELOPER_DIR:-}" ]]; then
    if [[ "$(xcode-select -p 2>/dev/null)" == "/Library/Developer/CommandLineTools" && -d "$XCODE_DEV" ]] \
          && DEVELOPER_DIR="$XCODE_DEV" xcodebuild -license check >/dev/null 2>&1; then
        export DEVELOPER_DIR="$XCODE_DEV"
    fi
fi

info() { printf '\033[36m%s\033[0m\n' "$*"; }
note() { printf '\033[90m%s\033[0m\n' "$*"; }
warn() { printf '\033[33m%s\033[0m\n' "$*"; }
die()  { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

require() {
    command -v "$1" >/dev/null 2>&1 || die "$1 is not installed or not on PATH. $2"
}

require_wasm_toolchain() {
    require wasm-pack "Run ./setup-dev.sh first."
    rustup target list --installed | grep -q '^wasm32-unknown-unknown$' \
        || die "Rust target wasm32-unknown-unknown is not installed. Run 'rustup target add wasm32-unknown-unknown' first."
}

VIEWPORT_DIR="$REPO_ROOT/editor/src/lib/viewport-wasm"
STAMP_PATH="$VIEWPORT_DIR/.jengine-viewport.json"

# Build the stock viewport and stamp it as such. The viewport directory is
# shared with project builds and the editor reads the stamp to decide whether
# a project's packages are linked - a project stamp left beside a stock module
# makes it trust a package that silently does nothing. No BOM: JSON.parse
# rejects one (printf writes none).
build_stock_viewport() {
    local profile="${1:---release}"
    if [[ "$profile" == "--release" ]]; then
        # The builder's stock build: wasm-pack, then wasm-opt at the editor's
        # preferred level, the cleanup and the stamp - the same code as the
        # editor's Rebuild viewport. Pass a level as $2 to override it.
        (cd "$REPO_ROOT" && cargo run --quiet -p jengine-builder --bin jengine -- \
            viewport-build --stock ${2:+--wasm-opt "$2"})
        return
    fi
    (cd "$REPO_ROOT" && wasm-pack build crates/jengine-editor-viewport --target web "$profile" \
        --out-dir ../../editor/src/lib/viewport-wasm)
    rm -f "$VIEWPORT_DIR/package.json" "$VIEWPORT_DIR/.gitignore"
    printf '%s' '{ "project": "", "packages": [], "stock": true }' > "$STAMP_PATH"
}
