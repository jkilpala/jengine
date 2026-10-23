# Build the editor viewport WASM *with a project's own Rust packages linked*.
#
# The viewport is one prebuilt WASM module shared by every project, so a
# project's package cannot be linked into it ahead of time. jengine-builder
# generates a wrapper crate at <project>/.jengine/viewport/ that re-exports the
# viewport and registers the project's packages at module init, builds it into
# <project>/.jengine/viewport/pkg/ and copies that over
# editor/src/lib/viewport-wasm/ - a drop-in replacement, so the editor's imports
# are unchanged.
#
# Each project keeps its build, so by default this only COPIES it in when
# nothing it was compiled from has changed (`jengine viewport-use`). -Force
# compiles regardless (`jengine viewport-build`).
#
# The work itself lives in `jengine_builder::viewport`, so this script, the
# `jengine viewport-build` CLI command and the editor's "Rebuild viewport"
# button all run the same code. A package that behaves differently depending on
# how it was built is exactly the confusion this feature removes.
#
# A project with no Rust packages needs none of this - the stock viewport
# already carries every built-in package, and the command says so and stops.
#
# Usage:
#   ./build-project-viewport.ps1 -Project projects/engine-test-lab [-Force]
param(
    [Parameter(Mandatory = $true)][string]$Project,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

if (-not (Get-Command wasm-pack -ErrorAction SilentlyContinue)) {
    throw "wasm-pack is not installed or not on PATH. Install it first, then rerun this script."
}

# `rustup target list --installed` returns an array, and `-match` / `-notmatch`
# filter arrays rather than testing them. Join first - see build-viewport.ps1.
$installedTargets = (rustup target list --installed) -join "`n"
if ($installedTargets -notmatch 'wasm32-unknown-unknown') {
    throw "Rust target wasm32-unknown-unknown is not installed. Run 'rustup target add wasm32-unknown-unknown' first."
}

$projectPath = (Resolve-Path $Project).Path

$command = if ($Force) { 'viewport-build' } else { 'viewport-use' }
# Cargo reports warnings and progress on stderr even when it succeeds - on a
# drive without hard links (exFAT) every incremental build warns - and under
# Windows PowerShell 5.1 with `Stop` a redirected stderr line is a
# terminating error. Judge by the exit code, as build-lua-runtime.ps1 does.
$ErrorActionPreference = 'Continue'
cargo run --quiet -p jengine-builder --bin jengine -- $command $projectPath
$ErrorActionPreference = 'Stop'
if ($LASTEXITCODE -ne 0) {
    throw "viewport build failed with exit code $LASTEXITCODE."
}

Write-Host ""
Write-Host "NOTE: the editor now carries THIS project's packages. Opening another"
Write-Host "      project loads that project's own cached build, or offers a rebuild"
Write-Host "      if it has none or its sources changed since."
