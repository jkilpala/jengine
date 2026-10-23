param(
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

function Assert-Command {
    param(
        [string]$Name,
        [string]$Hint
    )

    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "$Name is not installed or not on PATH. $Hint"
    }
}

function Install-WasmPack {
    if (Get-Command wasm-pack -ErrorAction SilentlyContinue) {
        Write-Host "wasm-pack already installed." -ForegroundColor DarkGray
        return
    }

    if ($DryRun) {
        Write-Host "[dry-run] Would install wasm-pack via 'cargo install wasm-pack --locked'." -ForegroundColor Yellow
        return
    }

    Write-Host "Installing wasm-pack..." -ForegroundColor Cyan
    & cargo install wasm-pack --locked
    if ($LASTEXITCODE -ne 0) {
        throw "cargo install wasm-pack failed."
    }
}

function Install-TauriCli {
    if (Get-Command cargo-tauri -ErrorAction SilentlyContinue) {
        Write-Host "cargo tauri already installed." -ForegroundColor DarkGray
        return
    }

    if ($DryRun) {
        Write-Host "[dry-run] Would install Tauri CLI via 'cargo install tauri-cli --locked --version ^2.0.0'." -ForegroundColor Yellow
        return
    }

    Write-Host "Installing Tauri CLI..." -ForegroundColor Cyan
    & cargo install tauri-cli --locked --version '^2.0.0'
    if ($LASTEXITCODE -ne 0) {
        throw "cargo install tauri-cli failed."
    }
}

function Ensure-WasmTarget {
    $installedTargets = & rustup target list --installed
    if ($installedTargets -match 'wasm32-unknown-unknown') {
        Write-Host "Rust target wasm32-unknown-unknown already installed." -ForegroundColor DarkGray
        return
    }

    if ($DryRun) {
        Write-Host "[dry-run] Would install Rust target wasm32-unknown-unknown via 'rustup target add wasm32-unknown-unknown'." -ForegroundColor Yellow
        return
    }

    Write-Host "Installing Rust target wasm32-unknown-unknown..." -ForegroundColor Cyan
    & rustup target add wasm32-unknown-unknown
    if ($LASTEXITCODE -ne 0) {
        throw "rustup target add wasm32-unknown-unknown failed."
    }
}

function Install-EditorDependencies {
    if ($DryRun) {
        Write-Host "[dry-run] Would install editor dependencies via 'npm ci' in editor/." -ForegroundColor Yellow
        return
    }

    Push-Location (Join-Path $PSScriptRoot 'editor')
    try {
        Write-Host "Installing editor npm dependencies..." -ForegroundColor Cyan
        & npm ci
        if ($LASTEXITCODE -ne 0) {
            throw "npm ci failed in editor/."
        }
    }
    finally {
        Pop-Location
    }
}

function Warn-IfMissingCxxBuildTools {
    $vswhereCandidates = @(
        "$env:ProgramFiles(x86)\Microsoft Visual Studio\Installer\vswhere.exe",
        "$env:ProgramFiles\Microsoft Visual Studio\Installer\vswhere.exe"
    )

    $vswhere = $null
    foreach ($candidate in $vswhereCandidates) {
        if (Test-Path $candidate) {
            $vswhere = $candidate
            break
        }
    }

    if (-not $vswhere) {
        Write-Host "Warning: Visual Studio Build Tools were not detected. Native Rust/Tauri builds may still fail until the C++ workload is installed." -ForegroundColor Yellow
        return
    }

    $installationPath = & $vswhere -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $installationPath) {
        Write-Host "Warning: Visual Studio C++ build tools were not detected. Install the 'Desktop development with C++' workload for native builds." -ForegroundColor Yellow
    }
}

Assert-Command rustup "Install Rust from https://rustup.rs/."
Assert-Command cargo "Install Rust from https://rustup.rs/."
Assert-Command node "Install Node.js 20+ from https://nodejs.org/."
Assert-Command npm "Install Node.js 20+ from https://nodejs.org/."

$nodeVersion = & node --version
if ($nodeVersion -notmatch '^v(\d+)') {
    throw "Could not determine Node.js version from '$nodeVersion'."
}

if ([int]$Matches[1] -lt 20) {
    throw "Node.js 20+ is required; found $nodeVersion."
}

Write-Host "Node.js version: $nodeVersion" -ForegroundColor DarkGray

Install-WasmPack
Ensure-WasmTarget
Install-TauriCli
Install-EditorDependencies
Warn-IfMissingCxxBuildTools

Write-Host ""
Write-Host "Setup complete." -ForegroundColor Green
Write-Host "You can now run .\run-editor.ps1 or .\build-editor.ps1." -ForegroundColor Green
