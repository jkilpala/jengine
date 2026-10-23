# Build the jEngine Editor for release: the installers (NSIS setup .exe and
# .msi, from `cargo tauri build`) and a portable zip. The editor's .exe carries
# its frontend and needs WebView2, which Windows 10 and 11 ship. Beside it go
# its resources (`tauri.release.conf.json`): the stock viewport module, which
# the built frontend imports at run time (`editor/src-tauri/src/viewport_module.rs`),
# and the engine's source, which builds compile from when the user has no
# engine checkout (`jengine_builder::engine_root`).
#
# The viewport is built at the editor preference's wasm-opt level (Project
# Settings > Graphics > Viewport build); a release should be `optimized`:
#   ./build-editor-release.ps1 -WasmOpt optimized
#
# The engine source is this checkout's - unless this is the monorepo the
# engine is developed in (it has tools/release/export.py), whose source is
# exported first so the installer ships what the public repository holds.
# -EngineSource <dir> names an engine tree to ship instead.
param([string]$WasmOpt = "", [string]$EngineSource = "")

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
Set-Location $PSScriptRoot

# CARGO_TARGET_DIR moves cargo's output; the version is the workspace's.
$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $PSScriptRoot "target" }
$version = (Select-String -Path (Join-Path $PSScriptRoot "Cargo.toml") -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$resources = Join-Path $PSScriptRoot "editor\src-tauri\release-resources"

# Step 1: Build the viewport WASM module
Write-Host "=== Building viewport WASM ===" -ForegroundColor Cyan
if ($WasmOpt) { & .\build-viewport.ps1 -WasmOpt $WasmOpt } else { & .\build-viewport.ps1 }
if ($LASTEXITCODE -ne 0) {
    Write-Host "Viewport WASM build failed!" -ForegroundColor Red
    exit 1
}

# Step 2: Stage the resources - the stock viewport and the engine source.
Write-Host "=== Staging the viewport and the engine source ===" -ForegroundColor Cyan
Remove-Item $resources -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $resources "viewport") | Out-Null
foreach ($f in "jengine_editor_viewport.js", "jengine_editor_viewport_bg.wasm", ".jengine-viewport.json") {
    Copy-Item (Join-Path $PSScriptRoot "editor\src\lib\viewport-wasm\$f") (Join-Path $resources "viewport\$f")
}
if (-not $EngineSource) {
    if (Test-Path (Join-Path $PSScriptRoot "tools\release\export.py")) {
        $EngineSource = Join-Path $targetDir "engine-export"
        Write-Host "Exporting the public engine tree (HEAD) to $EngineSource"
        python tools/release/export.py --out $EngineSource
        if ($LASTEXITCODE -ne 0) {
            Write-Host "Export failed - nothing private may ship in an installer." -ForegroundColor Red
            exit 1
        }
    } else {
        $EngineSource = $PSScriptRoot
    }
}
# Cargo warns on stderr even when it succeeds; judge it by its exit code.
$ErrorActionPreference = 'Continue'
cargo run --quiet -p jengine-builder --bin jengine -- engine-bundle --from $EngineSource --out (Join-Path $resources "engine-source.zip")
if ($LASTEXITCODE -ne 0) {
    Write-Host "Packing the engine source failed!" -ForegroundColor Red
    exit 1
}

# Step 3: Build the Tauri app (release) and its installers
Write-Host "=== Building Tauri editor (release) ===" -ForegroundColor Cyan
cargo tauri build --config editor/src-tauri/tauri.release.conf.json
if ($LASTEXITCODE -ne 0) {
    Write-Host "Tauri build failed!" -ForegroundColor Red
    exit 1
}

$exe = Join-Path $targetDir "release\jengine-editor.exe"
$bundleDir = Join-Path $targetDir "release\bundle"

# Step 4: The portable zip - the executable, its resources laid out as the
# installer lays them out (beside it), its licence and the notices of what it
# is built on.
Write-Host "=== Packing the portable zip ===" -ForegroundColor Cyan
$portableDir = Join-Path $bundleDir "portable"
$staging = Join-Path $portableDir "jEngine Editor $version"
Remove-Item $portableDir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $staging | Out-Null
Copy-Item $exe (Join-Path $staging "jengine-editor.exe")
Copy-Item (Join-Path $resources "*") $staging -Recurse -Force
Copy-Item (Join-Path $PSScriptRoot "LICENSE") $staging
Copy-Item (Join-Path $PSScriptRoot "THIRD-PARTY-NOTICES.md") $staging
$zip = Join-Path $portableDir "jEngine-Editor_${version}_x64-portable.zip"
Compress-Archive -Path $staging -DestinationPath $zip -Force
Remove-Item $staging -Recurse -Force

Write-Host ""
Write-Host "=== Release build complete ===" -ForegroundColor Green
Write-Host "Executable: $exe"
Get-ChildItem $bundleDir -Recurse -Include *.exe, *.msi, *.zip | ForEach-Object {
    Write-Host ("  {0}  ({1:N1} MB)" -f $_.FullName, ($_.Length / 1MB))
}
