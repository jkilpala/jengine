# Build the viewport WASM module for the editor.
#
# The build lives in `jengine_builder::viewport::build_stock` (wasm-pack, the
# wasm-opt pass at the editor's preferred level, the cleanup and the stock
# stamp), so this script, run-editor and the editor agree. Pass a level to
# override the preference (Project Settings > Graphics > Viewport build):
#   ./build-viewport.ps1 -WasmOpt fast      # off | fast | optimized | size | max
param([string]$WasmOpt = "")

$ErrorActionPreference = 'Stop'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

if (-not (Get-Command wasm-pack -ErrorAction SilentlyContinue)) {
	throw "wasm-pack is not installed or not on PATH. Install it first, then rerun this script."
}

# `rustup target list --installed` returns an array, and `-match` / `-notmatch`
# act as *filters* on arrays rather than boolean tests — `-notmatch` would
# return the other installed targets (truthy) and throw spuriously. Test the
# joined string instead.
$installedTargets = (rustup target list --installed) -join "`n"
if ($installedTargets -notmatch 'wasm32-unknown-unknown') {
	throw "Rust target wasm32-unknown-unknown is not installed. Run 'rustup target add wasm32-unknown-unknown' first."
}

$optArgs = if ($WasmOpt) { @("--wasm-opt", $WasmOpt) } else { @() }
# Cargo warns on stderr even when it succeeds (on exFAT, every incremental
# build), which `Stop` turns into a terminating error once output is
# redirected. Judge by the exit code - see build-lua-runtime.ps1.
$ErrorActionPreference = 'Continue'
cargo run --quiet -p jengine-builder --bin jengine -- viewport-build --stock @optArgs
$ErrorActionPreference = 'Stop'
if ($LASTEXITCODE -ne 0) {
	throw "viewport build failed with exit code $LASTEXITCODE."
}
Write-Host "Viewport WASM built to editor/src/lib/viewport-wasm/"
