#!/usr/bin/env bash
# Build the stock viewport WASM for the editor - the twin of build-viewport.ps1.
#   ./build-viewport.sh [--dev]
. "$(dirname "$0")/tools/sh/common.sh"

require_wasm_toolchain
profile=--release
[[ "${1:-}" == "--dev" ]] && profile=--dev
build_stock_viewport "$profile"
info "Viewport WASM built to editor/src/lib/viewport-wasm/"
