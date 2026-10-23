# Bundle the Fengari Lua runtime for deployed web builds.
#
# `editor/src/lib/lua-runtime.ts` is the one implementation of the browser
# Lua VM: the editor imports it directly through Vite, and web builds ship
# this bundle of the same source. Rebuild after changing that file — the
# committed artifact is what `jengine-builder` embeds.
$ErrorActionPreference = 'Stop'

$out = "crates/jengine-builder/assets/lua-runtime.js"

Push-Location editor
try {
	if (-not (Test-Path "node_modules")) {
		throw "editor/node_modules is missing. Run 'npm install' in editor/ first."
	}
	# esbuild reports on stderr even when it succeeds, and under Windows
	# PowerShell 5.1 with `Stop` that is a terminating error - the build
	# looked failed while the bundle had been written. Judge by the exit code.
	$ErrorActionPreference = 'Continue'
	npx esbuild src/lib/lua-runtime.ts --bundle --format=iife --minify --target=es2020 --outfile="../$out" 2>&1 | ForEach-Object { "$_" } | Where-Object { $_.Trim() -and $_ -ne 'System.Management.Automation.RemoteException' }
	$ErrorActionPreference = 'Stop'
	if ($LASTEXITCODE -ne 0) {
		throw "esbuild failed with exit code $LASTEXITCODE."
	}
}
finally {
	Pop-Location
}

$size = [math]::Round((Get-Item $out).Length / 1KB)
Write-Host "Lua runtime bundled to $out ($size KB)"
