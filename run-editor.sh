#!/usr/bin/env bash
# The editor's dev loop on macOS / Linux - the twin of run-editor.ps1:
# rebuild the viewport WASM so its JS glue matches the current Rust, then
# `cargo tauri dev`.
#
#   ./run-editor.sh                  # viewport for whichever project the stamp names
#   ./run-editor.sh --project <path> # ... for this project
#   ./run-editor.sh --stock          # the stock viewport, dropping project packages
#   ./run-editor.sh --dev-wasm       # unoptimized viewport - never judge frame rate on it
#   ./run-editor.sh --clean          # cargo clean -p jengine-core first (slow)
. "$(dirname "$0")/tools/sh/common.sh"

CLEAN=0 DEV_WASM=0 STOCK=0 PROJECT=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --clean) CLEAN=1 ;;
        --dev-wasm) DEV_WASM=1 ;;
        --stock) STOCK=1 ;;
        --project) PROJECT="$2"; shift ;;
        *) die "unknown argument: $1" ;;
    esac
    shift
done

cd "$REPO_ROOT"
require_wasm_toolchain
require cargo-tauri "Run ./setup-dev.sh first."
[[ -d editor/node_modules ]] || die "editor/node_modules is missing. Run ./setup-dev.sh first."

# A stale Vite from a previous session holds port 5173; Tauri would then
# fall back to another port while devUrl still points at 5173. Kill it.
pkill -x jengine-editor 2>/dev/null || true
for pid in $(lsof -ti tcp:5173 2>/dev/null); do
    warn "Killing stale process on port 5173 (PID $pid)..."
    kill -9 "$pid" 2>/dev/null || true
done

if (( CLEAN )); then
    info "Cleaning jengine-core..."
    cargo clean -p jengine-core
fi

# Which project's packages should the viewport carry? Defaults to the one the
# stamp names, so a project viewport survives a relaunch (see run-editor.ps1
# for the history of that).
viewport_project=""
if (( STOCK )); then
    warn "--stock: building the stock viewport, dropping any project packages."
elif [[ -n "$PROJECT" ]]; then
    viewport_project="$(cd "$PROJECT" && pwd)"
elif [[ -f "$STAMP_PATH" ]]; then
    # An unreadable stamp must not stop the editor launching: fall back to stock.
    stamped="$(node -e '
        try {
            const s = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8").replace(/^﻿/, ""));
            if (!s.stock && s.project) console.log(s.project);
        } catch (e) {}' "$STAMP_PATH")"
    if [[ -n "$stamped" ]]; then
        if [[ -d "$stamped" ]]; then
            viewport_project="$stamped"
        else
            warn "Viewport was built for '$stamped', which does not exist here - building stock."
        fi
    fi
fi

if [[ -n "$viewport_project" ]]; then
    (( DEV_WASM )) && warn "--dev-wasm ignored: a project viewport is release-only."
    info "Loading editor viewport WASM with the packages of '$viewport_project'..."
    note "  (pass --stock for the stock viewport, or --project <path> for another project)"
    "$REPO_ROOT/build-project-viewport.sh" "$viewport_project"
elif (( DEV_WASM )); then
    warn "Building editor viewport WASM (DEV - unoptimized; frame rates NOT representative)..."
    build_stock_viewport --dev
else
    info "Building editor viewport WASM (release)..."
    build_stock_viewport --release
fi

cd "$REPO_ROOT/editor"
exec cargo tauri dev
