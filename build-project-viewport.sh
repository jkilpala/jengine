#!/usr/bin/env bash
# Build the editor viewport with a project's own Rust packages linked - the
# twin of build-project-viewport.ps1, and the same code underneath
# (`jengine viewport-use` / `viewport-build`, `jengine_builder::viewport`).
# By default it only copies the project's cached build in when nothing it was
# compiled from changed; --force compiles regardless.
#
#   ./build-project-viewport.sh projects/engine-test-lab [--force]
. "$(dirname "$0")/tools/sh/common.sh"

[[ $# -ge 1 ]] || die "usage: $0 <project> [--force]"
project="$(cd "$1" && pwd)"
command=viewport-use
[[ "${2:-}" == "--force" ]] && command=viewport-build

require_wasm_toolchain
(cd "$REPO_ROOT" && cargo run --quiet -p jengine-builder --bin jengine -- "$command" "$project")

echo
echo "NOTE: the editor now carries THIS project's packages. Opening another"
echo "      project loads that project's own cached build, or offers a rebuild"
echo "      if it has none or its sources changed since."
