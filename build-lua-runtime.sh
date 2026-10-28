#!/usr/bin/env bash
# Bundle the Fengari Lua runtime for deployed web builds - the twin of
# build-lua-runtime.ps1. Rebuild after changing editor/src/lib/lua-runtime.ts.
. "$(dirname "$0")/tools/sh/common.sh"

out="crates/jengine-builder/assets/lua-runtime.js"
[[ -d "$REPO_ROOT/editor/node_modules" ]] || die "editor/node_modules is missing. Run 'npm install' in editor/ first."
# Fengari's `%` and `^` by Luau's rules first (a no-op once done).
(cd "$REPO_ROOT/editor" && node scripts/patch-fengari.mjs) || die "patching fengari-web failed"
(cd "$REPO_ROOT/editor" && npx esbuild src/lib/lua-runtime.ts --bundle --format=iife --minify \
    --target=es2020 --outfile="../$out")
info "Lua runtime bundled to $out ($(( $(wc -c < "$REPO_ROOT/$out") / 1024 )) KB)"
