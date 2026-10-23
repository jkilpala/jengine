# Build the jEngine Editor (debug build)
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
Set-Location $PSScriptRoot

# Step 1: Build the viewport WASM module
Write-Host "=== Building viewport WASM ===" -ForegroundColor Cyan
& .\build-viewport.ps1
if ($LASTEXITCODE -ne 0) {
    Write-Host "Viewport WASM build failed!" -ForegroundColor Red
    exit 1
}

# Step 2: Build the Tauri app
Write-Host "=== Building Tauri editor (debug) ===" -ForegroundColor Cyan
cargo tauri build --debug
if ($LASTEXITCODE -ne 0) {
    Write-Host "Tauri build failed!" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "=== Build complete ===" -ForegroundColor Green
# CARGO_TARGET_DIR moves cargo's output; the version is the workspace's.
$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $PSScriptRoot "target" }
$version = (Select-String -Path (Join-Path $PSScriptRoot "Cargo.toml") -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
Write-Host "Executable: $(Join-Path $targetDir 'debug\jengine-editor.exe')"
Write-Host "Installer:  $(Join-Path $targetDir "debug\bundle\nsis\jengine-editor_${version}_x64-setup.exe")"
